//! Rate budgets: token buckets kept in the store, so that a restart does not
//! hand a system a second full burst.
//!
//! A bucket's level is kept in requests times the refill period in
//! milliseconds, so every refill is a whole number and nothing is rounded
//! away. Time is the transaction's moment and nothing else.

use std::time::Duration;

use a2m_core::{Failure, Timestamp};
use a2m_ledger::{Owner, SqlValue, Tx};

use crate::failure;
use crate::types::{BudgetKey, RateLimit};

/// One day, the longest refill period.
const LONGEST_PERIOD_MS: i64 = 86_400_000;

/// What asking the buckets gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Take {
    /// This many requests were taken from every bucket; the slowest bucket
    /// gains as many again in `over`.
    Granted { requests: u32, over: Duration },
    /// Nothing was taken. A grant is possible at `until`.
    Wait { until: Timestamp },
}

struct Bucket {
    capacity: i64,
    refill: i64,
    per_ms: i64,
    level: i64,
}

impl Bucket {
    fn full(&self) -> i64 {
        self.capacity.saturating_mul(self.per_ms)
    }

    /// The level after `elapsed_ms` more milliseconds.
    fn refilled(&self, elapsed_ms: i64) -> i64 {
        let gained = i128::from(elapsed_ms.max(0)) * i128::from(self.refill);
        let level = i128::from(self.level) + gained;
        i64::try_from(level.min(i128::from(self.full()))).unwrap_or(self.full())
    }

    /// How many milliseconds until the bucket holds `requests`.
    fn wait_for(&self, requests: i64) -> i64 {
        let short = requests.saturating_mul(self.per_ms) - self.level;
        if short <= 0 {
            0
        } else {
            // Rounded up: a moment too early would grant nothing.
            (short + self.refill - 1) / self.refill
        }
    }
}

fn limit_parts(limit: &RateLimit) -> Result<(i64, i64, i64), Failure> {
    let per_ms = i64::try_from(limit.per.as_millis()).unwrap_or(i64::MAX);
    if limit.capacity == 0 || limit.refill == 0 || !(1..=LONGEST_PERIOD_MS).contains(&per_ms) {
        return Err(failure::invalid(
            "a rate limit holds at least one request and refills at least one in a period between one millisecond and one day",
        ));
    }
    Ok((i64::from(limit.capacity), i64::from(limit.refill), per_ms))
}

/// Sets the limit of a budget. A new budget starts full. An existing one
/// keeps what it holds, cut to the new capacity.
pub(crate) fn set_limit(
    tx: &mut Tx<'_>,
    key: &BudgetKey,
    limit: &RateLimit,
) -> Result<(), Failure> {
    let (capacity, refill, per_ms) = limit_parts(limit)?;
    let now = tx.now();
    let bucket = load(tx, key, now)?;
    let level = match bucket {
        // What it holds, in requests, carried over to the new period.
        Some(old) => (old.level / old.per_ms).min(capacity) * per_ms,
        None => capacity * per_ms,
    };
    tx.tables(Owner::Sched).execute(
        "INSERT INTO budgets (budget_key, capacity, refill, per_ms, level, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (budget_key) DO UPDATE
         SET capacity = ?2, refill = ?3, per_ms = ?4, level = ?5, updated_at = ?6",
        &[
            key.as_str().into(),
            capacity.into(),
            refill.into(),
            per_ms.into(),
            level.into(),
            now.into(),
        ],
    )?;
    Ok(())
}

/// The bucket as it stands at `now`, refilled by the time that has passed.
fn load(tx: &mut Tx<'_>, key: &BudgetKey, now: Timestamp) -> Result<Option<Bucket>, Failure> {
    let Some(row) = tx.tables(Owner::Sched).query_first(
        "SELECT capacity, refill, per_ms, level, updated_at FROM budgets WHERE budget_key = ?1",
        &[key.as_str().into()],
    )?
    else {
        return Ok(None);
    };
    let mut bucket = Bucket {
        capacity: row.int(0)?,
        refill: row.int(1)?,
        per_ms: row.int(2)?,
        level: row.int(3)?,
    };
    if bucket.capacity <= 0 || bucket.refill <= 0 || bucket.per_ms <= 0 {
        return Err(failure::corrupt("a budget with no capacity or no refill"));
    }
    // A clock that went backwards refills nothing.
    bucket.level = bucket.refilled(now.as_unix_millis().saturating_sub(row.int(4)?));
    Ok(Some(bucket))
}

/// Takes up to `want` requests from every one of `keys` at once: the same
/// number from each, never more than the emptiest holds. With `whole`, it is
/// `want` or nothing. A budget nobody set is made with `default`, full.
pub(crate) fn take(
    tx: &mut Tx<'_>,
    default: &RateLimit,
    keys: &[BudgetKey],
    want: u32,
    whole: bool,
) -> Result<Take, Failure> {
    if want == 0 {
        return Err(failure::invalid("a grant of no requests"));
    }
    let now = tx.now();
    let mut buckets = Vec::new();
    for key in keys {
        let bucket = match load(tx, key, now)? {
            Some(bucket) => bucket,
            None => {
                set_limit(tx, key, default)?;
                load(tx, key, now)?
                    .ok_or_else(|| failure::corrupt("a budget that was just made"))?
            }
        };
        buckets.push((key, bucket));
    }
    let want = i64::from(want);
    let available = buckets
        .iter()
        .map(|(_, bucket)| bucket.level / bucket.per_ms)
        .min()
        .unwrap_or(want);
    let granted = if whole && available < want {
        0
    } else {
        available.min(want)
    };
    if granted == 0 {
        // The moment the slowest bucket holds enough. A grant larger than a
        // bucket's capacity is never possible, so it waits for a full one.
        let wait_ms = buckets
            .iter()
            .map(|(_, bucket)| {
                let needed = if whole { want.min(bucket.capacity) } else { 1 };
                bucket.wait_for(needed)
            })
            .max()
            .unwrap_or(0)
            .max(1);
        return Ok(Take::Wait {
            until: Timestamp::from_unix_millis(now.as_unix_millis().saturating_add(wait_ms)),
        });
    }
    let mut over_ms = 0;
    for (key, bucket) in &buckets {
        let level = bucket.level - granted * bucket.per_ms;
        tx.tables(Owner::Sched).execute(
            "UPDATE budgets SET level = ?2, updated_at = ?3 WHERE budget_key = ?1",
            &[SqlValue::from(key.as_str()), level.into(), now.into()],
        )?;
        over_ms = over_ms.max((granted * bucket.per_ms + bucket.refill - 1) / bucket.refill);
    }
    Ok(Take::Granted {
        requests: u32::try_from(granted).unwrap_or(u32::MAX),
        over: Duration::from_millis(u64::try_from(over_ms).unwrap_or(0)),
    })
}

#[cfg(test)]
mod tests {
    use super::Bucket;

    #[test]
    fn a_bucket_refills_in_whole_steps_and_never_past_its_capacity() {
        // Three requests every two seconds, at most ten.
        let bucket = Bucket {
            capacity: 10,
            refill: 3,
            per_ms: 2_000,
            level: 0,
        };
        assert_eq!(bucket.refilled(0), 0);
        // One request is 2000 units; 667 ms give 2001.
        assert_eq!(bucket.refilled(666) / bucket.per_ms, 0);
        assert_eq!(bucket.refilled(667) / bucket.per_ms, 1);
        assert_eq!(bucket.wait_for(1), 667);
        assert_eq!(bucket.refilled(1_000_000_000), bucket.full());
        assert_eq!(bucket.refilled(-5), 0);
        assert_eq!(bucket.wait_for(0), 0);
    }
}
