//! # mcbench-agent
//!
//! The turn loop: one user turn of a chat or one benchmark attempt, from the
//! completion request through tool dispatch and tool results to the turn's
//! end, with the events the interface shows as they stream. It holds every
//! text that reaches models that is not a tool's own answer: the system
//! prompt, the tool schemas and descriptions, the verification budgets'
//! wording, the resolution and title prompts, the prompt library's tool
//! (module [`contract`]). It replaces `server/agent.js`, `agent-context.js`,
//! `prompt.js`, `prompt-generation.js`, `prompt-match.js`,
//! `verification-contract.js` (its text), `image-input.js`, the turn half of
//! `turn-stream.js` and the turn lifecycle of `server/index.js`.
//!
//! ## Text that reaches models
//!
//! It is copied byte for byte from today's sources, never retyped:
//! `server/prompt.js` 16-116 (`systemPrompt`), `agent.js` 29-168 (the
//! `bash`, `build`, `see_build` and `view_image` schemas) and 700-711 (results
//! the loop words; 170-520, the tool bodies' results, are `mcbench-tools`'),
//! `verification-contract.js` 95-170 (the
//! `test_build` schema, which quotes the budgets), `prompts.js` 18-38 and
//! 128-158 (the prompt library's tool), `prompt-generation.js` 8-25 and the
//! title prompt of `index.js` 205-230. The private benchmark pins it: the
//! sha256 of `toolsFor()`, `systemPrompt()` and `BUDGETS` with the import
//! order revision for its 8 forms (the benchmark's tool contract script),
//! which is [`contract::TOOL_CONTRACT_SHA256`] on `main` 5398055 and on this
//! branch. `mcbench-lite headless contract` must print the same.
//!
//! ## Owner
//!
//! The stage-three session. [`TurnEvent`] is a seam with the stage-four
//! session (the interface shows it): a change to it is proposed to that
//! session first.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | providers | `mcbench_providers::CompletionRequest` out (built in one function, `turn::request`), `Delta`s and the `AssistantMessage` back | [`run_turn`] calls `Provider::complete`, through `retry::complete_with_retry` for an API provider and once for a harness | the async runtime |
//! | tools | each call's name and parsed arguments, `mcbench_tools::Chat`; `ToolOutcome` back | [`execute_tool_call`] calls `mcbench_tools::Tool::call` | the async runtime; a batch's calls run one after another, in call order, as today |
//! | store | the chat's messages (in memory as [`api::in_memory`]), each message appended through `NewMessage`'s constructors, saves, turn ends | [`run_turn`] and [`lifecycle::Turns`] call `mcbench-store` | `spawn_blocking` |
//! | interface | [`TurnEvent`]s, in order, for each turn | [`run_turn`] calls the [`TurnSink`] it is given; [`lifecycle::TurnStream`] replays them to every sink attached | the async runtime to the interface thread |
//! | interface | the chat list ([`ConversationList`]) and a bench's details ([`BenchDetails`], module `bench`), re-exported by `mcbench_ui` | the core makes them and publishes them in its updates | the core's runtime to the interface thread |
//! | the core | [`lifecycle::Turns`] and its [`lifecycle::Services`] (providers, tools, the sandbox's reset, the title model, the chat list's changes) | the core owns `Turns` and implements `Services` | the async runtime |
//! | headless | `headless turn` (`mcbench-headless`), `headless contract` | `mcbench-headless` calls [`run_turn`], [`execute_tool_call`], [`message_content`] and [`contract`] | the headless process |
//!
//! ## Behaviour defined by
//!
//! Today's tests (docs/native-app.md, Appendix A), 116 tests in 19 files:
//! `agent-context.test.js` 3, `agent-context-kept.test.js` 2,
//! `agent-retry.test.js` 2, `agent-screenshots.test.js` 1 (its `renderShots`
//! stub stays a stub), `agent-verification.test.js` 11 (with
//! `mcbench-tools`), `agent-vision.test.js` 7, `build-verdict.test.js` 3,
//! `prompt.test.js` 2, `prompts.test.js` 7, `prompt-generation.test.js` 2,
//! `prompt-match.test.js` 4, `resolution.test.js` 9 (the resolution prompt
//! text), `turn-lifecycle.test.js` 3, `turn-stream.test.js` 5,
//! `server-turns.test.js` 32 (the turn lifecycle), and with
//! `mcbench-engine` `settle.test.js` 7, `settle-flow.test.js` 7,
//! `settle-flow.integration.test.js` 2 and `settle-ticks.test.js` 7; and the
//! tool-contract hash for all 8 forms. And two differential proofs against
//! today's `server/` on clones of a data folder (`tests/common` names the
//! environment): `tests/api_diff.rs`, every request [`api::api_messages`]
//! makes for every stored chat at every assistant message, for a model that
//! takes images and a text-only one, under three context budgets, against
//! `apiMessages` byte for byte with the archive files; and
//! `tests/replay_diff.rs`, stored turns and synthetic ones replayed through
//! today's chat route and through [`lifecycle::Turns`] with the same replay
//! model and tool results: events, requests and every row.
//!
//! ## Decided
//!
//! How the Rust side reads the text (`native/README.md`, Decisions of stage
//! three). The schemas are templates today: `test_build`'s quotes the
//! budgets (`One to ${BUDGETS.tests} experiments`, `${BUDGETS.wallMs /
//! 1000}-second`), the strict placement mode keeps `scheduled_ticks`, and
//! `bash`'s depends on network access; the hash is over `JSON.stringify` of
//! the JavaScript objects. No file under `server/` changes. This crate's
//! `contract/` folder holds the text as files generated mechanically from
//! `server/`'s sources by the script beside them, never edited by hand and
//! read with `include_str!`; the values a template quotes are written with
//! `mcbench-json`'s JavaScript number formatting. Two tests hold it: the
//! sha256 for the benchmark's 8 forms equals
//! [`contract::TOOL_CONTRACT_SHA256`] (always run), and with Node present
//! the script regenerates every file and every form is compared with Node's
//! output byte for byte. The title request and Improve with AI's
//! instructions are generated the same way (`contract/texts/`, captured from
//! a run of `server/index.js`), so [`texts`] holds no typed model text.
//!
//! How the generated files hold the budgets (module [`contract`]): as holes
//! `⟦key⟧` and `⟦key/divisor⟧` that `contract/generate.js` finds by
//! loading `server/` under sentinel budgets and proves against other
//! budgets; Rust fills them with JavaScript's `Number::toString`. A
//! [`contract::Form`] is what `toolsFor`, `systemPrompt` and `runTurn`'s
//! `chat` take (placement, image input, network access, chat, resolution);
//! the budgets are given beside it, as the process's
//! (`mcbench_tools::verify::Budgets::from_env`) or a regime's.
//!
//! [`run_turn`] is an `async fn` on the core's tokio runtime
//! (`native/README.md`, Decisions of stage three), cancelled by a
//! `tokio_util::sync::CancellationToken`; a [`TurnSink`] takes events
//! synchronously and returns at once. It takes a [`TurnSetup`] of owned
//! handles (the store and the chat as `Arc`s, the provider and tools as
//! `Arc<dyn _>`), because a turn is a task of its own that outlives the
//! call that started it.
//!
//! The request's history works on the messages as `server/store.js` holds
//! them in memory, parsed from `StoredMessage::in_memory_text`, as
//! order-preserving JSON values ([`context`], [`api`]): the context
//! archive's bytes and file name are `JSON.stringify` of those, so a typed
//! message cannot stand in for them.
//!
//! A harness's tool calls come to the loop the way they do today: the
//! harness provider (`mcbench_providers::harness`) relays each call the CLI
//! makes through its bridge as one completion's tool call, and the next
//! completion's request delivers the stored result back to the waiting call
//! (`harness.js` `RelaySession`, `pendingResult`), so they are stored,
//! dispatched and emitted by [`run_turn`] as an API provider's are. Every
//! request carries the stored branch as `CompletionRequest::transcript`
//! ([`turn::transcript`]: id, role, seeBuild, stopped, reply) for the
//! harness's resume and record rules. `tests/harness_diff.rs` holds both
//! CLIs' turns through [`lifecycle::Turns`] to today's chat route. The
//! benchmark's external harnesses call tools through `headless turn`'s
//! `call` (`attempt-host.js`), which runs [`execute_tool_call`].
//!
//! The store, SQLite and the files are reached through tokio's blocking
//! pool ([`run_turn`]'s `with_chat`, [`lifecycle::Turns`]' `on_store`).
//!
//! ## Open
//!
//! Nothing: the call's summary, the result's view and a tool message's
//! text are `mcbench-tools`' ([`CallShown`], [`ToolShown`],
//! [`message_content`]), the tools are its `Tools`.

pub use mcbench_json::stringify_pretty;
pub use mcbench_providers::{ModelFacts, ModelInfo, Provider, ToolSchema};
// Settings' sign-in and install views, for the interface (`mcbench_ui`
// names them through this crate).
pub use mcbench_providers::harness::install::{InstallView, Progress};
pub use mcbench_providers::login::{SignInView, State as SignInState};
pub use mcbench_store::{Conversation, Store};
pub use mcbench_tools::{Chat, Tool, ToolOutcome};

pub mod api;
pub mod bench;
pub mod context;
pub mod contract;
mod events;
pub mod lifecycle;
pub mod list;
pub mod texts;
pub mod turn;

pub use bench::{BenchDetails, BenchRound, RoundBranch, RoundStats};
pub use events::{ProgressShown, TurnEvent, TurnSink};
pub use list::{ConversationList, ListedChat};
pub use mcbench_tools::{CallShown, Diff, ToolShown, call_shown, message_content, tool_shown};
pub use turn::{TurnError, TurnSetup, execute_tool_call, run_turn};

/// The most files a message carries (`server/index.js` `readMessage`:
/// `files.length > 8` is refused with `A message supports at most 8
/// attachments`).
pub const MAX_ATTACHMENTS: usize = 8;

/// The largest file a message carries (`server/store.js`
/// `MAX_ATTACHMENT_BYTES`, 100 MiB: `Attachment "<name>" exceeds 100 MB`).
pub const MAX_ATTACHMENT_BYTES: u64 = mcbench_store::MAX_ATTACHMENT_BYTES;

/// The most a message's files hold together (`server/store.js`
/// `MAX_MESSAGE_ATTACHMENT_BYTES`, 200 MiB: `Attachments exceed 200 MB
/// total`).
pub const MAX_ATTACHMENTS_BYTES: u64 = mcbench_store::MAX_MESSAGE_ATTACHMENT_BYTES;

/// The text of a tool message (`agent.js` `toolMessageContent`): `bash`'s
/// transcript as it is when the command ran, the result as JSON otherwise.
pub fn tool_message_content(name: &str, outcome: &ToolOutcome) -> String {
    message_content(name, &outcome.result)
}

#[cfg(test)]
mod seam {
    use super::*;

    struct FakeSink;
    impl TurnSink for FakeSink {
        fn emit(&mut self, _: TurnEvent) {
            unimplemented!("a fake for the seam test")
        }
    }

    #[test]
    fn a_fake_can_be_written_against_the_trait() {
        let _: Box<dyn TurnSink> = Box::new(FakeSink);
    }

    #[test]
    fn the_contract_has_eight_forms() {
        let forms = contract::Regime::ALL
            .into_iter()
            .flat_map(|regime| contract::benchmark_forms().map(|(name, form)| (regime, name, form)))
            .collect::<Vec<_>>();
        assert_eq!(forms.len(), 8);
        assert_eq!(contract::TOOL_CONTRACT_SHA256.len(), 64);
    }
}
