//! A warm observing tab (decision 0488): an observing tab a read is done
//! with, made fresh and kept for the next read of the same surface and
//! session, so that the next read is not given a tab the browser must make
//! a page and a process for.
//!
//! A tab is kept only when nothing unexpected happened in it, and it is
//! made fresh before it is kept: its session storage is cleared while its
//! document is still the read's, the document is erased so that its leaving
//! runs nothing of the page's, the tab is sent to `about:blank` by the
//! engine, and its history is emptied. Its interception, its environment,
//! its asserted reads, and its socket script stay as they were set when it
//! was opened: a tab is handed again only to a read that would have opened
//! it with the same, so it refuses what a new tab would.

use std::sync::atomic::Ordering;

use a2m_provider::Deadline;

use super::{Moves, Page, Status};
use crate::origin::origin_of;
use crate::protocol::{
    ClearStorage, GetFrameTree, GetNavigationHistory, GetStorageItems, Navigate,
    ResetNavigationHistory, SetDocumentContent, StorageId,
};

fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Why a tab is not kept warm: the first thing found that a new tab would
/// not have, or a step of making it fresh that did not hold.
pub(crate) type NotKept = &'static str;

impl Page {
    /// Whether the tab holds nothing a new tab would not: open, answering,
    /// with nothing refused that was not taken, no dialog, no window, no
    /// download, no tool of the page, no exchange recorded, and its browser
    /// still the one its profile runs, not ending.
    pub(crate) fn as_new(&self) -> Result<(), NotKept> {
        if *self.status.borrow() != Status::Open || self.released.load(Ordering::SeqCst) {
            return Err("the tab is not open");
        }
        if self.probing.load(Ordering::SeqCst) {
            return Err("the tab is being probed after a call it did not answer");
        }
        if self.safe.is_none() {
            return Err("the tab is not an observing tab");
        }
        if self.recording.load(Ordering::SeqCst) {
            return Err("the tab recorded its page's exchanges");
        }
        if !lock(&self.blocked).is_empty() {
            return Err("the tab refused something that was not taken");
        }
        if lock(&self.open_dialog).is_some() {
            return Err("a dialog is open");
        }
        if !lock(&self.download).untouched() {
            return Err("a download was allowed or began");
        }
        if !lock(&self.site).untouched() {
            return Err("the page's tools were heard");
        }
        if !self.browser.windows_of(&self.target).is_empty() {
            return Err("a window the page opened is still there");
        }
        if self.browser.client.closed().is_some() || self.browser.going.load(Ordering::SeqCst) {
            return Err("the tab's browser is gone or ending");
        }
        Ok(())
    }

    /// Makes the tab what a new observing tab of the same setup is, or says
    /// why it cannot be (decision 0488, point 2). The tab must hold nothing
    /// a new tab would not ([`Self::as_new`]), its frame must hold no frame
    /// of its own, and its document must be on its one navigable origin or
    /// on none. Then: that origin's session storage is cleared, and found
    /// empty; the document is erased; the tab is sent to `about:blank` and
    /// waits for that document; its history is emptied, and found to hold
    /// that one entry; and still nothing was refused, and no request of a
    /// page awaits its answer.
    pub(crate) async fn made_fresh(&self, deadline: Deadline) -> Result<(), NotKept> {
        self.as_new()?;
        // A `data:` page has an opaque origin, which holds no storage.
        let mut allowed = self
            .origins
            .iter()
            .filter(|origin| origin_of(origin).is_some());
        let (Some(origin), None) = (allowed.next(), allowed.next()) else {
            return Err("the tab may be on more than one origin");
        };
        let tree = self
            .call(&GetFrameTree {}, deadline)
            .await
            .map_err(|_| "the frames of the tab could not be listed")?;
        if !tree.frame_tree.child_frames.is_empty() {
            return Err("the page holds a frame");
        }
        let on = origin_of(&tree.frame_tree.frame.url);
        match on.as_deref() {
            Some(on) if on == origin => {
                let storage = || StorageId {
                    security_origin: origin,
                    is_local_storage: false,
                };
                self.call(
                    &ClearStorage {
                        storage_id: storage(),
                    },
                    deadline,
                )
                .await
                .map_err(|_| "the tab's session storage could not be cleared")?;
                let left = self
                    .call(
                        &GetStorageItems {
                            storage_id: storage(),
                        },
                        deadline,
                    )
                    .await
                    .map_err(|_| "the tab's session storage could not be read")?;
                if !left.entries.is_empty() {
                    return Err("the tab's session storage was not empty once cleared");
                }
            }
            // A document on no origin of the web holds no storage of one;
            // a tab on an origin of the web it may not be on is not kept.
            Some(_) => return Err("the tab's document is on another origin"),
            None => {
                if self.nav.borrow().loaded.is_some() {
                    return Err("the tab left its origin's document for one on none");
                }
            }
        }
        // As at a release: the document's handlers go before it does.
        self.call(
            &SetDocumentContent {
                frame_id: &self.target,
                html: "",
            },
            deadline,
        )
        .await
        .map_err(|_| "the tab's document could not be erased")?;
        let mut nav = self.nav.subscribe();
        let navigated = self
            .call(&Navigate { url: "about:blank" }, deadline)
            .await
            .map_err(|_| "the tab could not be sent to an empty page")?;
        if navigated.error_text.is_some_and(|why| !why.is_empty()) {
            return Err("the tab could not be sent to an empty page");
        }
        let Some(loader) = navigated.loader_id else {
            return Err("the browser named no document for the empty page");
        };
        // The empty page's document is parsed: a protocol event.
        let parsed = tokio::time::timeout_at(
            deadline.at(),
            nav.wait_for(|nav| nav.loaded.as_deref() == Some(loader.as_str())),
        )
        .await
        .is_ok_and(|seen| seen.is_ok());
        if !parsed {
            return Err("the empty page was not parsed by the deadline");
        }
        self.call(&ResetNavigationHistory {}, deadline)
            .await
            .map_err(|_| "the tab's history could not be emptied")?;
        let history = self
            .call(&GetNavigationHistory {}, deadline)
            .await
            .map_err(|_| "the tab's history could not be read")?;
        let one = history.current_index == 0
            && history.entries.len() == 1
            && history.entries[0].url == "about:blank";
        if !one {
            return Err("the tab's history holds more than the empty page");
        }
        // What the read's page left: nothing awaits its answer, nothing was
        // refused on its way out, and nothing of it is kept.
        if *self.waiting.borrow() != 0 || !lock(&self.pending).is_empty() {
            return Err("a request of the page still awaits its answer");
        }
        self.as_new()?;
        lock(&self.exchanges).clear();
        self.dropped.store(0, Ordering::SeqCst);
        *lock(&self.document) = None;
        *self.moves() = Moves::default();
        Ok(())
    }
}
