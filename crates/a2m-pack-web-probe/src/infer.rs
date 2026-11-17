//! Endpoint inference: exchanges grouped into endpoints, with path
//! templates in two tiers.
//!
//! Tier one makes a segment a parameter only when its form says it is an
//! identifier: a number, a universally unique identifier, a long
//! hexadecimal string, or a date. Tier two makes a parameter of a segment
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
}

impl Exchange {
    /// Whether the system answered.
    #[must_use]
    pub fn answered(&self) -> bool {
        self.status.is_some() && self.answer != Answer::Refused
    }

    /// The exchange as the `found` of an `exchange` finding.
    #[must_use]
    pub fn found(&self) -> Json {
        object([
            ("format", Json::from("1")),
            ("method", Json::from(self.method.as_str())),
            ("path", Json::from(self.path.as_str())),
            ("query", query_json(self.query.iter().map(|(k, v)| (k, [*v])))),
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

/// The type of a segment that is an identifier by its form, if it is one.
fn identifier_type(segment: &str) -> Option<&'static str> {
    match type_of_text(segment) {
        kind @ ("integer" | "uuid" | "hex" | "date") => Some(kind),
        _ => None,
    }
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

/// Whether two endpoints answer alike: the same names at the top of their
/// JSON answers, or no JSON answer on either side.
fn answers_alike(one: &Endpoint, other: &Endpoint) -> bool {
    let top = |shape: &Option<Shape>| {
        shape.as_ref().map(|shape| {
            let json = shape.to_json();
            let names: Vec<String> = json
                .get("fields")
                .and_then(Json::as_object)
                .map(|fields| fields.keys().cloned().collect())
                .unwrap_or_default();
            (json.get("type").cloned(), names)
        })
    };
    top(&one.response) == top(&other.response)
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
                    if !answers_alike(one, other) {
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
