//! # mcbench-tools
//!
//! The tools models call, behind ONE trait, [`Tool`]: `build`,
//! `test_build`, `see_build`, `bash`, `view_image` and, in chats,
//! `prompt_library`, dispatched as today by [`Tools::execute`]
//! (`agent.js` `executeToolCall`); and what they are made of: the settle of
//! a build ([`settle`]), `test_build`'s engine verifier with the
//! benchmark's budgets ([`verify`]), `see_build`'s shot resolution and
//! pictures ([`shots`]), schematic export ([`export`]), and share
//! ([`share`]). It replaces the tool bodies of `server/agent.js`
//! (`executeToolCall`, `executeBuild`, `executeSeeBuild`, `executeBash`,
//! `executeViewImage`, `executeTestBuild`, `toolMessageContent`,
//! `minecraftReport`, `incompleteBed`, `unsupportedBlocks`,
//! `pruneReports`), `prompts.js`'s tool body, the settle half of
//! `shot-world.js`, `minecraft-verify.js`'s engine verifier,
//! `verification-liveness.js`, `shot.js`, `share.js`, `share-viewer.js` and
//! `schematic.js`.
//!
//! The schemas and descriptions of the tools are not here: they are text
//! that reaches models and live with the system prompt in `mcbench-agent`'s
//! contract, copied from today's sources. What a tool answers (its result
//! JSON, its error sentences, `see_build`'s `described`, the verifier's
//! report) also reaches models and is today's text and today's key order,
//! byte for byte: results are `serde_json` values (`preserve_order`) built
//! in today's key order and written by `mcbench_json::stringify`
//! ([`message_content`]); sentences are copied from `server/`.
//!
//! | Module | Today's code |
//! |---|---|
//! | [`build`] | `executeBuild`, `minecraftReport`, `incompleteBed`, `unsupportedBlocks` |
//! | [`settle`] | `shot-world.js` `settle`, `runSettle`, `checkTicks`, `currentTicks`, `idChanges` |
//! | [`bash`] | `executeBash` (the transcript is `mcbench-sandbox`'s) |
//! | [`view_image`] | `executeViewImage` |
//! | [`test_build`] | the tool over [`verify::execute_test_build`] (`executeTestBuild`), `pruneReports` |
//! | [`see_build`] | `executeSeeBuild` and its branch of `executeToolCall` |
//! | [`shots`] | `shot.js` `resolveShot`, `renderShots`, `montageCells`, `composeMontage` over the process's one renderer |
//! | [`prompt_library`] | `executeToolCall`'s `prompt_library` branch over `mcbench-store`'s library |
//! | [`shown`] | `toolMessageContent`, `tool_start`'s summary, `web/app.js` `itemsFromMessages`' reading of a tool result |
//! | [`export`] | `schematic.js` |
//! | [`share`] | `share.js` (the viewer, `share-viewer.js`, is given as [`share::SiteViewer`]) |
//! | [`share_viewer`] | `share-viewer.js` (`viewerFiles`, `writeViewer`, `sharePageHtml`), and BlueMap's web app from its pinned jar without Java (today the bake unpacks it with Java) |
//! | [`budget`] | the adapter of `mcbench-store`'s `Storage` to `mcbench_sandbox::Budget` |
//!
//! ## Owner
//!
//! The stage-three session.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | agent | a tool's name, its parsed arguments and the [`Chat`] in; [`ToolOutcome`] out (the result JSON, screenshots to attach, saved images; the chat's world replaced by a build), [`Progress`] through a callback, a [`ToolError`] when the call throws (the turn fails); [`message_content`], [`ToolShown`] and [`CallShown`] for the messages and the interface | `mcbench-agent` calls [`Tools::execute`] for each call | the async runtime; calls of one batch run in order |
//! | renderer | `mcbench_render::WorldUpload` and resolved `Shot`s; pictures back | [`shots::SharedRenderer`] calls `mcbench_render::Renderer` (`world`, `shots`, `montage`); its owner starts the renderer once and gives it ([`shots::SharedRenderer::set`]): the app's live view, `headless turn`, `headless shots` | `spawn_blocking`; the implementation's render thread |
//! | engine | settles ([`settle::Settler`] over `SettlePool` and `Settles`), `VERIFY` requests and reports | [`verify`] and `build` call `mcbench-engine`; the verifier through [`verify::VerifyEngine`], which `mcbench_engine::VerifyEngines` implements (tests give a fake) | engine child processes |
//! | build | a program and a world clone; `mc.readFile` and `mc.settled` through `BuildHost` | `build` calls `mcbench_build::BuildRunner` | the V8 child process |
//! | sandbox | `bash` commands, `view_image`'s PNG, `/work` files | `bash`, `view_image`, `build`, `test_build` call `mcbench_sandbox` | sandbox processes |
//! | store | images, the chat's storage budget, world chunks, report references, the prompt library | the tools call `mcbench-store` | `spawn_blocking` |
//! | verify | a `test_build` call ([`verify::execute_test_build`] on the [`Services`]' `VerifierPool`), the chat's `/work` and budget as its `Work` ([`test_build::ChatWork`]) | [`test_build`] calls [`verify`] | engine threads |
//! | headless | [`verify::VerifierPool`] and its settings per request | `mcbench-lite headless grade` runs one lane of it | the headless process |
//!
//! ## Behaviour defined by
//!
//! Today's tests (docs/native-app.md, Appendix A) whose subject is a tool
//! body, ported in `tests/ported/` (23, on the engine children and the
//! V8 child; each names its Node test): `build-verdict.test.js` 3,
//! `settle.test.js` 5 of 7, `settle-ticks.test.js` 5 of 7,
//! `settle-flow.test.js` 1 of 7, `agent-vision.test.js` 1 of 7,
//! and `storage-limits.test.js`'s three `test_build` tests;
//! `schematic.test.js` 5. Beside
//! them the differential replay of every stored `build`, `test_build`,
//! `view_image` and `bash` call of the fixture and 408 generated calls
//! against today's `executeToolCall` (`tests/node/prepare.js`,
//! `tests/node/replay.js`, `examples/replay.rs`, `tests/replay.rs`), the
//! three-way check of [`call_shown`] and [`tool_shown`] against the
//! server's `tool_start` and the page's `itemsFromMessages`
//! (`tests/node/shown.js`, `tests/shown.rs`), and the schematic against
//! `writeSchematic` on every stored world (`tests/node/schematic.js`,
//! `examples/schematic.rs`). `see_build`'s renderer client and framing:
//! `tests/shots.rs` (15, a fake renderer: `shot-context.test.js` 7,
//! `shot-cancellation.test.js` 1, `shot-recovery.test.js` 1 of 2,
//! `shot-renderer.test.js` 2 of 9, `shot-framing.test.js` 3 of 4, and
//! `composeMontage`; the file says why the rest are not ported), and the
//! differential of `see_build` itself against today's `executeToolCall`
//! with the helper built from `native/draw` (`tests/node/see-build.js`,
//! `examples/see_build.rs`, `tests/see_build_diff.rs`: the tool message,
//! every JPEG's sha256 and their order, on the software floor and on a
//! hardware adapter), and the camera's arithmetic against V8's
//! (`tests/node/math.js`, `tests/math.rs`). `tests/ported` needs `MCBENCH_TABLES` (each
//! test reports ignored without it); `tests/shown.rs` and `tests/replay.rs`
//! are ignored without `MCBENCH_TOOLS_SHOWN` and `MCBENCH_TOOLS_REPLAY` and
//! fail when the file they name is missing.
//!
//! [`share_viewer`]: `share-viewer.test.js` 12 of 12 in
//! `tests/share_viewer.rs` (the six whose subject is the bridge's or the
//! mesher worker's JavaScript run that JavaScript in Node against the files
//! [`share_viewer::write_viewer`] wrote, and need `MCBENCH_SHARE_NODE`), and
//! its differential against today's `writeViewer` and `sharePageHtml` and
//! the bake's web app (`tests/node/share-viewer.js`, needing
//! `MCBENCH_DIFF_DATA` too).
//!
//! The verifier's, ported in `tests/` (a file each, named after today's):
//!
//! | Today's file | Ported | Not ported, and why |
//! |---|---|---|
//! | `minecraft-verify.test.js` (31) | 24 | 7 are Paper's: its asynchronous chunk preparation, an import cancelled inside Paper, a kept runtime and a dead one, the runtime's exit while preparing and while running, the plugin's compiler |
//! | `engine-verify.test.js` (8) | 8 | (the case of a test cancelled while its engine program is built is a test cancelled while its engine child starts: nothing is built) |
//! | `agent-verification.test.js` (11) | 8 | a dropped completion's retry and the turn loop (`mcbench-agent`'s), the build tool's revisions (`build`'s) |
//! | `verification-liveness.test.js` (3) | 0 | the Paper process's liveness; an engine child that dies answers `The engine stopped (<signal>): ...` itself (`mcbench-engine`) |
//! | `verifier-capabilities.test.js` (5) | 2 | the schema and the guidance (3) are `mcbench-agent`'s contract |
//! | `test-build-lanes.integration.test.js` (1) | 1 | |
//!
//! The differential harness (`tests/differential.rs`, `tests/node/`) runs
//! the same `test_build` calls and grader experiments through today's Node
//! verifier on the released engine and through [`verify`] on the
//! app's engine children, and compares the results and report files.
//!
//! ## Decided
//!
//! What a tool is given of its chat ([`Chat`]): its id and model, its world
//! (which `build` replaces and whose settle ticks every settle sets, as
//! `shot-world.js` sets them on today's object), its `/work`, whether the
//! model takes images and whether it is a chat turn: what the five tool
//! bodies read of `conversation` today.
//!
//! Number formatting in result JSON: `mcbench_json::stringify`, JavaScript's
//! rules; the replay compares every stored and replayed result byte for
//! byte.
//!
//! A tool call that throws today (`executeBuild` writing `/work` files after
//! the world was persisted, a world that cannot be cloned, an unknown block
//! id in a bed check) fails the turn: [`Tool::call`] answers [`ToolError`]
//! with the message, which the agent stores as today's unanswered result.
//!
//! A build runs to its end once started, as today (`runProgram` takes no
//! signal): the build child is not killed by a stopped turn, so no model
//! reads `mcbench_build::runner::CANCELLED`; a call that starts after the
//! turn stopped is answered `Cancelled by user` as today.
//!
//! The shared page is today's page (the main session's ruling for 2.0.0):
//! BlueMap's web app with the owner's mesher, not the renderer's WebGL2
//! build ([`share_viewer`]). BlueMap's web app is fetched from BlueMap's
//! pinned release at the first publish, never shipped.
//!
//! `see_build` draws with ONE renderer per process, shared by every call
//! and, in the app, with the live view ([`shots::SharedRenderer`]): a call
//! before the renderer has started waits for it, one after a failed start
//! answers today's `Screenshot initialization: <the renderer's sentence>`.
//! Today's helper is one per server too; the in-process renderer is never
//! restarted, so its failed start, or a second lost device, lasts for the
//! process.
//!
//! ## Open
//!
//! Share (GitHub Pages publish through `git` and `gh`, the X post): settled
//! in step 3b with the share page's open risks (docs/native-app.md, Share
//! pages).
//!
//! The viewer folder's gzip members ([`share_viewer`]): every file of it
//! is today's after inflating, the `.gz` bytes are flate2's, so its viewer
//! id differs from Node's for the same content (as the schematic's gzip
//! layer below).
//!
//! The schematic's gzip layer: the NBT inside is byte-identical to
//! `writeSchematic`'s on every stored world, the compressed bytes are not
//! (Node's zlib is Chromium's fork, whose deflate matches neither
//! `miniz_oxide`, `zlib-rs` nor madler zlib 1.3.2 on these files; every
//! reader inflates both alike). Settled by the session: keep it, or vendor
//! Chromium's zlib (a C dependency) if byte identity of the file matters.
//!
//! A build error cut at 8,000 UTF-16 units inside a surrogate pair:
//! `JSON.stringify` writes the lone half as `\udxxx`, a Rust string holds
//! U+FFFD (as `mcbench-build`'s Open item on lone surrogates).
//!
//! `see_build`'s camera where a model gives its own eye and target: V8's
//! `Math.sin` and `Math.cos` are a port of glibc's (LGPL), fdlibm's (used
//! here) differ by one unit in the last place for about 1% of arguments
//! ([`shots`]); porting V8's is a licence question for the owner.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

pub use mcbench_sandbox::Workspace;
pub use mcbench_world::World;

pub mod bash;
pub mod budget;
pub mod build;
pub mod engines;
pub mod export;
pub mod js;
pub mod prompt_library;
pub mod see_build;
pub mod settle;
pub mod share_viewer;
pub mod shown;
pub mod test_build;
#[doc(hidden)]
pub mod testing;
pub mod view_image;
pub mod work;

pub use shown::{CallShown, Diff, ToolShown, call_of, call_shown, message_content, tool_shown};
pub use work::WorkFolders;

/// What a tool is given of the chat it runs in.
pub struct Chat {
    /// The chat's id.
    pub id: String,
    /// The model's id (the prompt library records which model added a
    /// prompt).
    pub model: String,
    /// The chat's world; `build` replaces it, and a settle sets its settle
    /// ticks.
    pub world: World,
    /// The chat's `/work` folder.
    pub workspace: Workspace,
    /// Whether the model takes images (`see_build` and `view_image` refuse
    /// otherwise, with today's message).
    pub image_input: bool,
    /// A chat turn rather than a benchmark attempt (`prompt_library` is
    /// offered).
    pub chat: bool,
}

/// An image a tool attaches: stored under the chat's images, referred to by
/// URL in the messages (base64 only when a request is sent).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRef {
    /// The URL the message stores (`/api/images/<chat>/<name>`).
    pub url: String,
}

/// Screenshots that follow a tool's result as a user message
/// (`executeToolCall`'s `screenshots`: a text line and the images).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screenshots {
    /// The line before them, as today (`[see_build] Screenshots of the
    /// current build:`, `[view_image] /work/<path>:`).
    pub text: String,
    /// The images, in shot order.
    pub images: Vec<ImageRef>,
}

/// What a tool call produced (`executeToolCall`'s `{ result, screenshots,
/// images }`).
#[derive(Clone, Debug, PartialEq)]
pub struct ToolOutcome {
    /// The result object the tool message carries ([`message_content`]
    /// writes it as JSON, or `bash`'s transcript as plain text).
    pub result: Value,
    /// Screenshots to attach after the result.
    pub screenshots: Option<Screenshots>,
    /// Images saved without attaching them (`see_build` with `attach: false`).
    pub images: Vec<ImageRef>,
}

impl ToolOutcome {
    /// A result with nothing after it.
    pub fn result(result: Value) -> ToolOutcome {
        ToolOutcome {
            result,
            screenshots: None,
            images: Vec::new(),
        }
    }

    /// The tool message's content (`toolMessageContent(name, result)`).
    pub fn content(&self, name: &str) -> String {
        message_content(name, &self.result)
    }

    /// The images shown with the result (`tool_result`'s `images`: the
    /// screenshots' or the saved ones of an ok `see_build` or
    /// `view_image`).
    pub fn shown_images(&self, name: &str) -> Vec<&ImageRef> {
        if !matches!(name, "see_build" | "view_image") || !js::truthy(self.result.get("ok")) {
            return Vec::new();
        }
        match &self.screenshots {
            Some(screenshots) => screenshots.images.iter().collect(),
            None => self.images.iter().collect(),
        }
    }

    /// What the interface shows of this result: [`tool_shown`] of its
    /// content, with its images as paths in the data folder.
    pub fn shown(&self, name: &str, path_of: impl Fn(&str) -> PathBuf) -> ToolShown {
        let mut shown = tool_shown(name, &self.content(name));
        shown.images = self
            .shown_images(name)
            .into_iter()
            .map(|image| path_of(&image.url))
            .collect();
        shown
    }
}

/// A tool call that threw: the turn fails with this message (today's
/// `runCall` rejection, which `store.unansweredResult` records).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolError(pub String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ToolError {}

impl From<String> for ToolError {
    fn from(message: String) -> ToolError {
        ToolError(message)
    }
}

/// A tool's progress, for the transcript (`tool_progress` events: the page
/// reads only `message`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// What the tool is doing, as today (`test_build`: `Test 1: <phase>;
    /// Test 2: ...`).
    pub message: String,
    /// The update as the tool gave it (`tool_progress`'s other fields:
    /// `phase`, `loadedColumns`, ...), its `message` the joined one.
    pub raw: Map<String, Value>,
}

/// A tool a model can call. Every tool implements it; each holds the
/// [`Services`] it uses.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The name models call it by (`build`, `test_build`, `see_build`,
    /// `bash`, `view_image`, `prompt_library`).
    fn name(&self) -> &'static str;
    /// Runs one call with its parsed arguments (an object; the agent refuses
    /// anything else with today's message before a tool is reached).
    async fn call(
        &self,
        chat: &mut Chat,
        arguments: &Map<String, Value>,
        progress: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError>;
}

/// What the tools use: the store, the settles, the build runner, the
/// sandbox, the block registry and Minecraft's facts, the budgets and the
/// verifier.
pub struct Services {
    /// The store (images, the budget, world chunks, report references).
    pub store: Arc<mcbench_store::Store>,
    /// The prompt library.
    pub prompts: Arc<mcbench_store::PromptLibrary>,
    /// The settles.
    pub settler: Arc<settle::Settler>,
    /// Build programs.
    pub runner: Arc<dyn mcbench_build::BuildRunner>,
    /// The `bash` sandbox.
    pub sandbox: Arc<dyn mcbench_sandbox::Sandbox>,
    /// The block registry (`blocks.json`), as the build child checks names.
    pub registry: Arc<Value>,
    /// Minecraft's facts.
    pub facts: Arc<mcbench_world::facts::MinecraftFacts>,
    /// The verifier's budgets (`BUDGETS`).
    pub budgets: verify::Budgets,
    /// `test_build`'s verifier (`minecraftVerifier`).
    pub verifier: Arc<verify::VerifierPool>,
    /// The process's one renderer, which `see_build` draws with (in the
    /// app the live view's).
    pub renderer: Arc<shots::SharedRenderer>,
}

impl Services {
    /// Runs `job` with the store off the runtime's workers.
    pub(crate) async fn blocking<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mcbench_store::Store) -> T + Send + 'static,
    ) -> T {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || job(&store))
            .await
            .expect("a store job does not panic")
    }
}

/// `VISION_TOOLS`.
const VISION_TOOLS: [&str; 2] = ["see_build", "view_image"];

/// The tools, dispatched as `executeToolCall` dispatches them: one
/// [`Tool`] per name, the app's ([`Tools::new`]) or any set
/// ([`Tools::of`], for a fake or a mixture in tests and replays).
#[derive(Clone)]
pub struct Tools {
    tools: Vec<Arc<dyn Tool>>,
}

impl Tools {
    /// Every tool over `services`.
    pub fn new(services: Arc<Services>) -> Tools {
        Tools::of(vec![
            Arc::new(build::Build(services.clone())),
            Arc::new(test_build::TestBuild(services.clone())),
            Arc::new(see_build::SeeBuild(services.clone())),
            Arc::new(bash::Bash(services.clone())),
            Arc::new(view_image::ViewImage(services.clone())),
            Arc::new(prompt_library::PromptLibraryTool(services)),
        ])
    }

    /// These tools, by their names.
    pub fn of(tools: Vec<Arc<dyn Tool>>) -> Tools {
        Tools { tools }
    }

    /// These tools with `tool` in place of the one of its name.
    pub fn with(mut self, tool: Arc<dyn Tool>) -> Tools {
        self.tools.retain(|other| other.name() != tool.name());
        self.tools.push(tool);
        self
    }

    fn named(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|tool| tool.name() == name)
            .map(|tool| &**tool)
    }

    /// `executeToolCall(conversation, name, args, ...)`: a call made after
    /// the turn stopped is cancelled; `prompt_library` only in a chat;
    /// `build` and `test_build` for every model; the vision tools refused to
    /// a model that takes no images; any other name unknown. An `Err` is a
    /// call that threw: the turn fails with its message.
    pub async fn execute(
        &self,
        chat: &mut Chat,
        name: &str,
        arguments: &Map<String, Value>,
        progress: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError> {
        if cancel.is_cancelled() {
            return Ok(ToolOutcome::result(
                json!({"ok": false, "cancelled": true, "error": "Cancelled by user"}),
            ));
        }
        let unknown =
            || ToolOutcome::result(json!({"ok": false, "error": format!("Unknown tool: {name}")}));
        let gated = match name {
            "prompt_library" if !chat.chat => return Ok(unknown()),
            "prompt_library" | "build" | "test_build" => false,
            _ => !chat.image_input && VISION_TOOLS.contains(&name),
        };
        if gated {
            return Ok(ToolOutcome::result(json!({
                "ok": false,
                "error": "The selected model does not support image input"
            })));
        }
        match self.named(name) {
            Some(tool) => tool.call(chat, arguments, progress, cancel).await,
            None => Ok(unknown()),
        }
    }
}

pub mod verify;

pub mod shots;

pub mod share;

#[cfg(test)]
mod seam {
    use super::*;

    struct FakeTool;
    #[async_trait]
    impl Tool for FakeTool {
        fn name(&self) -> &'static str {
            unimplemented!("a fake for the seam test")
        }
        async fn call(
            &self,
            _: &mut Chat,
            _: &Map<String, Value>,
            _: &mut (dyn FnMut(Progress) + Send),
            _: &CancellationToken,
        ) -> Result<ToolOutcome, ToolError> {
            unimplemented!("a fake for the seam test")
        }
    }

    #[test]
    fn a_fake_can_be_written_against_the_trait() {
        let _: Box<dyn Tool> = Box::new(FakeTool);
    }
}
