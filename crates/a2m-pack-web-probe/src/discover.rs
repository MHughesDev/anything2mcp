//! The three discoverers: `web_probe`, `web_snapshot`, and `web_capture`.
//!
//! **Discovery cannot write.** Each discoverer here holds an observer of
//! the base and opens observing tabs only. Such a tab has no method that
//! acts, and the browser fails every request of the page, its frames, and
//! its workers that is not a read, and refuses its sockets. A link is
//! followed only when it is a plain link to a page of the surface and
//! neither its address nor its name looks like an action. The sandbox
//! port is not used: this crate has no driver to send a step to.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Coverage, Digest, EvidenceLevel, Failure, FailureClass, Finding, FindingKind, LimitationCode,
    Value,
};
use a2m_pack_web_browser::{
    Address, BrowserError, Deadline, EXCHANGES_KEPT, Look, ObserveTab, RawValue, ReadWhat,
    RequestShape, SnapshotLimits,
};
use a2m_provider::{
    BoxFuture, Discoverer, DiscovererName, DiscoveryReport, DiscoveryScope, FindingSink,
    SandboxPort, SurfaceDescription, TrafficCapture, take_capture,
};
use serde_json::Value as Json;

use crate::infer::{Answer, Exchange, infer_endpoints, matched_requests};
use crate::look::{
    Arrival, Claim, Core, FOCUS_TIER_TWO, Place, Standing, failure, json_body, link, link_target,
    looks_like_an_action,
};
use crate::screen;
use crate::shape::{Shape, object};
use crate::{SURFACE_KIND, codes};

/// The methods of a request. Anything else an observing tab reports is a
/// thing the page made, such as a socket.
const METHODS: &[&str] = &[
    "GET", "HEAD", "OPTIONS", "POST", "PUT", "PATCH", "DELETE", "CONNECT", "TRACE",
];

/// One rule that names the kind of interface a start page shows. The
/// first rule that holds names it. Data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfaceRule {
    /// The kind of interface.
    pub name: &'static str,
    /// What must hold.
    pub when: Marker,
}

/// What a rule looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// The document is JSON.
    JsonDocument,
    /// The page shows an element of one of these roles.
    AnyRole(&'static [&'static str]),
    /// The page shows anything at all.
    AnyElement,
}

/// The rules for the kind of interface, in order.
pub const INTERFACE_RULES: &[InterfaceRule] = &[
    InterfaceRule {
        name: "json_document",
        when: Marker::JsonDocument,
    },
    InterfaceRule {
        name: "form",
        when: Marker::AnyRole(screen::FIELD_ROLES),
    },
    InterfaceRule {
        name: "record_list",
        when: Marker::AnyRole(&["table", "grid", "treegrid"]),
    },
    InterfaceRule {
        name: "document",
        when: Marker::AnyElement,
    },
];

/// What is said when no rule holds: nothing a person could perceive was
/// there when the page was read.
pub const EMPTY_INTERFACE: &str = "empty";

pub(crate) fn suits_web(surface: &SurfaceDescription) -> bool {
    surface.kind.as_str() == SURFACE_KIND
        && (surface.address.starts_with("http://") || surface.address.starts_with("https://"))
}

/// What the page tried and the tab refused, as a limitation.
fn refused_note(refused: &[RequestShape]) -> Option<(LimitationCode, String)> {
    if refused.is_empty() {
        return None;
    }
    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    for one in refused {
        *counts.entry(one.method.as_str()).or_default() += 1;
    }
    let listed: Vec<String> = counts
        .iter()
        .map(|(method, count)| format!("{method} {count}"))
        .collect();
    Some((
        LimitationCode::NotExercised,
        format!(
            "The page tried {} thing(s) an observing tab refuses ({}). Nothing left the browser, so what they would have done was not seen.",
            refused.len(),
            listed.join(", ")
        ),
    ))
}

/// The name, in a `screen` finding, of the origins the page tried to send a
/// request to that the observing tab refused: the origins outside the
/// surface's own, as the tab reported them (decision 0292). A finding holds
/// the field only when there are some, each origin once, in order. The
/// drafting rules of the web pack's drivers read it into the drafted
/// operation's `permissions.destinations`, for a person to review.
pub const REFUSED_ORIGINS: &str = "refused_origins";

/// What an observing tab refused, taken apart: the origins outside its
/// reach that its page tried to send to, each once and in order, and
/// everything else it refused.
fn outside_origins(refused: Vec<RequestShape>) -> (BTreeSet<String>, Vec<RequestShape>) {
    let mut origins = BTreeSet::new();
    let mut others = Vec::new();
    for one in refused {
        if one.method != a2m_pack_web_browser::OUTSIDE {
            others.push(one);
            continue;
        }
        // An address that does not read as one of `http` or `https` is not
        // an origin a person could declare: it is counted with the rest.
        match Place::parse(&one.address) {
            Some(place) => {
                origins.insert(place.origin);
            }
            None => others.push(one),
        }
    }
    (origins, others)
}

/// The limitation that says which origins the page tried to send to.
fn outside_note(origins: &BTreeSet<String>) -> Option<(LimitationCode, String)> {
    if origins.is_empty() {
        return None;
    }
    let listed: Vec<&str> = origins.iter().map(String::as_str).collect();
    Some((
        LimitationCode::NotExercised,
        format!(
            "The page tried to send requests to {} origin(s) outside the surface's own ({}). The observing tab refused them before they left the browser, so what they would have loaded was not seen. They are listed in the finding as `{REFUSED_ORIGINS}`.",
            origins.len(),
            listed.join(", ")
        ),
    ))
}

/// How long `web_capture` waits for the answers to the requests one page
/// made by itself, at most.
const PAGE_WAIT: Duration = Duration::from_secs(10);

/// The limitation of an address whose query held an entry that is not
/// kept.
pub(crate) fn query_left_out() -> (LimitationCode, String) {
    (
        LimitationCode::NotExercised,
        "The query held an entry with no name, or a name that is not a plain word; such an entry is a value, and was not kept.".to_owned(),
    )
}

/// The limitation of an answer an object of which read as a dictionary.
pub(crate) fn dictionary() -> (LimitationCode, String) {
    (
        LimitationCode::NotExercised,
        "An object of the answer had names that read as values (more of them than a record has, or one that is not a plain name), as a dictionary's keys are; none of its names were kept, so whether they are structure is not known.".to_owned(),
    )
}

/// The limitations of every member of a matched group, each once, in the
/// order first seen: what one page or request showed holds for the whole.
pub(crate) fn union<'a>(
    each: impl Iterator<Item = &'a [(LimitationCode, String)]>,
) -> Vec<(LimitationCode, String)> {
    let mut out: Vec<(LimitationCode, String)> = Vec::new();
    for one in each.flatten() {
        if !out.contains(one) {
            out.push(one.clone());
        }
    }
    out
}

fn single_variant(what: &str) -> (LimitationCode, String) {
    (
        LimitationCode::SingleVariant,
        format!("{what} was looked at once, signed in as nobody, at one size."),
    )
}

/// The kinds of finding this crate makes.
pub(crate) struct Kinds {
    surface: FindingKind,
    sign_in_method: FindingKind,
    pub(crate) screen: FindingKind,
    pub(crate) exchange: FindingKind,
    pub(crate) endpoint: FindingKind,
}

impl Kinds {
    pub(crate) fn new() -> Result<Self, Failure> {
        Ok(Self {
            surface: FindingKind::new("surface")?,
            sign_in_method: FindingKind::new("sign_in_method")?,
            screen: FindingKind::new("screen")?,
            exchange: FindingKind::new("exchange")?,
            endpoint: FindingKind::new("endpoint")?,
        })
    }
}

/// What every discoverer of this crate is made of.
pub(crate) struct Parts {
    pub(crate) core: Core,
    pub(crate) kinds: Kinds,
}

/// Sends each finding on once: the same observation made twice has one
/// identifier, and is one finding.
pub(crate) struct Once<'a> {
    out: &'a mut dyn FindingSink,
    sent: BTreeSet<Digest>,
}

impl<'a> Once<'a> {
    pub(crate) fn new(out: &'a mut dyn FindingSink) -> Self {
        Self {
            out,
            sent: BTreeSet::new(),
        }
    }

    pub(crate) fn found(&mut self, finding: &Finding) -> Result<(), Failure> {
        if self.sent.insert(finding.id()) {
            self.out.found(finding.clone())?;
        }
        Ok(())
    }
}

// ---- web_probe ----

/// `web_probe`: what kind of interface the surface's start page shows,
/// and whether it stands open, asks for a sign-in, or is blocked.
pub(crate) struct WebProbe(pub(crate) Arc<Parts>, pub(crate) DiscovererName);

impl WebProbe {
    fn interface(json: bool, roles: &BTreeSet<&str>) -> &'static str {
        INTERFACE_RULES
            .iter()
            .find(|rule| match rule.when {
                Marker::JsonDocument => json,
                Marker::AnyRole(wanted) => wanted.iter().any(|role| roles.contains(role)),
                Marker::AnyElement => !roles.is_empty(),
            })
            .map_or(EMPTY_INTERFACE, |rule| rule.name)
    }

    async fn look(
        &self,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let Parts { core, kinds } = &*self.0;
        let (address, start) = Core::start(scope)?;
        let tab = core.tab(scope).await?;
        let arrival = core.arrive(&tab, &address, &start, scope).await?;
        let mut limitations = vec![single_variant("The start page")];
        // A page that does not stand open is named, never passed: nothing
        // of it is read.
        let interface = if arrival.standing == Standing::Open {
            let json = json_body(&tab, scope).await?.is_some();
            let snapshot = tab
                .snapshot(&SnapshotLimits::default(), scope.deadline)
                .await
                .map_err(|error| failure(&error))?;
            if snapshot.cut {
                limitations.push((
                    LimitationCode::Truncated,
                    "The page holds more elements than a snapshot keeps.".to_owned(),
                ));
            }
            let roles: BTreeSet<&str> = snapshot
                .elements
                .iter()
                .map(|one| one.role.as_str())
                .collect();
            Json::from(Self::interface(json, &roles))
        } else {
            Json::Null
        };
        limitations.extend(refused_note(&tab.blocked()));
        let (answer, block) = match &arrival.standing {
            Standing::Open => ("open", Json::Null),
            Standing::SignIn(_) => ("sign_in", Json::Null),
            Standing::Blocked(code) => ("blocked", Json::from(code.as_str())),
        };
        let found = object([
            ("format", Json::from("1")),
            ("answer", Json::from(answer)),
            ("block", block),
            ("status", arrival.status.map_or(Json::Null, Json::from)),
            (
                "redirected",
                Json::Bool(arrival.asked.plain() != arrival.ended.plain()),
            ),
            ("interface", interface),
            // No platform profile is built, so none can match: the
            // platform is unknown, never guessed, and the tier follows.
            ("platform", Json::Null),
            ("platform_state", Json::from("unknown_platform")),
            ("tier", Json::from("unsupported")),
        ]);
        let surface = core.finding(
            scope,
            arrival.asked.subject(scope.redact)?,
            &kinds.surface,
            &found,
            &Claim {
                level: EvidenceLevel::Discovered,
                limitations,
                parents: Vec::new(),
            },
        )?;
        out.found(surface.clone())?;
        if let Standing::SignIn(observation) = &arrival.standing {
            let method = match *observation {
                "status_401" => "challenge",
                "other_origin" => "redirect",
                _ => "form",
            };
            let found = object([
                ("format", Json::from("1")),
                ("method", Json::from(method)),
                ("observation", Json::from(*observation)),
                ("recommended_mode", Json::Null),
            ]);
            out.found(core.finding(
                scope,
                arrival.asked.subject(scope.redact)?,
                &kinds.sign_in_method,
                &found,
                &Claim {
                    level: EvidenceLevel::Inferred,
                    limitations: vec![(
                        LimitationCode::NotExercised,
                        "The sign-in was seen and not tried.".to_owned(),
                    )],
                    parents: vec![&surface],
                },
            )?)?;
        }
        Ok(DiscoveryReport {
            coverage: Coverage::Partial,
            ..DiscoveryReport::default()
        })
    }
}

impl Discoverer for WebProbe {
    fn name(&self) -> DiscovererName {
        self.1.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        suits_web(surface)
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(self.look(scope, out))
    }
}

// ---- web_auth ----

/// The names of the signs of a sign-in that `web_auth` classifies, as the
/// finding's `sign_in` holds them. They are the session keeper's names
/// for them (`a2m_auth::SignInSign`), which turns them into a
/// recommendation; a test of the product holds the two lists equal.
pub const SIGN_IN_SIGNS: &[&str] = &["password_form", "other_origin", "challenge", "code_step"];

/// The words of an address that reads as a sign-in page. A page at such an
/// address that shows no field this pack classifies is a sign-in no rule
/// covers: `sign_in_unrecognized`, never a guess (rule 7).
pub const SIGN_IN_WORDS: &[&str] = &["sign-in", "signin", "sign_in", "login", "log-in", "logon"];

/// Whether a path reads as a sign-in page.
fn reads_as_a_sign_in(path: &str) -> bool {
    path.to_ascii_lowercase()
        .split(['/', '?', '#', '.'])
        .any(|part| SIGN_IN_WORDS.contains(&part))
}

/// `web_auth`: how the surface's start page asks for a sign-in, from the
/// page's structure and where it ended alone (decisions 0254 and 0321).
/// It classifies a password form, a move to another origin, an answer of
/// status 401, and a field for a one-time code. It reads no field's value,
/// types nothing, and sends no form: its tab is an observing tab, which
/// fails every request that is not a read. A second factor that does not
/// show on the page is `unknown`, never guessed.
pub(crate) struct WebAuth(pub(crate) Arc<Parts>, pub(crate) DiscovererName);

impl WebAuth {
    async fn look(
        &self,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let Parts { core, kinds } = &*self.0;
        let (address, start) = Core::start(scope)?;
        let tab = core.tab(scope).await?;
        let arrival = core.arrive(&tab, &address, &start, scope).await?;
        let code_field = || async {
            crate::look::present(
                &tab,
                &crate::look::css("input[autocomplete=one-time-code]"),
                scope,
            )
            .await
        };
        let (sign, code) = match &arrival.standing {
            Standing::SignIn("other_origin") => ("other_origin", false),
            Standing::SignIn("status_401") => ("challenge", false),
            Standing::SignIn(_) => ("password_form", code_field().await?),
            Standing::Blocked(code) => {
                return Err(Failure::new(
                    FailureClass::EdgeBlocked,
                    code.clone(),
                    "The surface stopped answering as itself; how it signs in was not seen.",
                    "Look at the surface in a browser, and ask its owner before trying again.",
                ));
            }
            Standing::Open if code_field().await? => ("code_step", true),
            Standing::Open if reads_as_a_sign_in(&arrival.ended.path) => {
                return Err(Failure::new(
                    FailureClass::UnknownVariant,
                    codes::SIGN_IN_UNRECOGNIZED,
                    "The surface's start page reads as a sign-in, and shows neither a password field, nor a field for a one-time code, nor a move to another origin: no rule of this pack classifies it.",
                    "Say how the surface signs in: declare it in `auth.yaml` with `draft.write`, or ask a person which method it is.",
                ));
            }
            Standing::Open => {
                // Nothing asks for a sign-in: there is nothing to classify.
                return Ok(DiscoveryReport {
                    coverage: Coverage::Partial,
                    ..DiscoveryReport::default()
                });
            }
        };
        let identity_origin = if sign == "other_origin" {
            Json::from(arrival.ended.origin.clone())
        } else {
            Json::Null
        };
        let found = object([
            ("format", Json::from("1")),
            ("sign_in", Json::from(sign)),
            (
                "second_factor",
                Json::from(if code { "asked" } else { "unknown" }),
            ),
            (
                "redirected",
                Json::Bool(arrival.asked.plain() != arrival.ended.plain()),
            ),
            ("identity_origin", identity_origin),
            ("status", arrival.status.map_or(Json::Null, Json::from)),
            ("submitted", Json::Bool(false)),
        ]);
        let mut limitations = vec![(
            LimitationCode::NotExercised,
            "The sign-in was classified from the page's structure and where it ended; no field was read and nothing was sent.".to_owned(),
        )];
        if !code {
            limitations.push((
                LimitationCode::SingleVariant,
                "A second factor, a passkey, or a link sent by mail does not show before a sign-in is sent: whether one follows is not known, and a person says.".to_owned(),
            ));
        }
        out.found(core.finding(
            scope,
            arrival.asked.subject(scope.redact)?,
            &kinds.sign_in_method,
            &found,
            &Claim {
                level: EvidenceLevel::Inferred,
                limitations,
                parents: Vec::new(),
            },
        )?)?;
        Ok(DiscoveryReport {
            coverage: Coverage::Partial,
            ..DiscoveryReport::default()
        })
    }
}

impl Discoverer for WebAuth {
    fn name(&self) -> DiscovererName {
        self.1.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        suits_web(surface)
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(self.look(scope, out))
    }
}

// ---- the pages of one discovery ----

/// The pages to look at, in order, each once, within the page budget and
/// the time bound.
struct Pages {
    queue: VecDeque<(Address, u32, bool)>,
    seen: BTreeSet<String>,
    looked: u64,
    report: DiscoveryReport,
}

impl Pages {
    fn new(scope: &DiscoveryScope<'_>) -> Result<(Self, Place), Failure> {
        let (address, start) = Core::start(scope)?;
        let mut pages = Self {
            queue: VecDeque::new(),
            seen: BTreeSet::new(),
            looked: 0,
            report: DiscoveryReport {
                coverage: Coverage::Partial,
                ..DiscoveryReport::default()
            },
        };
        pages.add(address, 0, false);
        for page in Core::focus_pages(scope, &start)? {
            pages.add(page, 0, true);
        }
        Ok((pages, start))
    }

    fn add(&mut self, address: Address, depth: u32, focused: bool) {
        if self.seen.insert(address.as_str().to_owned()) {
            self.queue.push_back((address, depth, focused));
        }
    }

    /// Whether a page that was asked for at `asked` and ended at `ended` is
    /// a page of its own. A page that redirected to an address already
    /// looked at, or waiting to be, is that page and not another: it is
    /// looked at once, under the address it ended at, so that one page seen
    /// twice is never taken for two pages of one screen at addresses that
    /// differ in a key. The address it ended at is then counted as seen, so
    /// a link to it is not followed again.
    fn own_page(&mut self, asked: &Address, ended: Option<&str>) -> bool {
        match ended {
            Some(ended) if ended != asked.as_str() => self.seen.insert(ended.to_owned()),
            _ => true,
        }
    }

    /// The next page, or nothing when none is left or a bound was hit. A
    /// bound that is hit is named, and what was left is counted.
    fn next(&mut self, max_pages: u64, scope: &DiscoveryScope<'_>) -> Option<(Address, u32, bool)> {
        let bound = if self.queue.is_empty() {
            return None;
        } else if self.looked >= max_pages {
            "pages"
        } else if scope.deadline.passed() {
            "seconds"
        } else {
            self.looked += 1;
            return self.queue.pop_front();
        };
        self.report.bounds_hit.push(bound.to_owned());
        self.report.dropped += u64::try_from(self.queue.len()).unwrap_or(u64::MAX);
        self.queue.clear();
        None
    }
}

/// Waits for the focus's ready text on a page the focus named.
async fn ready(
    tab: &ObserveTab,
    focused: bool,
    scope: &DiscoveryScope<'_>,
    limitations: &mut Vec<(LimitationCode, String)>,
) -> Result<(), Failure> {
    if let (true, Some(text)) = (focused, Core::ready_text(scope))
        && !Core::ready(tab, text, scope).await?
    {
        limitations.push((
            LimitationCode::TimeLimit,
            "The page did not show the ready text in time; it was read as it stood.".to_owned(),
        ));
    }
    Ok(())
}

// ---- web_snapshot ----

/// One page a snapshot was taken of: where it ended, what was found, and
/// why that is weaker than it looks.
pub(crate) type Looked = (Place, Json, Vec<(LimitationCode, String)>);

/// `web_snapshot`: the screens of the surface's start page, of the pages
/// the focus names, and of the pages plain links lead to, as deep as the
/// bounds allow.
pub(crate) struct WebSnapshot(pub(crate) Arc<Parts>, pub(crate) DiscovererName);

/// What became of the links of one page.
#[derive(Default)]
struct Links {
    follow: Vec<Address>,
    /// Links whose address or name looks like an action.
    refused: u64,
    /// Links that share a name, or could not be read.
    unread: u64,
    /// Links that lead outside the surface, or nowhere.
    elsewhere: u64,
}

impl WebSnapshot {
    async fn links(
        tab: &ObserveTab,
        names: &[&str],
        page: &Place,
        scope: &DiscoveryScope<'_>,
    ) -> Result<Links, Failure> {
        let mut links = Links::default();
        for name in names {
            let element = match tab.locate(&link(name), scope.deadline).await {
                Ok(element) => element,
                Err(BrowserError::NotFound | BrowserError::Ambiguous { .. }) => {
                    links.unread += 1;
                    continue;
                }
                Err(error) => return Err(failure(&error)),
            };
            let what = ReadWhat::Attribute {
                name: "href".to_owned(),
            };
            let href = match tab.read(&element, &what, scope.deadline).await {
                Ok(RawValue::Text(href)) => href,
                Ok(RawValue::Flag(_) | RawValue::Absent)
                | Err(BrowserError::NotFound | BrowserError::TooLarge { .. }) => {
                    links.unread += 1;
                    continue;
                }
                Err(error) => return Err(failure(&error)),
            };
            if looks_like_an_action(&href) || looks_like_an_action(name) {
                links.refused += 1;
            } else if let Some(target) = link_target(&href, page) {
                links.follow.push(target);
            } else {
                links.elsewhere += 1;
            }
        }
        Ok(links)
    }

    async fn look(
        &self,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let Parts { core, kinds } = &*self.0;
        let (mut pages, start) = Pages::new(scope)?;
        let mut out = Once::new(out);
        let tab = core.tab(scope).await?;
        let mut seen = Vec::new();
        while let Some((address, depth, focused)) = pages.next(core.max_pages, scope) {
            let arrival: Arrival = core.arrive(&tab, &address, &start, scope).await?;
            if depth == 0 {
                arrival.must_be_open()?;
            } else if arrival.standing != Standing::Open {
                // A page a link led to that does not stand open is not
                // read, and the discovery carries on with the others.
                pages.report.dropped += 1;
                continue;
            }
            if !pages.own_page(&address, arrival.ended_address.as_deref()) {
                continue;
            }
            let mut limitations = vec![single_variant("The screen")];
            ready(&tab, focused, scope, &mut limitations).await?;
            let snapshot = tab
                .snapshot(&SnapshotLimits::default(), scope.deadline)
                .await
                .map_err(|error| failure(&error))?;
            if snapshot.cut {
                limitations.push((
                    LimitationCode::Truncated,
                    "The page holds more elements than a snapshot keeps.".to_owned(),
                ));
            }
            let (names, shared) = screen::link_names(&snapshot);
            if depth < scope.bounds.depth {
                let links = Self::links(&tab, &names, &arrival.ended, scope).await?;
                if links.refused > 0 {
                    limitations.push((
                        LimitationCode::NotExercised,
                        format!(
                            "{} link(s) look like an action and were not followed.",
                            links.refused
                        ),
                    ));
                }
                pages.report.dropped += links.refused + links.unread + links.elsewhere + shared;
                for target in links.follow {
                    pages.add(target, depth + 1, false);
                }
            }
            let (outside, others) = outside_origins(tab.blocked());
            limitations.extend(refused_note(&others));
            limitations.extend(outside_note(&outside));
            let mut found = screen::found(&snapshot, &arrival.title);
            if !outside.is_empty()
                && let Json::Object(fields) = &mut found
            {
                fields.insert(
                    REFUSED_ORIGINS.to_owned(),
                    Json::Array(outside.iter().map(|one| Json::from(one.as_str())).collect()),
                );
                *fields = crate::shape::sorted(std::mem::take(fields));
            }
            seen.push((arrival.ended, found, limitations));
        }
        Self::give(core, kinds, scope, &seen, &mut out)?;
        Ok(pages.report)
    }

    /// The `screen` findings of the pages looked at, in the order they
    /// were looked at: one for each page, and one for each group of pages
    /// that show one screen at addresses that differ in one segment, in
    /// place of those pages' own (screen matching).
    pub(crate) fn give(
        core: &Core,
        kinds: &Kinds,
        scope: &DiscoveryScope<'_>,
        seen: &[Looked],
        out: &mut Once<'_>,
    ) -> Result<(), Failure> {
        let listed: Vec<(&str, &str, &Json)> = seen
            .iter()
            .map(|(place, found, _)| (place.origin.as_str(), place.path.as_str(), found))
            .collect();
        let groups = screen::matched(&listed);
        for (at, (place, found, limitations)) in seen.iter().enumerate() {
            let (subject, found, limitations) = match groups
                .iter()
                .find(|group| group.pages.contains(&at))
            {
                // Given once, where its first page was looked at.
                Some(group) if group.pages.iter().min() != Some(&at) => continue,
                Some(group) => {
                    let first = &seen[group.pages[0]];
                    let mut first_found = first.1.clone();
                    let record_form =
                        screen::signature(&first.1).is_some_and(|sig| screen::of_a_record(&sig));
                    if record_form {
                        let pages: Vec<&Json> =
                            group.pages.iter().map(|page| &seen[*page].1).collect();
                        screen::mark_varying(&mut first_found, &pages);
                    }
                    let mut fields = first_found.as_object().cloned().unwrap_or_default();
                    // Every page of the group may have tried other origins.
                    let refused: BTreeSet<&str> = group
                        .pages
                        .iter()
                        .filter_map(|page| seen[*page].1.get(REFUSED_ORIGINS))
                        .filter_map(Json::as_array)
                        .flatten()
                        .filter_map(Json::as_str)
                        .collect();
                    if !refused.is_empty() {
                        fields.insert(
                            REFUSED_ORIGINS.to_owned(),
                            Json::Array(refused.into_iter().map(Json::from).collect()),
                        );
                    }
                    fields.insert(
                        "pages".to_owned(),
                        Json::Array(
                            group
                                .pages
                                .iter()
                                .map(|page| Json::from(seen[*page].0.path.as_str()))
                                .collect(),
                        ),
                    );
                    let found = Json::Object(crate::shape::sorted(fields));
                    let mut limitations =
                        union(group.pages.iter().map(|page| seen[*page].2.as_slice()));
                    limitations.push((
                            LimitationCode::Sampled,
                            format!(
                                "Matched from {} pages whose addresses differ in one segment and whose {} the same; the snapshot is of the first.",
                                group.pages.len(),
                                if record_form {
                                    "structures are, but for the names of their headings, which are kept as types where they differ"
                                } else {
                                    "tables and labelled values are"
                                }
                            ),
                        ));
                    let pattern = Place {
                        origin: place.origin.clone(),
                        path: group.pattern.clone(),
                        query: BTreeMap::new(),
                        query_left_out: false,
                    };
                    (pattern.subject(scope.redact)?, found, limitations)
                }
                None => (
                    place.subject(scope.redact)?,
                    found.clone(),
                    limitations.clone(),
                ),
            };
            out.found(&core.finding(
                scope,
                subject,
                &kinds.screen,
                &found,
                &Claim {
                    level: EvidenceLevel::Discovered,
                    limitations,
                    parents: Vec::new(),
                },
            )?)?;
        }
        Ok(())
    }
}

impl Discoverer for WebSnapshot {
    fn name(&self) -> DiscovererName {
        self.1.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        suits_web(surface)
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(self.look(scope, out))
    }
}

// ---- web_capture ----

/// `web_capture`: the exchanges of the pages it is sent to, by structure,
/// and the endpoints they group into. An exchange is the request for a
/// document the tab was sent to, with the shape of a JSON answer, or a
/// request the page tried and the tab refused, or a request a page's own
/// script made with GET to the surface's origin, as the observing tab
/// recorded it, which names the page that made it. A request of a script
/// to another origin is not followed (decision 0187).
pub(crate) struct WebCapture(pub(crate) Arc<Parts>, pub(crate) DiscovererName);

impl WebCapture {
    async fn look(
        &self,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let Parts { core, kinds } = &*self.0;
        let (mut pages, start) = Pages::new(scope)?;
        let mut out = Once::new(out);
        let tab = core.tab(scope).await?;
        // The page's own requests reach this discoverer through the
        // contract's traffic capture, and pass its check (decision 0391).
        TrafficCapture::start(&tab);
        let mut exchanges: Vec<(Exchange, Vec<(LimitationCode, String)>)> = Vec::new();
        while let Some((address, _, focused)) = pages.next(core.max_pages, scope) {
            let arrival = core.arrive(&tab, &address, &start, scope).await?;
            arrival.must_be_open()?;
            let mut limitations = vec![(
                LimitationCode::SingleVariant,
                "One answer was seen, signed in as nobody. Its headers are not read.".to_owned(),
            )];
            ready(&tab, focused, scope, &mut limitations).await?;
            let answer = match json_body(&tab, scope).await? {
                Some(json) => Answer::Json(Shape::of(&json)),
                None => Answer::Document,
            };
            if matches!(&answer, Answer::Json(shape) if shape.withheld()) {
                limitations.push(dictionary());
            }
            let mut things = Vec::new();
            let mut refused = Vec::new();
            for one in tab.blocked() {
                if !METHODS.contains(&one.method.as_str()) {
                    things.push(one);
                    continue;
                }
                match Place::parse(&one.address) {
                    Some(place) if place.origin == start.origin => refused.push(Exchange {
                        method: one.method,
                        origin: place.origin,
                        path: place.path,
                        query: BTreeMap::new(),
                        status: None,
                        answer: Answer::Refused,
                        made_by: None,
                        credential_headers: Vec::new(),
                        set_headers: Vec::new(),
                        query_left_out: false,
                        body: None,
                    }),
                    // A request to a system that is not this surface.
                    _ => pages.report.dropped += 1,
                }
            }
            limitations.extend(refused_note(&things));
            if arrival.ended.query_left_out {
                limitations.push(query_left_out());
            }
            // What the page asked for by itself, with GET, from its own
            // origin, as the tab recorded it: names and kinds only. The wait
            // for its answers is bounded for each page, so that one request
            // that never ends does not hold the whole discovery.
            let until = Deadline::after(PAGE_WAIT.min(scope.deadline.remaining()));
            let taken = take_capture(&tab, until, scope.redact).await;
            if taken.withheld > 0 {
                pages.report.dropped += taken.withheld as u64;
                limitations.push((
                    LimitationCode::NotExercised,
                    format!(
                        "{} request(s) the page made by itself are left out whole: their method or origin held a secret or was not of its form.",
                        taken.withheld
                    ),
                ));
            }
            if taken.awaited > 0 {
                limitations.push((
                    LimitationCode::NotExercised,
                    format!(
                        "{} request(s) the page made by itself had not been answered within {} seconds of its load, and are left out.",
                        taken.awaited,
                        PAGE_WAIT.as_secs()
                    ),
                ));
            }
            if taken.dropped > 0 {
                limitations.push((
                    LimitationCode::Truncated,
                    format!(
                        "{} request(s) the page made by itself are left out: the tab keeps at most {EXCHANGES_KEPT} at once.",
                        taken.dropped
                    ),
                ));
            }
            exchanges.push((
                Exchange {
                    method: "GET".to_owned(),
                    origin: arrival.ended.origin.clone(),
                    path: arrival.ended.path.clone(),
                    query: arrival.ended.query.clone(),
                    status: arrival.status,
                    answer,
                    made_by: None,
                    credential_headers: Vec::new(),
                    set_headers: Vec::new(),
                    query_left_out: arrival.ended.query_left_out,
                    body: None,
                },
                limitations,
            ));
            for own in taken.requests {
                if own.origin != start.origin {
                    pages.report.dropped += 1;
                    continue;
                }
                let mut limitations = vec![(
                    LimitationCode::SingleVariant,
                    "A request the page made by itself, seen once, signed in as nobody. Its query and its answer are kept as names, the answer's to one level inside an object.".to_owned(),
                )];
                if !own.credential_headers.is_empty() {
                    limitations.push((
                        LimitationCode::NotExercised,
                        format!(
                            "The request carried {} header(s) that hold a credential or an anti-forgery token; they are named, and their values were not kept.",
                            own.credential_headers.len()
                        ),
                    ));
                }
                if !own.set_headers.is_empty() {
                    limitations.push((
                        LimitationCode::NotExercised,
                        format!(
                            "The page's script set {} header(s) of the request; they are named, their values were not kept, and a read made again from inside the page would not send them.",
                            own.set_headers.len()
                        ),
                    ));
                }
                if own.query_left_out {
                    limitations.push(query_left_out());
                }
                if own.headers_left_out {
                    limitations.push((
                        LimitationCode::NotExercised,
                        "A header of the request is left out: its name held a secret or was not a plain name.".to_owned(),
                    ));
                }
                if own.names_withheld {
                    limitations.push(dictionary());
                }
                let answer = match &own.fields {
                    Some(fields) => Answer::Json(Shape::of_fields(fields)),
                    None => Answer::Document,
                };
                exchanges.push((
                    Exchange {
                        method: own.method,
                        origin: own.origin,
                        path: own.path,
                        query: own
                            .query
                            .into_iter()
                            .map(|name| (name, "unknown"))
                            .collect(),
                        status: Some(own.status),
                        answer,
                        made_by: Some(arrival.ended.path.clone()),
                        credential_headers: own.credential_headers,
                        set_headers: own.set_headers,
                        query_left_out: own.query_left_out,
                        body: None,
                    },
                    limitations,
                ));
            }
            for one in refused {
                exchanges.push((
                    one,
                    vec![(
                        LimitationCode::NotExercised,
                        "The page tried this request and the observing tab refused it before it left the browser. The system never answered it.".to_owned(),
                    )],
                ));
            }
        }

        // Requests a page made by itself that are one read by key are given
        // as one finding, at the place of the first, with the segment that
        // differs as a placeholder, and the requests' own are not given
        // (decision 0190).
        let only: Vec<Exchange> = exchanges.iter().map(|(one, _)| one.clone()).collect();
        let groups = matched_requests(&only);
        // The finding each exchange is given in, by its place, for the
        // parents of the endpoints below.
        let mut given: Vec<Option<Finding>> = vec![None; exchanges.len()];
        for (at, (exchange, limitations)) in exchanges.iter().enumerate() {
            let group = groups.iter().find(|group| group.members.contains(&at));
            if let Some(group) = group
                && group.members.first() != Some(&at)
            {
                continue;
            }
            let (path, found, limitations) = match group {
                Some(group) => {
                    let mut found = exchange.found();
                    if let Json::Object(fields) = &mut found {
                        fields.insert("path".to_owned(), Json::from(group.pattern.as_str()));
                        fields.insert("made_by".to_owned(), Json::from(group.page.as_str()));
                        fields.insert(
                            "paths".to_owned(),
                            Json::Array(
                                group
                                    .members
                                    .iter()
                                    .map(|member| Json::from(only[*member].path.as_str()))
                                    .collect(),
                            ),
                        );
                        found = Json::Object(crate::shape::sorted(fields.clone()));
                    }
                    let mut limitations = union(
                        group
                            .members
                            .iter()
                            .map(|member| exchanges[*member].1.as_slice()),
                    );
                    limitations.push((
                        LimitationCode::Sampled,
                        format!(
                            "Matched from {} requests whose paths differ in one segment, made by pages whose paths differ in the same value, with one answer's shape; the names are of the first.",
                            group.members.len()
                        ),
                    ));
                    (group.pattern.clone(), found, limitations)
                }
                None => (exchange.path.clone(), exchange.found(), limitations.clone()),
            };
            let place = Place {
                origin: exchange.origin.clone(),
                path,
                query: BTreeMap::new(),
                query_left_out: false,
            };
            let finding = core.finding(
                scope,
                place.subject(scope.redact)?,
                &kinds.exchange,
                &found,
                &Claim {
                    level: if exchange.answered() {
                        EvidenceLevel::Discovered
                    } else {
                        EvidenceLevel::Inferred
                    },
                    limitations,
                    parents: Vec::new(),
                },
            )?;
            out.found(&finding)?;
            match group {
                Some(group) => {
                    for member in &group.members {
                        given[*member] = Some(finding.clone());
                    }
                }
                None => given[at] = Some(finding),
            }
        }
        let tier_two = matches!(scope.focus.get(FOCUS_TIER_TWO), Some(Value::Bool(true)));
        for endpoint in infer_endpoints(&only, tier_two) {
            let mut limitations = vec![(
                LimitationCode::Sampled,
                format!(
                    "Made from {} exchange(s) of one capture. Of the requests a page's own scripts make, only those with GET to the surface's origin are seen.",
                    endpoint.seen
                ),
            )];
            if !endpoint.answered() {
                limitations.push((
                    LimitationCode::NotExercised,
                    "The system never answered this endpoint: the request was refused before it left the browser.".to_owned(),
                ));
            }
            if endpoint.has_tier_two() {
                limitations.push((
                    LimitationCode::Sampled,
                    "A parameter of the path template rests on variance alone: the segment differed between otherwise identical requests.".to_owned(),
                ));
            }
            if endpoint.keeps_a_short_number() {
                limitations.push((
                    LimitationCode::Sampled,
                    format!(
                        "A number of fewer than {} digits in the path is kept as written: a short number is a parameter only where requests showed it varying.",
                        crate::infer::LEAST_DIGITS
                    ),
                ));
            }
            if endpoint.response.as_ref().is_some_and(Shape::has_optional) {
                limitations.push((
                    LimitationCode::Sampled,
                    "A field was in some answers and not in others; it is marked optional."
                        .to_owned(),
                ));
            }
            if endpoint.response.as_ref().is_some_and(Shape::withheld) {
                limitations.push(dictionary());
            }
            if endpoint.response.as_ref().is_some_and(Shape::cut) {
                limitations.push((
                    LimitationCode::Truncated,
                    "An answer holds more names than a shape keeps.".to_owned(),
                ));
            }
            let mut parents: Vec<&Finding> = Vec::new();
            for source in &endpoint.sources {
                if let Some(parent) = given.get(*source).and_then(Option::as_ref)
                    && !parents.iter().any(|known| known.id() == parent.id())
                {
                    parents.push(parent);
                }
            }
            let place = Place {
                origin: endpoint.origin.clone(),
                path: endpoint.template(),
                query: BTreeMap::new(),
                query_left_out: false,
            };
            out.found(&core.finding(
                scope,
                place.subject(scope.redact)?,
                &kinds.endpoint,
                &endpoint.found(),
                &Claim {
                    level: endpoint.level(),
                    limitations,
                    parents,
                },
            )?)?;
        }
        Ok(pages.report)
    }
}

impl Discoverer for WebCapture {
    fn name(&self) -> DiscovererName {
        self.1.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        suits_web(surface)
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(self.look(scope, out))
    }
}
