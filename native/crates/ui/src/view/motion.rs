//! Animations that exist only for looks (chat-fix, W6): the running dot's
//! pulse, the working line's stepping dots and a dot's arrival easing. They
//! move only while the window is active and its adapter is a hardware one.
//! While the window is inactive they rest (the dot on its pulse's mid
//! colour, the working dots at their mid opacity) and the working line's
//! clock stops; on a software-emulated adapter (GPUI's
//! `GpuSpecs::is_software_emulated`: WARP on Windows, llvmpipe or lavapipe
//! on Linux) they never move, since each step redraws the whole window on
//! the CPU (one running chat cost about two cores on WARP, the Windows
//! pass). The clock still ticks there: the elapsed time is information.
//! Transitions that a user's action starts and that end within 220 ms (a
//! popover's or tooltip's entry and exit, the palette's opening, the mode
//! switcher's word) are kept.

use gpui_kit::*;

/// The kind of adapter the window draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    /// A GPU.
    Hardware,
    /// The CPU emulating one.
    Software,
}

/// The adapter a test (or a check) says the window has.
struct AdapterOverride(Adapter);

impl Global for AdapterOverride {}

/// Makes every window report `adapter` (tests; the test platform reports
/// none, which is a hardware adapter).
pub fn set_adapter(cx: &mut App, adapter: Adapter) {
    cx.set_global(AdapterOverride(adapter));
}

/// The adapter the window draws with: as set by [`set_adapter`], else as
/// GPUI reports it (no report: a hardware adapter, as on macOS).
pub fn adapter(window: &Window, cx: &App) -> Adapter {
    if let Some(set) = cx.try_global::<AdapterOverride>() {
        return set.0;
    }
    match window.gpu_specs() {
        Some(specs) if specs.is_software_emulated => Adapter::Software,
        _ => Adapter::Hardware,
    }
}

/// Whether an animation for looks moves now: the window active, on a
/// hardware adapter.
pub fn moving(window: &Window, cx: &App) -> bool {
    window.is_window_active() && adapter(window, cx) == Adapter::Hardware
}
