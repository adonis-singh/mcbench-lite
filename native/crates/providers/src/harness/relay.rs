//! One harness run relayed to the turn loop (`harness.js` `RelaySession`).
//!
//! The harness side calls [`Relay::call`] for each tool call and the
//! `update`-like methods as it streams; the turn loop's side waits with
//! [`Relay::next`] for the following completion and [`Relay::deliver`]s the
//! stored tool result.
//!
//! States: running (the harness works; its tool calls queue and surface one
//! at a time as pending), ended (the harness reported its outcome, which the
//! next completion collects) and closed (final: the turn is over or was
//! stopped). Harness events outside running are dropped.
//!
//! A relay is plain state: its owner (one provider's core, behind one lock,
//! as Node's single thread holds it) runs what closing needs besides the
//! state (stopping the CLI) once [`Relay::take_close`] reports a close.

use std::collections::VecDeque;

use serde_json::{Map, Value, json};
use tokio::sync::{mpsc, oneshot};

use super::text;
use crate::Delta;

/// What a call is answered with: MCP content blocks, or why it is not.
pub(crate) type CallAnswer = Result<Vec<Value>, String>;

struct Call {
    id: String,
    name: String,
    args: Value,
    reply: oneshot::Sender<CallAnswer>,
    serial: u64,
}

/// What a waiting completion reads its deltas from.
pub(crate) type Deltas = mpsc::UnboundedReceiver<Delta>;
/// What a waiting completion reads its message (or error) from.
pub(crate) type Answer = oneshot::Receiver<Result<Map<String, Value>, String>>;

/// A completion waiting for the relay: its deltas and its message.
pub(crate) struct Waiter {
    deltas: Option<mpsc::UnboundedSender<Delta>>,
    done: oneshot::Sender<Result<Map<String, Value>, String>>,
}

impl Waiter {
    /// A waiter, and what its completion reads.
    pub(crate) fn new(with_deltas: bool) -> (Self, Deltas, Answer) {
        let (send, deltas) = mpsc::unbounded_channel();
        let (done_send, done) = oneshot::channel();
        (
            Waiter {
                deltas: with_deltas.then_some(send),
                done: done_send,
            },
            deltas,
            done,
        )
    }

    fn delta(&self, delta: Delta) {
        if let Some(deltas) = &self.deltas {
            let _ = deltas.send(delta);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Running,
    Ended,
    Closed,
}

struct Draft {
    index: u32,
    name: String,
    args: String,
    sent: Option<usize>,
}

/// A relayed run (`RelaySession`).
pub(crate) struct Relay {
    name: &'static str,
    pub(crate) state: State,
    calls: VecDeque<Call>,
    pending: Option<Call>,
    waiter: Option<Waiter>,
    pub(crate) text: String,
    pub(crate) reasoning: String,
    usage: Option<Value>,
    notices: Vec<Value>,
    plan: Option<Value>,
    rate_limit: Option<Value>,
    context_tokens: Option<Value>,
    drafts: Vec<(String, Draft)>,
    outcome: Option<Option<String>>,
    reason: Option<String>,
    close: Option<bool>,
    serial: u64,
}

impl Relay {
    /// A relay of the harness `name`; none can be made on a stopped turn
    /// (`signal.throwIfAborted()`).
    pub(crate) fn new(name: &'static str, aborted: Option<&str>) -> Result<Self, String> {
        if let Some(reason) = aborted {
            return Err(reason.to_owned());
        }
        Ok(Relay {
            name,
            state: State::Running,
            calls: VecDeque::new(),
            pending: None,
            waiter: None,
            text: String::new(),
            reasoning: String::new(),
            usage: None,
            notices: Vec::new(),
            plan: None,
            rate_limit: None,
            context_tokens: None,
            drafts: Vec::new(),
            outcome: None,
            reason: None,
            close: None,
            serial: 0,
        })
    }

    /// `closeReason`.
    pub(crate) fn close_reason(&self) -> String {
        self.reason
            .clone()
            .unwrap_or_else(|| format!("{}{}", self.name, text::RUN_CLOSED))
    }

    /// The pending call's id.
    pub(crate) fn pending_id(&self) -> Option<&str> {
        self.pending.as_ref().map(|c| c.id.as_str())
    }

    /// The run's outcome error once ended (`outcome.error`).
    pub(crate) fn outcome_error(&self) -> Option<&str> {
        self.outcome.as_ref().and_then(|e| e.as_deref())
    }

    /// A tool call from the harness: answered once the turn delivers its
    /// result, or refused at once when the run is not running. The serial
    /// names it to [`Relay::cancel_call`].
    pub(crate) fn call(
        &mut self,
        id: &str,
        name: &str,
        args: Value,
    ) -> (oneshot::Receiver<CallAnswer>, u64) {
        let (reply, answer) = oneshot::channel();
        self.serial += 1;
        let serial = self.serial;
        if self.state != State::Running {
            let _ = reply.send(Err(text::TURN_ENDED.to_owned()));
            return (answer, serial);
        }
        self.calls.push_back(Call {
            id: id.to_owned(),
            name: name.to_owned(),
            args,
            reply,
            serial,
        });
        self.pump();
        (answer, serial)
    }

    /// The call's MCP request was cancelled: a queued call never runs, and
    /// a running one's result is dropped.
    pub(crate) fn cancel_call(&mut self, id: &str, serial: u64) {
        self.calls.retain(|c| c.serial != serial);
        self.drafts.retain(|(draft, _)| draft != id);
    }

    /// `update({ text, reasoning, usage })`.
    pub(crate) fn update(
        &mut self,
        text: Option<&str>,
        reasoning: Option<&str>,
        usage: Option<Value>,
    ) {
        if self.state != State::Running {
            return;
        }
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            self.text.push_str(text);
            if let Some(w) = &self.waiter {
                w.delta(Delta::Content(text.to_owned()));
            }
        }
        if let Some(reasoning) = reasoning.filter(|t| !t.is_empty()) {
            self.reasoning.push_str(reasoning);
            if let Some(w) = &self.waiter {
                w.delta(Delta::Reasoning(reasoning.to_owned()));
            }
        }
        if let Some(usage) = usage.filter(|u| !u.is_null()) {
            self.usage = Some(usage);
        }
    }

    /// A tool call whose arguments the model is still writing.
    pub(crate) fn draft_tool_call(&mut self, id: &str, index: u32, name: &str) {
        if self.state != State::Running {
            return;
        }
        let draft = Draft {
            index,
            name: name.to_owned(),
            args: String::new(),
            sent: None,
        };
        match self.drafts.iter_mut().find(|(d, _)| d == id) {
            Some(slot) => slot.1 = draft,
            None => self.drafts.push((id.to_owned(), draft)),
        }
        self.flush_drafts();
    }

    /// More of a draft's arguments.
    pub(crate) fn draft_args(&mut self, id: &str, text: &str) -> Result<(), String> {
        if self.state != State::Running {
            return Ok(());
        }
        let Some((_, draft)) = self.drafts.iter_mut().find(|(d, _)| d == id) else {
            return Err("Cannot read properties of undefined (reading 'args')".to_owned());
        };
        draft.args.push_str(text);
        self.flush_drafts();
        Ok(())
    }

    /// Argument text written while no completion was waiting goes out once
    /// one is.
    fn flush_drafts(&mut self) {
        let Some(waiter) = &self.waiter else { return };
        if waiter.deltas.is_none() {
            return;
        }
        for (id, draft) in &mut self.drafts {
            let length = draft.args.encode_utf16().count();
            if draft.sent == Some(length) {
                continue;
            }
            let from = draft.sent.unwrap_or(0);
            let args: String = String::from_utf16_lossy(
                &draft.args.encode_utf16().skip(from).collect::<Vec<u16>>(),
            );
            waiter.delta(Delta::ToolCall {
                index: draft.index,
                id: id.clone(),
                name: draft.name.clone(),
                arguments: args,
            });
            draft.sent = Some(length);
        }
    }

    /// A transient activity line, or none.
    pub(crate) fn status(&mut self, text: Option<String>) {
        if self.state != State::Running {
            return;
        }
        if let Some(w) = &self.waiter {
            w.delta(Delta::Status(text));
        }
    }

    /// A notice; an empty one is dropped.
    pub(crate) fn notice(&mut self, level: &str, text: Option<&Value>) {
        if self.state != State::Running || !crate::bridge::truthy(text) {
            return;
        }
        let text = text.unwrap().clone();
        let mut entry = Map::new();
        entry.insert("level".to_owned(), json!(level));
        entry.insert("text".to_owned(), text.clone());
        self.notices.push(Value::Object(entry));
        if let Some(w) = &self.waiter {
            w.delta(Delta::Notice {
                level: level.to_owned(),
                text: crate::bridge::js_string(&text),
            });
        }
    }

    /// The harness's task list.
    pub(crate) fn set_plan(&mut self, items: Value) {
        if self.state != State::Running {
            return;
        }
        self.plan = Some(items.clone());
        if let Some(w) = &self.waiter {
            w.delta(Delta::Plan(items));
        }
    }

    /// Usage-limit state.
    pub(crate) fn set_rate_limit(&mut self, info: Option<Value>) {
        if self.state == State::Running {
            self.rate_limit = info;
        }
    }

    /// How much context the latest request used.
    pub(crate) fn set_context_tokens(&mut self, tokens: Value) {
        if self.state == State::Running {
            self.context_tokens = Some(tokens);
        }
    }

    /// The model is under way (`onStarted`).
    pub(crate) fn started(&mut self) {
        if let Some(w) = &self.waiter {
            w.delta(Delta::Started);
        }
    }

    /// The harness run ended. `save` records its session and runs only for
    /// a turn that completed normally; a save that fails fails the turn.
    pub(crate) fn finish(
        &mut self,
        error: Option<String>,
        save: Option<&mut dyn FnMut() -> Result<(), String>>,
    ) {
        if self.state != State::Running {
            return;
        }
        let mut failure = error;
        if failure.is_none() && (self.pending.is_some() || !self.calls.is_empty()) {
            failure = Some(format!("{}{}", self.name, text::UNANSWERED));
        }
        if failure.is_none()
            && let Some(save) = save
            && let Err(e) = save()
        {
            failure = Some(e);
        }
        self.state = State::Ended;
        self.outcome = Some(failure);
        // The harness is gone: its calls get no answer. A pending call
        // stays known so that delivering its result reports how the run
        // ended.
        if let Some(pending) = self.pending.take() {
            let (reply, _) = oneshot::channel();
            let Call {
                id,
                name,
                args,
                reply: answer,
                serial,
            } = pending;
            let _ = answer.send(Err(text::TURN_ENDED.to_owned()));
            self.pending = Some(Call {
                id,
                name,
                args,
                reply,
                serial,
            });
        }
        for call in self.calls.drain(..) {
            let _ = call.reply.send(Err(text::TURN_ENDED.to_owned()));
        }
        self.pump();
    }

    /// Delivers the turn's result for the pending call. A run that is not
    /// running is closed, and its outcome (or the close's reason) returned.
    pub(crate) fn deliver(&mut self, content: Vec<Value>) -> Result<(), String> {
        if self.state != State::Running {
            let error = if self.state == State::Ended {
                self.outcome_error().unwrap_or_default().to_owned()
            } else {
                self.close_reason()
            };
            self.close();
            return Err(error);
        }
        let call = self.pending.take().expect("a pending call to deliver to");
        let _ = call.reply.send(Ok(content));
        Ok(())
    }

    /// Waits for the next completion's message.
    pub(crate) fn next(&mut self, waiter: Waiter) {
        self.waiter = Some(waiter);
        self.pump();
    }

    fn message(&mut self) -> Map<String, Value> {
        let mut entry = Map::new();
        entry.insert("role".to_owned(), json!("assistant"));
        entry.insert("content".to_owned(), json!(std::mem::take(&mut self.text)));
        let reasoning = std::mem::take(&mut self.reasoning);
        if !reasoning.is_empty() {
            entry.insert("reasoning".to_owned(), json!(reasoning));
        }
        if !self.notices.is_empty() {
            entry.insert(
                "notices".to_owned(),
                Value::Array(std::mem::take(&mut self.notices)),
            );
        }
        if let Some(plan) = self.plan.take() {
            entry.insert("plan".to_owned(), plan);
        }
        if let Some(rate_limit) = &self.rate_limit
            && crate::bridge::truthy(Some(rate_limit))
        {
            entry.insert("rateLimit".to_owned(), rate_limit.clone());
        }
        if let Some(tokens) = &self.context_tokens {
            entry.insert("contextTokens".to_owned(), tokens.clone());
        }
        entry
    }

    /// Answers the waiting completion, if the state has something for it.
    fn pump(&mut self) {
        let Some(waiter) = self.waiter.take() else {
            return;
        };
        if self.state == State::Closed {
            let _ = waiter.done.send(Err(self.close_reason()));
            return;
        }
        self.waiter = Some(waiter);
        self.flush_drafts();
        let waiter = self.waiter.take().unwrap();
        if self.state == State::Running {
            let Some(call) = self.calls.pop_front() else {
                self.waiter = Some(waiter);
                return;
            };
            let args = mcbench_json::stringify(&call.args);
            let drafted = self.drafts.iter().position(|(d, _)| *d == call.id);
            match drafted {
                Some(i) => {
                    self.drafts.remove(i);
                }
                None => waiter.delta(Delta::ToolCall {
                    index: 0,
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: args.clone(),
                }),
            }
            let mut message = self.message();
            message.insert("finish_reason".to_owned(), json!("tool_calls"));
            message.insert(
                "tool_calls".to_owned(),
                json!([{ "id": call.id, "type": "function", "function": { "name": call.name, "arguments": args } }]),
            );
            self.pending = Some(call);
            let _ = waiter.done.send(Ok(message));
            return;
        }
        let error = self.outcome_error().map(str::to_owned);
        let mut message = self.message();
        message.insert("finish_reason".to_owned(), json!("stop"));
        if let Some(usage) = &self.usage {
            message.insert("usage".to_owned(), usage.clone());
        }
        self.close();
        let _ = waiter.done.send(match error {
            Some(error) => Err(error),
            None => Ok(message),
        });
    }

    /// Closes the run; its owner stops the harness afterwards when
    /// [`Relay::take_close`] says it was running.
    pub(crate) fn close(&mut self) {
        if self.state == State::Closed {
            return;
        }
        let running = self.state == State::Running;
        self.state = State::Closed;
        for call in self.pending.take().into_iter().chain(self.calls.drain(..)) {
            let _ = call.reply.send(Err(text::TURN_ENDED.to_owned()));
        }
        self.drafts.clear();
        self.close = Some(running);
        self.pump();
    }

    /// Closes the run because the turn was stopped, with the stop's reason.
    pub(crate) fn abort(&mut self, reason: &str) {
        if self.state == State::Closed {
            return;
        }
        self.reason = Some(reason.to_owned());
        self.close();
    }

    /// Once after a close: whether the run was running then (and so its
    /// harness must be stopped).
    pub(crate) fn take_close(&mut self) -> Option<bool> {
        self.close.take()
    }
}
