//! Endpoint inference: exchanges grouped into endpoints, with path
//! templates in two tiers.
//!
//! Tier one makes a segment a parameter only when its form says it is an
//! identifier: a number of at least four digits, a universally unique
//! identifier, a long hexadecimal string, or a date. A shorter number stays
//! literal unless tier two finds it varying (decision 0322). Tier two makes a parameter of a segment
//! that varied between otherwise identical requests, and only when the
//! caller asks: it rests on variance alone, so an endpoint with one such
//! segment is `inferred`, whatever answered.
//!
//! Nothing here reads a page. The input is what a capture kept, and the
//! same input always gives the same output, in the same order.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::EvidenceLevel;
use serde_json::Value as Json;

use crate::shape::{Shape, object, type_of_text};

/// What the answer to an exchange was, by kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The request never left the browser: the observing tab refused it.
    Refused,
    /// A JSON document, kept as its shape.
    Json(Shape),
    /// A page, or anything else that is not JSON. Nothing of it is kept.
    Document,
}

/// One request, and what came back, by structure. It holds no value: a
/// query holds names and types, an answer holds a shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    /// The method, in capitals.
    pub method: String,
    /// The origin: scheme, host, and port.
    pub origin: String,
    /// The path, without its query.
    pub path: String,
    /// The names of the query's parameters, each with the type of its
    /// value.
    pub query: BTreeMap<String, &'static str>,
    /// The status of the answer, when there was one.
    pub status: Option<u16>,
    /// What came back.
    pub answer: Answer,
    /// For a request the page made by itself: the path of the page that
    /// made it. `None` for the request of a document the tab was sent to.
    pub made_by: Option<String>,
    /// The names of the request's headers that carry a credential or an
    /// anti-forgery token. Never their values.
    pub credential_headers: Vec<String>,
    /// For a request the page made by itself: the names of the request's
    /// headers its script set, which a read made again from inside the
    /// page would not send. Never their values (decision 0195).
    pub set_headers: Vec<String>,
    /// Whether the query held an entry that is not kept: one with no `=`,
    /// or one whose name is not a plain name.
    pub query_left_out: bool,
}

impl Exchange {
    /// Whether the system answered.
    #[must_use]
    pub fn answered(&self) -> bool {
        self.status.is_some() && self.answer != Answer::Refused
    }

    /// The exchange as the `found` of an `exchange` finding. A request the
    /// page made by itself also names the page, as `made_by`, the names of
    /// the headers that carried a credential, as `credential_headers`, the
    /// names of the headers its script set, as `set_headers`, and whether
    /// an entry of its query was left out, as `query_left_out`.
    #[must_use]
    pub fn found(&self) -> Json {
        let mut found = self.found_of_a_document();
        if let (Some(page), Json::Object(fields)) = (&self.made_by, &mut found) {
            fields.insert("made_by".to_owned(), Json::from(page.as_str()));
            fields.insert(
                "credential_headers".to_owned(),
                Json::Array(
                    self.credential_headers
                        .iter()
                        .map(|name| Json::from(name.as_str()))
                        .collect(),
                ),
            );
            fields.insert(
                "set_headers".to_owned(),
                Json::Array(
                    self.set_headers
                        .iter()
                        .map(|name| Json::from(name.as_str()))
                        .collect(),
                ),
            );
            fields.insert("query_left_out".to_owned(), Json::Bool(self.query_left_out));
            return Json::Object(crate::shape::sorted(fields.clone()));
        }
        found
    }

    fn found_of_a_document(&self) -> Json {
        object([
            ("format", Json::from("1")),
            ("method", Json::from(self.method.as_str())),
            ("path", Json::from(self.path.as_str())),
            (
                "query",
                query_json(self.query.iter().map(|(k, v)| (k, [*v]))),
            ),
            ("answered", Json::Bool(self.answered())),
            ("status", self.status.map_or(Json::Null, Json::from)),
            (
                "content",
                Json::from(match self.answer {
                    Answer::Refused => "none",
                    Answer::Json(_) => "json",
                    Answer::Document => "document",
                }),
            ),
            (
                "response",
                match &self.answer {
                    Answer::Json(shape) => shape.to_json(),
                    Answer::Refused | Answer::Document => Json::Null,
                },
            ),
        ])
    }
}

fn query_json<'a, I, T>(query: I) -> Json
where
    I: Iterator<Item = (&'a String, T)>,
    T: IntoIterator<Item = &'static str>,
{
    Json::Object(
        query
            .map(|(name, types)| {
                let mut types: Vec<Json> = types.into_iter().map(Json::from).collect();
                let value = if types.len() == 1 {
                    types.remove(0)
                } else {
                    Json::Array(types)
                };
                (name.clone(), value)
            })
            .collect::<BTreeMap<String, Json>>()
            .into_iter()
            .collect(),
    )
}

/// Which rule made a segment a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// The segment's form: a number, an identifier, a date.
    One,
    /// The segment varied between otherwise identical requests.
    Two,
}

/// One segment of a path template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// The text, as every request held it.
    Literal(String),
    /// A parameter.
    Parameter {
        /// The types of the values seen there.
        types: BTreeSet<&'static str>,
        /// The rule that made it a parameter.
        tier: Tier,
        /// How many different values were seen. The values are not kept.
        literals: BTreeSet<a2m_core::Digest>,
    },
}

/// One endpoint: the exchanges of one method, origin, and path template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The method, in capitals.
    pub method: String,
    /// The origin.
    pub origin: String,
    /// The path template's segments.
    pub segments: Vec<Segment>,
    /// Every status that came back.
    pub statuses: BTreeSet<u16>,
    /// The query's parameter names, each with the types seen.
    pub query: BTreeMap<String, BTreeSet<&'static str>>,
    /// The shape of its JSON answers, merged; nothing when none was JSON.
    pub response: Option<Shape>,
    /// How many exchanges it was made from.
    pub seen: u64,
    /// Which exchanges, by their place in the input.
    pub sources: Vec<usize>,
}

impl Endpoint {
    /// The path template, with each parameter as `{p1}`, `{p2}`, in
    /// order.
    #[must_use]
    pub fn template(&self) -> String {
        let mut out = String::new();
        let mut number = 0;
        for segment in &self.segments {
            out.push('/');
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Parameter { .. } => {
                    number += 1;
                    out.push_str(&format!("{{p{number}}}"));
                }
            }
        }
        if out.is_empty() {
            out.push('/');
        }
        out
    }

    /// Whether the system answered at least one of its exchanges.
    #[must_use]
    pub fn answered(&self) -> bool {
        !self.statuses.is_empty()
    }

    /// Whether a segment is a parameter by variance alone.
    #[must_use]
    pub fn has_tier_two(&self) -> bool {
        self.segments.iter().any(|segment| {
            matches!(
                segment,
                Segment::Parameter {
                    tier: Tier::Two,
                    ..
                }
            )
        })
    }

    /// Whether a segment of the template is a number of fewer than
    /// [`LEAST_DIGITS`] digits, kept literal because nothing showed it
    /// varying.
    #[must_use]
    pub fn keeps_a_short_number(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, Segment::Literal(text) if is_short_number(text)))
    }

    /// Whether any segment is a parameter.
    #[must_use]
    pub fn has_parameter(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, Segment::Parameter { .. }))
    }

    /// The level of the endpoint: `discovered` when it answered and no
    /// segment rests on variance alone, and `inferred` otherwise. A
    /// derived finding is never stronger than its weakest part.
    #[must_use]
    pub fn level(&self) -> EvidenceLevel {
        if self.answered() && !self.has_tier_two() {
            EvidenceLevel::Discovered
        } else {
            EvidenceLevel::Inferred
        }
    }

    /// The endpoint as the `found` of an `endpoint` finding. The first
    /// seven names are the form the kind's owner defines; the rest is
    /// what capture adds.
    #[must_use]
    pub fn found(&self) -> Json {
        let mut number = 0;
        let mut parameters = Vec::new();
        for (position, segment) in self.segments.iter().enumerate() {
            if let Segment::Parameter {
                types,
                tier,
                literals,
            } = segment
            {
                number += 1;
                parameters.push(object([
                    ("name", Json::from(format!("p{number}"))),
                    ("position", Json::from(position)),
                    (
                        "types",
                        Json::Array(types.iter().map(|name| Json::from(*name)).collect()),
                    ),
                    (
                        "tier",
                        Json::from(match tier {
                            Tier::One => 1,
                            Tier::Two => 2,
                        }),
                    ),
                    ("values_seen", Json::from(literals.len())),
                ]));
            }
        }
        let template_level = if !self.has_parameter() {
            "observed"
        } else {
            "inferred"
        };
        object([
            ("format", Json::from("1")),
            ("method", Json::from(self.method.as_str())),
            ("path", Json::from(self.template())),
            (
                "status",
                Json::from(self.statuses.iter().next().copied().unwrap_or(0)),
            ),
            ("content_type", Json::Null),
            ("allow", Json::Null),
            ("challenges", Json::Array(Vec::new())),
            ("answered", Json::Bool(self.answered())),
            (
                "statuses",
                Json::Array(self.statuses.iter().map(|s| Json::from(*s)).collect()),
            ),
            (
                "template",
                object([
                    ("level", Json::from(template_level)),
                    ("parameters", Json::Array(parameters)),
                ]),
            ),
            (
                "query",
                query_json(
                    self.query
                        .iter()
                        .map(|(name, types)| (name, types.iter().copied())),
                ),
            ),
            (
                "response",
                self.response.as_ref().map_or(Json::Null, Shape::to_json),
            ),
            ("seen", Json::from(self.seen)),
        ])
    }
}

/// The fewest digits a run of digits has to be a parameter by its form
/// alone. A starting value for the first test to tune, not a measured
/// figure (decision 0322 point 4).
pub const LEAST_DIGITS: usize = 4;

/// Whether a segment is a number shorter than [`LEAST_DIGITS`].
fn is_short_number(segment: &str) -> bool {
    type_of_text(segment) == "integer" && segment.trim_start_matches('-').len() < LEAST_DIGITS
}

/// The type of a segment that is an identifier by its form, if it is one.
/// A number of fewer than [`LEAST_DIGITS`] digits is not one.
pub fn identifier_type(segment: &str) -> Option<&'static str> {
    if is_short_number(segment) {
        return None;
    }
    match type_of_text(segment) {
        kind @ ("integer" | "uuid" | "hex" | "date") => Some(kind),
        _ => None,
    }
}

/// Whether a segment is a plain word: letters, with digits, hyphens, and
/// underscores after them. A number and an identifier are not.
fn is_word(segment: &str) -> bool {
    segment
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && type_of_text(segment) == "text"
}

/// Whether the two literals that differ at `place` between endpoints
/// `first` and `second` are both words that some other endpoint of the
/// capture uses as a literal in the same place: after the same segments, of
/// the same origin, and in a path of another length or another method. Two
/// words that the capture elsewhere treats as fixed names are siblings, not
/// the values of one parameter (decision 0322 point 5).
fn literal_siblings(endpoints: &[Endpoint], first: usize, second: usize, place: usize) -> bool {
    let (Segment::Literal(a), Segment::Literal(b)) = (
        &endpoints[first].segments[place],
        &endpoints[second].segments[place],
    ) else {
        return false;
    };
    if !is_word(a) || !is_word(b) {
        return false;
    }
    let elsewhere = |word: &str| {
        endpoints.iter().enumerate().any(|(at, other)| {
            at != first
                && at != second
                && other.origin == endpoints[first].origin
                && other.segments.len() > place
                && (other.segments.len() != endpoints[first].segments.len()
                    || other.method != endpoints[first].method)
                && other.segments[..place] == endpoints[first].segments[..place]
                && matches!(&other.segments[place], Segment::Literal(text) if text == word)
        })
    };
    elsewhere(a) && elsewhere(b)
}

fn digest_of(text: &str) -> a2m_core::Digest {
    a2m_core::Digest::of(text.as_bytes())
}

fn segments_of(path: &str) -> Vec<Segment> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| match identifier_type(segment) {
            Some(kind) => Segment::Parameter {
                types: BTreeSet::from([kind]),
                tier: Tier::One,
                literals: BTreeSet::from([digest_of(segment)]),
            },
            None => Segment::Literal(segment.to_owned()),
        })
        .collect()
}

/// Whether two endpoints are one by tier one: the same literals in the
/// same places, and a parameter wherever the other has one.
fn same_template(one: &[Segment], other: &[Segment]) -> bool {
    one.len() == other.len()
        && one.iter().zip(other).all(|pair| match pair {
            (Segment::Literal(a), Segment::Literal(b)) => a == b,
            (Segment::Parameter { .. }, Segment::Parameter { .. }) => true,
            _ => false,
        })
}

/// The one place where two templates hold different literals, when every
/// other place agrees or holds a parameter on either side.
fn one_difference(one: &[Segment], other: &[Segment]) -> Option<usize> {
    if one.len() != other.len() {
        return None;
    }
    let mut place = None;
    for (at, pair) in one.iter().zip(other).enumerate() {
        if let (Segment::Literal(a), Segment::Literal(b)) = pair
            && a != b
        {
            if place.is_some() {
                return None;
            }
            place = Some(at);
        }
    }
    place
}

/// Whether two endpoints answer alike: both answered with JSON, and the
/// tops of their answers hold the same names. Pages, and requests that
/// were never answered, are not joined by variance: nothing says they
/// are one endpoint.
fn answers_alike(one: &Endpoint, other: &Endpoint) -> bool {
    let top = |shape: &Shape| {
        let json = shape.to_json();
        let names: Vec<String> = json
            .get("fields")
            .and_then(Json::as_object)
            .map(|fields| fields.keys().cloned().collect())
            .unwrap_or_default();
        (json.get("type").cloned(), names)
    };
    match (&one.response, &other.response) {
        (Some(mine), Some(theirs)) => top(mine) == top(theirs),
        _ => false,
    }
}

fn widen(segment: &Segment, tier: Tier) -> Segment {
    match segment {
        Segment::Literal(text) => Segment::Parameter {
            types: BTreeSet::from([type_of_text(text)]),
            tier,
            literals: BTreeSet::from([digest_of(text)]),
        },
        parameter @ Segment::Parameter { .. } => parameter.clone(),
    }
}

fn join_segment(one: &Segment, other: &Segment) -> Segment {
    match (one, other) {
        (Segment::Literal(_), Segment::Literal(_)) => one.clone(),
        _ => {
            let (a, b) = (widen(one, Tier::Two), widen(other, Tier::Two));
            match (a, b) {
                (
                    Segment::Parameter {
                        mut types,
                        tier,
                        mut literals,
                    },
                    Segment::Parameter {
                        types: more,
                        tier: other_tier,
                        literals: others,
                    },
                ) => {
                    types.extend(more);
                    literals.extend(others);
                    Segment::Parameter {
                        types,
                        // The weaker rule wins: a place one side knew only
                        // by variance is known only by variance.
                        tier: tier.max(other_tier),
                        literals,
                    }
                }
                (segment, _) => segment,
            }
        }
    }
}

fn absorb(into: &mut Endpoint, other: &Endpoint) {
    into.segments = into
        .segments
        .iter()
        .zip(&other.segments)
        .map(|(one, two)| join_segment(one, two))
        .collect();
    into.statuses.extend(&other.statuses);
    for (name, types) in &other.query {
        into.query.entry(name.clone()).or_default().extend(types);
    }
    into.response = match (&into.response, &other.response) {
        (Some(mine), Some(theirs)) => Some(mine.merged(theirs)),
        (Some(one), None) | (None, Some(one)) => Some(one.clone()),
        (None, None) => None,
    };
    into.seen += other.seen;
    into.sources.extend(&other.sources);
    into.sources.sort_unstable();
}

/// Groups exchanges into endpoints by method, origin, and path template.
/// Tier one always applies; tier two only when `tier_two` is set. The
/// result is in the order of method, origin, and template.
#[must_use]
pub fn infer_endpoints(exchanges: &[Exchange], tier_two: bool) -> Vec<Endpoint> {
    let mut endpoints: Vec<Endpoint> = Vec::new();
    for (index, exchange) in exchanges.iter().enumerate() {
        let one = Endpoint {
            method: exchange.method.clone(),
            origin: exchange.origin.clone(),
            segments: segments_of(&exchange.path),
            statuses: if exchange.answered() {
                exchange.status.into_iter().collect()
            } else {
                BTreeSet::new()
            },
            query: exchange
                .query
                .iter()
                .map(|(name, kind)| (name.clone(), BTreeSet::from([*kind])))
                .collect(),
            response: match &exchange.answer {
                Answer::Json(shape) => Some(shape.clone()),
                Answer::Refused | Answer::Document => None,
            },
            seen: 1,
            sources: vec![index],
        };
        let same = endpoints.iter_mut().find(|known| {
            known.method == one.method
                && known.origin == one.origin
                && same_template(&known.segments, &one.segments)
        });
        match same {
            Some(known) => absorb(known, &one),
            None => endpoints.push(one),
        }
    }
    if tier_two {
        // Joined until nothing more joins. Each join removes one endpoint,
        // so the loop ends.
        'again: loop {
            for first in 0..endpoints.len() {
                for second in first + 1..endpoints.len() {
                    let (one, other) = (&endpoints[first], &endpoints[second]);
                    let place = if one.method == other.method && one.origin == other.origin {
                        one_difference(&one.segments, &other.segments)
                    } else {
                        None
                    };
                    let Some(place) = place else {
                        continue;
                    };
                    if !answers_alike(one, other)
                        || literal_siblings(&endpoints, first, second, place)
                    {
                        continue;
                    }
                    let mut other = endpoints.remove(second);
                    other.segments[place] = widen(&other.segments[place], Tier::Two);
                    let one = &mut endpoints[first];
                    one.segments[place] = widen(&one.segments[place], Tier::Two);
                    absorb(one, &other);
                    continue 'again;
                }
            }
            break;
        }
    }
    endpoints.sort_by(|a, b| {
        (&a.method, &a.origin, a.template()).cmp(&(&b.method, &b.origin, b.template()))
    });
    endpoints
}

/// Requests a page made by itself that are one read by key: the requests,
/// of one method and origin, answered with one status and one shape, whose
/// paths differ in exactly one segment, made by pages whose paths differ in
/// exactly one segment too, where each page's segment is its request's
/// (decision 0190). Request matching, as screen matching joins pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedRequests {
    /// The request's path, with the segment that differs as a placeholder.
    pub pattern: String,
    /// The path of the page that makes it, with its segment as the same
    /// placeholder.
    pub page: String,
    /// The requests, by their place in the input, in order of path.
    pub members: Vec<usize>,
}

/// Request matching over the exchanges of one capture: only those a page
/// made by itself, answered, with a JSON answer. A request that two groups
/// would hold is in neither; a group of fewer than two is none. In order of
/// the first request of each. The placeholder is named from the segment
/// before it in the request's path, as one record of it (`orders` gives
/// `order_id`), and the page's placeholder takes the same name.
#[must_use]
pub fn matched_requests(exchanges: &[Exchange]) -> Vec<MatchedRequests> {
    fn split(path: &str) -> Option<Vec<&str>> {
        let rest = path.strip_prefix('/')?;
        let segments: Vec<&str> = rest.split('/').collect();
        (!rest.is_empty() && segments.iter().all(|segment| crate::screen::plain(segment)))
            .then_some(segments)
    }
    type Key = (
        String,
        String,
        u16,
        String,
        usize,
        Vec<String>,
        usize,
        Vec<String>,
    );
    let mut groups: BTreeMap<Key, Vec<usize>> = BTreeMap::new();
    for (at, one) in exchanges.iter().enumerate() {
        let (Some(page), Some(status), Answer::Json(shape)) =
            (&one.made_by, one.status, &one.answer)
        else {
            continue;
        };
        let (Some(request), Some(made_by)) = (split(&one.path), split(page)) else {
            continue;
        };
        // The segment that varies is the page's key and the request's: the
        // same text at a place of each.
        for (hole, value) in request.iter().enumerate() {
            for (page_hole, _) in made_by.iter().enumerate().filter(|(_, own)| *own == value) {
                let blank = |segments: &[&str], at: usize| -> Vec<String> {
                    segments
                        .iter()
                        .enumerate()
                        .map(|(place, segment)| {
                            if place == at {
                                String::new()
                            } else {
                                (*segment).to_owned()
                            }
                        })
                        .collect()
                };
                groups
                    .entry((
                        one.origin.clone(),
                        one.method.clone(),
                        status,
                        shape.to_json().to_string(),
                        hole,
                        blank(&request, hole),
                        page_hole,
                        blank(&made_by, page_hole),
                    ))
                    .or_default()
                    .push(at);
            }
        }
    }
    // Two requests with one value at the varying place are one request
    // seen twice, not two records.
    groups.retain(|(_, _, _, _, hole, _, _, _), members| {
        let values: BTreeSet<&str> = members
            .iter()
            .filter_map(|at| {
                exchanges[*at]
                    .path
                    .trim_start_matches('/')
                    .split('/')
                    .nth(*hole)
            })
            .collect();
        values.len() >= 2
    });
    let mut held: BTreeMap<usize, u32> = BTreeMap::new();
    for members in groups.values() {
        for at in members {
            *held.entry(*at).or_default() += 1;
        }
    }
    let mut out: Vec<MatchedRequests> = groups
        .into_iter()
        .filter_map(|((_, _, _, _, hole, _, page_hole, _), members)| {
            let mut members: Vec<usize> = members
                .into_iter()
                .filter(|at| held.get(at) == Some(&1))
                .collect();
            if members.len() < 2 {
                return None;
            }
            members.sort_by(|a, b| exchanges[*a].path.cmp(&exchanges[*b].path));
            let first = &exchanges[members[0]];
            let request: Vec<&str> = first.path.trim_start_matches('/').split('/').collect();
            let page: Vec<&str> = first
                .made_by
                .as_deref()?
                .trim_start_matches('/')
                .split('/')
                .collect();
            let name = crate::screen::placeholder(&request, hole);
            let fill = |segments: &[&str], at: usize| -> String {
                let parts: Vec<String> = segments
                    .iter()
                    .enumerate()
                    .map(|(place, segment)| {
                        if place == at {
                            format!("{{{name}}}")
                        } else {
                            (*segment).to_owned()
                        }
                    })
                    .collect();
                format!("/{}", parts.join("/"))
            };
            Some(MatchedRequests {
                pattern: fill(&request, hole),
                page: fill(&page, page_hole),
                members,
            })
        })
        .collect();
    out.sort_by_key(|group| group.members.iter().min().copied());
    out
}

/// Whether a text is an identifier by its form alone, as a key of an
/// object: a number of any length, a universally unique identifier, a long
/// hexadecimal string, or a date. The length rule of a path segment does
/// not apply: a dictionary keyed by `1` and `2` is a dictionary.
pub(crate) fn is_identifier_by_form(text: &str) -> bool {
    matches!(type_of_text(text), "integer" | "uuid" | "hex" | "date")
}
