//! The window's commands (`mcbench_ui::Command`): each runs as a task on
//! the core's runtime and ends in the updates the seam's rules name.
//!
//! A command that changes state always ends in the update that carries
//! the state it changed, with `Update::Error` (the route's sentence)
//! before it when it failed: archive, restore, delete and mark seen in
//! `Update::Conversations` (and `Update::Archive` again once the archive
//! was read since the core started); a branch switch and a dismissed error
//! in the chat's `Update::Transcript`, or `Update::TranscriptFailed` when
//! the chat itself can no longer be read; the favourites in
//! `Update::Favorites`; a sign-in, its code and its cancelling in
//! `Update::SignIn`; a sign-out in `Update::Providers` and the settings
//! read fresh; an install in `Update::Install`; a setup retry in
//! `Update::Setup`; clearing the votes in `Update::Leaderboard`, then
//! `Update::Bench` for every bench whose details were asked for. Saving
//! the window's preferences has no update on success. Saving the settings
//! and saving the prompt library are the exceptions: settings end in
//! `Update::Providers`, `Update::Favorites`, then
//! `Update::SettingsSaved(Ok)`, and a refusal is `SettingsSaved(Err)` alone
//! (its sentence belongs in the Settings panel); the library ends in
//! `Update::PromptsSaved` alone (the library as saved, or the refusal's
//! sentence), never an `Update::Prompts` of its own. A read answers with
//! its result or the route's sentence. A send refused before its turn
//! starts is `Update::SendRefused` with the route's sentence. Sharing's five
//! commands end as `share.rs` states.
//!
//! Bench (`bench.rs`): a send to `Target::NewBench` or `Target::Bench`
//! ends in `Update::BenchSent` after the `Update::Conversations` that lists
//! the bench and the `Update::Bench` that holds the round, or in
//! `Update::SendRefused`; an `effort` on either, or a `resolution` that is
//! not the bench's on `Target::Bench`, is refused naming the field.
//! `ReadBench` answers `Update::Bench`. `Vote` ends in `Update::Bench` then
//! `Update::Leaderboard`, a refusal in `Update::Error` then the unchanged
//! `Update::Bench`. `Reveal` and `BenchBranch` end in
//! `Update::Conversations` then `Update::Bench`, `Update::Error` first on a
//! failure. `RetryMember` ends in `Update::Conversations`, `Update::Error`
//! first on a refusal. `Fork` ends in `Update::Forked`, after the
//! `Update::Conversations` that has the new chat when it worked.
//! `GeneratePrompt` ends in `Update::Generated` with its request's number;
//! `CancelGenerate` fires that request's token, which ends it in
//! `Generated::Cancelled`, and publishes nothing for a request that has
//! ended.
//!
//! The live world (`live.rs`): `Live`, `Viewer` and `ExportWorld` are
//! handed to the live world's task in the order sent and end as
//! `mcbench_ui::LiveCommand` and `ViewerInput` state; `ExportWorld` ends in
//! `Update::Exported`. `DescribeSelection` is a read ending in
//! `Update::SelectionDescribed` (the selection route's sentences as its
//! `Err`). `ExportSchematic` writes the schematic route's bytes to a
//! temporary file beside `to` renamed over it, ending in `Update::Exported`
//! (the route's sentence, or the file's error, as its `Err`).

use std::sync::atomic::Ordering;

use mcbench_agent::lifecycle::{ChatMessage, Outgoing as TurnOutgoing, Refusal};
use mcbench_store::json::Json;
use mcbench_ui::{Auth, Command, Outgoing, Provider, Subject, Target, Update};

use super::routes::logins::SignInCall;
use super::{Core, preferences, refusal, settings};

impl Core {
    /// Runs one command to its end (`mcbench_ui::Core::send` spawns this
    /// and returns): when it resolves, every update the command publishes
    /// itself has been published.
    pub async fn command(&self, command: Command) {
        match command {
            Command::Send(outgoing) => self.send_message(outgoing).await,
            Command::Stop { chat } => self.0.turns.stop(&chat),
            Command::Branch { chat, message } => {
                match self.0.turns.branch(&chat, &message.0).await {
                    Ok(()) => self.0.live.changed(self, &chat),
                    Err(error) => self.publish(Update::Error(error.message)),
                }
                self.end_in_transcript(&chat).await;
            }
            Command::Open { chat } => self.open(&chat).await,
            Command::JoinTurn { chat } => self.join_turn(&chat),
            Command::MarkSeen { chat } => {
                if let Err(error) = self.seen(&chat).await {
                    self.publish(Update::Error(error.message));
                }
                self.end_in_lists().await;
            }
            Command::DismissError { chat, message } => {
                if let Err(error) = self.dismiss_error(&chat, &message.0).await {
                    self.publish(Update::Error(error.message));
                }
                self.end_in_transcript(&chat).await;
            }
            Command::Archive(subject) => {
                let done = match &subject {
                    Subject::Chat(id) => self.archive_conversation(id).await,
                    Subject::Bench(id) => self.archive_bench(id).await,
                };
                self.end_in_lists_after(done).await;
            }
            Command::Restore(subject) => {
                let done = match &subject {
                    Subject::Chat(id) => self.restore_conversation(id).await,
                    Subject::Bench(id) => self.restore_bench(id).await,
                };
                self.end_in_lists_after(done).await;
            }
            Command::Delete(subject) => {
                let done = match &subject {
                    Subject::Chat(id) => self.delete_conversation(id).await,
                    Subject::Bench(id) => {
                        let done = self.delete_bench(id).await;
                        if done.is_ok() {
                            self.forget_bench(id);
                        }
                        done
                    }
                };
                self.end_in_lists_after(done).await;
            }
            Command::ReadArchive => {
                self.0.archive_read.store(true, Ordering::SeqCst);
                let archive = self.archive_list().await.map_err(|e| e.message);
                self.publish(Update::Archive(archive));
            }
            Command::Search {
                scope,
                query,
                request,
            } => {
                let results = self.search_hits(scope, &query).await.map_err(|e| e.message);
                self.publish(Update::Found { request, results });
            }
            Command::Models { provider } => {
                let models = self.model_list(&provider).await.map_err(|e| e.message);
                self.publish(Update::Models { provider, models });
            }
            Command::ModelInfo { provider, model } => {
                let facts = self
                    .model_facts(&provider, &model)
                    .await
                    .map_err(|e| e.message);
                self.publish(Update::ModelInfo {
                    provider,
                    model,
                    facts,
                });
            }
            Command::SetFavorites(favorites) => {
                if let Err(error) = self.set_favorites(&favorites).await {
                    // `web/app.js`'s notice for a star that was not saved.
                    self.publish(Update::Error(format!(
                        "The star was not saved, and went back: {}",
                        error.message
                    )));
                }
                self.publish_favorites().await;
            }
            Command::SavePreferences(kept) => {
                let file = preferences::file(&self.0.store.folder().root);
                let saved = self
                    .blocking(move |_| preferences::save(&file, &kept))
                    .await;
                if let Err(error) = saved {
                    self.publish(Update::Error(error));
                }
            }
            Command::ReadSettings { refresh } => {
                let view = self.settings_view(refresh).await.map_err(|e| e.message);
                if refresh && view.is_ok() {
                    self.publish_providers().await;
                }
                self.publish(Update::Settings(view));
            }
            Command::SaveSettings(change) => match self.save_settings(&change).await {
                Ok(()) => {
                    self.publish_providers().await;
                    self.publish_favorites().await;
                    self.publish(Update::SettingsSaved(Ok(())));
                }
                Err(error) => self.publish(Update::SettingsSaved(Err(error.message))),
            },
            Command::SignIn { provider, method } => {
                self.sign_in_command(&provider, SignInCall::Start(method.as_deref()))
                    .await
            }
            Command::SignInInput { provider, text } => {
                self.sign_in_command(&provider, SignInCall::Input(&text))
                    .await
            }
            Command::CancelSignIn { provider } => {
                self.sign_in_command(&provider, SignInCall::Cancel).await
            }
            Command::SignOut { provider } => {
                if let Err(error) = self.sign_out(&provider).await {
                    self.publish(Update::Error(error.message));
                }
                self.publish_providers().await;
                self.publish_fresh_settings().await;
            }
            Command::InstallCli { program } => self.install_command(&program).await,
            Command::RetrySetup => match self.retry_setup() {
                Ok((_, steps)) => self.publish(Update::Setup(steps)),
                Err(error) => {
                    self.publish(Update::Error(error.message));
                    self.publish(Update::Setup(self.setup_view().1));
                }
            },
            Command::ReadLeaderboard => self.publish_leaderboard().await,
            Command::ClearVotes => {
                if let Err(error) = self.clear_votes().await {
                    self.publish(Update::Error(error.message));
                }
                self.publish_leaderboard().await;
                self.publish_every_bench().await;
            }
            // Sharing (`share.rs`).
            command @ (Command::ReadShares
            | Command::RemoveShare { .. }
            | Command::ReadShare { .. }
            | Command::Publish { .. }
            | Command::Unshare { .. }) => self.share_command(command).await,
            command @ (Command::Live(_) | Command::Viewer(_) | Command::ExportWorld { .. }) => {
                self.0.live.run(command).await;
            }
            Command::DescribeSelection {
                chat,
                request,
                min,
                max,
            } => {
                let corner = |c: [i64; 3]| Json::Array(c.iter().map(|&v| Json::from(v)).collect());
                let result = self
                    .selection(&chat, &corner(min), &corner(max))
                    .await
                    .map_err(|e| e.message);
                self.publish(Update::SelectionDescribed { request, result });
            }
            Command::ExportSchematic { chat, to } => {
                let written = match self.schematic(&chat).await {
                    Ok((_, bytes)) => {
                        self.blocking(move |_| super::live::write_beside(&to, &bytes).map(|()| to))
                            .await
                    }
                    Err(error) => Err(error.message),
                };
                self.publish(Update::Exported(written));
            }
            Command::InstallWsl => self.install_wsl().await,
            Command::RestartComputer => self.restart_computer().await,
            Command::ReadPrompts => self.publish_prompts().await,
            Command::ReadBench { bench } => self.read_bench_command(&bench).await,
            Command::Vote { bench, round, pick } => self.vote_command(&bench, &round, &pick).await,
            Command::Reveal { bench } => {
                if let Err(error) = self.reveal(&bench).await {
                    self.publish(Update::Error(error.message));
                }
                self.end_in_lists_and_bench(&bench).await;
            }
            Command::BenchBranch { bench, round } => {
                if let Err(error) = self.bench_branch(&bench, &round).await {
                    self.publish(Update::Error(error.message));
                }
                self.end_in_lists_and_bench(&bench).await;
            }
            Command::RetryMember { bench, member } => {
                let done = self.retry_member(&bench, &member).await;
                self.end_in_lists_after(done).await;
            }
            Command::Fork { member, round } => self.fork_command(&member, round).await,
            Command::SavePrompts(library) => self.save_prompts_command(&library).await,
            Command::GeneratePrompt {
                request,
                provider,
                model,
                effort,
                text,
            } => {
                self.generate_command(request, &provider, &model, effort.as_deref(), &text)
                    .await
            }
            Command::CancelGenerate { request } => self.cancel_generate(request),
            Command::CreateChat { provider, model } => {
                self.create_chat_command(&provider, &model).await
            }
            Command::ReadProviders => self.publish_providers().await,
        }
    }

    /// `Command::CreateChat` (`POST /api/conversations`, the page's
    /// `createdChat`): the new row's id, or the route's text, as
    /// `Update::ChatCreated`. The row has no message, so the list leaves it
    /// out until its first one (`emptyChat`) and the store sweeps it by its
    /// 7-day rule unless this run made or loaded it; the core keeps nothing
    /// about it for the window.
    async fn create_chat_command(&self, provider: &str, model: &str) {
        let created = self.create_conversation(provider, model).await;
        self.publish(Update::ChatCreated(
            created.map(|row| row.id).map_err(|error| error.message),
        ));
    }

    /// The message of an `Outgoing` as the chat route's body has it: each
    /// file `{ name, type, size, data }` with its bytes as a data URL, as
    /// today's page sends them; its selections as stored.
    pub(crate) fn turn_message(outgoing: &Outgoing) -> Result<TurnOutgoing, Refusal> {
        use base64::Engine as _;
        let files = outgoing
            .attachments
            .iter()
            .map(|file| {
                mcbench_store::object! {
                    "name" => file.name.as_str(),
                    "type" => file.content_type.as_str(),
                    "size" => mcbench_store::json::number(file.size as f64),
                    "data" => format!(
                        "data:{};base64,{}",
                        file.content_type,
                        base64::engine::general_purpose::STANDARD.encode(&file.data)
                    ),
                }
            })
            .collect();
        let selections = outgoing
            .selections
            .iter()
            .map(|s| {
                Ok(mcbench_store::Selection {
                    letter: s.letter.clone(),
                    label: s.label.clone(),
                    min: s.min,
                    max: s.max,
                    blocks: u64::try_from(s.blocks).map_err(|_| {
                        refusal(400, "A selection holds no negative count of blocks")
                    })?,
                    block: s.block.clone(),
                })
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        Ok(TurnOutgoing {
            text: outgoing.text.clone(),
            files,
            selections,
            prompts: outgoing.prompts.clone(),
        })
    }

    /// `Command::Send`: to a chat (`send_chat`), a new bench or a bench
    /// (`bench.rs`). A refusal before the turns start is
    /// `Update::SendRefused`.
    async fn send_message(&self, outgoing: Outgoing) {
        let sent = match &outgoing.target {
            Target::NewChat { .. } | Target::Chat { .. } => self.send_chat(&outgoing).await,
            Target::NewBench { members, blind } => {
                self.send_new_bench(&outgoing, members, *blind).await
            }
            Target::Bench(id) => self.send_round(&outgoing, id).await,
        };
        if let Err(error) = sent {
            self.publish(Update::SendRefused {
                target: outgoing.target,
                error: error.message,
            });
        }
    }

    /// A send to a chat: a new chat's row first for `Target::NewChat`
    /// (`POST /api/conversations`), then the chat route with the message,
    /// its provider and model, its effort and resolution and `editOf`; the
    /// turn's start is `Update::TurnJoined`, then its events.
    /// `Target::Chat` carries the provider and model the picker has at the
    /// send, as the page's body does, and the route sets the chat's from
    /// them (`handleChat`: `conversation.provider = provider`,
    /// `conversation.model = model`), after its own checks (`Choose a
    /// configured provider`, `Choose a model`).
    async fn send_chat(&self, outgoing: &Outgoing) -> Result<(), Refusal> {
        let message = Self::turn_message(outgoing)?;
        let (chat, provider, model) = match &outgoing.target {
            Target::NewChat { provider, model } => {
                let created = self.create_conversation(provider, model).await?;
                (created.id, provider.clone(), model.clone())
            }
            Target::Chat {
                id,
                provider,
                model,
            } => (id.clone(), provider.clone(), model.clone()),
            Target::NewBench { .. } | Target::Bench(_) => unreachable!("a chat's send"),
        };
        self.chat(
            &chat,
            ChatMessage {
                provider,
                model,
                effort: Some(outgoing.effort.clone().unwrap_or_default()),
                resolution: outgoing.resolution.value(),
                message,
                edit_of: outgoing.edit_of.as_ref().map(|id| id.0.clone()),
            },
        )
        .await
        .map(drop)
    }

    /// `Command::Open` (`GET /api/conversations/:id`): the transcript (or
    /// `Update::TranscriptFailed`) and nothing else; the live world's view is
    /// `Command::Live(Show)`'s.
    /// It does not mark the chat seen, as today's route does not: only
    /// `Command::MarkSeen` does, which the window sends.
    async fn open(&self, chat: &str) {
        match self.conversation(chat).await {
            Ok(transcript) => self.publish(transcript.into_update()),
            Err(error) => self.publish(Update::TranscriptFailed {
                chat: chat.to_string(),
                error: error.message,
            }),
        }
    }

    /// The chat's transcript as it is now, or why it cannot be read.
    async fn end_in_transcript(&self, chat: &str) {
        match self.conversation(chat).await {
            Ok(transcript) => self.publish(transcript.into_update()),
            Err(error) => self.publish(Update::TranscriptFailed {
                chat: chat.to_string(),
                error: error.message,
            }),
        }
    }

    /// `Update::Error` when `done` failed, then the lists.
    async fn end_in_lists_after(&self, done: Result<(), Refusal>) {
        if let Err(error) = done {
            self.publish(Update::Error(error.message));
        }
        self.end_in_lists().await;
    }

    /// The chat list as it is now, and the archive when it was read.
    async fn end_in_lists(&self) {
        self.publish_list_now().await;
        self.publish_archive_if_read().await;
    }

    /// The archive (`Update::Archive`) when it was read since the core
    /// started.
    pub(crate) async fn publish_archive_if_read(&self) {
        if self.0.archive_read.load(Ordering::SeqCst) {
            let archive = self.archive_list().await.map_err(|e| e.message);
            self.publish(Update::Archive(archive));
        }
    }

    /// The prompt library (`Update::Prompts`).
    pub(crate) async fn publish_prompts(&self) {
        let library = self.prompts().await.map_err(|e| e.message);
        self.publish(Update::Prompts(library));
    }

    /// `settings.get().shareRepo`: the saved repository for shared builds;
    /// none when none is set.
    pub(crate) async fn share_repo(&self) -> Option<String> {
        self.blocking(|core| {
            core.0
                .providers
                .settings()
                .0
                .get("shareRepo")
                .and_then(|repo| repo.as_str())
                .filter(|repo| !repo.is_empty())
                .map(str::to_string)
        })
        .await
    }

    /// The starred models (`Update::Favorites`) as the settings hold them.
    pub(crate) async fn publish_favorites(&self) {
        self.publish(Update::Favorites(self.favorites().await));
    }

    /// The providers (`Update::Providers`): the list, or the text today's
    /// server answers `GET /api/config` with when the read fails (the page's
    /// `loadConfig` shows it as a notice beside `Models failed to load.`).
    /// A failed read is that one update and no `Update::Error` beside it,
    /// wherever the providers are published; the text also goes to the
    /// log. `Command::ReadProviders` is the read alone (the page's `Try
    /// again`).
    pub(crate) async fn publish_providers(&self) {
        let list = self.provider_list().await.map_err(|error| {
            eprintln!("[providers] {}", error.message);
            error.message
        });
        self.publish(Update::Providers(list));
    }

    /// `GET /api/config`'s `providers`, typed, in `PROVIDERS`' order, Cursor
    /// left out (it is not in the native app).
    pub async fn provider_list(&self) -> Result<Vec<Provider>, Refusal> {
        self.blocking(|core| {
            let providers = &core.0.providers;
            let public = settings::public_settings(providers, &providers.settings())
                .map_err(|e| refusal(500, e))?;
            let strings = |value: &Json| -> Vec<String> {
                value
                    .as_array()
                    .expect("a list of model ids")
                    .iter()
                    .map(|id| id.as_str().expect("a model id").to_string())
                    .collect()
            };
            Ok(mcbench_store::PROVIDERS
                .iter()
                .filter(|info| info.id != "cursor")
                .map(|info| {
                    let entry = &public["providers"][info.id];
                    Provider {
                        id: info.id.to_string(),
                        label: info.label.to_string(),
                        connected: entry["connected"].as_bool().expect("connected"),
                        auth: match info.auth.unwrap_or("api-key") {
                            "api-key" => Auth::ApiKey,
                            "cli-login" => Auth::CliLogin,
                            other => panic!("{} signs in by {other}", info.id),
                        },
                        harness: info.harness,
                        enabled: entry["enabled"].as_bool().expect("enabled"),
                        hidden_models: strings(&entry["hiddenModels"]),
                        model_order: strings(&entry["modelOrder"]),
                    }
                })
                .collect())
        })
        .await
    }
}
