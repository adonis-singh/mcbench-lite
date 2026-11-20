//! A bench's routes (`server/index.js`: `POST /api/benches`, and under
//! `/api/benches/:id`: `GET`, `vote`, `reveal`, `branch`, `rounds`,
//! `retry`, `archive`; `restore`, `DELETE` and the votes' reset are the
//! archive's, `share` is share's).

use mcbench_agent::lifecycle::{BenchRequest, Outgoing, Refusal};
use mcbench_agent::{BenchDetails, RoundStats};
use mcbench_store::json::{Json, Object};
use mcbench_store::{Bench, Message, StoredMessage, Vote};

use crate::core::{Core, refusal};

/// `Date.parse` of `toISOString()`'s text (`YYYY-MM-DDTHH:MM:SS.mmmZ`),
/// epoch ms; `NaN` (here `None`) for any other text.
pub fn parse_iso(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() != 24 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' {
        return None;
    }
    if b[16] != b':' || b[19] != b'.' || b[23] != b'Z' {
        return None;
    }
    let n = |from: usize, to: usize| text.get(from..to)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0, 4)?, n(5, 7)?, n(8, 10)?);
    let (h, mi, s, ms) = (n(11, 13)?, n(14, 16)?, n(17, 19)?, n(20, 23)?);
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((((days * 24 + h) * 60 + mi) * 60 + s) * 1000 + ms)
}

/// `roundStats(member)`: per round, the wall time from its prompt to its
/// latest message and the tokens its requests reported (null when none
/// did).
pub fn round_stats(messages: &[StoredMessage]) -> Result<Vec<RoundStats>, String> {
    struct Round {
        started: Option<i64>,
        ms: Option<i64>,
        input: Option<f64>,
        output: Option<f64>,
    }
    let mut rounds: Vec<Round> = Vec::new();
    for stored in messages {
        let value = mcbench_json::parse(stored.payload_text())?;
        let created = value
            .get("createdAt")
            .and_then(Json::as_str)
            .and_then(parse_iso);
        if matches!(&stored.message, Message::User(user) if !user.see_build) {
            rounds.push(Round {
                started: created,
                ms: None,
                input: None,
                output: None,
            });
            continue;
        }
        let Some(round) = rounds.last_mut() else {
            continue;
        };
        round.ms = match (created, round.started) {
            (Some(at), Some(started)) => Some(at - started),
            _ => None,
        };
        let usage = value.get("telemetry").and_then(|t| t.get("usage"));
        let count = |key: &str| usage.and_then(|u| u.get(key)).and_then(Json::as_f64);
        if let Some(n) = count("inputTokens") {
            round.input = Some(round.input.unwrap_or(0.0) + n);
        }
        if let Some(n) = count("outputTokens") {
            round.output = Some(round.output.unwrap_or(0.0) + n);
        }
    }
    let whole = |n: Option<f64>, what: &str| -> Result<Option<u64>, String> {
        n.map(|n| {
            if n >= 0.0 && n.fract() == 0.0 && n <= u64::MAX as f64 {
                Ok(n as u64)
            } else {
                Err(format!("a round's {what} is {n}, not a whole count"))
            }
        })
        .transpose()
    };
    rounds
        .into_iter()
        .map(|r| {
            Ok(RoundStats {
                ms: whole(r.ms.map(|ms| ms as f64), "time")?,
                input_tokens: whole(r.input, "input tokens")?,
                output_tokens: whole(r.output, "output tokens")?,
            })
        })
        .collect()
}

/// The votes as `getBench` holds them: by round, `{ pick, blind }`.
pub fn votes_json(votes: &[(String, Vote)]) -> Json {
    let mut object = Object::new();
    for (round, vote) in votes {
        object.insert(
            round.clone(),
            mcbench_store::object! { "pick" => vote.pick.clone(), "blind" => vote.blind },
        );
    }
    Json::Object(object)
}

impl Core {
    pub(crate) fn bench_of(&self, id: &str) -> Result<Bench, Refusal> {
        self.0
            .store
            .bench(id)?
            .ok_or_else(|| refusal(404, "Bench not found"))
    }

    /// `POST /api/benches`: answers the new bench's id.
    pub async fn create_bench(&self, body: BenchRequest) -> Result<String, Refusal> {
        self.0.turns.create_bench(body).await
    }

    /// `GET /api/benches/:id`: the bench without its nodes, and per model
    /// its rounds' wall times and tokens (`stats`); its JSON is
    /// [`BenchDetails::to_json`].
    pub async fn bench(&self, id: &str) -> Result<BenchDetails, Refusal> {
        let id = id.to_string();
        self.blocking(move |core| core.bench_now(&id)).await
    }

    /// [`Core::bench`], blocking (the store).
    pub(crate) fn bench_now(&self, id: &str) -> Result<BenchDetails, Refusal> {
        let bench = self.bench_of(id)?;
        let mut stats = Vec::new();
        for member in self.0.store.bench_members(id)? {
            let member = member.lock().expect("a chat's lock");
            stats.push((member.id.0.clone(), round_stats(&member.messages)?));
        }
        BenchDetails::of(&bench, stats).map_err(|e| refusal(500, e))
    }

    /// `memberRoundState(id, index)`.
    fn member_round_state(&self, id: &str, index: usize) -> Result<&'static str, Refusal> {
        let (round, halted) = self.0.turns.member_progress(id)?;
        let index = index as i64;
        Ok(if round > index + 1 {
            "done"
        } else if round == index + 1 {
            if self.0.turns.held(id) {
                "running"
            } else if halted.is_some() {
                "failed"
            } else {
                "done"
            }
        } else if halted.is_some() {
            "held"
        } else {
            "queued"
        })
    }

    /// `POST /api/benches/:id/vote` with `{ roundId, pick }`: the best build
    /// of a round (a member id), a tie (`"tie"`) or the vote taken back
    /// (`null`), while the names are hidden. Answers `votes`.
    pub async fn vote(&self, id: &str, round: &Json, pick: &Json) -> Result<Json, Refusal> {
        let (id, round, pick) = (id.to_string(), round.clone(), pick.clone());
        self.blocking(move |core| {
            let mut bench = core.bench_of(&id)?;
            if !bench.blind || bench.revealed {
                return Err(refusal(
                    409,
                    "Votes are cast while the models' names are hidden",
                ));
            }
            let index = (0..bench.rounds.len())
                .find(|&i| bench.rounds[i].get("id") == Some(&round))
                .ok_or_else(|| refusal(400, "Unknown round"))?;
            let round_id = round
                .as_str()
                .expect("a round's id is a string")
                .to_string();
            let members: Vec<String> = core
                .0
                .store
                .bench_members(&id)?
                .iter()
                .map(|m| m.lock().expect("a chat's lock").id.0.clone())
                .collect();
            let states = members
                .iter()
                .map(|m| core.member_round_state(m, index))
                .collect::<Result<Vec<_>, _>>()?;
            if states.iter().any(|s| *s == "running" || *s == "queued") {
                return Err(refusal(
                    409,
                    "Vote once every model has finished this round",
                ));
            }
            let judged: Vec<String> = members
                .iter()
                .zip(&states)
                .filter(|(_, s)| **s == "done")
                .map(|(m, _)| m.clone())
                .collect();
            if judged.len() < 2 {
                return Err(refusal(409, "A vote needs at least two finished builds"));
            }
            match &pick {
                Json::Null => core.0.store.clear_vote(&mut bench, &round_id)?,
                Json::String(tie) if tie == "tie" => core
                    .0
                    .store
                    .cast_vote(&mut bench, &round_id, None, &judged)?,
                Json::String(member) if judged.contains(member) => {
                    core.0
                        .store
                        .cast_vote(&mut bench, &round_id, Some(member), &judged)?
                }
                _ => return Err(refusal(400, "Pick a model that finished this round")),
            }
            Ok(votes_json(&bench.votes))
        })
        .await
    }

    /// `POST /api/benches/:id/reveal`: every model's name shows for good.
    pub async fn reveal(&self, id: &str) -> Result<(), Refusal> {
        let id = id.to_string();
        self.blocking(move |core| {
            let mut bench = core.bench_of(&id)?;
            core.0.store.reveal_bench(&mut bench).map_err(Refusal::from)
        })
        .await?;
        self.list_changed();
        Ok(())
    }

    /// `POST /api/benches/:id/branch` with `{ roundId }`: answers the
    /// round's number.
    pub async fn bench_branch(&self, id: &str, round: &str) -> Result<usize, Refusal> {
        let bench = id.to_string();
        self.blocking(move |core| core.bench_of(&bench).map(drop))
            .await?;
        self.0.turns.bench_branch(id, round).await
    }

    /// `POST /api/benches/:id/rounds`: the next round, or with `edit_of` a
    /// new version of that round. Answers the bench's round count.
    pub async fn bench_round(
        &self,
        id: &str,
        message: Outgoing,
        edit_of: Option<String>,
    ) -> Result<usize, Refusal> {
        self.0.turns.bench_round(id, message, edit_of).await
    }

    /// `POST /api/benches/:id/retry` with `{ memberId }`.
    pub async fn retry_member(&self, id: &str, member: &str) -> Result<(), Refusal> {
        self.0.turns.retry_member(id, member).await
    }

    /// `POST /api/benches/:id/archive`: refused while any of its models
    /// runs; its boxes go, everything on disk stays until the purge.
    pub async fn archive_bench(&self, id: &str) -> Result<(), Refusal> {
        let bench = id.to_string();
        let members = self
            .blocking(move |core| -> Result<Vec<String>, Refusal> {
                core.bench_of(&bench)?;
                Ok(core
                    .0
                    .store
                    .bench_members(&bench)?
                    .iter()
                    .map(|m| m.lock().expect("a chat's lock").id.0.clone())
                    .collect())
            })
            .await?;
        if std::iter::once(format!("bench:{id}"))
            .chain(members)
            .any(|key| self.0.turns.held(&key))
        {
            return Err(refusal(409, "Stop the running models before archiving"));
        }
        let bench = id.to_string();
        let archived = self
            .blocking(move |core| core.0.store.archive_bench(&bench).map_err(Refusal::from))
            .await?;
        for member in archived {
            self.0.backend.reset_sandbox(&member).await?;
        }
        self.list_changed();
        Ok(())
    }
}
