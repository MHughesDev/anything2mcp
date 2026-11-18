//! The auth provider `form_login`: a sign-in through the application's own
//! sign-in page, in a browser the engine owns (decision 0245).
//!
//! The person stores the username and the password once, on the engine's
//! own page or with `secret.set`; they are sealed in the vault for that
//! person. When the session keeper signs in, this provider opens the
//! sign-in page the target declares in an acting tab of the session's own
//! browser, fills the two fields the screen model locates, presses the
//! declared action, and waits for one of the declared signs: the action's
//! confirmation (signed in), a variant that shows a refused credential, a
//! variant that shows a block, or the screen that asks for a one-time code.
//! A page that shows none of them is a stop, never a guess. A refused
//! credential is tried once and never again; a block is a named stop; a
//! code is asked of the person each time, and is never stored.
//!
//! What the browser holds for the application's origin afterwards, its
//! cookies, is the session's state. Each value is registered with the
//! redactor and kept as a secret part of the session's material, which the
//! keeper seals for the person. A run's driver gets it back only through
//! the lease, and places it into the browser's in-memory context.
//!
//! Not built: the sign-in in a visible window that the person carries out
//! themself, single sign-on through an identity provider's pages, the
//! application's own sign-out, and a session kept in page storage.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use a2m_auth::{
    AuthEntry, AuthProvider, BoxFuture, CredentialSink, ExpirySign, Material, PersonAnswer,
    PersonAsk, PersonReason, ProfileRef, ProviderName, ProviderNeeds, RawSign, SignInContext,
    SignedIn,
};
use a2m_core::{
    AccountName, Digest, Failure, FailureClass, Identity, Secret, SessionId, Timestamp,
};
use a2m_pack_web_browser::{
    ActTab, Address, BrowserError, BrowserState, Browsers, Deadline, Input, Look, Origins,
    PageEnvironment, Primitive, ProfileKey, StateCookie, Visibility,
};
use serde::{Deserialize, Serialize};

use crate::codes;
use crate::driver::origin_of;
use crate::model::{Action, Cond, Field, PAYLOAD_FORMAT, Screen, ScreenSection, Segment, Variant};
use crate::screens::{PACK, SCREEN_KIND, WEB_SURFACE};

/// The provider's name, as `auth.yaml` writes it.
pub const FORM_LOGIN: &str = "form_login";

/// The name of the one value this provider places into a driver's client:
/// the session's browser state, which the web pack's own drivers take and
/// no other client is given.
pub(crate) const BROWSER_STATE: &str = "a2m-browser-state";

/// The credential roles: the username and the password, both stored.
const USER: &str = "user";
const PASSWORD: &str = "password";

/// The settings `auth.yaml` gives the provider. Each names a part of a
/// screen model; no locator is written here.
const SCREEN: &str = "screen";
const USER_FIELD: &str = "user_field";
const PASSWORD_FIELD: &str = "pass_field";
const SUBMIT: &str = "submit";
const REFUSED: &str = "refused";
const BLOCKED: &str = "blocked";
const CODE_SCREEN: &str = "code_screen";
const CODE_FIELD: &str = "code_field";
const CODE_SUBMIT: &str = "code_submit";
const CODE_REFUSED: &str = "code_refused";
/// How long, in milliseconds, each wait for the page may take.
const WAIT_MS: &str = "wait_ms";
const SETTINGS: &[&str] = &[
    WAIT_MS,
    SCREEN,
    USER_FIELD,
    PASSWORD_FIELD,
    SUBMIT,
    REFUSED,
    BLOCKED,
    CODE_SCREEN,
    CODE_FIELD,
    CODE_SUBMIT,
    CODE_REFUSED,
];
const CODE_SETTINGS: &[&str] = &[CODE_SCREEN, CODE_FIELD, CODE_SUBMIT];

/// How long each wait for the page may take when the entry does not say:
/// for the sign-in page, and for a sign after each press.
const PAGE_TIME: Duration = Duration::from_secs(30);

/// How long the page is given to say a sign already holds, when it is
/// asked once.
const LOOK_ONCE: Duration = Duration::from_millis(250);

/// The material's plain part that lists the cookies' names and rules.
const COOKIES: &str = "cookies";

fn invalid(why: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        a2m_auth::codes::AUTH_SETTINGS_INVALID,
        format!("The auth entry for `form_login` cannot be used: {why}."),
        "Name the sign-in screen and its parts under `settings`, as the web pack's design says, and store `user` and `password` under `credentials`.",
    )
}

fn refused() -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        a2m_auth::codes::CREDENTIAL_REFUSED,
        "The application's sign-in page refused the stored username or password. It is not tried again.",
        "Check the stored credential, store it again on the engine's sign-in page, and sign in once more.",
    )
}

fn needs_code(why: &str) -> Failure {
    Failure::new(
        FailureClass::SessionExpired,
        a2m_auth::codes::NEEDS_MFA,
        format!("The application asks for a one-time code, and {why}."),
        "Sign in with `anything2mcp session login` while you are there to enter the code.",
    )
}

fn blocked() -> Failure {
    Failure::new(
        FailureClass::EdgeBlocked,
        codes::SIGN_IN_BLOCKED,
        "The application's sign-in page shows the block the target declares. Nothing gets past it.",
        "Look at the application in a browser of your own; sign in again once the block is lifted.",
    )
}

fn no_sign(when: &str) -> Failure {
    Failure::new(
        FailureClass::UnknownVariant,
        codes::UNEXPECTED_PAGE,
        format!(
            "The sign-in page showed none of the signs the target declares {when}: not signed in, not refused, not blocked, and no code asked."
        ),
        "Look at the sign-in page; declare the state it is in as a variant of its screen, or correct the screen.",
    )
}

fn page_lost(error: &BrowserError, what: &str) -> Failure {
    error.failure().unwrap_or_else(|| {
        Failure::new(
            FailureClass::Transient,
            a2m_provider::codes::CONNECTION_LOST,
            format!("The browser did not complete {what} of the sign-in."),
            "Sign in again.",
        )
    })
}

/// The sign-in a target declares, resolved against its compiled screens.
struct Pages {
    origin: Address,
    page: Address,
    screen: Screen,
    user: Field,
    password: Field,
    submit: Action,
    refused: Variant,
    blocked: Option<Variant>,
    code: Option<CodePages>,
}

struct CodePages {
    screen: Screen,
    field: Field,
    submit: Action,
    refused: Option<Variant>,
}

fn screen_of<'a>(section: &'a ScreenSection, name: &str) -> Result<&'a Screen, Failure> {
    section
        .get(name)
        .ok_or_else(|| invalid(&format!("no screen is named `{}`", codes::shown(name))))
}

fn field_of(screen: &Screen, name: &str) -> Result<Field, Failure> {
    let field = screen
        .fields
        .get(name)
        .ok_or_else(|| invalid(&format!("its screen has no field `{}`", codes::shown(name))))?;
    if field.control != "text" {
        return Err(invalid(&format!(
            "the field `{}` is not of the control `text`",
            codes::shown(name)
        )));
    }
    Ok(field.clone())
}

fn action_of(screen: &Screen, name: &str) -> Result<Action, Failure> {
    screen.actions.get(name).cloned().ok_or_else(|| {
        invalid(&format!(
            "its screen has no action `{}`",
            codes::shown(name)
        ))
    })
}

fn variant_of(screen: &Screen, name: &str) -> Result<Variant, Failure> {
    screen.variants.get(name).cloned().ok_or_else(|| {
        invalid(&format!(
            "its screen has no variant `{}`",
            codes::shown(name)
        ))
    })
}

impl Pages {
    fn of(entry: &AuthEntry) -> Result<Self, Failure> {
        let mut webs = entry
            .surfaces
            .surfaces
            .iter()
            .filter(|surface| surface.kind == WEB_SURFACE);
        let surface = webs.next().ok_or_else(|| {
            invalid("it covers no surface of the kind `web`, or the engine gave it nothing of its surfaces")
        })?;
        if webs.next().is_some() {
            return Err(invalid(
                "it covers more than one surface of the kind `web`; a form sign-in is one application's",
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
            .ok_or_else(|| invalid("the target has no screens"))?;
        if section.format != PAYLOAD_FORMAT {
            return Err(invalid(
                "its screens were compiled by another version of the web pack",
            ));
        }
        let screens: ScreenSection = section
            .payload
            .decode()
            .map_err(|_| invalid("its screens do not decode"))?;
        let setting = |name: &str| entry.required(name);
        let screen = screen_of(&screens, setting(SCREEN)?)?;
        if screen.surface != surface.name.as_str() {
            return Err(invalid("its sign-in screen is on another surface"));
        }
        let origin_text = origin_of(&surface.address)?;
        let origin = Address::new(&origin_text)?;
        let page = Address::new(&format!("{origin_text}{}", fixed_path(screen)?))?;
        let code = if CODE_SETTINGS
            .iter()
            .any(|name| entry.optional(name).is_some())
        {
            let code_screen = screen_of(&screens, setting(CODE_SCREEN)?)?;
            if code_screen.surface != screen.surface {
                return Err(invalid("its code screen is on another surface"));
            }
            Some(CodePages {
                field: field_of(code_screen, setting(CODE_FIELD)?)?,
                submit: action_of(code_screen, setting(CODE_SUBMIT)?)?,
                refused: entry
                    .optional(CODE_REFUSED)
                    .map(|name| variant_of(code_screen, name))
                    .transpose()?,
                screen: code_screen.clone(),
            })
        } else {
            if entry.optional(CODE_REFUSED).is_some() {
                return Err(invalid("it names `code_refused` and no code screen"));
            }
            None
        };
        Ok(Self {
            user: field_of(screen, setting(USER_FIELD)?)?,
            password: field_of(screen, setting(PASSWORD_FIELD)?)?,
            submit: action_of(screen, setting(SUBMIT)?)?,
            refused: variant_of(screen, setting(REFUSED)?)?,
            blocked: entry
                .optional(BLOCKED)
                .map(|name| variant_of(screen, name))
                .transpose()?,
            screen: screen.clone(),
            origin,
            page,
            code,
        })
    }
}

/// How long each wait for the page may take.
fn page_time(entry: &AuthEntry) -> Result<Duration, Failure> {
    match entry.optional(WAIT_MS) {
        None => Ok(PAGE_TIME),
        Some(text) => text
            .parse::<u64>()
            .ok()
            .filter(|ms| (100..=600_000).contains(ms))
            .map(Duration::from_millis)
            .ok_or_else(|| invalid("`wait_ms` is not a whole number from 100 to 600000")),
    }
}

/// The path of a screen that takes no input.
fn fixed_path(screen: &Screen) -> Result<String, Failure> {
    let mut path = String::new();
    for segment in &screen.reach {
        match segment {
            Segment::Literal(text) => {
                path.push('/');
                path.push_str(text);
            }
            Segment::Hole(_) => {
                return Err(invalid("its sign-in screen's address has a placeholder"));
            }
        }
    }
    if path.is_empty() {
        path.push('/');
    }
    Ok(path)
}

/// The engine-owned profile of one session: one per session, so that no
/// two persons, and no two sessions, share a browser.
pub(crate) fn profile_name(session: SessionId) -> String {
    let digest = Digest::of(session.to_string().as_bytes()).to_string();
    let short: String = digest
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(32)
        .collect();
    format!("s{short}")
}

/// One cookie, as the material's plain part lists it: everything but the
/// value, which is a secret part of its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CookieRule {
    name: String,
    domain: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires: Option<serde_json::Number>,
    http_only: bool,
    secure: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    same_site: Option<String>,
}

/// One cookie with its value, as the driver is handed it.
#[derive(Debug, Serialize, Deserialize)]
struct PlacedCookie {
    #[serde(flatten)]
    rule: CookieRule,
    value: String,
}

fn secret_part(position: usize) -> String {
    format!("cookie_{position}")
}

/// Turns the browser state a driver was handed back into the browser's
/// form. `None` when it is not the form this provider places.
pub(crate) fn state_from(placed: &Secret) -> Option<BrowserState> {
    let cookies: Vec<PlacedCookie> = serde_json::from_str(placed.expose()).ok()?;
    Some(BrowserState {
        cookies: cookies
            .into_iter()
            .map(|cookie| StateCookie {
                name: cookie.rule.name,
                value: Secret::new(cookie.value),
                domain: cookie.rule.domain,
                path: cookie.rule.path,
                expires: cookie.rule.expires,
                http_only: cookie.rule.http_only,
                secure: cookie.rule.secure,
                same_site: cookie.rule.same_site,
            })
            .collect(),
    })
}

/// The auth provider `form_login`.
pub(crate) struct FormLogin {
    browsers: Browsers,
    /// The credentials the application refused, each as a digest of the
    /// session's key and the stored values, in memory only. One of them is
    /// never sent again: a lockout is real damage, and the keeper counts a
    /// refusal of a sign-in a person asked for as no attempt (decision
    /// 0245). A credential stored anew with another value is tried.
    refused: Mutex<BTreeSet<Digest>>,
}

impl FormLogin {
    pub(crate) fn new(browsers: Browsers) -> Self {
        Self {
            browsers,
            refused: Mutex::new(BTreeSet::new()),
        }
    }

    fn refused_set(&self) -> MutexGuard<'_, BTreeSet<Digest>> {
        self.refused.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn sign_in_now(&self, cx: &SignInContext<'_>) -> Result<Material, Failure> {
        let entry = cx.entry();
        let pages = Pages::of(entry)?;
        // A stored credential is sent only where a published version of the
        // target says: anyone who can write a draft could name an address
        // of their own in it (decision 0245, with decision 0248).
        if !entry.surfaces.from_published {
            return Err(Failure::new(
                FailureClass::SessionExpired,
                a2m_auth::codes::NEEDS_LOGIN,
                "This sign-in is declared by drafts, and a stored password is filled in only on a page a published version of the target declares. Nothing was opened, filled, or sent.",
                "Publish the target, then sign in with `anything2mcp session login`.",
            ));
        }
        // A credential belongs to the target it was stored for.
        let own = format!("vault://{}/", cx.session().key.target);
        for role in [USER, PASSWORD] {
            if entry
                .credentials
                .get(role)
                .is_some_and(|reference| !reference.as_str().starts_with(&own))
            {
                return Err(invalid(&format!(
                    "the credential of the role `{role}` is not one stored for this target"
                )));
            }
        }
        let user = cx.credential(USER).await?;
        let password = cx.credential(PASSWORD).await?;
        let tried = {
            let key = &cx.session().key;
            let mut hashed = Vec::new();
            for part in [
                key.workspace.to_string(),
                key.target.to_string(),
                key.environment.to_string(),
                key.identity.to_string(),
            ] {
                hashed.extend_from_slice(part.as_bytes());
                hashed.push(0);
            }
            hashed.extend_from_slice(user.expose().as_bytes());
            hashed.push(0);
            hashed.extend_from_slice(password.expose().as_bytes());
            Digest::of(&hashed)
        };
        if self.refused_set().contains(&tried) {
            return Err(refused());
        }
        let name = profile_name(cx.session().session);
        let profile = ProfileKey {
            target: cx.session().key.target.clone(),
            environment: cx.session().key.environment.clone(),
            identity: Identity::Account(AccountName::new(&name)?),
        };
        let origins = Origins::of([&pages.origin]);
        let browser = self
            .browsers
            .open(&profile, Visibility::Background)
            .await
            .map_err(|error| page_lost(&error, "the start"))?;
        let tab = browser
            .acting_tab(&PageEnvironment::default(), &origins)
            .await
            .map_err(|error| page_lost(&error, "the opening of a tab"))?;
        // Each wait for the page has the time the entry gives it.
        let wait = page_time(entry)?;
        let deadline = Deadline::after(wait);
        let signs = Signs::of(&pages);

        tab.goto(&pages.page, deadline)
            .await
            .map_err(|error| page_lost(&error, "the move to the sign-in page"))?;
        // The page is the sign-in screen, or the block it declares.
        let mut first = vec![pages.screen.ready.primitive()];
        first.extend(pages.blocked.iter().map(visible));
        wait_for(&tab, Primitive::Any(first), deadline, "when it was opened").await?;
        if signs.holds(&tab, Sign::Blocked, deadline).await? {
            return Err(blocked());
        }
        // A sign that already shows could not be told from the answer.
        for sign in [Sign::Refused, Sign::Code, Sign::SignedIn] {
            if signs.holds(&tab, sign, deadline).await? {
                return Err(no_sign(
                    "before the credential was entered; one of its signs already showed",
                ));
            }
        }

        fill(&tab, &pages.user, &user, deadline, "the username").await?;
        fill(&tab, &pages.password, &password, deadline, "the password").await?;
        press(&tab, &pages.submit, deadline).await?;

        let deadline = Deadline::after(wait);
        let mut sign = signs
            .next(&tab, deadline, "after the credential was sent")
            .await?;
        if sign == Sign::Code {
            let Some(code_pages) = &pages.code else {
                return Err(no_sign("after the credential was sent"));
            };
            let code = match cx
                .ask_person(PersonAsk::EnterCode {
                    reason: PersonReason::SecondFactor,
                })
                .await
            {
                PersonAnswer::Done(returned) => match returned.get("code") {
                    Some(code) if !code.expose().is_empty() => Secret::new(code.expose()),
                    _ => return Err(needs_code("no code came back")),
                },
                PersonAnswer::Declined => return Err(needs_code("the person declined")),
                PersonAnswer::NoAnswer => return Err(needs_code("nobody entered one in time")),
                PersonAnswer::Unreachable => {
                    return Err(needs_code("nobody is there to enter it"));
                }
            };
            cx.register(&code);
            // The person's answer took the person's time, not the page's.
            let deadline = Deadline::after(wait);
            fill(&tab, &code_pages.field, &code, deadline, "the code").await?;
            drop(code);
            press(&tab, &code_pages.submit, deadline).await?;
            let deadline = Deadline::after(wait);
            sign = signs
                .next(&tab, deadline, "after the code was sent")
                .await?;
        }
        match sign {
            Sign::SignedIn => {}
            Sign::Refused => {
                self.refused_set().insert(tried);
                return Err(refused());
            }
            Sign::CodeRefused => return Err(needs_code("the code entered was refused")),
            Sign::Blocked => return Err(blocked()),
            Sign::Code => return Err(no_sign("after the code was sent")),
        }

        let state = browser
            .export_state(&origins, Deadline::after(wait))
            .await
            .map_err(|error| page_lost(&error, "the reading of the session's state"))?;
        drop(tab);
        if state.cookies.is_empty() {
            return Err(Failure::new(
                FailureClass::UnknownVariant,
                codes::UNEXPECTED_PAGE,
                "The sign-in showed its success, and the browser holds no cookie for the application. A session kept in page storage is not built.",
                "Look at how the application keeps its session.",
            ));
        }
        let mut rules = Vec::new();
        let mut material = Material::new(self.name())
            .with_profile(ProfileRef::new(&name)?)
            .expiring_at(None);
        for (position, cookie) in state.cookies.into_iter().enumerate() {
            cx.register(&cookie.value);
            rules.push(CookieRule {
                name: cookie.name,
                domain: cookie.domain,
                path: cookie.path,
                expires: cookie.expires,
                http_only: cookie.http_only,
                secure: cookie.secure,
                same_site: cookie.same_site,
            });
            material = material.with_secret(&secret_part(position), cookie.value);
        }
        let listed = serde_json::to_string(&rules)
            .map_err(|_| invalid("the session's cookies could not be listed"))?;
        Ok(material.with_plain(COOKIES, &listed))
    }
}

/// One of the declared signs a sign-in ends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    SignedIn,
    Refused,
    Blocked,
    Code,
    CodeRefused,
}

/// The declared signs, each as a condition the browser waits on.
struct Signs {
    each: Vec<(Sign, Primitive)>,
}

fn visible(variant: &Variant) -> Primitive {
    Primitive::Visible(variant.when.query())
}

impl Signs {
    fn of(pages: &Pages) -> Self {
        // The order is the order of asking: a block before a refusal,
        // either before a success, so that a page that shows two is read
        // as the one that stops.
        let mut each = Vec::new();
        if let Some(blocked) = &pages.blocked {
            each.push((Sign::Blocked, visible(blocked)));
        }
        each.push((Sign::Refused, visible(&pages.refused)));
        if let Some(code) = &pages.code {
            if let Some(refused) = &code.refused {
                each.push((Sign::CodeRefused, visible(refused)));
            }
            each.push((Sign::Code, code.screen.ready.primitive()));
            each.push((Sign::SignedIn, code.submit.confirm.primitive()));
        }
        each.push((Sign::SignedIn, pages.submit.confirm.primitive()));
        Self { each }
    }

    async fn holds(&self, tab: &ActTab, sign: Sign, deadline: Deadline) -> Result<bool, Failure> {
        for (each, condition) in &self.each {
            if *each == sign && holds_now(tab, condition, deadline).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Waits for the first declared sign, and says which it is.
    async fn next(&self, tab: &ActTab, deadline: Deadline, when: &str) -> Result<Sign, Failure> {
        let any = Primitive::Any(self.each.iter().map(|(_, c)| c.clone()).collect());
        wait_for(tab, any, deadline, when).await?;
        for (sign, condition) in &self.each {
            if holds_now(tab, condition, deadline).await? {
                return Ok(*sign);
            }
        }
        Err(no_sign(when))
    }
}

async fn wait_for(
    tab: &ActTab,
    condition: Primitive,
    deadline: Deadline,
    when: &str,
) -> Result<(), Failure> {
    match tab.wait(&condition, deadline).await {
        Ok(()) => Ok(()),
        Err(BrowserError::Timeout { .. }) => Err(no_sign(when)),
        Err(error) => Err(page_lost(&error, "a wait")),
    }
}

async fn holds_now(
    tab: &ActTab,
    condition: &Primitive,
    deadline: Deadline,
) -> Result<bool, Failure> {
    let once = Deadline::after(LOOK_ONCE.min(deadline.remaining()));
    match tab.wait(condition, once).await {
        Ok(()) => Ok(true),
        Err(BrowserError::Timeout { .. }) => Ok(false),
        Err(error) => Err(page_lost(&error, "a look at the page")),
    }
}

/// The one element a field's or an action's locators find, in order; a
/// later locator is tried only when an earlier one finds nothing.
async fn element(
    tab: &ActTab,
    locators: &[crate::model::Locator],
    deadline: Deadline,
    what: &str,
) -> Result<a2m_pack_web_browser::Element, Failure> {
    for locator in locators {
        match tab.locate(&locator.query(), deadline).await {
            Ok(found) => return Ok(found),
            Err(BrowserError::NotFound) => {}
            Err(BrowserError::Ambiguous { count }) => {
                return Err(Failure::new(
                    FailureClass::Drift,
                    codes::LOCATOR_AMBIGUOUS,
                    format!("A locator of {what} on the sign-in page matches {count} elements."),
                    "Correct the screen so that each locator finds one element.",
                ));
            }
            Err(error) => return Err(page_lost(&error, "the finding of a field")),
        }
    }
    Err(Failure::new(
        FailureClass::Drift,
        codes::LOCATOR_NOT_FOUND,
        format!("No locator of {what} finds anything on the sign-in page."),
        "Look at the sign-in page and correct its screen.",
    ))
}

async fn fill(
    tab: &ActTab,
    field: &Field,
    value: &Secret,
    deadline: Deadline,
    what: &str,
) -> Result<(), Failure> {
    let found = element(tab, &field.locate, deadline, what).await?;
    tab.input(&found, &Input::Fill(value.expose().to_owned()), deadline)
        .await
        .map_err(|error| page_lost(&error, "the entering of a field"))
}

async fn press(tab: &ActTab, action: &Action, deadline: Deadline) -> Result<(), Failure> {
    let found = element(tab, &action.locate, deadline, "the sign-in action").await?;
    tab.input(&found, &Input::Click, deadline)
        .await
        .map_err(|error| page_lost(&error, "the press of the sign-in action"))
}

impl AuthProvider for FormLogin {
    fn name(&self) -> ProviderName {
        ProviderName::fixed(FORM_LOGIN)
    }

    fn needs(&self) -> ProviderNeeds {
        ProviderNeeds {
            // The stored credential signs in; a person is asked only for a
            // code the application asks for.
            person: false,
            silent_renewal: false,
            light_prompt: false,
            movable: true,
            // The declared sign of success is the application's own word.
            confirms: true,
        }
    }

    fn check(&self, entry: &AuthEntry) -> Result<(), Failure> {
        entry.only(SETTINGS, &[USER, PASSWORD])?;
        for role in [USER, PASSWORD] {
            if !entry.credentials.contains_key(role) {
                return Err(invalid(&format!(
                    "no credential is named for the role `{role}`"
                )));
            }
        }
        // A refused credential is a stop, never a second try: a lockout is
        // real damage to the person's account.
        if entry.attempt_limit != 1 {
            return Err(invalid(
                "`attempt_limit` is not 1; a form sign-in tries a stored credential once",
            ));
        }
        page_time(entry)?;
        Pages::of(entry).map(|_| ())
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
        // The provider has no rung below a full sign-in; the keeper never
        // asks for one.
        Box::pin(async {
            Err(Failure::new(
                FailureClass::SessionExpired,
                a2m_auth::codes::NEEDS_LOGIN,
                "A form sign-in is renewed only by signing in again.",
                "Sign in with `anything2mcp session login`.",
            ))
        })
    }

    fn sign_out<'a>(
        &'a self,
        material: &'a Material,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            // The session's browser is closed: its in-memory context, and
            // every cookie in it, goes with it. The application's own
            // sign-out is not built.
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
        let incomplete = || {
            Failure::new(
                FailureClass::SessionExpired,
                a2m_auth::codes::NEEDS_LOGIN,
                "The session cannot be used: its saved state is incomplete.",
                "Sign in with `anything2mcp session login`, then run the tool again or resume the run.",
            )
        };
        let rules: Vec<CookieRule> = material
            .plain(COOKIES)
            .and_then(|listed| serde_json::from_str(listed).ok())
            .ok_or_else(incomplete)?;
        let mut placed = Vec::new();
        for (position, rule) in rules.into_iter().enumerate() {
            let value = material
                .secret(&secret_part(position))
                .ok_or_else(incomplete)?;
            placed.push(PlacedCookie {
                rule,
                value: value.expose().to_owned(),
            });
        }
        let state = serde_json::to_string(&placed).map_err(|_| incomplete())?;
        sink.header(BROWSER_STATE, &Secret::new(state));
        Ok(())
    }
}

/// The screens a target's auth entries declare as a sign that the session
/// has ended, for one surface: each signal of the form `{screen: <name>}`.
/// A signal of any other form is refused: unknown means stop.
pub(crate) fn expiry_screens(
    plan: &a2m_core::Plan,
    surface: &str,
    screens: &BTreeMap<usize, ScreenSection>,
) -> Result<Vec<(String, Cond)>, String> {
    let Some(index) = plan
        .surfaces
        .iter()
        .position(|known| known.name.as_str() == surface)
    else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for auth in &plan.auth {
        if !auth.surfaces.iter().any(|covered| covered.get() == index) {
            continue;
        }
        for signal in &auth.expiry_signals {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct ScreenSignal {
                screen: String,
            }
            let signal: ScreenSignal = signal.decode().map_err(|_| {
                "a sign of expiry of a web surface is written `{screen: <name>}`, and one is not"
                    .to_owned()
            })?;
            let screen = screens
                .values()
                .find_map(|section| section.get(&signal.screen))
                .ok_or_else(|| {
                    format!(
                        "the sign of expiry names the screen `{}`, which the target does not have",
                        codes::shown(&signal.screen)
                    )
                })?;
            found.push((signal.screen, screen.ready.clone()));
        }
    }
    Ok(found)
}
