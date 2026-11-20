//! A chat's routes (`server/index.js`: `POST /api/conversations`, and
//! under `/api/conversations/:id`: `GET`, `chat`, `stream`, `stop`,
//! `branch`, `seen`, `dismiss-error`, `archive`, `fork`, `selection`,
//! `schematic`; `restore` and `DELETE` are the archive's, in
//! `archive.rs`).

use std::path::Path;
use std::sync::Arc;

use mcbench_agent::lifecycle::{ChatMessage, Refusal, TurnStream};
use mcbench_store::json::Json;

use crate::core::turns::{Join, TurnUpdates};
use crate::core::views::{self, ListedChat, Transcript};
use crate::core::{Core, refusal};

/// A new chat (`POST /api/conversations`' body).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    /// `id`.
    pub id: String,
    /// `provider`.
    pub provider: String,
    /// `model`.
    pub model: String,
}

impl Created {
    /// The route's body.
    pub fn to_json(&self) -> Json {
        mcbench_store::object! { "id" => self.id.as_str(), "provider" => self.provider.as_str(), "model" => self.model.as_str() }
    }
}

/// `settings.PROVIDERS` has `key`.
pub(crate) fn known_provider(key: &str) -> bool {
    crate::core::providers::info(key).is_some()
}

/// `fs.cpSync(from, to, { recursive: true, force: true })` of a chat's
/// `/work`: files copied (cloned where the disk can), folders made, links
/// copied as links.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_symlink() {
            let link = std::fs::read_link(entry.path())?;
            let _ = std::fs::remove_file(&target);
            #[cfg(unix)]
            std::os::unix::fs::symlink(link, &target)?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(link, &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

impl Core {
    /// `GET /api/conversations/stream`'s first line (`listPayload`); each
    /// later line is an `Update::Conversations`.
    pub async fn conversations(&self) -> Result<views::ConversationList, Refusal> {
        self.blocking(|core| core.list_now()).await
    }

    /// `POST /api/conversations`: a new chat with this provider and model.
    pub async fn create_conversation(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<Created, Refusal> {
        let provider = mcbench_providers::api::js::trim(provider).to_string();
        let model = mcbench_providers::api::js::trim(model).to_string();
        if !known_provider(&provider) || model.is_empty() {
            return Err(refusal(400, "Provider and model are required"));
        }
        self.blocking(move |core| {
            let live = core.0.store.create_conversation(
                &provider,
                &model,
                mcbench_store::NewConversation::default(),
            )?;
            let conversation = live.lock().expect("a chat's lock");
            Ok(Created {
                id: conversation.id.0.clone(),
                provider: conversation.provider.clone(),
                model: conversation.model.clone(),
            })
        })
        .await
    }

    /// `GET /api/conversations/:id`: its transcript (`Transcript::to_json`
    /// is the route's body).
    pub async fn conversation(&self, id: &str) -> Result<Transcript, Refusal> {
        let id = id.to_string();
        self.blocking(move |core| core.transcript_now(&id)).await
    }

    /// `POST /api/conversations/:id/chat`: the message stored and its turn
    /// started; the window gets `Update::TurnJoined`, then its events as
    /// `Update::Turn`, the transcript when it starts and once it has ended
    /// (`core/turns.rs`).
    pub async fn chat(&self, id: &str, body: ChatMessage) -> Result<Arc<TurnStream>, Refusal> {
        let stream = self
            .0
            .turns
            .send(
                id,
                body,
                Box::new(TurnUpdates {
                    core: Arc::downgrade(&self.0),
                    chat: id.to_string(),
                    join: Join::Held,
                }),
            )
            .await?;
        Ok(stream)
    }

    /// `POST /api/conversations/:id/stop`.
    pub fn stop_turn(&self, id: &str) {
        self.0.turns.stop(id);
    }

    /// `POST /api/conversations/:id/branch`: the branch through `message`
    /// becomes the chat's; the route's body is `Transcript::branch_json`.
    pub async fn branch(&self, id: &str, message: &str) -> Result<Transcript, Refusal> {
        self.0.turns.branch(id, message).await?;
        let transcript = self.conversation(id).await?;
        self.0.live.changed(self, id);
        Ok(transcript)
    }

    /// `POST /api/conversations/:id/seen`.
    pub async fn seen(&self, id: &str) -> Result<(), Refusal> {
        let id = id.to_string();
        self.blocking(move |core| core.0.store.mark_seen(&id).map_err(Refusal::from))
            .await?;
        self.list_changed();
        Ok(())
    }

    /// `POST /api/conversations/:id/dismiss-error`.
    pub async fn dismiss_error(&self, id: &str, message: &str) -> Result<(), Refusal> {
        self.0.turns.dismiss_error(id, message).await
    }

    /// `ownChat(id)`: a chat of its own (404, or 409 for a bench's chat).
    fn own_chat(&self, id: &str) -> Result<mcbench_store::LiveConversation, Refusal> {
        let live = self
            .0
            .store
            .conversation(id)?
            .ok_or_else(|| refusal(404, "Conversation not found"))?;
        if live.lock().expect("a chat's lock").bench.is_some() {
            return Err(refusal(
                409,
                "A bench chat is changed only through its bench",
            ));
        }
        Ok(live)
    }

    /// `POST /api/conversations/:id/archive`: kept for 7 days; its boxes go.
    pub async fn archive_conversation(&self, id: &str) -> Result<(), Refusal> {
        let chat = id.to_string();
        self.blocking(move |core| core.own_chat(&chat).map(drop))
            .await?;
        if self.0.turns.held(id) {
            return Err(refusal(409, "Stop the running turn before archiving"));
        }
        let chat = id.to_string();
        self.blocking(move |core| {
            core.0
                .store
                .archive_conversation(&chat)
                .map_err(Refusal::from)
        })
        .await?;
        self.0.backend.reset_sandbox(id).await?;
        self.list_changed();
        Ok(())
    }

    /// `POST /api/conversations/:id/fork` with `{ round }`: a bench model's
    /// chat continued as a new chat from the end of round `round` (a whole
    /// number from 1), made whole or not at all; a blind bench is revealed
    /// once the fork is made. Answers the fork's row of the list.
    pub async fn fork(&self, id: &str, round: &Json) -> Result<ListedChat, Refusal> {
        let source_id = id.to_string();
        let round = round.clone();
        let fork = self
            .blocking(move |core| -> Result<String, Refusal> {
                let store = &core.0.store;
                let source = store
                    .conversation(&source_id)?
                    .filter(|live| live.lock().expect("a chat's lock").bench.is_some())
                    .ok_or_else(|| refusal(404, "Bench chat not found"))?;
                let source = source.lock().expect("a chat's lock");
                let prompts: Vec<usize> = source
                    .messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| {
                        matches!(&m.message, mcbench_store::Message::User(user) if !user.see_build)
                    })
                    .map(|(i, _)| i)
                    .collect();
                let number = round
                    .as_f64()
                    .filter(|n| n.fract() == 0.0 && *n >= 1.0 && *n <= prompts.len() as f64)
                    .ok_or_else(|| refusal(400, "Unknown round"))?
                    as usize;
                let next = prompts.get(number).copied();
                if next.is_none() && core.0.turns.held(&source_id) {
                    return Err(refusal(
                        409,
                        "Wait for this round to finish before forking it",
                    ));
                }
                let fork_id = store.host().uuid();
                let folders = mcbench_tools::WorkFolders {
                    root: store.folder().root.join("sandbox"),
                };
                let work = folders.work_dir(&fork_id)?;
                let made = (|| -> Result<(), Refusal> {
                    copy_tree(&folders.work_dir(&source_id)?, &work)
                        .map_err(|e| refusal(500, e.to_string()))?;
                    match next {
                        Some(index) => {
                            let world = source
                                .checkpoint(&source.messages[index].id)
                                .ok_or_else(|| refusal(500, "The round has no checkpoint"))?
                                .clone();
                            store.fork_conversation(
                                &source,
                                index,
                                &world,
                                Some(fork_id.clone()),
                            )?
                        }
                        None => store.fork_conversation(
                            &source,
                            source.messages.len(),
                            &source.world,
                            Some(fork_id.clone()),
                        )?,
                    };
                    Ok(())
                })();
                if let Err(error) = made {
                    let _ = std::fs::remove_dir_all(&work);
                    return Err(error);
                }
                let (bench, _) = source.bench.clone().expect("a bench chat");
                drop(source);
                if let Some(mut bench) = store.bench(&bench.0)?
                    && bench.blind
                    && !bench.revealed
                {
                    store.reveal_bench(&mut bench)?;
                }
                Ok(fork_id)
            })
            .await?;
        self.list_changed();
        self.blocking(move |core| {
            views::conversation_list(&core.0.store, &core.0.turns)?
                .into_iter()
                .find(|row| row.summary.id.0 == fork)
                .ok_or_else(|| refusal(500, "The fork is not listed"))
        })
        .await
    }

    /// `POST /api/conversations/:id/selection` with `{ min, max }`: what the
    /// box holds, named by Minecraft's block names (setup must be done).
    pub async fn selection(
        &self,
        id: &str,
        min: &Json,
        max: &Json,
    ) -> Result<mcbench_world::selection::SelectionDescription, Refusal> {
        let names = self.block_names().await?;
        let (id, min, max) = (id.to_string(), min.clone(), max.clone());
        self.blocking(move |core| {
            let live = core
                .0
                .store
                .conversation(&id)?
                .ok_or_else(|| refusal(404, "Conversation not found"))?;
            let world = live.lock().expect("a chat's lock").world.clone();
            mcbench_world::selection::describe_selection(&world, &min, &max, &names)
                .map_err(|e| refusal(400, e.0))
        })
        .await
    }

    /// The block table's names, once setup has the shipped data.
    async fn block_names(&self) -> Result<Arc<indexmap::IndexMap<String, String>>, Refusal> {
        self.0
            .live
            .sources()
            .map(|sources| Arc::clone(&sources.names))
            .ok_or_else(|| refusal(503, "Minecraft's data is not ready yet"))
    }

    /// `GET /api/conversations/:id/schematic`: the file's name and the
    /// `.schem` bytes of the build as Minecraft keeps it at tick 0.
    pub async fn schematic(&self, id: &str) -> Result<(String, Vec<u8>), Refusal> {
        let chat = id.to_string();
        let (title, mut world) = self
            .blocking(move |core| -> Result<_, Refusal> {
                let live = core
                    .0
                    .store
                    .conversation(&chat)?
                    .ok_or_else(|| refusal(404, "Conversation not found"))?;
                let conversation = live.lock().expect("a chat's lock");
                Ok((conversation.title.clone(), conversation.world.clone()))
            })
            .await?;
        let name = {
            // JavaScript's `\w` is ASCII's.
            let kept: String = mcbench_store::regex!(r"[^A-Za-z0-9_ .-]+")
                .replace_all(&title, "")
                .into_owned();
            let kept = mcbench_providers::api::js::trim(&kept).to_string();
            if kept.is_empty() {
                "build".to_string()
            } else {
                kept
            }
        };
        let sources = self
            .0
            .live
            .sources()
            .ok_or_else(|| refusal(503, "Minecraft's data is not ready yet"))?;
        let settled = sources
            .settler
            .settle(id, &mut world, &[0], None)
            .await
            .map_err(|e| refusal(500, e))?;
        let placed = &settled[&0].value.world;
        if placed.size() == 0 {
            return Err(refusal(
                400,
                "Minecraft keeps none of the blocks of this build: there is nothing to export",
            ));
        }
        let bytes = mcbench_tools::export::schematic(placed).map_err(|e| refusal(500, e))?;
        Ok((format!("{name}.schem"), bytes))
    }
}
