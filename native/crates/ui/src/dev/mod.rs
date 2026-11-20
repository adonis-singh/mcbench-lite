//! What the dev binary (`mcbench-ui-dev`) and the in-process tests run the
//! screen over: a stand-in core and the fixtures it serves. Never the
//! app's core.

pub mod fixtures;
pub mod gallery;
// pickers: the gallery's pickers.
pub mod gallery_pickers;
pub mod standin;
// settings agent: the gallery's Settings section.
pub mod gallery_settings;
// chat (stage four's chat agent): the stand-in's turns.
pub mod turns;
// pane agent: the fake 3D pane.
pub mod pane;
// pane agent: the gallery's 3D pane.
pub mod gallery_pane;

// par-shell: the dev binary's `--crash draw` and `--crash screen`.
/// Set by the dev binary: the next draw of the window (the shell's, or the
/// error screen's) panics, on the window's own thread.
pub static PANIC_WHILE_DRAWING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
