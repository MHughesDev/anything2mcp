//! A sign-in the person carries out themself, in a window of the engine's
//! own browser (decision 0254): the auth providers `window_login`, through
//! the application's own sign-in page, and `sso_oidc` and `sso_saml`,
//! through an identity provider's pages, as a person meets single sign-on
//! in a browser.
//!
//! The session's browser is started with a window, always in the engine's
//! own data directory and in-memory context (rule 5). Its first tab says
//! which target and which person the window is for. The sign-in tab opens
//! at the sign-in screen's declared address, and may be on the surface's
//! own origin and, for single sign-on, on the identity provider's declared
//! origins, each within the destinations the target declares; a move to
//! any other origin is ended in the browser and ends the sign-in with a
//! named stop. The engine fills nothing and reads no field: it waits for
//! the declared sign that the person is signed in, a screen's `ready`,
//! which it looks for on the application's pages alone; on the identity
//! provider's pages it looks at nothing. Then it takes the cookies of the
//! application's origin as the session's state, exactly as `form_login`
//! does, and closes the window. A declared time limit ends a sign-in
//! nobody finished with a named stop.
//!
//! No stored credential is used, so a sign-in declared by drafts may use
//! this way: the person's own page showed every address first (decision
//! 0248), and the person types only into the application's or the
//! identity provider's own pages.

use std::collections::BTreeSet;
use std::time::Duration;

use a2m_auth::{
    AuthEntry, AuthProvider, BoxFuture, CredentialSink, ExpirySign, Material, PersonAnswer,
    PersonAsk, PersonReason, ProviderName, ProviderNeeds, RawSign, SignInAddress, SignInContext,
    SignedIn,
};
use a2m_core::{AccountName, Failure, FailureClass, Identity, Timestamp};
use a2m_pack_web_browser::{
    Address, BrowserError, Browsers, Deadline, LaunchReason, Look, Origins, PageEnvironment,
    ProfileKey, Visibility,
};

use crate::codes;
use crate::driver::origin_of;
use crate::model::{Cond, PAYLOAD_FORMAT, ScreenSection};
use crate::screens::{PACK, SCREEN_KIND, WEB_SURFACE};
use crate::sign_in::{fixed_path, inject_state, material_of, page_lost, profile_name};

/// The person signs in on the application's own sign-in page.
pub const WINDOW_LOGIN: &str = "window_login";
/// Single sign-on through an identity provider's pages, as OpenID Connect.
pub const SSO_OIDC: &str = "sso_oidc";
/// Single sign-on through an identity provider's pages, as SAML.
pub const SSO_SAML: &str = "sso_saml";

const SCREEN: &str = "screen";
const SIGNED_IN: &str = "signed_in";
const WAIT_MS: &str = "wait_ms";
const IDENTITY_PROVIDERS: &str = "identity_providers";

/// How long the person has, when the entry does not say.
const PERSON_TIME: Duration = Duration::from_secs(300);

fn invalid(provider: &str, why: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        a2m_auth::codes::AUTH_SETTINGS_INVALID,
        format!("The auth entry for `{provider}` cannot be used: {why}."),
        "Name the sign-in screen and the screen that shows the person is signed in under `settings`, as the web pack's design says.",
    )
}

fn not_now(why: &str) -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        a2m_auth::codes::NEEDS_LOGIN,
        format!("The sign-in window was not finished: {why}."),
        "Sign in with `anything2mcp session login` while you are at this machine.",
    )
}

/// The sign-in an entry declares, resolved against the compiled screens.
struct Window {
    origin: Address,
    page: Address,
    signed_in: Cond,
    providers: Vec<Address>,
    wait: Duration,
}

/// The identity provider's origins an entry declares, each an origin with
/// no path, written once.
fn providers_of(provider: &str, entry: &AuthEntry) -> Result<Vec<Address>, Failure> {
    let listed = entry.optional(IDENTITY_PROVIDERS);
    let sso = provider != WINDOW_LOGIN;
    match (sso, listed) {
        (false, None) => return Ok(Vec::new()),
        (false, Some(_)) => {
            return Err(invalid(
                provider,
                "it names identity providers; a sign-in through them is `sso_oidc` or `sso_saml`",
            ));
        }
        (true, None) => {
            return Err(invalid(
                provider,
                "it names no identity provider under `identity_providers`",
            ));
        }
        (true, Some(_)) => {}
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for written in listed.unwrap_or_default().split_whitespace() {
        let address = Address::new(written).map_err(|_| {
            invalid(
                provider,
                "an identity provider is not written as an `http` or `https` origin",
            )
        })?;
        if address.origin() != written.trim_end_matches('/') {
            return Err(invalid(
                provider,
                "an identity provider is written with a path; write its origin alone",
            ));
        }
        if !seen.insert(address.origin()) {
            return Err(invalid(provider, "an identity provider is written twice"));
        }
        out.push(address);
    }
    if out.is_empty() {
        return Err(invalid(
            provider,
            "it names no identity provider under `identity_providers`",
        ));
    }
    Ok(out)
}

impl Window {
    fn of(provider: &str, entry: &AuthEntry) -> Result<Self, Failure> {
        let mut webs = entry
            .surfaces
            .surfaces
            .iter()
            .filter(|surface| surface.kind == WEB_SURFACE);
        let surface = webs.next().ok_or_else(|| {
            invalid(
                provider,
                "it covers no surface of the kind `web`, or the engine gave it nothing of its surfaces",
            )
        })?;
        if webs.next().is_some() {
            return Err(invalid(
                provider,
                "it covers more than one surface of the kind `web`; a sign-in window is one application's",
            ));
        }
        let section = entry
            .surfaces
            .sections
            .iter()
            .find(|section| {
                section.pack == PACK
                    && section.kind == SCREEN_KIND
                    && section.surface_kind == WEB_SURFACE
            })
            .ok_or_else(|| invalid(provider, "the target has no screens"))?;
        if section.format != PAYLOAD_FORMAT {
            return Err(invalid(
                provider,
                "its screens were compiled by another version of the web pack",
            ));
        }
        let screens: ScreenSection = section
            .payload
            .decode()
            .map_err(|_| invalid(provider, "its screens do not decode"))?;
        let screen_named = |setting: &str| -> Result<_, Failure> {
            let name = entry.required(setting)?;
            let screen = screens.get(name).ok_or_else(|| {
                invalid(
                    provider,
                    &format!("no screen is named `{}`", codes::shown(name)),
                )
            })?;
            if screen.surface != surface.name.as_str() {
                return Err(invalid(provider, "a screen it names is on another surface"));
            }
            Ok(screen)
        };
        let screen = screen_named(SCREEN)?;
        let signed_in = screen_named(SIGNED_IN)?.ready.clone();
        let origin_text = origin_of(&surface.address)?;
        let origin = Address::new(&origin_text)?;
        let page = Address::new(&format!("{origin_text}{}", fixed_path(screen)?))?;
        let providers = providers_of(provider, entry)?;
        if providers
            .iter()
            .any(|address| address.origin() == origin.origin())
        {
            return Err(invalid(
                provider,
                "an identity provider is the application's own origin",
            ));
        }
        let wait = match entry.optional(WAIT_MS) {
            None => PERSON_TIME,
            Some(text) => text
                .parse::<u64>()
                .ok()
                .filter(|ms| (1_000..=1_800_000).contains(ms))
                .map(Duration::from_millis)
                .ok_or_else(|| {
                    invalid(
                        provider,
                        "`wait_ms` is not a whole number from 1000 to 1800000",
                    )
                })?,
        };
        Ok(Self {
            origin,
            page,
            signed_in,
            providers,
            wait,
        })
    }
}

/// The auth providers through which a person signs in in a window.
pub(crate) struct WindowLogin {
    browsers: Browsers,
    name: &'static str,
}

impl WindowLogin {
    pub(crate) fn new(browsers: Browsers, name: &'static str) -> Self {
        Self { browsers, name }
    }

    async fn sign_in_now(&self, cx: &SignInContext<'_>) -> Result<Material, Failure> {
        let entry = cx.entry();
        let window = Window::of(self.name, entry)?;
        // Every address the person is sent to is one the target declares,
        // checked again just before the window opens (decision 0248).
        cx.check_address("the sign-in screen", window.page.as_str())?;
        for provider in &window.providers {
            cx.check_address("the setting `identity_providers`", provider.as_str())?;
        }
        // The window is for the person: with nobody there, it is not
        // opened.
        match cx
            .ask_person(PersonAsk::Notify {
                reason: PersonReason::SignIn,
            })
            .await
        {
            PersonAnswer::Done(_) => {}
            PersonAnswer::Declined => return Err(not_now("the person declined")),
            PersonAnswer::NoAnswer | PersonAnswer::Unreachable => {
                return Err(not_now("nobody is there to sign in"));
            }
        }
        let key = &cx.session().key;
        let name = profile_name(cx.session().session);
        let profile = ProfileKey {
            target: key.target.clone(),
            environment: key.environment.clone(),
            identity: Identity::Account(AccountName::new(&name)?),
        };
        let notice = format!(
            "Sign in to {} ({}) for {}",
            key.target, key.environment, key.identity
        );
        let browser = match self
            .browsers
            .open(&profile, Visibility::Window { notice })
            .await
        {
            Ok(browser) => browser,
            Err(BrowserError::LaunchFailed(LaunchReason::NoScreen)) => {
                return Err(not_now("this machine has no screen to show the window on"));
            }
            Err(error) => return Err(page_lost(&error, "the start")),
        };
        let outcome = self.carried_out(&browser, &window, cx, &name).await;
        // The window goes, whatever came of it: a run never uses it.
        drop(browser);
        self.browsers.close(&profile).await;
        outcome
    }

    async fn carried_out(
        &self,
        browser: &a2m_pack_web_browser::Browser,
        window: &Window,
        cx: &SignInContext<'_>,
        name: &str,
    ) -> Result<Material, Failure> {
        let own = Origins::of([&window.origin]);
        let passing = Origins::of(window.providers.iter());
        let tab = browser
            .sign_in_tab(&PageEnvironment::default(), &own, &passing)
            .await
            .map_err(|error| page_lost(&error, "the opening of a tab"))?;
        let deadline = Deadline::after(window.wait);
        let limit = || {
            not_now("the time the target gives the person passed before the sign of success showed")
        };
        match tab.goto(&window.page, deadline).await {
            Ok(_) | Err(BrowserError::ForeignOrigin(_)) => {}
            Err(BrowserError::Timeout { .. }) => return Err(limit()),
            Err(error) => return Err(page_lost(&error, "the move to the sign-in page")),
        }
        let signed_in = window.signed_in.primitive();
        loop {
            let seen = tab.changes();
            // What the tab refused: a move to an origin nobody declared,
            // a window, a download. Each ends the sign-in.
            if let Some(refused) = tab.blocked().first() {
                return Err(if refused.method == "NAVIGATION" {
                    Failure::new(
                        FailureClass::Policy,
                        codes::SURFACE_NOT_DECLARED,
                        "The sign-in window was sent to an origin the target does not declare. The browser ended the move, and the sign-in with it.",
                        "Declare the identity provider's origin under `identity_providers` and as a surface of the target, if it is one, and sign in again.",
                    )
                } else {
                    Failure::new(
                        FailureClass::UnknownVariant,
                        codes::UNEXPECTED_PAGE,
                        "A page of the sign-in opened a window or began a download, which the browser refused. The sign-in is ended.",
                        "Look at the sign-in in a browser of your own.",
                    )
                });
            }
            match tab.wait_unless_changed(&signed_in, seen, deadline).await {
                Ok(true) => break,
                Ok(false) => {}
                Err(BrowserError::Timeout { .. }) => return Err(limit()),
                Err(error) => return Err(page_lost(&error, "a wait")),
            }
        }
        // Only the application's origin's cookies are the session.
        let state = browser
            .export_state(&own, Deadline::after(Duration::from_secs(30)))
            .await
            .map_err(|error| page_lost(&error, "the reading of the session's state"))?;
        drop(tab);
        material_of(self.name(), name, state, cx)
    }
}

impl AuthProvider for WindowLogin {
    fn name(&self) -> ProviderName {
        ProviderName::fixed(self.name)
    }

    fn needs(&self) -> ProviderNeeds {
        ProviderNeeds {
            // The person signs in, every time; a renewal waits for them.
            person: true,
            silent_renewal: false,
            light_prompt: false,
            movable: true,
            confirms: true,
        }
    }

    fn check(&self, entry: &AuthEntry) -> Result<(), Failure> {
        entry.only(&[SCREEN, SIGNED_IN, WAIT_MS, IDENTITY_PROVIDERS], &[])?;
        if !entry.credentials.is_empty() {
            return Err(invalid(
                self.name,
                "it names a stored credential; the person types their own, in the window",
            ));
        }
        providers_of(self.name, entry)?;
        Window::of(self.name, entry).map(|_| ())
    }

    fn destinations(&self, entry: &AuthEntry) -> Vec<SignInAddress> {
        providers_of(self.name, entry)
            .unwrap_or_default()
            .into_iter()
            .map(|address| SignInAddress {
                what: "the setting `identity_providers`".to_owned(),
                address: address.as_str().to_owned(),
            })
            .collect()
    }

    fn sign_in<'a>(
        &'a self,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(self.sign_in_now(cx))
    }

    fn is_signed_in<'a>(
        &'a self,
        material: &'a Material,
        _now: Timestamp,
    ) -> BoxFuture<'a, SignedIn> {
        Box::pin(async move {
            SignedIn::Yes {
                until: material.expires_at(),
            }
        })
    }

    fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign> {
        match sign {
            RawSign::Status { code: 401, .. } => Some(ExpirySign::Refused),
            RawSign::Redirect { .. } => Some(ExpirySign::SignInRequested),
            RawSign::PastExpiry => Some(ExpirySign::PastExpiry),
            RawSign::Status { .. } => None,
        }
    }

    fn refresh<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(async {
            Err(not_now(
                "a sign-in in a window is renewed only by the person signing in again",
            ))
        })
    }

    fn sign_out<'a>(
        &'a self,
        material: &'a Material,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            if let Some(profile) = material.profile()
                && let Ok(account) = AccountName::new(profile.as_str())
            {
                let key = ProfileKey {
                    target: cx.session().key.target.clone(),
                    environment: cx.session().key.environment.clone(),
                    identity: Identity::Account(account),
                };
                self.browsers.close(&key).await;
            }
            Ok(())
        })
    }

    fn inject(&self, material: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure> {
        inject_state(material, sink)
    }
}
