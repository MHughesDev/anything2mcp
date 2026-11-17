//! Who waits, in memory: for a moment, for an exclusive resource, for a
//! record lock.
//!
//! Nothing here sleeps. A wait ends when what it waits for happens, or when
//! a sweep finds that its moment has come on the store's clock.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use a2m_core::{JobId, Timestamp};
use tokio::sync::oneshot;

use crate::types::ResourceKey;

/// How many freed record locks are remembered for tickets taken earlier.
const FREED_KEPT: usize = 1024;

/// A job that waits, during its run, for resources it must hold alone.
pub(crate) struct ResourceWaiter {
    pub(crate) id: u64,
    pub(crate) job: JobId,
    pub(crate) keys: Vec<ResourceKey>,
    pub(crate) granted: oneshot::Sender<()>,
}

/// The moment a caller looked at a record lock, taken before it asks the
/// ledger for the lock. A wait that starts with it does not miss a release
/// that happened in between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockTicket {
    pub(crate) key: ResourceKey,
    pub(crate) generation: u64,
}

impl LockTicket {
    /// The lock the ticket is for.
    #[must_use]
    pub fn key(&self) -> &ResourceKey {
        &self.key
    }
}

#[derive(Default)]
pub(crate) struct Waits {
    next_id: u64,
    /// Waits for a moment, by moment and then by arrival.
    timers: BTreeMap<(i64, u64), oneshot::Sender<()>>,
    /// Waits for exclusive resources, in order of arrival.
    resources: Vec<ResourceWaiter>,
    /// Waits for a record lock, in order of arrival for each lock.
    locks: BTreeMap<ResourceKey, VecDeque<(u64, oneshot::Sender<()>)>>,
    /// The generation at which each lock was last freed.
    freed: BTreeMap<ResourceKey, u64>,
    generation: u64,
    /// Tickets older than this may have missed a release nobody remembers.
    forgotten_below: u64,
}

impl Waits {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    // ---- moments ---------------------------------------------------------

    pub(crate) fn add_timer(&mut self, at: Timestamp) -> ((i64, u64), oneshot::Receiver<()>) {
        let key = (at.as_unix_millis(), self.id());
        let (fire, fired) = oneshot::channel();
        self.timers.insert(key, fire);
        (key, fired)
    }

    pub(crate) fn remove_timer(&mut self, key: (i64, u64)) {
        self.timers.remove(&key);
    }

    /// Ends every wait whose moment is `now` or earlier, and says how many.
    pub(crate) fn fire_timers(&mut self, now: Timestamp) -> usize {
        let later = self
            .timers
            .split_off(&(now.as_unix_millis().saturating_add(1), 0));
        let due = std::mem::replace(&mut self.timers, later);
        due.into_values()
            .map(|fire| fire.send(()))
            .filter(Result::is_ok)
            .count()
    }

    pub(crate) fn next_timer(&self) -> Option<Timestamp> {
        self.timers
            .keys()
            .next()
            .map(|(at, _)| Timestamp::from_unix_millis(*at))
    }

    // ---- exclusive resources ---------------------------------------------

    pub(crate) fn add_resource_waiter(
        &mut self,
        job: JobId,
        keys: Vec<ResourceKey>,
    ) -> (u64, oneshot::Receiver<()>) {
        let id = self.id();
        let (granted, wait) = oneshot::channel();
        self.resources.push(ResourceWaiter {
            id,
            job,
            keys,
            granted,
        });
        (id, wait)
    }

    /// Forgets the waiters nobody listens to any more, and lists the rest in
    /// order.
    pub(crate) fn resource_waiters(&mut self) -> Vec<(u64, JobId, Vec<ResourceKey>)> {
        self.resources.retain(|waiter| !waiter.granted.is_closed());
        self.resources
            .iter()
            .map(|waiter| (waiter.id, waiter.job, waiter.keys.clone()))
            .collect()
    }

    /// Every key someone waits for.
    pub(crate) fn waited_for(&mut self) -> BTreeSet<ResourceKey> {
        self.resource_waiters()
            .into_iter()
            .flat_map(|(_, _, keys)| keys)
            .collect()
    }

    /// Takes a waiter out. `None` when it was granted or removed before.
    pub(crate) fn remove_resource_waiter(&mut self, id: u64) -> Option<ResourceWaiter> {
        let at = self.resources.iter().position(|waiter| waiter.id == id)?;
        Some(self.resources.remove(at))
    }

    // ---- record locks ----------------------------------------------------

    pub(crate) fn ticket(&self, key: &ResourceKey) -> LockTicket {
        LockTicket {
            key: key.clone(),
            generation: self.generation,
        }
    }

    /// Whether the lock was freed, or may have been, since the ticket.
    pub(crate) fn freed_since(&self, ticket: &LockTicket) -> bool {
        ticket.generation < self.forgotten_below
            || self
                .freed
                .get(&ticket.key)
                .is_some_and(|at| *at > ticket.generation)
    }

    pub(crate) fn add_lock_waiter(&mut self, key: &ResourceKey) -> (u64, oneshot::Receiver<()>) {
        let id = self.id();
        let (wake, woken) = oneshot::channel();
        self.locks
            .entry(key.clone())
            .or_default()
            .push_back((id, wake));
        (id, woken)
    }

    /// Takes a lock waiter out. `false` when it was woken before.
    pub(crate) fn remove_lock_waiter(&mut self, key: &ResourceKey, id: u64) -> bool {
        let Some(queue) = self.locks.get_mut(key) else {
            return false;
        };
        let before = queue.len();
        queue.retain(|(waiter, _)| *waiter != id);
        let removed = queue.len() < before;
        if queue.is_empty() {
            self.locks.remove(key);
        }
        removed
    }

    /// The lock was released: remembers it, and wakes the waiter that came
    /// first.
    pub(crate) fn lock_freed(&mut self, key: &ResourceKey) {
        self.generation += 1;
        self.freed.insert(key.clone(), self.generation);
        if self.freed.len() > FREED_KEPT {
            let cutoff = self.generation.saturating_sub(FREED_KEPT as u64 / 2);
            self.freed.retain(|_, at| *at > cutoff);
            self.forgotten_below = cutoff;
        }
        if let Some(queue) = self.locks.get_mut(key) {
            while let Some((_, wake)) = queue.pop_front() {
                if wake.send(()).is_ok() {
                    break;
                }
            }
            if queue.is_empty() {
                self.locks.remove(key);
            }
        }
    }

    /// Locks may have been released that nobody named: wakes every waiter,
    /// so that each asks the ledger again.
    pub(crate) fn locks_changed(&mut self) {
        self.generation += 1;
        self.forgotten_below = self.generation;
        self.freed.clear();
        for (_, queue) in std::mem::take(&mut self.locks) {
            for (_, wake) in queue {
                let _ = wake.send(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::Timestamp;

    use super::Waits;
    use crate::types::ResourceKey;

    fn key(name: &str) -> ResourceKey {
        ResourceKey::new("record", name).unwrap()
    }

    #[test]
    fn timers_fire_in_order_and_only_when_their_moment_has_come() {
        let mut waits = Waits::default();
        let (_, mut early) = waits.add_timer(Timestamp::from_unix_millis(10));
        let (_, mut late) = waits.add_timer(Timestamp::from_unix_millis(20));
        assert_eq!(waits.next_timer(), Some(Timestamp::from_unix_millis(10)));
        assert_eq!(waits.fire_timers(Timestamp::from_unix_millis(9)), 0);
        assert_eq!(waits.fire_timers(Timestamp::from_unix_millis(10)), 1);
        assert!(early.try_recv().is_ok());
        assert!(late.try_recv().is_err());
        assert_eq!(waits.next_timer(), Some(Timestamp::from_unix_millis(20)));
    }

    #[test]
    fn a_ticket_sees_a_release_that_came_before_the_wait() {
        let mut waits = Waits::default();
        let ticket = waits.ticket(&key("a"));
        assert!(!waits.freed_since(&ticket));
        waits.lock_freed(&key("b"));
        assert!(!waits.freed_since(&ticket), "another lock was freed");
        waits.lock_freed(&key("a"));
        assert!(waits.freed_since(&ticket));
        assert!(!waits.freed_since(&waits.ticket(&key("a"))));
    }

    #[test]
    fn lock_waiters_are_woken_one_at_a_time_in_the_order_they_came() {
        let mut waits = Waits::default();
        let (_, mut first) = waits.add_lock_waiter(&key("a"));
        let (gone, dropped) = waits.add_lock_waiter(&key("a"));
        let (_, mut third) = waits.add_lock_waiter(&key("a"));
        drop(dropped);
        waits.lock_freed(&key("a"));
        assert!(first.try_recv().is_ok());
        assert!(third.try_recv().is_err());
        // The second gave up; the next release skips it.
        waits.lock_freed(&key("a"));
        assert!(third.try_recv().is_ok());
        assert!(!waits.remove_lock_waiter(&key("a"), gone));
    }
}
