//! The web pack's base: a browser engine that speaks the DevTools protocol
//! to Chrome, Edge, or Chromium over the debugging pipe, with a client of
//! its own (decision 0070).
//!
//! It starts browsers through the scoped supervisor with a data directory
//! the engine owns ([`Browsers`]), opens tabs in one of two postures
//! ([`ActTab`], which may act, and [`ObserveTab`], which cannot), locates
//! elements by meaning, reads them, waits on named conditions, takes
//! snapshots from the accessibility tree, and says for every act whether
//! its input reached the page ([`Dispatch`]). It is not a driver and it
//! registers nothing: `a2m-pack-web-ui` and `a2m-pack-web-probe` are built
//! on it.
//!
//! No type of the protocol client is public, so that the base can change
//! without touching the crates above. The design is
//! `docs/architecture/crates/a2m-pack-web-browser.md`.
//!
//! An observing tab records, by structure alone, the requests its page
//! makes by itself with GET to its own origins ([`PageExchange`]), and reads
//! from inside its page with GET alone ([`ObserveTab::read_from_page`]); an
//! acting tab writes from inside its page with the page's own anti-forgery
//! token, which never leaves the page ([`ActTab::write_from_page`]).
//!
//! Every tab is opened in a browser context of the browser's own making,
//! which keeps cookies and page storage in memory, so that nothing a page
//! stores is written to the data directory; a session's cookies are taken
//! out with [`Browser::export_state`] and placed back with
//! [`Browser::import_state`] (decision 0245).
//!
//! A browser may show a window ([`Visibility::Window`]) for a sign-in a
//! person carries out themself, in a tab held to the origins given that
//! hears no network event ([`Browser::sign_in_tab`]); an acting tab's
//! network events carry no body of a request its page's script makes
//! (decision 0254). A window also holds a tab a person uses while the
//! engine watches ([`Observer::watching_tab`], [`WatchTab`]): it fails no
//! request for its method, records each by structure with its body's names
//! and kinds, and takes a snapshot of each screen with no value kept
//! (decision 0392, point 1).
//!
//! Not built yet: page storage in a session's state, frames, the stream of every network exchange,
//! declared pop-ups and downloads (an undeclared one is refused and
//! reported), uploads, recycling, and the platform profiles.

mod browsers;
mod capture;
mod client;
mod error;
mod launch;
mod origin;
mod page_read;
mod page_write;
mod protocol;
mod safe;
mod snapshot;
mod structure;
mod tab;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(test)]
mod tests;

pub use a2m_provider::Deadline;

pub use browsers::{
    Address, BROWSER_KIND, Browser, BrowserHealth, BrowserSettings, BrowserState, Browsers,
    DIGESTS_SETTING, DOWNLOADS_FOLDER, HANG_PROBE, HANGS_BEFORE_RECYCLE, Observer, OpenBrowser,
    PROFILE_ROOT, ProfileKey, STAGING_FOLDER, SharedBrowsers, StateCookie, Visibility,
};
pub use capture::captured;
pub use error::{BrowserError, Dispatch, LaunchReason, Reason, RequestShape, codes};
pub use launch::{FIXED_ARGUMENTS, KNOWN_PLACES, KnownPlace, known_places};
pub use origin::Origins;
pub use page_read::{MAX_SEGMENT_BYTES, PageAnswer, PagePath, plain_segment};
pub use page_write::{
    AntiForgery, BodyEncoding, FieldValue, LONGEST_FIELD_TEXT, LONGEST_WRITE_NAME, MOST_FIELDS,
    PageWrite, REFUSED_HEADERS, TokenFrom, TokenProblem, TokenTo, WriteMethod, plain_write_name,
};
pub use safe::{LONGEST_SAFE_BODY, SafeBody, SafeRequest, SafeRequests};
pub use snapshot::{ElementState, MASK, Snapshot, SnapshotElement, SnapshotLimits};
pub use structure::{
    ACTION_WORDS, LONGEST_NAME, LONGEST_NAME_PART, LONGEST_SEGMENT, LONGEST_SEGMENT_PART,
    MOST_NAMES, PLACEHOLDER, is_plain_name, is_plain_segment_word, looks_like_an_action,
    names_are_structure, path_by_structure, query_names,
};
pub use tab::{
    ActTab, BROWSER_DEFAULT_HEADERS, BROWSER_HEADERS, By, CREDENTIAL_HEADER_WORDS, DialogAnswer,
    Download, DownloadError, EXCHANGES_KEPT, Element, Input, Key, LONGEST_WATCHED_BODY, Loaded,
    Look, OUTSIDE, ObserveTab, OpenDialog, PageEnvironment, PageExchange, PageExchanges,
    PageRequest, PageResponse, Primitive, Query, RawValue, ReadWhat, SCREENS_KEPT, SETTLE_BOUND,
    SiteAnswer, SiteTool, TextMatch, WatchStart, WatchTab, WatchedScreens, digest_of,
};
