//! The live world's time source: the tick clock and the tick counter's
//! throttle read it. The app's is the runtime's ([`RuntimeClock`]); a test
//! gives one it drives ([`super::LiveParts::clock`]).

use std::time::Duration;

use async_trait::async_trait;

/// A monotonic time source.
#[async_trait]
pub trait Clock: Send + Sync + 'static {
    /// The time since the clock's origin.
    fn now(&self) -> Duration;
    /// Resolves once [`Clock::now`] is `at` or later.
    async fn until(&self, at: Duration);
}

/// The runtime's clock (`tokio::time`), from when it was made.
pub struct RuntimeClock(tokio::time::Instant);

impl Default for RuntimeClock {
    fn default() -> RuntimeClock {
        RuntimeClock(tokio::time::Instant::now())
    }
}

#[async_trait]
impl Clock for RuntimeClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    async fn until(&self, at: Duration) {
        tokio::time::sleep_until(self.0 + at).await
    }
}
