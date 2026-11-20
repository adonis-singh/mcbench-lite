//! Benches: one prompt and its follow-ups (rounds) sent to several models,
//! each model's run an ordinary chat tagged with the bench; votes and the
//! leaderboard (`server/store.js`).

use std::collections::HashMap;
use std::sync::MutexGuard;

use rusqlite::{OptionalExtension, params};

use crate::chats::NewConversation;
use crate::json::Truthy;
use crate::json::{self, Json};
use crate::payload::{self, Content, PromptLink};
use crate::{BenchId, Conversation, Error, LiveConversation, Store, iso};

/// A model of a new bench.
#[derive(Clone, Debug)]
pub struct BenchMember {
    /// `provider`.
    pub provider: String,
    /// `model`.
    pub model: String,
    /// `effort`.
    pub effort: Option<String>,
}

/// What `createBench(id, ...)` takes.
#[derive(Clone, Debug)]
pub struct NewBench {
    /// `title`.
    pub title: String,
    /// `blind`: the models' names are hidden until revealed.
    pub blind: bool,
    /// `resolution` (1 when `None`).
    pub resolution: Option<u32>,
    /// The first round's content, whose files are already stored under the
    /// bench's id.
    pub content: Content,
    /// The library prompts the first round was sent from.
    pub prompts: Vec<PromptLink>,
    /// The models, in slot order.
    pub members: Vec<BenchMember>,
}

/// A vote on a round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vote {
    /// The model picked as best, or `None` for a tie.
    pub pick: Option<String>,
    /// Whether the names were hidden when it was cast.
    pub blind: bool,
}

/// A bench as `getBench` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Bench {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `createdAt`.
    pub created_at: String,
    /// `head`: the active line's last round.
    pub head: Option<String>,
    /// `nodes`: every round ever sent, as stored (`{ id, parentId,
    /// content, prompts?, createdAt }`).
    pub nodes: Vec<Json>,
    /// `rounds`: the active line, each node with its `branch` (`{ index,
    /// count, previousId, nextId }`).
    pub rounds: Vec<Json>,
    /// `blind`.
    pub blind: bool,
    /// `revealed`.
    pub revealed: bool,
    /// `order`: the models in the order their letters run.
    pub order: Json,
    /// `votes`, by round id, in the order JavaScript's object keeps them.
    pub votes: Vec<(String, Vote)>,
}

impl Bench {
    /// The content of round `index` of the active line.
    pub fn round_content(&self, index: usize) -> Result<Content, Error> {
        let round = self
            .rounds
            .get(index)
            .ok_or_else(|| Error::plain(format!("Bench {} has no round {index}", self.id)))?;
        let content = round.get("content").ok_or("a round has no content")?;
        serde_json::from_str(&json::stringify(content))
            .map_err(|error| Error::plain(error.to_string()))
    }

    /// The id of round `index` of the active line.
    pub fn round_id(&self, index: usize) -> Option<&str> {
        self.rounds.get(index)?.get("id")?.as_str()
    }

    /// The library prompts round `index` of the active line was sent from.
    pub fn round_prompts(&self, index: usize) -> Result<Option<Vec<PromptLink>>, Error> {
        match self
            .rounds
            .get(index)
            .and_then(|round| round.get("prompts"))
        {
            None => Ok(None),
            Some(links) => serde_json::from_str(&json::stringify(links))
                .map(Some)
                .map_err(|error| Error::plain(error.to_string())),
        }
    }

    fn set_vote(&mut self, round: &str, vote: Vote) {
        set_ordered(&mut self.votes, round, vote);
    }
}

/// Sets `key` in `pairs` as a JavaScript object does.
fn set_ordered<V>(pairs: &mut Vec<(String, V)>, key: &str, value: V) {
    let mut object = json::Object::new();
    for (k, _) in pairs.iter() {
        object.insert((k).to_string(), Json::Null);
    }
    if let Some(slot) = pairs.iter_mut().find(|(k, _)| k == key) {
        slot.1 = value;
        return;
    }
    object.insert((key).to_string(), Json::Null);
    let at = object.keys().position(|k| k == key).expect("just set");
    pairs.insert(at, (key.to_string(), value));
}

/// How far a bench chat's stored branch has got (`benchProgress`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchProgress {
    /// The rounds it was sent.
    pub round: i64,
    /// `stopped` or `failed` when its last turn did not finish (a turn
    /// still under way reads as failed), else `None`.
    pub halted: Option<&'static str>,
}

/// A row of the leaderboard.
#[derive(Clone, Debug, PartialEq)]
pub struct LeaderboardEntry {
    /// `provider`.
    pub provider: String,
    /// `model`.
    pub model: String,
    /// `effort`.
    pub effort: Option<String>,
    /// Rounds it was picked as best.
    pub wins: i64,
    /// Votes it was judged in.
    pub votes: i64,
    /// Its rating, rounded (`Math.round`).
    pub rating: f64,
    /// Benches it was judged in.
    pub benches: usize,
}

/// `Math.round`: halves go up.
fn js_round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `roundChain(nodes, headId)`: the rounds from the first to `head`, each
/// with where it stands among the rounds sent in its place.
pub(crate) fn round_chain(nodes: &[Json], head: Option<&Json>) -> Vec<Json> {
    // `new Map(nodes.map((node) => [node.id, node]))`: the last of an id.
    let by_id = |id: Option<&Json>| -> Option<usize> {
        let id = id?;
        nodes
            .iter()
            .rposition(|node| node.get("id").unwrap_or(&Json::Null) == id)
    };
    let mut rounds = Vec::new();
    let mut at = by_id(head);
    while let Some(index) = at {
        let node = &nodes[index];
        let parent = node.get("parentId");
        let siblings: Vec<usize> = (0..nodes.len())
            .filter(|&i| nodes[i].get("parentId") == parent)
            .collect();
        let position = siblings
            .iter()
            .position(|&i| i == index)
            .expect("a node is its own sibling");
        let sibling_id = |i: Option<&usize>| -> Json {
            match i.and_then(|&i| nodes[i].get("id")) {
                Some(id) if id.truthy() => id.clone(),
                _ => Json::Null,
            }
        };
        let mut round = node.clone();
        if let Some(object) = round.as_object_mut() {
            object.insert("branch".into(), crate::object! {
                    "index" => position,
                    "count" => siblings.len(),
                    "previousId" => sibling_id(position.checked_sub(1).and_then(|p| siblings.get(p))),
                    "nextId" => sibling_id(siblings.get(position + 1)),
                });
        }
        rounds.push(round);
        at = by_id(parent);
    }
    rounds.reverse();
    rounds
}

/// A bench as the sidebar lists it (`listBenches()`'s rows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedBench {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `createdAt`.
    pub created_at: String,
    /// `head`: the round its branch ends at.
    pub head: Option<String>,
    /// `resolution`: its models' chats' (`null` when it has none left).
    pub resolution: Option<i64>,
    /// `rounds`: how many rounds its branch holds.
    pub rounds: usize,
    /// `prompts`: the library prompts any of its rounds was sent from, on
    /// any branch, first seen first.
    pub prompts: Vec<String>,
    /// `blind`.
    pub blind: bool,
    /// `revealed`.
    pub revealed: bool,
    /// `order`: the models' chats in the order their letters run.
    pub order: Vec<String>,
}

impl ListedBench {
    /// The row as `listBenches()` gives it, in its key order.
    pub fn to_json(&self) -> Json {
        crate::object! {
            "id" => self.id.as_str(),
            "title" => self.title.as_str(),
            "createdAt" => self.created_at.as_str(),
            "head" => self.head.clone(),
            "resolution" => self.resolution,
            "rounds" => self.rounds,
            "prompts" => self.prompts.clone(),
            "blind" => self.blind,
            "revealed" => self.revealed,
            "order" => self.order.clone(),
        }
    }
}

impl Store {
    /// `roundNode(parentId, content, prompts)`: a round, with the library
    /// prompts it was sent from when there are any.
    fn round_node(&self, parent: Json, content: &Content, prompts: &[PromptLink]) -> Json {
        let mut node = json::Object::new();
        node.insert("id".into(), Json::String(self.host.uuid()));
        node.insert("parentId".into(), parent);
        node.insert("content".into(), payload::to_json(content));
        if !prompts.is_empty() {
            node.insert("prompts".into(), payload::to_json(&prompts));
        }
        node.insert("createdAt".into(), Json::String(iso(self.now())));
        Json::Object(node)
    }

    /// `createBench(id, ...)`: a bench, written whole with its first round
    /// and its models' chats, or not at all.
    pub fn create_bench(
        &self,
        id: &str,
        bench: NewBench,
    ) -> Result<(Bench, Vec<LiveConversation>), Error> {
        let mut chats = Vec::new();
        for (slot, member) in bench.members.iter().enumerate() {
            chats.push(self.new_conversation(
                &member.provider,
                &member.model,
                NewConversation {
                    title: bench.title.clone(),
                    effort: member.effort.clone(),
                    resolution: bench.resolution.unwrap_or(1),
                    bench: Some((BenchId(id.to_string()), slot as u32)),
                },
            )?);
        }
        let mut order: Vec<String> = chats.iter().map(|chat| chat.id.0.clone()).collect();
        if bench.blind {
            for i in (1..order.len()).rev() {
                let j = self.host.random_int(i as u32 + 1) as usize;
                order.swap(i, j);
            }
        }
        let round = self.round_node(Json::Null, &bench.content, &bench.prompts);
        let round_id = round
            .get("id")
            .and_then(Json::as_str)
            .expect("an id")
            .to_string();
        let created_at = round
            .get("createdAt")
            .and_then(Json::as_str)
            .expect("a time")
            .to_string();
        let mut live = Vec::new();
        let result = self.transaction(|| {
            self.with(|inner| {
                inner
                    .db
                    .prepare_cached(
                        "
        INSERT INTO benches (id, title, created_at, rounds_json, head_round_id, blind, order_json) VALUES (?, ?, ?, ?, ?, ?, ?)
      ",
                    )?
                    .execute(params![
                        id,
                        bench.title,
                        created_at,
                        json::stringify(&Json::Array(vec![round.clone()])),
                        round_id,
                        bench.blind as i64,
                        json::stringify(&Json::from(order.clone())),
                    ])?;
                Ok::<_, Error>(())
            })?;
            self.index_doc(id, &format!("title:{id}"), &bench.title)?;
            self.index_doc(id, &format!("round:{id}:{round_id}"), &bench.content_text())?;
            for chat in chats.drain(..) {
                let chat = self.make_live(chat);
                live.push(chat.clone());
                self.save(&mut chat.lock().expect("a new chat's lock"))?;
            }
            Ok(())
        });
        if let Err(error) = result {
            for chat in &live {
                let id = chat.lock().expect("a chat's lock").id.0.clone();
                self.with(|inner| inner.live.remove(&id));
            }
            return Err(error);
        }
        let stored = self
            .bench(id)?
            .ok_or_else(|| Error::plain(format!("Bench {id} not found")))?;
        Ok((stored, live))
    }

    /// `getBench(id)`: the bench, or `None` when it is not stored or is
    /// archived.
    pub fn bench(&self, id: &str) -> Result<Option<Bench>, Error> {
        self.with(|inner| {
            let row = inner
                .db
                .prepare_cached("SELECT * FROM benches WHERE id = ? AND archived_at IS NULL")?
                .query_row([id], |row| {
                    Ok((
                        row.get::<_, String>("id")?,
                        row.get::<_, String>("title")?,
                        row.get::<_, String>("created_at")?,
                        row.get::<_, String>("rounds_json")?,
                        row.get::<_, Option<String>>("head_round_id")?,
                        row.get::<_, i64>("blind")?,
                        row.get::<_, Option<i64>>("revealed_at")?,
                        row.get::<_, String>("order_json")?,
                    ))
                })
                .optional()?;
            let Some((
                row_id,
                title,
                created_at,
                rounds_json,
                head,
                blind,
                revealed_at,
                order_json,
            )) = row
            else {
                return Ok(None);
            };
            let nodes = json::parse(&rounds_json)
                .map_err(|error| Error::plain(format!("Bench {id} rounds: {error}")))?;
            let nodes = nodes
                .as_array()
                .ok_or("rounds_json is not an array")?
                .to_vec();
            let mut votes = Vec::new();
            let mut statement = inner.db.prepare_cached(
                "SELECT round_id, pick, blind FROM bench_votes WHERE bench_id = ?",
            )?;
            for vote in statement.query_map([id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })? {
                let (round, pick, blind) = vote?;
                set_ordered(
                    &mut votes,
                    &round,
                    Vote {
                        pick,
                        blind: blind != 0,
                    },
                );
            }
            let head_json = head.clone().map(Json::String);
            Ok(Some(Bench {
                id: row_id,
                title,
                created_at,
                rounds: round_chain(&nodes, head_json.as_ref()),
                head,
                nodes,
                blind: blind != 0,
                revealed: revealed_at.is_some(),
                order: json::parse(&order_json)?,
                votes,
            }))
        })
    }

    fn reload(&self, bench: &mut Bench) -> Result<(), Error> {
        if let Some(stored) = self.bench(&bench.id)? {
            *bench = stored;
        }
        Ok(())
    }

    /// `revealBench(bench)`.
    pub fn reveal_bench(&self, bench: &mut Bench) -> Result<(), Error> {
        let now = self.now();
        self.with(|inner| {
            inner
                .db
                .prepare_cached(
                    "UPDATE benches SET revealed_at = ? WHERE id = ? AND revealed_at IS NULL",
                )?
                .execute(params![now, bench.id])?;
            Ok::<_, Error>(())
        })?;
        bench.revealed = true;
        Ok(())
    }

    /// `castVote(bench, roundId, pick, judged)`: `pick` is a member id, or
    /// `None` for a tie, among the `judged` member ids.
    pub fn cast_vote(
        &self,
        bench: &mut Bench,
        round: &str,
        pick: Option<&str>,
        judged: &[String],
    ) -> Result<(), Error> {
        if !bench.blind || bench.revealed {
            return Err(Error::plain(format!(
                "Bench {} shows its models' names: it takes no votes",
                bench.id
            )));
        }
        let now = self.now();
        self.with(|inner| {
            inner
                .db
                .prepare_cached(
                    "
    INSERT INTO bench_votes (round_id, bench_id, pick, members_json, blind, created_at) VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT(round_id) DO UPDATE SET pick = excluded.pick, members_json = excluded.members_json, blind = excluded.blind, created_at = excluded.created_at
  ",
                )?
                .execute(params![round, bench.id, pick, json::stringify(&Json::from(judged.to_vec())), 1, now])?;
            Ok::<_, Error>(())
        })?;
        bench.set_vote(
            round,
            Vote {
                pick: pick.map(str::to_string),
                blind: true,
            },
        );
        Ok(())
    }

    /// `voteCount()`.
    pub fn vote_count(&self) -> Result<i64, Error> {
        self.with(|inner| {
            Ok(inner
                .db
                .query_row("SELECT COUNT(*) AS count FROM bench_votes", [], |row| {
                    row.get(0)
                })?)
        })
    }

    /// `clearAllVotes()`.
    pub fn clear_all_votes(&self) -> Result<(), Error> {
        self.with(|inner| {
            inner.db.execute("DELETE FROM bench_votes", [])?;
            Ok(())
        })
    }

    /// `clearVote(bench, roundId)`.
    pub fn clear_vote(&self, bench: &mut Bench, round: &str) -> Result<(), Error> {
        self.with(|inner| {
            inner
                .db
                .execute("DELETE FROM bench_votes WHERE round_id = ?", [round])?;
            Ok::<_, Error>(())
        })?;
        bench.votes.retain(|(id, _)| id != round);
        Ok(())
    }

    /// `leaderboard()`: ratings from every blind vote in benches still kept,
    /// oldest first. A pick beats each other model judged with it; a tie
    /// draws them all. A model at two efforts is two entries.
    pub fn leaderboard(&self) -> Result<Vec<LeaderboardEntry>, Error> {
        self.with(|inner| {
            let mut members = HashMap::new();
            let mut statement = inner.db.prepare_cached(
                "SELECT id, provider, model, effort FROM conversations WHERE bench_id IS NOT NULL",
            )?;
            for row in statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })? {
                let (id, provider, model, effort) = row?;
                members.insert(id, (provider, model, effort));
            }
            struct Entry {
                provider: String,
                model: String,
                effort: Option<String>,
                rating: f64,
                wins: i64,
                votes: i64,
                benches: Vec<String>,
            }
            let mut entries: Vec<(String, Entry)> = Vec::new();
            let entry = |id: &str, entries: &mut Vec<(String, Entry)>| -> Result<usize, Error> {
                let (provider, model, effort) = members.get(id).ok_or_else(|| {
                    Error::plain(format!(
                        "Cannot destructure a vote's member {id}: it is not a bench model"
                    ))
                })?;
                let effort = effort.clone().filter(|effort| !effort.is_empty());
                let key = format!("{model}\u{0}{}", effort.as_deref().unwrap_or(""));
                if let Some(at) = entries.iter().position(|(k, _)| *k == key) {
                    return Ok(at);
                }
                entries.push((
                    key,
                    Entry {
                        provider: provider.clone(),
                        model: model.clone(),
                        effort,
                        rating: 1000.0,
                        wins: 0,
                        votes: 0,
                        benches: Vec::new(),
                    },
                ));
                Ok(entries.len() - 1)
            };
            let mut statement = inner.db.prepare_cached(
                "
    SELECT v.bench_id, v.pick, v.members_json FROM bench_votes v JOIN benches b ON b.id = v.bench_id
    WHERE v.blind = 1 AND b.archived_at IS NULL ORDER BY v.created_at
  ",
            )?;
            let votes = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for (bench, pick, members_json) in votes {
                let judged_ids = json::parse(&members_json)?;
                let mut judged = Vec::new();
                for id in judged_ids
                    .as_array()
                    .ok_or("members_json is not an array")?
                {
                    let id = id
                        .as_str()
                        .ok_or("a judged member is not an id")?
                        .to_string();
                    let at = entry(&id, &mut entries)?;
                    judged.push((id, at));
                }
                let before: HashMap<usize, f64> = judged
                    .iter()
                    .map(|(_, at)| (*at, entries[*at].1.rating))
                    .collect();
                let play = |a: usize, b: usize, score: f64, entries: &mut Vec<(String, Entry)>| {
                    let expected = 1.0 / (1.0 + 10f64.powf((before[&b] - before[&a]) / 400.0));
                    entries[a].1.rating += 24.0 * (score - expected);
                    entries[b].1.rating -= 24.0 * (score - expected);
                };
                for (id, at) in &judged {
                    let e = &mut entries[*at].1;
                    e.votes += 1;
                    if !e.benches.contains(&bench) {
                        e.benches.push(bench.clone());
                    }
                    if Some(id) == pick.as_ref() {
                        e.wins += 1;
                    }
                }
                for i in 0..judged.len() {
                    for j in i + 1..judged.len() {
                        let (a, b) = (&judged[i], &judged[j]);
                        match &pick {
                            None => play(a.1, b.1, 0.5, &mut entries),
                            Some(pick) if a.0 == *pick => play(a.1, b.1, 1.0, &mut entries),
                            Some(pick) if b.0 == *pick => play(b.1, a.1, 1.0, &mut entries),
                            Some(_) => {}
                        }
                    }
                }
            }
            let mut board: Vec<LeaderboardEntry> = entries
                .into_iter()
                .map(|(_, e)| LeaderboardEntry {
                    provider: e.provider,
                    model: e.model,
                    effort: e.effort,
                    wins: e.wins,
                    votes: e.votes,
                    rating: js_round(e.rating),
                    benches: e.benches.len(),
                })
                .collect();
            board.sort_by(|a, b| {
                b.rating
                    .partial_cmp(&a.rating)
                    .expect("ratings are numbers")
            });
            Ok(board)
        })
    }

    /// `appendRound(bench, content, prompts)`: a new round at the head.
    pub fn append_round(
        &self,
        bench: &mut Bench,
        content: &Content,
        prompts: &[PromptLink],
    ) -> Result<(), Error> {
        let node = self.round_node(
            bench.head.clone().map_or(Json::Null, Json::String),
            content,
            prompts,
        );
        let node_id = node
            .get("id")
            .and_then(Json::as_str)
            .expect("an id")
            .to_string();
        let mut nodes = bench.nodes.clone();
        nodes.push(node);
        self.transaction(|| {
            self.with(|inner| {
                inner
                    .db
                    .prepare_cached(
                        "UPDATE benches SET rounds_json = ?, head_round_id = ? WHERE id = ?",
                    )?
                    .execute(params![
                        json::stringify(&Json::Array(nodes)),
                        node_id,
                        bench.id
                    ])?;
                Ok::<_, Error>(())
            })?;
            self.index_doc(
                &bench.id,
                &format!("round:{}:{node_id}", bench.id),
                &content_text(content),
            )
        })?;
        self.reload(bench)
    }

    /// `setBenchHead(bench, headId)`.
    fn set_bench_head(&self, bench: &mut Bench, head: &Json) -> Result<(), Error> {
        let head = match head {
            Json::Null => None,
            Json::String(text) => Some(text.clone()),
            other => {
                return Err(Error::plain(format!(
                    "a round's id is not text: {}",
                    json::stringify(other)
                )));
            }
        };
        self.with(|inner| {
            inner
                .db
                .prepare_cached("UPDATE benches SET head_round_id = ? WHERE id = ?")?
                .execute(params![head, bench.id])?;
            Ok::<_, Error>(())
        })?;
        self.reload(bench)
    }

    /// `benchChange(bench, members, change)`: a change to a bench's line and
    /// its models' chats, stored whole or not at all. One that fails drops
    /// the chats from `live` and reads `bench` again.
    fn bench_change(
        &self,
        bench: &mut Bench,
        members: &[String],
        change: impl FnOnce(&mut Bench) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let result = self.transaction(|| change(bench));
        if let Err(error) = result {
            for member in members {
                self.with(|inner| inner.live.remove(member));
            }
            self.reload(bench)?;
            return Err(error);
        }
        Ok(())
    }

    /// `editBenchRound(bench, index, content, links)`: the line goes back to
    /// before round `index`, every model sent it returns to its world then,
    /// and the new version follows on the line. Returns the models that went
    /// back. Hold none of the bench's chats when calling.
    pub fn edit_bench_round(
        &self,
        bench: &mut Bench,
        index: usize,
        content: &Content,
        links: &[PromptLink],
    ) -> Result<Vec<LiveConversation>, Error> {
        let members = self.bench_members(&bench.id)?;
        let mut reverted = Vec::new();
        for member in &members {
            if member
                .lock()
                .expect("a chat's lock")
                .prompts()
                .get(index)
                .is_some()
            {
                reverted.push(member.clone());
            }
        }
        let mut guards: Vec<MutexGuard<'_, Conversation>> = reverted
            .iter()
            .map(|m| m.lock().expect("a chat's lock"))
            .collect();
        let ids: Vec<String> = guards.iter().map(|guard| guard.id.0.clone()).collect();
        self.bench_change(bench, &ids, |bench| {
            for guard in guards.iter_mut() {
                let prompt = guard.prompts()[index].id.0.clone();
                self.revert(guard, &prompt)?;
            }
            let parent = bench
                .rounds
                .get(index)
                .ok_or("Cannot read properties of undefined (reading 'parentId')")?
                .get("parentId")
                .cloned()
                .unwrap_or(Json::Null);
            self.set_bench_head(bench, &parent)?;
            self.append_round(bench, content, links)
        })?;
        drop(guards);
        Ok(reverted)
    }

    /// `followBenchBranch(bench, roundId)`: the latest line through round
    /// `round` becomes the active one, and each model's chat goes on the
    /// branch that answered it. Hold none of the bench's chats when calling.
    pub fn follow_bench_branch(&self, bench: &mut Bench, round: &str) -> Result<(), Error> {
        let members = self.bench_members(&bench.id)?;
        let mut guards: Vec<MutexGuard<'_, Conversation>> = members
            .iter()
            .map(|m| m.lock().expect("a chat's lock"))
            .collect();
        let ids: Vec<String> = guards.iter().map(|guard| guard.id.0.clone()).collect();
        self.bench_change(bench, &ids, |bench| {
            let mut head = Json::String(round.to_string());
            while let Some(child) = bench
                .nodes
                .iter()
                .rfind(|node| node.get("parentId") == Some(&head))
            {
                head = child.get("id").cloned().unwrap_or(Json::Null);
            }
            self.set_bench_head(bench, &head)?;
            let line: Vec<String> = bench
                .rounds
                .iter()
                .map(|round| {
                    round
                        .get("id")
                        .and_then(Json::as_str)
                        .unwrap_or("")
                        .to_string()
                })
                .collect();
            for guard in guards.iter_mut() {
                self.follow_rounds(guard, &line)?;
            }
            Ok(())
        })
    }

    /// `retryBenchRound(bench, member)`: a model's last round is run again;
    /// the vote on that round is taken back.
    pub fn retry_bench_round(
        &self,
        bench: &mut Bench,
        member: &mut Conversation,
    ) -> Result<(), Error> {
        let sent: Vec<String> = member
            .prompts()
            .iter()
            .map(|message| message.id.0.clone())
            .collect();
        let ids = [member.id.0.clone()];
        self.bench_change(bench, &ids, |bench| {
            let round = sent
                .len()
                .checked_sub(1)
                .and_then(|at| bench.round_id(at))
                .ok_or("Cannot read properties of undefined (reading 'id')")?
                .to_string();
            self.clear_vote(bench, &round)?;
            let last = sent
                .last()
                .ok_or("Cannot read properties of undefined (reading 'id')")?;
            self.revert(member, last)?;
            Ok(())
        })
    }

    /// `setBenchTitle(bench, title)`: the bench's and its models' chats'.
    pub fn set_bench_title(&self, bench: &mut Bench, title: &str) -> Result<(), Error> {
        self.transaction(|| {
            self.with(|inner| {
                inner
                    .db
                    .prepare_cached("UPDATE benches SET title = ? WHERE id = ?")?
                    .execute(params![title, bench.id])?;
                inner
                    .db
                    .prepare_cached("UPDATE conversations SET title = ? WHERE bench_id = ?")?
                    .execute(params![title, bench.id])?;
                Ok::<_, Error>(())
            })?;
            self.index_doc(&bench.id, &format!("title:{}", bench.id), title)
        })?;
        bench.title = title.to_string();
        for member in self.bench_members(&bench.id)? {
            member.lock().expect("a chat's lock").title = title.to_string();
        }
        Ok(())
    }

    /// `listBenches()`: every bench not archived, newest first.
    pub fn list_benches(&self) -> Result<Vec<ListedBench>, Error> {
        self.with(|inner| {
            let mut statement = inner.db.prepare_cached(
                "
    SELECT id, title, created_at AS createdAt, rounds_json, head_round_id AS head, blind, revealed_at, order_json,
      -- A bench's resolution is its models' chats': all are made with the bench's.
      (SELECT max(resolution) FROM conversations WHERE conversations.bench_id = benches.id) AS resolution
    FROM benches WHERE archived_at IS NULL ORDER BY created_at DESC
  ",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>("id")?,
                        row.get::<_, String>("title")?,
                        row.get::<_, String>("createdAt")?,
                        row.get::<_, String>("rounds_json")?,
                        row.get::<_, Option<String>>("head")?,
                        row.get::<_, i64>("blind")?,
                        row.get::<_, Option<i64>>("revealed_at")?,
                        row.get::<_, String>("order_json")?,
                        row.get::<_, Option<i64>>("resolution")?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut benches = Vec::new();
            for (id, title, created_at, rounds_json, head, blind, revealed_at, order_json, resolution) in rows {
                let nodes = json::parse(&rounds_json)?;
                let nodes = nodes.as_array().ok_or("rounds_json is not an array")?;
                let head_json = head.clone().map(Json::String);
                let mut prompts: Vec<String> = Vec::new();
                for node in nodes {
                    if let Some(links) = node.get("prompts").filter(|links| links.truthy()) {
                        for link in links.as_array().ok_or("a round's prompts are not an array")? {
                            let id = link.get("id").and_then(Json::as_str).ok_or("a prompt link has no id")?.to_string();
                            if !prompts.contains(&id) {
                                prompts.push(id);
                            }
                        }
                    }
                }
                let order = json::parse(&order_json)?;
                let order = order
                    .as_array()
                    .ok_or("a bench's order_json is not an array")?
                    .iter()
                    .map(|member| member.as_str().map(str::to_string))
                    .collect::<Option<Vec<String>>>()
                    .ok_or("a bench's order_json holds a member that is not a chat id")?;
                let rounds = round_chain(nodes, head_json.as_ref()).len();
                benches.push(ListedBench {
                    id,
                    title,
                    created_at,
                    head,
                    resolution,
                    rounds,
                    prompts,
                    blind: blind != 0,
                    revealed: revealed_at.is_some(),
                    order,
                });
            }
            Ok(benches)
        })
    }

    /// `benchMembers(benchId)`: the models' chats not archived, by slot.
    pub fn bench_members(&self, bench: &str) -> Result<Vec<LiveConversation>, Error> {
        let ids: Vec<String> = self.with(|inner| {
            let mut statement = inner.db.prepare_cached(
                "
    SELECT id FROM conversations WHERE bench_id = ? AND archived_at IS NULL ORDER BY bench_slot
  ",
            )?;
            let rows = statement
                .query_map([bench], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, Error>(rows)
        })?;
        let mut members = Vec::new();
        for id in ids {
            members.push(
                self.conversation(&id)?
                    .ok_or_else(|| Error::plain(format!("Conversation {id} not found")))?,
            );
        }
        Ok(members)
    }

    /// `archiveBench(id)`: the bench and its models' chats; returns the
    /// chats archived.
    pub fn archive_bench(&self, id: &str) -> Result<Vec<String>, Error> {
        let members: Vec<String> = self.with(|inner| {
            let mut statement = inner.db.prepare_cached(
                "SELECT id FROM conversations WHERE bench_id = ? AND archived_at IS NULL",
            )?;
            let rows = statement
                .query_map([id], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, Error>(rows)
        })?;
        let now = self.now();
        self.transaction(|| {
            let changes = self.with(|inner| {
                inner
                    .db
                    .prepare_cached(
                        "UPDATE benches SET archived_at = ? WHERE id = ? AND archived_at IS NULL",
                    )?
                    .execute(params![now, id])
            })?;
            if changes != 1 {
                return Err(Error::plain(format!("Bench {id} not found")));
            }
            for member in &members {
                self.archive_conversation(member)?;
            }
            Ok(())
        })?;
        Ok(members)
    }

    /// `benchProgress(id)`: how far a bench chat's stored branch has got.
    pub fn bench_progress(&self, id: &str) -> Result<BenchProgress, Error> {
        let (round, head): (i64, Option<String>) = self.with(|inner| {
            inner.db.prepare_cached(
                "
    WITH RECURSIVE chain(id, parent_id, payload_json, depth) AS (
      SELECT id, parent_id, payload_json, 0 FROM messages
        WHERE id = (SELECT current_message_id FROM conversations WHERE id = ?)
      UNION ALL
      SELECT m.id, m.parent_id, m.payload_json, chain.depth + 1 FROM messages m JOIN chain ON m.id = chain.parent_id
    )
    SELECT
      COALESCE(SUM(json_extract(payload_json, '$.role') = 'user' AND COALESCE(json_extract(payload_json, '$.seeBuild'), 0) = 0), 0) AS round,
      MAX(CASE WHEN depth = 0 THEN payload_json END) AS head
    FROM chain
  ",
            )?
            .query_row([id], |row| Ok((row.get(0)?, row.get(1)?)))
        })?;
        let end = match head {
            Some(head) => payload::decode(&head, &format!("Conversation {id} head"))?.turn_end(),
            None => Some("final"),
        };
        Ok(BenchProgress {
            round,
            halted: match end {
                Some("final") => None,
                Some("stopped") => Some("stopped"),
                _ => Some("failed"),
            },
        })
    }
}

/// `messageText({ role: 'user', content })`.
fn content_text(content: &Content) -> String {
    content.text()
}

impl NewBench {
    fn content_text(&self) -> String {
        content_text(&self.content)
    }
}
