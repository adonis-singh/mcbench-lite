//! A bench's details as `GET /api/benches/:id` gives them (`server/index.js`:
//! `getBench` without its `nodes`, and `stats`, each model's `roundStats`),
//! typed, with their JSON in today's key order. The interface takes
//! [`BenchDetails`]; the core makes it with [`BenchDetails::of`].

use mcbench_store::json::{Json, Object};
use mcbench_store::{Bench, Content, PromptLink, Vote};

/// A bench's details (`GET /api/benches/<id>`, its `nodes` left out).
#[derive(Clone, Debug, PartialEq)]
pub struct BenchDetails {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `createdAt`.
    pub created_at: String,
    /// `head`: the active line's last round.
    pub head: Option<String>,
    /// `rounds`: the active line, first first.
    pub rounds: Vec<BenchRound>,
    /// `blind`.
    pub blind: bool,
    /// `revealed`.
    pub revealed: bool,
    /// `order`: the members' chats in the order their letters run.
    pub order: Vec<String>,
    /// `votes`: by round id, in the order JavaScript's object keeps them.
    pub votes: Vec<(String, Vote)>,
    /// `stats`: by member id (the bench's members in slot order), one per
    /// round the member was sent.
    pub stats: Vec<(String, Vec<RoundStats>)>,
}

/// One round of a bench's line (`rounds[]`).
#[derive(Clone, Debug, PartialEq)]
pub struct BenchRound {
    /// `id`.
    pub id: String,
    /// `parentId`.
    pub parent_id: Option<String>,
    /// `content`: its text parts and files (`/api/attachments/...`
    /// addresses), the stored round's `content` as it is.
    pub content: Content,
    /// `prompts`: the library prompts it was sent from (absent from the
    /// stored round when there are none: today's `roundNode` writes none).
    pub prompts: Vec<PromptLink>,
    /// `createdAt`.
    pub created_at: String,
    /// `branch`: where it stands among the rounds sent in its place.
    pub branch: RoundBranch,
}

/// A round's place among its versions (`branch`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundBranch {
    /// `index`, from 0.
    pub index: usize,
    /// `count`.
    pub count: usize,
    /// `previousId`.
    pub previous_id: Option<String>,
    /// `nextId`.
    pub next_id: Option<String>,
}

/// A member's time and tokens on a round (`stats[member][round]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RoundStats {
    /// `ms`: from its prompt to its latest message.
    pub ms: Option<u64>,
    /// `inputTokens`.
    pub input_tokens: Option<u64>,
    /// `outputTokens`.
    pub output_tokens: Option<u64>,
}

fn text(value: Option<&Json>, what: &str) -> Result<String, String> {
    value
        .and_then(Json::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("a bench round's {what} is not text"))
}

fn text_or_null(value: Option<&Json>, what: &str) -> Result<Option<String>, String> {
    match value {
        Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s.clone())),
        _ => Err(format!("a bench round's {what} is not text or null")),
    }
}

fn count(value: Option<&Json>, what: &str) -> Result<usize, String> {
    value
        .and_then(Json::as_u64)
        .map(|n| n as usize)
        .ok_or_else(|| format!("a bench round's {what} is not a count"))
}

impl BenchRound {
    /// A round of `getBench`'s `rounds`: a stored node (`{ id, parentId,
    /// content, prompts?, createdAt }`) with its `branch`.
    pub fn of(round: &Json) -> Result<BenchRound, String> {
        let object = round.as_object().ok_or("a bench round is not an object")?;
        for key in object.keys() {
            if ![
                "id",
                "parentId",
                "content",
                "prompts",
                "createdAt",
                "branch",
            ]
            .contains(&key.as_str())
            {
                return Err(format!("a bench round has a field {key}"));
            }
        }
        let content = object
            .get("content")
            .ok_or("a bench round has no content")?;
        let branch = object.get("branch").ok_or("a bench round has no branch")?;
        Ok(BenchRound {
            id: text(object.get("id"), "id")?,
            parent_id: text_or_null(object.get("parentId"), "parentId")?,
            content: serde_json::from_value(content.clone()).map_err(|e| e.to_string())?,
            prompts: match object.get("prompts") {
                None => Vec::new(),
                Some(links) => serde_json::from_value(links.clone()).map_err(|e| e.to_string())?,
            },
            created_at: text(object.get("createdAt"), "createdAt")?,
            branch: RoundBranch {
                index: count(branch.get("index"), "branch index")?,
                count: count(branch.get("count"), "branch count")?,
                previous_id: text_or_null(branch.get("previousId"), "branch previousId")?,
                next_id: text_or_null(branch.get("nextId"), "branch nextId")?,
            },
        })
    }

    /// `rounds[i]` as the route gives it: the stored node's keys in
    /// `roundNode`'s order, `prompts` only when there are some, then
    /// `branch`.
    pub fn to_json(&self) -> Json {
        let mut object = Object::new();
        object.insert("id".into(), self.id.as_str().into());
        object.insert("parentId".into(), self.parent_id.clone().into());
        object.insert(
            "content".into(),
            serde_json::to_value(&self.content).expect("a round's content is JSON"),
        );
        if !self.prompts.is_empty() {
            object.insert(
                "prompts".into(),
                serde_json::to_value(&self.prompts).expect("prompt links are JSON"),
            );
        }
        object.insert("createdAt".into(), self.created_at.as_str().into());
        let branch = &self.branch;
        object.insert(
            "branch".into(),
            mcbench_store::object! {
                "index" => branch.index, "count" => branch.count,
                "previousId" => branch.previous_id.clone(), "nextId" => branch.next_id.clone(),
            },
        );
        Json::Object(object)
    }
}

impl RoundStats {
    /// `stats[member][round]`: `{ ms, inputTokens, outputTokens }`.
    pub fn to_json(&self) -> Json {
        let number =
            |n: Option<u64>| n.map_or(Json::Null, |n| mcbench_store::json::number(n as f64));
        mcbench_store::object! {
            "ms" => number(self.ms), "inputTokens" => number(self.input_tokens),
            "outputTokens" => number(self.output_tokens),
        }
    }
}

impl BenchDetails {
    /// The details of `bench` as `getBench` read it, with each member's
    /// stats.
    pub fn of(
        bench: &Bench,
        stats: Vec<(String, Vec<RoundStats>)>,
    ) -> Result<BenchDetails, String> {
        let order = bench
            .order
            .as_array()
            .ok_or("a bench's order is not a list")?
            .iter()
            .map(|id| {
                id.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "a bench's order holds a non-text id".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(BenchDetails {
            id: bench.id.clone(),
            title: bench.title.clone(),
            created_at: bench.created_at.clone(),
            head: bench.head.clone(),
            rounds: bench
                .rounds
                .iter()
                .map(BenchRound::of)
                .collect::<Result<_, _>>()?,
            blind: bench.blind,
            revealed: bench.revealed,
            order,
            votes: bench.votes.clone(),
            stats,
        })
    }

    /// `GET /api/benches/:id`'s body.
    pub fn to_json(&self) -> Json {
        let mut votes = Object::new();
        for (round, vote) in &self.votes {
            votes.insert(
                round.clone(),
                mcbench_store::object! { "pick" => vote.pick.clone(), "blind" => vote.blind },
            );
        }
        let mut stats = Object::new();
        for (member, rounds) in &self.stats {
            stats.insert(
                member.clone(),
                Json::Array(rounds.iter().map(RoundStats::to_json).collect()),
            );
        }
        mcbench_store::object! {
            "id" => self.id.as_str(), "title" => self.title.as_str(), "createdAt" => self.created_at.as_str(),
            "head" => self.head.clone(),
            "rounds" => Json::Array(self.rounds.iter().map(BenchRound::to_json).collect()),
            "blind" => self.blind, "revealed" => self.revealed,
            "order" => Json::Array(self.order.iter().map(|id| Json::from(id.as_str())).collect()),
            "votes" => Json::Object(votes),
            "stats" => Json::Object(stats),
        }
    }
}
