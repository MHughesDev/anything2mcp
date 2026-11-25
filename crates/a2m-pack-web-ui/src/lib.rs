//! The web pack's drivers: a target can be a web application, used through
//! its interface as a person uses it.
//!
//! The pack owns the `screens/` file kind for surfaces of the kind `web`,
//! where every locator lives, and compiles each screen into its own payload
//! in the compiled target. Its driver, of the mode `ui`, opens one acting tab
//! for each run in a browser the engine owns, and carries out `goto`, `wait`,
//! `read`, `set`, and `invoke` steps on it through `a2m-pack-web-browser`:
//! elements found by role and name, each wait a named condition, each control
//! set and read by its own rule, and a saving action not done until the page
//! shows its confirmation.
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
//! `site_tools` mode; `page_http` writes beyond `update` by PUT; a session
//! kept in page storage; the application's own sign-out; platform
//! profiles; the controls beyond text, select, check box, radio, and date;
//! reading a grid by a row's key; the verbs `open`, `search`, `delete`, `upload`, and `download`;
//! identity by a stamp or a search; setting a finding against existing
//! screen models, and drafting a field that is set or a type from a
//! snapshot.
//!
//! Its auth provider `form_login` signs in through the application's own
//! sign-in page with a username and password stored for the person, asks
//! the person for a one-time code when the page asks for one, and keeps
//! the cookies the browser then holds as the session's sealed state; the
//! `ui` driver places them into the session's browser, and stops a run as
//! `session_expired` when the page shows a screen the target declares as a
//! sign of expiry (decision 0245); `page_http` stops so too, when its page
//! is answered with that screen or its request with a status the target
//! declares (decision 0254). Its providers `window_login`, `sso_oidc`, and
//! `sso_saml` open a window of the engine's own browser in which the person
//! signs in themself, on the application's pages or through an identity
//! provider's, and keep the same state (decision 0254).
//!
//! Its drafting rules turn a snapshot the web pack's discoverers took into
//! a screen that reads one of the page's tables by its headings and a
//! `list` read over it (decision 0183), and a screen of the values the page
//! labels and a `get` read of them; a placeholder the discoverers matched
//! into the page's path is each read's key (decision 0187).
//!
//! A pack crate sees the kernel up to the provider contract and the base
//! of its own pack; the layering table in
//! `docs/reference/technical-reference.md` is the rule, and this crate's
//! design is `docs/architecture/crates/a2m-pack-web-ui.md`.

pub mod codes;
mod draft;
mod draft_request;
mod driver;
mod lower;
mod model;
mod page_http;
pub mod profiles;
mod requests;
mod screens;
mod sign_in;
mod window;

use std::sync::Arc;

use a2m_core::{ControlName, Failure, FindingKind, Mode, Value};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_pack_web_browser::BrowserSettings;
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder};

pub use a2m_pack_web_browser::SharedBrowsers;
pub use lower::{CALL_TOOL, GOTO, INVOKE, READ, SET, WAIT};
pub use page_http::{PAGE_HTTP_MODE, REQUEST};
pub use requests::REQUEST_KIND;
pub use screens::{CONTROLS, PACK, SCREEN_KIND, WEB_SURFACE};
pub use sign_in::FORM_LOGIN;

/// The name of the mode this pack drives.
pub const UI_MODE: &str = "ui";
/// The name of the mode that calls the tools a page registers.
pub const SITE_TOOLS_MODE: &str = "site_tools";

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
        .optional("refuse_quietly", JsonSchema::list(JsonSchema::text()))
        .optional("profile", JsonSchema::text())
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
        to.auth_provider(Arc::new(sign_in::FormLogin::new(browsers.clone())))?;
        for name in [window::WINDOW_LOGIN, window::SSO_OIDC, window::SSO_SAML] {
            to.auth_provider(Arc::new(window::WindowLogin::new(browsers.clone(), name)))?;
        }
        let page_http = Mode::new(PAGE_HTTP_MODE)?;
        to.driver(
            page_http.clone(),
            Arc::new(page_http::PageHttpDriver::new(browsers.clone())),
        )?;
        // One driver serves the two modes that run in a page of the
        // interface: they share its tabs, its screens, and what it holds
        // of the surfaces the edge refused.
        let interface = Arc::new(driver::UiDriver::new(
            browsers,
            setting("max_snapshot_elements").unwrap_or(400),
        ));
        to.driver(mode.clone(), interface.clone())?;
        let site_tools = Mode::new(SITE_TOOLS_MODE)?;
        to.driver(site_tools.clone(), interface)?;
        let mut compiler = CompilerRegistrations::new()
            .surface_kind(SurfaceKindSpec::new(
                WEB_SURFACE,
                ADDRESS_KEY,
                surface_fields(),
            ))
            .file_kind(Arc::new(screens::ScreenKind::new()))
            .file_kind(Arc::new(requests::RequestKind::new()))
            .lowering(Arc::new(lower::UiLowering::new(mode)))
            .lowering(Arc::new(page_http::PageHttpLowering::new(page_http)))
            .lowering(Arc::new(lower::SiteToolsLowering::new(site_tools)))
            .lint_rule(Arc::new(screens::CssNotLast))
            .lint_rule(Arc::new(screens::GeneratedId))
            .lint_rule(Arc::new(screens::PlatformProfileUnknown))
            .lint_rule(Arc::new(screens::PlatformProfileReadonly))
            .lint_rule(Arc::new(screens::CssUsed))
            .lint_rule(Arc::new(screens::NoFingerprint))
            .drafting_rule(Arc::new(draft::TableScreenRule {
                kind: FindingKind::new(draft::SCREEN_FINDING)?,
            }))
            .drafting_rule(Arc::new(draft::TableReadRule {
                kind: FindingKind::new(draft::SCREEN_FINDING)?,
            }))
            .drafting_rule(Arc::new(draft_request::RequestEntryRule {
                kind: FindingKind::new(draft_request::EXCHANGE_FINDING)?,
            }))
            .drafting_rule(Arc::new(draft_request::RequestReadRule {
                kind: FindingKind::new(draft_request::EXCHANGE_FINDING)?,
            }));
        for control in CONTROLS {
            compiler = compiler.control(ControlName::new(control)?);
        }
        to.compiler(compiler)
    }
}
