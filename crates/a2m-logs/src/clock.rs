//! Where time comes from. Nothing in this crate reads a clock except through
//! this trait, so a test can fix the time and get the same output every run.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use a2m_core::Timestamp;

/// A source of time.
///
/// `elapsed` measures durations and never goes backwards. `now` is the time of
/// day, used only to stamp log lines and spans; nothing that is hashed or
/// compared ever reads it.
pub trait Clock: Send + Sync {
    /// Time since a fixed start of this clock's choosing.
    fn elapsed(&self) -> Duration;

    /// The time of day.
    fn now(&self) -> Timestamp;
}

/// The machine's own clocks.
#[derive(Debug)]
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// A clock whose `elapsed` starts from now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    fn now(&self) -> Timestamp {
        // A machine whose clock is set before 1970 reports the start of 1970.
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        Timestamp::from_unix_millis(i64::try_from(millis).unwrap_or(i64::MAX))
    }
}

/// A clock that moves only when it is told to. For tests and for the benchmark
/// harness, where the same run must give the same numbers.
#[derive(Debug, Default)]
pub struct ManualClock {
    elapsed_micros: AtomicU64,
    unix_millis: AtomicI64,
}

impl ManualClock {
    /// A clock at zero elapsed time, showing `now` as the time of day.
    #[must_use]
    pub fn starting_at(now: Timestamp) -> Self {
        Self {
            elapsed_micros: AtomicU64::new(0),
            unix_millis: AtomicI64::new(now.as_unix_millis()),
        }
    }

    /// Moves both the elapsed time and the time of day forward.
    pub fn advance(&self, by: Duration) {
        let micros = u64::try_from(by.as_micros()).unwrap_or(u64::MAX);
        self.elapsed_micros.fetch_add(micros, Ordering::SeqCst);
        let millis = i64::try_from(by.as_millis()).unwrap_or(i64::MAX);
        self.unix_millis.fetch_add(millis, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn elapsed(&self) -> Duration {
        Duration::from_micros(self.elapsed_micros.load(Ordering::SeqCst))
    }

    fn now(&self) -> Timestamp {
        Timestamp::from_unix_millis(self.unix_millis.load(Ordering::SeqCst))
    }
}

/// A duration in whole microseconds, as traces, spans, and metrics write it.
pub(crate) fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use a2m_core::Timestamp;

    use super::{Clock, ManualClock, SystemClock};

    #[test]
    fn a_manual_clock_moves_only_when_told() {
        let clock = ManualClock::starting_at(Timestamp::from_unix_millis(1_000));
        assert_eq!(clock.elapsed(), Duration::ZERO);
        assert_eq!(clock.now().as_unix_millis(), 1_000);
        clock.advance(Duration::from_millis(250));
        assert_eq!(clock.elapsed(), Duration::from_millis(250));
        assert_eq!(clock.now().as_unix_millis(), 1_250);
    }

    #[test]
    fn the_system_clock_never_goes_backwards() {
        let clock = SystemClock::new();
        let first = clock.elapsed();
        let second = clock.elapsed();
        assert!(second >= first);
        assert!(clock.now().as_unix_millis() > 0);
    }
}
