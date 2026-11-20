//! A turn's updates to the window, from ONE sink per turn ([`TurnUpdates`])
//! that every event of the turn passes through under the turn stream's
//! lock, and the join of a running turn placed between two of its events
//! under that same lock (`TurnStream::with_events`). What the window relies
//! on (agreed with the interface's owner, 2026-11-17):
//!
//! A turn's events go to the window at all times: the sink a send attaches
//! (`Core::chat`), or for a turn no request started (a bench round) the one
//! the lifecycle attaches before its first event (`Services::turn_sink`).
//! A send's turn says `Update::TurnJoined { chat, since }` before its first
//! event.
//!
//! `Command::JoinTurn` while a turn runs: `Update::TurnJoined`, then the
//! chat's `Update::Transcript` as it was at the turn's snapshot (kept when
//! the snapshot passed), then every event so far in order, all while no
//! event of the turn can be pushed; then the live events, each once. A join
//! before the turn's first event is said with that event. A turn that has
//! published its last event (`Done` or `Error`) is not running for a join:
//! `Update::TurnNotRunning`.
//!
//! A turn's end: `TurnEvent::Done` (or `Error`), then the chat's
//! `Update::Transcript` (with no running turn), then lists in which the
//! chat runs no turn: from the moment that transcript is published the
//! turn counts as ended for every list and transcript the core reads
//! ([`Core::running_since`]), though the lifecycle still holds the chat
//! while it closes the turn.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use mcbench_agent::lifecycle::TurnStream;
use mcbench_agent::{TurnEvent, TurnSink};
use mcbench_ui::Update;

use super::views::{self, Transcript};
use super::{Core, Inner};

/// When a turn's sink says the turn was joined.
pub(crate) enum Join {
    /// It has no marker of its own to say (a bench round's turn), or said it.
    Said,
    /// Before its first event, the turn's start read then (a send's: the
    /// turn holds its chat while it emits).
    Held,
}

/// What the core keeps of the turns the window sees.
#[derive(Default)]
pub(crate) struct TurnViews {
    /// A join made before a turn's first event: its start, said with that
    /// event.
    joins: HashMap<String, i64>,
    /// Each chat's transcript at its turn's snapshot.
    snapshots: HashMap<String, Transcript>,
    /// The turns whose last event is published, while the lifecycle still
    /// holds them.
    ended: HashMap<String, Arc<TurnStream>>,
}

/// A turn's events to the window (see the module's doc).
pub(crate) struct TurnUpdates {
    pub(crate) core: Weak<Inner>,
    pub(crate) chat: String,
    pub(crate) join: Join,
}

impl TurnSink for TurnUpdates {
    fn emit(&mut self, event: TurnEvent) {
        let Some(inner) = self.core.upgrade() else {
            return;
        };
        let core = Core(inner);
        let chat = self.chat.clone();
        let own = matches!(std::mem::replace(&mut self.join, Join::Said), Join::Held);
        let pending = core.views().joins.remove(&chat);
        if own || pending.is_some() {
            let since = core
                .0
                .turns
                .turn(&chat)
                .expect("a turn emits while it holds its chat")
                .started_at();
            core.publish(Update::TurnJoined {
                chat: chat.clone(),
                since,
            });
        }
        match &event {
            TurnEvent::ToolResult { name, .. } if name == "prompt_library" => {
                let library = core.clone();
                core.0
                    .handle
                    .spawn(async move { library.publish_prompts().await });
            }
            // The prompt is stored: the transcript goes out before the
            // snapshot's own update, and is kept for a later join; read off
            // the worker's async duty (`block_in_place`) because a sink is
            // called synchronously.
            TurnEvent::Snapshot => {
                match tokio::task::block_in_place(|| core.transcript_now(&chat)) {
                    Ok(transcript) => {
                        core.views()
                            .snapshots
                            .insert(chat.clone(), transcript.clone());
                        core.publish(transcript.into_update());
                    }
                    Err(refused) => core.publish(Update::Error(refused.message)),
                }
            }
            TurnEvent::WorldChanged => core.0.live.changed(&core, &chat),
            TurnEvent::Done | TurnEvent::Error { .. } => {
                let stream = core
                    .0
                    .turns
                    .turn(&chat)
                    .expect("a turn's last event comes while it holds its chat");
                core.publish(Update::Turn {
                    chat: chat.clone(),
                    event,
                });
                // The transcript with no running turn, and from then on the
                // turn ended for every read: under the lists' order, so a
                // list is read and published wholly before it (the turn
                // running) or wholly after (ended).
                tokio::task::block_in_place(|| {
                    let _order = core.0.list_order.lock().expect("the lists' order");
                    let made = views::transcript(&core.0.store, &core.0.turns, &chat);
                    match made {
                        Ok(mut transcript) => {
                            transcript.header.running_since = None;
                            core.publish(transcript.into_update());
                        }
                        Err(refused) => core.publish(Update::TranscriptFailed {
                            chat: chat.clone(),
                            error: refused.message,
                        }),
                    }
                    core.views().ended.insert(chat.clone(), Arc::clone(&stream));
                });
                let after = core.clone();
                core.0.handle.spawn(async move {
                    stream.done().await;
                    {
                        let mut views = after.views();
                        views.snapshots.remove(&chat);
                        if views
                            .ended
                            .get(&chat)
                            .is_some_and(|ended| Arc::ptr_eq(ended, &stream))
                        {
                            views.ended.remove(&chat);
                        }
                    }
                    after.list_changed();
                    after.0.live.changed(&after, &chat);
                });
                return;
            }
            _ => {}
        }
        core.publish(Update::Turn { chat, event });
    }
}

impl Core {
    pub(crate) fn views(&self) -> std::sync::MutexGuard<'_, TurnViews> {
        self.0.turn_views.lock().expect("the turns' views")
    }

    /// When chat `id`'s running turn started, or none: a turn whose last
    /// event is published runs no more.
    pub(crate) fn running_since(&self, id: &str) -> Option<i64> {
        let turn = self.0.turns.turn(id)?;
        let ended = self
            .views()
            .ended
            .get(id)
            .is_some_and(|ended| Arc::ptr_eq(ended, &turn));
        (!ended).then(|| turn.started_at())
    }

    /// The chat list now with each row's running turn as
    /// [`Core::running_since`] gives it (blocking: the store).
    pub(crate) fn list_now(
        &self,
    ) -> Result<views::ConversationList, mcbench_agent::lifecycle::Refusal> {
        let mut list = views::list(&self.0.store, &self.0.turns)?;
        for row in &mut list.conversations {
            row.running_since = self.running_since(&row.summary.id.0);
        }
        Ok(list)
    }

    /// Publishes the chat list now (`Update::Conversations`), read and
    /// published under the lists' order (see the module's doc), then the
    /// details of the benches shown that changed (`bench.rs`).
    pub(crate) async fn publish_list_now(&self) {
        self.publish_lists_and(None).await
    }

    /// [`Core::publish_list_now`], with bench `also`'s details published
    /// after the list whether or not they changed.
    pub(crate) async fn publish_lists_and(&self, also: Option<&str>) {
        let also = also.map(str::to_string);
        self.blocking(move |core| {
            let _order = core.0.list_order.lock().expect("the lists' order");
            match core.list_now() {
                Ok(list) => {
                    core.publish(Update::Conversations(list));
                    core.publish_benches_held(also.as_deref());
                }
                Err(error) => {
                    core.publish(Update::Error(error.message));
                    if let Some(id) = &also {
                        core.publish_bench_held(id);
                    }
                }
            }
        })
        .await
    }

    /// Chat `id`'s transcript now (blocking: the store).
    pub(crate) fn transcript_now(
        &self,
        id: &str,
    ) -> Result<Transcript, mcbench_agent::lifecycle::Refusal> {
        let mut transcript = views::transcript(&self.0.store, &self.0.turns, id)?;
        transcript.header.running_since = self.running_since(id);
        Ok(transcript)
    }

    /// `Command::JoinTurn` (`GET .../stream`): see the module's doc.
    pub(crate) fn join_turn(&self, chat: &str) {
        let not_running = || Update::TurnNotRunning {
            chat: chat.to_string(),
        };
        let Some(turn) = self.0.turns.turn(chat) else {
            return self.publish(not_running());
        };
        let joined = turn.with_events(|events| {
            let mut views = self.views();
            if views
                .ended
                .get(chat)
                .is_some_and(|ended| Arc::ptr_eq(ended, &turn))
            {
                drop(views);
                return self.publish(not_running());
            }
            if events.is_empty() {
                views.joins.insert(chat.to_string(), turn.started_at());
                return;
            }
            let snapshot = views.snapshots.get(chat).cloned();
            drop(views);
            self.publish(Update::TurnJoined {
                chat: chat.to_string(),
                since: turn.started_at(),
            });
            for event in events {
                if matches!(event, TurnEvent::Snapshot) {
                    let snapshot = snapshot
                        .clone()
                        .expect("a turn's snapshot is kept when it passes");
                    self.publish(snapshot.into_update());
                }
                self.publish(Update::Turn {
                    chat: chat.to_string(),
                    event: event.clone(),
                });
            }
        });
        if joined.is_err() {
            self.publish(not_running());
        }
    }
}
