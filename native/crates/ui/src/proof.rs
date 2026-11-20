//! Test hooks for staging builds, compiled only with the `proof-hooks`
//! feature (release scripts never enable it).
//!
//! `MCBENCH_UI_PANIC_AFTER_MS`, read once as [`crate::run`] starts: that many
//! milliseconds after the window's first frame, the window's thread panics
//! with [`MESSAGE`], for the error screen's proof on a real app.

use std::time::Duration;

/// The variable naming the delay, in milliseconds after the first frame.
pub const PANIC_AFTER: &str = "MCBENCH_UI_PANIC_AFTER_MS";

/// The panic's message.
pub const MESSAGE: &str = "a panic on purpose (MCBENCH_UI_PANIC_AFTER_MS)";

/// The delay the variable's value names, or none when it is unset.
pub fn panic_after(value: Option<&str>) -> Option<Duration> {
    value.map(|ms| {
        Duration::from_millis(
            ms.parse()
                .expect("MCBENCH_UI_PANIC_AFTER_MS is a whole number of milliseconds"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delay_is_parsed() {
        assert_eq!(panic_after(Some("3000")), Some(Duration::from_millis(3000)));
        assert_eq!(panic_after(None), None);
    }

    #[test]
    #[should_panic(expected = "whole number of milliseconds")]
    fn a_delay_that_is_not_a_number_stops_the_start() {
        panic_after(Some("soon"));
    }
}
