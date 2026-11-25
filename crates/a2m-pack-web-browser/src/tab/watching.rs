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

use super::{Page, PageExchanges, Status, fields_of, without_fragment, without_query};
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

/// How many documents' addresses a watching tab keeps, its frames' among
/// them: the newest.
const ADDRESSES_KEPT: usize = 64;

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

/// A document of a watching tab's own frame, by the order the documents
/// were parsed in: what [`WatchTab::exchanges_through`] takes the requests
/// up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocumentMark(pub(crate) usize);

/// One screen a watching tab looked at, as [`WatchTab::next_screen`] gives
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedScreen {
    /// The screen's address, without its query.
    pub address: String,
    /// The document of the tab's frame the screen was in.
    pub document: DocumentMark,
    /// Its snapshot, by structure, as [`WatchedScreens::snapshots`] keeps
    /// one; `None` when it was not taken.
    pub snapshot: Option<Snapshot>,
    /// Whether no snapshot was taken because the tab had already taken
    /// [`SCREENS_KEPT`].
    pub past_bound: bool,
}

/// What a watching tab holds of the screens of its page: each it looked
/// at, oldest first, until it is handed out.
#[derive(Debug, Default)]
pub(crate) struct Screens {
    looked: std::collections::VecDeque<WatchedScreen>,
    count: usize,
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
        self.page.take_exchanges(deadline, None).await
    }

    /// Records the page's requests. A watching tab already does from the
    /// moment it opens, so this changes nothing; it is the contract's
    /// `start`.
    pub(crate) fn record(&self) {
        self.page
            .recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// The snapshots of the screens the person came to since the last call,
    /// and of [`Self::next_screen`].
    #[must_use]
    pub fn screens(&self) -> WatchedScreens {
        let mut out = WatchedScreens::default();
        while let Some(screen) = self.next_screen() {
            match screen.snapshot {
                Some(snapshot) => out.snapshots.push(snapshot),
                None if screen.past_bound => out.bound_hit = true,
                None => out.missed += 1,
            }
        }
        out
    }

    /// The oldest screen the person came to that has not been handed out,
    /// with the document it was in, whether its snapshot was taken or not:
    /// one is given for each address the tab looked at.
    #[must_use]
    pub fn next_screen(&self) -> Option<WatchedScreen> {
        self.page
            .screens
            .lock()
            .ok()
            .and_then(|mut screens| screens.looked.pop_front())
    }

    /// As [`Self::exchanges`], but only the requests sent under the
    /// documents of the tab's frame up to `through`, which a screen of
    /// [`Self::next_screen`] names, and only those are waited for: a
    /// request of a document the person came to after it is kept for the
    /// next take. Every request is taken with `None`.
    pub async fn exchanges_through(
        &self,
        through: Option<DocumentMark>,
        deadline: Deadline,
    ) -> PageExchanges {
        self.page.take_exchanges(deadline, through).await
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
        if kind == "Document" {
            self.document_at(&sent.loader_id, &request.url);
        }
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
    /// its deadline. One whose answer came whole and is being read is left
    /// to the read, which records it once, with its fields.
    pub(super) fn left_behind(&self, loader: &str) {
        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        let ended: Vec<String> = pending
            .iter()
            .filter(|(_, one)| {
                !one.reading && one.loader.as_deref().is_some_and(|own| own != loader)
            })
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
                    exchanges.push((one.loader, one.exchange));
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

    /// Takes a snapshot of the screen the page is on, when its document or
    /// its address differs from the last one looked at and the address is
    /// on one of the tab's origins, once its requests have ended or
    /// [`SETTLE_BOUND`] passed.
    ///
    /// Each screen is looked at in one document of the tab's frame, the
    /// last parsed, at the address the browser said that document is at,
    /// and is handed out with it whatever comes of its snapshot: when the
    /// person moves on to another document before the screen's requests
    /// end, or while the snapshot is taken, the snapshot is not kept, since
    /// it may hold the next document's screen, and the screen is handed out
    /// as missed, so that its requests are still put to it.
    async fn look_at_screen(&self, last: &mut Option<String>) {
        // A document the browser named no address for, such as the empty
        // one a tab opens on, is no screen of the surface's.
        let loader = self.last_parsed();
        let Some(address) = self.address_of(loader.as_deref()) else {
            return;
        };
        let at = without_query(&address).to_owned();
        // One screen for each address of each document: a document loaded
        // again at the same address is another visit.
        let seen = format!("{} {at}", loader.as_deref().unwrap_or_default());
        if last.as_deref() == Some(seen.as_str()) {
            return;
        }
        if !Address::new(&at).is_ok_and(|address| self.origins.allows(&address)) {
            return;
        }
        *last = Some(seen);
        let document = self.document_mark(loader.as_deref());
        let full = self
            .screens
            .lock()
            .map(|screens| screens.count >= SCREENS_KEPT)
            .unwrap_or(true);
        let mut screen = WatchedScreen {
            address: at,
            document,
            snapshot: None,
            past_bound: full,
        };
        if !full && self.settle(loader.as_deref()).await {
            let taken = self
                .snapshot(
                    &SnapshotLimits::default(),
                    Deadline::after(self.call_deadline),
                )
                .await;
            if let Ok(mut snapshot) = taken
                && self.last_parsed() == loader
                && without_query(&snapshot.address) == screen.address
            {
                for element in &mut snapshot.elements {
                    if element.value.as_deref() != Some(MASK) {
                        element.value = None;
                    }
                }
                screen.snapshot = Some(snapshot);
            }
        }
        if let Ok(mut screens) = self.screens.lock() {
            if screen.snapshot.is_some() {
                screens.count += 1;
            }
            screens.looked.push_back(screen);
        }
        self.looked.send_modify(|count| *count += 1);
    }

    /// The loader of the last document of the tab's own frame parsed.
    fn last_parsed(&self) -> Option<String> {
        self.documents
            .lock()
            .ok()
            .and_then(|documents| documents.last().cloned())
    }

    /// The address the browser said the document under `loader` is at,
    /// with every known secret replaced.
    fn address_of(&self, loader: Option<&str>) -> Option<String> {
        let loader = loader?;
        let addresses = self.addresses.lock().ok()?;
        let (_, address) = addresses.iter().rev().find(|(own, _)| own == loader)?;
        Some(self.redact.redact(address))
    }

    /// The browser says the document under `loader` is at `address`: its
    /// request was sent, or redirected, or its frame's address changed
    /// within it.
    pub(super) fn document_at(&self, loader: &str, address: &str) {
        if loader.is_empty() {
            return;
        }
        if let Ok(mut addresses) = self.addresses.lock() {
            addresses.retain(|(own, _)| own != loader);
            if addresses.len() >= ADDRESSES_KEPT {
                addresses.pop_front();
            }
            addresses.push_back((loader.to_owned(), without_fragment(address).to_owned()));
        }
    }

    /// The frame's address changed within its document: the last parsed
    /// is at `address` now.
    pub(super) fn moved_within(&self, address: &str) {
        if let Some(loader) = self.last_parsed() {
            self.document_at(&loader, address);
        }
    }

    /// The mark of the document of the tab's frame under `loader`: the
    /// last parsed when the browser named none, or it is not yet known.
    fn document_mark(&self, loader: Option<&str>) -> DocumentMark {
        let documents = self
            .documents
            .lock()
            .map(|known| known.clone())
            .unwrap_or_default();
        let last = documents.len().saturating_sub(1);
        DocumentMark(
            loader
                .and_then(|loader| documents.iter().position(|known| known == loader))
                .unwrap_or(last),
        )
    }

    /// A document of the tab's own frame has been parsed under `loader`.
    pub(super) fn parsed(&self, loader: &str) {
        if let Ok(mut documents) = self.documents.lock()
            && !documents.iter().any(|known| known == loader)
        {
            documents.push(loader.to_owned());
        }
    }

    /// Waits until the browser says the page's document, the one under
    /// `loader`, is idle on the network and none of its recorded requests
    /// awaits its answer, or [`SETTLE_BOUND`] passes; `false`, at once,
    /// when another document of the frame is parsed first, as when a
    /// person leaves a page before it is quiet.
    async fn settle(&self, loader: Option<&str>) -> bool {
        let loader = loader.map(str::to_owned);
        let mut nav = self.nav.subscribe();
        let mut waiting = self.waiting.subscribe();
        let mut changes = self.changes.subscribe();
        let quiet = async {
            let _ = nav
                .wait_for(|nav| loader.is_none() || nav.idle == loader)
                .await;
            let _ = waiting.wait_for(|count| *count == 0).await;
        };
        // A document is parsed before the count of changes grows.
        let left = async {
            loop {
                if self.last_parsed() != loader || changes.changed().await.is_err() {
                    return;
                }
            }
        };
        tokio::select! {
            () = quiet => {}
            () = left => return false,
            () = tokio::time::sleep(SETTLE_BOUND) => {}
        }
        self.last_parsed() == loader
    }
}
