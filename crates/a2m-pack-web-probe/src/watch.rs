//! `web_use`: the discoverer a person drives (decision 0392, points 1 to 4;
//! package E2).
//!
//! It is called only by a capture job, through `DiscovererHandle::watch`,
//! once the person has begun the capture on the engine's own page. It
//! opens a watching tab of the base in a window, on the surface's own
//! address, and sends nothing after that: the tab has no method that
//! presses, types, navigates, or sends. Every request in the tab is the
//! person's, and none is failed for its method. It reads the tab only
//! through the contract's `take_capture`, so every request it keeps has
//! passed the contract's floor and the redactor, and the snapshots the tab
//! takes of each screen the person comes to, which keep no value.
//!
//! When the capture ends, or the person closes the window, it gives:
//!
//! - an `exchange` finding for each request the pages sent, every method,
//!   by structure: requests of one method to one path from one page are
//!   one finding, which says how often they were seen (`seen`) and in how
//!   many shapes of body and answer (`shapes`); requests a page made by
//!   itself that differ in one segment with their page are one read by key,
//!   as `web_capture` matches them (decision 0190);
//! - a `screen` finding for each screen, as `web_snapshot` gives one, marked
//!   as seen as a person used it (`seen_by`), so that what is drafted from
//!   it is held;
//! - an `endpoint` finding for each endpoint those requests group into,
//!   by the inference `web_capture` uses, with the shape of the bodies
//!   they sent.
//!
//! An exchange is `discovered`: the system answered. An endpoint whose path
//! holds a parameter is `inferred`, with the limitation `sampled` (decision
//! 0392, point 3). Every finding has coverage `partial`.
//!
//! The page that sent a request is taken as the screen the person was on
//! when the capture took the request: the requests are taken each time the
//! tab has taken the snapshot of a screen, which it does once the screen's
//! own requests have ended. A request sent as the person moved on, such as
//! a save, may be put to the screen they came to, and each finding says
//! so.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{EvidenceLevel, Failure, Finding, LimitationCode};
use a2m_pack_web_browser::{Deadline, SCREENS_KEPT, Snapshot, WatchTab};
use a2m_provider::{
    BoxFuture, CaptureEnd, CapturedRequest, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, SurfaceDescription, take_capture,
};
use serde_json::Value as Json;

use crate::discover::{
    Kinds, Looked, Once, Parts, WebSnapshot, dictionary, query_left_out, suits_web, union,
};
use crate::infer::{Answer, Body, Exchange, infer_endpoints, matched_requests};
use crate::look::{Claim, Core, Place};
use crate::screen;
use crate::shape::Shape;

/// The name, in a `screen` finding of this discoverer, that says the
/// screen was seen as a person used it, and the value it holds.
pub const SEEN_BY: (&str, &str) = ("seen_by", "person");

/// How long the window is given to open.
const OPEN_WAIT: Duration = Duration::from_secs(60);

/// How long the requests of a screen are waited for, at most, once its
/// snapshot is taken. The tab takes it after they have ended, so this is
/// only for one that started late.
const TAKE_WAIT: Duration = Duration::from_secs(2);

/// How long the last requests are waited for once the capture has ended:
/// well inside the time the contract gives a discoverer to return.
const LAST_WAIT: Duration = Duration::from_millis(1500);

type Noted = Vec<(LimitationCode, String)>;

/// What the capture has taken so far.
struct Taken {
    /// Each request, with the page it is put to, what it is noted for, and
    /// the visit it was taken on: how many screens the tab had taken then.
    requests: Vec<(Exchange, Noted, usize)>,
    /// Each screen, as `web_snapshot` keeps one.
    screens: Vec<Looked>,
    /// The path of the screen the person is on, by the last snapshot.
    current: String,
    origin: String,
    /// Whether the tab came to a screen past the most it keeps.
    screens_bound: bool,
    report: DiscoveryReport,
}

fn attributed() -> (LimitationCode, String) {
    (
        LimitationCode::Sampled,
        "The page that sent it is taken as the screen the person was on when the capture took the request; a request sent as the person moved to another screen may be put to that one.".to_owned(),
    )
}

/// A request of the capture as an exchange, put to the page `page`.
fn exchange(own: CapturedRequest, page: &str) -> (Exchange, Noted) {
    let mut noted = vec![
        (
            LimitationCode::SingleVariant,
            "A request sent while a person used the application in the watching window, recorded by structure: its query, its body, and its answer are kept as names and kinds to one level inside an object, never a value.".to_owned(),
        ),
        attributed(),
    ];
    if !own.credential_headers.is_empty() {
        noted.push((
            LimitationCode::NotExercised,
            format!(
                "The request carried {} header(s) that hold a credential or an anti-forgery token; they are named, and their values were not kept.",
                own.credential_headers.len()
            ),
        ));
    }
    if !own.set_headers.is_empty() {
        noted.push((
            LimitationCode::NotExercised,
            format!(
                "The page's script set {} header(s) of the request; they are named, their values were not kept, and a read made again from inside the page would not send them.",
                own.set_headers.len()
            ),
        ));
    }
    if own.query_left_out {
        noted.push(query_left_out());
    }
    if own.headers_left_out {
        noted.push((
            LimitationCode::NotExercised,
            "A header of the request is left out: its name held a secret or was not a plain name."
                .to_owned(),
        ));
    }
    if own.names_withheld {
        noted.push(dictionary());
    }
    if own.body_names_withheld {
        noted.push((
            LimitationCode::NotExercised,
            "Names of the body the request sent were withheld: they read as values, or one held a secret. Only the names kept are given.".to_owned(),
        ));
    }
    let body = (!own.body_media_type.is_empty()).then(|| Body {
        media_type: own.body_media_type.clone(),
        fields: own.body_fields.clone(),
        names_withheld: own.body_names_withheld,
    });
    let answer = match &own.fields {
        Some(fields) => Answer::Json(Shape::of_fields(fields)),
        None => Answer::Document,
    };
    (
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
            made_by: Some(page.to_owned()),
            credential_headers: own.credential_headers,
            set_headers: own.set_headers,
            query_left_out: own.query_left_out,
            body,
        },
        noted,
    )
}

/// A screen the person came to, as `web_snapshot` keeps one, marked as
/// seen in a person's use.
fn looked(snapshot: &Snapshot) -> Option<Looked> {
    let place = Place::parse(&snapshot.address)?;
    let mut found = screen::found(snapshot, &snapshot.title);
    if let Json::Object(fields) = &mut found {
        fields.insert(SEEN_BY.0.to_owned(), Json::from(SEEN_BY.1));
        found = Json::Object(crate::shape::sorted(std::mem::take(fields)));
    }
    let mut noted = vec![(
        LimitationCode::SingleVariant,
        "The screen was seen while a person used the application in the watching window, once for each visit, and the engine never opened it itself. What fills it is kept as types.".to_owned(),
    )];
    if snapshot.cut {
        noted.push((
            LimitationCode::Truncated,
            "The page holds more elements than a snapshot keeps.".to_owned(),
        ));
    }
    Some((place, found, noted))
}

impl Taken {
    /// Takes the snapshots the tab has taken since the last call, then the
    /// requests its pages sent, put to the screen the person is on.
    async fn take(&mut self, tab: &WatchTab, scope: &DiscoveryScope<'_>, until: Deadline) {
        let screens = tab.screens();
        self.screens_bound |= screens.bound_hit;
        self.report.dropped += screens.missed as u64;
        for snapshot in &screens.snapshots {
            match looked(snapshot) {
                Some(seen) => {
                    self.current.clone_from(&seen.0.path);
                    self.screens.push(seen);
                }
                None => self.report.dropped += 1,
            }
        }
        let blocked = tab.blocked();
        self.report.dropped += blocked.len() as u64;
        let taken = take_capture(tab, until, scope.redact).await;
        self.report.dropped += (taken.withheld + taken.dropped) as u64;
        for own in taken.requests {
            if own.origin != self.origin {
                self.report.dropped += 1;
                continue;
            }
            let (one, noted) = exchange(own, &self.current);
            self.requests.push((one, noted, self.screens.len()));
        }
    }

    /// The findings, through the sink, each once.
    fn give(
        self,
        core: &Core,
        kinds: &Kinds,
        scope: &DiscoveryScope<'_>,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let mut out = Once::new(out);
        let Self {
            requests,
            mut screens,
            screens_bound,
            report,
            ..
        } = self;
        if screens_bound {
            for (_, _, noted) in &mut screens {
                noted.push((
                    LimitationCode::Truncated,
                    format!(
                        "A capture keeps at most {SCREENS_KEPT} screens; the screens the person came to after that were not kept."
                    ),
                ));
            }
        }
        WebSnapshot::give(core, kinds, scope, &screens, &mut out)?;

        let only: Vec<Exchange> = requests.iter().map(|(one, _, _)| one.clone()).collect();
        let visits: Vec<usize> = requests.iter().map(|(_, _, visit)| *visit).collect();
        // The groups: first the reads by key, then the requests of one
        // method to one path from one page.
        let mut groups: Vec<Group> = matched_requests(&only)
            .into_iter()
            .map(|matched| Group {
                matched: Some((matched.pattern, matched.page)),
                members: matched.members,
            })
            .collect();
        let grouped: BTreeSet<usize> = groups
            .iter()
            .flat_map(|group| group.members.iter().copied())
            .collect();
        for (at, one) in only.iter().enumerate() {
            if grouped.contains(&at) {
                continue;
            }
            let same = groups.iter_mut().find(|group| {
                group.matched.is_none()
                    && group.members.first().is_some_and(|first| {
                        let first = &only[*first];
                        (&first.method, &first.origin, &first.path, &first.made_by)
                            == (&one.method, &one.origin, &one.path, &one.made_by)
                    })
            });
            match same {
                Some(group) => group.members.push(at),
                None => groups.push(Group {
                    matched: None,
                    members: vec![at],
                }),
            }
        }
        groups.sort_by_key(|group| group.members.first().copied());

        let mut given: Vec<Option<Finding>> = vec![None; only.len()];
        for group in &groups {
            let Some(&first) = group.members.first() else {
                continue;
            };
            let exchange = &only[first];
            let seen = group.members.len();
            let shapes: BTreeSet<String> = group
                .members
                .iter()
                .map(|member| only[*member].found().to_string())
                .collect();
            // A page that keeps its state in the browser may ask, on every
            // visit after its first, in a shape that only adds to the first
            // one's: counted as one shape (decision 0998, point 3).
            let kept = (group.matched.is_none() && shapes.len() == 2)
                .then(|| kept_by_the_browser(&group.members, &only, &visits))
                .flatten();
            let shapes = if group.matched.is_some() || kept.is_some() {
                1
            } else {
                shapes.len()
            };
            let mut found = kept.clone().unwrap_or_else(|| exchange.found());
            let mut noted = union(
                group
                    .members
                    .iter()
                    .map(|member| requests[*member].1.as_slice()),
            );
            if kept.is_some() {
                noted.push((
                    LimitationCode::Sampled,
                    "The requests of the first visit to the page came in one shape, and those of every later visit in one that only adds fields to it, as a page does that keeps its state in the browser: they are counted as one shape, the later one, and a field only the later ones held is optional in the answer.".to_owned(),
                ));
            }
            let mut path = exchange.path.clone();
            if let Json::Object(fields) = &mut found {
                if let Some((pattern, page)) = &group.matched {
                    fields.insert("path".to_owned(), Json::from(pattern.as_str()));
                    fields.insert("made_by".to_owned(), Json::from(page.as_str()));
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
                    path.clone_from(pattern);
                    noted.push((
                        LimitationCode::Sampled,
                        format!(
                            "Matched from {seen} requests whose paths differ in one segment, made by pages whose paths differ in the same value, with one answer's shape; the names are of the first."
                        ),
                    ));
                }
                fields.insert("seen".to_owned(), Json::from(seen));
                fields.insert("shapes".to_owned(), Json::from(shapes));
                found = Json::Object(crate::shape::sorted(std::mem::take(fields)));
            }
            noted.push((
                LimitationCode::Sampled,
                format!(
                    "Seen {seen} time(s) in one capture of a person's use, in {shapes} shape(s) of body and answer: a sample of what the application does, not what it promises."
                ),
            ));
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
                    limitations: noted,
                    parents: Vec::new(),
                },
            )?;
            out.found(&finding)?;
            for member in &group.members {
                given[*member] = Some(finding.clone());
            }
        }

        for endpoint in infer_endpoints(&only, false) {
            let mut noted = vec![(
                LimitationCode::Sampled,
                format!(
                    "Made from {} request(s) seen in one capture of a person's use.",
                    endpoint.seen
                ),
            )];
            if endpoint.response.as_ref().is_some_and(Shape::has_optional)
                || endpoint.request.as_ref().is_some_and(Shape::has_optional)
            {
                noted.push((
                    LimitationCode::Sampled,
                    "A field was in some bodies or answers and not in others; it is marked optional.".to_owned(),
                ));
            }
            if endpoint.response.as_ref().is_some_and(Shape::withheld) {
                noted.push(dictionary());
            }
            let mut parents: Vec<&Finding> = Vec::new();
            for source in &endpoint.sources {
                if let Some(parent) = given.get(*source).and_then(Option::as_ref)
                    && !parents.iter().any(|known| known.id() == parent.id())
                {
                    parents.push(parent);
                }
            }
            // A path with a placeholder, and anything drafted from a
            // capture, is `inferred` (decision 0392, point 3).
            let level = if endpoint.has_parameter() {
                EvidenceLevel::Inferred
            } else {
                endpoint.level()
            };
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
                    level,
                    limitations: noted,
                    parents,
                },
            )?)?;
        }
        Ok(report)
    }
}

/// The one shape of a group of requests in two, when the change is the one
/// a page that keeps its state in the browser makes (decision 0998, point 3): the
/// first shape is that of every request of the first visit to the page and
/// of no later one; the second is that of every request of every later
/// visit; and the second differs from the first only in fields it adds, to
/// the body, to the answer, or to both, each field the two share being of
/// one kind. Its found is the second's, with each field of the answer only
/// the second held marked optional. `None` for any other two shapes: a
/// change back, a change within one visit, a field of another kind, names
/// withheld, or anything else that differs.
pub(crate) fn kept_by_the_browser(
    members: &[usize],
    only: &[Exchange],
    visits: &[usize],
) -> Option<Json> {
    let first = *members.first()?;
    let first_found = only[first].found();
    let first_visit = visits[first];
    let mut later: Option<Json> = None;
    for member in members {
        let found = only[*member].found();
        if found == first_found {
            if visits[*member] != first_visit {
                return None;
            }
        } else {
            if visits[*member] == first_visit {
                return None;
            }
            match &later {
                Some(known) if *known != found => return None,
                Some(_) => {}
                None => later = Some(found),
            }
        }
    }
    let later = later?;
    let (Json::Object(a), Json::Object(b)) = (&first_found, &later) else {
        return None;
    };
    // Nothing but the body and the answer differs.
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for key in keys {
        if key != "body" && key != "response" && a.get(key) != b.get(key) {
            return None;
        }
    }
    // The body: one media type, every name kept, and the first's fields
    // among the second's, each of one kind.
    match (a.get("body"), b.get("body")) {
        (None, None) => {}
        (Some(one), Some(two)) if one == two => {}
        (Some(one), Some(two)) => {
            if one.get("media_type") != two.get("media_type")
                || one.get("names_withheld") != Some(&Json::Bool(false))
                || two.get("names_withheld") != Some(&Json::Bool(false))
                || !adds_only(one.get("fields")?, two.get("fields")?)
            {
                return None;
            }
        }
        _ => return None,
    }
    // The answer: the same, or an object whose fields the second adds to.
    let (one, two) = (a.get("response"), b.get("response"));
    let mut found = later.clone();
    if one != two {
        let (one, two) = (one?, two?);
        if one.get("type") != Some(&Json::from("object"))
            || two.get("type") != one.get("type")
            || one.get("names_withheld").is_some()
            || two.get("names_withheld").is_some()
            || !adds_only(one.get("fields")?, two.get("fields")?)
        {
            return None;
        }
        let first_fields = one.get("fields")?.as_object()?;
        if let Some(Json::Object(fields)) = found
            .get_mut("response")
            .and_then(|response| response.get_mut("fields"))
        {
            for (name, field) in fields.iter_mut() {
                if !first_fields.contains_key(name)
                    && let Json::Object(field) = field
                {
                    field.insert("optional".to_owned(), Json::Bool(true));
                }
            }
        }
    }
    Some(found)
}

/// Whether `two`, an object of fields, holds every field of `one` as `one`
/// holds it, and more.
fn adds_only(one: &Json, two: &Json) -> bool {
    let (Some(one), Some(two)) = (one.as_object(), two.as_object()) else {
        return false;
    };
    one.len() < two.len() && one.iter().all(|(name, kind)| two.get(name) == Some(kind))
}

/// Requests given as one `exchange` finding.
struct Group {
    /// For a read by key: the path with its placeholder, and the page's.
    matched: Option<(String, String)>,
    /// The requests, by their place in the capture.
    members: Vec<usize>,
}

/// `web_use`: what a person's use of a web surface shows, in a watching
/// tab.
pub(crate) struct WebUse(pub(crate) Arc<Parts>, pub(crate) DiscovererName);

impl WebUse {
    async fn watch_use(
        &self,
        scope: &DiscoveryScope<'_>,
        end: &CaptureEnd,
        out: &mut dyn FindingSink,
    ) -> Result<DiscoveryReport, Failure> {
        let Parts { core, kinds } = &*self.0;
        let (_, start) = Core::start(scope)?;
        if end.is_set() {
            return Ok(DiscoveryReport::default());
        }
        let opening = core.watching_tab(
            scope,
            Deadline::after(OPEN_WAIT.min(scope.deadline.remaining())),
        );
        let tab = tokio::select! {
            tab = opening => tab?,
            () = end.ended() => return Ok(DiscoveryReport::default()),
        };
        let mut taken = Taken {
            requests: Vec::new(),
            screens: Vec::new(),
            current: start.path.clone(),
            origin: start.origin.clone(),
            screens_bound: false,
            report: DiscoveryReport::default(),
        };
        loop {
            let looked = tab.looked_at();
            let go_on = tokio::select! {
                () = end.ended() => false,
                _ = tab.closed(scope.deadline) => false,
                grew = tab.looked_at_since(looked, scope.deadline) => grew,
            };
            if !go_on {
                break;
            }
            let until = Deadline::after(TAKE_WAIT.min(scope.deadline.remaining()));
            tokio::select! {
                () = taken.take(&tab, scope, until) => {}
                () = end.ended() => break,
            }
        }
        // What the tab holds once the capture has ended. A request still
        // unanswered by then is left out, and counted.
        taken.take(&tab, scope, Deadline::after(LAST_WAIT)).await;
        drop(tab);
        taken.give(core, kinds, scope, out)
    }
}

impl Discoverer for WebUse {
    fn name(&self) -> DiscovererName {
        self.1.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        suits_web(surface)
    }

    fn discover<'a>(
        &'a self,
        _port: &'a dyn SandboxPort,
        _scope: &'a DiscoveryScope<'a>,
        _out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async {
            Err(a2m_provider::codes::row::EFFECT_NOT_DECLARED.failure(
                "The discoverer `web_use` watches a person's use, and a person begins it; it does not explore.",
                "Ask for a capture with `capture.start`; the person begins it on the engine's page.",
            ))
        })
    }

    fn watch<'a>(
        &'a self,
        scope: &'a DiscoveryScope<'a>,
        end: &'a CaptureEnd,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(self.watch_use(scope, end, out))
    }
}
