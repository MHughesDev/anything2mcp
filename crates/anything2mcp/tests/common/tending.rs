//! How a test that moves the engine's clock by hand waits for a watch's
//! check to end. The wait is on the watch itself, its state and its count
//! of checks, and is bounded by real time: a check is a read of the test
//! application, which a loaded machine makes slow, and the engine's clock
//! does not measure it. The clock moves on between two passes, as it would
//! while a check's requests wait for their budget, which refills by the
//! store's clock; it never reaches the watch's next due time, so no other
//! check comes due while the test waits, however many passes a slow read
//! takes.

use std::time::Duration;

use serde_json::Value as Json;

/// How long, in real time, a test tends for a check to end before it fails
/// by name.
pub const CHECK_WAIT: Duration = Duration::from_secs(180);

/// How far the engine's clock moves between two passes of tending.
pub const PASS_STEP: Duration = Duration::from_millis(100);

/// The first due time of the watch `before`, as `watch get` gave it while
/// the watch waited, that is later than `now`: the due time after the one
/// the clock has reached. A test that waits for a check moves the clock no
/// further than just before it.
pub fn next_due_after(before: &Json, now: i64) -> i64 {
    let due = before["next_due_at"]
        .as_i64()
        .unwrap_or_else(|| panic!("the watch waits for no due time: {before}"));
    let interval = before["interval_seconds"].as_i64().unwrap() * 1_000;
    assert!(
        now >= due,
        "the clock has not reached the due time: {before}"
    );
    due + interval * ((now - due) / interval + 1)
}

/// Whether the clock, at `now`, may move on by [`PASS_STEP`] and stay
/// before `next`.
pub fn may_step(now: i64, next: i64) -> bool {
    now + i64::try_from(PASS_STEP.as_millis()).unwrap() < next
}
