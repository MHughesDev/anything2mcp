//! What the kernel and the approval desk tell each other about runs, in
//! memory: that something about a run changed, which runs a person has
//! approved and nobody has resumed yet, and the link a run's person was
//! last given.
//!
//! None of it decides anything. Whether a write may proceed is the policy
//! engine's answer, read from the store when the run is resumed. What is
//! kept here only shapes what a caller is told, and is gone with the
//! process.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use a2m_core::{Digest, RunId, RunState};
use tokio::sync::watch;

use crate::person::SurfaceLink;

/// The most runs whose answer or link is remembered. The oldest go first.
const REMEMBERED: usize = 1024;

/// The link a run's person was given when the run last rested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeptLink {
    /// The state the run rested in.
    pub(crate) state: RunState,
    /// The diff the person was asked about, for a run that waits for an
    /// approval.
    pub(crate) diff: Option<Digest>,
    /// The link and its words.
    pub(crate) link: SurfaceLink,
}

/// Shared by the kernel and the approval desk.
pub(crate) struct RunSignals {
    changed: watch::Sender<u64>,
    approved: Mutex<BTreeMap<RunId, Digest>>,
    links: Mutex<BTreeMap<RunId, KeptLink>>,
}

impl Default for RunSignals {
    fn default() -> Self {
        Self {
            changed: watch::Sender::new(0),
            approved: Mutex::new(BTreeMap::new()),
            links: Mutex::new(BTreeMap::new()),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Keeps `map` within its bound. A run's identifier begins with the moment
/// it was made, so the first entry is the oldest run.
fn bound<T>(map: &mut BTreeMap<RunId, T>) {
    while map.len() > REMEMBERED {
        map.pop_first();
    }
}

impl RunSignals {
    /// Tells everyone who waits for a run that something about a run
    /// changed.
    pub(crate) fn changed(&self) {
        self.changed
            .send_modify(|count| *count = count.wrapping_add(1));
    }

    /// A receiver that is told whenever something about a run changed.
    pub(crate) fn watch(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// Notes that a person approved `diff` of `run` at the desk.
    pub(crate) fn note_approved(&self, run: RunId, diff: Digest) {
        let mut approved = lock(&self.approved);
        approved.insert(run, diff);
        bound(&mut approved);
    }

    /// Forgets what was noted about `run`: it was resumed, declined, or has
    /// ended.
    pub(crate) fn forget(&self, run: RunId) {
        lock(&self.approved).remove(&run);
        lock(&self.links).remove(&run);
    }

    /// Whether a person approved exactly `diff` of `run` in this process
    /// and the run has not been worked on since.
    pub(crate) fn approved(&self, run: RunId, diff: &Digest) -> bool {
        lock(&self.approved).get(&run) == Some(diff)
    }

    /// Keeps the link the run's person was given.
    pub(crate) fn keep_link(&self, run: RunId, link: KeptLink) {
        let mut links = lock(&self.links);
        links.insert(run, link);
        bound(&mut links);
    }

    /// The link the run's person was given when it rested in `state` with
    /// `diff`, when one is remembered.
    pub(crate) fn link(
        &self,
        run: RunId,
        state: RunState,
        diff: Option<&Digest>,
    ) -> Option<SurfaceLink> {
        lock(&self.links)
            .get(&run)
            .filter(|kept| kept.state == state && kept.diff.as_ref() == diff)
            .map(|kept| kept.link.clone())
    }
}
