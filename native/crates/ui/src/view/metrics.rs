//! First numbers for the planning session: time to the first frame, each
//! frame's time, and key-to-paint (a key's arrival at the window to the end
//! of the next frame drawn). Summarised into the action log when the app
//! quits (`{"action":"metrics",...}`); nothing is kept without a log.
//!
//! For the Mac's frame-pacing runs and speed rows (agent mac-real): when
//! each key arrived, and each update from the core applied with the frame
//! painted after it, so a gap between frames can be read against what was
//! changing, and an opened chat or the first chat list timed to its frame.

use std::time::{Duration, Instant};

/// The measurements of one run.
pub struct Metrics {
    started: Instant,
    first_frame: Option<Duration>,
    frames: Vec<Instant>,
    pending_key: Option<Instant>,
    key_to_paint: Vec<Duration>,
    render_started: Option<Instant>,
    frame_work: Vec<Duration>,
    /// When each wheel event reached the window.
    scrolls: Vec<Instant>,
    /// Wheel events not yet painted: the first one's arrival.
    pending_scroll: Option<Instant>,
    scroll_to_paint: Vec<Duration>,
    /// When each key reached the window.
    keys: Vec<Instant>,
    /// Updates applied since the last frame: their kind and when.
    pending_updates: Vec<(&'static str, Instant)>,
    /// Every update applied: its kind, when, and the end of the frame
    /// painted after it.
    updates: Vec<(&'static str, Instant, Instant)>,
}

impl Metrics {
    /// Measurements from `started`, the process's start.
    pub fn new(started: Instant) -> Self {
        Metrics {
            started,
            first_frame: None,
            frames: Vec::new(),
            pending_key: None,
            key_to_paint: Vec::new(),
            render_started: None,
            frame_work: Vec::new(),
            scrolls: Vec::new(),
            pending_scroll: None,
            scroll_to_paint: Vec::new(),
            keys: Vec::new(),
            pending_updates: Vec::new(),
            updates: Vec::new(),
        }
    }

    /// The time to the first frame, once it painted (the `proof` hook's wait).
    #[cfg(feature = "proof-hooks")]
    pub fn first_frame(&self) -> Option<Duration> {
        self.first_frame
    }

    /// A key reached the window.
    pub fn key(&mut self) {
        let now = Instant::now();
        self.keys.push(now);
        self.pending_key.get_or_insert(now);
    }

    /// An update from the core was applied (`kind` is its variant's name).
    pub fn update(&mut self, kind: &'static str) {
        self.pending_updates.push((kind, Instant::now()));
    }

    /// A wheel event reached the window.
    pub fn scroll(&mut self) {
        let now = Instant::now();
        self.scrolls.push(now);
        self.pending_scroll.get_or_insert(now);
    }

    /// The screen began rendering a frame.
    pub fn rendering(&mut self) {
        self.render_started = Some(Instant::now());
    }

    /// The screen finished painting a frame; whether it was the first.
    pub fn painted(&mut self) -> bool {
        let now = Instant::now();
        if let Some(started) = self.render_started.take() {
            self.frame_work.push(now - started);
        }
        let first = self.first_frame.is_none();
        self.first_frame.get_or_insert(now - self.started);
        self.frames.push(now);
        if let Some(key) = self.pending_key.take() {
            self.key_to_paint.push(now - key);
        }
        if let Some(scroll) = self.pending_scroll.take() {
            self.scroll_to_paint.push(now - scroll);
        }
        for (kind, at) in self.pending_updates.drain(..) {
            self.updates.push((kind, at, now));
        }
        first
    }

    /// The summary: counts, and milliseconds as mean, p50, p99 and worst.
    pub fn summary(&self) -> serde_json::Value {
        let intervals: Vec<Duration> = self.frames.windows(2).map(|w| w[1] - w[0]).collect();
        serde_json::json!({
            "first_frame_ms": self.first_frame.map(ms),
            "frames": self.frames.len(),
            "frame_interval_ms": stats(&intervals),
            "key_to_paint_ms": stats(&self.key_to_paint),
            "frame_work_ms": stats(&self.frame_work),
            "frame_times_ms": self.frames.iter().map(|f| ms(*f - self.started)).collect::<Vec<_>>(),
            "scroll_to_paint_ms": stats(&self.scroll_to_paint),
            "scroll_times_ms": self.scrolls.iter().map(|f| ms(*f - self.started)).collect::<Vec<_>>(),
            "key_times_ms": self.keys.iter().map(|k| ms(*k - self.started)).collect::<Vec<_>>(),
            "first_list_ms": self
                .updates
                .iter()
                .find(|(kind, _, _)| *kind == "Conversations")
                .map(|(_, _, painted)| ms(*painted - self.started)),
            "updates": self
                .updates
                .iter()
                .map(|(kind, at, painted)| serde_json::json!([kind, ms(*at - self.started), ms(*painted - self.started)]))
                .collect::<Vec<_>>(),
            // Applied but not painted by the quit: a stall that lasted to the end.
            "unpainted_updates": self
                .pending_updates
                .iter()
                .map(|(kind, at)| serde_json::json!([kind, ms(*at - self.started)]))
                .collect::<Vec<_>>(),
            "unpainted_key_ms": self.pending_key.map(|k| ms(k - self.started)),
        })
    }
}

fn ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 1e6).round() / 1e3
}

fn stats(values: &[Duration]) -> serde_json::Value {
    if values.is_empty() {
        return serde_json::Value::Null;
    }
    let mut sorted: Vec<f64> = values.iter().map(|d| ms(*d)).collect();
    sorted.sort_by(f64::total_cmp);
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    serde_json::json!({
        "count": sorted.len(),
        "mean": (sorted.iter().sum::<f64>() / sorted.len() as f64 * 1e3).round() / 1e3,
        "p50": at(0.5),
        "p99": at(0.99),
        "worst": sorted[sorted.len() - 1],
    })
}

/// An update's variant name, for [`Metrics::update`].
pub fn kind(update: &crate::Update) -> &'static str {
    use crate::Update;
    match update {
        Update::Conversations(..) => "Conversations",
        Update::Transcript { .. } => "Transcript",
        Update::TranscriptFailed { .. } => "TranscriptFailed",
        Update::Turn { .. } => "Turn",
        Update::TurnJoined { .. } => "TurnJoined",
        Update::TurnNotRunning { .. } => "TurnNotRunning",
        Update::SendRefused { .. } => "SendRefused",
        Update::Archive(..) => "Archive",
        Update::Found { .. } => "Found",
        Update::Providers(..) => "Providers",
        Update::Models { .. } => "Models",
        Update::ModelInfo { .. } => "ModelInfo",
        Update::Favorites(..) => "Favorites",
        Update::Setup(..) => "Setup",
        Update::Live(..) => "Live",
        Update::Error(..) => "Error",
        Update::Fatal(..) => "Fatal",
        Update::Prompts(..) => "Prompts",
        // settings agent
        Update::Settings(..) => "Settings",
        Update::SettingsSaved(..) => "SettingsSaved",
        Update::SignIn { .. } => "SignIn",
        Update::Install { .. } => "Install",
        Update::Leaderboard(..) => "Leaderboard",
        Update::Shares(..) => "Shares",
        Update::ShareRemoved { .. } => "ShareRemoved",
        // pane agent
        Update::LivePane(..) => "LivePane",
        Update::Clicked { .. } => "Clicked",
        Update::PlayerNote(..) => "PlayerNote",
        Update::SelectionDescribed { .. } => "SelectionDescribed",
        Update::Exported(..) => "Exported",
        // bench agent
        Update::Bench { .. } => "Bench",
        Update::BenchSent { .. } => "BenchSent",
        Update::Forked { .. } => "Forked",
        Update::PromptsSaved(..) => "PromptsSaved",
        Update::Generated { .. } => "Generated",
        // par-chat
        Update::ChatCreated(..) => "ChatCreated",
        // share agent
        Update::ShareRead { .. } => "ShareRead",
        Update::PublishStep { .. } => "PublishStep",
        Update::ShareChanged { .. } => "ShareChanged",
    }
}
