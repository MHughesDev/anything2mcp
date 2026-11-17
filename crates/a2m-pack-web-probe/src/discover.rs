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

use a2m_core::{
    Coverage, Digest, EvidenceLevel, Failure, Finding, FindingKind, LimitationCode, Value,
};
use a2m_pack_web_browser::{
    Address, BrowserError, Look, ObserveTab, RawValue, ReadWhat, RequestShape, SnapshotLimits,
};
use a2m_provider::{
    BoxFuture, Discoverer, DiscovererName, DiscoveryReport, DiscoveryScope, FindingSink,
    SandboxPort, SurfaceDescription,
};
use serde_json::Value as Json;

use crate::SURFACE_KIND;
use crate::infer::{Answer, Exchange, infer_endpoints};
use crate::look::{
    Arrival, Claim, Core, FOCUS_TIER_TWO, Place, Standing, failure, json_body, link, link_target,
    looks_like_an_action,
};
use crate::screen;
use crate::shape::{Shape, object};

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

fn suits_web(surface: &SurfaceDescription) -> bool {
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
    screen: FindingKind,
    exchange: FindingKind,
    endpoint: FindingKind,
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
struct Once<'a> {
    out: &'a mut dyn FindingSink,
    sent: BTreeSet<Digest>,
}

impl<'a> Once<'a> {
    fn new(out: &'a mut dyn FindingSink) -> Self {
        Self {
            out,
            sent: BTreeSet::new(),
        }
    }

    fn found(&mut self, finding: &Finding) -> Result<(), Failure> {
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
                .map_err(|error| failure(&error, scope))?;
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
                Err(error) => return Err(failure(&error, scope)),
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
                Err(error) => return Err(failure(&error, scope)),
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
            let mut limitations = vec![single_variant("The screen")];
            ready(&tab, focused, scope, &mut limitations).await?;
            let snapshot = tab
                .snapshot(&SnapshotLimits::default(), scope.deadline)
                .await
                .map_err(|error| failure(&error, scope))?;
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
            limitations.extend(refused_note(&tab.blocked()));
            out.found(&core.finding(
                scope,
                arrival.ended.subject(scope.redact)?,
                &kinds.screen,
                &screen::found(&snapshot, &arrival.title),
                &Claim {
                    level: EvidenceLevel::Discovered,
                    limitations,
                    parents: Vec::new(),
                },
            )?)?;
        }
        Ok(pages.report)
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
/// request the page tried and the tab refused. The requests a page's own
/// scripts read with are not seen: the base has no stream of exchanges
/// yet.
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
                    }),
                    // A request to a system that is not this surface.
                    _ => pages.report.dropped += 1,
                }
            }
            limitations.extend(refused_note(&things));
            exchanges.push((
                Exchange {
                    method: "GET".to_owned(),
                    origin: arrival.ended.origin.clone(),
                    path: arrival.ended.path.clone(),
                    query: arrival.ended.query.clone(),
                    status: arrival.status,
                    answer,
                },
                limitations,
            ));
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

        let mut findings = Vec::new();
        for (exchange, limitations) in &exchanges {
            let place = Place {
                origin: exchange.origin.clone(),
                path: exchange.path.clone(),
                query: BTreeMap::new(),
            };
            let finding = core.finding(
                scope,
                place.subject(scope.redact)?,
                &kinds.exchange,
                &exchange.found(),
                &Claim {
                    level: if exchange.answered() {
                        EvidenceLevel::Discovered
                    } else {
                        EvidenceLevel::Inferred
                    },
                    limitations: limitations.clone(),
                    parents: Vec::new(),
                },
            )?;
            out.found(&finding)?;
            findings.push(finding);
        }

        let tier_two = matches!(scope.focus.get(FOCUS_TIER_TWO), Some(Value::Bool(true)));
        let only: Vec<Exchange> = exchanges
            .into_iter()
            .map(|(exchange, _)| exchange)
            .collect();
        for endpoint in infer_endpoints(&only, tier_two) {
            let mut limitations = vec![(
                LimitationCode::Sampled,
                format!(
                    "Made from {} exchange(s) of one capture. The requests a page's own scripts read with are not seen.",
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
            if endpoint.response.as_ref().is_some_and(Shape::has_optional) {
                limitations.push((
                    LimitationCode::Sampled,
                    "A field was in some answers and not in others; it is marked optional."
                        .to_owned(),
                ));
            }
            if endpoint.response.as_ref().is_some_and(Shape::cut) {
                limitations.push((
                    LimitationCode::Truncated,
                    "An answer holds more names than a shape keeps.".to_owned(),
                ));
            }
            let mut parents: Vec<&Finding> = Vec::new();
            for source in &endpoint.sources {
                if let Some(parent) = findings.get(*source)
                    && !parents.iter().any(|known| known.id() == parent.id())
                {
                    parents.push(parent);
                }
            }
            let place = Place {
                origin: endpoint.origin.clone(),
                path: endpoint.template(),
                query: BTreeMap::new(),
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
