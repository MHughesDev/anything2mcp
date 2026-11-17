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
//! makes by itself with GET to its own origins ([`PageExchange`]).
//!
//! Not built yet: a visible window, exporting and importing a session's
//! state, the watching tab, frames, the stream of every network exchange,
//! declared pop-ups and downloads (an undeclared one is refused and
//! reported), uploads, recycling, and the platform profiles.

mod browsers;
mod client;
mod error;
mod launch;
mod origin;
mod page_read;
mod protocol;
mod snapshot;
mod tab;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(test)]
mod tests;

pub use a2m_provider::Deadline;

pub use browsers::{
    Address, BROWSER_KIND, Browser, BrowserHealth, BrowserSettings, Browsers, DIGESTS_SETTING,
    Observer, OpenBrowser, PROFILE_ROOT, ProfileKey, SharedBrowsers, Visibility,
};
pub use error::{BrowserError, Dispatch, LaunchReason, Reason, RequestShape, codes};
pub use launch::{FIXED_ARGUMENTS, KNOWN_PLACES, KnownPlace, known_places};
pub use origin::Origins;
pub use page_read::{MAX_SEGMENT_BYTES, PageAnswer, PagePath, plain_segment};
pub use snapshot::{ElementState, MASK, Snapshot, SnapshotElement, SnapshotLimits};
pub use tab::{
    ActTab, By, CREDENTIAL_HEADER_WORDS, DialogAnswer, Element, Input, Key, Loaded, Look,
    ObserveTab, PageEnvironment, PageExchange, PageRequest, PageResponse, Primitive, Query,
    RawValue, ReadWhat, SafeRequest, SafeRequests, TextMatch,
};
