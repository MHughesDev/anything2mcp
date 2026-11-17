//! The web pack's drivers: a target can be a web application, used through
//! its interface as a person uses it.
//!
//! The pack owns the `screens/` file kind for surfaces of the kind `web`,
//! where every locator lives, and compiles each screen into its own
//! payload in the plan. Its driver, of the mode `ui`, opens one acting tab
//! for each run in a browser the engine owns, and carries out `goto`,
//! `wait`, `read`, `set`, and `invoke` steps on it through
//! `a2m-pack-web-browser`: elements found by role and name, each wait a
//! named condition, each control set and read by its own rule, and a
//! saving action not done until the page shows its confirmation.
//!
//! The product crate registers the pack with one call, [`add_to`], and
//! gives it the cell of browsers it also gives the web pack's discoverers,
//! so that the two share one set of browsers:
//!
//! ```text
//! let browsers = a2m_pack_web_ui::SharedBrowsers::default();
//! a2m_pack_web_ui::add_to(&mut builder, browsers.clone());
//! ```
//!
//! Not built yet, and so not declared in the pack's manifest: the
//! `page_http` and `site_tools` modes; the `form_login`, `sso_oidc`, and
//! `sso_saml` auth providers and the sign-in window; signs of expiry;
//! frames; platform profiles; the controls beyond text, select, and check
//! box; the verbs `open`, `search`, `delete`, `upload`, and `download`;
//! identity by a stamp or a search; screen matching and the drafting
//! rules.
//!
//! A pack crate sees the kernel up to the provider contract and the base
//! of its own pack; the layering table in
//! `docs/reference/technical-reference.md` is the rule, and this crate's
//! design is `docs/architecture/crates/a2m-pack-web-ui.md`.

pub mod codes;
mod driver;
mod lower;
mod model;
mod screens;

use std::sync::Arc;

use a2m_core::{ControlName, Failure, Mode, Value};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_pack_web_browser::BrowserSettings;
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder};

pub use a2m_pack_web_browser::SharedBrowsers;
pub use lower::{GOTO, INVOKE, READ, SET, WAIT};
pub use screens::{CONTROLS, PACK, SCREEN_KIND, WEB_SURFACE};

/// The name of the mode this pack drives.
pub const UI_MODE: &str = "ui";

/// The key of a surface of the kind `web` that holds its address.
pub const ADDRESS_KEY: &str = "origin";

/// The web pack's drivers. Made by [`pack`].
#[derive(Debug, Clone)]
pub struct WebUiPack {
    browsers: SharedBrowsers,
}

/// The pack, over the cell of browsers the product crate also hands the
/// web pack's discoverers. The first of the two to register builds the
/// browsers; the other receives the same.
#[must_use]
pub fn pack(browsers: SharedBrowsers) -> WebUiPack {
    WebUiPack { browsers }
}

/// Hands the pack to the registry builder. This is the one call the
/// product crate makes for it. The host's roots must hold a folder named
/// `profiles` that may be written: the browsers' data directories go there.
pub fn add_to(builder: &mut RegistryBuilder, browsers: SharedBrowsers) {
    builder.add_pack(&pack(browsers));
}

/// The fields a surface of the kind `web` has.
fn surface_fields() -> JsonSchema {
    JsonSchema::object()
        .required(ADDRESS_KEY, JsonSchema::text())
        .optional(
            "viewport",
            JsonSchema::object()
                .required("width", JsonSchema::int_between(200, 8000))
                .required("height", JsonSchema::int_between(200, 8000)),
        )
        .optional("color_scheme", JsonSchema::words(&["light", "dark"]))
        .optional("reduced_motion", JsonSchema::bool())
}

impl Pack for WebUiPack {
    fn manifest(&self) -> &'static str {
        include_str!("anything2mcp-provider.toml")
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let mode = Mode::new(UI_MODE)?;
        let setting = |key: &str| {
            cx.settings()
                .get(key)
                .and_then(Value::as_int)
                .and_then(|number| usize::try_from(number).ok())
        };
        // The reviewed digests of the browser programs, from this pack's
        // own settings; a value that cannot be read stops the pack.
        let mut settings = BrowserSettings::default().with_pack_settings(cx.settings())?;
        if let Some(most) = setting("max_read_chars") {
            settings.max_read_chars = most;
        }
        let browsers = self.browsers.get_or_create(cx, settings)?;
        to.driver(
            mode.clone(),
            Arc::new(driver::UiDriver::new(
                browsers,
                setting("max_snapshot_elements").unwrap_or(400),
            )),
        )?;
        let mut compiler = CompilerRegistrations::new()
            .surface_kind(SurfaceKindSpec::new(
                WEB_SURFACE,
                ADDRESS_KEY,
                surface_fields(),
            ))
            .file_kind(Arc::new(screens::ScreenKind::new()))
            .lowering(Arc::new(lower::UiLowering::new(mode)))
            .lint_rule(Arc::new(screens::CssNotLast))
            .lint_rule(Arc::new(screens::CssUsed))
            .lint_rule(Arc::new(screens::NoFingerprint));
        for control in CONTROLS {
            compiler = compiler.control(ControlName::new(control)?);
        }
        to.compiler(compiler)
    }
}
