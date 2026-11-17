//! The numbers a set of timings is reduced to.
//!
//! Everything is whole nanoseconds. A percentile is taken by the nearest-rank
//! method: the smallest sample such that at least that share of the samples is
//! at or below it. It is always one of the samples, never an interpolation, so
//! no float is involved and the same samples always give the same numbers.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// What the measured iterations of one operation came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    /// How many iterations were measured.
    pub count: u64,
    /// The fastest iteration.
    pub min_ns: u64,
    /// The 50th percentile.
    pub median_ns: u64,
    /// The 95th percentile.
    pub p95_ns: u64,
    /// The 99th percentile.
    pub p99_ns: u64,
    /// The slowest iteration.
    pub max_ns: u64,
}

impl Stats {
    /// Reduces timings to their statistics. `None` when there is no timing.
    #[must_use]
    pub fn from_durations(samples: &[Duration]) -> Option<Self> {
        let nanos: Vec<u64> = samples.iter().map(|d| whole_nanos(*d)).collect();
        Self::from_nanos(&nanos)
    }

    /// Reduces timings in nanoseconds to their statistics. `None` when there is no timing.
    #[must_use]
    pub fn from_nanos(samples: &[u64]) -> Option<Self> {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        Some(Self {
            count: u64::try_from(sorted.len()).unwrap_or(u64::MAX),
            min_ns: *sorted.first()?,
            median_ns: percentile(&sorted, 50)?,
            p95_ns: percentile(&sorted, 95)?,
            p99_ns: percentile(&sorted, 99)?,
            max_ns: *sorted.last()?,
        })
    }
}

/// The nearest-rank percentile of samples sorted from smallest to largest.
///
/// `None` when there is no sample or the percentile is not between 1 and 100.
#[must_use]
pub fn percentile(sorted: &[u64], percent: usize) -> Option<u64> {
    if sorted.is_empty() || !(1..=100).contains(&percent) {
        return None;
    }
    // The rank is the count times the share, rounded up; ranks start at one.
    let rank = (sorted.len() * percent).div_ceil(100);
    sorted.get(rank.saturating_sub(1)).copied()
}

/// A duration as whole nanoseconds; one too long to hold is held as the largest value.
#[must_use]
pub fn whole_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// The signed difference `a - b` of two unsigned numbers, held within the range of the result.
#[must_use]
pub fn signed_difference(a: u64, b: u64) -> i64 {
    if a >= b {
        i64::try_from(a - b).unwrap_or(i64::MAX)
    } else {
        i64::try_from(b - a).map_or(i64::MIN, |d| -d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_to_a_hundred_gives_the_textbook_percentiles() {
        // Shuffled on purpose: the order of the samples must not matter.
        let mut samples: Vec<u64> = (1..=100).collect();
        samples.reverse();
        samples.swap(3, 77);
        let stats = Stats::from_nanos(&samples).unwrap();
        assert_eq!(
            stats,
            Stats {
                count: 100,
                min_ns: 1,
                median_ns: 50,
                p95_ns: 95,
                p99_ns: 99,
                max_ns: 100,
            }
        );
    }

    #[test]
    fn small_sets_take_the_nearest_rank() {
        // Four samples: the median is the second, and both high percentiles are the last.
        let stats = Stats::from_nanos(&[40, 10, 30, 20]).unwrap();
        assert_eq!(
            (stats.min_ns, stats.median_ns, stats.p95_ns, stats.max_ns),
            (10, 20, 40, 40)
        );
        // Five samples: the median is the middle one.
        assert_eq!(
            Stats::from_nanos(&[5, 1, 4, 2, 3]).unwrap().median_ns,
            3,
            "an odd count has a true middle"
        );
        // One sample is every statistic.
        let one = Stats::from_durations(&[Duration::from_micros(7)]).unwrap();
        assert_eq!(
            (one.count, one.min_ns, one.p99_ns, one.max_ns),
            (1, 7000, 7000, 7000)
        );
        // Twenty samples: the 95th percentile is the 19th, the 99th is the 20th.
        let twenty: Vec<u64> = (1..=20).map(|n| n * 10).collect();
        let stats = Stats::from_nanos(&twenty).unwrap();
        assert_eq!((stats.p95_ns, stats.p99_ns), (190, 200));
    }

    #[test]
    fn nothing_measured_has_no_statistics() {
        assert_eq!(Stats::from_nanos(&[]), None);
        assert_eq!(percentile(&[1, 2, 3], 0), None);
        assert_eq!(percentile(&[1, 2, 3], 101), None);
    }

    #[test]
    fn differences_keep_their_sign() {
        assert_eq!(signed_difference(9, 4), 5);
        assert_eq!(signed_difference(4, 9), -5);
        assert_eq!(signed_difference(u64::MAX, 0), i64::MAX);
        assert_eq!(signed_difference(0, u64::MAX), i64::MIN);
    }
}
