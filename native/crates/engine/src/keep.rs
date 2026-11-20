//! What is kept of each conversation's settles, the port of
//! `server/shot-world.js` `settle` and its `kept` map: the settles of a
//! conversation's latest two worlds, by ticks, the running ones and the
//! done ones, and the conversation's latest settled world (what the next
//! settle's palette and unchanged chunks are taken from). The settled
//! value `R` is the caller's (`shot-world.js` keeps a `World` made from the
//! blueprint, which is `mcbench-world`'s), and so is the settle's run.

use crate::EngineError;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

/// The latest worlds kept of a conversation (a build settles its candidate
/// before it is applied, and if it is not, the world it started from is
/// still the conversation's).
pub const KEPT_WORLDS: usize = 2;
/// The conversations settled most recently stay, so a long-running app does
/// not hold the settled world of every conversation it ever built.
pub const KEPT_CONVERSATIONS: usize = 16;

/// Which world a settle is of: the world (by an identity of the caller's,
/// `shot-world.js` compares the object) at a revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SettlesKey {
    /// The world's identity.
    pub world: u64,
    /// Its revision: a world changed since is settled again.
    pub revision: u64,
}

type Pending<R> = watch::Receiver<Option<Result<R, EngineError>>>;

struct Entry<R> {
    key: SettlesKey,
    pending: HashMap<i32, Pending<R>>,
    done: HashMap<i32, R>,
}

struct Chat<R> {
    worlds: Vec<Arc<Mutex<Entry<R>>>>,
    latest: Arc<Mutex<Option<R>>>,
}

struct Kept<R> {
    // Oldest first.
    order: Vec<String>,
    chats: HashMap<String, Arc<Mutex<Chat<R>>>>,
}

/// The settles kept, by conversation. Cloned handles share them.
pub struct Settles<R> {
    kept: Arc<Mutex<Kept<R>>>,
}

impl<R> Clone for Settles<R> {
    fn clone(&self) -> Self {
        Settles {
            kept: self.kept.clone(),
        }
    }
}

impl<R: Clone + Send + Sync + 'static> Default for Settles<R> {
    fn default() -> Self {
        Settles::new()
    }
}

/// `shot-world.js` `checkTicks`: a number of ticks within what `test_build`
/// may simulate (`max`, the operator's `MCBENCH_VERIFY_MAX_SIMULATION_TICKS`).
pub fn check_ticks(value: i64, label: &str, max: i64) -> Result<i32, String> {
    if value < 0 {
        return Err(format!(
            "{label} must be a whole number of ticks, 0 or more"
        ));
    }
    if value > max {
        return Err(format!(
            "{label} is {value} ticks; the budget of simulated ticks, the same as test_build's, is {max} (MCBENCH_VERIFY_MAX_SIMULATION_TICKS)"
        ));
    }
    Ok(value as i32)
}

/// One tick's settle as [`Settles::settle`] answers it: the value, and
/// whether it was settled before this call.
#[derive(Clone, Debug)]
pub struct KeptSettle<R> {
    /// The ticks.
    pub ticks: i32,
    /// The settled value.
    pub value: R,
    /// Settled before this call.
    pub cached: bool,
}

impl<R: Clone + Send + Sync + 'static> Settles<R> {
    /// Nothing kept.
    pub fn new() -> Settles<R> {
        Settles {
            kept: Arc::new(Mutex::new(Kept {
                order: Vec::new(),
                chats: HashMap::new(),
            })),
        }
    }

    /// The settles of `world`, the conversation's current world, for each
    /// of `ticks` (checked by the caller with [`check_ticks`]): those made
    /// before are kept; the rest are made now in one call of `run`, which
    /// is given the conversation's latest settled value (it reads it and
    /// sets it, as `runSettle` does with `chat.latest`) and the ticks
    /// missing, sorted. What is kept is decided when `settle` is called, as
    /// `shot-world.js` decides it before its first `await`. A conversation
    /// dropped (more than [`KEPT_CONVERSATIONS`] were settled since) leaves
    /// with all of it, and a run of it that ends later writes only to what
    /// was dropped.
    pub fn settle<F, Fut>(
        &self,
        conversation: &str,
        world: SettlesKey,
        ticks: &[i32],
        run: F,
    ) -> impl Future<Output = Result<Vec<KeptSettle<R>>, EngineError>> + Send + 'static + use<R, F, Fut>
    where
        F: FnOnce(Arc<Mutex<Option<R>>>, Vec<i32>) -> Fut,
        Fut: Future<Output = Result<Vec<(i32, R)>, EngineError>> + Send + 'static,
    {
        let mut wanted = ticks.to_vec();
        wanted.sort();
        wanted.dedup();
        let mut kept = self.kept.lock().unwrap();
        let chat = kept.chats.get(conversation).cloned().unwrap_or_else(|| {
            Arc::new(Mutex::new(Chat {
                worlds: Vec::new(),
                latest: Arc::new(Mutex::new(None)),
            }))
        });
        let mut chat_state = chat.lock().unwrap();
        let entry = chat_state
            .worlds
            .iter()
            .find(|entry| entry.lock().unwrap().key == world)
            .cloned()
            .unwrap_or_else(|| {
                Arc::new(Mutex::new(Entry {
                    key: world,
                    pending: HashMap::new(),
                    done: HashMap::new(),
                }))
            });
        let missing: Vec<i32> = {
            let entry = entry.lock().unwrap();
            wanted
                .iter()
                .copied()
                .filter(|tick| !entry.pending.contains_key(tick) && !entry.done.contains_key(tick))
                .collect()
        };
        if !missing.is_empty() {
            let running = run(chat_state.latest.clone(), missing.clone());
            let mut senders = Vec::new();
            {
                let mut entry = entry.lock().unwrap();
                for &tick in &missing {
                    let (sender, receiver) = watch::channel(None);
                    entry.pending.insert(tick, receiver);
                    senders.push((tick, sender));
                }
            }
            let (chat, entry) = (chat.clone(), entry.clone());
            tokio::spawn(async move {
                let result = running.await;
                let mut chat = chat.lock().unwrap();
                let mut kept_entry = entry.lock().unwrap();
                for (tick, sender) in senders {
                    kept_entry.pending.remove(&tick);
                    let answer = match &result {
                        Ok(values) => {
                            let value = values
                                .iter()
                                .find(|(at, _)| *at == tick)
                                .map(|(_, value)| value.clone())
                                .ok_or_else(|| {
                                    EngineError::Stopped(format!(
                                        "The settle gave nothing for {tick} ticks"
                                    ))
                                });
                            if let Ok(value) = &value {
                                kept_entry.done.insert(tick, value.clone());
                            }
                            value
                        }
                        Err(error) => Err(error.clone()),
                    };
                    // A world that could not be settled is not kept.
                    if answer.is_err()
                        && kept_entry.pending.is_empty()
                        && kept_entry.done.is_empty()
                    {
                        chat.worlds.retain(|other| !Arc::ptr_eq(other, &entry));
                    }
                    let _ = sender.send(Some(answer));
                }
            });
        }
        chat_state
            .worlds
            .retain(|other| !Arc::ptr_eq(other, &entry));
        chat_state.worlds.insert(0, entry.clone());
        chat_state.worlds.truncate(KEPT_WORLDS);
        drop(chat_state);
        kept.order.retain(|other| other != conversation);
        kept.order.push(conversation.to_string());
        kept.chats.insert(conversation.to_string(), chat);
        while kept.order.len() > KEPT_CONVERSATIONS {
            let oldest = kept.order.remove(0);
            kept.chats.remove(&oldest);
        }
        drop(kept);
        let asked: Vec<(i32, Result<R, Pending<R>>)> = {
            let entry = entry.lock().unwrap();
            wanted
                .iter()
                .map(|&tick| match entry.done.get(&tick) {
                    Some(value) => (tick, Ok(value.clone())),
                    None => (tick, Err(entry.pending[&tick].clone())),
                })
                .collect()
        };
        async move {
            let mut out = Vec::new();
            for (tick, answer) in asked {
                let value = match answer {
                    Ok(value) => value,
                    Err(mut pending) => {
                        let answered = pending
                            .wait_for(Option::is_some)
                            .await
                            .map_err(|_| EngineError::Stopped("The settle stopped".into()))?;
                        answered.clone().unwrap()?
                    }
                };
                out.push(KeptSettle {
                    ticks: tick,
                    value,
                    cached: !missing.contains(&tick),
                });
            }
            Ok(out)
        }
    }

    /// The settle of `world` after `tick` ticks done for the conversation
    /// (`shotWorld`, `shotEntities`), or `None` when that was not settled or
    /// the conversation was dropped.
    pub fn done(&self, conversation: &str, world: SettlesKey, tick: i32) -> Option<R> {
        let kept = self.kept.lock().unwrap();
        let chat = kept.chats.get(conversation)?.lock().unwrap();
        let entry = chat
            .worlds
            .iter()
            .find(|entry| entry.lock().unwrap().key == world)?
            .lock()
            .unwrap();
        entry.done.get(&tick).cloned()
    }
}
