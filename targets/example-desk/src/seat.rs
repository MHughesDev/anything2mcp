//! The one desktop of a test process on Windows and macOS: the tests of one
//! binary run in parallel, and two windows of one program would be two
//! applications to the pack, so a session takes the seat and waits while
//! another test holds it.
//!
//! A test that already holds the seat and asks for it again would wait on
//! itself: the seat is given back only when its first session is gone,
//! which that test still holds. It is told at once, by a failure that says
//! to drop the first scene, instead of holding every other test of the
//! process until the wait runs out. The seat is the holder's thread's: a
//! test runs on its own thread, and a Tokio test polls its body on that
//! thread.
//!
//! Built on every system for its own tests; only Windows and macOS sit on
//! it.

use std::sync::{Condvar, Mutex, PoisonError};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use crate::StandError;

/// One desktop, held by one thread at a time.
pub(crate) struct Seat {
    holder: Mutex<Option<ThreadId>>,
    freed: Condvar,
}

/// The seat, taken; dropping it gives the seat back.
pub(crate) struct Taken {
    seat: &'static Seat,
}

impl Seat {
    pub(crate) const fn new() -> Self {
        Self {
            holder: Mutex::new(None),
            freed: Condvar::new(),
        }
    }

    /// Takes the seat for this thread, waiting up to `limit` while another
    /// thread holds it.
    ///
    /// # Errors
    ///
    /// At once, when this thread holds the seat already; after `limit`,
    /// when another thread still holds it.
    pub(crate) fn take(&'static self, limit: Duration) -> Result<Taken, StandError> {
        let me = std::thread::current().id();
        let start = Instant::now();
        let mut holder = self.holder.lock().unwrap_or_else(PoisonError::into_inner);
        while let Some(held_by) = *holder {
            if held_by == me {
                return Err(StandError(
                    "this test holds the desktop already, and would wait on itself: drop its first scene, session, and stand before it starts another".to_owned(),
                ));
            }
            let left = limit.saturating_sub(start.elapsed());
            if left.is_zero() {
                return Err(StandError(format!(
                    "another test of this process held the desktop for {} seconds",
                    limit.as_secs()
                )));
            }
            holder = self
                .freed
                .wait_timeout(holder, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *holder = Some(me);
        Ok(Taken { seat: self })
    }
}

impl Drop for Taken {
    fn drop(&mut self) {
        *self
            .seat
            .holder
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        self.seat.freed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test stops on a helper that fails")]

    use super::*;

    /// Far longer than any answer below should take: a test that waits it
    /// out has found the fault.
    const LIMIT: Duration = Duration::from_secs(60);
    /// How soon a refusal comes.
    const AT_ONCE: Duration = Duration::from_secs(5);

    #[test]
    fn a_test_that_asks_again_for_the_desktop_it_holds_is_told_at_once() {
        static SEAT: Seat = Seat::new();
        let first = SEAT.take(LIMIT).unwrap();
        let start = Instant::now();
        let Err(again) = SEAT.take(LIMIT) else {
            panic!("the seat was taken twice");
        };
        assert!(start.elapsed() < AT_ONCE, "{:?}", start.elapsed());
        assert!(again.0.contains("drop its first scene"), "{again}");
        // Once the first is given back, the same test takes it again.
        drop(first);
        drop(SEAT.take(LIMIT).unwrap());
    }

    #[test]
    fn another_test_waits_for_the_desktop_and_takes_it_once_it_is_given_back() {
        static SEAT: Seat = Seat::new();
        let first = SEAT.take(LIMIT).unwrap();
        let (taking, taken) = std::sync::mpsc::channel();
        let other = std::thread::spawn(move || {
            let seat = SEAT.take(LIMIT);
            let _ = taking.send(());
            seat.map(drop)
        });
        // It waits while the seat is held, and takes it once it is given
        // back: well inside the limit.
        assert!(taken.try_recv().is_err());
        let start = Instant::now();
        drop(first);
        taken.recv_timeout(AT_ONCE).unwrap();
        assert!(start.elapsed() < AT_ONCE, "{:?}", start.elapsed());
        other.join().unwrap().unwrap();
    }

    #[test]
    fn a_wait_for_a_desktop_held_elsewhere_ends_at_its_limit() {
        static SEAT: Seat = Seat::new();
        let (held, holding) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let taken = SEAT.take(LIMIT).unwrap();
            held.send(()).unwrap();
            let _ = released.recv();
            drop(taken);
        });
        holding.recv().unwrap();
        let Err(waited) = SEAT.take(Duration::from_millis(50)) else {
            panic!("the seat was taken while another thread held it");
        };
        assert!(waited.0.contains("another test"), "{waited}");
        release.send(()).unwrap();
        holder.join().unwrap();
    }
}
