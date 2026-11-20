//! `--a11y-dump`: the window's AccessKit tree as JSON
//! (`Window::debug_a11y_tree_json`), written whenever it changes. The tree
//! exists while an assistive client is connected (on Linux, the AT-SPI
//! registry with a client listening), so the dump follows the frames that
//! client sees. The window is read every 100 ms on the interface thread;
//! the comparison and the writing run on a background thread, and the file
//! is rewritten only when the tree differs from the last one written (the
//! frame counter and time stamp aside).

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{AnyWindowHandle, App};

/// Starts writing `window`'s tree to `path`.
pub fn start(window: AnyWindowHandle, path: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut last = String::new();
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            // Only the tree's JSON is made on the interface thread; parsing,
            // comparing, printing and writing it run on a background thread
            // (they took the interface thread for every change while a
            // reader scrolled; open item e).
            let started = std::time::Instant::now();
            let Ok(json) = window.update(cx, |_, window, _| window.debug_a11y_tree_json()) else {
                return;
            };
            let on_thread = started.elapsed();
            if on_thread > Duration::from_millis(16) {
                eprintln!(
                    "a11y dump: {} ms on the interface thread",
                    on_thread.as_millis()
                );
            }
            let Some(json) = json else { continue };
            let path = path.clone();
            let previous = std::mem::take(&mut last);
            last = cx
                .background_executor()
                .spawn(async move { write_if_changed(&json, previous, &path) })
                .await;
        }
    })
    .detach();
}

/// Writes the tree when it differs from `last` (its frame counter and time
/// aside); returns what to compare the next one with.
fn write_if_changed(json: &str, last: String, path: &std::path::Path) -> String {
    let mut tree: serde_json::Value = serde_json::from_str(json).expect("the tree dump is JSON");
    let stable = {
        let mut copy = tree.clone();
        strip_volatile(&mut copy);
        copy.to_string()
    };
    if stable == last {
        return last;
    }
    strip_volatile_time(&mut tree);
    let text = serde_json::to_string_pretty(&tree).expect("JSON prints");
    std::fs::write(path, text)
        .unwrap_or_else(|e| panic!("cannot write the tree to {}: {e}", path.display()));
    stable
}

fn strip_volatile(tree: &mut serde_json::Value) {
    strip_volatile_time(tree);
    if let Some(frame) = tree["frame"].as_object_mut() {
        frame.remove("frame_number");
    }
}

fn strip_volatile_time(tree: &mut serde_json::Value) {
    if let Some(frame) = tree["frame"].as_object_mut() {
        frame.remove("rendered_at");
    }
}
