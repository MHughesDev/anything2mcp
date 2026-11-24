//! The watching tab (decision 0392, point 1): a tab of the engine's own
//! browser, in a window, that a person uses while the engine watches.
//!
//! The engine opens it on the surface's own address, once that address has
//! passed the check every address of a discovery passes ([`WatchStart`]),
//! and sends it nowhere after that. It has no method that presses, types,
//! navigates, or sends a request, and nothing in it is failed for its
//! method: every request in it is the person's. It is held to its origins
//! as a sign-in tab is: a move of the tab to another origin is ended in the
//! browser and reported. It records every request its page sends to one of
//! its origins, by structure alone, from the browser's network events (it
//! holds no request of a script), and the body each sends by its media type
//! and the names and kinds of its fields; and it takes a snapshot of each
//! screen the person visits, with no value of any element but the mark of
//! a password.

use std::collections::BTreeMap;
use std::sync::Weak;

use a2m_core::{Failure, FailureClass};
use serde_json::Value as Json;

use super::{Page, PageExchanges, Status, fields_of, without_query};
use crate::browsers::{Address, Browser};
use crate::error::{BrowserError, RequestShape, codes};
use crate::origin::Origins;
use crate::protocol::{HeldBody, RequestSent, Sent};
use crate::snapshot::{MASK, Snapshot, SnapshotLimits};
use crate::structure::looks_like_an_action;
use a2m_provider::Deadline;

/// The longest body of a request whose structure a watching tab reads, in
/// bytes. The browser leaves a longer one out of its event, and the request
/// is recorded with its media type and no fields.
pub const LONGEST_WATCHED_BODY: usize = 64 * 1024;

/// The most screens a watching tab takes a snapshot of. Past it, no more
/// are taken, and [`WatchedScreens::bound_hit`] says so.
pub const SCREENS_KEPT: usize = 40;

/// How long a watching tab waits, after its page's address changes, for
/// the page's requests to end before it takes the snapshot all the same.
pub const SETTLE_BOUND: std::time::Duration = std::time::Duration::from_secs(10);

/// The resource types of the requests of a page that a watching tab
/// records whatever their method: a script's, and a beacon. A request of a
/// document is recorded when it carries a body (a form the person posts).
const RECORDED_KINDS: &[&str] = &["XHR", "Fetch", "Ping"];

/// The address a watching tab is opened on: one on the origins given whose
/// path, query, and fragment do not look like an action by
/// [`crate::ACTION_WORDS`], the check every address of a discovery passes
/// (decision 0144).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchStart(Address);

impl WatchStart {
    /// Checks the address against the origins and the action words.
    ///
    /// # Errors
    ///
    /// `policy`, `surface_not_declared` for an address on none of the
    /// origins; `policy`, `address_looks_like_an_action` for one that looks
    /// like an action. Nothing is opened or sent.
    pub fn new(address: &Address, origins: &Origins) -> Result<Self, Failure> {
        if !origins.allows(address) {
            return Err(Failure::new(
                FailureClass::Policy,
                codes::SURFACE_NOT_DECLARED,
                "A watching tab was to be opened on an address outside the surface's origins. Nothing was opened.",
                "Open it on the surface's own address.",
            ));
        }
        let text = address.as_str();
        let rest = text.split_once("://").map_or(text, |(_, rest)| rest);
        let after_origin = rest.find(['/', '?', '#']).map_or("", |at| &rest[at..]);
        if looks_like_an_action(after_origin) {
            return Err(Failure::new(
                FailureClass::Policy,
                codes::ADDRESS_LOOKS_LIKE_AN_ACTION,
                "A watching tab was to be opened on an address that looks like an action, and a system may act on a plain visit to such an address. Nothing was opened.",
                "Open it on the surface's own address; the engine never sends a tab to an address that looks like an action.",
            ));
        }
        Ok(Self(address.clone()))
    }

    /// The address.
    #[must_use]
    pub fn address(&self) -> &Address {
        &self.0
    }
}

/// What [`WatchTab::screens`] gives: the snapshots taken since the last
/// call, and what was not taken.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchedScreens {
    /// One snapshot for each address the page came to, oldest first, by
    /// structure: no element keeps a value, but a password input keeps
    /// [`MASK`]. A snapshot's address has no query.
    pub snapshots: Vec<Snapshot>,
    /// Whether the tab came to an address after it had taken
    /// [`SCREENS_KEPT`] snapshots, and took none of it.
    pub bound_hit: bool,
    /// How many addresses the page came to whose snapshot could not be
    /// taken, because the page moved on or closed first.
    pub missed: usize,
}

/// What a watching tab holds of the screens of its page.
#[derive(Debug, Default)]
pub(crate) struct Screens {
    taken: Vec<Snapshot>,
    count: usize,
    bound_hit: bool,
    missed: usize,
}

/// A tab a person uses while the engine watches: it has no method that
/// presses, types, navigates, or sends, and fails no request for its
/// method. Closed when dropped, with its window.
pub struct WatchTab {
    pub(crate) page: std::sync::Arc<Page>,
    /// The window's browser, held so that its notice stays open.
    _browser: Browser,
}

impl std::fmt::Debug for WatchTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchTab")
            .field("tab", &self.page.serial)
            .finish_non_exhaustive()
    }
}

impl Drop for WatchTab {
    fn drop(&mut self) {
        self.page.release();
    }
}

impl WatchTab {
    /// The tab, watching its page's screens, sent once to `start`.
    pub(crate) async fn open(
        page: std::sync::Arc<Page>,
        browser: Browser,
        start: &WatchStart,
        deadline: Deadline,
    ) -> Result<Self, BrowserError> {
        let tab = Self {
            page,
            _browser: browser,
        };
        tokio::spawn(watch_screens(std::sync::Arc::downgrade(&tab.page)));
        tab.page.goto(start.address(), deadline).await?;
        Ok(tab)
    }

    /// What the tab ended since the last call, oldest first: a move of the
    /// tab to an origin it was not opened for (`NAVIGATION`), a request to
    /// one (`OUTSIDE`), a window its page opened (`POPUP`), a download
    /// (`DOWNLOAD`), and a socket outside its origins. No request is in it
    /// for its method.
    #[must_use]
    pub fn blocked(&self) -> Vec<RequestShape> {
        self.page.take_blocked()
    }

    /// The requests its page sent since the last call to one of the tab's
    /// origins, whatever their method, from a script, a beacon, or a form
    /// the person posted, and their answers, by structure alone: oldest
    /// first, once none is awaiting its answer, or when the deadline passes
    /// with what was answered by then.
    pub async fn exchanges(&self, deadline: Deadline) -> PageExchanges {
        self.page.take_exchanges(deadline).await
    }

    /// Records the page's requests. A watching tab already does from the
    /// moment it opens, so this changes nothing; it is the contract's
    /// `start`.
    pub(crate) fn record(&self) {
        self.page
            .recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// The snapshots of the screens the person came to since the last call.
    #[must_use]
    pub fn screens(&self) -> WatchedScreens {
        self.page
            .screens
            .lock()
            .map(|mut screens| WatchedScreens {
                snapshots: std::mem::take(&mut screens.taken),
                bound_hit: std::mem::take(&mut screens.bound_hit),
                missed: std::mem::take(&mut screens.missed),
            })
            .unwrap_or_default()
    }

    /// How many documents of the tab have loaded, how many times its
    /// address changed in its document, and how many things it ended,
    /// together, so far. [`Self::changed_since`] waits for it to grow.
    #[must_use]
    pub fn changes(&self) -> u64 {
        *self.page.changes.borrow()
    }

    /// Waits until the count [`Self::changes`] gave as `seen` grows: a
    /// protocol event, never a guess at how long a page takes.
    ///
    /// # Errors
    ///
    /// [`BrowserError::Timeout`] at the deadline; [`BrowserError::Crashed`]
    /// or [`BrowserError::Disconnected`] when the tab goes.
    pub async fn changed_since(&self, seen: u64, deadline: Deadline) -> Result<(), BrowserError> {
        self.page.changed_since(seen, deadline).await
    }

    /// How many addresses of its page the tab has looked at so far: one
    /// for each it took a snapshot of, missed, or came to past
    /// [`SCREENS_KEPT`]. [`Self::looked_at_since`] waits for it to grow.
    #[must_use]
    pub fn looked_at(&self) -> usize {
        *self.page.looked.borrow()
    }

    /// Waits until the count [`Self::looked_at`] gave as `seen` grows, or
    /// the deadline passes; `true` when it grew.
    pub async fn looked_at_since(&self, seen: usize, deadline: Deadline) -> bool {
        let mut looked = self.page.looked.subscribe();
        tokio::time::timeout_at(deadline.at(), looked.wait_for(|count| *count > seen))
            .await
            .is_ok_and(|grew| grew.is_ok())
    }

    /// Whether the tab is still open: the person has not closed it or its
    /// window, and the browser is there.
    #[must_use]
    pub fn is_open(&self) -> bool {
        !matches!(*self.page.status.borrow(), Status::Gone | Status::Crashed)
    }

    /// Waits until the person closes the tab or its window, or the browser
    /// goes, or the deadline passes; `true` when it closed.
    pub async fn closed(&self, deadline: Deadline) -> bool {
        let mut status = self.page.status.subscribe();
        tokio::time::timeout_at(
            deadline.at(),
            status.wait_for(|status| matches!(status, Status::Gone | Status::Crashed)),
        )
        .await
        .is_ok()
    }
}

/// Takes a snapshot of each screen the page of a watching tab comes to,
/// until the tab is gone.
async fn watch_screens(page: Weak<Page>) {
    let Some((mut changes, mut status)) = page
        .upgrade()
        .map(|page| (page.changes.subscribe(), page.status.subscribe()))
    else {
        return;
    };
    let mut last: Option<String> = None;
    loop {
        {
            let Some(now) = page.upgrade() else {
                return;
            };
            if matches!(*now.status.borrow(), Status::Gone | Status::Crashed) {
                return;
            }
            now.look_at_screen(&mut last).await;
        }
        tokio::select! {
            changed = changes.changed() => if changed.is_err() { return; },
            _ = status.wait_for(|status| matches!(status, Status::Gone | Status::Crashed)) => return,
        }
    }
}

impl Page {
    /// A request of a watching tab's page about to leave: recorded, by
    /// structure, when it is to one of the tab's origins and is a script's,
    /// a beacon, or a document's that carries a body.
    pub(super) fn heard_sent(&self, sent: &RequestSent) {
        let request = &sent.request;
        let kind = sent.resource_type.as_str();
        let recorded = RECORDED_KINDS.contains(&kind)
            || (kind == "Document" && !matches!(request.body.sent(0), Sent::Nothing));
        if !self.recording.load(std::sync::atomic::Ordering::SeqCst)
            || !recorded
            || !self.origins.allows_reported(&request.url)
        {
            return;
        }
        let Some(mut exchange) = self.exchange_of(request) else {
            return;
        };
        let (media_type, fields, withheld) = self.body_structure(&request.headers, &request.body);
        exchange.body_media_type = media_type;
        exchange.body_fields = fields;
        exchange.body_names_withheld = withheld;
        let loader = (!sent.loader_id.is_empty()).then(|| sent.loader_id.clone());
        self.note_pending(&sent.request_id, exchange, loader);
    }

    /// A new document of the tab's own frame has loaded under `loader`:
    /// what the documents before it sent and the browser will now say no
    /// more of is closed. A request whose answer's head had come is an
    /// exchange with no fields, since its body never came whole (a page
    /// that never reads an answer leaves it so, measured); one with no
    /// answer is forgotten. Without this a take would wait for them until
    /// its deadline.
    pub(super) fn left_behind(&self, loader: &str) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        let ended: Vec<String> = pending
            .iter()
            .filter(|(_, one)| one.loader.as_deref().is_some_and(|own| own != loader))
            .map(|(number, _)| number.clone())
            .collect();
        if ended.is_empty() {
            return;
        }
        for number in ended {
            let Some(one) = pending.remove(&number) else {
                continue;
            };
            if !one.answered {
                continue;
            }
            if let Ok(mut exchanges) = self.exchanges.lock() {
                if exchanges.len() < super::EXCHANGES_KEPT {
                    exchanges.push(one.exchange);
                } else {
                    self.dropped
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
        self.waiting.send_replace(pending.len());
    }

    /// A body's media type, and the names and kinds of its fields when it
    /// is JSON or a form given whole: no value is kept, and the bytes are
    /// dropped here.
    fn body_structure(
        &self,
        headers: &serde_json::Map<String, Json>,
        body: &HeldBody,
    ) -> (String, Option<BTreeMap<String, String>>, bool) {
        let bytes = match body.sent(LONGEST_WATCHED_BODY) {
            Sent::Nothing => return (String::new(), None, false),
            Sent::NotWhole => None,
            Sent::Whole(bytes) => Some(bytes),
        };
        let media_type = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .and_then(|(_, value)| value.as_str())
            .and_then(|value| value.split(';').next())
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let Some(bytes) = bytes else {
            return (media_type, None, false);
        };
        let named = |fields: BTreeMap<String, String>| -> BTreeMap<String, String> {
            fields
                .into_iter()
                .map(|(name, kind)| (self.redact.redact(&name), kind))
                .collect()
        };
        if media_type == "application/json" || media_type.ends_with("+json") {
            return match serde_json::from_slice::<Json>(&bytes) {
                Ok(json) => {
                    let (fields, withheld) = fields_of(&json);
                    (media_type, Some(named(fields)), withheld)
                }
                Err(_) => (media_type, None, false),
            };
        }
        if media_type == "application/x-www-form-urlencoded" {
            let Ok(text) = std::str::from_utf8(&bytes) else {
                return (media_type, None, false);
            };
            let (names, left_out) = crate::query_names(text);
            if !crate::names_are_structure(names.iter()) {
                return (media_type, Some(BTreeMap::new()), true);
            }
            let fields = names
                .into_iter()
                .map(|name| (name, "string".to_owned()))
                .collect();
            return (media_type, Some(named(fields)), left_out);
        }
        (media_type, None, false)
    }

    /// Takes a snapshot of the screen the page is on, when its address
    /// differs from the last one looked at and is on one of the tab's
    /// origins, once its requests have ended or [`SETTLE_BOUND`] passed.
    async fn look_at_screen(&self, last: &mut Option<String>) {
        let deadline = Deadline::after(self.call_deadline);
        let Ok((address, _)) = self.address_and_title(deadline).await else {
            return;
        };
        let at = without_query(&address).to_owned();
        if last.as_deref() == Some(at.as_str()) {
            return;
        }
        if !Address::new(&at).is_ok_and(|address| self.origins.allows(&address)) {
            return;
        }
        *last = Some(at);
        let full = self
            .screens
            .lock()
            .map(|mut screens| {
                let full = screens.count >= SCREENS_KEPT;
                screens.bound_hit |= full;
                full
            })
            .unwrap_or(true);
        if full {
            self.looked.send_modify(|count| *count += 1);
            return;
        }
        self.settle().await;
        let taken = self
            .snapshot(
                &SnapshotLimits::default(),
                Deadline::after(self.call_deadline),
            )
            .await;
        if let Ok(mut screens) = self.screens.lock() {
            match taken {
                Ok(mut snapshot) => {
                    for element in &mut snapshot.elements {
                        if element.value.as_deref() != Some(MASK) {
                            element.value = None;
                        }
                    }
                    screens.taken.push(snapshot);
                    screens.count += 1;
                }
                Err(_) => screens.missed += 1,
            }
        }
        self.looked.send_modify(|count| *count += 1);
    }

    /// Waits until the browser says the page's document is idle on the
    /// network and none of its recorded requests awaits its answer, or
    /// [`SETTLE_BOUND`] passes.
    async fn settle(&self) {
        let loaded = self.nav.borrow().loaded.clone();
        let mut nav = self.nav.subscribe();
        let mut waiting = self.waiting.subscribe();
        let _ = tokio::time::timeout(SETTLE_BOUND, async {
            let _ = nav
                .wait_for(|nav| loaded.is_none() || nav.idle == loaded)
                .await;
            let _ = waiting.wait_for(|count| *count == 0).await;
        })
        .await;
    }
}
