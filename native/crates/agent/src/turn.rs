//! One user turn (`server/agent.js` `runTurn`) and one tool call
//! (`executeToolCall`).

use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use mcbench_providers::api::{js, v8json};
use mcbench_providers::harness::TranscriptEntry;
use mcbench_providers::retry;
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, Outcome, Provider, ProviderError, ProviderId,
};
use mcbench_store::{
    Conversation, FunctionCall, LiveConversation, NewMessage, Notice, PlanItem, Reply, Store,
    ToolCall, ToolTelemetry, iso,
};
use mcbench_tools::verify::Budgets;
use mcbench_tools::{
    Chat, Progress, ToolError, ToolOutcome, Tools, Workspace, call_of, message_content,
};

use crate::api::{self, History};
use crate::context::{Archives, WorkFiles};
use crate::contract::{self, Form, Placement};
use crate::events::{ProgressShown, TurnEvent, TurnSink};

/// A turn's sink, shared by the callbacks of one completion.
pub type SharedSink = Arc<Mutex<dyn TurnSink>>;

/// What a turn runs with: owned handles, since a turn is a task of its own.
#[derive(Clone)]
pub struct TurnSetup {
    /// The store.
    pub store: Arc<Store>,
    /// The chat, as loaded.
    pub conversation: LiveConversation,
    /// The chat's provider.
    pub provider: Arc<dyn Provider>,
    /// The tools a call is dispatched to, by name.
    pub tools: Tools,
    /// A chat turn (the prompt library is offered) rather than a bench's or
    /// the benchmark's (`runTurn`'s `chat`).
    pub chat: bool,
    /// `test_build`'s placement mode (`minecraftVerifier.placementMode`).
    pub placement: Placement,
    /// Whether `bash` has its networked command (`sandbox.networkAccess()`).
    pub network_access: bool,
    /// The verifier's budgets the schemas quote.
    pub budgets: Budgets,
    /// `MCBENCH_CONTEXT_MAX_BYTES`.
    pub max_context_bytes: usize,
    /// `keepExchanges`.
    pub keep_exchanges: usize,
    /// The archives this process wrote.
    pub archives: Arc<Archives>,
    /// Where archives go.
    pub work: Arc<dyn WorkFiles>,
}

/// Why a turn failed: what `executeTurn` reads of the thrown error
/// (`message`, `telemetry`, `reply`).
#[derive(Clone, Debug, PartialEq)]
pub struct TurnError {
    /// `String(err.message || err)`.
    pub message: String,
    /// `err.telemetry`: a failed completion's.
    pub telemetry: Option<Box<mcbench_store::Telemetry>>,
    /// `err.reply`: the text that had streamed (`''` when none).
    pub reply: String,
}

impl TurnError {
    fn plain(message: impl Into<String>) -> TurnError {
        TurnError {
            message: message.into(),
            telemetry: None,
            reply: String::new(),
        }
    }
}

impl From<String> for TurnError {
    fn from(message: String) -> TurnError {
        TurnError::plain(message)
    }
}

fn count(value: Option<f64>) -> Result<Option<u64>, String> {
    match value {
        None => Ok(None),
        Some(x) if x >= 0.0 && x.fract() == 0.0 && x <= 9_007_199_254_740_991.0 => {
            Ok(Some(x as u64))
        }
        Some(x) => Err(format!("A token count of {x} is not a whole number")),
    }
}

/// The provider's telemetry as the store holds it.
pub fn stored_telemetry(
    t: &mcbench_providers::Telemetry,
) -> Result<mcbench_store::Telemetry, String> {
    let usage = match &t.usage {
        None => None,
        Some(u) => Some(mcbench_store::Usage {
            input_tokens: count(u.input_tokens)?,
            output_tokens: count(u.output_tokens)?,
            cached_input_tokens: count(u.cached_input_tokens)?,
            cache_write_tokens: count(u.cache_write_tokens)?,
            reasoning_tokens: count(u.reasoning_tokens)?,
        }),
    };
    let (context_tokens, failed, cancelled) = match t.outcome {
        Outcome::Completed { context_tokens } => (Some(count(context_tokens)?), None, None),
        Outcome::Failed { cancelled } => (None, Some(true), Some(cancelled)),
    };
    Ok(mcbench_store::Telemetry {
        provider: t.provider.key().into(),
        model: t.model.clone(),
        effort: t.effort.clone(),
        started_at: t.started_at.clone(),
        wall_ms: t.wall_ms,
        first_delta_ms: t.first_delta_ms,
        context_bytes: t.context_bytes,
        message_count: t.message_count,
        usage,
        context_tokens,
        failed,
        cancelled,
    })
}

fn provider_error(error: ProviderError) -> TurnError {
    let telemetry = match error.telemetry.as_deref().map(stored_telemetry) {
        Some(Ok(telemetry)) => Some(Box::new(telemetry)),
        Some(Err(message)) => return TurnError::plain(message),
        None => None,
    };
    TurnError {
        message: error.message,
        telemetry,
        reply: error.reply,
    }
}

/// The reply `agent.js` 662-673 stores.
pub fn reply_of(message: &AssistantMessage) -> Result<Reply, String> {
    let decode = |value: &Value, what: &str| -> Result<Value, String> {
        let _ = what;
        Ok(value.clone())
    };
    let notices = match &message.notices {
        None => None,
        Some(items) => Some(
            items
                .iter()
                .map(|n| {
                    serde_json::from_value::<Notice>(decode(n, "notice")?)
                        .map_err(|e| format!("a harness notice: {e}"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    };
    let plan = match &message.plan {
        None => None,
        Some(plan) => Some(
            serde_json::from_value::<Vec<PlanItem>>(plan.clone())
                .map_err(|e| format!("a harness plan: {e}"))?,
        ),
    };
    Ok(Reply {
        content: message.content.clone(),
        reasoning: message.reasoning.clone(),
        reasoning_details: message.reasoning_details.clone(),
        finish_reason: message.finish_reason.clone(),
        telemetry: Some(stored_telemetry(&message.telemetry)?),
        notices,
        plan,
        rate_limit: message.rate_limit.clone(),
        tool_calls: message
            .tool_calls
            .iter()
            .map(|call| ToolCall {
                id: call.id.clone(),
                kind: call.kind.clone(),
                function: FunctionCall {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                },
                extra_content: call.extra_content.clone(),
            })
            .collect(),
    })
}

/// Runs `f` on the store's thread pool with the chat locked.
async fn with_chat<T: Send + 'static>(
    setup: &TurnSetup,
    f: impl FnOnce(&Store, &mut Conversation) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let store = Arc::clone(&setup.store);
    let live = Arc::clone(&setup.conversation);
    tokio::task::spawn_blocking(move || {
        let mut conversation = live.lock().expect("a chat's lock");
        f(&store, &mut conversation)
    })
    .await
    .expect("a store task does not panic")
}

fn emit(sink: &SharedSink, event: TurnEvent) {
    sink.lock().expect("a turn sink's lock").emit(event);
}

fn refusal(error: String) -> Value {
    let mut out = Map::new();
    out.insert("ok".into(), Value::Bool(false));
    out.insert("error".into(), Value::String(error));
    Value::Object(out)
}

/// `see_build` and `view_image` (`VISION_TOOLS`).
fn vision(name: &str) -> bool {
    matches!(name, "see_build" | "view_image")
}

/// `executeToolCall(conversation, name, args, { imageInput, signal, chat })`:
/// one parsed call dispatched to the tool of its name, or refused as today
/// ([`Tools::execute`]). An `Err` is a call that threw, which fails the
/// turn.
pub async fn execute_tool_call(
    tools: &Tools,
    chat: &mut Chat,
    name: &str,
    args: &Map<String, Value>,
    progress: &mut (dyn FnMut(Progress) + Send),
    cancel: &CancellationToken,
) -> Result<ToolOutcome, ToolError> {
    tools.execute(chat, name, args, progress, cancel).await
}

/// What a harness reads of the stored branch (`conversation` in
/// `runTurn`'s request: `harness.js` reads each message's id, role,
/// seeBuild, stopped and reply); an API provider ignores it.
pub fn transcript(messages: &[mcbench_store::StoredMessage]) -> Vec<TranscriptEntry> {
    messages
        .iter()
        .map(|stored| {
            let (role, see_build, stopped, reply) = match &stored.message {
                mcbench_store::Message::User(user) => ("user", user.see_build, false, None),
                mcbench_store::Message::Assistant(a) => {
                    ("assistant", false, a.stopped, a.reply.clone())
                }
                mcbench_store::Message::Tool(_) => ("tool", false, false, None),
                mcbench_store::Message::Error(_) => ("error", false, false, None),
            };
            TranscriptEntry {
                id: stored.id.0.clone(),
                role: role.into(),
                see_build,
                stopped,
                reply,
            }
        })
        .collect()
}

/// The request of one completion (`runTurn`'s `complete({...})`).
async fn request(setup: &TurnSetup, form: Form) -> Result<CompletionRequest, TurnError> {
    let contract = contract::of(form, &setup.budgets)?;
    let (archives, work) = (Arc::clone(&setup.archives), Arc::clone(&setup.work));
    let (max_bytes, keep) = (setup.max_context_bytes, setup.keep_exchanges);
    let system_prompt = contract.system_prompt;
    let image_input = form.image_input;
    let (id, model, effort, messages, transcript) = with_chat(setup, move |store, conversation| {
        let memory = api::in_memory(&conversation.messages)?;
        let messages = api::api_messages(
            store,
            &History {
                chat: &conversation.id.0,
                messages: &memory,
                max_bytes,
                keep_exchanges: keep,
            },
            image_input,
            &system_prompt,
            &archives,
            &*work,
        )?;
        Ok((
            conversation.id.0.clone(),
            conversation.model.clone(),
            conversation.effort.clone().filter(|e| !e.is_empty()),
            messages,
            transcript(&conversation.messages),
        ))
    })
    .await?;
    Ok(CompletionRequest {
        conversation_id: Some(id),
        model,
        effort,
        messages,
        tools: contract.tools,
        chat: setup.chat,
        transcript: Some(transcript),
    })
}

/// Runs one user turn until the model answers without a tool call, fails,
/// or `cancel` is cancelled (`agent.js` `runTurn`): the completion (retried
/// by today's rules for an API provider, once for a harness), its message
/// stored, each call parsed, refused or dispatched in order and its result
/// stored, the screenshots after the batch, a save after every step. A
/// failure stores what the batch owes (a result for each unanswered call,
/// then the promised screenshots) and is returned for the caller to end the
/// turn with (`store.endTurn`). `on_started` is called once the model is
/// under way.
pub async fn run_turn(
    setup: &TurnSetup,
    sink: &SharedSink,
    cancel: &CancellationToken,
    on_started: &mut (dyn FnMut() + Send),
) -> Result<(), TurnError> {
    let (provider_key, model, resolution, chat_id) = {
        let conversation = setup.conversation.lock().expect("a chat's lock");
        (
            conversation.provider.clone(),
            conversation.model.clone(),
            conversation.resolution,
            conversation.id.0.clone(),
        )
    };
    let image_input = match setup.provider.model_info(&model).await {
        Ok(facts) => facts.image_input != Some(false),
        Err(error) => {
            eprintln!("[model-info] {provider_key}/{model}: {}", error.message);
            true
        }
    };
    let provider_id: ProviderId =
        serde_json::from_value(Value::String(provider_key.clone())).map_err(|_| {
            TurnError::plain("Cannot destructure property 'harness' of 'PROVIDERS[conversation.provider]' as it is undefined.")
        })?;
    let harness = provider_id.is_harness();
    if !harness {
        on_started();
    }
    let form = Form {
        placement: setup.placement,
        image_input,
        network_access: setup.network_access,
        chat: setup.chat,
        resolution,
    };
    let workspace = Workspace {
        chat: chat_id.clone(),
        host: setup.store.folder().root.join("sandbox").join(&chat_id),
    };
    loop {
        if cancel.is_cancelled() {
            return Err(TurnError::plain(retry::ABORTED));
        }
        let request = request(setup, form).await?;
        let completed = {
            let mut on_delta = |delta: Delta| match delta {
                Delta::Started => on_started(),
                Delta::Content(text) => {
                    if !text.is_empty() {
                        emit(sink, TurnEvent::AssistantDelta(text));
                    }
                }
                Delta::Reasoning(text) => {
                    if !text.is_empty() {
                        emit(sink, TurnEvent::ReasoningDelta(text));
                    }
                }
                Delta::ToolCall {
                    index,
                    id,
                    name,
                    arguments,
                } => emit(
                    sink,
                    TurnEvent::ToolCallDelta {
                        index,
                        id: Some(id),
                        name: Some(name),
                        arguments,
                    },
                ),
                Delta::Status(text) => emit(sink, TurnEvent::HarnessStatus(text)),
                Delta::Notice { level, text } => emit(sink, TurnEvent::Notice { level, text }),
                Delta::Plan(items) => emit(sink, TurnEvent::Plan(items)),
            };
            if harness {
                setup
                    .provider
                    .complete(&request, &mut on_delta, cancel)
                    .await
            } else {
                let mut on_retry = |event: retry::RetryEvent| {
                    emit(
                        sink,
                        TurnEvent::CompletionRetry {
                            attempt: event.attempt,
                            delay_ms: event.delay_ms,
                            error: event.error,
                        },
                    )
                };
                retry::complete_with_retry(
                    &*setup.provider,
                    &request,
                    &mut on_delta,
                    &mut on_retry,
                    cancel,
                )
                .await
            }
        };
        let message = completed.map_err(provider_error)?;
        let reply = reply_of(&message)?;
        with_chat(setup, move |store, conversation| {
            store
                .append_message(conversation, NewMessage::reply(&reply), None)
                .map(drop)
                .map_err(|e| e.message)
        })
        .await?;
        if let Some(reasoning) = message.reasoning.as_ref().filter(|r| !r.is_empty()) {
            emit(sink, TurnEvent::Reasoning(reasoning.clone()));
        }
        if message.tool_calls.is_empty() {
            emit(
                sink,
                TurnEvent::Assistant {
                    text: message.content.clone(),
                    last: true,
                },
            );
            return Ok(());
        }
        if !message.content.is_empty() {
            emit(
                sink,
                TurnEvent::Assistant {
                    text: message.content.clone(),
                    last: false,
                },
            );
        }
        let (world, chat_model) = with_chat(setup, |store, conversation| {
            store.save(conversation).map_err(|e| e.message)?;
            let world = conversation.world.clone();
            Ok((world, conversation.model.clone()))
        })
        .await?;
        let mut chat = Chat {
            id: chat_id.clone(),
            model: chat_model,
            world,
            workspace: workspace.clone(),
            image_input,
            chat: setup.chat,
        };
        let mut screenshots = Vec::new();
        let mut answered = 0;
        let mut failure = None;
        for call in &message.tool_calls {
            if let Err(error) =
                run_call(setup, sink, cancel, &mut chat, call, &mut screenshots).await
            {
                failure = Some(error);
                break;
            }
            answered += 1;
        }
        let calls: Vec<String> = message.tool_calls[answered..]
            .iter()
            .map(|call| call.id.clone())
            .collect();
        let stopped = cancel.is_cancelled();
        let reason = failure.clone();
        with_chat(setup, move |store, conversation| {
            if let Some(reason) = &reason {
                for call in &calls {
                    store
                        .append_message(
                            conversation,
                            NewMessage::unanswered(call, stopped, reason),
                            None,
                        )
                        .map_err(|e| e.message)?;
                }
            }
            for (text, urls) in &screenshots {
                store
                    .append_message(conversation, NewMessage::screenshots(text, urls), None)
                    .map_err(|e| e.message)?;
            }
            // Not saved after a failure: the caller's endTurn saves these
            // with the error.
            if reason.is_none() {
                store.save(conversation).map_err(|e| e.message)?;
            }
            Ok(())
        })
        .await?;
        if let Some(failure) = failure {
            return Err(TurnError::plain(failure));
        }
    }
}

/// One call of a batch (`runCall`): its start, its result and its tool
/// message, saved. An error is a store failure, which fails the turn.
async fn run_call(
    setup: &TurnSetup,
    sink: &SharedSink,
    cancel: &CancellationToken,
    chat: &mut Chat,
    call: &mcbench_providers::ToolCall,
    screenshots: &mut Vec<(String, Vec<String>)>,
) -> Result<(), String> {
    let host = Arc::clone(setup.store.host());
    let started = host.now_ms();
    let name = call.name.clone();
    let text = if call.arguments.is_empty() {
        "{}"
    } else {
        call.arguments.as_str()
    };
    let (args, refused) = match v8json::parse(text) {
        Err(message) => (
            Value::Null,
            Some(format!("Invalid tool arguments JSON: {message}")),
        ),
        Ok(value @ Value::Object(_)) => (value, None),
        Ok(value) => (
            value,
            Some("Invalid tool arguments JSON: Tool arguments must be an object".into()),
        ),
    };
    emit(
        sink,
        TurnEvent::ToolStart {
            call_id: call.id.clone(),
            name: name.clone(),
            shown: Box::new(call_of(&name, &args, refused.is_some())),
        },
    );
    let (outcome, shown_images) = match refused {
        Some(error) => (ToolOutcome::result(refusal(error)), None),
        None => {
            let Value::Object(map) = &args else {
                unreachable!("an object")
            };
            let mut progress = |progress: Progress| {
                emit(
                    sink,
                    TurnEvent::ToolProgress {
                        call_id: call.id.clone(),
                        name: name.clone(),
                        progress: ProgressShown {
                            message: progress.message,
                        },
                        raw: Value::Object(progress.raw),
                    },
                );
            };
            let before = (chat.world.uid(), chat.world.settle_ticks);
            let executed =
                execute_tool_call(&setup.tools, chat, &name, map, &mut progress, cancel).await;
            // A build replaces the chat's world, and every settle sets the
            // ticks of the world it settled (today's object, which the
            // conversation holds), even when the call then threw.
            if (chat.world.uid(), chat.world.settle_ticks) != before {
                setup.conversation.lock().expect("a chat's lock").world = chat.world.clone();
            }
            // A call that threw fails the turn: no result is emitted or
            // stored for it, as `runCall` rejects today.
            let executed = executed.map_err(|error| error.0)?;
            let shown: Vec<String> = match &executed.screenshots {
                Some(batch) => batch.images.iter().map(|i| i.url.clone()).collect(),
                None => executed.images.iter().map(|i| i.url.clone()).collect(),
            };
            if let Some(batch) = &executed.screenshots {
                screenshots.push((
                    batch.text.clone(),
                    batch.images.iter().map(|i| i.url.clone()).collect(),
                ));
            }
            (executed, Some(shown))
        }
    };
    let result = outcome.result.clone();
    let ok = js::truthy(result.get("ok"));
    let image_urls = if vision(&name) && ok {
        shown_images
    } else {
        None
    };
    emit(
        sink,
        TurnEvent::ToolResult {
            call_id: call.id.clone(),
            name: name.clone(),
            shown: Box::new({
                // The store wrote these images for this call; an address it
                // refuses now is a broken invariant (today's event carries
                // the address alone, so Node has no step here).
                let mut shown = outcome.shown(&name, |url| {
                    setup.store.file_path(url).unwrap_or_else(|error| {
                        panic!("the image {url} the store just wrote is refused: {error}")
                    })
                });
                if image_urls.is_none() {
                    shown.images.clear();
                }
                shown
            }),
            result: result.clone(),
            image_urls,
        },
    );
    if name == "build" && ok && js::truthy(result.get("changed")) {
        emit(sink, TurnEvent::WorldChanged);
    }
    let telemetry = ToolTelemetry {
        name: name.clone(),
        started_at: iso(started),
        wall_ms: (host.now_ms() - started).max(0) as u64,
        ok: result.get("ok") == Some(&Value::Bool(true)),
        cancelled: result.get("cancelled") == Some(&Value::Bool(true)),
    };
    let content = message_content(&name, &result);
    let id = call.id.clone();
    with_chat(setup, move |store, conversation| {
        store
            .append_message(
                conversation,
                NewMessage::tool_result(&id, &telemetry, &content),
                None,
            )
            .map_err(|e| e.message)?;
        store.save(conversation).map_err(|e| e.message)
    })
    .await
}
