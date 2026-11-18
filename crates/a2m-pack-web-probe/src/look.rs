//! What the three discoverers share: opening an observing tab, arriving at
//! a page and saying how it stands, the rules for links, and making a
//! finding.
//!
//! Everything here looks. The tab is an observing tab of the base: it has
//! no method that acts, and the browser refuses every request of the page
//! that is not a read.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    AccountName, Coverage, EvidenceLevel, Failure, FailureClass, FailureCode, Finding, FindingKind,
    Identity, LimitationCode, ProducedBy, Redact, Source, Subject, Value,
};
use a2m_pack_web_browser::{
    Address, BrowserError, By, Deadline, Look, ObserveTab, Observer, Origins, PageEnvironment,
    ProfileKey, Query, RawValue, ReadWhat, SafeRequests, is_plain_name, path_by_structure,
};
use a2m_provider::{DiscoveryScope, ProviderName};
use a2m_telemetry::Clock;
use serde_json::Value as Json;

use crate::codes;
use crate::shape::type_of_text;

/// The name, in a discovery's focus, of the list of paths to look at
/// beside the surface's own address. Each starts with `/` and may carry a
/// query.
pub const FOCUS_PATHS: &str = "paths";
/// The name, in a discovery's focus, of a text that must be visible on a
/// page the focus names before the page is read.
pub const FOCUS_READY_TEXT: &str = "ready_text";
/// The name, in a discovery's focus, of the switch that lets endpoint
/// inference make a parameter of a segment by variance alone.
pub const FOCUS_TIER_TWO: &str = "tier_two";

/// The account name of the profile discovery looks through. It is the
/// engine's own, and is signed in to nothing.
pub const DISCOVERY_ACCOUNT: &str = "discovery";

/// How long a page is waited for to show the focus's ready text.
const READY_WAIT: Duration = Duration::from_secs(10);

/// The words that make an address or a link look like an action, and the
/// check, are the web pack's base's, so that the drivers hold a key and a
/// page request to the same list (decision 0195).
pub use a2m_pack_web_browser::{ACTION_WORDS, looks_like_an_action};

/// An address, taken apart. It holds no value of a query: only the names,
/// each with the type of its value, and only those that are plain names; and
/// its path is by structure, every segment that is not a plain word kept as
/// a placeholder (decision 0195).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) origin: String,
    pub(crate) path: String,
    pub(crate) query: BTreeMap<String, &'static str>,
    /// Whether the query held an entry that is not kept: one with no `=`,
    /// whose whole text is a value, or one whose name is not a plain name.
    pub(crate) query_left_out: bool,
}

impl Place {
    /// Takes an absolute `http` or `https` address apart.
    pub(crate) fn parse(address: &str) -> Option<Self> {
        let (scheme, rest) = address.split_once("://")?;
        if scheme != "http" && scheme != "https" {
            return None;
        }
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (host, tail) = rest.split_at(end);
        if host.is_empty() || host.contains('@') {
            return None;
        }
        let tail = tail.split('#').next().unwrap_or_default();
        let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
        let mut names = BTreeMap::new();
        let mut query_left_out = false;
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            match pair.split_once('=') {
                Some((name, value)) if is_plain_name(name) => {
                    names.insert(name.to_owned(), type_of_text(value));
                }
                // A bare entry is a value, and so is a name that is not a
                // plain one: neither is kept.
                _ => query_left_out = true,
            }
        }
        Some(Self {
            origin: format!("{scheme}://{}", host.to_lowercase()),
            path: if path.is_empty() {
                "/".to_owned()
            } else {
                path_by_structure(path)
            },
            query: names,
            query_left_out,
        })
    }

    /// The origin and the path, with no query.
    pub(crate) fn plain(&self) -> String {
        format!("{}{}", self.origin, self.path)
    }

    /// The place as the subject of a finding. The path passes the
    /// redactor: a path can carry a token.
    pub(crate) fn subject(&self, redact: &dyn Redact) -> Result<Subject, Failure> {
        Ok(Subject::Address(a2m_core::Address::new(
            &self.origin,
            &redact.redact(&self.path),
        )?))
    }
}

/// How a page stands once the tab has arrived at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Standing {
    /// It answered as itself.
    Open,
    /// It asks for a sign-in. The observation says how that was seen:
    /// `status_401`, `password_form`, or `other_origin`.
    SignIn(&'static str),
    /// It stopped answering as itself.
    Blocked(FailureCode),
}

/// A page the tab arrived at.
#[derive(Debug, Clone)]
pub(crate) struct Arrival {
    /// Where the tab was sent.
    pub(crate) asked: Place,
    /// Where it ended, after redirects.
    pub(crate) ended: Place,
    /// The page's title, through the redactor.
    pub(crate) title: String,
    /// The status of the document's answer.
    pub(crate) status: Option<u16>,
    /// How the page stands.
    pub(crate) standing: Standing,
}

impl Arrival {
    /// The failure a discoverer that reads pages stops with, when the
    /// page does not stand open.
    pub(crate) fn must_be_open(&self) -> Result<(), Failure> {
        match &self.standing {
            Standing::Open => Ok(()),
            Standing::SignIn(_) => Err(Failure::new(
                FailureClass::SessionExpired,
                codes::NEEDS_LOGIN,
                "The page asks for a sign-in, and discovery looks as nobody.",
                "Run the probe to see the sign-in method; a page behind a sign-in is looked at only by a session the keeper holds.",
            )),
            Standing::Blocked(code) => Err(Failure::new(
                FailureClass::EdgeBlocked,
                code.clone(),
                "The surface stopped answering as itself; discovery of it stops here.",
                "Look at the surface in a browser, and ask its owner before trying again.",
            )),
        }
    }
}

/// A selector query.
pub(crate) fn css(selector: &str) -> Query {
    Query::new(By::Css {
        selector: selector.to_owned(),
    })
}

/// What a base error means for a discoverer that was only looking.
pub(crate) fn failure(error: &BrowserError) -> Failure {
    if let Some(failure) = error.failure() {
        return failure;
    }
    match error {
        BrowserError::NotLoaded(_) => Failure::new(
            FailureClass::Transient,
            codes::PAGE_NOT_LOADED,
            "The page did not load.",
            "Check that the surface's address is right and that the system is up, then run discovery again.",
        ),
        BrowserError::Timeout { .. }
        | BrowserError::Crashed { .. }
        | BrowserError::Disconnected { .. } => Failure::new(
            FailureClass::Transient,
            a2m_pack_web_browser::codes::CONNECTION_LOST,
            "The browser was lost, or did not answer in time, while a page was read.",
            "Run discovery again.",
        ),
        other => unreadable(other),
    }
}

/// The failure of a page that could not be read. It names what kind of
/// answer the browser gave and holds none of its text: the text of an
/// answer no rule covers can be the page's own, and free text from the
/// system goes into no message (spec section 502).
pub(crate) fn unreadable(error: &BrowserError) -> Failure {
    let what = match error {
        BrowserError::NotFound => "an element it looked for is not there",
        BrowserError::Ambiguous { .. } => "an element it looked for is there more than once",
        BrowserError::NotActionable(_) => "an element it looked for cannot be used",
        BrowserError::UnexpectedDialog { .. } => "the page opened a dialog",
        BrowserError::PasswordField => "it would have read a password",
        BrowserError::TooLarge { .. } => "what it read is larger than the limit",
        BrowserError::RequestFailed(_) => "a request of the page did not complete",
        _ => "the browser answered in a way no rule covers",
    };
    Failure::new(
        FailureClass::UnknownVariant,
        codes::PAGE_UNREADABLE,
        format!("The page could not be read: {what}."),
        "Look at the page in a browser; report this if the page is an ordinary one.",
    )
}

/// Whether exactly one, or more than one, rendered element matches.
pub(crate) async fn present(
    tab: &ObserveTab,
    query: &Query,
    scope: &DiscoveryScope<'_>,
) -> Result<bool, Failure> {
    match tab.locate(query, scope.deadline).await {
        Ok(_) | Err(BrowserError::Ambiguous { .. }) => Ok(true),
        Err(BrowserError::NotFound) => Ok(false),
        Err(error) => Err(failure(&error)),
    }
}

/// The text of the document's body when it is a JSON document, parsed.
/// Nothing for a page, or for a body longer than the base reads.
pub(crate) async fn json_body(
    tab: &ObserveTab,
    scope: &DiscoveryScope<'_>,
) -> Result<Option<Json>, Failure> {
    let body = match tab.locate(&css("body"), scope.deadline).await {
        Ok(body) => body,
        Err(BrowserError::NotFound | BrowserError::Ambiguous { .. }) => return Ok(None),
        Err(error) => return Err(failure(&error)),
    };
    let text = match tab.read(&body, &ReadWhat::Text, scope.deadline).await {
        Ok(RawValue::Text(text)) => text,
        Ok(RawValue::Flag(_) | RawValue::Absent)
        | Err(BrowserError::TooLarge { .. } | BrowserError::NotFound) => return Ok(None),
        Err(error) => return Err(failure(&error)),
    };
    let text = text.trim();
    if !(text.starts_with('{') || text.starts_with('[')) {
        return Ok(None);
    }
    Ok(serde_json::from_str(text).ok())
}

/// How strong a finding is, why it is weaker than it looks, and what it
/// was worked out from.
pub(crate) struct Claim<'a> {
    pub(crate) level: EvidenceLevel,
    pub(crate) limitations: Vec<(LimitationCode, String)>,
    pub(crate) parents: Vec<&'a Finding>,
}

/// What the discoverers share.
pub(crate) struct Core {
    observer: Observer,
    provider: ProviderName,
    clock: Arc<dyn Clock>,
    pub(crate) max_pages: u64,
}

impl Core {
    pub(crate) fn new(
        observer: Observer,
        provider: ProviderName,
        clock: Arc<dyn Clock>,
        max_pages: u64,
    ) -> Self {
        Self {
            observer,
            provider,
            clock,
            max_pages,
        }
    }

    /// An observing tab in the discovery profile of the scope's target
    /// and environment. Nothing is marked safe: every request of the page
    /// that is not a read is refused. The tab is opened for the origin of
    /// the surface it observes, and the browser keeps it there.
    pub(crate) async fn tab(&self, scope: &DiscoveryScope<'_>) -> Result<ObserveTab, Failure> {
        let (surface, _) = Self::start(scope)?;
        let profile = ProfileKey {
            target: scope.surface.target.clone(),
            environment: scope.surface.environment.clone(),
            identity: Identity::Account(AccountName::new(DISCOVERY_ACCOUNT)?),
        };
        self.observer
            .observing_tab(
                &profile,
                &PageEnvironment::default(),
                &SafeRequests::default(),
                &Origins::of([&surface]),
            )
            .await
            .map_err(|error| failure(&error))
    }

    /// The surface's own address, taken apart.
    pub(crate) fn start(scope: &DiscoveryScope<'_>) -> Result<(Address, Place), Failure> {
        let address = Address::new(&scope.surface.surface.address)?;
        let place = Place::parse(address.as_str()).ok_or_else(|| not_declared("its address"))?;
        Ok((address, place))
    }

    /// The pages the focus names, as addresses under the surface's own
    /// origin. A focus can choose a path and nothing else.
    pub(crate) fn focus_pages(
        scope: &DiscoveryScope<'_>,
        start: &Place,
    ) -> Result<Vec<Address>, Failure> {
        let Some(paths) = scope.focus.get(FOCUS_PATHS) else {
            return Ok(Vec::new());
        };
        let invalid = || {
            Failure::new(
                FailureClass::Input,
                codes::FOCUS_INVALID,
                "The focus `paths` is a list of paths of the surface, each starting with one `/`.",
                "Give paths such as `/orders`; an address of another system cannot be looked at through this surface.",
            )
        };
        let mut pages = Vec::new();
        for path in paths.as_list().ok_or_else(invalid)? {
            let path = path.as_text().ok_or_else(invalid)?;
            let plain = path.starts_with('/')
                && !path.starts_with("//")
                && !path.contains(|c: char| c == '\\' || c.is_whitespace() || c.is_control());
            if !plain {
                return Err(invalid());
            }
            pages.push(Address::new(&format!("{}{path}", start.origin))?);
        }
        Ok(pages)
    }

    /// The focus's ready text, when it names one.
    pub(crate) fn ready_text<'a>(scope: &'a DiscoveryScope<'_>) -> Option<&'a str> {
        scope.focus.get(FOCUS_READY_TEXT).and_then(Value::as_text)
    }

    /// Sends the tab to an address and says how the page stands. A page
    /// that ended at another origin is not read at all.
    pub(crate) async fn arrive(
        &self,
        tab: &ObserveTab,
        address: &Address,
        declared: &Place,
        scope: &DiscoveryScope<'_>,
    ) -> Result<Arrival, Failure> {
        let asked = Place::parse(address.as_str()).ok_or_else(|| not_declared("the address"))?;
        if asked.origin != declared.origin {
            return Err(not_declared("the address"));
        }
        // Every address the tab is sent to passes the same check, whatever
        // named it: the surface, a focus, or a link (decision 0144).
        if looks_like_an_action(after_origin(address.as_str())) {
            return Err(looks_like_an_action_refused("is"));
        }
        let loaded = match tab.goto(address, scope.deadline).await {
            Ok(loaded) => loaded,
            // The answer sent the tab to another origin, and the browser
            // ended the move there: nothing of that origin was fetched. A
            // surface that sends its visitor elsewhere is seen as asking for
            // a sign-in there, as before the browser held the tab.
            Err(BrowserError::ForeignOrigin(origin)) => {
                let ended = Place::parse(&origin).ok_or_else(|| not_declared("the address"))?;
                return Ok(Arrival {
                    asked,
                    ended,
                    title: String::new(),
                    status: None,
                    standing: Standing::SignIn("other_origin"),
                });
            }
            Err(error) => return Err(failure(&error)),
        };
        // A redirect is followed by the browser before this crate sees it;
        // one that ended at an address that looks like an action is not
        // read, and stops the discovery.
        if looks_like_an_action(after_origin(&loaded.address)) {
            return Err(looks_like_an_action_refused(
                "was sent on to an address that",
            ));
        }
        let ended = Place::parse(&loaded.address).ok_or_else(|| {
            Failure::new(
                FailureClass::UnknownVariant,
                codes::PAGE_UNREADABLE,
                "The page ended at an address that is not an http or https address.",
                "Look at the page in a browser.",
            )
        })?;
        let standing = if ended.origin != declared.origin {
            Standing::SignIn("other_origin")
        } else {
            match loaded.status {
                Some(401) => Standing::SignIn("status_401"),
                Some(403) => Standing::Blocked(codes::FORBIDDEN_BY_EDGE),
                Some(429) => Standing::Blocked(codes::RATE_LIMITED_BY_EDGE),
                _ if present(tab, &css("input[type=password]"), scope).await? => {
                    Standing::SignIn("password_form")
                }
                _ => Standing::Open,
            }
        };
        Ok(Arrival {
            asked,
            ended,
            title: codes::shown(&scope.redact.redact(&loaded.title)),
            status: loaded.status,
            standing,
        })
    }

    /// Waits, within a bound, until the ready text is visible. Says
    /// whether it was seen.
    pub(crate) async fn ready(
        tab: &ObserveTab,
        text: &str,
        scope: &DiscoveryScope<'_>,
    ) -> Result<bool, Failure> {
        let until = a2m_pack_web_browser::Primitive::Visible(Query::new(By::Text {
            text: text.to_owned(),
            contains: true,
        }));
        let deadline = if scope.deadline.remaining() > READY_WAIT {
            Deadline::after(READY_WAIT)
        } else {
            scope.deadline
        };
        match tab.wait(&until, deadline).await {
            Ok(()) => Ok(true),
            Err(BrowserError::Timeout { .. }) => Ok(false),
            Err(error) => Err(failure(&error)),
        }
    }

    /// Makes a finding from the live system. What was found passes the
    /// redactor as one text, names and all.
    pub(crate) fn finding(
        &self,
        scope: &DiscoveryScope<'_>,
        subject: Subject,
        kind: &FindingKind,
        found: &Json,
        claim: &Claim<'_>,
    ) -> Result<Finding, Failure> {
        let Claim {
            level,
            limitations,
            parents,
        } = claim;
        let mut builder = Finding::about(
            subject,
            kind.clone(),
            scope.redact.redact(&found.to_string()),
            Source::LiveSystem,
            ProducedBy {
                provider: self.provider.to_string(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                component: None,
            },
            self.clock.now(),
        )
        .level(*level)
        .coverage(Coverage::Partial);
        for (code, sentence) in limitations {
            builder = builder.limitation(*code, scope.redact.redact(sentence));
        }
        for parent in parents {
            builder = builder.derived_from(parent);
        }
        Ok(builder.build()?)
    }
}

/// What follows the origin of an address: its path and its query. The
/// host is the surface's own, and a host's name is not an action.
fn after_origin(address: &str) -> &str {
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    rest.find(['/', '?', '#']).map_or("", |at| &rest[at..])
}

fn looks_like_an_action_refused(how: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        codes::ADDRESS_LOOKS_LIKE_AN_ACTION,
        format!(
            "Discovery was to look at a page whose address {how} looks like an action, and a system may act on a plain visit to such an address. It was not looked at."
        ),
        "Leave the address out of the focus, or look at the page in a browser yourself; discovery never visits an address that looks like an action.",
    )
}

fn not_declared(what: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        codes::SURFACE_NOT_DECLARED,
        format!(
            "Discovery was asked to look outside the surface it was given: {what} is not an address of that surface."
        ),
        "Declare the other system as a surface of the target, and run discovery on it.",
    )
}

/// The text match of a link's own name, for locating it again.
pub(crate) fn link(name: &str) -> Query {
    Query::role("link", name)
}

/// Where a link leads, when it is a plain link to a page of the declared
/// surface: an address of the same origin, without the query's values
/// being looked at. Nothing for a fragment, another origin, or a scheme
/// that is not a page.
pub(crate) fn link_target(href: &str, page: &Place) -> Option<Address> {
    let href = href.trim();
    let absolute = if href.starts_with("http://") || href.starts_with("https://") {
        href.to_owned()
    } else if href.starts_with('/') && !href.starts_with("//") {
        format!("{}{href}", page.origin)
    } else {
        return None;
    };
    let place = Place::parse(&absolute)?;
    if place.origin != page.origin {
        return None;
    }
    let without_fragment = absolute.split('#').next().unwrap_or_default();
    Address::new(without_fragment).ok()
}
