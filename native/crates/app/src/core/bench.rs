//! Bench and the prompt library through the window's commands
//! (`ui-seam-bench-prompts.md`): a bench's sends, its details, its votes,
//! reveal, branch, retry and fork, the library's save and a prompt's
//! improve or write, each ending as `commands.rs` states.
//!
//! The details the core publishes unasked: it keeps the benches whose
//! details the window asked for since the core started (`ReadBench`, or a
//! send to the bench; a deleted bench leaves the set) with the details last
//! published for each. Every chat list the core publishes is followed, under
//! the lists' order, by `Update::Bench` for each bench of the set whose
//! details are no longer the ones last published: a round sent or edited, a
//! member's turn ending, a reveal, a branch switch all change the list, and
//! the details' change goes out after the `Conversations` of the same
//! change. A vote and the votes cleared change no list: their commands
//! publish the details themselves.

use std::collections::HashMap;

use mcbench_agent::BenchDetails;
use mcbench_agent::lifecycle::{BenchRequest, Refusal};
use mcbench_store::json::Json;
use mcbench_ui::{BenchChoice, Generated, LibraryPrompt, Outgoing, Target, Update, VotePick};
use tokio_util::sync::CancellationToken;

use super::{Core, refusal};

/// The details a bench of the set was last published with (`None` until
/// they are).
pub(crate) type Shown = HashMap<String, Option<Result<BenchDetails, String>>>;

/// A bench's details as an update carries them.
fn read(core: &Core, id: &str) -> Result<BenchDetails, Refusal> {
    core.bench_now(id)
}

impl Core {
    fn shown(&self) -> std::sync::MutexGuard<'_, Shown> {
        self.0.benches_shown.lock().expect("the benches shown")
    }

    /// Bench `id` joins the set whose details are published on change.
    fn watch_bench(&self, id: &str) {
        self.shown().entry(id.to_string()).or_insert(None);
    }

    /// Publishes bench `id`'s details as they are now, recording them when
    /// the bench is of the set (blocking; under the lists' order). A bench
    /// that is not there any more leaves the set.
    pub(crate) fn publish_bench_held(&self, id: &str) {
        let details = read(self, id);
        let gone = matches!(&details, Err(refused) if refused.status == 404);
        let details = details.map_err(|refused| refused.message);
        {
            let mut shown = self.shown();
            if gone {
                shown.remove(id);
            } else if let Some(last) = shown.get_mut(id) {
                *last = Some(details.clone());
            }
        }
        self.publish(Update::Bench {
            bench: id.to_string(),
            details,
        });
    }

    /// After a chat list is published (blocking; under the lists' order):
    /// `Update::Bench` for each bench of the set whose details changed
    /// since they were last published, and for `also` (a command's own
    /// bench) whether or not they did. A bench that is not there any more
    /// leaves the set and publishes nothing unasked.
    pub(crate) fn publish_benches_held(&self, also: Option<&str>) {
        let ids: Vec<String> = self.shown().keys().cloned().collect();
        for id in &ids {
            if Some(id.as_str()) == also {
                continue;
            }
            let details = read(self, id);
            if matches!(&details, Err(refused) if refused.status == 404) {
                self.shown().remove(id);
                continue;
            }
            let details = details.map_err(|refused| refused.message);
            let changed = {
                let mut shown = self.shown();
                match shown.get_mut(id) {
                    Some(last) if last.as_ref() != Some(&details) => {
                        *last = Some(details.clone());
                        true
                    }
                    _ => false,
                }
            };
            if changed {
                self.publish(Update::Bench {
                    bench: id.clone(),
                    details,
                });
            }
        }
        if let Some(id) = also {
            self.publish_bench_held(id);
        }
    }

    /// Bench `id`'s details now (`Update::Bench`), under the lists' order.
    pub(crate) async fn publish_bench(&self, id: &str) {
        let id = id.to_string();
        self.blocking(move |core| {
            let _order = core.0.list_order.lock().expect("the lists' order");
            core.publish_bench_held(&id);
        })
        .await
    }

    /// The chat list now, then bench `id`'s details whether or not they
    /// changed (and every other bench of the set whose did), then the
    /// archive when it was read.
    pub(crate) async fn end_in_lists_and_bench(&self, id: &str) {
        self.publish_lists_and(Some(id)).await;
        self.publish_archive_if_read().await;
    }

    /// `Update::Leaderboard` read fresh.
    pub(crate) async fn publish_leaderboard(&self) {
        let board = self.leaderboard_board().await.map_err(|e| e.message);
        self.publish(Update::Leaderboard(board));
    }

    /// `Update::Bench` for every bench of the set (the votes cleared).
    pub(crate) async fn publish_every_bench(&self) {
        let ids: Vec<String> = self.shown().keys().cloned().collect();
        for id in ids {
            self.publish_bench(&id).await;
        }
    }

    /// A deleted bench leaves the set.
    pub(crate) fn forget_bench(&self, id: &str) {
        self.shown().remove(id);
    }

    /// `Command::ReadBench`.
    pub(crate) async fn read_bench_command(&self, id: &str) {
        self.watch_bench(id);
        self.publish_bench(id).await;
    }

    /// `effort` is each model's own on a bench: a message that carries one
    /// is a window's mistake, refused naming the field.
    fn no_effort(outgoing: &Outgoing) -> Result<(), Refusal> {
        match &outgoing.effort {
            None => Ok(()),
            Some(_) => Err(refusal(
                400,
                "effort is not sent with a bench's round: each model has its own",
            )),
        }
    }

    /// `Command::Send` to `Target::NewBench`: `POST /api/benches`, then the
    /// list that has it, its details, and `Update::BenchSent`.
    pub(crate) async fn send_new_bench(
        &self,
        outgoing: &Outgoing,
        members: &[BenchChoice],
        blind: bool,
    ) -> Result<(), Refusal> {
        Self::no_effort(outgoing)?;
        let message = Self::turn_message(outgoing)?;
        let id = self
            .create_bench(BenchRequest {
                members: members
                    .iter()
                    .map(|m| (m.provider.clone(), m.model.clone(), m.effort.clone()))
                    .collect(),
                blind,
                resolution: outgoing.resolution.value(),
                message,
            })
            .await?;
        self.watch_bench(&id);
        self.publish_lists_and(None).await;
        self.publish(Update::BenchSent {
            target: outgoing.target.clone(),
            bench: id,
            round: 1,
        });
        Ok(())
    }

    /// `Command::Send` to `Target::Bench`: `POST /api/benches/:id/rounds`
    /// (`edit_of`: the round's id), then the list, the details with the
    /// round, and `Update::BenchSent` with today's answer's `rounds`.
    pub(crate) async fn send_round(&self, outgoing: &Outgoing, id: &str) -> Result<(), Refusal> {
        Self::no_effort(outgoing)?;
        let bench = id.to_string();
        let own = self
            .blocking(move |core| -> Result<u32, Refusal> {
                core.bench_of(&bench)?;
                let members = core.0.store.bench_members(&bench)?;
                let first = members
                    .first()
                    .ok_or_else(|| refusal(500, "The bench has no models"))?;
                Ok(first.lock().expect("a chat's lock").resolution)
            })
            .await?;
        let sent = outgoing.resolution.value();
        if sent != own {
            return Err(refusal(
                400,
                format!("resolution {sent} is not this bench's ({own})"),
            ));
        }
        let message = Self::turn_message(outgoing)?;
        let round = self
            .bench_round(
                id,
                message,
                outgoing.edit_of.as_ref().map(|round| round.0.clone()),
            )
            .await?;
        self.watch_bench(id);
        self.publish_lists_and(None).await;
        self.publish(Update::BenchSent {
            target: Target::Bench(id.to_string()),
            bench: id.to_string(),
            round,
        });
        Ok(())
    }

    /// `Command::Vote`: `Update::Bench` with the new votes, then
    /// `Update::Leaderboard`; a refusal is `Update::Error`, then the
    /// unchanged `Update::Bench`.
    pub(crate) async fn vote_command(&self, id: &str, round: &str, pick: &VotePick) {
        let pick = match pick {
            VotePick::Member(member) => Json::String(member.clone()),
            VotePick::Tie => Json::String("tie".into()),
            VotePick::TakeBack => Json::Null,
        };
        match self.vote(id, &Json::String(round.to_string()), &pick).await {
            Ok(_) => {
                self.publish_bench(id).await;
                self.publish_leaderboard().await;
            }
            Err(error) => {
                self.publish(Update::Error(error.message));
                self.publish_bench(id).await;
            }
        }
    }

    /// `Command::Fork`: `Update::Forked` last, after the list with the new
    /// chat on success.
    pub(crate) async fn fork_command(&self, member: &str, round: usize) {
        let chat = match self
            .fork(member, &mcbench_store::json::number(round as f64))
            .await
        {
            Ok(row) => {
                self.publish_lists_and(None).await;
                Ok(row.summary.id.0)
            }
            Err(error) => Err(error.message),
        };
        self.publish(Update::Forked {
            member: member.to_string(),
            chat,
        });
    }

    /// `Command::SavePrompts`: `PUT /api/prompts` with the library as the
    /// window holds it; `Update::PromptsSaved` alone.
    pub(crate) async fn save_prompts_command(&self, library: &[LibraryPrompt]) {
        let body = Json::Array(library.iter().map(LibraryPrompt::to_json).collect());
        let saved = self.save_prompts(&body).await.map_err(|e| e.message);
        self.publish(Update::PromptsSaved(saved));
    }

    /// `Command::GeneratePrompt`: the request's token is kept by its number
    /// until it ends (a child of the core's own, so shutdown fires it);
    /// `Update::Generated` with `Cancelled` when it was fired.
    pub(crate) async fn generate_command(
        &self,
        request: u64,
        provider: &str,
        model: &str,
        effort: Option<&str>,
        text: &str,
    ) {
        let token = self.generate_token(request);
        let made = self
            .generate_prompt(provider, model, effort.unwrap_or_default(), text, &token)
            .await;
        // Decided, published and ended under the requests' lock: a cancel
        // either comes first (the outcome is `Cancelled`) or finds nothing.
        {
            let mut generating = self.generating();
            let outcome = if token.is_cancelled() {
                Generated::Cancelled
            } else {
                match made {
                    Ok(text) => Generated::Text(text),
                    Err(error) => Generated::Failed(error.message),
                }
            };
            self.publish(Update::Generated { request, outcome });
            generating.remove(&request);
        }
        self.0.prompt_ended.send_replace(());
    }

    /// `Command::CancelGenerate`: fires the request's token; nothing for a
    /// request that has ended.
    pub(crate) fn cancel_generate(&self, request: u64) {
        if let Some(token) = self.generating().get(&request) {
            token.cancel();
        }
    }

    /// Request `request`'s token, made the first time it is asked for: by
    /// `mcbench_ui::Core::send` as the command is taken, so a cancel sent
    /// after it finds it whichever command's task runs first.
    pub(crate) fn generate_token(&self, request: u64) -> CancellationToken {
        self.generating()
            .entry(request)
            .or_insert_with(|| self.0.stopping.child_token())
            .clone()
    }

    /// Resolves once no prompt request is under way.
    pub(crate) async fn prompt_requests_ended(&self) {
        let mut ended = self.0.prompt_ended.subscribe();
        while !self.generating().is_empty() {
            ended.changed().await.expect("the core holds the sender");
        }
    }

    fn generating(&self) -> std::sync::MutexGuard<'_, HashMap<u64, CancellationToken>> {
        self.0.generating.lock().expect("the prompt requests")
    }
}
