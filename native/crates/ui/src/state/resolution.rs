//! A chat's build resolution as the lists show it (`web/resolution.mjs`,
//! `RESOLUTIONS`; inventory `components.md` 1.4 and D.2): each stop's
//! label and the short tag a list row shows above Standard; and (pickers
//! agent) the picker's stops with their help, ported from
//! `test/resolution.test.js`'s two page tests.

use crate::Resolution;

/// Every stop, in order (`RESOLUTIONS`).
pub const STOPS: [Resolution; 4] = [
    Resolution::Standard,
    Resolution::High,
    Resolution::VeryHigh,
    Resolution::Maximum,
];

/// A stop's label (`label`).
pub fn label(stop: Resolution) -> &'static str {
    match stop {
        Resolution::Standard => "Standard",
        Resolution::High => "High (2\u{d7})",
        Resolution::VeryHigh => "Very high (5\u{d7})",
        Resolution::Maximum => "Maximum (10\u{d7})",
    }
}

/// A stop's short tag (`short`).
pub fn short(stop: Resolution) -> &'static str {
    match stop {
        Resolution::Standard => "1\u{d7}",
        Resolution::High => "2\u{d7}",
        Resolution::VeryHigh => "5\u{d7}",
        Resolution::Maximum => "10\u{d7}",
    }
}

/// The stop of a listed row's stored number (`resolutionStop`): any other
/// number is an error naming it.
pub fn stop(value: i64) -> Resolution {
    match u32::try_from(value) {
        Ok(v @ (1 | 2 | 5 | 10)) => Resolution::of(v),
        _ => panic!("{value} is not a resolution (1, 2, 5, 10)"),
    }
}

/// `ResolutionTag` (D.2): nothing at Standard; else the short tag and its
/// title `Resolution: <label>`.
pub fn tag(value: i64) -> Option<(&'static str, String)> {
    let stop = stop(value);
    (stop != Resolution::Standard).then(|| (short(stop), format!("Resolution: {}", label(stop))))
}

// pickers: the resolution picker's stops.
impl Resolution {
    /// The stops in order (`RESOLUTIONS`).
    pub const ALL: [Resolution; 4] = STOPS;

    /// `label` ([`label`]).
    pub fn label(self) -> &'static str {
        label(self)
    }

    /// `short` ([`short`]).
    pub fn short(self) -> &'static str {
        short(self)
    }

    /// `feature`: about how many blocks a main feature gets (none at
    /// Standard).
    pub fn feature(self) -> Option<u32> {
        match self {
            Resolution::Standard => None,
            Resolution::High => Some(20),
            Resolution::VeryHigh => Some(50),
            Resolution::Maximum => Some(100),
        }
    }

    /// `across`: about how many blocks across the whole build (none at
    /// Standard).
    pub fn across(self) -> Option<u32> {
        match self {
            Resolution::Standard => None,
            Resolution::High => Some(100),
            Resolution::VeryHigh => Some(250),
            Resolution::Maximum => Some(500),
        }
    }

    /// What the stop does, under the picker's slider (`resolutionHelp`).
    pub fn help(self) -> String {
        match (self.feature(), self.across()) {
            (Some(feature), Some(across)) => format!(
                "About {feature} blocks for a main feature such as an eye or a window. About {across} blocks across the whole build."
            ),
            _ => "The model chooses the size.".to_string(),
        }
    }

    /// The stop's place among the four (the slider's value, 0 to 3).
    pub fn index(self) -> usize {
        Resolution::ALL
            .iter()
            .position(|r| *r == self)
            .expect("every stop is one of the four")
    }

    /// The tick's text under the slider: the full label for Standard, the
    /// short one for the others.
    pub fn tick(self) -> &'static str {
        match self {
            Resolution::Standard => self.label(),
            _ => self.short(),
        }
    }
}

/// The trigger's and the tag's name (`Resolution: <label>`), and the
/// read-only trigger's (`... (set when the bench was made)`).
pub fn trigger_name(stop: Resolution, read_only: bool) -> String {
    if read_only {
        format!("Resolution: {} (set when the bench was made)", stop.label())
    } else {
        format!("Resolution: {}", stop.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stops_are_standard_high_very_high_and_maximum_and_any_other_value_throws() {
        let labels: Vec<_> = STOPS.iter().map(|s| label(*s)).collect();
        assert_eq!(
            labels,
            [
                "Standard",
                "High (2\u{d7})",
                "Very high (5\u{d7})",
                "Maximum (10\u{d7})"
            ]
        );
        assert_eq!(stop(5), Resolution::VeryHigh);
        let refused = std::panic::catch_unwind(|| stop(3)).expect_err("3 is no stop");
        assert_eq!(
            refused.downcast_ref::<String>().map(String::as_str),
            Some("3 is not a resolution (1, 2, 5, 10)")
        );
    }

    #[test]
    fn a_list_row_shows_the_short_tag_only_above_standard() {
        assert_eq!(tag(1), None);
        assert_eq!(
            tag(10),
            Some(("10\u{d7}", "Resolution: Maximum (10\u{d7})".to_string()))
        );
    }

    #[test]
    fn each_stop_has_its_numbers_and_of_refuses_any_other_value() {
        let stops: Vec<(u32, &str, Option<u32>, Option<u32>)> = Resolution::ALL
            .iter()
            .map(|s| (s.value(), s.label(), s.feature(), s.across()))
            .collect();
        assert_eq!(
            stops,
            [
                (1, "Standard", None, None),
                (2, "High (2\u{d7})", Some(20), Some(100)),
                (5, "Very high (5\u{d7})", Some(50), Some(250)),
                (10, "Maximum (10\u{d7})", Some(100), Some(500)),
            ]
        );
        assert_eq!(Resolution::of(5).label(), "Very high (5\u{d7})");
        for value in [0, 3] {
            let thrown = std::panic::catch_unwind(|| Resolution::of(value));
            let message = thrown.expect_err("not a stop");
            let text = message
                .downcast_ref::<String>()
                .expect("a formatted message");
            assert!(text.contains("not a resolution"), "{text}");
        }
    }

    #[test]
    fn each_stops_help_in_the_picker_says_only_what_that_stop_does() {
        let help: Vec<String> = Resolution::ALL.iter().map(|s| s.help()).collect();
        assert_eq!(
            help,
            [
                "The model chooses the size.",
                "About 20 blocks for a main feature such as an eye or a window. About 100 blocks across the whole build.",
                "About 50 blocks for a main feature such as an eye or a window. About 250 blocks across the whole build.",
                "About 100 blocks for a main feature such as an eye or a window. About 500 blocks across the whole build.",
            ]
        );
    }

    #[test]
    fn the_trigger_tag_and_ticks_say_what_the_page_says() {
        assert_eq!(
            trigger_name(Resolution::High, false),
            "Resolution: High (2\u{d7})"
        );
        assert_eq!(
            trigger_name(Resolution::Standard, true),
            "Resolution: Standard (set when the bench was made)"
        );
        let ticks: Vec<&str> = Resolution::ALL.iter().map(|s| s.tick()).collect();
        assert_eq!(ticks, ["Standard", "2\u{d7}", "5\u{d7}", "10\u{d7}"]);
    }
}
