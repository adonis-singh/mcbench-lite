//! The window's rebindable shortcuts and its layers, wired: the pure rules
//! are [`crate::state::keymap`] and [`crate::state::layers`]; this module
//! holds the keymap in force, which layers are open, and what runs.
//!
//! One line each, for the agents that build on the shell:
//!
//! | To | Call |
//! |---|---|
//! | make a shortcut run something | `keys::on_bind(cx, BindAction::Search, move \|window, cx\| ..)` |
//! | say a layer opened, with how Escape closes it | `keys::open_layer(cx, Layer::Command, move \|window, cx\| ..)` |
//! | say it closed | `keys::close_layer(cx, Layer::Command)` |
//! | ask whether a modal layer covers the app (type-ahead) | `keys::modal_open(cx)` |
//! | cancel an edit on Escape when no layer is open | `keys::on_escape(cx, move \|window, cx\| ..)` |
//! | rebind (Settings) | `keys::set_keymap(cx, keymap)` |
//!
//! A key press that matches a bind is the app's whether or not its action
//! runs (the page prevents its default either way): it does not reach the
//! focused element. Escape is the `CloseLayer` action bound with no
//! context, so a popover's own Escape (its `Popover` context) wins and goes
//! no further, as the page's popovers stop theirs.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::*;

use super::{CloseLayer, Press};
use crate::state::keymap::{self, BindAction, Keymap};
use crate::state::layers::{self, Layer, Open};

/// The shortcuts and layers of the app.
#[derive(Default)]
pub struct Keys {
    keymap: Keymap,
    binds: HashMap<BindAction, Press>,
    open: Open,
    closers: HashMap<Layer, Press>,
    escape: Option<Press>,
    // settings agent: the shortcut recorder, while it records.
    recorder: Option<Recorder>,
}

// settings agent
/// What the shortcut recorder gets while it records: a combination, or
/// none for Escape (cancel).
pub type Recorder = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;

impl Global for Keys {}

/// Whether Mod is Cmd here.
pub const MAC: bool = cfg!(target_os = "macos");

/// Installs the keys: the keymap's defaults, the interceptor that runs
/// binds, and Escape's layer closing. `view::setup` calls it.
pub fn install(cx: &mut App) {
    cx.set_global(Keys::default());
    cx.bind_keys([KeyBinding::new("escape", CloseLayer, Some("Screen"))]);
    cx.intercept_keystrokes(|event, window, cx| {
        let stroke = &event.keystroke;
        // settings agent: while the recorder records, every key is its and
        // goes no further: Escape cancels, a combination is recorded, a bare
        // modifier or a plain key is swallowed and it records on.
        if let Some(recorder) = cx.global::<Keys>().recorder.clone() {
            cx.stop_propagation();
            if stroke.key == "escape" {
                recorder(None, window, cx);
                return;
            }
            let press = keymap::Press {
                platform: stroke.modifiers.platform,
                control: stroke.modifiers.control,
                alt: stroke.modifiers.alt,
                shift: stroke.modifiers.shift,
                key: stroke.key.clone(),
            };
            if let Some(combo) = keymap::combo_of(&press, MAC) {
                recorder(Some(combo), window, cx);
            }
            return;
        }
        let press = keymap::Press {
            platform: stroke.modifiers.platform,
            control: stroke.modifiers.control,
            alt: stroke.modifiers.alt,
            shift: stroke.modifiers.shift,
            key: stroke.key.clone(),
        };
        let Some(combo) = keymap::combo_of(&press, MAC) else {
            return;
        };
        let keys = cx.global::<Keys>();
        let Some(action) = keys.keymap.action(&combo) else {
            return;
        };
        let runs = layers::bind_runs(action, &keys.open);
        let run = keys.binds.get(&action).cloned();
        // The app's key either way: it goes no further.
        cx.stop_propagation();
        if let Some(run) = run.filter(|_| runs) {
            run(window, cx);
        }
    })
    .detach();
}

/// Escape (`CloseLayer`): the closer of the topmost open layer runs; with
/// none open, the edit's cancel (when one is registered). The window's
/// root element handles `CloseLayer` with this.
pub fn close_top(window: &mut Window, cx: &mut App) {
    let keys = cx.global::<Keys>();
    let run = match layers::top_layer(&keys.open) {
        Some(layer) => keys.closers.get(&layer).cloned(),
        None => keys.escape.clone(),
    };
    if let Some(run) = run {
        run(window, cx);
    }
}

// par-chat (stage four's parity pass)
/// Escape pressed in a text field of the window's own (`closeTopLayer`
/// with the field as its target): the closer of the topmost open layer
/// runs; with none open nothing happens, since an edit is cancelled only
/// from outside a field (`web/app.js` 4128-4132).
pub fn close_top_from_field(window: &mut Window, cx: &mut App) {
    let keys = cx.global::<Keys>();
    let run = layers::top_layer(&keys.open).and_then(|layer| keys.closers.get(&layer).cloned());
    if let Some(run) = run {
        run(window, cx);
    }
}
// end of par-chat's block

/// Makes `action`'s shortcut run `run`.
pub fn on_bind(cx: &mut App, action: BindAction, run: impl Fn(&mut Window, &mut App) + 'static) {
    cx.global_mut::<Keys>().binds.insert(action, Rc::new(run));
}

/// Says `layer` is open; `close` is what Escape does to it.
pub fn open_layer(cx: &mut App, layer: Layer, close: impl Fn(&mut Window, &mut App) + 'static) {
    let keys = cx.global_mut::<Keys>();
    keys.open.insert(layer);
    keys.closers.insert(layer, Rc::new(close));
}

/// Says `layer` closed.
pub fn close_layer(cx: &mut App, layer: Layer) {
    let keys = cx.global_mut::<Keys>();
    keys.open.remove(&layer);
    keys.closers.remove(&layer);
}

/// Whether `layer` is open.
pub fn is_open(cx: &App, layer: Layer) -> bool {
    cx.global::<Keys>().open.contains(&layer)
}

/// Whether a modal layer covers the app.
pub fn modal_open(cx: &App) -> bool {
    layers::modal_open(&cx.global::<Keys>().open)
}

/// What Escape does when no layer is open (cancels an edit).
pub fn on_escape(cx: &mut App, run: impl Fn(&mut Window, &mut App) + 'static) {
    cx.global_mut::<Keys>().escape = Some(Rc::new(run));
}

/// The keymap in force.
pub fn keymap(cx: &App) -> &Keymap {
    &cx.global::<Keys>().keymap
}

/// Rebinds (Settings, or the stored binds at start).
pub fn set_keymap(cx: &mut App, keymap: Keymap) {
    cx.global_mut::<Keys>().keymap = keymap;
}

/// An action's shortcut as the platform writes it (`⌘B`, `Ctrl+B`), for a
/// title or a tip.
pub fn shown(cx: &App, action: BindAction) -> String {
    keymap::format_bind(keymap(cx).bind(action), MAC)
}

// settings agent: the shortcut recorder.
/// Starts recording: every key goes to `recorder` until [`stop_recording`].
pub fn record(cx: &mut App, recorder: impl Fn(Option<String>, &mut Window, &mut App) + 'static) {
    cx.global_mut::<Keys>().recorder = Some(Rc::new(recorder));
}

/// Stops recording.
pub fn stop_recording(cx: &mut App) {
    cx.global_mut::<Keys>().recorder = None;
}
