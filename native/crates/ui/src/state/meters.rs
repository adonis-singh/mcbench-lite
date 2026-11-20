//! The composer's context meter and limit chip (`ContextMeter`,
//! `LimitChip`, `LIMIT_WINDOWS`, `formatTokens`, `web/app.js:177-196` and
//! 289-295; inventory `components.md` A.4, C.3 and C.4; `app.md` 5.3 and
//! 6.6), from the transcript's latest usage: the last assistant message
//! with telemetry whose request did not fail (`usageMeta`), its telemetry's
//! provider, model and context tokens and its `rateLimit`. Never shown
//! inline in the transcript.

use mcbench_store::{Message, StoredMessage};
use serde_json::Value;

/// `formatTokens`: `1M` and `1.2M` from a million, `12k` from a thousand,
/// else the number.
pub fn format_tokens(count: f64) -> String {
    if count >= 1e6 {
        if count % 1e6 == 0. {
            format!("{}M", (count / 1e6) as u64)
        } else {
            format!("{:.1}M", count / 1e6)
        }
    } else if count >= 1e3 {
        format!("{}k", (count / 1e3).round() as u64)
    } else {
        format!("{count}")
    }
}

/// The meter: its share of the context, whether it is high (above 85 %),
/// and its tooltip.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    /// `min(used / window, 1)`.
    pub share: f32,
    /// Above 85 % (`high`).
    pub high: bool,
    /// `<pct>% of context · <used> of <window> tokens`.
    pub tip: String,
    // chat-fix-2: the meter's name says the tip's words once with its
    // value: the percentage is the value, not the name.
    /// The meter's accessible name: `Context · <used> of <window> tokens`.
    pub name: String,
}

/// The meter of `used` tokens in a `window` (a window of 0 is full).
pub fn context(used: f64, window: f64) -> Context {
    let share = if window == 0. {
        1.
    } else {
        (used / window).min(1.)
    };
    Context {
        share: share as f32,
        high: share > 0.85,
        tip: format!(
            "{}% of context \u{b7} {} of {} tokens",
            (share * 100.).round() as i64,
            format_tokens(used),
            format_tokens(window)
        ),
        name: format!(
            "Context \u{b7} {} of {} tokens",
            format_tokens(used),
            format_tokens(window)
        ),
    }
}

/// The rate limit of the last request (Claude Code's `rate_limit_info`).
#[derive(Clone, Debug, PartialEq)]
pub struct RateLimit {
    /// `status`: `allowed`, `allowed_warning`, `rejected`.
    pub status: String,
    /// `rateLimitType`.
    pub kind: Option<String>,
    /// `resetsAt`, seconds since the epoch.
    pub resets_at: Option<f64>,
}

/// `LIMIT_WINDOWS`, else `usage`.
pub fn limit_window(kind: Option<&str>) -> &'static str {
    match kind {
        Some("five_hour") => "5-hour",
        Some("seven_day") => "weekly",
        Some("seven_day_opus") => "weekly Opus",
        Some("seven_day_sonnet") => "weekly Sonnet",
        _ => "usage",
    }
}

/// The chip: its text, whether the limit was reached, and its tooltip.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    /// `<window> limit reached` or `Near <window> limit`.
    pub text: String,
    /// `rejected`.
    pub rejected: bool,
    /// `Resets at <time>`, none without a reset time.
    pub tip: Option<String>,
}

/// The chip of a rate limit: nothing while allowed. `clock` words the
/// reset time (local time, hour and minutes: [`clock`]).
pub fn chip(limit: Option<&RateLimit>, clock: impl Fn(f64) -> String) -> Option<Chip> {
    let limit = limit.filter(|l| l.status != "allowed")?;
    let window = limit_window(limit.kind.as_deref());
    let rejected = limit.status == "rejected";
    Some(Chip {
        text: if rejected {
            format!("{window} limit reached")
        } else {
            format!("Near {window} limit")
        },
        rejected,
        tip: limit
            .resets_at
            .filter(|at| *at != 0.)
            .map(|at| format!("Resets at {}", clock(at))),
    })
}

/// A time of day as `toLocaleTimeString([], { hour: 'numeric', minute:
/// '2-digit' })` writes it in the page's usual locale (en-US): `3:40 PM`.
/// `offset` is the local time's offset from UTC in seconds at that time.
pub fn clock_at(epoch_seconds: f64, offset: i64) -> String {
    let local = epoch_seconds.floor() as i64 + offset;
    let minutes_of_day = local.rem_euclid(86_400) / 60;
    let (hour, minute) = (minutes_of_day / 60, minutes_of_day % 60);
    let (twelve, half) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    format!("{twelve}:{minute:02} {half}")
}

/// [`clock_at`] in this machine's time zone.
pub fn clock(epoch_seconds: f64) -> String {
    clock_at(epoch_seconds, local_offset(epoch_seconds as i64))
}

#[cfg(unix)]
fn local_offset(at: i64) -> i64 {
    let time: libc::time_t = at as libc::time_t;
    // SAFETY: localtime_r writes the broken-down time into `tm`, which is
    // ours, and reads only `time`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let done = unsafe { libc::localtime_r(&time, &mut tm) };
    assert!(!done.is_null(), "localtime_r cannot convert {at}");
    tm.tm_gmtoff as i64
}

#[cfg(windows)]
fn local_offset(_at: i64) -> i64 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    // SAFETY: GetTimeZoneInformation fills the structure, which is ours.
    let mut info: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut info) };
    // 2 is TIME_ZONE_ID_DAYLIGHT: daylight time is in force now.
    let bias = info.Bias
        + if id == 2 {
            info.DaylightBias
        } else {
            info.StandardBias
        };
    -(bias as i64) * 60
}

/// The transcript's latest usage (`usageMeta`).
#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    /// The telemetry's provider.
    pub provider: String,
    /// Its model.
    pub model: String,
    /// `contextTokens`, when known.
    pub context_tokens: Option<u64>,
    /// `rateLimit`, when the harness sent one.
    pub rate_limit: Option<RateLimit>,
}

/// The latest usage of a chat's stored messages: the last assistant message
/// with telemetry whose request did not fail (a stopped or failed request
/// reports none, so the meter keeps the last real one).
pub fn latest_usage(messages: &[StoredMessage]) -> Option<Usage> {
    messages.iter().rev().find_map(|stored| {
        let Message::Assistant(assistant) = &stored.message else {
            return None;
        };
        let telemetry = assistant.telemetry.as_ref()?;
        if telemetry.failed == Some(true) {
            return None;
        }
        Some(Usage {
            provider: telemetry.provider.clone(),
            model: telemetry.model.clone(),
            context_tokens: telemetry.context_tokens.flatten(),
            rate_limit: assistant.rate_limit.as_ref().map(rate_limit_of),
        })
    })
}

/// A stored `rateLimit` (the CLI's object as it sent it).
pub fn rate_limit_of(value: &Value) -> RateLimit {
    RateLimit {
        status: value["status"]
            .as_str()
            .unwrap_or_else(|| panic!("a rateLimit without a status: {value}"))
            .to_string(),
        kind: value["rateLimitType"].as_str().map(str::to_string),
        resets_at: value["resetsAt"].as_f64(),
    }
}

/// What the composer shows of a usage for the chosen model: the chip and,
/// when the context tokens and the model's window are both known, the
/// meter. Nothing when the usage is another model's.
pub fn shown(
    usage: Option<&Usage>,
    provider: &str,
    model: &str,
    window: Option<f64>,
    clock: impl Fn(f64) -> String,
) -> (Option<Chip>, Option<Context>) {
    let Some(usage) = usage.filter(|u| u.provider == provider && u.model == model) else {
        return (None, None);
    };
    let meter = match (usage.context_tokens, window) {
        (Some(used), Some(window)) if used > 0 && window > 0. => Some(context(used as f64, window)),
        _ => None,
    };
    (chip(usage.rate_limit.as_ref(), clock), meter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_read_as_the_page_writes_them() {
        assert_eq!(format_tokens(950.), "950");
        assert_eq!(format_tokens(84_400.), "84k");
        assert_eq!(format_tokens(200_000.), "200k");
        assert_eq!(format_tokens(1_000_000.), "1M");
        assert_eq!(format_tokens(1_200_000.), "1.2M");
    }

    #[test]
    fn the_meter_fills_to_its_share_and_says_it() {
        let c = context(84_000., 200_000.);
        assert_eq!(c.tip, "42% of context \u{b7} 84k of 200k tokens");
        assert!(!c.high);
        assert!((c.share - 0.42).abs() < 1e-6);
        let full = context(190_000., 200_000.);
        assert!(full.high);
        assert_eq!(context(300_000., 200_000.).share, 1.);
        assert_eq!(context(10., 0.).share, 1.);
    }

    #[test]
    fn the_chip_shows_only_near_or_at_a_limit() {
        let clock = |_: f64| "3:40 PM".to_string();
        let limit = |status: &str, kind: Option<&str>, at: Option<f64>| RateLimit {
            status: status.into(),
            kind: kind.map(str::to_string),
            resets_at: at,
        };
        assert_eq!(chip(None, clock), None);
        assert_eq!(chip(Some(&limit("allowed", None, None)), clock), None);
        let near = chip(
            Some(&limit("allowed_warning", Some("seven_day_opus"), Some(1.))),
            clock,
        )
        .expect("near");
        assert_eq!(near.text, "Near weekly Opus limit");
        assert_eq!(near.tip.as_deref(), Some("Resets at 3:40 PM"));
        assert!(!near.rejected);
        let at = chip(Some(&limit("rejected", Some("five_hour"), None)), clock).expect("at");
        assert_eq!(at.text, "5-hour limit reached");
        assert!(at.rejected && at.tip.is_none());
        let other = chip(
            Some(&limit("allowed_warning", Some("monthly"), None)),
            clock,
        )
        .expect("other");
        assert_eq!(other.text, "Near usage limit");
    }

    #[test]
    fn the_reset_time_is_the_local_hour_and_minutes() {
        // 2026-11-17 15:40:00 UTC.
        let at = 1_794_930_000.;
        assert_eq!(clock_at(at, 0), "3:40 PM");
        assert_eq!(clock_at(at, -15 * 3600 - 40 * 60), "12:00 AM");
        assert_eq!(clock_at(at, 9 * 3600 + 20 * 60), "1:00 AM");
        assert_eq!(clock_at(at, -3 * 3600 - 40 * 60), "12:00 PM");
    }

    #[test]
    fn the_meter_and_chip_are_the_chosen_models_only() {
        let usage = Usage {
            provider: "claude-code".into(),
            model: "claude-opus-5-5".into(),
            context_tokens: Some(84_000),
            rate_limit: Some(RateLimit {
                status: "rejected".into(),
                kind: Some("seven_day".into()),
                resets_at: None,
            }),
        };
        let clock = |_: f64| String::new();
        let (chip, meter) = shown(
            Some(&usage),
            "claude-code",
            "claude-opus-5-5",
            Some(200_000.),
            clock,
        );
        assert_eq!(chip.expect("a chip").text, "weekly limit reached");
        assert_eq!(
            meter.expect("a meter").tip,
            "42% of context \u{b7} 84k of 200k tokens"
        );
        assert_eq!(
            shown(
                Some(&usage),
                "anthropic",
                "claude-opus-5-5",
                Some(200_000.),
                clock
            ),
            (None, None)
        );
        let (_, no_window) = shown(Some(&usage), "claude-code", "claude-opus-5-5", None, clock);
        assert!(no_window.is_none());
    }
}
