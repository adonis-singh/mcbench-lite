//! # mcbench-ui
//!
//! The native interface (stage four: GPUI with gpui-kit, vendored under
//! `native/vendor`; docs/native-app.md, Interface toolkit). It holds its side
//! of the seam with the core: what the window sends ([`Command`]) and what
//! the core publishes ([`Update`]), the two traits they cross ([`Core`],
//! [`Window`]), and [`run`], which opens the window. Its pure state (the
//! transcript's items, rows, markdown, selection) is in [`state`] with no
//! GPUI types; the views are in [`view`]; [`crash`] is installed first.
//!
//! It replaces `web/app.js`, `style.css`, `index.html`, `theme.js` and
//! `page-errors.js`, the page's state machines (`web/*.mjs`: `transcript`,
//! `composer`, `draft`, `filters`, `archive`, `favorites`, `share`,
//! `published`, `shared`, `prompt-sync`, `settings-panel`, `setup-status`,
//! `address`, `command`, `bench-details`, `resolution`, `clipboard`,
//! `upload-image`, `vendor-icon`, `tool-stream`, `layers`, `place`), and the
//! page's side of today's HTTP routes and NDJSON streams (`server/index.js`:
//! `/api/conversations/stream`, a chat's `stream`, `chat`, `stop`, `branch`,
//! `world`, the settings, setup and bench routes). Turn streams become
//! in-process updates.
//!
//! The chat screen's accessibility contract is
//! `native/crates/ui/ACCESSIBILITY.md`; `cargo test` checks it in process,
//! `native/crates/ui/verify/` on each platform.
//!
//! ## Owner
//!
//! The stage-four session. [`Command`] and [`Update`] are a seam with the
//! stage-three session, which implements [`Core`]; a change to either is
//! proposed to that session first. They name types of the stage-three
//! session's crates (`mcbench_agent::TurnEvent`,
//! `mcbench_agent::ConversationList`, `mcbench_store::{StoredMessage,
//! ConversationHeader, MessageTree}`, `mcbench_setup::Step`,
//! `mcbench_world::Selection`), so a change to those is a seam change for
//! this crate too. The stored messages and tool results are read only in
//! [`state::seam`].
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | window to core | [`Command`]s: send (with edit and branch, retry), stop, branch, open a chat, join its turn, mark seen, dismiss an error, archive, restore, delete, read the archive, search, models and model info, favourites, save the window's preferences, settings changes, the viewer's input; each names the route it replaces | the interface calls [`Core::send`]; it never blocks on the core | the interface thread hands the command over and returns |
//! | core to window | [`Update`]s: the lists, transcripts and their turns as they stream (joined, lost, not running), refused sends, the archive, search results, providers, models, favourites, setup progress, the live world's frames and state, errors | the core calls [`Window::publish`] | the core's threads; [`Window::publish`] puts the update on an unbounded channel that GPUI's thread drains |
//! | preferences | [`Preferences`]: today's `localStorage` keys, the place and the window | the binary calls [`Core::preferences`] once before the window opens | the main thread, before the event loop |
//! | renderer | the live view's frames ([`Frame`]), camera inputs, overlays and picks, through the [`LivePane`] handle the core hands over once ([`Update::LivePane`]) | the window calls the pane; the pane wakes the window | render thread to interface thread (the waker and pick answers post to the window's channel; paint calls [`LivePane::take`]) |
//! | binary | `mcbench-lite` (no subcommand, or `app`) calls [`run`] with the core and [`Options`] | the binary calls this crate | the main thread becomes the interface thread |
//!
//! ## Behaviour defined by
//!
//! Class 4's page modules (docs/native-app.md, Appendix A), ported as this
//! crate's tests, 122 tests in 22 files: `address.test.js` 5,
//! `archive.test.js` 3, `bench-details.test.js` 2, `command.test.js` 2,
//! `composer.test.js` 2, `draft.test.js` 8, `favorites.test.js` 3,
//! `filters.test.js` 6, `layers.test.js` 2, `live-world-states.test.js` 27,
//! `page-errors.test.js` 3, `page-input.test.js` 2, `place.test.js` 2,
//! `prompt-sync.test.js` 8, `published.test.js` 5, `settings-panel.test.js`
//! 6, `share-page.test.js` 8, `shared.test.js` 5, `tool-stream.test.js` 3,
//! `transcript.test.js` 15, `upload-image.test.js` 2 and
//! `vendor-icon.test.js` 3; with the addresses of `page-addresses.test.js`
//! (2) and the fatal error of `server-fatal.test.js` (1). Each of the 34
//! features of docs/native-app.md (Stage four) is done only after it is
//! checked in the rig with real input; `viewer-click.integration.test.js`
//! becomes the rig's click checks.
//!
//! ## Where the next work goes
//!
//! Every part below is a file of its own; adding one touches only its own
//! file and the one line that lists it.
//!
//! | To add | Where |
//! |---|---|
//! | a sidebar section (search, filters, archived, benches, prompts) | `view/sidebar/<section>.rs`, an `impl Sidebar` drawing into [`view::sidebar::Sidebar`]'s `sections`; [`view::sidebar::chats`] is the model |
//! | a transcript item kind | its variant in [`state::transcript`], its row in [`state::rows`], its drawing in `view/chat/transcript.rs` `render_row` (markdown pieces in [`view::markdown`]) |
//! | a composer part (pickers, attachments, prompt tokens, drafts) | `view/chat/<part>.rs` beside [`view::chat::composer`], drawn from `composer_block` |
//! | a picker | a [`view::controls::popover::PopoverState`] with its trigger and panel ([`view::controls::popover`]); menus with [`view::controls::popover::Menu`] |
//! | a pane header control (share, rounds, reveal, fork, run bench) | [`view::shell::header::actions()`] |
//! | a place's pane (benches, leaderboard, prompts; Settings is a layer: [`view::settings`]) | a view in its own module, matched in [`view::shell::Shell`]'s render instead of [`view::shell::panes::placeholder`] |
//! | a layer (palette, lightbox, drawer, full-screen viewer, share) | `view::keys::open_layer(cx, Layer::…, close)` when it opens, `close_layer` when it closes ([`view::keys`]) |
//! | a shortcut's action | `view::keys::on_bind(cx, BindAction::…, run)`; the keymap is data ([`state::keymap`]) |
//! | a seam command or update | [`Command`] or [`Update`] here, with the route it replaces (a seam change: proposed to stage three first) |
//! | a stand-in answer | a match arm in [`dev::standin::StandIn`]'s `send`, its data in both sources (fixture and exported folder) |
//! | a colour, shadow, radius or kind of text | [`view::theme`], the one place a colour is written |
//! | an icon or logo | [`view::icons`] |
//! | a control | [`view::controls`]; every control in [`dev::gallery`] (`mcbench-ui-dev --gallery`) |
//! | a contract row and its test | `ACCESSIBILITY.md`; a test file with `mod common;` (`tests/common`) |
//!
//! ## Open
//!
//! The commands for the rest of today's routes (benches: create, vote,
//! reveal, rounds, retry one; share; prompt library; provider sign-in and
//! CLI install; fork; settings, typed): added by the stage-four session as
//! it builds each feature, each a seam change proposed to the stage-three
//! session with the route it replaces.
//!
//! A zero-copy texture per platform for the live view's frames, later: it
//! changes `mcbench_render::live::Frame`'s pixels and the one function that
//! paints them (`view::world::paint`), nothing of the seam.
//!
//! ## The live view (pane agent)
//!
//! Agreed by the three sessions on 2026-11-17 (`ui-seam-viewer.md` with the
//! renderer's and the core's answers, in the sessions' shared notes). The
//! renderer's types are re-exported from `mcbench_render::live`; the window's
//! commands and the core's updates are below ([`LiveCommand`],
//! [`ViewerInput`], [`LiveState`], [`PaneHandle`]). Frames never cross the
//! update channel: the window takes them from the pane in its paint.
//!
//! The seam's two rules hold with the two agreed exceptions, which follow
//! the `SendRefused` precedent: a failed placement, a failed restart and a
//! world the engine stopped end in [`Update::Live`] whose phase or running
//! state carries the sentence with no [`Update::Error`] before it (the page
//! shows it in the banner, not as a notice); and player input
//! ([`ViewerInput::PlayerInput`]) ends in nothing, being held for the next
//! tick.

pub mod crash;
pub mod dev;
#[cfg(feature = "proof-hooks")]
pub mod proof;
pub mod state;
pub mod view;

use std::fs::File;
use std::io::Write as _;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use gpui_kit::{
    AppContext as _, Bounds, Point as GpuiPoint, Size, WindowBounds, WindowOptions, px,
};

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

pub use mcbench_agent::{ConversationList, ListedChat, ModelFacts, ModelInfo, TurnEvent};
pub use mcbench_setup::Step;
pub use mcbench_store::{
    Archive, ArchivedBench, ArchivedConversation, AttachmentFile, ConversationHeader,
    ConversationSummary, LibraryPrompt, ListedBench, MessageId, MessageTree, PromptAddedBy,
    PromptAttachment, PromptLink, SearchHit, SearchScope, StoredMessage, TreeMessage,
};
pub use mcbench_world::Selection;

// ---------------------------------------------------------------------------
// The seam with the core: what the window sends and what the core
// publishes. Each item names the route of today's server it replaces
// (inventory `style-viewer-routes.md` Part 3, `app.md` 13). A failure the
// page shows as a notice (`showError`) comes back as `Update::Error` with
// the server's text, unless an item says otherwise.
// ---------------------------------------------------------------------------

/// Where a message goes (`POST /api/conversations/<id>/chat`, and for a new
/// chat first `POST /api/conversations`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    // par-chat (stage four's parity pass), SEAM CHANGE agreed with the
    // core's owner 2026-11-18: the provider and model travel with every
    // message to an open chat, as the page sends them (`web/app.js`
    // `send`, `POST .../chat` `{ provider, model }`), and the core sets the
    // chat's model from them (`server/index.js` `handleChat`). An unknown
    // provider is refused with `Choose a configured provider`, an empty
    // model with `Choose a model` (`Update::SendRefused`), and nothing
    // changes.
    /// A chat by id, with the provider and model as the picker has them at
    /// the send (an edit's too).
    Chat {
        /// The chat's id.
        id: String,
        /// The provider's key (`provider`).
        provider: String,
        /// The model (`model`).
        model: String,
    },
    /// A new chat with this provider and model: the core creates its row
    /// (`POST /api/conversations` `{ provider, model }`), lists it, and
    /// sends the message to it.
    NewChat {
        /// The provider's key.
        provider: String,
        /// The model.
        model: String,
    },
    // bench agent: a bench's first round and its later rounds go as a
    // message does (`ui-seam-bench-prompts.md`, a seam addition proposed to
    // stage three). For both, `Outgoing.selections` is empty and `effort`
    // none; `resolution` is the new bench's (a later round keeps the
    // bench's); `edit_of` is the round's id for a round's new version.
    /// A new bench (`POST /api/benches` `{ members, blind, resolution,
    /// text, files, prompts }`): the core makes it, runs its first round
    /// on every model, and answers `Update::BenchSent`.
    NewBench {
        /// The models, in the order chosen (`members`).
        members: Vec<BenchChoice>,
        /// Their names are hidden until revealed (`blind`).
        blind: bool,
    },
    /// The next round of a bench, or a round's new version with `edit_of`
    /// (`POST /api/benches/<id>/rounds` `{ text, files, prompts, editOf }`).
    Bench(String),
}

// bench agent: the types of Bench's seam (`ui-seam-bench-prompts.md`).
/// One model of a new bench (`members[]`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BenchChoice {
    /// The provider.
    pub provider: String,
    /// The model.
    pub model: String,
    /// The effort; none is the model's default (`''`).
    pub effort: Option<String>,
}

/// A vote's pick (`POST /api/benches/<id>/vote` `pick`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VotePick {
    /// A member's id: its build is the best.
    Member(String),
    /// `"tie"`: no build is better.
    Tie,
    /// `null`: the vote taken back.
    TakeBack,
}

/// How an improve or write ended (`POST /api/prompts/generate`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Generated {
    /// The prompt's new text.
    Text(String),
    /// Why it failed (the route's sentence).
    Failed(String),
    /// `Command::CancelGenerate` stopped it.
    Cancelled,
}

pub use mcbench_agent::{BenchDetails, BenchRound, RoundBranch, RoundStats};
pub use mcbench_store::{Content, LeaderboardEntry, Part, Vote};

/// A resolution stop (`web/resolution.mjs`): 1, 2, 5 or 10.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resolution {
    /// `1`, `Standard`.
    Standard,
    /// `2`, `High (2×)`.
    High,
    /// `5`, `Very high (5×)`.
    VeryHigh,
    /// `10`, `Maximum (10×)`.
    Maximum,
}

impl Resolution {
    /// The stop's number as stored.
    pub fn value(self) -> u32 {
        match self {
            Resolution::Standard => 1,
            Resolution::High => 2,
            Resolution::VeryHigh => 5,
            Resolution::Maximum => 10,
        }
    }

    /// The stop of a stored number; any other is an error
    /// (`<value> is not a resolution (1, 2, 5, 10)`).
    pub fn of(value: u32) -> Resolution {
        match value {
            1 => Resolution::Standard,
            2 => Resolution::High,
            5 => Resolution::VeryHigh,
            10 => Resolution::Maximum,
            other => panic!("{other} is not a resolution (1, 2, 5, 10)"),
        }
    }
}

/// A message the user sends (`POST /api/conversations/<id>/chat`'s body:
/// `text`, `files`, `selections`, `prompts`, `effort`, `resolution`,
/// `editOf`). `provider` and `model` travel with every chat message, as on
/// the page, in [`Target::Chat`] or [`Target::NewChat`]; the route sets the
/// chat's provider, model, effort and resolution from each message
/// (`server/index.js` `handleChat`: `conversation.provider = provider`,
/// `conversation.model = model`).
#[derive(Clone, Debug)]
pub struct Outgoing {
    /// Where it goes.
    pub target: Target,
    /// The text, trimmed.
    pub text: String,
    /// Files attached, from the picker or pasted, read and (pictures)
    /// redrawn by the interface as today's page reads them (`files`: up to
    /// the limits of `state::upload`, a courtesy before the send); the core
    /// stores them. The route's refusals come back as `Update::SendRefused`.
    pub attachments: Vec<OutgoingFile>,
    /// The viewer's selections.
    pub selections: Vec<Selection>,
    /// The library prompts it came from.
    pub prompts: Vec<PromptLink>,
    /// The effort: none is the model's default (`''`).
    pub effort: Option<String>,
    /// The resolution.
    pub resolution: Resolution,
    /// The user message this one replaces (`editOf`): edit and branch, and
    /// retry (the same text again from that message).
    pub edit_of: Option<MessageId>,
}

// chat (stage four's chat agent): SEAM CHANGE, `Outgoing::attachments`
// carries the files' bytes (today's `files: [{ name, type, size, data }]`)
// instead of paths: the interface reads every file to count it toward the
// limits, redraw a picture and show it, and a pasted file or a copied
// message pasted back has no path.
/// A file a message carries (`POST .../chat` `files[]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutgoingFile {
    /// `name`: as the user gave it (a redrawn `a.png` keeps its name).
    pub name: String,
    /// `type`: `image/jpeg` for a redrawn picture, else the file's type
    /// (`application/octet-stream` when unknown).
    pub content_type: String,
    /// `size`: the file's own size in bytes, before any redrawing.
    pub size: u64,
    /// The bytes to store (the redrawn JPEG for a picture).
    pub data: Arc<[u8]>,
}

// pane (stage four's pane agent): the live view's seam, agreed by the
// three sessions (`ui-seam-viewer.md`); see the module's "The live view".
pub use mcbench_render::live::{
    CameraInput, CameraMode, CameraState, DragButton, Frame, LiveFacts, LivePane, OverlayBox,
    Overlays, PaneSize, PaneStatus, Pick, PlayerView, TimeOfDay, ViewKey, keys as camera_keys,
};
pub use mcbench_world::selection::SelectionDescription;

/// The window's handle on the 3D pane, published once by the core when the
/// renderer has started ([`Update::LivePane`]); the window keeps it for the
/// app's life.
#[derive(Clone)]
pub struct PaneHandle(pub Arc<dyn LivePane>);

impl std::fmt::Debug for PaneHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LivePane")
    }
}

/// What the live world does ([`Command::Live`]); the page's toolbar and the
/// view it asks for (`web/live-world.mjs`, the core worker's `place`,
/// `play`, `pause`, `step`, `restart`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveCommand {
    /// Gives the live world a view, or none (the prompt library, Settings):
    /// the only thing that places a world (`Open` places nothing). Ends in
    /// [`Update::Live`] at each phase: `Preparing`, then `Shown` (playing,
    /// tick 0) or `Failed` (exception one: the sentence in the phase, no
    /// `Update::Error`). `afresh` places the world anew even when the same
    /// view is shown (`Try again`, a stopped world). The view already on
    /// show without `afresh` is brought to its latest version with no
    /// `Preparing`; a new view asked while a placement is in flight waits
    /// behind it. The renderer frames a view's first placement at once, a
    /// turn that moved the bounds over 700 ms, else keeps the camera; a
    /// bench view is looked at once more 250 ms after a change. With no
    /// renderer joined every `Show` ends in `Failed` with the sentence of
    /// [`Update::LivePane`]'s `Err`.
    Show {
        /// `"<chat>"` or `"bench:<bench>:<round>"`; none ends the view.
        view: Option<ViewKey>,
        /// Place anew.
        afresh: bool,
    },
    /// Ticks again: [`Update::Live`] with `Running::Playing` at once.
    Play,
    /// Stops ticking: [`Update::Live`] with `Running::Paused` at once.
    Pause,
    /// One tick while paused: [`Update::Live`] with the tick after it,
    /// unthrottled, once its patch is handed to the renderer.
    Step,
    /// Places the world again at tick 0: [`Update::Live`] with tick 0 and
    /// no `Preparing` (a paused world stays paused, a standing player
    /// stands again where it stood); a failed restart is exception one.
    /// A stopped world takes a `Restart` (it places the world again, as
    /// the page's `restart` and the core do), so the window keeps Restart
    /// enabled while Play, Pause and Step are disabled.
    /// `Play`, `Pause`, `Step` and `Restart` with nothing on show, or
    /// refused, end in `Update::Error` (`There is no live world on show`, or
    /// the page's sentence), then `Update::Live`.
    Restart,
}

/// Input to the live world's engine ([`Command::Viewer`]): a click on a
/// block and player mode. The camera's input goes to the pane, not here.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewerInput {
    /// A click on a block (`web/live-world.mjs` 711 to 722): ends in
    /// [`Update::Clicked`] naming the block and the input taken; its change
    /// arrives as the next tick's patch. A click whose `view` is not the live
    /// world's ends in `Clicked { input: None }` with an empty name (the
    /// window announces nothing); an older `revision` is run at its
    /// coordinates. An engine failure is `Update::Error` with its sentence,
    /// then `Clicked { input: None }`.
    Click {
        /// The view the click was made in.
        view: ViewKey,
        /// The revision it showed.
        revision: u64,
        /// The block, Minecraft's coordinates.
        block: [i32; 3],
        /// The pick's yaw, degrees.
        yaw: f32,
    },
    /// Enters player mode beside the build, from the map camera: ends in
    /// [`Update::Live`] with `PlayerState::Spawning` then `Standing`, or
    /// [`Update::PlayerNote`] with the spawn's error, then `Update::Live`.
    /// The core does not use `yaw` and `pitch`: the player faces the build
    /// from the spot beside it (`besideBuild`); the window takes the look
    /// from the first frame of the player's eye.
    EnterPlayer {
        /// The view.
        view: ViewKey,
        /// The map camera's target.
        target: [f64; 3],
        /// The map camera's heading, radians.
        rotation: f64,
        /// The player's yaw, degrees.
        yaw: f32,
        /// The player's pitch, degrees.
        pitch: f32,
    },
    /// Moves the player (a fly in player mode): as `EnterPlayer`.
    SpawnPlayer {
        /// The view.
        view: ViewKey,
        /// The feet.
        at: [f64; 3],
        /// Yaw, degrees.
        yaw: f32,
        /// Pitch, degrees.
        pitch: f32,
        /// Flying.
        flying: bool,
    },
    /// The player's held keys, the taps since the last, and the look.
    /// Exception two: ends in nothing; the core holds the latest and the
    /// next tick takes `keys | taps` and clears the taps.
    PlayerInput {
        /// The keys held, one bit each (`core/src/player` input bits).
        keys: u32,
        /// Keys pressed and released since the last input.
        taps: u32,
        /// Yaw, degrees.
        yaw: f32,
        /// Pitch, degrees.
        pitch: f32,
    },
    /// Leaves player mode: [`Update::Live`] with `PlayerState::None` after
    /// any spawn on its way.
    LeavePlayer,
}

/// Where the live world is (`web/live-world.mjs` phases).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LivePhase {
    /// No view.
    Idle,
    /// Placing the view's world (the cover's `Preparing …`).
    Preparing,
    /// Shown.
    Shown,
    /// The placement failed: the core's sentence, and whether nothing was
    /// ever shown (the cover) or a world stays shown (the banner).
    Failed {
        /// The sentence.
        error: String,
        /// Nothing was shown of this view.
        nothing_shown: bool,
    },
}

/// Whether the shown world ticks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Running {
    /// Ticking.
    Playing,
    /// Paused.
    Paused,
    /// The engine stopped it: `The live world stopped: …`; only a fresh
    /// placement (`Restart`, `Show { afresh: true }`) runs it again.
    Stopped(String),
}

/// The player of player mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    /// None.
    None,
    /// On its way.
    Spawning,
    /// Standing in the world.
    Standing,
}

/// The live world as the core holds it ([`Update::Live`]).
#[derive(Clone, Debug, PartialEq)]
pub struct LiveState {
    /// The view; none: no live world.
    pub view: Option<ViewKey>,
    /// The revision shown.
    pub revision: Option<u64>,
    /// The phase.
    pub phase: LivePhase,
    /// Whether it ticks; none before it is shown.
    pub running: Option<Running>,
    /// Ticks since the world was placed. While running, published at most
    /// every 250 ms with the last count always sent.
    pub tick: u64,
    /// The player.
    pub player: PlayerState,
    /// The box of the world's blocks, Minecraft's coordinates.
    pub bounds: Option<[[i32; 3]; 2]>,
    /// The engine's y offset; a selection is named in build coordinates
    /// (y minus it), as the page does.
    pub y_offset: i32,
    /// The blocks of the stored world (share's and the status line's count).
    pub blocks: u64,
    /// A bench round's builds: each member's id (its chat) and the box of
    /// its build in the round's world, in order (`mcbench_world::bench`'s
    /// `Placed`), in Minecraft's coordinates as every box of this seam
    /// (`y_offset` included); the window names them.
    pub layout: Vec<(String, [[i32; 3]; 2])>,
}
// end of the pane agent's block

/// A chat or a bench, for the actions both have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// A Build chat.
    Chat(String),
    /// A bench.
    Bench(String),
}

/// The window's own preferences, which the core keeps in
/// `preferences.json` in the data folder ([`Preferences::to_text`],
/// [`Preferences::from_text`]; no file is the first run and gives the
/// defaults; a field the file lacks or cannot give takes its default and an
/// unknown one is ignored, so an update never costs the preferences; a file
/// that is not a JSON object is an `Update::Error` naming it, then the
/// defaults): today's `localStorage` keys, and the place and the
/// window the native app remembers. The binary reads them at start ([`Core::preferences`]);
/// the window saves them whole when one changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    /// `mcb-theme`: light, or dark (the default).
    pub light: bool,
    /// `mcb-sidebar`.
    pub sidebar_open: bool,
    /// `mcb-sidebar-width`.
    pub sidebar_width: f32,
    /// `mcb-chat-width`: none is the default `minmax(360px, 42%)`.
    pub chat_width: Option<f32>,
    /// `mcb-binds`: an action's id to its combination, over the defaults.
    pub binds: BTreeMap<String, String>,
    /// `mcb-app-mode`: `bench` or `build`, the landing's mode.
    pub bench_mode: bool,
    /// `mcb-last-model`: provider and model.
    pub last_model: Option<(String, String)>,
    /// `mcb-last-resolution`.
    pub last_resolution: Resolution,
    /// The place open at quit, in today's address syntax.
    pub place: Option<String>,
    /// The window's display and bounds at quit.
    pub window: Option<state::layout::Placement>,
    // sidebar agent: what the page keeps of the sidebar's folds.
    /// `mcb-open-benches`: the benches folded open in the sidebar.
    pub open_benches: Vec<String>,
    /// `mcb-prompts-open`: the prompts section is unfolded (the default).
    pub prompts_open: bool,
    // pane agent: the live view's Day/Night switch (BlueMap keeps it in the
    // browser's storage; the owner: "Keep Day/Night").
    /// The live view draws night (`TimeOfDay::Night`); day by default.
    pub night: bool,
    // bench agent: what the page keeps of Bench and the prompt editor.
    /// `mcb-bench-models`: the new bench's line-up.
    pub bench_models: Vec<BenchChoice>,
    /// `mcb-bench-blind`: the new bench is blind (the default).
    pub bench_blind: bool,
    /// `mcb-bench-resolution`: the new bench's resolution.
    pub bench_resolution: Resolution,
    /// `mcb-prompt-ai-model`: the editor's generation model and effort.
    pub prompt_ai_model: Option<BenchChoice>,
    /// `mcb-improve-source`: `note` (write from a note) rather than `text`.
    pub improve_source_note: bool,
}

impl Preferences {
    /// The preferences as the core keeps them (`preferences.json` in the
    /// data folder): one JSON object with every field.
    pub fn to_text(&self) -> String {
        let window = self.window.as_ref().map(|w| {
            serde_json::json!({
                "display": w.display, "x": w.bounds.x, "y": w.bounds.y,
                "width": w.bounds.width, "height": w.bounds.height,
            })
        });
        serde_json::json!({
            "light": self.light,
            "sidebarOpen": self.sidebar_open,
            "sidebarWidth": self.sidebar_width,
            "chatWidth": self.chat_width,
            "binds": self.binds,
            "benchMode": self.bench_mode,
            "lastModel": self.last_model.as_ref().map(|(p, m)| serde_json::json!([p, m])),
            "lastResolution": self.last_resolution.value(),
            "place": self.place,
            "window": window,
            "openBenches": self.open_benches,
            "promptsOpen": self.prompts_open,
            "night": self.night,
            "benchModels": self.bench_models.iter().map(choice_json).collect::<Vec<_>>(),
            "benchBlind": self.bench_blind,
            "benchResolution": self.bench_resolution.value(),
            "promptAiModel": self.prompt_ai_model.as_ref().map(choice_json),
            "improveSource": if self.improve_source_note { "note" } else { "text" },
        })
        .to_string()
    }

    /// The preferences of a `preferences.json`, each field read on its own
    /// (as the page reads each `localStorage` key): a field the text lacks,
    /// or one whose value cannot be read, takes [`Preferences::default`]'s
    /// value; a field this build does not know is ignored, so a file of an
    /// earlier or a later build loads. Only text that is not a JSON object
    /// is an error. [`Preferences::from_text_noting`] also names the fields
    /// whose value could not be read.
    pub fn from_text(text: &str) -> Result<Preferences, String> {
        Preferences::from_text_noting(text).map(|(preferences, _)| preferences)
    }

    /// [`Preferences::from_text`], and the fields present in the text whose
    /// value could not be read (wholly, or an entry of a collection that was
    /// dropped), in the order they are read, for the caller to log.
    pub fn from_text_noting(text: &str) -> Result<(Preferences, Vec<String>), String> {
        use serde_json::Value;
        let value: Value =
            serde_json::from_str(text).map_err(|e| format!("the preferences are not JSON: {e}"))?;
        let object = value
            .as_object()
            .ok_or("the preferences are not a JSON object")?;
        let d = Preferences::default();
        let mut noted = Vec::new();
        let p = Preferences {
            light: one(object, "light", &mut noted, d.light, Value::as_bool),
            sidebar_open: one(
                object,
                "sidebarOpen",
                &mut noted,
                d.sidebar_open,
                Value::as_bool,
            ),
            sidebar_width: one(object, "sidebarWidth", &mut noted, d.sidebar_width, number),
            chat_width: one(object, "chatWidth", &mut noted, d.chat_width, |v| {
                nullable(v, number)
            }),
            binds: match object.get("binds") {
                None => d.binds,
                Some(Value::Object(entries)) => {
                    let kept: BTreeMap<String, String> = entries
                        .iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect();
                    if kept.len() < entries.len() {
                        noted.push("binds".to_string());
                    }
                    kept
                }
                Some(_) => {
                    noted.push("binds".to_string());
                    d.binds
                }
            },
            bench_mode: one(
                object,
                "benchMode",
                &mut noted,
                d.bench_mode,
                Value::as_bool,
            ),
            last_model: one(object, "lastModel", &mut noted, d.last_model, |v| match v {
                Value::Null => Some(None),
                Value::Array(pair) if pair.len() == 2 => Some(Some((
                    pair[0].as_str()?.to_string(),
                    pair[1].as_str()?.to_string(),
                ))),
                _ => None,
            }),
            last_resolution: one(
                object,
                "lastResolution",
                &mut noted,
                d.last_resolution,
                resolution,
            ),
            place: one(object, "place", &mut noted, d.place, |v| {
                nullable(v, |v| v.as_str().map(str::to_string))
            }),
            window: one(object, "window", &mut noted, d.window, |v| {
                nullable(v, |v| {
                    let w = v.as_object()?;
                    let keys = ["display", "x", "y", "width", "height"];
                    if w.len() != keys.len() || !keys.iter().all(|k| w.contains_key(*k)) {
                        return None;
                    }
                    Some(state::layout::Placement {
                        display: w["display"].as_str()?.to_string(),
                        bounds: state::layout::Rect {
                            x: number(&w["x"])?,
                            y: number(&w["y"])?,
                            width: number(&w["width"])?,
                            height: number(&w["height"])?,
                        },
                    })
                })
            }),
            open_benches: list(object, "openBenches", &mut noted, d.open_benches, |v| {
                v.as_str().map(str::to_string)
            }),
            prompts_open: one(
                object,
                "promptsOpen",
                &mut noted,
                d.prompts_open,
                Value::as_bool,
            ),
            night: one(object, "night", &mut noted, d.night, Value::as_bool),
            bench_models: list(object, "benchModels", &mut noted, d.bench_models, choice),
            bench_blind: one(
                object,
                "benchBlind",
                &mut noted,
                d.bench_blind,
                Value::as_bool,
            ),
            bench_resolution: one(
                object,
                "benchResolution",
                &mut noted,
                d.bench_resolution,
                resolution,
            ),
            prompt_ai_model: one(
                object,
                "promptAiModel",
                &mut noted,
                d.prompt_ai_model,
                |v| nullable(v, choice),
            ),
            improve_source_note: one(
                object,
                "improveSource",
                &mut noted,
                d.improve_source_note,
                |v| match v.as_str()? {
                    "text" => Some(false),
                    "note" => Some(true),
                    _ => None,
                },
            ),
        };
        Ok((p, noted))
    }
}

/// The field `name` of the preferences read by `parse`: the default when
/// the text lacks it, and when `parse` cannot read it (then `name` is noted).
fn one<T>(
    object: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    noted: &mut Vec<String>,
    default: T,
    parse: impl FnOnce(&serde_json::Value) -> Option<T>,
) -> T {
    let Some(value) = object.get(name) else {
        return default;
    };
    parse(value).unwrap_or_else(|| {
        noted.push(name.to_string());
        default
    })
}

/// The list `name` of the preferences, each entry read by `entry`: an entry
/// that cannot be read is dropped and the rest kept; not a list is the
/// default. Either way `name` is noted.
fn list<T>(
    object: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    noted: &mut Vec<String>,
    default: Vec<T>,
    entry: impl Fn(&serde_json::Value) -> Option<T>,
) -> Vec<T> {
    let Some(value) = object.get(name) else {
        return default;
    };
    let Some(entries) = value.as_array() else {
        noted.push(name.to_string());
        return default;
    };
    let kept: Vec<T> = entries.iter().filter_map(entry).collect();
    if kept.len() < entries.len() {
        noted.push(name.to_string());
    }
    kept
}

/// Null as none, anything else read by `parse`.
fn nullable<T>(
    value: &serde_json::Value,
    parse: impl FnOnce(&serde_json::Value) -> Option<T>,
) -> Option<Option<T>> {
    match value {
        serde_json::Value::Null => Some(None),
        v => parse(v).map(Some),
    }
}

/// A number of the preferences.
fn number(value: &serde_json::Value) -> Option<f32> {
    value.as_f64().map(|x| x as f32)
}

/// A resolution stop of the preferences: 1, 2, 5 or 10.
fn resolution(value: &serde_json::Value) -> Option<Resolution> {
    match value.as_u64()? {
        r @ (1 | 2 | 5 | 10) => Some(Resolution::of(r as u32)),
        _ => None,
    }
}

/// A bench model of the preferences: exactly `{ provider, model, effort }`,
/// the effort a string or null.
fn choice(value: &serde_json::Value) -> Option<BenchChoice> {
    let o = value.as_object()?;
    if o.len() != 3 {
        return None;
    }
    let effort = match o.get("effort")? {
        serde_json::Value::Null => None,
        e => Some(e.as_str()?.to_string()),
    };
    Some(BenchChoice {
        provider: o.get("provider")?.as_str()?.to_string(),
        model: o.get("model")?.as_str()?.to_string(),
        effort,
    })
}

/// A bench model as the preferences keep it: `{ provider, model, effort }`.
fn choice_json(c: &BenchChoice) -> serde_json::Value {
    serde_json::json!({ "provider": c.provider, "model": c.model, "effort": c.effort })
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            light: false,
            sidebar_open: true,
            sidebar_width: state::layout::SIDEBAR_WIDTH,
            chat_width: None,
            binds: BTreeMap::new(),
            bench_mode: false,
            last_model: None,
            last_resolution: Resolution::Standard,
            place: None,
            window: None,
            open_benches: Vec::new(),
            prompts_open: true,
            night: false,
            bench_models: Vec::new(),
            bench_blind: true,
            bench_resolution: Resolution::Standard,
            prompt_ai_model: None,
            improve_source_note: false,
        }
    }
}

/// What the window sends the core.
#[derive(Clone, Debug)]
pub enum Command {
    /// Sends a message, starting a turn (`POST .../chat`; a new chat's row
    /// first, `POST /api/conversations`). Publishes `Update::TurnJoined`
    /// together with the turn's first event (not at once: it comes just
    /// before that event), the chat's `Update::Transcript` as stored at the
    /// turn's start (the message in it), the turn's `Update::Turn` events,
    /// and at the end `TurnEvent::Done` or `TurnEvent::Error`, the
    /// `Transcript`, then `Update::Conversations`. Every refusal before the
    /// turn starts is `Update::SendRefused` with the core's sentence: the
    /// route's 400, 409, 503 and 507 texts, a chat not found, a new chat
    /// refused at its creation, and any internal failure.
    Send(Outgoing),
    /// Stops the chat's turn (`POST .../stop`). With no turn running it
    /// publishes nothing. A stopped turn's events end with
    /// `TurnEvent::Assistant` (`(stopped by user)`), then `TurnEvent::Done`,
    /// then the chat's `Transcript` and `Conversations`, as any turn's end.
    Stop {
        /// The chat.
        chat: String,
    },
    /// Shows another branch: the one that holds this message
    /// (`POST .../branch` `{ messageId }`). Publishes the chat's
    /// `Update::Transcript` (the new branch); on a failure `Update::Error`
    /// with the route's text, then the unchanged `Transcript`, or
    /// `Update::TranscriptFailed` when the chat can no longer be read.
    Branch {
        /// The chat.
        chat: String,
        /// A message of the branch.
        message: MessageId,
    },
    /// Opens a chat (`GET /api/conversations/<id>`): publishes its
    /// `Update::Transcript`, or `Update::TranscriptFailed` when it cannot be
    /// read, and nothing else; it marks nothing seen (the window sends
    /// `Command::MarkSeen`). An `Update::Conversations` that follows any
    /// command is the coalesced list, and can arrive later than the
    /// `Transcript`. It joins no running turn: the window joins with
    /// `Command::JoinTurn`.
    Open {
        /// The chat.
        chat: String,
    },
    /// Joins a running turn (`GET .../stream`): `Update::TurnJoined`, then
    /// the chat's `Update::Transcript` as stored at the turn's start, then
    /// every event of the turn so far, then the live ones, each once (the
    /// core keeps this order; the window does not de-duplicate); a join
    /// before the turn's first event publishes its `TurnJoined` together
    /// with that event, not at once; or
    /// `Update::TurnNotRunning` when it has ended (also after its last
    /// event, while the chat is still being released), on which the window
    /// `Open`s the chat again, once. A turn ends with `TurnEvent::Done` or
    /// `TurnEvent::Error`, then the chat's `Transcript` (its header's
    /// `running_since` none), then `Update::Conversations` with its
    /// `running_since` none; no list showing the chat idle comes before
    /// that transcript. A turn the window did not send (a bench round)
    /// reaches it with no `TurnJoined` unless the window joins it.
    JoinTurn {
        /// The chat.
        chat: String,
    },
    /// Marks a chat seen (`POST .../seen`), the only command that does. The
    /// window sends it where the page marks seen (`app.md` 2.7 and 7.7,
    /// [`state::seen`]), each once per change: the open chat while it is
    /// unread and the window is visible (on opening it, when it turns
    /// unread while open, and when the window becomes visible again); at a
    /// turn's end while the window is visible; and on a bench's comparison,
    /// for each unread model. Publishes `Update::Conversations` (then
    /// `Update::Archive` when the archive was read), after `Update::Error`
    /// on a failure.
    MarkSeen {
        /// The chat.
        chat: String,
    },
    /// Dismisses a stored error (`POST .../dismiss-error` `{ messageId }`):
    /// the chat's `Update::Transcript`; on a failure `Update::Error`, then
    /// the unchanged `Transcript`, or `Update::TranscriptFailed`.
    DismissError {
        /// The chat.
        chat: String,
        /// The error message.
        message: MessageId,
    },
    /// Archives a chat or bench (`POST .../archive`):
    /// `Update::Conversations` first, then `Update::Archive(Ok)` when the
    /// archive was read (after `Update::Error` on a failure).
    Archive(Subject),
    /// Restores one (`POST .../restore`): as `Archive`.
    Restore(Subject),
    /// Deletes an archived one for good (`DELETE /api/conversations/<id>`,
    /// `DELETE /api/benches/<id>`): as `Archive`.
    Delete(Subject),
    /// Reads the archive (`GET /api/archive`): `Update::Archive`, `Ok` or
    /// the `Err` with the core's sentence.
    ReadArchive,
    /// Searches (`GET /api/search?scope=&q=`): `Update::Found` with the
    /// same `request`, so an answer to an older query can be told apart;
    /// its results `Ok` or the `Err` with the core's sentence.
    Search {
        /// Which list.
        scope: SearchScope,
        /// The query.
        query: String,
        /// The caller's number for this query.
        request: u64,
    },
    /// Reads a provider's models (`GET /api/models?provider=`):
    /// `Update::Models`, `Ok` or the `Err` with the core's sentence.
    Models {
        /// The provider.
        provider: String,
    },
    /// Reads a model's capabilities (`GET /api/model-info`):
    /// `Update::ModelInfo`, `Ok` or the `Err` with the core's sentence.
    ModelInfo {
        /// The provider.
        provider: String,
        /// The model.
        model: String,
    },
    /// Saves the starred models (`PUT /api/model-favorites`):
    /// `Update::Favorites` with what it kept; on a failure `Update::Error`
    /// (`The star was not saved, and went back: …`), then the unchanged
    /// `Favorites`.
    SetFavorites(Vec<(String, String)>),
    /// Saves the window's preferences: nothing, or `Update::Error` naming
    /// the file on a failure.
    SavePreferences(Preferences),
    /// Input to the live world's engine: a click on a block and player mode
    /// (each [`ViewerInput`] names its ending).
    Viewer(ViewerInput),
    // sidebar agent: the prompt library's titles for the sidebar and the
    // palette (a seam addition, proposed to stage three).
    /// Reads the prompt library (`GET /api/prompts`): `Update::Prompts`,
    /// `Ok` or the `Err` with the core's sentence. The core also publishes
    /// `Prompts` unasked after every `prompt_library` tool result of a
    /// turn, a read-only or failed call included; the window applies it.
    ReadPrompts,
    // settings agent: Settings, sign-in, CLI installs, setup, the vote
    // count and the published builds (`ui-seam-settings.md`, agreed with
    // stage three 2026-11-17 18:20). It replaces `Settings(Value)`.
    /// Reads the settings (`GET /api/settings`, with `refresh` its
    /// `?refresh=1`: logins, keys and CLI versions read again):
    /// `Update::Settings`; with `refresh`, `Update::Providers` first when
    /// the read worked.
    ReadSettings {
        /// Read logins, keys and versions again (`Check again`).
        refresh: bool,
    },
    /// Saves the whole draft (`PUT /api/settings`): `Update::Providers`,
    /// `Update::Favorites`, then `Update::SettingsSaved(Ok(()))`; a refusal
    /// is `Update::SettingsSaved(Err(text))` alone (the text belongs in the
    /// panel).
    SaveSettings(SettingsChange),
    /// Starts a provider's sign-in (`POST /api/provider-logins`):
    /// `Update::SignIn`; a refusal is `Update::Error`, then `SignIn` with
    /// the unchanged state.
    SignIn {
        /// The provider.
        provider: String,
        /// `claudeai` or `console` for Claude Code; none for the others.
        method: Option<String>,
    },
    /// The code pasted into a sign-in that asks for one
    /// (`POST /api/provider-logins/:provider/input`): as `SignIn`.
    SignInInput {
        /// The provider.
        provider: String,
        /// The pasted code.
        text: String,
    },
    /// Cancels a sign-in (`DELETE /api/provider-logins/:provider`): as
    /// `SignIn`.
    CancelSignIn {
        /// The provider.
        provider: String,
    },
    /// Signs out (`POST /api/provider-logouts`): `Update::Providers`, then
    /// `Update::Settings` read fresh; `Update::Error` first on a failure.
    SignOut {
        /// The provider.
        provider: String,
    },
    /// Installs a CLI (`POST /api/cli-installs`): `Update::Install`. A
    /// refusal is `Update::Error`, then `Update::Install` when an install of
    /// that CLI exists, then fresh `Update::Settings` as the last update.
    InstallCli {
        /// `claude` or `codex`.
        program: String,
    },
    /// Tries the failed setup steps again (`POST /api/setup/retry`):
    /// `Update::Setup`; `No setup step failed` is `Update::Error` first.
    RetrySetup,
    /// Reads the leaderboard (`GET /api/leaderboard`):
    /// `Update::Leaderboard`.
    ReadLeaderboard,
    /// Deletes every vote (`DELETE /api/votes`): `Update::Leaderboard`,
    /// `Update::Error` first on a failure.
    ClearVotes,
    /// Reads the published builds (`GET /api/shares`): `Update::Shares`.
    ReadShares,
    /// Takes a published build down (`DELETE /api/shares/:shareId`):
    /// `Update::ShareRemoved`, then `Update::Shares` when it worked.
    RemoveShare {
        /// Its share id.
        share_id: String,
    },
    // settings agent: installing WSL from the setup screen
    // (`ui-seam-wsl-install.md`, Windows only).
    /// Installs WSL (`wsl.exe --install --no-distribution`, elevated:
    /// Windows asks for an administrator's permission): `Update::Setup` at
    /// each change of the shell's step (running while the installer runs,
    /// then needing a restart, ready, or failed); `Update::Error` first on a
    /// failure to start it.
    InstallWsl,
    /// Restarts the computer (the app registered to start once after the
    /// next sign-in; an ordinary restart, never forced). Only a refusal of
    /// the restart request itself is `Update::Error`, then `Update::Setup`.
    /// When Windows accepts the request and a program with unsaved work then
    /// holds it up, or the person cancels it there, the app is told nothing
    /// and nothing is published: the shell's step still needs the restart,
    /// and its row still offers it.
    RestartComputer,
    // pane agent: the live view's commands (`ui-seam-viewer.md`).
    /// The live world: its view and its toolbar (each [`LiveCommand`] names
    /// its ending).
    Live(LiveCommand),
    /// Names a selection box (`POST /api/conversations/<id>/selection`
    /// `{ min, max }`), in build coordinates: a read,
    /// [`Update::SelectionDescribed`] with the same `request` and its
    /// `Result` (the route's 404 and 400 sentences as the `Err`;
    /// `Minecraft's data is not ready yet` before setup gave the tables).
    DescribeSelection {
        /// The chat.
        chat: String,
        /// The caller's number for this box.
        request: u64,
        /// The lowest corner.
        min: [i64; 3],
        /// The highest corner.
        max: [i64; 3],
    },
    /// Writes the chat's world as a WorldEdit schematic to `to`
    /// (`GET /api/conversations/<id>/schematic`), through a temporary file
    /// beside it renamed over it: [`Update::Exported`] (`Err` with
    /// `Minecraft's data is not ready yet` before setup gave the tables).
    ExportSchematic {
        /// The chat.
        chat: String,
        /// The file the save dialog named.
        to: PathBuf,
    },
    /// Writes the live world as a Minecraft world (a zip of region files,
    /// today's `web/world-export.mjs`) to `to`, as `ExportSchematic`:
    /// [`Update::Exported`]; an empty world's is `Err` with `There is no live
    /// world on show`. The zip's dates use the machine's UTC offset at the
    /// moment of export.
    ExportWorld {
        /// The view.
        view: ViewKey,
        /// The world's name inside the zip (`level.dat`'s `LevelName`).
        name: String,
        /// The file the save dialog named.
        to: PathBuf,
    },
    // bench agent: Bench and the prompt library (`ui-seam-bench-prompts.md`,
    // a seam addition proposed to stage three). A bench's rounds are sent
    // with `Command::Send` and its two targets.
    /// Reads a bench's details (`GET /api/benches/<id>`, `Core::bench`):
    /// `Update::Bench`, `Ok` or the `Err` with the core's sentence.
    ReadBench {
        /// The bench.
        bench: String,
    },
    /// Votes on a round of a blind bench (`POST /api/benches/<id>/vote`,
    /// `Core::vote`): `Update::Bench` with the votes, then
    /// `Update::Leaderboard`; a refusal is `Update::Error`, then the
    /// unchanged `Update::Bench`.
    Vote {
        /// The bench.
        bench: String,
        /// The round's id.
        round: String,
        /// The pick.
        pick: VotePick,
    },
    /// Reveals the models' names for good (`POST /api/benches/<id>/reveal`,
    /// `Core::reveal`): `Update::Conversations`, then `Update::Bench`;
    /// `Update::Error` first on a failure.
    Reveal {
        /// The bench.
        bench: String,
    },
    /// Follows another version of a round (`POST /api/benches/<id>/branch`
    /// `{ roundId }`, `Core::bench_branch`): `Update::Conversations`, then
    /// `Update::Bench` on the new line; a refusal is `Update::Error`, then
    /// `Update::Conversations`, then the unchanged `Update::Bench`.
    BenchBranch {
        /// The bench.
        bench: String,
        /// The round to follow.
        round: String,
    },
    /// Runs a failed or stopped model's round again
    /// (`POST /api/benches/<id>/retry` `{ memberId }`, `Core::retry_member`):
    /// `Update::Conversations`; `Update::Error` first on a refusal.
    RetryMember {
        /// The bench.
        bench: String,
        /// The model's chat.
        member: String,
    },
    /// Continues a bench model's chat in Build
    /// (`POST /api/conversations/<id>/fork` `{ round }`, `Core::fork`):
    /// `Update::Forked` last; on success after the `Update::Conversations`
    /// that has the new chat.
    Fork {
        /// The model's chat.
        member: String,
        /// The round to continue from, from 1.
        round: usize,
    },
    /// Saves the whole library in its order (`PUT /api/prompts`,
    /// `Core::save_prompts`): `Update::PromptsSaved`, the library as saved
    /// or the refusal's text (the exception to the second rule: the sync
    /// tells its own save's answer from an unasked `Update::Prompts`).
    SavePrompts(Vec<LibraryPrompt>),
    /// Improves a prompt's text or writes one from a note
    /// (`POST /api/prompts/generate`, `Core::generate_prompt`):
    /// `Update::Generated` with the same `request`.
    GeneratePrompt {
        /// The window's number for this request.
        request: u64,
        /// The provider.
        provider: String,
        /// The model.
        model: String,
        /// The effort; none is the model's default.
        effort: Option<String>,
        /// The text to improve, or the note to write from.
        text: String,
    },
    /// Stops a request (the page's abort): its `Update::Generated` with
    /// `Generated::Cancelled`, or nothing when it has ended.
    CancelGenerate {
        /// The request.
        request: u64,
    },
    // par-chat (stage four's parity pass), SEAM ADDITION agreed with the
    // core's owner 2026-11-18 (`ui-seam-parity.md` item 1).
    /// Makes a new chat's row (`POST /api/conversations { provider, model
    /// }`), as the page does at New chat (`createdChat`): ends in exactly
    /// one `Update::ChatCreated`. The core keeps a row with no message out
    /// of `Update::Conversations` (so this publishes no list) and sweeps it
    /// by the store's 7-day rule; it remembers nothing for the window (two
    /// commands make two rows: reusing an unused one is the window's).
    CreateChat {
        /// The provider's key.
        provider: String,
        /// The model.
        model: String,
    },
    // par-chat, SEAM ADDITION (`ui-seam-parity.md` item 2).
    /// Reads the providers again (`GET /api/config`, the page's
    /// `loadConfig` behind `Try again`): ends in exactly one
    /// `Update::Providers`, `Ok` or `Err`; it publishes neither the
    /// favourites nor the settings.
    ReadProviders,
    // share agent (stage four's parity pass), SEAM ADDITION agreed with the
    // core's owner 2026-11-18 (`ui-seam-share.md`): the Share panel's
    // commands, by subject (a Build chat or a bench). Settings' list keeps
    // `ReadShares` and `RemoveShare`.
    /// Reads a subject's share (`GET /api/conversations/<id>/share` or
    /// `GET /api/benches/<id>/share`, with `GET /api/settings`'s
    /// `shareRepo`, which the page reads at each opening): ends in exactly
    /// one `Update::ShareRead` for the subject, `Err` with the route's text
    /// (`Conversation not found`, `Bench not found`, or the site's failure).
    ReadShare {
        /// The chat or bench.
        subject: Subject,
    },
    /// Publishes a subject's build to its public page
    /// (`POST /api/conversations/<id>/share`, `POST /api/benches/<id>/share`,
    /// the NDJSON publish stream): `Update::PublishStep` at each move,
    /// `Exporting` first, ending in exactly one `PublishStep::Live` or
    /// `PublishStep::Failed`. A refusal before the publish starts is
    /// `Failed` alone with the route's text, in the routes' order: the chat
    /// or bench not found, `Set the GitHub repository for shared builds in
    /// Settings`, nothing built (`The build is empty: there is nothing to
    /// share`, `No model in this bench has built anything yet`), a turn
    /// running (`Wait for the running turn before sharing`, `Wait for the
    /// bench's running turns before sharing`), then `This build is already
    /// being published`. No `Update::Error` beside it: the panel shows the
    /// text (the exception to the second rule, as `SendRefused`). A publish
    /// whose task panics or is dropped ends in `Failed` with `The publish
    /// stopped before the page was live`. The first publish fetches
    /// BlueMap's web app for the shared page, and its failure is a `Failed`
    /// too (`BlueMap 5.22 could not be downloaded from GitHub for the
    /// shared page (<the download's error>). Try again.`). Closing the panel or leaving the
    /// place stops nothing: there is no cancel. The page publishes the
    /// subject as stored (a chat's world at its settle ticks, a bench's
    /// latest build of every model): no round or tick.
    Publish {
        /// The chat or bench.
        subject: Subject,
    },
    /// Takes a subject's page down (`DELETE /api/conversations/<id>/share`,
    /// `DELETE /api/benches/<id>/share`): `Update::ShareChanged` with no
    /// url; on a failure `Update::Error` with the route's text (`This build
    /// is not shared`, `Wait for the build to finish publishing`, the site's
    /// failure), then `Update::ShareChanged` with the url it still has.
    Unshare {
        /// The chat or bench.
        subject: Subject,
    },
    // end of the share agent's block
}

/// How a provider signs in (`auth`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auth {
    /// `api-key`.
    ApiKey,
    /// `cli-login`.
    CliLogin,
}

/// A provider as the pickers need it (`GET /api/config` `providers`; no
/// Cursor, which is not in the native app).
#[derive(Clone, Debug, PartialEq)]
pub struct Provider {
    /// Its key.
    pub id: String,
    /// `label`.
    pub label: String,
    /// `connected`.
    pub connected: bool,
    /// `auth`.
    pub auth: Auth,
    /// `harness`.
    pub harness: bool,
    /// `enabled`.
    pub enabled: bool,
    /// `hiddenModels`.
    pub hidden_models: Vec<String>,
    /// `modelOrder`.
    pub model_order: Vec<String>,
}

// settings agent: the types of Settings' seam (`ui-seam-settings.md`, agreed
// with stage three 2026-11-17 18:20). `SignInView`, `InstallView` and
// `Progress` are `mcbench-providers`' own; the rest are this crate's.
pub use mcbench_agent::{InstallView, Progress};
pub use mcbench_agent::{SignInState, SignInView};

/// The settings as the core reads them (`GET /api/settings`).
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsView {
    /// When they were read, ms since the epoch (`Checked 2m ago`).
    pub checked_at: i64,
    /// The model that names new chats; none: each chat's own model.
    pub title_model: Option<TitleModel>,
    /// The repository shared builds publish to; none: sharing off.
    pub share_repo: Option<String>,
    /// The providers, in the core's order (no Cursor).
    pub providers: Vec<ProviderSettings>,
    /// `settings.json` in the app's data folder, where a key is saved.
    pub settings_file: PathBuf,
}

/// The title model (`titleModel`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleModel {
    /// The provider.
    pub provider: String,
    /// The model.
    pub model: String,
    /// The effort; empty is the model's default.
    pub effort: String,
}

/// Where a provider's key comes from (`keySource`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    /// `saved`: in the settings file.
    Saved,
    /// `env`: the provider's variable.
    Env,
}

/// One provider as Settings show it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderSettings {
    /// Its key.
    pub id: String,
    /// `label`.
    pub label: String,
    /// `auth`.
    pub auth: Auth,
    /// `harness`.
    pub harness: bool,
    /// `envKey`: its key's variable (`OPENROUTER_API_KEY`); none for a CLI
    /// login.
    pub env_key: Option<String>,
    /// `connected`.
    pub connected: bool,
    /// `enabled`.
    pub enabled: bool,
    /// `keySource`; the saved key itself never leaves the core.
    pub key_source: Option<KeySource>,
    /// `signedIn`.
    pub signed_in: bool,
    /// `account`.
    pub account: Option<String>,
    /// `plan`.
    pub plan: Option<String>,
    /// `version`.
    pub version: Option<String>,
    /// `summary`: the core's line (`Key saved`, `Signed in · Max`, `CLI
    /// not installed` …), shown as it is.
    pub summary: String,
    /// `hiddenModels`.
    pub hidden_models: Vec<String>,
    /// `modelOrder`.
    pub model_order: Vec<String>,
    /// `runtime`: a CLI harness's binary and config folder.
    pub runtime: Option<Runtime>,
    /// `signIn`: a provider with a sign-in.
    pub sign_in: Option<SignInOffer>,
    /// `endpoint`.
    pub endpoint: Option<String>,
    /// `protocol`.
    pub protocol: String,
}

/// A CLI harness's runtime (`runtime`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runtime {
    /// The program's name (`claude`).
    pub binary: String,
    /// The variable that names it (`MCBENCH_CLAUDE_BINARY`).
    pub binary_env: String,
    /// The variable of its config folder (`CLAUDE_CONFIG_DIR`).
    pub home_env: String,
    /// The binary variable's value, when set.
    pub binary_value: Option<String>,
    /// The config folder in use.
    pub resolved_home: String,
}

/// A provider's sign-in (`signIn`).
#[derive(Clone, Debug, PartialEq)]
pub struct SignInOffer {
    /// Its methods (`claudeai`, `console`); empty: one `Sign in`.
    pub methods: Vec<String>,
    /// The CLI it needs.
    pub program: Program,
    /// The CLI is on this machine.
    pub installed: bool,
    /// The latest sign-in.
    pub current: Option<SignInView>,
    /// The program's latest install.
    pub install: Option<InstallView>,
}

/// A CLI the app installs (`program`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// `claude` or `codex`.
    pub name: String,
    /// `Claude Code 2.1.311`.
    pub title: String,
    /// The download's size in bytes.
    pub bytes: u64,
}

/// The whole draft, saved (`PUT /api/settings`'s body).
#[derive(Clone, PartialEq, Eq)]
pub struct SettingsChange {
    /// `titleModel`.
    pub title_model: Option<TitleModel>,
    /// `shareRepo`.
    pub share_repo: Option<String>,
    /// Each provider's editable fields.
    pub providers: Vec<ProviderChange>,
}

impl std::fmt::Debug for SettingsChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsChange")
            .field("title_model", &self.title_model)
            .field("share_repo", &self.share_repo)
            .field("providers", &self.providers)
            .finish()
    }
}

/// One provider's editable fields.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderChange {
    /// The provider.
    pub id: String,
    /// `enabled`.
    pub enabled: bool,
    /// A key typed in Settings; empty keeps the one saved (a secret: its
    /// `Debug` prints `***`).
    pub api_key: String,
    /// `hiddenModels`.
    pub hidden_models: Vec<String>,
    /// `modelOrder`.
    pub model_order: Vec<String>,
}

impl std::fmt::Debug for ProviderChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderChange")
            .field("id", &self.id)
            .field("enabled", &self.enabled)
            .field("api_key", &"***")
            .field("hidden_models", &self.hidden_models)
            .field("model_order", &self.model_order)
            .finish()
    }
}

/// The published builds (`GET /api/shares`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    /// The repository; none: no repository set.
    pub repo: Option<String>,
    /// The builds.
    pub builds: Vec<PublishedBuild>,
}

/// One published build (today's fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedBuild {
    /// `shareId`.
    pub share_id: String,
    /// `title`.
    pub title: String,
    /// `kind`: `chat` or `bench`.
    pub kind: String,
    /// `labels`: a chat's model, a bench's builds.
    pub labels: Vec<String>,
    /// `url`: its page.
    pub url: String,
    /// `publishedAt`: ms since the epoch.
    pub published_at: i64,
    /// `conversationId`: none once the chat is deleted.
    pub conversation_id: Option<String>,
    /// `benchId`: none once the bench is deleted.
    pub bench_id: Option<String>,
    /// `archived`.
    pub archived: bool,
}

/// The leaderboard (`GET /api/leaderboard`).
#[derive(Clone, Debug, PartialEq)]
pub struct Leaderboard {
    /// Every vote.
    pub votes: u64,
    // bench agent: the ranking (`entries`, the core's order).
    /// The models ranked by blind votes.
    pub entries: Vec<LeaderboardEntry>,
}

/// What the core publishes to the window.
#[derive(Clone, Debug)]
pub enum Update {
    /// The list, after start and after every change of it, a turn's start
    /// and end included (a line of `GET /api/conversations/stream`:
    /// `listPayload`).
    Conversations(ConversationList),
    /// A chat's stored transcript (`GET /api/conversations/<id>`, a turn's
    /// `turn_snapshot`, `POST .../branch`'s answer): its header, the
    /// messages of its active branch as stored, and its branch tree.
    Transcript {
        /// The chat.
        chat: String,
        /// Its title, model, effort and running turn.
        header: ConversationHeader,
        /// The messages, oldest first.
        messages: Vec<StoredMessage>,
        /// Every branch's messages (`messageTree`).
        tree: MessageTree,
    },
    /// A chat could not be read (`This chat could not be read: …`).
    TranscriptFailed {
        /// The chat.
        chat: String,
        /// Why.
        error: String,
    },
    /// One event of a running turn, as it streams. The core publishes
    /// every running turn's events, for every chat (bench rounds and chats
    /// running in the background included); the window applies a chat's
    /// only after its `TurnJoined` and drops the rest.
    Turn {
        /// The chat.
        chat: String,
        /// The event.
        event: TurnEvent,
    },
    /// A turn was joined (`Command::JoinTurn`, or a send the core took):
    /// its start. The window resets the chat's turn view: the
    /// `Transcript` that follows is the chat at the turn's start, then the
    /// events so far and the live ones (also when the window's own send
    /// began the turn and it joins again).
    TurnJoined {
        /// The chat.
        chat: String,
        /// The turn's start, epoch ms.
        since: i64,
    },
    /// No turn runs for the chat (`404 No turn is running`): it ended; the
    /// window reads the chat again.
    TurnNotRunning {
        /// The chat.
        chat: String,
    },
    /// A send the core refused before its turn began: the draft goes back
    /// and the text is shown.
    SendRefused {
        /// Where it was going.
        target: Target,
        /// The route's text.
        error: String,
    },
    /// The archive (`GET /api/archive`), or why it could not be read (the
    /// archived section's `Archived items failed to load.`).
    Archive(Result<Archive, String>),
    /// A search's results (`GET /api/search`), or why the search failed
    /// (a read's answer carries a `Result`: the palette and the sidebar's
    /// search show the sentence where their results would be).
    Found {
        /// The request it answers.
        request: u64,
        /// The results, best first; a snippet marks its matches with
        /// `\u{1}` and `\u{2}` (split by `state::seam::snippet_parts`).
        results: Result<Vec<SearchHit>, String>,
    },
    /// The providers (`GET /api/config` `providers`), at attach and when
    /// Settings change them, and `Command::ReadProviders`' end.
    // par-chat (stage four's parity pass), SEAM CHANGE agreed with the
    // core's owner 2026-11-18 (`ui-seam-parity.md` item 2): a read that
    // failed is `Err` with the read's text (no `Update::Error` beside it);
    // the sidebar says `Models failed to load.` with `Try again`.
    Providers(Result<Vec<Provider>, String>),
    /// A provider's models, or its failure's text (it stays on the
    /// picker, not in a notice).
    Models {
        /// The provider.
        provider: String,
        /// The models, newest first, or the error.
        models: Result<Vec<ModelInfo>, String>,
    },
    /// A model's capabilities, or the failure's text.
    ModelInfo {
        /// The provider.
        provider: String,
        /// The model.
        model: String,
        /// Its capabilities, or the error.
        facts: Result<ModelFacts, String>,
    },
    /// The starred models (`modelFavorites`), at attach and after a save.
    Favorites(Vec<(String, String)>),
    /// The setup steps, whenever one changes.
    Setup(Vec<Step>),
    /// The live world: after each [`LiveCommand`], each phase, a player's
    /// change, and while running at most every 250 ms with the tick (pane
    /// agent; it replaces the three fields of the placeholder).
    Live(LiveState),
    /// An error to show as a notice (`showError`).
    Error(String),
    /// The core failed and stopped; the window shows this and offers to quit
    /// (`server-fatal.test.js`).
    Fatal(String),
    // sidebar agent: the prompt library (agreed with stage three).
    /// The prompt library (`GET /api/prompts` `prompts`), or the store's
    /// sentence when it could not be read (the screen's own line is
    /// `Prompts failed to load.`): after `Command::ReadPrompts` and whenever
    /// the library changes.
    Prompts(Result<Vec<LibraryPrompt>, String>),
    // settings agent: Settings, sign-in, installs, the vote count and the
    // published builds (`ui-seam-settings.md`).
    /// The settings (`GET /api/settings`), or why they could not be read:
    /// after `Command::ReadSettings`, after a sign-out, and unasked after a
    /// sign-in or an install reached `done`.
    Settings(Result<SettingsView, String>),
    /// A save's end: `Ok` after the providers and favourites it changed,
    /// or the refusal's text (shown inside Settings).
    SettingsSaved(Result<(), String>),
    /// A provider's latest sign-in, at its start and at every change of
    /// its view; none when none was started.
    SignIn {
        /// The provider.
        provider: String,
        /// Its view.
        state: Option<SignInView>,
    },
    /// A CLI's install, at its start, at progress changes and at its end.
    Install {
        /// `claude` or `codex`.
        program: String,
        /// Its view.
        state: InstallView,
    },
    /// The leaderboard (`GET /api/leaderboard`), or why it could not be
    /// read.
    Leaderboard(Result<Leaderboard, String>),
    /// The published builds (`GET /api/shares`), or why they could not be
    /// read.
    Shares(Result<Published, String>),
    /// A removal's end (`DELETE /api/shares/:shareId`).
    ShareRemoved {
        /// The build.
        share_id: String,
        /// `Ok`, or why it stayed.
        result: Result<(), String>,
    },
    // pane agent: the live view's updates (`ui-seam-viewer.md`).
    /// The 3D pane's handle, once, when the renderer has started (after
    /// setup gave it its assets), or the renderer's sentence
    /// (`No graphics adapter: …`).
    LivePane(Result<PaneHandle, String>),
    /// A click's end ([`ViewerInput::Click`]): the block's Minecraft name
    /// and the input taken (`toggle`, `press`, `use`), none for a block
    /// that takes no input or a click for a view not shown (then the name
    /// is empty).
    Clicked {
        /// The view the click was sent for.
        view: ViewKey,
        /// The block.
        block: [i32; 3],
        /// Its name (`Lever`).
        name: String,
        /// The input taken.
        input: Option<String>,
    },
    /// A note of player mode: a spawn's error, or `Fell out of the world`
    /// when the core respawned a player below y −128.
    PlayerNote(String),
    /// A selection's name ([`Command::DescribeSelection`]).
    SelectionDescribed {
        /// The request it answers.
        request: u64,
        /// The name, or the route's sentence.
        result: Result<SelectionDescription, String>,
    },
    /// An export's end: the file written (the path asked for), or why not.
    Exported(Result<PathBuf, String>),
    // bench agent: Bench and the prompt library (`ui-seam-bench-prompts.md`).
    /// A bench's details, or why they could not be read: after
    /// `Command::ReadBench`, a vote, a reveal, a branch switch, a send to
    /// the bench, the votes cleared; and unasked, after each chat list, for
    /// every watched bench whose details changed (a bench is watched from
    /// its `ReadBench` or a send to it until `Delete(Subject::Bench)`).
    Bench {
        /// The bench.
        bench: String,
        /// Its details, or the core's sentence.
        details: Result<BenchDetails, String>,
    },
    /// A bench send the core took: the bench (the new one for
    /// `Target::NewBench`) and the round to show, after the
    /// `Update::Conversations` that lists it running.
    BenchSent {
        /// Where it went.
        target: Target,
        /// The bench.
        bench: String,
        /// The round to show, from 1.
        round: usize,
    },
    /// A fork's end: the new Build chat, or the route's text.
    Forked {
        /// The bench model's chat.
        member: String,
        /// The new chat's id.
        chat: Result<String, String>,
    },
    /// A library save's end: the library as saved, or the refusal's text.
    PromptsSaved(Result<Vec<LibraryPrompt>, String>),
    /// An improve or write's end.
    Generated {
        /// The request it answers.
        request: u64,
        /// The text, the failure, or the cancel.
        outcome: Generated,
    },
    // par-chat (stage four's parity pass), SEAM ADDITION (item 1).
    /// `Command::CreateChat`'s end: the new row's id, or the route's text
    /// (`Provider and model are required`, or the store's), which the
    /// window shows as a notice.
    ChatCreated(Result<String, String>),
    // share agent (stage four's parity pass), SEAM ADDITION agreed with the
    // core's owner 2026-11-18 (`ui-seam-share.md`).
    /// `Command::ReadShare`'s end.
    ShareRead {
        /// The chat or bench.
        subject: Subject,
        /// The saved repository and the page's address, or the route's
        /// text.
        read: Result<ShareRead, String>,
    },
    /// One move of a publish (`Command::Publish`), for every publish
    /// whoever looks: the window applies it to the subject's share whether
    /// its panel is open or not.
    PublishStep {
        /// The chat or bench.
        subject: Subject,
        /// The move.
        step: PublishStep,
    },
    /// A subject's page as it now is: `Command::Unshare`'s end (none when
    /// it worked; the url it still has after the `Update::Error` of a
    /// failure); and unasked, with none, after a `Command::RemoveShare` that
    /// took down the build of a chat or bench that exists, between its
    /// `ShareRemoved` and its `Shares`.
    ShareChanged {
        /// The chat or bench.
        subject: Subject,
        /// The page's address; none: not shared.
        url: Option<String>,
    },
    // end of the share agent's block
}

// share agent (stage four's parity pass), SEAM ADDITION (`ui-seam-share.md`).
/// A subject's share as read ([`Update::ShareRead`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareRead {
    /// The saved repository for shared builds (`shareRepo`); none: no
    /// repository set.
    pub repo: Option<String>,
    /// The page's address (`{ url }`); none: not shared (also when no
    /// repository is set).
    pub url: Option<String>,
}

/// A publish's move (today's NDJSON lines, `server/share.js` `Publish`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishStep {
    /// `exporting`: the build is settled and its shot taken; the first.
    Exporting,
    /// `publishing`: the site's git work and push.
    Publishing,
    /// `waiting`: for GitHub Pages to serve it.
    Waiting,
    /// `live`: served at `url`; the last.
    Live {
        /// The page's address.
        url: String,
    },
    /// `failed`, from any of the first three or as a refusal: the last.
    Failed {
        /// The core's text.
        error: String,
    },
}
// end of the share agent's block

/// The core, as the window sees it. The stage-three session implements it.
pub trait Core: Send + Sync {
    /// The window's stored preferences, read once by the binary before the
    /// window opens (its size, place and theme are needed for the first
    /// frame).
    fn preferences(&self) -> Preferences;
    /// Gives the core the window to publish to; called once, before any
    /// `send`. The core then publishes `Update::Conversations`,
    /// `Update::Providers` and `Update::Favorites`.
    fn attach(&self, window: Arc<dyn Window>);
    /// Hands a command to the core and returns at once.
    fn send(&self, command: Command);
    /// The file of an `/api/images/<chat>/<name>` address (a stored
    /// picture); it only builds and checks a path, so the interface's
    /// thread may call it. A missing file is `Err("Image not found")`.
    fn image_file(&self, address: &str) -> Result<PathBuf, String>;
    /// The file of an `/api/attachments/<owner>/<name>` address; as
    /// `image_file`. A missing file is `Err("Attachment not found")`.
    fn attachment_file(&self, address: &str) -> Result<AttachmentFile, String>;
    // pane agent: a block's icon (`GET /api/blocks/:id/icon.png`), agreed
    // with the core session 2026-11-18.
    /// The PNG bytes of a block's icon, `id` as the page puts it in
    /// `/api/blocks/<id>/icon.png` (`minecraft:` taken off: `oak_planks`).
    /// Synchronous: the interface calls it off the paint path and keeps
    /// the answer for the run (the bytes never change for a name within
    /// one run). `Err("Unknown block")` for a name the block table lacks,
    /// `Err("Minecraft's data is not ready yet")` before setup has the
    /// data, any other `Err` the maker's own sentence.
    fn block_icon(&self, id: &str) -> Result<Vec<u8>, String>;
    // pane agent: the core stopped before the process exits (a live
    // renderer's thread polling its device while exit tears the driver
    // down crashed every quit).
    /// Stops what the core runs (its turns, as a press of Stop leaves each
    /// one stored; the live world and its renderer's thread; the engines;
    /// the children) and returns when they have stopped. Idempotent, with
    /// no deadline of its own: the joins alone take well under a second,
    /// running turns as long as their Stop takes (the provider's stream
    /// cancelled, a running command killed, the turn's last write).
    ///
    /// The window calls it once, on every way the app quits (the quit key,
    /// the last window closed, the system's quit on macOS), from GPUI's
    /// quit ([`stop_core_on_quit`]): on the interface thread, never on a
    /// thread of the core's runtime (it blocks on that runtime), after the
    /// windows are closed, so nothing sits on screen frozen while it waits;
    /// the process exits after it returns. Not called when the window never
    /// opened ([`fatal`] has no core), on a panic (the crash handler ends
    /// the process), or when the process is killed.
    fn shutdown(&self);
}

/// The window, as the core sees it. The stage-four session implements it.
pub trait Window: Send + Sync {
    /// Takes one update; callable from any thread, returns at once.
    fn publish(&self, update: Update);
}

/// Records user-visible actions, one JSON line each (`{"action": ..., ...}`):
/// sent text, stop, opened chat, new chat, link activated, copied text, a
/// fold toggled. The dev binary's `--log`; the platform checks read it to
/// see that a control did something.
#[derive(Clone)]
pub struct ActionLog(Arc<Mutex<File>>);

impl ActionLog {
    /// A log that writes to `path`, created or truncated.
    pub fn create(path: &std::path::Path) -> Self {
        let file = File::create(path)
            .unwrap_or_else(|e| panic!("cannot create the action log {}: {e}", path.display()));
        ActionLog(Arc::new(Mutex::new(file)))
    }

    /// Records one action with its fields.
    pub fn record(&self, action: &str, fields: serde_json::Value) {
        let mut line = serde_json::json!({ "action": action });
        if let serde_json::Value::Object(extra) = fields {
            line.as_object_mut().expect("an object").extend(extra);
        }
        let mut file = self.0.lock().expect("the action log's lock");
        writeln!(file, "{line}").expect("the action log is writable");
        file.flush().expect("the action log is writable");
    }
}

// linux-start: the window's identity.
/// The window's application id: its Wayland `app_id` and both halves of
/// its X11 `WM_CLASS`, which a desktop entry's file name and
/// `StartupWMClass` name, so the desktop ties the window to its launcher
/// and icon. The dev binary's windows carry it too.
pub const APP_ID: &str = "mcbench-lite";

/// How [`run`] opens the window.
pub struct Options {
    /// The window's size in logical pixels.
    pub size: (f32, f32),
    /// The window's origin, when given (`--origin`).
    pub origin: Option<(f32, f32)>,
    /// The size was given (`--size`): it wins over the remembered window.
    pub size_given: bool,
    /// Writes the window's accessibility tree as JSON here after each frame
    /// that changes it.
    pub a11y_dump: Option<PathBuf>,
    /// Records user-visible actions here.
    pub log: Option<ActionLog>,
    /// Quits after this long.
    pub quit_after: Option<Duration>,
    /// Opens a link: the browser in the app, only the log in the dev binary.
    pub opener: view::Opener,
    /// When the process started, for the time to the first frame.
    pub started: std::time::Instant,
    /// The place to open (`--open <address>`); the landing when none.
    pub open: Option<state::place::Place>,
    // chat (stage four's chat agent)
    /// Picks files to attach: the platform's file dialog in the app; the
    /// dev binary's `--pick` paths in the checks.
    pub picker: view::Picker,
    /// Selections the new chat's draft starts with: the dev binary's
    /// `--select` (the viewer makes them in a later milestone).
    pub selections: Vec<Selection>,
    // pane agent
    /// Where exports are saved: the platform's save dialog in the app; a
    /// folder of the dev binary's `--save-to` in the checks.
    pub saver: view::world::Saver,
    // mac-real
    /// The longest frame interval while the window is not the active one
    /// and its input is sparse (fewer than 60 events a second): GPUI's
    /// default, about 30 frames a second, in the app. The dev binary's
    /// `--active-pacing` sets none, so the rig's window, which is never the
    /// active one, paces its frames as a person's active window does (a
    /// measurement flag).
    pub inactive_frame_interval: Option<Duration>,
}

impl Options {
    /// The app's options: links open in the browser.
    pub fn app() -> Self {
        Options {
            size: (1280., 820.),
            origin: None,
            size_given: false,
            a11y_dump: None,
            // pane agent: `MCBENCH_UI_LOG` names an action log for the
            // checks of the real app (`verify/linux/real_pane.sh`: the
            // renderer line, the pane's costs); unset, there is none.
            log: std::env::var_os("MCBENCH_UI_LOG")
                .filter(|v| !v.is_empty())
                .map(|path| ActionLog::create(std::path::Path::new(&path))),
            quit_after: None,
            opener: Rc::new(|url, cx| cx.open_url(url)),
            started: std::time::Instant::now(),
            open: None,
            picker: view::platform_picker(),
            selections: Vec::new(),
            saver: view::world::platform_saver(),
            inactive_frame_interval: WindowOptions::default().inactive_frame_interval,
        }
    }

    /// The main window's options: its place and size, title, smallest size
    /// and frame pacing while inactive.
    fn window_options(
        &self,
        bounds: Bounds<gpui_kit::Pixels>,
        display_id: Option<gpui_kit::DisplayId>,
    ) -> WindowOptions {
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            display_id,
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some(view::shell::APP_NAME.into()),
                ..Default::default()
            }),
            window_min_size: Some(Size::new(px(640.), px(420.))),
            app_id: Some(APP_ID.to_string()),
            inactive_frame_interval: self.inactive_frame_interval,
            ..Default::default()
        }
    }
}

/// [`Window`] for the core: each update goes onto a channel GPUI's thread
/// drains, so `publish` returns at once from any thread.
struct Channel(futures::channel::mpsc::UnboundedSender<Update>);

impl Window for Channel {
    fn publish(&self, update: Update) {
        self.0
            .unbounded_send(update)
            .expect("the window's channel is open while the core runs");
    }
}

/// Opens the window on the main thread and runs until the user quits:
/// opens the chat screen, attaches the core, and drains its updates onto
/// the screen. The binary installs [`crash`] first thing, before anything
/// else runs; `run` refuses to start without it.
pub fn run(core: Arc<dyn Core>, options: Options) -> ! {
    assert!(
        crash::installed(),
        "crash::install runs before mcbench_ui::run"
    );
    let (sender, mut receiver) = futures::channel::mpsc::unbounded::<Update>();
    let app = gpui_kit::application()
        .with_assets(view::icons::Assets)
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed);
    let preferences = core.preferences();
    #[cfg(feature = "proof-hooks")]
    let panic_after = proof::panic_after(std::env::var(proof::PANIC_AFTER).ok().as_deref());
    app.run(move |cx| {
        view::setup(cx);
        view::set_menus(cx);
        // pane agent: the core stopped in GPUI's quit, on every path.
        stop_core_on_quit(core.clone(), cx);
        // pane agent: where exports are saved.
        view::world::set_saver(cx, options.saver.clone());
        view::set_theme(
            if preferences.light {
                view::theme::Mode::Light
            } else {
                view::theme::Mode::Dark
            },
            cx,
        );
        view::keys::set_keymap(cx, state::keymap::Keymap::merged(&preferences.binds));
        let (bounds, display_id) = window_place(&options, &preferences, cx);
        let window_options = options.window_options(bounds, display_id);
        let metrics = Rc::new(std::cell::RefCell::new(view::metrics::Metrics::new(
            options.started,
        )));
        let shell_options = view::shell::ShellOptions {
            core: core.clone(),
            log: options.log.clone(),
            opener: options.opener.clone(),
            metrics: metrics.clone(),
            // `--open` wins; else the place open at the last quit.
            start: options.open.clone().or_else(|| {
                preferences
                    .place
                    .as_deref()
                    .and_then(state::address::address_place)
            }),
            preferences: preferences.clone(),
            picker: options.picker.clone(),
            selections: options.selections.clone(),
        };
        let keys = metrics.clone();
        cx.intercept_keystrokes(move |_, _, _| keys.borrow_mut().key())
            .detach();
        if let Some(log) = options.log.clone() {
            let summary = metrics.clone();
            cx.on_app_quit(move |_| {
                log.record("metrics", summary.borrow().summary());
                async {}
            })
            .detach();
        }
        let (handle, screen) = gpui_kit::open_window(window_options, cx, |window, cx| {
            cx.new(|cx| view::shell::Shell::new(shell_options, window, cx))
        })
        .unwrap_or_else(|error| no_window(error));
        if let Some(log) = &options.log {
            log_renderer(log, handle, cx);
        }
        // Staging builds only: the window's thread panics on purpose that
        // long after the first frame (`proof`).
        #[cfg(feature = "proof-hooks")]
        if let Some(after) = panic_after {
            let painted = metrics.clone();
            cx.spawn(async move |cx| {
                while painted.borrow().first_frame().is_none() {
                    cx.background_executor()
                        .timer(Duration::from_millis(10))
                        .await;
                }
                cx.background_executor().timer(after).await;
                panic!("{}", proof::MESSAGE);
            })
            .detach();
        }
        // Launched from a shell or a file manager, the app comes to the front
        // with its window key (macOS left the launcher in front).
        cx.activate(true);
        core.attach(Arc::new(Channel(sender)));
        let drain = screen.clone();
        cx.spawn(async move |cx| {
            use futures::StreamExt as _;
            while let Some(update) = receiver.next().await {
                // pane agent: the core keeps publishing while the app quits
                // (a running world's ticks every 250 ms); once the window
                // has closed, the updates have no screen and the drain ends.
                if handle
                    .update(cx, |_, window, cx| {
                        drain.update(cx, |screen, cx| screen.apply(update, window, cx))
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
        if let Some(path) = options.a11y_dump.clone() {
            view::dump::start(handle, path, cx);
        }
        if let Some(after) = options.quit_after {
            cx.spawn(async move |cx| {
                cx.background_executor().timer(after).await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });
    std::process::exit(view::fatal::EXIT_STATUS.load(std::sync::atomic::Ordering::SeqCst))
}

// pane agent: the one place every quit passes.
/// Has GPUI's quit stop `core` ([`Core::shutdown`]): every way the app
/// ends runs GPUI's `App::shutdown` (the quit key's `quit`, the last
/// window closed, AppKit's terminate on macOS, where the run never
/// returns), which closes the windows and then polls the quit handlers'
/// futures; this one's first poll stops the core, however long that takes
/// (the handlers' 200 ms timeout is checked between polls). Asking before
/// a quit belongs before GPUI's `quit` (quit-ask: the shell's `Quit`,
/// `CloseWindow` and close-request handlers, `view::shell::quit`), never
/// here.
pub fn stop_core_on_quit(core: Arc<dyn Core>, cx: &mut gpui_kit::App) {
    cx.on_app_quit(move |_| {
        let core = core.clone();
        async move { core.shutdown() }
    })
    .detach();
}

// linux-start: the renderer named at start, and no window without one.
/// Ends the process when the window cannot open (on Linux: no adapter whose
/// device can configure the window's surface, as GPUI's renderer tests
/// each), with a sentence on stderr and status 3, before anything is drawn.
/// No crash log: the system lacks what the window needs, the app did not
/// fail.
fn no_window(error: impl std::fmt::Display) -> ! {
    eprintln!("mcbench-lite cannot open its window: {error:#}");
    std::process::exit(3)
}

/// Records the renderer the window draws with, once at start: the backend,
/// the adapter, its driver and whether it is the CPU emulating a GPU, as
/// GPUI reports them (GPUI reports nothing on macOS, where it draws only
/// through Metal); then the two font families the stacks resolved to
/// ([`view::fonts`]).
fn log_renderer(log: &ActionLog, handle: gpui_kit::AnyWindowHandle, cx: &mut gpui_kit::App) {
    let specs = handle
        .update(cx, |_, window, _| window.gpu_specs())
        .expect("the window just opened");
    let fields = match specs {
        Some(specs) => serde_json::json!({
            "backend": specs.backend,
            "adapter": specs.device_name,
            "driver": specs.driver_name,
            "driver_info": specs.driver_info,
            "software": specs.is_software_emulated,
        }),
        None => serde_json::json!({
            "backend": "Metal",
            "adapter": null,
            "driver": null,
            "driver_info": null,
            "software": false,
        }),
    };
    log.record("renderer", fields);
    let fonts = view::controls::fonts(cx);
    log.record(
        "fonts",
        serde_json::json!({ "ui": fonts.ui.to_string(), "data": fonts.data.to_string() }),
    );
}

/// Where the window opens: `--size` and `--origin` when given; else where
/// it was at the last quit, on its display if that is still there (else
/// the primary), moved onto the display's visible area; else 1280 by 820
/// centred in the primary display's visible area, shrunk to fit it
/// ([`state::layout::first`]).
fn window_place(
    options: &Options,
    preferences: &Preferences,
    cx: &gpui_kit::App,
) -> (Bounds<gpui_kit::Pixels>, Option<gpui_kit::DisplayId>) {
    let given = |origin: Option<(f32, f32)>, (w, h): (f32, f32)| Bounds {
        origin: origin
            .map(|(x, y)| GpuiPoint::new(px(x), px(y)))
            .unwrap_or_default(),
        size: Size::new(px(w), px(h)),
    };
    if options.origin.is_some() || options.size_given {
        return (given(options.origin, options.size), None);
    }
    let Some(saved) = &preferences.window else {
        let primary = cx
            .primary_display()
            .expect("the window needs a display to open on");
        let area = primary.visible_bounds();
        let rect = state::layout::first(
            state::layout::Rect {
                x: f32::from(area.origin.x),
                y: f32::from(area.origin.y),
                width: f32::from(area.size.width),
                height: f32::from(area.size.height),
            },
            options.size,
        );
        return (
            Bounds {
                origin: GpuiPoint::new(px(rect.x), px(rect.y)),
                size: Size::new(px(rect.width), px(rect.height)),
            },
            Some(primary.id()),
        );
    };
    let mut displays: Vec<(String, state::layout::Rect, gpui_kit::DisplayId)> = cx
        .displays()
        .into_iter()
        .map(|d| {
            let area = d.visible_bounds();
            (
                display_name(&*d),
                state::layout::Rect {
                    x: f32::from(area.origin.x),
                    y: f32::from(area.origin.y),
                    width: f32::from(area.size.width),
                    height: f32::from(area.size.height),
                },
                d.id(),
            )
        })
        .collect();
    let primary = cx.primary_display().map(|d| d.id());
    displays.sort_by_key(|(_, _, id)| Some(*id) != primary);
    let pairs: Vec<(String, state::layout::Rect)> =
        displays.iter().map(|(n, r, _)| (n.clone(), *r)).collect();
    let (name, rect) = state::layout::restore(saved, &pairs);
    let id = displays
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(.., id)| *id);
    (
        Bounds {
            origin: GpuiPoint::new(px(rect.x), px(rect.y)),
            size: Size::new(px(rect.width), px(rect.height)),
        },
        id,
    )
}

/// A display's identity for the preferences: its UUID where the platform
/// gives one, else its id.
pub fn display_name(display: &dyn gpui_kit::PlatformDisplay) -> String {
    match display.uuid() {
        Ok(uuid) => uuid.to_string(),
        Err(_) => format!("display-{:?}", display.id()),
    }
}

/// Opens the controls gallery ([`dev::gallery`]) and runs until the user
/// quits: the dev binary's `--gallery`, for the platform checks.
pub fn run_gallery(options: Options) -> ! {
    assert!(
        crash::installed(),
        "crash::install runs before mcbench_ui::run_gallery"
    );
    let app = gpui_kit::application()
        .with_assets(view::icons::Assets)
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed);
    app.run(move |cx| {
        view::setup(cx);
        view::set_menus(cx);
        let (width, height) = options.size;
        let bounds = Bounds {
            origin: options
                .origin
                .map(|(x, y)| GpuiPoint::new(px(x), px(y)))
                .unwrap_or_default(),
            size: Size::new(px(width), px(height)),
        };
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some(format!("Gallery \u{b7} {}", view::shell::APP_NAME).into()),
                ..Default::default()
            }),
            app_id: Some(APP_ID.to_string()),
            ..Default::default()
        };
        let log = options.log.clone();
        let (handle, _) = gpui_kit::open_window(window_options, cx, |_, cx| {
            cx.new(|cx| dev::gallery::Gallery::new(log, cx))
        })
        .unwrap_or_else(|error| no_window(error));
        cx.activate(true);
        if let Some(path) = options.a11y_dump.clone() {
            view::dump::start(handle, path, cx);
        }
        if let Some(after) = options.quit_after {
            cx.spawn(async move |cx| {
                cx.background_executor().timer(after).await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });
    std::process::exit(0)
}

/// Shows `message` on the fatal screen and exits with status 2 when the
/// user quits or closes the window: for a failure before the core can run
/// (the data folder cannot be made), which a GUI app's stderr would hide.
/// The binary installs [`crash`] first, as for [`run`].
pub fn fatal(message: String, options: Options) -> ! {
    assert!(
        crash::installed(),
        "crash::install runs before mcbench_ui::fatal"
    );
    let app = gpui_kit::application()
        .with_assets(view::icons::Assets)
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed);
    app.run(move |cx| {
        view::setup(cx);
        view::set_menus(cx);
        let (width, height) = options.size;
        let bounds = Bounds {
            origin: options
                .origin
                .map(|(x, y)| GpuiPoint::new(px(x), px(y)))
                .unwrap_or_default(),
            size: Size::new(px(width), px(height)),
        };
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some(format!("{} \u{2014} stopped", view::shell::APP_NAME).into()),
                ..Default::default()
            }),
            app_id: Some(APP_ID.to_string()),
            ..Default::default()
        };
        let (handle, _) = gpui_kit::open_window(window_options, cx, |window, cx| {
            cx.new(|cx| view::fatal::FatalScreen::new(message, window, cx))
        })
        .unwrap_or_else(|error| no_window(error));
        cx.activate(true);
        if let Some(path) = options.a11y_dump.clone() {
            view::dump::start(handle, path, cx);
        }
    });
    std::process::exit(2)
}

/// The default build compiles no test hook: the `proof` module is behind
/// its feature and nothing outside it names the variable.
#[cfg(all(test, not(feature = "proof-hooks")))]
mod no_proof_hooks {
    #[test]
    fn the_default_build_ignores_the_panic_variable() {
        let variable = concat!("MCBENCH_UI_", "PANIC_AFTER_MS");
        let lib = include_str!("lib.rs");
        assert!(lib.contains("#[cfg(feature = \"proof-hooks\")]\npub mod proof;"));
        assert!(!lib.contains(variable));
        const { assert!(!cfg!(feature = "proof-hooks")) };
    }
}

#[cfg(test)]
mod pacing {
    use super::*;

    #[test]
    fn the_app_keeps_gpuis_inactive_pacing() {
        let options = Options::app();
        assert_eq!(
            options.inactive_frame_interval,
            WindowOptions::default().inactive_frame_interval
        );
        assert!(options.inactive_frame_interval.is_some());
    }

    #[test]
    fn no_inactive_interval_reaches_the_window_options() {
        let mut options = Options::app();
        options.inactive_frame_interval = None;
        let bounds = Bounds {
            origin: GpuiPoint::new(px(0.), px(0.)),
            size: Size::new(px(1280.), px(820.)),
        };
        assert_eq!(
            options.window_options(bounds, None).inactive_frame_interval,
            None
        );
    }
}

#[cfg(test)]
mod seam {
    use super::*;

    struct FakeCore;
    impl Core for FakeCore {
        fn attach(&self, _: Arc<dyn Window>) {
            unimplemented!("a fake for the seam test")
        }
        fn send(&self, _: Command) {
            unimplemented!("a fake for the seam test")
        }
        fn preferences(&self) -> Preferences {
            unimplemented!("a fake for the seam test")
        }
        fn image_file(&self, _: &str) -> Result<PathBuf, String> {
            unimplemented!("a fake for the seam test")
        }
        fn attachment_file(&self, _: &str) -> Result<AttachmentFile, String> {
            unimplemented!("a fake for the seam test")
        }
        fn block_icon(&self, _: &str) -> Result<Vec<u8>, String> {
            unimplemented!("a fake for the seam test")
        }
        fn shutdown(&self) {
            unimplemented!("a fake for the seam test")
        }
    }

    struct FakeWindow;
    impl Window for FakeWindow {
        fn publish(&self, _: Update) {
            unimplemented!("a fake for the seam test")
        }
    }

    #[test]
    fn a_fake_can_be_written_against_each_trait() {
        let _: Arc<dyn Core> = Arc::new(FakeCore);
        let _: Arc<dyn Window> = Arc::new(FakeWindow);
    }
}
