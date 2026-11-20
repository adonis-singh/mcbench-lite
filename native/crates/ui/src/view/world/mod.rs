//! The 3D pane (pane agent): the live Minecraft world beside the chat. The
//! window holds the renderer's [`crate::LivePane`] handle, paints the frames
//! it takes from it ([`frame`]), maps the pointer, the wheel and the keys to
//! the renderer's camera, and draws the toolbar, the cover and banner, a
//! clicked block's label, the selection tool, player mode's HUD, the
//! exports and a bench comparison's labels, as today's page
//! (`inventory/style-viewer-routes.md` Part 2) and the agreed seam
//! (`ui-seam-viewer.md`) say. The pure rules are [`crate::state::live`].
//!
//! | Part | Where |
//! |---|---|
//! | the frame path (take, upload, paint, resize) | [`frame`] |
//! | the cover, banner and toolbar texts | [`crate::state::live::chrome`] |
//! | the camera's and the player's keys | [`crate::state::live::camera_key`], [`crate::state::live::player_key`] |
//! | the bench comparison's interface (the bench agent's place calls it) | [`round_shown`], [`frame_round`], [`frame_member`]; the labels from [`Pane::set_members`] |

pub mod bluemap;
pub mod frame;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::chat::ChatPane;
use super::controls::fonts;
use super::icons::{self, Icon};
use super::shell::Shell;
use super::shell::announce::Announcer;
use super::theme::{self, TextKind, colour};
use crate::state::layers::Layer;
use crate::state::live::{self, CameraKind, Mods, Offer};
use crate::{
    ActionLog, CameraInput, CameraMode, Command, Core, DragButton, LiveCommand, LiveFacts,
    LivePhase, LiveState, OverlayBox, Overlays, PaneStatus, Pick, PlayerState, Selection,
    TimeOfDay, Update, ViewKey, ViewerInput,
};
use bluemap::BarIcon;
use frame::{FrameView, Store};

/// Asks where to save an export, with the name it suggests: the path, none
/// when cancelled, or why no dialog could be shown.
pub type Saver = Rc<
    dyn Fn(&str, &mut App) -> futures::channel::oneshot::Receiver<Result<Option<PathBuf>, String>>,
>;

/// The saver in force ([`set_saver`]): the platform's save dialog in the
/// app, a folder of the dev binary's `--save-to` in the checks.
pub struct SaveTo(pub Saver);

impl Global for SaveTo {}

/// Installs the saver.
pub fn set_saver(cx: &mut App, saver: Saver) {
    cx.set_global(SaveTo(saver));
}

/// The platform's save dialog (`App::prompt_for_new_path`), opening in the
/// downloads folder (today's page downloads there).
pub fn platform_saver() -> Saver {
    Rc::new(|name, cx| {
        let (sender, receiver) = futures::channel::oneshot::channel();
        let folder = dirs_downloads();
        let prompt = cx.prompt_for_new_path(&folder, Some(name));
        cx.foreground_executor()
            .spawn(async move {
                let answer = match prompt.await {
                    Ok(Ok(path)) => Ok(path),
                    Ok(Err(error)) => Err(format!("The save dialog could not be shown: {error}")),
                    Err(_) => Ok(None),
                };
                let _ = sender.send(answer);
            })
            .detach();
        receiver
    })
}

fn dirs_downloads() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .expect("the user's home folder is named by HOME or USERPROFILE");
    home.join("Downloads")
}

/// Saves into `folder` under the suggested name, with no dialog (the dev
/// binary's `--save-to`, the tests).
pub fn fixed_saver(folder: PathBuf) -> Saver {
    Rc::new(move |name, _| {
        let (sender, receiver) = futures::channel::oneshot::channel();
        let _ = sender.send(Ok(Some(folder.join(name))));
        receiver
    })
}

/// Takes the pointer to the window, or says why the platform refused it.
pub type Locker = Rc<dyn Fn(&mut Window) -> Result<(), String>>;

/// The locker in force ([`set_locker`]): the platform's pointer lock in the
/// app, a refusing one in the checks that need a refusal.
pub struct TakePointer(pub Locker);

impl Global for TakePointer {}

/// Installs the locker.
pub fn set_locker(cx: &mut App, locker: Locker) {
    cx.set_global(TakePointer(locker));
}

/// The platform's pointer lock (`Window::set_pointer_locked`, vendor patch
/// 50).
pub fn platform_locker() -> Locker {
    Rc::new(|window| {
        window
            .set_pointer_locked(true)
            .map_err(|error| error.to_string())
    })
}

/// Refuses every lock with `reason` (the tests).
pub fn refusing_locker(reason: &'static str) -> Locker {
    Rc::new(move |_| Err(reason.to_string()))
}

/// Whether the pane fills the window and whether player mode is on, for
/// the type-ahead ([`keeps_key`]).
#[derive(Default)]
pub struct PaneKeys {
    full: bool,
    player: bool,
}

impl Global for PaneKeys {}

/// Whether a printable key stays with the 3D pane instead of going to the
/// composer: always while the pane fills the window; while the pane has
/// focus, in player mode, or a camera key (`CAMERA_KEYS`).
pub fn keeps_key(key: &str, window: &Window, cx: &App) -> bool {
    let flags = cx.try_global::<PaneKeys>();
    if flags.is_some_and(|f| f.full) {
        return true;
    }
    let focused = window
        .context_stack()
        .iter()
        .any(|c| c.contains("LivePane"));
    focused && (flags.is_some_and(|f| f.player) || live::pane_keeps(key))
}

/// A selection icon's anchor index and its block.
type IconMark = (usize, Option<String>);

/// A key pressed while the pane has the focus, before anything moves the
/// focus away (the composer's type-ahead): the block's label closes.
pub fn key_on_world(window: &Window, cx: &mut App) {
    let on_world = window
        .context_stack()
        .iter()
        .any(|c| c.contains("LivePane"));
    if on_world {
        with_shell(cx, |shell, _, cx| {
            shell.world().update(cx, |pane, cx| pane.close_popup(cx))
        });
    }
}

/// Told when the night switch changes.
pub type OnNight = Rc<dyn Fn(bool, &mut App)>;

/// The bar's x and z fields, the whole blocks they last showed, and their
/// subscriptions.
type PositionFields = ([Entity<InputState>; 2], (i64, i64), Vec<Subscription>);

/// What an update gives back to the shell.
pub enum Back {
    /// A named selection for the open chat's draft.
    Selection(Selection),
    /// A notice to show (`showError`).
    Notice(String),
}

/// What a pick was for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    /// A click on a block.
    Click,
    /// A selection's first block.
    SelectStart,
    /// A selection drag's move.
    SelectMove,
    /// Player mode's right click, or Enter at the centre.
    Use,
}

struct Drag {
    mouse: MouseButton,
    button: Option<DragButton>,
    down: [f32; 2],
    last: [f32; 2],
    down_at: Instant,
    /// The movement sent so far and how many moves (the `drag` log line).
    moved: [f32; 2],
    moves: u32,
}

#[derive(Default)]
struct SelectDrag {
    start: Option<[i32; 3]>,
    end: Option<[i32; 3]>,
    in_flight: bool,
    waiting: Option<(u64, [f32; 2])>,
}

#[derive(Default)]
struct Player {
    standing: bool,
    /// The look is taken from the first frame of the player's eye (the
    /// core stands it facing the build, not where the window asked).
    look_pending: bool,
    /// The eye when player mode began: frames of that eye are the map's
    /// view the renderer holds until the player stands.
    map_eye: [f64; 3],
    yaw: f32,
    pitch: f32,
    keys: u32,
    taps: u32,
    held: HashMap<String, u32>,
    dirty: bool,
    flush: bool,
}

/// The 3D pane.
pub struct Pane {
    core: Arc<dyn Core>,
    log: Option<ActionLog>,
    announcer: Entity<Announcer>,
    store: Rc<RefCell<Store>>,
    frames: Entity<FrameView>,
    focus: FocusHandle,
    chat: Option<WeakEntity<ChatPane>>,
    pane_error: Option<String>,
    status: PaneStatus,
    facts: LiveFacts,
    state: Option<LiveState>,
    view: Option<ViewKey>,
    painted: Option<(ViewKey, u64)>,
    title: String,
    selectable_chat: Option<String>,
    bench: Option<(String, usize)>,
    bench_mode: bool,
    preparing: bool,
    night: bool,
    full: bool,
    keyboard: bool,
    held: u16,
    key_bits: HashMap<String, u16>,
    mode: CameraMode,
    drag: Option<Drag>,
    selecting: bool,
    select: Option<SelectDrag>,
    marked: Option<[i32; 3]>,
    /// When the marked block's label opened (its 300 ms fade in).
    marked_opened: Option<Instant>,
    /// A closed label fading out over 300 ms (its box gone at once).
    closing: Option<([i32; 3], Instant)>,
    members: Vec<(String, String)>,
    player: Option<Player>,
    /// The pointer is locked to the window: player mode's look, or
    /// spectator's locked look (BlueMap's `document.pointerLockElement`).
    locked: bool,
    note: Option<(String, Instant)>,
    picks: futures::channel::mpsc::UnboundedSender<(Purpose, Pick)>,
    next_request: u64,
    /// Each selection asked to be named, with the navigation it was asked
    /// in (`navigationRef`).
    asked: HashMap<u64, ([i32; 3], [i32; 3], u64)>,
    /// Counts the views left: a name that comes back after one is dropped.
    navigations: u64,
    sent_overlays: Option<Overlays>,
    /// The bar's x and z fields, made at the first render, and the
    /// position they last showed.
    position: Option<PositionFields>,
    /// The selection chip under the pointer, its box in build coordinates.
    hovered: Option<([i64; 3], [i64; 3])>,
    /// Told when the night switch changes (the shell keeps it).
    pub on_night: Option<OnNight>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Pane {
    /// A pane with no handle yet (the core publishes it once the renderer
    /// started), in the night the preferences keep.
    pub fn new(
        core: Arc<dyn Core>,
        log: Option<ActionLog>,
        announcer: Entity<Announcer>,
        night: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = Rc::new(RefCell::new(Store::default()));
        let frames = cx.new(|_| FrameView {
            store: store.clone(),
        });
        let this = cx.entity().downgrade();
        store.borrow_mut().on_frame = Some(Rc::new(move |cx| {
            let _ = this.update(cx, |pane, cx| pane.frame_painted(cx));
        }));
        let this = cx.entity().downgrade();
        store.borrow_mut().on_motion = Some(Rc::new(move |delta, window, cx| {
            let _ = this.update(cx, |pane, cx| pane.motion(delta, window, cx));
        }));
        let (picks, mut answers) = futures::channel::mpsc::unbounded::<(Purpose, Pick)>();
        let pick_task = cx.spawn(async move |this, cx| {
            use futures::StreamExt as _;
            while let Some((purpose, pick)) = answers.next().await {
                if this
                    .update(cx, |pane, cx| pane.picked(purpose, pick, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        // The core holds the last `PlayerInput`'s keys until the next one:
        // every path that ends a hold sends one with the keys released
        // (the key's up, blur, the pointer freed, the window hidden,
        // leaving player mode, a modal layer opening).
        let visibility = cx.observe_window_visibility(window, |pane, _, window, cx| {
            pane.tell_visible(window);
            if !window.visibility().is_visible() {
                pane.free_pointer(window, cx);
                pane.release_keys();
            }
        });
        let layers = cx.observe_global_in::<crate::view::keys::Keys>(window, |pane, window, cx| {
            if crate::view::keys::modal_open(cx) {
                pane.free_pointer(window, cx);
                pane.release_keys();
            }
        });
        let focus = cx.focus_handle().tab_stop(true);
        // The window losing focus gives the pointer back, as a browser's
        // pointer lock ends.
        let activation = cx.observe_window_activation(window, |pane, window, cx| {
            if !window.is_window_active() {
                pane.free_pointer(window, cx);
                pane.release_keys();
            }
        });
        // Any key on the world closes the block's label (BlueMap's keydown
        // `removeHandler`), the type-ahead's too, which never reaches the
        // pane's own key handler.
        let keys = {
            let this = cx.entity().downgrade();
            let focus = focus.clone();
            cx.intercept_keystrokes(move |_, window, cx| {
                if focus.is_focused(window) {
                    let _ = this.update(cx, |pane, cx| pane.close_popup(cx));
                }
            })
        };
        let blur = cx.on_blur(&focus, window, |pane, _, _| pane.release_keys());

        Pane {
            core,
            log,
            announcer,
            store,
            frames,
            focus,
            chat: None,
            pane_error: None,
            status: PaneStatus::Drawing,
            facts: LiveFacts::default(),
            state: None,
            view: None,
            painted: None,
            title: String::new(),
            selectable_chat: None,
            bench: None,
            bench_mode: false,
            preparing: false,
            night,
            full: false,
            keyboard: false,
            held: 0,
            key_bits: HashMap::new(),
            mode: CameraMode::Perspective,
            drag: None,
            selecting: false,
            select: None,
            marked_opened: None,
            closing: None,
            marked: None,
            members: Vec::new(),
            player: None,
            locked: false,
            note: None,
            picks,
            next_request: 0,
            asked: HashMap::new(),
            navigations: 0,
            sent_overlays: None,
            on_night: None,
            position: None,
            hovered: None,
            _tasks: vec![pick_task],
            _subscriptions: vec![visibility, blur, activation, layers, keys],
        }
    }

    fn log(&self, action: &str, fields: serde_json::Value) {
        if let Some(log) = &self.log {
            log.record(action, fields);
        }
    }

    fn pane(&self) -> Option<Arc<dyn crate::LivePane>> {
        self.store.borrow().pane.clone()
    }

    /// The pane's focus (the Canvas node, a Tab stop after the chat column).
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Whether the pane fills the window.
    pub fn full(&self) -> bool {
        self.full
    }

    /// The view the window shows now.
    pub fn view(&self) -> Option<&ViewKey> {
        self.view.as_ref()
    }

    /// The core's live state as last published.
    pub fn state(&self) -> Option<&LiveState> {
        self.state.as_ref()
    }

    /// Whether the selection tool is on.
    pub fn selecting(&self) -> bool {
        self.selecting
    }

    /// What the frame path cost so far.
    pub fn costs(&self) -> frame::Costs {
        self.store.borrow().costs.clone()
    }

    /// The camera keys held now.
    pub fn held(&self) -> u16 {
        self.held
    }

    /// Player mode's look as the window holds it (yaw, pitch), none outside
    /// player mode.
    pub fn player_look(&self) -> Option<(f32, f32)> {
        self.player.as_ref().map(|p| (p.yaw, p.pitch))
    }

    /// Shows `view` (the place's world), naming it `title`; `chat` is the
    /// Build chat whose draft takes selections; `bench` a comparison's
    /// title and round; sends `Live(Show)` when the view changes.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        view: Option<ViewKey>,
        title: String,
        chat: Option<(String, WeakEntity<ChatPane>)>,
        bench: Option<(String, usize)>,
        bench_mode: bool,
        preparing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.title = title;
        self.selectable_chat = chat.as_ref().map(|(id, _)| id.clone());
        self.chat = chat.map(|(_, c)| c);
        self.bench = bench;
        self.bench_mode = bench_mode;
        self.preparing = preparing;
        if view != self.view {
            self.leave_view(window, cx);
            self.view = view.clone();
            self.store.borrow_mut().view = view.clone();
            self.painted = None;
            self.log(
                "live show",
                serde_json::json!({ "view": view.as_ref().map(|v| v.0.clone()), "t_ms": since_start_ms() }),
            );
            self.core.send(Command::Live(LiveCommand::Show {
                view,
                afresh: false,
            }));
            self.tell_visible(window);
        }
        self.sync_overlays(cx);
        cx.notify();
    }

    /// Leaving a view: the selection tool, the marked block, player mode.
    fn leave_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigations += 1;
        self.selecting = false;
        self.select = None;
        self.marked = None;
        self.free_pointer(window, cx);
        self.release_keys();
        if let Some(player) = self.player.take() {
            if let Some(pane) = self.pane() {
                pane.camera(CameraInput::Mode(CameraMode::Perspective));
            }
            self.mode = CameraMode::Perspective;
            // A spawn still on its way stands no one in the next world
            // (`web/app.js` 3198).
            if !player.standing {
                self.show_note(live::ANOTHER_WORLD.into(), cx);
            }
        }
    }

    fn tell_visible(&self, window: &Window) {
        if let Some(pane) = self.pane() {
            pane.visible(window.visibility().is_visible() && self.view.is_some());
        }
    }

    /// A live view's update (`Update::LivePane`, `Live`, `Clicked`,
    /// `PlayerNote`, `SelectionDescribed`, `Exported`); a described
    /// selection comes back for the composer's draft.
    pub fn apply(
        &mut self,
        update: Update,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Back> {
        match update {
            Update::LivePane(Ok(handle)) => {
                self.log(
                    "live pane",
                    serde_json::json!({ "ok": true, "t_ms": since_start_ms() }),
                );
                let timed = Arc::new(frame::Timed {
                    inner: handle.0,
                    since: std::sync::Mutex::new(None),
                    arrived: Default::default(),
                });
                self.store.borrow_mut().input_since = Some(timed.clone());
                let pane: Arc<dyn crate::LivePane> = timed;
                let (sender, mut receiver) = futures::channel::mpsc::unbounded::<()>();
                pane.set_waker(Box::new(move || {
                    let _ = sender.unbounded_send(());
                }));
                let frames = self.frames.downgrade();
                let this = cx.entity().downgrade();
                self._tasks.push(cx.spawn(async move |_, cx| {
                    use futures::StreamExt as _;
                    while receiver.next().await.is_some() {
                        // Every wake waiting is one paint.
                        while receiver.try_recv().is_ok() {}
                        if frames.update(cx, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                        let _ = this.update(cx, |pane, cx| pane.refresh_status(cx));
                    }
                }));
                pane.time_of_day(if self.night {
                    TimeOfDay::Night
                } else {
                    TimeOfDay::Day
                });
                let told = self.store.borrow().told;
                if let Some(size) = told {
                    pane.resize(size);
                }
                self.facts = pane.facts();
                self.status = pane.status();
                self.store.borrow_mut().pane = Some(pane);
                self.tell_visible(window);
                self.sent_overlays = None;
                self.sync_overlays(cx);
            }
            Update::LivePane(Err(error)) => {
                self.log(
                    "live pane",
                    serde_json::json!({ "error": error, "t_ms": since_start_ms() }),
                );
                self.pane_error = Some(error);
            }
            Update::Live(state) => {
                // Each change of phase or of running, timed: the start's
                // parts (the core's world load, the renderer's start).
                let was = self
                    .state
                    .as_ref()
                    .map(|s| (s.phase.clone(), s.running.is_some(), s.revision));
                if was != Some((state.phase.clone(), state.running.is_some(), state.revision)) {
                    self.log(
                        "live state",
                        serde_json::json!({ "phase": format!("{:?}", state.phase), "running": state.running.is_some(), "revision": state.revision, "t_ms": since_start_ms() }),
                    );
                }
                // The core let the player go (its engine gone, a fresh
                // placement that failed): player mode ends with the page's
                // note (`web/bluemap-bridge.js` 197-205); a world no longer
                // running ends it too (182).
                if state.player == PlayerState::None
                    && self.player.as_ref().is_some_and(|p| p.standing)
                {
                    self.end_player(live::PLAYER_GONE, window, cx);
                } else if self.player.is_some()
                    && !(state.phase == LivePhase::Shown && state.running.is_some())
                {
                    self.end_player(live::NEEDS_RUNNING, window, cx);
                }
                if state.player == PlayerState::Standing
                    && let Some(p) = &mut self.player
                    && !p.standing
                {
                    p.standing = true;
                    p.look_pending = true;
                }
                // The eye's frame may already be painted.
                self.adopt_look();
                self.state = Some(state);
                self.sync_overlays(cx);
            }
            Update::Clicked {
                view,
                block,
                name,
                input,
            } => {
                if Some(&view) == self.view.as_ref() && !name.is_empty() {
                    let text = live::clicked_text(&name, block, input.as_deref());
                    self.announcer.update(cx, |a, cx| a.polite(text, cx));
                }
            }
            Update::PlayerNote(note) => self.show_note(note, cx),
            Update::SelectionDescribed { request, result } => {
                let (min, max, navigation) = self.asked.remove(&request)?;
                // Named after the person went elsewhere: dropped, as the
                // page does (`if (navigationRef.current !== navigation)
                // return`).
                if navigation != self.navigations {
                    return None;
                }
                match result {
                    Ok(description) => {
                        let y = self.state.as_ref().map_or(0, |s| s.y_offset);
                        let (bmin, bmax) = live::build_box(min, max, y);
                        return Some(Back::Selection(Selection {
                            letter: String::new(),
                            label: description.label,
                            min: bmin,
                            max: bmax,
                            blocks: description.blocks as i64,
                            block: description.block,
                        }));
                    }
                    // A failed naming is a notice (`showError`).
                    Err(error) => return Some(Back::Notice(error)),
                }
            }
            Update::Exported(result) => {
                self.log("exported", serde_json::json!({ "ok": result.is_ok() }));
                match result {
                    // Today a download says nothing; the polite region says
                    // where it went.
                    Ok(path) => {
                        let text = format!(
                            "Saved {}",
                            path.file_name()
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                        );
                        self.announcer.update(cx, |a, cx| a.polite(text, cx));
                    }
                    Err(error) => return Some(Back::Notice(error)),
                }
            }
            _ => unreachable!("the shell hands the pane only the live view's updates"),
        }
        cx.notify();
        None
    }

    fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let Some(pane) = self.pane() else { return };
        let status = pane.status();
        if status != self.status {
            self.status = status;
            cx.notify();
        }
    }

    fn frame_painted(&mut self, cx: &mut Context<Self>) {
        let store = self.store.borrow();
        let Some(shown) = &store.shown else { return };
        let now = (shown.view.clone(), shown.revision);
        drop(store);
        self.adopt_look();
        if self.painted.as_ref() != Some(&now) {
            self.log(
                "pane frame",
                serde_json::json!({ "view": now.0.0, "revision": now.1, "first": self.painted.is_none(), "t_ms": since_start_ms() }),
            );
            self.painted = Some(now);
            cx.notify();
        } else if !self.members.is_empty() || self.drafts_shown() {
            // Labels move with the frame they belong to.
            cx.notify();
        }
    }

    /// Takes the player's look from the newest painted frame of its eye,
    /// once the core stood it (its look is the spot's, not the window's);
    /// a frame of the map's view held until then gives none.
    fn adopt_look(&mut self) {
        let camera = match &self.store.borrow().shown {
            Some(shown) => shown.camera,
            None => return,
        };
        if let Some(p) = &mut self.player
            && p.look_pending
            && let Some(look) = live::stood_look(&camera, p.map_eye)
        {
            (p.yaw, p.pitch) = look;
            p.look_pending = false;
        }
    }

    /// The camera's heading of the newest painted frame, in degrees.
    fn heading_degrees(&self) -> f64 {
        self.store
            .borrow()
            .shown
            .as_ref()
            .map_or(0.0, |s| s.camera.rotation.to_degrees())
    }

    /// The x and z fields: they show the camera's target, a whole block
    /// (`Math.floor`), while not being typed in; a typed number moves the
    /// target there (`CameraInput::Target`).
    fn position_fields(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> [Entity<InputState>; 2] {
        if self.position.is_none() {
            let make = |window: &mut Window, cx: &mut Context<Self>| {
                cx.new(|cx| InputState::new(window, cx))
            };
            let x = make(window, cx);
            let z = make(window, cx);
            let mut subscriptions = Vec::new();
            for field in [x.clone(), z.clone()] {
                subscriptions.push(cx.subscribe_in(
                    &field,
                    window,
                    |pane: &mut Self, _, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change) {
                            pane.typed_position(cx);
                        }
                    },
                ));
            }
            self.position = Some(([x, z], (i64::MIN, i64::MIN), subscriptions));
        }
        let target = self.store.borrow().shown.as_ref().map(|s| s.camera.target);
        let (fields, shown, _) = self.position.as_mut().expect("made above");
        if let Some(t) = target {
            let now = (t[0].floor() as i64, t[2].floor() as i64);
            if now != *shown {
                *shown = now;
                for (field, value) in fields.iter().zip([now.0, now.1]) {
                    if !Focusable::focus_handle(field.read(cx), cx).is_focused(window) {
                        field.update(cx, |f, cx| f.set_value(value.to_string(), window, cx));
                    }
                }
            }
        }
        fields.clone()
    }

    fn typed_position(&mut self, cx: &mut Context<Self>) {
        let Some(([x, z], _, _)) = &self.position else {
            return;
        };
        let parse = |e: &Entity<InputState>| e.read(cx).value().trim().parse::<f64>().ok();
        if let (Some(x), Some(z), Some(pane)) = (parse(x), parse(z), self.pane()) {
            pane.camera(CameraInput::Target { x, z });
        }
    }

    fn drafts_shown(&self) -> bool {
        self.selectable_chat.is_some()
    }

    fn show_note(&mut self, note: String, cx: &mut Context<Self>) {
        self.note = Some((note, Instant::now()));
        let shown = self.note.as_ref().map(|(_, at)| *at);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(live::NOTE_MS))
                .await;
            let _ = this.update(cx, |pane, cx| {
                if pane.note.as_ref().map(|(_, at)| *at) == shown {
                    pane.note = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    // -- the bench comparison's interface ----------------------------------

    /// The names over a bench round's builds, by member (the model's
    /// name, or `Build <letter>` while names are hidden); each label goes
    /// over its member's box in the core's layout.
    pub fn set_members(&mut self, members: Vec<(String, String)>, cx: &mut Context<Self>) {
        if members != self.members {
            self.members = members;
            cx.notify();
        }
    }

    /// The label of the layout's `index`th build.
    fn name_of(&self, index: usize) -> Option<String> {
        let (id, _) = self.state.as_ref()?.layout.get(index)?;
        self.members
            .iter()
            .find(|(m, _)| m == id)
            .map(|(_, name)| name.clone())
    }

    /// Flies to the `index`th member's build (a row's focus action, the
    /// keys 1 to 9), over 700 ms.
    pub fn focus_member(&mut self, index: usize) {
        let Some((_, [min, max])) = self
            .state
            .as_ref()
            .and_then(|s| s.layout.get(index).cloned())
        else {
            return;
        };
        self.fly(min, max);
    }

    /// Flies to the build of the member whose chat is `member`.
    pub fn focus_member_id(&mut self, member: &str) {
        let index = self
            .state
            .as_ref()
            .and_then(|s| s.layout.iter().position(|(id, _)| id == member));
        self.log(
            "frame member",
            serde_json::json!({ "member": member, "in_view": index.is_some() }),
        );
        if let Some(index) = index {
            self.focus_member(index);
        }
    }

    /// Flies to the whole round (a click on the round's prompt).
    pub fn focus_round(&mut self) {
        if let Some([min, max]) = self.state.as_ref().and_then(|s| s.bounds) {
            self.fly(min, max);
        }
    }

    fn fly(&mut self, min: [i32; 3], max: [i32; 3]) {
        let Some(pane) = self.pane() else { return };
        // In player mode the player is moved there instead
        // (`teleportTo`): a respawn, flying.
        let look = self.player.as_ref().map(|p| (p.yaw, p.pitch));
        if let (Some((player_yaw, player_pitch)), Some(view)) = (look, self.view.clone()) {
            let camera = pane.camera_state();
            let (at, yaw, pitch) = live::teleport(
                min,
                max,
                player_yaw,
                player_pitch,
                camera.v_fov,
                camera.aspect,
            );
            self.log(
                "teleport",
                serde_json::json!({ "at": at, "yaw": yaw, "pitch": pitch, "min": min, "max": max, "from_pitch": player_pitch, "v_fov": camera.v_fov, "aspect": camera.aspect }),
            );
            // The look the player is stood with is the one it sends next.
            if let Some(player) = &mut self.player {
                player.pitch = pitch;
            }
            self.core.send(Command::Viewer(ViewerInput::SpawnPlayer {
                view,
                at,
                yaw,
                pitch,
                flying: true,
            }));
            return;
        }
        pane.camera(CameraInput::Frame { min, max, ms: 700 });
    }

    // -- overlays -------------------------------------------------------------

    fn drafts(&self, cx: &App) -> Vec<Selection> {
        self.chat
            .as_ref()
            .and_then(|c| c.upgrade())
            .map(|c| c.read(cx).draft.selections.clone())
            .unwrap_or_default()
    }

    /// What is drawn over the world: the bench labels' anchors, then each
    /// draft box's icon anchor; the draft boxes (fill 0.08), the drag box
    /// (outlined, fill 0.16); the marked block. With them, each draft
    /// icon's anchor index and its block, for the markers drawn there.
    fn overlays(&self, cx: &App) -> (Overlays, Vec<IconMark>, Option<usize>) {
        let mut overlays = Overlays::default();
        let mut icons = Vec::new();
        if let Some(state) = &self.state {
            if self
                .view
                .as_ref()
                .is_some_and(|v| v.0.starts_with("bench:"))
            {
                for (_, [min, max]) in &state.layout {
                    overlays.anchors.push(live::label_anchor(*min, *max));
                }
            }
            if self.selectable_chat.is_some() {
                let lift = |c: [i64; 3]| [c[0] as i32, c[1] as i32 + state.y_offset, c[2] as i32];
                let drafts = self.drafts(cx);
                let sent = self
                    .hovered
                    .filter(|h| !drafts.iter().any(|s| (s.min, s.max) == *h));
                if let Some((min, max)) = sent {
                    // A sent message's chip: its box alone, lit, no icon.
                    overlays.boxes.push(OverlayBox {
                        min: lift(min),
                        max: lift(max),
                        fill: 0.22,
                        outline: true,
                    });
                } else {
                    for s in drafts {
                        let (min, max) = (lift(s.min), lift(s.max));
                        let lit = self.hovered == Some((s.min, s.max));
                        icons.push((overlays.anchors.len(), s.block.clone()));
                        overlays.anchors.push(live::icon_anchor(min, max));
                        overlays.boxes.push(OverlayBox {
                            min,
                            max,
                            fill: if lit { 0.22 } else { 0.08 },
                            outline: true,
                        });
                    }
                }
            }
        }
        if let Some(drag) = &self.select
            && let (Some(a), Some(b)) = (drag.start, drag.end.or(drag.start))
        {
            let (min, max) = live::span(a, b);
            overlays.boxes.push(OverlayBox {
                min,
                max,
                fill: 0.16,
                outline: true,
            });
        }
        overlays.marked = self.marked;
        // The marked block's label stands at the centre of the block's top
        // face (BlueMap's PopupMarker: the element at the marker's position
        // plus 0.5, 1, 0.5), while it shows and while it fades out.
        let popup = self
            .marked
            .or(self.closing.map(|(b, _)| b))
            .filter(|_| self.player.is_none())
            .map(|b| {
                overlays
                    .anchors
                    .push([b[0] as f64 + 0.5, b[1] as f64 + 1.0, b[2] as f64 + 0.5]);
                overlays.anchors.len() - 1
            });
        (overlays, icons, popup)
    }

    /// The marked block's label closes (a press, key or wheel on the
    /// world, BlueMap's `removeHandler`): its box at once, the label fading
    /// out over 300 ms.
    fn close_popup(&mut self, cx: &mut Context<Self>) {
        let Some(block) = self.marked.take() else {
            return;
        };
        let at = Instant::now();
        self.closing = Some((block, at));
        self.sync_overlays(cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(live::POPUP_FADE_MS))
                .await;
            let _ = this.update(cx, |pane, cx| {
                if pane.closing.is_some_and(|(_, t)| t == at) {
                    pane.closing = None;
                    pane.sync_overlays(cx);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn sync_overlays(&mut self, cx: &mut Context<Self>) {
        let (overlays, _, _) = self.overlays(cx);
        if self.sent_overlays.as_ref() == Some(&overlays) {
            return;
        }
        if let Some(pane) = self.pane() {
            pane.overlays(overlays.clone());
            self.sent_overlays = Some(overlays);
        }
    }

    // -- controls -----------------------------------------------------------

    fn send_live(&self, command: LiveCommand) {
        // The frames painted so far ride along, for the idle and running
        // measurements (`check_pane_perf.py`).
        let costs = self.store.borrow().costs.clone();
        self.log(
            "live",
            serde_json::json!({
                "command": format!("{command:?}"),
                "frames": costs.uploads.len(),
                "costs": costs_json(&costs),
            }),
        );
        self.core.send(Command::Live(command));
    }

    fn try_again(&self) {
        self.send_live(LiveCommand::Show {
            view: self.view.clone(),
            afresh: true,
        });
    }

    /// Fill the window, or back beside the chat (the `viewer` layer).
    pub fn set_full(&mut self, full: bool, cx: &mut Context<Self>) {
        if full == self.full {
            return;
        }
        self.full = full;
        self.log("fill", serde_json::json!({ "full": full }));
        if full {
            let this = cx.entity().downgrade();
            super::keys::open_layer(cx, Layer::Viewer, move |_, cx| {
                let _ = this.update(cx, |pane, cx| pane.set_full(false, cx));
            });
        } else {
            super::keys::close_layer(cx, Layer::Viewer);
        }
        cx.notify();
    }

    /// Day or night.
    pub fn set_night(&mut self, night: bool, cx: &mut Context<Self>) {
        self.night = night;
        if let Some(pane) = self.pane() {
            pane.time_of_day(if night {
                TimeOfDay::Night
            } else {
                TimeOfDay::Day
            });
        }
        self.log("day night", serde_json::json!({ "night": night }));
        if let Some(on_night) = self.on_night.clone() {
            on_night(night, cx);
        }
        cx.notify();
    }

    /// The selection tool on or off (its button, the `select` bind).
    pub fn toggle_select(&mut self, cx: &mut Context<Self>) {
        if self.selectable_chat.is_none() {
            return;
        }
        self.selecting = !self.selecting;
        self.select = None;
        self.log("select", serde_json::json!({ "on": self.selecting }));
        self.sync_overlays(cx);
        cx.notify();
    }

    fn set_mode(&mut self, mode: CameraMode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.pane() else { return };
        if mode == CameraMode::Player {
            return self.enter_player(cx);
        }
        self.free_pointer(window, cx);
        self.release_keys();
        if self.player.take().is_some() {
            self.core.send(Command::Viewer(ViewerInput::LeavePlayer));
        }
        self.mode = mode;
        pane.camera(CameraInput::Mode(mode));
        self.log(
            "camera mode",
            serde_json::json!({ "mode": format!("{mode:?}") }),
        );
        cx.notify();
    }

    /// Player mode ends with a note: back to the map's perspective view,
    /// the player's leave sent (`leavePlayerMode`).
    fn end_player(&mut self, note: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.free_pointer(window, cx);
        self.release_keys();
        if self.player.take().is_some() {
            self.core.send(Command::Viewer(ViewerInput::LeavePlayer));
        }
        self.mode = CameraMode::Perspective;
        if let Some(pane) = self.pane() {
            pane.camera(CameraInput::Mode(CameraMode::Perspective));
        }
        self.log("player", serde_json::json!({ "on": false, "note": note }));
        self.show_note(note.into(), cx);
    }

    fn enter_player(&mut self, cx: &mut Context<Self>) {
        let (Some(pane), Some(view)) = (self.pane(), self.view.clone()) else {
            return;
        };
        if self.player.is_some() {
            return;
        }
        let running = self
            .state
            .as_ref()
            .is_some_and(|s| s.phase == LivePhase::Shown && s.running.is_some());
        if !running {
            return self.show_note(live::NEEDS_RUNNING.into(), cx);
        }
        let camera = pane.camera_state();
        let (yaw, pitch) = live::start_look(camera.rotation, camera.angle);
        self.core.send(Command::Viewer(ViewerInput::EnterPlayer {
            view,
            target: camera.target,
            rotation: camera.rotation,
            yaw,
            pitch,
        }));
        pane.camera(CameraInput::Mode(CameraMode::Player));
        self.mode = CameraMode::Player;
        self.player = Some(Player {
            yaw,
            pitch,
            map_eye: camera.eye,
            ..Player::default()
        });
        self.release_keys();
        self.log("player", serde_json::json!({ "on": true }));
        cx.notify();
    }

    fn export(&mut self, world: bool, cx: &mut Context<Self>) {
        let name = live::export_name(
            (!self.title.is_empty() && self.bench.is_none()).then_some(self.title.as_str()),
            self.bench.as_ref().map(|(t, r)| (t.as_str(), *r)),
        );
        let file = if world {
            format!("{name}.zip")
        } else {
            format!("{name}.schem")
        };
        let saver = cx.global::<SaveTo>().0.clone();
        let answer = saver(&file, cx);
        let core = self.core.clone();
        let chat = self.selectable_chat.clone();
        let view = self.view.clone();
        let log = self.log.clone();
        cx.spawn(async move |this, cx| {
            let Ok(answer) = answer.await else { return };
            let to = match answer {
                Ok(Some(to)) => to,
                Ok(None) => return,
                Err(error) => {
                    let _ = this.update(cx, |pane, cx| {
                        pane.announcer.update(cx, |a, cx| a.assertive(error, cx));
                    });
                    return;
                }
            };
            if let Some(log) = &log {
                log.record("export", serde_json::json!({ "world": world, "to": to.file_name().map(|n| n.to_string_lossy().into_owned()) }));
            }
            match (world, chat, view) {
                (false, Some(chat), _) => core.send(Command::ExportSchematic { chat, to }),
                (true, _, Some(view)) => core.send(Command::ExportWorld { view, name, to }),
                _ => {}
            }
        })
        .detach();
    }

    // -- pointer --------------------------------------------------------------

    fn mods(m: &Modifiers) -> Mods {
        Mods {
            control: m.control,
            alt: m.alt,
            shift: m.shift,
            platform: m.platform,
        }
    }

    fn pick(&self, purpose: Purpose, at: [f32; 2]) -> bool {
        let (Some(pane), Some((seq, at))) = (self.pane(), self.store.borrow().in_frame(at)) else {
            return false;
        };
        // The frame's camera and size with it: a block's pixel can be
        // projected back through them (verify/linux/real_lever.py).
        let (camera, size) = match &self.store.borrow().shown {
            Some(shown) => (Some(shown.camera), Some(shown.size)),
            None => (None, None),
        };
        self.log(
            "pick",
            serde_json::json!({
                "purpose": format!("{purpose:?}"),
                "seq": seq,
                "at": at,
                "size": size,
                "camera": camera.map(|c| serde_json::json!({
                    "mode": format!("{:?}", c.mode),
                    "target": c.target,
                    "eye": c.eye,
                    "rotation": c.rotation,
                    "angle": c.angle,
                    "v_fov": c.v_fov,
                    "aspect": c.aspect,
                })),
            }),
        );
        let sender = self.picks.clone();
        pane.pick(
            seq,
            at,
            Box::new(move |pick| {
                let _ = sender.unbounded_send((purpose, pick));
            }),
        );
        true
    }

    fn centre(&self) -> Option<[f32; 2]> {
        self.store
            .borrow()
            .physical()
            .map(|[w, h]| [w as f32 / 2.0, h as f32 / 2.0])
    }

    fn picked(&mut self, purpose: Purpose, pick: Pick, cx: &mut Context<Self>) {
        self.log(
            "picked",
            serde_json::json!({ "purpose": format!("{purpose:?}"), "seq": pick.seq, "block": pick.block, "view": pick.view.0 }),
        );
        // A pick for a view the window has left is dropped.
        if Some(&pick.view) != self.view.as_ref() {
            return;
        }
        match purpose {
            Purpose::Click | Purpose::Use => {
                let Some(block) = pick.block else { return };
                if purpose == Purpose::Use {
                    // The block at the crosshair, its distance from the eye
                    // and whether the reach lets it be used.
                    let distance = self.reach_distance(block);
                    let within = distance.is_some_and(|d| d <= REACH);
                    self.log(
                        "use",
                        serde_json::json!({ "block": block, "distance": distance, "within_reach": within }),
                    );
                    if !within {
                        return;
                    }
                }
                let revision = self.state.as_ref().and_then(|s| s.revision).unwrap_or(0);
                let yaw = match &self.player {
                    Some(p) => p.yaw,
                    None => pick.yaw,
                };
                self.core.send(Command::Viewer(ViewerInput::Click {
                    view: pick.view,
                    revision,
                    block,
                    yaw,
                }));
                self.log("click", serde_json::json!({ "block": block }));
                if self.player.is_none() {
                    self.marked = Some(block);
                    self.marked_opened = Some(Instant::now());
                    self.closing = None;
                    self.sync_overlays(cx);
                }
            }
            Purpose::SelectStart => {
                let Some(select) = &mut self.select else {
                    return;
                };
                select.in_flight = false;
                match pick.block {
                    Some(block) => {
                        select.start = Some(block);
                        select.end = Some(block);
                    }
                    // Nothing starts on the sky.
                    None => self.select = None,
                }
                self.next_select_pick();
                self.sync_overlays(cx);
            }
            Purpose::SelectMove => {
                let Some(select) = &mut self.select else {
                    return;
                };
                select.in_flight = false;
                if let Some(block) = pick.block {
                    select.end = Some(block);
                }
                self.next_select_pick();
                self.sync_overlays(cx);
            }
        }
        cx.notify();
    }

    /// The distance from the eye to `block`'s centre.
    fn reach_distance(&self, block: [i32; 3]) -> Option<f64> {
        let eye = self.pane()?.camera_state().eye;
        Some(
            (0..3)
                .map(|a| (block[a] as f64 + 0.5 - eye[a]).powi(2))
                .sum::<f64>()
                .sqrt(),
        )
    }

    fn next_select_pick(&mut self) {
        let Some(select) = &mut self.select else {
            return;
        };
        if select.in_flight || select.start.is_none() {
            return;
        }
        let Some((_, at)) = select.waiting.take() else {
            return;
        };
        select.in_flight = true;
        if !self.pick(Purpose::SelectMove, at)
            && let Some(select) = &mut self.select
        {
            select.in_flight = false;
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.store.borrow().at(event.position) else {
            return;
        };
        self.close_popup(cx);
        window.focus(&self.focus, cx);
        self.keyboard = false;
        let Some(pane) = self.pane() else { return };
        // A double click fills the window or leaves it in any mode, player
        // mode included (`web/bluemap-bridge.js` 951-955).
        if self.player.is_some() && event.click_count == 2 && event.button == MouseButton::Left {
            let full = !self.full;
            self.set_full(full, cx);
        }
        if self.player.is_some() {
            match event.button {
                MouseButton::Left if !self.locked => self.take_pointer(window, cx),
                MouseButton::Right if self.locked => {
                    if let Some(centre) = self.centre() {
                        self.pick(Purpose::Use, centre);
                    }
                }
                _ => {}
            }
            cx.notify();
            return;
        }
        if event.click_count == 2 && event.button == MouseButton::Left {
            if let Some((seq, at)) = self.store.borrow().in_frame(at) {
                pane.camera(CameraInput::DoubleClick { seq, at });
            }
            let full = !self.full;
            self.set_full(full, cx);
        }
        pane.camera(CameraInput::PointerDown);
        if self.selecting && event.button == MouseButton::Left {
            self.select = Some(SelectDrag {
                in_flight: true,
                ..SelectDrag::default()
            });
            if !self.pick(Purpose::SelectStart, at) {
                self.select = None;
            }
            self.drag = Some(Drag {
                mouse: MouseButton::Left,
                button: None,
                down: at,
                last: at,
                down_at: Instant::now(),
                moved: [0.0; 2],
                moves: 0,
            });
            return;
        }
        let button = match event.button {
            MouseButton::Left if event.modifiers.alt => Some(DragButton::AltLeft),
            MouseButton::Left if event.modifiers.control => Some(DragButton::CtrlLeft),
            MouseButton::Left => Some(DragButton::Left),
            MouseButton::Right => Some(DragButton::Right),
            _ => None,
        };
        self.drag = Some(Drag {
            mouse: event.button,
            button,
            down: at,
            last: at,
            down_at: Instant::now(),
            moved: [0.0; 2],
            moves: 0,
        });
    }

    fn mouse_move(&mut self, position: Point<Pixels>) {
        let Some(at) = self.store.borrow().at(position) else {
            return;
        };
        if self.player.is_some() || self.locked {
            // In player mode and under spectator's lock the look comes from
            // the locked pointer's motion ([`Pane::motion`]), not from
            // positions (BlueMap's drags look only while not locked).
            return;
        }
        let Some(drag) = &mut self.drag else { return };
        let (dx, dy) = (at[0] - drag.last[0], at[1] - drag.last[1]);
        drag.last = at;
        drag.moved = [drag.moved[0] + dx, drag.moved[1] + dy];
        drag.moves += 1;
        match drag.button {
            Some(button) => {
                if let Some(pane) = self.pane() {
                    pane.camera(CameraInput::Drag { button, dx, dy });
                }
            }
            None => {
                if let Some(select) = &mut self.select {
                    let seq = self.store.borrow().shown.as_ref().map_or(0, |s| s.seq);
                    // The latest pointer replaces a waiting one.
                    select.waiting = Some((seq, at));
                }
                self.next_select_pick();
            }
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        if drag.mouse != event.button {
            self.drag = Some(drag);
            return;
        }
        let at = self.store.borrow().at(event.position).unwrap_or(drag.last);
        let ms = drag.down_at.elapsed().as_millis() as u64;
        // Each press and release on the world, with what it moved: a
        // drag that changed nothing shows here whether the pane had it.
        self.log(
            "drag",
            serde_json::json!({
                "button": format!("{:?}", drag.button),
                "down": drag.down,
                "up": at,
                "moved": drag.moved,
                "moves": drag.moves,
                "ms": ms,
            }),
        );
        match (drag.button, event.button) {
            (None, MouseButton::Left) => {
                // The selection's release: named, then a chip.
                if let Some(select) = self.select.take()
                    && let (Some(a), Some(b)) = (select.start, select.end)
                {
                    let (min, max) = live::span(a, b);
                    self.describe(min, max);
                }
                self.sync_overlays(cx);
            }
            (Some(DragButton::Left), MouseButton::Left) if live::is_click(0, ms, drag.down, at) => {
                self.pick(Purpose::Click, at);
                // In spectator the click also takes the pointer
                // (FreeFlightControls.js 146-165).
                if self.mode == CameraMode::Spectator && !self.locked {
                    self.take_pointer(window, cx);
                }
            }
            (Some(DragButton::Right), MouseButton::Right)
                if live::is_right_click(drag.down, at) =>
            {
                self.remove_box_at(at, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn describe(&mut self, min: [i32; 3], max: [i32; 3]) {
        let Some(chat) = self.selectable_chat.clone() else {
            return;
        };
        let y = self.state.as_ref().map_or(0, |s| s.y_offset);
        let (bmin, bmax) = live::build_box(min, max, y);
        self.next_request += 1;
        let request = self.next_request;
        self.asked.insert(request, (min, max, self.navigations));
        self.log("selection", serde_json::json!({ "min": bmin, "max": bmax }));
        self.core.send(Command::DescribeSelection {
            chat,
            request,
            min: bmin,
            max: bmax,
        });
    }

    /// A right click on a draft box removes the nearest one the ray enters.
    fn remove_box_at(&mut self, at: [f32; 2], cx: &mut Context<Self>) {
        let (Some(pane), Some(chat)) = (self.pane(), self.chat.as_ref().and_then(|c| c.upgrade()))
        else {
            return;
        };
        let Some(pane_px) = self.store.borrow().physical() else {
            return;
        };
        let y = self.state.as_ref().map_or(0, |s| s.y_offset);
        let camera = pane.camera_state();
        let (o, d) = ray(&camera, at, pane_px);
        let nearest = chat
            .read(cx)
            .draft
            .selections
            .iter()
            .filter_map(|s| {
                let lift = |c: [i64; 3]| [c[0] as f64, (c[1] + y as i64) as f64, c[2] as f64];
                enter(o, d, lift(s.min), lift(s.max)).map(|t| (t, s.letter.clone()))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, letter)) = nearest {
            chat.update(cx, |c, cx| {
                c.draft.selections.retain(|s| s.letter != letter);
                cx.notify();
            });
            self.log("removed selection", serde_json::json!({ "letter": letter }));
            self.sync_overlays(cx);
        }
    }

    fn wheel(&mut self, event: &ScrollEvent) {
        if self.player.is_some() {
            return;
        }
        let Some(pane) = self.pane() else { return };
        let (delta, lines) = match event.delta {
            ScrollDelta::Lines(p) => (-p.y, true),
            ScrollDelta::Pixels(p) => (-f32::from(p.y), false),
        };
        if delta != 0.0 {
            // What one notch sends (check_pane.py counts them).
            self.log(
                "wheel",
                serde_json::json!({ "delta": delta, "lines": lines }),
            );
            pane.camera(CameraInput::Wheel { delta, lines });
        }
    }

    fn pinch(&mut self, event: &PinchEvent) {
        if let Some(pane) = self.pane()
            && self.player.is_none()
        {
            pane.camera(CameraInput::Pinch {
                scale: 1.0 + event.delta,
            });
        }
    }

    // -- keys -----------------------------------------------------------------

    fn kind(&self) -> CameraKind {
        match self.mode {
            CameraMode::Spectator => CameraKind::Spectator,
            _ => CameraKind::Map,
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        let mods = Self::mods(&stroke.modifiers);
        let key = stroke.key.as_str();
        if self.player.is_some() {
            if let Some(bit) = live::player_key(key, mods) {
                cx.stop_propagation();
                if event.is_held {
                    return;
                }
                let player = self.player.as_mut().expect("checked");
                if player.held.insert(key.to_string(), bit).is_none() && player.keys & bit == 0 {
                    player.keys |= bit;
                    player.taps |= bit;
                    self.player_dirty(window, cx);
                }
            }
            // Every key down and up player mode receives, with the bits it
            // leaves held (a key that stayed down shows here).
            let keys = self.player.as_ref().map_or(0, |p| p.keys);
            self.log(
                "player key",
                serde_json::json!({ "key": key, "down": true, "mods": format!("{mods:?}"), "keys": keys, "t_ms": since_start_ms() }),
            );
            return;
        }
        if key == "enter" && mods == Mods::default() {
            // Enter uses the block at the centre (keyboard focus marks it).
            self.keyboard = true;
            if let Some(centre) = self.centre() {
                self.pick(Purpose::Click, centre);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self
            .view
            .as_ref()
            .is_some_and(|v| v.0.starts_with("bench:"))
            && mods == Mods::default()
            && let Some(digit) = key.parse::<usize>().ok().filter(|d| (1..=9).contains(d))
        {
            self.focus_member(digit - 1);
            cx.stop_propagation();
            return;
        }
        if let Some(bit) = live::camera_key(key, mods, self.kind()) {
            cx.stop_propagation();
            self.keyboard = true;
            if event.is_held {
                return;
            }
            self.key_bits.insert(key.to_string(), bit);
            if self.held & bit == 0 {
                self.held |= bit;
                if let Some(pane) = self.pane() {
                    pane.camera(CameraInput::Keys(self.held));
                }
            }
        }
        let _ = window;
    }

    /// Escape (`CloseLayer`) with the pane's focus: it stops the selection
    /// tool, or frees a locked pointer, and goes no further; else
    /// it closes the topmost layer as anywhere (`web/bluemap-bridge.js`
    /// 1079-1086).
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.toggle_select(cx);
        } else if self.locked {
            self.free_pointer(window, cx);
        } else {
            cx.propagate();
        }
    }

    /// A click takes the pointer (`requestPointerLock`); a refusal is said
    /// as a note and the pointer stays free.
    fn take_pointer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let take = cx.global::<TakePointer>().0.clone();
        match take(window) {
            Ok(()) => {
                self.locked = true;
                let yaw = self.player.as_ref().map(|p| p.yaw);
                let mode = format!("{:?}", self.mode);
                self.log(
                    "pointer",
                    serde_json::json!({ "captured": true, "mode": mode, "yaw": yaw }),
                );
            }
            Err(error) => {
                let note = format!("The pointer could not be taken: {error}");
                self.log("pointer", serde_json::json!({ "refused": note }));
                self.show_note(note, cx);
            }
        }
        cx.notify();
    }

    /// The locked pointer moved (`movementX`, `movementY`). In spectator it
    /// goes to the renderer as physical pixels ([`CameraInput::LockedLook`]).
    /// In player mode the look turns by 0.15° a pixel, pitch held to ±90;
    /// the renderer turns at once, the core hears it at most once a paint.
    pub fn motion(&mut self, delta: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.locked {
            return;
        }
        if self.mode == CameraMode::Spectator {
            let scale = window.scale_factor();
            let (dx, dy) = (f32::from(delta.x) * scale, f32::from(delta.y) * scale);
            if let Some(pane) = self.pane() {
                pane.camera(CameraInput::LockedLook { dx, dy });
            }
            self.log("locked look", serde_json::json!({ "dx": dx, "dy": dy }));
            return;
        }
        let Some(player) = &mut self.player else {
            return;
        };
        player.yaw += f32::from(delta.x) * live::LOOK_PER_PIXEL;
        player.pitch =
            (player.pitch + f32::from(delta.y) * live::LOOK_PER_PIXEL).clamp(-90.0, 90.0);
        let (yaw, pitch) = (player.yaw, player.pitch);
        if let Some(pane) = self.pane() {
            pane.camera(CameraInput::Look { yaw, pitch });
        }
        self.log("look", serde_json::json!({ "yaw": yaw, "pitch": pitch }));
        self.player_dirty(window, cx);
    }

    /// Gives the pointer back where it was taken (Escape, the window losing
    /// focus or hidden, a modal layer, leaving player mode or spectator).
    fn free_pointer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.locked {
            return;
        }
        self.locked = false;
        if let Err(error) = window.set_pointer_locked(false) {
            self.log(
                "pointer",
                serde_json::json!({ "release_failed": error.to_string() }),
            );
        }
        self.log("pointer", serde_json::json!({ "captured": false }));
        self.release_keys();
        cx.notify();
    }

    fn key_up(&mut self, event: &KeyUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if let Some(player) = &mut self.player {
            let was_held = player.held.remove(key);
            if let Some(bit) = was_held {
                if !player.held.values().any(|b| *b == bit) {
                    player.keys &= !bit;
                }
                self.player_dirty(window, cx);
            }
            let keys = self.player.as_ref().map_or(0, |p| p.keys);
            self.log(
                "player key",
                serde_json::json!({ "key": key, "down": false, "was_held": was_held.is_some(), "keys": keys, "t_ms": since_start_ms() }),
            );
            return;
        }
        // A held key's release always releases it, whatever is held.
        if let Some(bit) = self.key_bits.remove(key) {
            if !self.key_bits.values().any(|b| *b == bit) {
                self.held &= !bit;
            }
            if let Some(pane) = self.pane() {
                pane.camera(CameraInput::Keys(self.held));
            }
        }
    }

    fn modifiers(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Spectator: Shift goes down (BlueMap's free flight, `ShiftLeft`
        // with PageDown; GPUI does not say which Shift).
        if self.player.is_none() && self.mode == CameraMode::Spectator {
            let shift = event.modifiers.shift;
            let was = self.key_bits.contains_key("shift");
            if shift != was {
                if shift {
                    self.key_bits
                        .insert("shift".into(), crate::camera_keys::DOWN);
                    self.held |= crate::camera_keys::DOWN;
                } else {
                    self.key_bits.remove("shift");
                    if !self
                        .key_bits
                        .values()
                        .any(|b| *b == crate::camera_keys::DOWN)
                    {
                        self.held &= !crate::camera_keys::DOWN;
                    }
                }
                if let Some(pane) = self.pane() {
                    pane.camera(CameraInput::Keys(self.held));
                }
            }
            return;
        }
        let Some(player) = &mut self.player else {
            return;
        };
        let m = &event.modifiers;
        let mut keys = player.keys & !(live::SNEAK | live::SPRINT);
        if m.shift {
            keys |= live::SNEAK;
        }
        if m.control {
            keys |= live::SPRINT;
        }
        if keys != player.keys {
            player.taps |= keys & !player.keys;
            player.keys = keys;
            self.player_dirty(window, cx);
            self.log(
                "player key",
                serde_json::json!({ "modifiers": format!("{m:?}"), "keys": keys, "t_ms": since_start_ms() }),
            );
        }
    }

    /// A hold ended without a key's up (focus left the pane, the pointer
    /// freed, the window hidden, player mode ending, a modal layer): every
    /// key released, and in player mode the released `PlayerInput` sent
    /// now, not on the next paint (a hidden window may not paint).
    fn release_keys(&mut self) {
        self.key_bits.clear();
        if self.held != 0 {
            self.held = 0;
            if let Some(pane) = self.pane() {
                pane.camera(CameraInput::Keys(0));
            }
        }
        if let Some(player) = &mut self.player {
            player.held.clear();
            if player.keys != 0 {
                player.keys = 0;
                player.dirty = true;
                self.log(
                    "player key",
                    serde_json::json!({ "released": "all", "keys": 0, "t_ms": since_start_ms() }),
                );
                self.flush_player();
            }
        }
    }

    /// The player's input goes to the core at most once per paint.
    fn player_dirty(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(player) = &mut self.player else {
            return;
        };
        player.dirty = true;
        if player.flush {
            return;
        }
        player.flush = true;
        cx.on_next_frame(window, |pane, _, _| pane.flush_player());
    }

    fn flush_player(&mut self) {
        // The eye of the frame on screen, for the line below: a driver
        // steers by it (verify/linux/real_lever_walk.py).
        let eye = self.pane().map(|p| p.camera_state().eye);
        let Some(player) = &mut self.player else {
            return;
        };
        player.flush = false;
        if !player.dirty {
            return;
        }
        player.dirty = false;
        let input = ViewerInput::PlayerInput {
            keys: player.keys,
            taps: player.taps,
            yaw: player.yaw,
            pitch: player.pitch,
        };
        player.taps = 0;
        if let ViewerInput::PlayerInput {
            keys,
            taps,
            yaw,
            pitch,
        } = &input
        {
            self.log(
                "player input",
                serde_json::json!({ "keys": keys, "taps": taps, "yaw": yaw, "pitch": pitch, "eye": eye, "t_ms": since_start_ms() }),
            );
        }
        self.core.send(Command::Viewer(input));
    }
}

/// How far from the eye a block's centre may be to be used in player mode
/// (5 blocks to the centre, with the half diagonal of a block).
const REACH: f64 = 5.0 + 0.87;

/// Half the box the block's label is centred in: wider and taller than
/// the label (`Block: x: … y: … z: …` with its copy button).
const POPUP_HALF_W: f32 = 200.;
const POPUP_HALF_H: f32 = 24.;

/// Milliseconds since the process started (`crash::process_started`), for
/// the player's key and input lines.
fn since_start_ms() -> f64 {
    crate::crash::process_started().elapsed().as_secs_f64() * 1e3
}

fn durations(values: &[std::time::Duration]) -> serde_json::Value {
    let mut ms: Vec<f64> = values.iter().map(|d| d.as_secs_f64() * 1e3).collect();
    ms.sort_by(f64::total_cmp);
    if ms.is_empty() {
        return serde_json::json!({ "count": 0 });
    }
    let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
    serde_json::json!({
        "count": ms.len(),
        "mean_ms": ms.iter().sum::<f64>() / ms.len() as f64,
        "p50_ms": at(0.5),
        "p95_ms": at(0.95),
        "p99_ms": at(0.99),
        "worst_ms": at(1.0),
    })
}

fn costs_json(costs: &frame::Costs) -> serde_json::Value {
    let mut ms: Vec<f64> = costs
        .uploads
        .iter()
        .map(|d| d.as_secs_f64() * 1e3)
        .collect();
    ms.sort_by(f64::total_cmp);
    let at = |q: f64| {
        if ms.is_empty() {
            0.0
        } else {
            ms[((ms.len() - 1) as f64 * q).round() as usize]
        }
    };
    serde_json::json!({
        "frames": ms.len(),
        "dropped": costs.dropped,
        "repaints": costs.repaints,
        "mean_ms": if ms.is_empty() { 0.0 } else { ms.iter().sum::<f64>() / ms.len() as f64 },
        "p50_ms": at(0.5),
        "p99_ms": at(0.99),
        "worst_ms": at(1.0),
        "input_to_paint": durations(&costs.input_to_paint),
        // The frame path split: the window's wait from a frame's arrival
        // to its take, and the renderer's time from a take to the next
        // frame's arrival.
        "wait": durations(&costs.waits),
        "render": durations(&costs.renders),
        // The last frames painted: how long ago, and their counters.
        "recent": costs
            .recent
            .iter()
            .map(|(at, seq)| serde_json::json!({ "ms_ago": at.elapsed().as_millis() as u64, "seq": seq }))
            .collect::<Vec<_>>(),
    })
}

/// The ray of a pane pixel from a camera (BlueMap's orbit: the heading
/// looks along `(sin r, 0, -cos r)`), for a right click's box removal.
fn ray(c: &crate::CameraState, at: [f32; 2], pane: [u32; 2]) -> ([f64; 3], [f64; 3]) {
    let norm = |a: [f64; 3]| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let forward = norm([
        c.target[0] - c.eye[0],
        c.target[1] - c.eye[1],
        c.target[2] - c.eye[2],
    ]);
    let right = [c.rotation.cos(), 0.0, c.rotation.sin()];
    let up = [
        right[1] * forward[2] - right[2] * forward[1],
        right[2] * forward[0] - right[0] * forward[2],
        right[0] * forward[1] - right[1] * forward[0],
    ];
    let t = (c.v_fov.to_radians() / 2.0).tan();
    let (w, h) = (pane[0].max(1) as f64, pane[1].max(1) as f64);
    let nx = (2.0 * at[0] as f64 / w - 1.0) * t * (w / h);
    let ny = (1.0 - 2.0 * at[1] as f64 / h) * t;
    (
        c.eye,
        norm([0, 1, 2].map(|a| forward[a] + right[a] * nx + up[a] * ny)),
    )
}

/// Where a ray enters a box of blocks, both corners included.
fn enter(o: [f64; 3], d: [f64; 3], min: [f64; 3], max: [f64; 3]) -> Option<f64> {
    let (mut near, mut far) = (0.0f64, f64::INFINITY);
    for a in 0..3 {
        let (lo, hi) = (min[a], max[a] + 1.0);
        if d[a].abs() < 1e-12 {
            if o[a] < lo || o[a] > hi {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((lo - o[a]) / d[a], (hi - o[a]) / d[a]);
        near = near.max(t0.min(t1));
        far = far.min(t0.max(t1));
        if near > far {
            return None;
        }
    }
    Some(near)
}

// -- drawing ----------------------------------------------------------------

/// A button of BlueMap's bar (`SvgButton.vue`): 32 by 32 on `--theme-bg`,
/// its picture 28.8 px, `--theme-bg-hover` under the pointer,
/// `--theme-bg-light` while active; named by today's title, toggled for a
/// switch, a focus ring, a Tab stop. `separated` draws the 1 px
/// `--theme-bg-light` line on its left that the bar puts between items.
#[allow(clippy::too_many_arguments)]
fn bar_button(
    id: &'static str,
    picture: AnyElement,
    name: impl Into<SharedString>,
    pressed: Option<bool>,
    separated: bool,
    window: &mut Window,
    cx: &mut Context<Pane>,
    press: impl Fn(&mut Pane, &mut Window, &mut Context<Pane>) + 'static,
) -> AnyElement {
    let name = name.into();
    // Its title, the browser's tooltip (BlueMap's buttons' titles are their
    // names; the selection tool's says more, `web/bluemap-bridge.js` 108).
    let title: SharedString = if name.as_ref() == live::SELECT {
        live::SELECT_TITLE.into()
    } else {
        name.clone()
    };
    let focus = super::controls::focus_for(id, window, cx).tab_stop(true);
    let press = Rc::new(press);
    let click = press.clone();
    let element = super::ring(
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(name.clone())
            .when_some(pressed, |d, p| {
                d.aria_toggled(if p {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                })
            })
            .track_focus(&focus)
            .key_context("Part"),
    )
    .flex_none()
    .size(px(32.))
    .flex()
    .items_center()
    .justify_center()
    .bg(if pressed == Some(true) {
        colour::live_bar_light()
    } else {
        colour::live_bar()
    })
    .when(separated, |d| {
        d.border_l_1().border_color(colour::live_bar_light())
    })
    .cursor_pointer()
    .when(pressed != Some(true), |d| {
        d.hover(|s| s.bg(colour::live_bar_hover()))
    })
    .on_click(cx.listener(move |pane, _: &ClickEvent, window, cx| click(pane, window, cx)))
    .on_action(cx.listener(move |pane, _: &super::Activate, window, cx| press(pane, window, cx)))
    .child(picture);
    super::controls::tip(element, SharedString::from(format!("{id}-tip")), title).into_any_element()
}

/// The day/night switch's picture (`DayNightSwitch.vue`): the moon in
/// `--theme-moon-day`, the stars in `--theme-stars-day`.
fn day_night_picture() -> AnyElement {
    div()
        .relative()
        .size(px(28.8))
        .child(div().absolute().inset_0().child(bluemap::mask(
            "day-night-moon",
            gpui_kit::rgb(0xffff00).into(),
            28.8,
            0.,
        )))
        .child(div().absolute().inset_0().child(bluemap::mask(
            "day-night-stars",
            colour::live_bar_text(),
            28.8,
            0.,
        )))
        .into_any_element()
}

/// The compass (`Compass.vue`): north in `--theme-fg`, south in
/// `--theme-fg-light`, turned against the camera's heading.
fn compass_picture(turn: f32) -> AnyElement {
    div()
        .relative()
        .size(px(28.8))
        .child(div().absolute().inset_0().child(bluemap::mask(
            "compass-north",
            colour::live_bar_text(),
            28.8,
            turn,
        )))
        .child(div().absolute().inset_0().child(bluemap::mask(
            "compass-south",
            colour::live_bar_dim(),
            28.8,
            turn,
        )))
        .into_any_element()
}

/// One of BlueMap's position fields (`NumberInput.vue`): `x:` in
/// `--theme-fg-light` (1 em wide, 0.5 em either side), the value in
/// `--theme-fg` on `--theme-bg`, 32 px tall.
fn position_field(
    label: &'static str,
    input: &Entity<InputState>,
    separated: bool,
    cx: &App,
) -> AnyElement {
    let fonts = fonts(cx);
    div()
        .flex_1()
        .min_w(px(0.))
        .h(px(32.))
        .flex()
        .items_center()
        .bg(colour::live_bar())
        .when(separated, |d| {
            d.border_l_1().border_color(colour::live_bar_light())
        })
        .child(
            theme::text(div(), TextKind::LiveBar, &fonts)
                .text_color(colour::live_bar_dim())
                .w(px(16.))
                .mx(px(8.))
                .child(format!("{label}:")),
        )
        .child(
            div().flex_1().min_w(px(0.)).child(
                Input::new(input)
                    .aria_label(format!("Camera position {label}"))
                    .appearance(false)
                    .text_size(px(16.))
                    .text_color(colour::live_bar_text()),
            ),
        )
        .into_any_element()
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        cx.set_global(PaneKeys {
            full: self.full,
            player: self.player.is_some(),
        });
        let fonts = fonts(cx);
        let status = self.status.clone();
        let painted = self.painted.as_ref().map(|(v, r)| (v, *r));
        let chrome = live::chrome(&live::Seen {
            pane_error: self.pane_error.as_deref(),
            status: Some(&status),
            view: self.view.as_ref(),
            state: self.state.as_ref(),
            painted,
            bench_mode: self.bench_mode,
            preparing: self.preparing,
            has_pane: self.pane().is_some(),
        });
        let state = self
            .state
            .clone()
            .filter(|s| s.view.as_ref() == self.view.as_ref());
        let described = live::description(
            &self.title,
            state.as_ref(),
            chrome.cover.as_ref().map(|(t, _)| t.as_str()),
        );
        let has_world = self.pane().is_some() && chrome.cover.is_none();
        let playable = has_world
            && state
                .as_ref()
                .is_some_and(|s| s.phase == LivePhase::Shown && s.running.is_some());

        // The cover and the banner.
        let offer_button = |id: &'static str, cx: &mut Context<Self>| {
            super::controls::Button::new(id, "Try again").on_press({
                let this = cx.entity().downgrade();
                move |_, cx| {
                    let _ = this.update(cx, |pane, _| pane.try_again());
                }
            })
        };
        let cover = chrome.cover.clone().map(|(text, offer)| {
            div()
                .id("live-cover")
                .role(Role::Status)
                .aria_label(text.clone())
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(9.))
                .bg(colour::viewer())
                .text_color(colour::ash())
                .child(icons::icon(Icon::Build, 20.))
                // The sentence wraps in a narrow pane, as the page's span does.
                .child(
                    theme::text(div(), TextKind::LiveCover, &fonts)
                        .min_w_0()
                        .child(text),
                )
                .when(offer == Some(Offer::TryAgain), |d| {
                    d.child(offer_button("live-cover-try", cx))
                })
        });
        let banner = chrome.banner.clone().map(|(text, offer)| {
            div()
                .id("live-banner")
                .role(Role::Status)
                .aria_label(text.clone())
                .occlude()
                .absolute()
                .left(px(16.))
                .right(px(16.))
                .bottom(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .px(px(16.))
                .py(px(12.))
                .border_1()
                .border_color(colour::line())
                .rounded(px(10.))
                .bg(colour::raised())
                .child(
                    theme::text(div(), TextKind::LiveBanner, &fonts)
                        .min_w_0()
                        .child(text),
                )
                .when(offer == Some(Offer::TryAgain), |d| {
                    d.child(offer_button("live-banner-try", cx))
                })
        });

        // The toolbar: play or pause, step, restart, the tick counter.
        let toolbar = (chrome.toolbar && state.is_some()).then(|| {
            let t = live::toolbar(state.as_ref().expect("checked"));
            let this = cx.entity().downgrade();
            let play = {
                let this = this.clone();
                let playing = t.playing;
                super::controls::Button::live(
                    "live-play",
                    if playing { Icon::Pause } else { Icon::Play },
                    if playing { "Pause" } else { "Play" },
                )
                .tip(if playing { "Pause" } else { "Play" })
                .disabled(!t.play_enabled)
                .on_press(move |_, cx| {
                    let _ = this.update(cx, |pane, _| {
                        pane.send_live(if playing {
                            LiveCommand::Pause
                        } else {
                            LiveCommand::Play
                        })
                    });
                })
            };
            let step = {
                let this = this.clone();
                super::controls::Button::live("live-step", Icon::Step, live::STEP)
                    .tip(live::STEP)
                    .disabled(!t.step_enabled)
                    .on_press(move |_, cx| {
                        let _ = this.update(cx, |pane, _| pane.send_live(LiveCommand::Step));
                    })
            };
            let restart =
                super::controls::Button::live("live-restart", Icon::Restart, live::RESTART)
                    .tip(live::RESTART)
                    .on_press(move |_, cx| {
                        let _ = this.update(cx, |pane, _| pane.send_live(LiveCommand::Restart));
                    });
            div()
                .id("live-toolbar")
                .role(Role::Toolbar)
                .aria_label(live::TOOLBAR)
                .occlude()
                .absolute()
                .left(px(12.))
                .bottom(px(12.))
                .flex()
                .items_center()
                .gap(px(2.))
                .p(px(4.))
                .border_1()
                .border_color(colour::line())
                .rounded(px(10.))
                .bg(colour::raised())
                .child(play)
                .child(step)
                .child(restart)
                .child(super::controls::pointer_tip(
                    theme::text(div(), TextKind::LiveTicks, &fonts)
                        .id("live-ticks")
                        .role(Role::Label)
                        .aria_label(format!("{}: {}", live::TICKS_TITLE, t.ticks))
                        // AT-SPI names a label by its value.
                        .aria_value(format!("{}: {}", live::TICKS_TITLE, t.ticks))
                        .pl(px(4.))
                        .pr(px(8.))
                        .child(t.ticks),
                    "live-ticks-tip",
                    live::TICKS_TITLE,
                ))
        });

        // BlueMap's control bar with the mcbench panel at its end
        // (`ControlBar.vue`, `web/bluemap-bridge.js` 94-158): one line along
        // the pane's top, 8 px in; at 575.98 px wide and narrower it spans
        // the pane on `--theme-bg` and hides Day/Night, the view switch and
        // Reset, as BlueMap's `thin-hide`. BlueMap's menu is not carried.
        let pane_width = self
            .store
            .borrow()
            .bounds
            .map_or(f32::from(window.viewport_size().width), |(b, _)| {
                f32::from(b.size.width)
            });
        let narrow = pane_width <= bluemap::NARROW;
        let mut bar = div()
            .id("live-bar")
            .role(Role::Toolbar)
            .aria_label(live::WORLD_CONTROLS)
            // A press on a control is not also a press on the world under it.
            .occlude()
            .absolute()
            .flex()
            .items_center()
            .h(px(32.))
            .map(|d| {
                if narrow {
                    d.top_0().left_0().right_0().bg(colour::live_bar())
                } else {
                    d.top(px(8.)).left(px(8.)).right(px(8.))
                }
            });
        // Day/Night, the view switch and Reset: on the bar beside the
        // fields; in a narrow pane, where BlueMap moves them into its menu
        // (`thin-hide`), which is not carried, on a second row under it.
        let mut modes = div().flex().flex_none().items_center().h(px(32.));
        if has_world {
            let night = self.night;
            {
                if !narrow {
                    // `.space.greedy`: 0.5 em that grows only into room
                    // left over; it never takes room from the fields.
                    bar = bar.child(div().w(px(8.)).flex_grow_1().flex_shrink_0());
                }
                modes = modes
                    .child(bar_button(
                        "live-night",
                        day_night_picture(),
                        live::DAY_NIGHT,
                        Some(night),
                        false,
                        window,
                        cx,
                        |p, _, cx| {
                            let night = !p.night;
                            p.set_night(night, cx)
                        },
                    ))
                    .child(div().flex_none().w(px(8.)));
                let views = [
                    (
                        "live-perspective",
                        BarIcon::Perspective,
                        live::PERSPECTIVE,
                        CameraMode::Perspective,
                    ),
                    ("live-flat", BarIcon::Flat, live::FLAT, CameraMode::Flat),
                    (
                        "live-player",
                        BarIcon::Player,
                        live::PLAYER,
                        CameraMode::Player,
                    ),
                    (
                        "live-spectator",
                        BarIcon::Spectator,
                        live::SPECTATOR,
                        CameraMode::Spectator,
                    ),
                ];
                for (i, (id, icon, name, mode)) in views.into_iter().enumerate() {
                    modes = modes.child(bar_button(
                        id,
                        bluemap::mask(icon.name(), colour::live_bar_dim(), 28.8, 0.)
                            .into_any_element(),
                        name,
                        Some(self.mode == mode),
                        i > 0,
                        window,
                        cx,
                        move |p, w, cx| p.set_mode(mode, w, cx),
                    ));
                }
                modes = modes.child(div().flex_none().w(px(8.))).child(bar_button(
                    "live-reset",
                    bluemap::mask(BarIcon::Reset.name(), colour::live_bar_dim(), 28.8, 0.)
                        .into_any_element(),
                    live::RESET,
                    None,
                    false,
                    window,
                    cx,
                    |p, w, cx| {
                        if p.player.is_some() {
                            p.set_mode(CameraMode::Perspective, w, cx);
                        }
                        if let Some(pane) = p.pane() {
                            pane.camera(CameraInput::Reset);
                        }
                    },
                ));
                if !narrow {
                    bar = bar.child(modes);
                    modes = div();
                }
            }
            // The position fields (`PositionInput.vue`, `.pos-input`: the
            // bar's whole width, at most 20 em beside the chat, shrinking
            // first when the buttons need the room) and the compass.
            let [x_field, z_field] = self.position_fields(window, cx);
            bar = bar
                .child(
                    div()
                        .flex()
                        .w_full()
                        .flex_shrink_1()
                        .min_w(px(0.))
                        .when(!narrow, |d| {
                            d.max_w(px(320.))
                                .border_l_1()
                                .border_color(colour::live_bar_light())
                        })
                        .child(position_field("x", &x_field, false, cx))
                        .child(position_field("z", &z_field, true, cx)),
                )
                .child({
                    let turn = -(self.heading_degrees().to_radians()) as f32;
                    bar_button(
                        "live-north",
                        compass_picture(turn),
                        live::COMPASS,
                        None,
                        true,
                        window,
                        cx,
                        |p, _, _| {
                            if let Some(pane) = p.pane() {
                                pane.camera(CameraInput::FaceNorth);
                            }
                        },
                    )
                });
            if self.selectable_chat.is_some() {
                let selecting = self.selecting;
                bar = bar.child(bar_button(
                    "live-select",
                    bluemap::mask(BarIcon::Select.name(), colour::live_bar_dim(), 28.8, 0.)
                        .into_any_element(),
                    live::SELECT,
                    Some(selecting),
                    true,
                    window,
                    cx,
                    |p, _, cx| p.toggle_select(cx),
                ));
                let has_blocks = state.as_ref().is_some_and(|s| s.blocks > 0);
                if has_blocks {
                    bar = bar.child(bar_button(
                        "live-download",
                        bluemap::mask(BarIcon::Download.name(), colour::live_bar_dim(), 28.8, 0.)
                            .into_any_element(),
                        live::DOWNLOAD,
                        None,
                        true,
                        window,
                        cx,
                        |p, _, cx| p.export(false, cx),
                    ));
                }
            }
            if playable {
                bar = bar.child(bar_button(
                    "live-export",
                    bluemap::mask(BarIcon::Export.name(), colour::live_bar_dim(), 28.8, 0.)
                        .into_any_element(),
                    live::EXPORT,
                    None,
                    true,
                    window,
                    cx,
                    |p, _, cx| p.export(true, cx),
                ));
            }
        } else {
            bar = bar.child(div().flex_1());
        }
        let full = self.full;
        bar = bar.child(bar_button(
            "live-fill",
            bluemap::mask(
                if full {
                    BarIcon::Collapse
                } else {
                    BarIcon::Expand
                }
                .name(),
                colour::live_bar_dim(),
                28.8,
                0.,
            )
            .into_any_element(),
            if full { live::BACK } else { live::FILL },
            None,
            has_world,
            window,
            cx,
            |p, _, cx| {
                let full = !p.full;
                p.set_full(full, cx)
            },
        ));
        let floor = self.facts.floor.clone().filter(|_| has_world);

        // Zoom buttons (`ZoomButtons.vue`: bottom right, 8 px in, a column
        // 2 em wide), not in free flight.
        let zoom = (has_world && matches!(self.mode, CameraMode::Perspective | CameraMode::Flat))
            .then(|| {
                div()
                    .id("live-zoom")
                    .occlude()
                    .absolute()
                    .right(px(8.))
                    .bottom(px(8.))
                    .flex()
                    .flex_col()
                    .child(bar_button(
                        "live-zoom-in",
                        bluemap::mask(BarIcon::ZoomIn.name(), colour::live_bar_dim(), 28.8, 0.)
                            .into_any_element(),
                        live::ZOOM_IN,
                        None,
                        false,
                        window,
                        cx,
                        |p, _, _| {
                            if let Some(pane) = p.pane() {
                                pane.camera(CameraInput::ZoomStep(-3));
                            }
                        },
                    ))
                    .child(bar_button(
                        "live-zoom-out",
                        bluemap::mask(BarIcon::ZoomOut.name(), colour::live_bar_dim(), 28.8, 0.)
                            .into_any_element(),
                        live::ZOOM_OUT,
                        None,
                        false,
                        window,
                        cx,
                        |p, _, _| {
                            if let Some(pane) = p.pane() {
                                pane.camera(CameraInput::ZoomStep(3));
                            }
                        },
                    ))
            });

        let bar = if narrow && has_world {
            bar.child(
                modes
                    .id("live-bar-modes")
                    .occlude()
                    .absolute()
                    .top(px(32.))
                    .left_0()
                    .right_0()
                    .bg(colour::live_bar()),
            )
        } else {
            bar
        };

        // Labels at the projected anchors of the frame they belong to.
        let store = self.store.borrow();
        let origin = store.bounds.map(|(b, _)| b.origin);
        let mut labels = Vec::new();
        if let (Some(origin), Some(state)) = (origin, &state)
            && self
                .view
                .as_ref()
                .is_some_and(|v| v.0.starts_with("bench:"))
        {
            for i in 0..state.layout.len() {
                if let (Some(p), Some(name)) = (store.anchor(i), self.name_of(i)) {
                    labels.push(
                        theme::text(div(), TextKind::LiveTag, &fonts)
                            .absolute()
                            .left(p.x - origin.x)
                            .top(p.y - origin.y - px(24.))
                            .px(px(7.))
                            .py(px(3.))
                            .rounded(px(7.))
                            .bg(colour::live_tag())
                            .child(name.clone())
                            .into_any_element(),
                    );
                }
            }
        }
        // A draft selection's marker (`.mcbench-icon`): its block's icon on
        // a white ring, or a dark square with a white inset ring.
        let (_, icon_marks, popup_anchor) = self.overlays(cx);
        let mut markers = Vec::new();
        if let Some(origin) = origin {
            for (n, (index, block)) in icon_marks.into_iter().enumerate() {
                let Some(p) = store.anchor(index) else {
                    continue;
                };
                let at = |d: f32, size: f32| {
                    div()
                        .absolute()
                        .left(p.x - origin.x - px(d))
                        .top(p.y - origin.y - px(d))
                        .size(px(size))
                };
                markers.push(match super::block_icons::icon(block.as_deref(), cx) {
                    // The 2 px ring is the box-shadow's spread, outside the 20 px.
                    Some(icon) => at(12., 24.)
                        .debug_selector(|| format!("live-icon-{n}-icon"))
                        .rounded(px(6.))
                        .bg(colour::live_mark_ring())
                        .p(px(2.))
                        .shadow(vec![BoxShadow {
                            color: hsla(0., 0., 0., 0.55),
                            offset: point(px(0.), px(1.)),
                            blur_radius: px(6.),
                            spread_radius: px(1.),
                            inset: false,
                        }])
                        .child(
                            div()
                                .size(px(20.))
                                .rounded(px(4.))
                                .overflow_hidden()
                                .child(img(icon.marker).size_full().rounded(px(4.))),
                        )
                        .into_any_element(),
                    None => at(10., 20.)
                        .debug_selector(|| format!("live-icon-{n}-empty"))
                        .rounded(px(4.))
                        .bg(colour::live_mark_empty())
                        .border(px(1.5))
                        .border_color(colour::live_mark_ring())
                        .into_any_element(),
                });
            }
        }
        let popup_at = popup_anchor
            .zip(origin)
            .and_then(|(i, origin)| store.anchor(i).map(|p| p - origin));
        drop(store);

        // The marked block's label (BlueMap's popup), with copy, centred on
        // the centre of the block's top face (a CSS2DObject's element is
        // centred on its point), fading in over 300 ms and, once closed,
        // out over 300 ms.
        let shown = self
            .marked
            .map(|b| (b, true, self.marked_opened))
            .or(self.closing.map(|(b, at)| (b, false, Some(at))));
        let marked = shown
            .filter(|_| self.player.is_none() && has_world)
            .zip(popup_at)
            .map(|((b, opening, _), at)| {
                let text = format!("{} {} {}", b[0], b[1], b[2]);
                let label = div()
                    .id("live-marked-box")
                    .occlude()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .bg(colour::live_bar())
                    .child(
                        theme::text(div(), TextKind::LiveBar, &fonts)
                            .id("live-marked")
                            .role(Role::Label)
                            .aria_label(format!("Block: x {} y {} z {}", b[0], b[1], b[2]))
                            .aria_value(format!("Block: x {} y {} z {}", b[0], b[1], b[2]))
                            .child(format!("Block: x: {} y: {} z: {}", b[0], b[1], b[2])),
                    )
                    .child(super::controls::CopyButton::new(
                        "live-marked-copy",
                        text,
                        "Copy to Clipboard",
                    ));
                // A box twice the label's widest around the point, the label
                // centred in it: the point is the label's centre.
                div()
                    .absolute()
                    .left(at.x - px(POPUP_HALF_W))
                    .top(at.y - px(POPUP_HALF_H))
                    .w(px(2. * POPUP_HALF_W))
                    .h(px(2. * POPUP_HALF_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(label.with_animation(
                        SharedString::from(format!(
                            "live-marked-fade-{opening}-{}-{}-{}",
                            b[0], b[1], b[2]
                        )),
                        Animation::new(std::time::Duration::from_millis(live::POPUP_FADE_MS)),
                        move |d, t| d.opacity(if opening { t } else { 1. - t }),
                    ))
                    .into_any_element()
            });

        // Player mode's HUD.
        let locked = self.locked;
        let hud = self.player.as_ref().map(|_| {
            let hint = if locked {
                live::HINT_TAKEN
            } else {
                live::HINT_FREE
            };
            div()
                .absolute()
                .inset_0()
                .when(locked, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(relative(0.5))
                            .left(relative(0.5))
                            .ml(px(-8.))
                            .mt(px(-8.))
                            .size(px(16.))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(7.))
                                    .top_0()
                                    .w(px(2.))
                                    .h(px(16.))
                                    .bg(colour::live_mark()),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(7.))
                                    .left_0()
                                    .h(px(2.))
                                    .w(px(16.))
                                    .bg(colour::live_mark()),
                            ),
                    )
                })
                .child(
                    div()
                        .absolute()
                        .bottom(px(64.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            theme::text(div(), TextKind::LiveHud, &fonts)
                                .id("live-hint")
                                .role(Role::Status)
                                .aria_label(hint)
                                .px(px(10.))
                                .py(px(5.))
                                .rounded(px(7.))
                                .bg(colour::live_hud())
                                .child(hint),
                        ),
                )
        });
        let note = self.note.as_ref().map(|(text, _)| {
            div()
                .absolute()
                .top(px(58.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    theme::text(div(), TextKind::LiveNote, &fonts)
                        .id("live-note")
                        .role(Role::Status)
                        .aria_label(text.clone())
                        .px(px(10.))
                        .py(px(5.))
                        .rounded(px(7.))
                        .bg(colour::live_hud())
                        .child(text.clone()),
                )
        });
        let centre_mark =
            (self.keyboard && self.player.is_none() && has_world && self.focus.is_focused(window))
                .then(|| {
                    div()
                        .absolute()
                        .top(relative(0.5))
                        .left(relative(0.5))
                        .ml(px(-5.))
                        .mt(px(-5.))
                        .size(px(10.))
                        .border_2()
                        .border_color(colour::live_mark())
                        .rounded(px(5.))
                });

        let selecting = self.selecting;
        // The page's ring is an outline (`:focus-visible`, 2 px gold),
        // which takes no room: here it is drawn over the frame's edge, and
        // the frame fills the box (a border would inset it and show the
        // black under it).
        let ring = self.focus.is_focused(window) && window.last_input_was_keyboard();
        let world = div()
            .id("live-pane")
            .role(Role::Canvas)
            .aria_label(live::PANE_NAME)
            .aria_description(described)
            .track_focus(&self.focus)
            .key_context("LivePane")
            .absolute()
            .inset_0()
            .when(selecting, |d| d.cursor(CursorStyle::Crosshair))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|p, e: &MouseDownEvent, w, cx| p.mouse_down(e, w, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|p, e: &MouseDownEvent, w, cx| p.mouse_down(e, w, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|p, e: &MouseUpEvent, w, cx| p.mouse_up(e, w, cx)),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|p, e: &MouseUpEvent, w, cx| p.mouse_up(e, w, cx)),
            )
            .on_mouse_move(cx.listener(|p, e: &MouseMoveEvent, _, _| p.mouse_move(e.position)))
            .on_scroll(cx.listener(|p, e: &ScrollEvent, _, cx| {
                p.close_popup(cx);
                p.wheel(e)
            }))
            .on_pinch(cx.listener(|p, e: &PinchEvent, _, _| p.pinch(e)))
            .on_action(cx.listener(|p, _: &super::CloseLayer, w, cx| p.escape(w, cx)))
            .on_key_down(cx.listener(|p, e: &KeyDownEvent, w, cx| p.key_down(e, w, cx)))
            .on_key_up(cx.listener(|p, e: &KeyUpEvent, w, cx| p.key_up(e, w, cx)))
            .on_modifiers_changed(
                cx.listener(|p, e: &ModifiersChangedEvent, w, cx| p.modifiers(e, w, cx)),
            )
            .child(self.frames.clone())
            .when(ring, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(colour::gold()),
                )
            });

        let tooltip = if full { live::EXIT_FULL } else { live::EXPAND };
        let content = div()
            .id("viewer")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(colour::viewer())
            .aria_description(tooltip)
            .child(world)
            .children(labels)
            .children(markers)
            .children(centre_mark)
            .children(hud)
            .children(note)
            .children(marked)
            .child(bar)
            .children(floor.map(|f| {
                theme::text(div(), TextKind::LiveHud, &fonts)
                    .id("live-floor")
                    .role(Role::Label)
                    .aria_label(f.clone())
                    .aria_value(f.clone())
                    .absolute()
                    .top(px(44.))
                    .right(px(8.))
                    .max_w(px(360.))
                    .child(f)
            }))
            .children(zoom)
            // The page's order (web/app.js:3359-3368): the cover, the
            // banner over it (a first placement's failure shows on the
            // cover), the toolbar over both.
            .children(cover)
            .children(banner)
            .children(toolbar);
        if full {
            // Over everything, from the window's top left (`.viewer.full`:
            // fixed, inset 0, z-index 15).
            let size = window.viewport_size();
            div()
                .size_full()
                .child(
                    deferred(
                        anchored()
                            .position(point(px(0.), px(0.)))
                            .child(div().w(size.width).h(size.height).child(content)),
                    )
                    .with_priority(1),
                )
                .into_any_element()
        } else {
            content.into_any_element()
        }
    }
}

// -- the bench comparison's interface ----------------------------------------------

/// Where the window's 3D pane is, for the places that drive it without
/// holding the shell (the bench comparison): installed by the shell.
pub struct WorldHook {
    window: AnyWindowHandle,
    shell: WeakEntity<Shell>,
}

impl Global for WorldHook {}

/// Installs the hook (the shell, when it is made).
pub fn install_hook(window: AnyWindowHandle, shell: WeakEntity<Shell>, cx: &mut App) {
    cx.set_global(WorldHook { window, shell });
}

/// Runs `f` on the shell after the current update; with no shell (a
/// gallery's comparison) there is no 3D pane and nothing runs.
fn with_shell(
    cx: &mut App,
    f: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
) {
    let Some(hook) = cx.try_global::<WorldHook>() else {
        return;
    };
    let (window, shell) = (hook.window, hook.shell.clone());
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| {
            let _ = shell.update(cx, |shell, cx| f(shell, window, cx));
        });
    });
}

/// The comparison showed another round: the pane shows that round's world
/// (`bench:<bench>:<round>`, the round switch).
pub fn round_shown(cx: &mut App) {
    with_shell(cx, |shell, window, cx| shell.sync_world(window, cx));
}

/// Frames the whole round on screen over 700 ms (a click on the round's
/// prompt on screen).
pub fn frame_round(cx: &mut App) {
    with_shell(cx, |shell, _, cx| {
        shell.world().update(cx, |pane, _| pane.focus_round())
    });
}

/// A selection chip is hovered (its box in build coordinates), or none:
/// a draft's box is lit (fill 0.22); a sent message's box is drawn alone,
/// lit (`viewerSelections`, `web/app.js` 5383-5397).
pub fn chip_hovered(hovered: Option<([i64; 3], [i64; 3])>, cx: &mut App) {
    with_shell(cx, move |shell, _, cx| {
        shell.world().update(cx, |pane, cx| {
            pane.hovered = hovered;
            pane.log("chip hovered", serde_json::json!({ "box": hovered }));
            pane.sync_overlays(cx);
        })
    });
}

/// Frames one member's build over 700 ms (a row's focus action, the keys
/// 1 to 9 of the comparison).
pub fn frame_member(member: String, cx: &mut App) {
    with_shell(cx, move |shell, _, cx| {
        shell
            .world()
            .update(cx, |pane, _| pane.focus_member_id(&member))
    });
}

// -- the shell's side -----------------------------------------------------------

/// The world a place shows.
pub struct LiveView {
    /// Its view key; none for a place with no world.
    pub view: Option<ViewKey>,
    /// Its title (the pane's description, the export's name).
    pub title: String,
    /// The Build chat whose draft takes selections.
    pub chat: Option<String>,
    /// A comparison's bench title and round.
    pub bench: Option<(String, usize)>,
}

impl Shell {
    /// The view the place shows: a Build chat's or a bench member's chat,
    /// a comparison's round (`bench:<bench>:<round>`, the round the bench
    /// agent's place shows, by default the bench's last), none elsewhere
    /// (the prompt library, the leaderboard, a new bench).
    pub fn live_view(&self, cx: &App) -> LiveView {
        use crate::state::place::Place;
        let lists = self.lists_entity().read(cx);
        match self.place() {
            Place::Chat { id } | Place::NewChat { id: Some(id) } => {
                let title = lists.chat(id).map(|c| c.title.clone()).unwrap_or_default();
                LiveView {
                    view: Some(ViewKey(id.clone())),
                    title,
                    chat: Some(id.clone()),
                    bench: None,
                }
            }
            Place::Member { member, .. } => {
                let title = lists
                    .chat(member)
                    .map(|c| c.title.clone())
                    .unwrap_or_default();
                LiveView {
                    view: Some(ViewKey(member.clone())),
                    title,
                    chat: None,
                    bench: None,
                }
            }
            Place::Compare { bench } => {
                let listed = lists.benches().iter().find(|b| b.id == *bench);
                let title = listed.map(|b| b.title.clone()).unwrap_or_default();
                let round = self.bench_pane().read(cx).round_of(bench);
                LiveView {
                    view: Some(ViewKey(format!("bench:{bench}:{round}"))),
                    title: title.clone(),
                    chat: None,
                    bench: Some((title, round)),
                }
            }
            _ => LiveView {
                view: None,
                title: String::new(),
                chat: None,
                bench: None,
            },
        }
    }

    /// Tells the pane the place's view (after each arrival and list).
    pub fn sync_world(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let LiveView {
            view,
            title,
            chat,
            bench,
        } = self.live_view(cx);
        let chat = chat.map(|id| (id, self.chat().downgrade()));
        let bench_mode = self.mode() == crate::state::place::Mode::Bench;
        // A comparison's labels: each member's name, in the layout's
        // order as the core gives it (the pane matches them by index).
        let names = match self.place() {
            crate::state::place::Place::Compare { bench: id } => {
                let lists = self.lists_entity();
                let listed = lists
                    .read(cx)
                    .benches()
                    .iter()
                    .find(|b| b.id == *id)
                    .cloned();
                listed.map(|b| self.bench_pane().read(cx).members(&b, cx))
            }
            _ => None,
        };
        let world = self.world();
        world.update(cx, |pane, cx| {
            pane.show(view, title, chat, bench, bench_mode, false, window, cx);
            if let Some(members) = names {
                pane.set_members(members.into_iter().map(|m| (m.id, m.name)).collect(), cx);
            }
        });
    }

    /// The live view's updates, for the pane; a named selection joins the
    /// open chat's draft ([`Shell::add_selection`]).
    pub(crate) fn live_update(
        &mut self,
        update: Update,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let world = self.world();
        match world.update(cx, |pane, cx| pane.apply(update, window, cx)) {
            None => {}
            Some(Back::Notice(text)) => self.notice(text, cx),
            Some(Back::Selection(selection)) => self.add_selection(selection, cx),
        }
    }

    /// `addSelection`: `selection` joins the open chat's draft as the next
    /// free letter A to Z (a 27th is dropped); its box and marker are drawn
    /// over the world.
    pub fn add_selection(&mut self, mut selection: Selection, cx: &mut Context<Self>) {
        let world = self.world();
        let chat = self.chat();
        chat.update(cx, |c, cx| {
            let used: Vec<String> = c
                .draft
                .selections
                .iter()
                .map(|s| s.letter.clone())
                .collect();
            if let Some(letter) = live::next_letter(&used) {
                selection.letter = letter;
                c.draft.selections.push(selection);
                cx.notify();
            }
        });
        world.update(cx, |pane, cx| pane.sync_overlays(cx));
    }
}
