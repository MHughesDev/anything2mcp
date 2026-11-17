//! The web pack's discoverers: the capability probe, snapshots of
//! screens, capture of exchanges, and endpoint inference.
//!
//! The crate looks at a web application through a browser the engine owns
//! and returns findings. It registers discoverers and a describer only:
//! no driver, no mode, no file kind. It cannot write, and the way it is
//! built makes sure of that: it holds an observer of the base, which opens
//! tabs that look and nothing else; the browser fails every request of a
//! page that is not a read; and a link that looks like an action is not
//! followed.
//!
//! The product crate registers the pack with one call, [`add_to`], and
//! gives it the cell of browsers it also gives the web pack's drivers.
//!
//! Not built yet, and so not declared in the pack's manifest: `web_auth`
//! beyond naming a sign-in wall, `web_declared`, the watching tab and the
//! capture of a person's own use, scenario steps, dependencies between
//! requests, the front-end logic mark, and the platform profiles. Of the
//! requests a page's own scripts make, only those with GET to the
//! surface's origin are captured, by structure, as the observing tab
//! records them.
//!
//! A pack crate sees the kernel up to the provider contract and the base
//! of its own pack; the layering table in
//! `docs/reference/technical-reference.md` is the rule, and this crate's
//! design is `docs/architecture/crates/a2m-pack-web-probe.md`.

pub mod codes;
mod describe;
mod discover;
mod infer;
mod look;
mod screen;
mod shape;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use a2m_core::{Failure, Value};
use a2m_pack_web_browser::{BrowserSettings, SharedBrowsers};
use a2m_provider::{DiscovererName, Pack, PackContext, Registrar, RegistryBuilder};

pub use discover::{EMPTY_INTERFACE, INTERFACE_RULES, InterfaceRule, Marker};
pub use infer::{Answer, Endpoint, Exchange, Segment, Tier, infer_endpoints};
pub use look::{
    ACTION_WORDS, DISCOVERY_ACCOUNT, FOCUS_PATHS, FOCUS_READY_TEXT, FOCUS_TIER_TWO,
    looks_like_an_action,
};
pub use screen::{FIELD_ROLES, NAMED_ROLES, RECORD_ROLES};
pub use shape::{Shape, type_of_text};

/// The pack's name as a provider.
pub const PACK: &str = "web_probe";
/// The surface kind the discoverers look at. `a2m-pack-web-ui` registers
/// it.
pub const SURFACE_KIND: &str = "web";
/// The discoverer that names the kind of interface and how it stands.
pub const WEB_PROBE: &str = "web_probe";
/// The discoverer that takes snapshots of screens.
pub const WEB_SNAPSHOT: &str = "web_snapshot";
/// The discoverer that captures exchanges and infers endpoints.
pub const WEB_CAPTURE: &str = "web_capture";

/// The most pages one discovery looks at, unless a setting says
/// otherwise.
const DEFAULT_MAX_PAGES: u64 = 20;

/// The web pack's discoverers, as a pack.
#[derive(Debug, Clone)]
pub struct WebProbePack {
    shared: SharedBrowsers,
    manifest: &'static str,
    settings: BrowserSettings,
}

/// The pack, over the cell of browsers the product crate made. The
/// drivers of the web pack are given the same cell, so that one profile
/// has one browser.
#[must_use]
pub fn pack(shared: SharedBrowsers) -> WebProbePack {
    WebProbePack {
        shared,
        manifest: include_str!("anything2mcp-provider.toml"),
        settings: BrowserSettings::default(),
    }
}

/// Hands the pack to the registry builder. This is the one call the
/// product crate makes for it.
pub fn add_to(builder: &mut RegistryBuilder, shared: SharedBrowsers) {
    builder.add_pack(&pack(shared));
}

impl Pack for WebProbePack {
    fn manifest(&self) -> &'static str {
        self.manifest
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        // The cell gives the browsers of whichever pack registered first.
        // Only the handle that looks is kept.
        let observer = self
            .shared
            .get_or_create(cx, self.settings.clone().with_pack_settings(cx.settings())?)?
            .observer();
        let max_pages = cx
            .settings()
            .get("max_pages")
            .and_then(Value::as_int)
            .and_then(|number| u64::try_from(number).ok())
            .unwrap_or(DEFAULT_MAX_PAGES);
        let parts = Arc::new(discover::Parts {
            core: look::Core::new(
                observer,
                cx.provider().clone(),
                cx.telemetry().clock(),
                max_pages,
            ),
            kinds: discover::Kinds::new()?,
        });
        to.discoverer(
            Arc::new(discover::WebProbe(
                Arc::clone(&parts),
                DiscovererName::new(WEB_PROBE)?,
            )),
            Arc::new(describe::WebDescriber),
        )?;
        to.discoverer(
            Arc::new(discover::WebSnapshot(
                Arc::clone(&parts),
                DiscovererName::new(WEB_SNAPSHOT)?,
            )),
            Arc::new(describe::WebDescriber),
        )?;
        to.discoverer(
            Arc::new(discover::WebCapture(
                parts,
                DiscovererName::new(WEB_CAPTURE)?,
            )),
            Arc::new(describe::WebDescriber),
        )?;
        Ok(())
    }
}
