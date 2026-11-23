//! The drafting rules from a request a page made by itself to a page
//! request and a `page_http` read over it (spec section 402, decision
//! 0190).
//!
//! An `exchange` finding of the web pack's discoverers that names the page
//! that made it (`made_by`) holds the request's method, path, status, and
//! the names and kinds of its JSON answer to one level, never a value.
//! When the discoverer matched several such requests into one read by key,
//! the path holds a placeholder, `/api/orders/{order_id}/totals`, and so
//! does the page's, `/orders/{order_id}/totals`. From it these rules draft:
//!
//! - `page_requests/{request}.yaml`: the entry, with the path, the page,
//!   `GET`, `effect: read`, and the answer's shape: each top-level name
//!   with the kind it was seen with, required when every answer held it;
//! - `operations/{request}_request.yaml`: `{target}_{request}_get`, a read
//!   whose inputs, and key, are the placeholders, and whose outputs are the
//!   answer's plain values: a string as `untrusted_text`, an integer as
//!   `integer`, a boolean as `boolean`.
//!
//! `{request}` is made from the plain words of the request's path. A
//! screen's reads are named from the page's path, so the reads of a page
//! and of a request it makes are told apart by their words: a page is a
//! document and the request a JSON answer, and no one path answered both
//! in one discovery.
//!
//! A read is drafted only from a request the driver can make again exactly
//! (decision 0195): one with no query, and with no header its page's script
//! set ([`REPRODUCED_HEADERS`] is the closed list of such headers the driver
//! sends alike, and it is empty). For any other request the entry is drafted
//! with an open question in place of its path, which names what the page
//! sent that a read would not, by names alone, and no operation is drafted:
//! a read that dropped them could be answered with another variant of the
//! same shape, and nothing would say so.
//!
//! What they never draft: anything for a request whose method is not GET,
//! whose answer is not JSON or not an object, whose status is not 200,
//! that went to an origin that is not exactly one `web` surface of the
//! target, or that no page made (a document the tab was sent to); an output
//! for a number with a fraction (no unit or scale was stated, and this
//! version reads none exactly), for a nested value, or for a value some
//! answers left out; an example; a test.

use a2m_core::{Failure, FileKindName, Finding, FindingKind, OperationName, Subject};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};
use serde_json::Value as Json;

use crate::model::Segment;
use crate::requests::REQUEST_KIND;

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// The finding kind the web pack's discoverers give for an exchange.
pub(crate) const EXCHANGE_FINDING: &str = "exchange";

/// The format of an `exchange` finding these rules read.
const FORMAT: &str = "1";

/// The longest name an entry or an operation may have.
const LONGEST: usize = 64;

/// The headers a page's script may have set that a read from inside the
/// page sends the same, so that a request that carried them can still be
/// made again exactly. `read_from_page` sends no header of its own, so
/// none. Data.
pub(crate) const REPRODUCED_HEADERS: &[&str] = &[];

/// The longest path a drafted entry, file, or description holds, in
/// characters. A path is plain segments and placeholders, by the rule of a
/// screen's address.
const LONGEST_PATH: usize = 160;

/// What the page sent with a request that a read made again from inside
/// the page would not: the query's names, whether an entry of it was left
/// out, and the names of the headers its script set. `None` when the
/// finding says it sent nothing more, so that the request can be made
/// again exactly. A finding that does not say is taken as not
/// reproducible.
fn not_reproduced(found: &Json) -> Option<(Vec<String>, bool, Vec<String>)> {
    let names = |key: &str| -> Option<Vec<String>> {
        match found.get(key)? {
            Json::Object(map) => Some(map.keys().cloned().collect()),
            Json::Array(list) => list
                .iter()
                .map(|one| one.as_str().map(str::to_owned))
                .collect(),
            _ => None,
        }
    };
    let (Some(query), Some(left_out), Some(headers)) = (
        names("query"),
        found.get("query_left_out").and_then(Json::as_bool),
        names("set_headers"),
    ) else {
        return Some((Vec::new(), true, Vec::new()));
    };
    let headers: Vec<String> = headers
        .into_iter()
        .filter(|name| !REPRODUCED_HEADERS.contains(&name.as_str()))
        .collect();
    (!query.is_empty() || left_out || !headers.is_empty()).then_some((query, left_out, headers))
}

fn unreadable() -> Failure {
    a2m_core::codes::row::FINDING_INVALID.failure(
        "An `exchange` finding is not in the form the web pack's discoverers write.",
        "Capture the exchanges again with the version of the web pack that is enabled.",
    )
}

/// The plain words of a path, and the names of its placeholders in order;
/// `None` when a segment is neither plain nor one whole placeholder.
fn words_and_holes(path: &str) -> Option<(String, Vec<String>)> {
    let segments = crate::screens::parse_reach(path).ok()?;
    let mut words = Vec::new();
    let mut holes = Vec::new();
    for segment in segments {
        match segment {
            Segment::Hole(name) => holes.push(name),
            Segment::Literal(text) => words.push(text),
        }
    }
    Some((crate::draft::snake(&words.join("_")), holes))
}

/// One value of the answer, as the entry declares it and as a read gives
/// it.
struct Value {
    name: String,
    /// The kind, in the catalog's words.
    kind: &'static str,
    /// The field type of the output it gives, when it gives one.
    output: Option<&'static str>,
    required: bool,
}

/// The top-level values of the answer the finding names, in order of name.
fn values(response: &Json) -> Option<Vec<Value>> {
    if response.get("type").and_then(Json::as_str) != Some("object") {
        return None;
    }
    let fields = response.get("fields")?.as_object()?;
    let mut out = Vec::new();
    for (name, field) in fields {
        // A name one level inside an object is the object's, not a value
        // of the answer's own.
        if name.contains('.') || name.contains('[') {
            continue;
        }
        let (kind, output) = match field.get("type").and_then(Json::as_str) {
            Some("text") => ("string", Some(UNTRUSTED_TEXT)),
            Some("integer") => ("integer", Some("integer")),
            Some("boolean") => ("boolean", Some("boolean")),
            Some("decimal") => ("number", None),
            Some("object") => ("object", None),
            Some("list") => ("array", None),
            Some("null") => ("null", None),
            // More than one kind was seen, or none this rule names: the
            // entry does not declare it.
            _ => continue,
        };
        let required = field.get("optional").and_then(Json::as_bool) != Some(true);
        out.push(Value {
            name: name.clone(),
            kind,
            output: output.filter(|_| required),
            required,
        });
    }
    Some(out)
}

/// What both rules draft from one finding.
struct Drafts {
    entries: Vec<DraftDoc>,
    operations: Vec<DraftDoc>,
}

fn drafts(finding: &Finding, cx: &DraftContext<'_>) -> Result<Drafts, Failure> {
    let mut out = Drafts {
        entries: Vec::new(),
        operations: Vec::new(),
    };
    let found: Json = serde_json::from_str(finding.found()).map_err(|_| unreadable())?;
    if found.get("format").and_then(Json::as_str) != Some(FORMAT) {
        return Err(unreadable());
    }
    let text = |key: &str| found.get(key).and_then(Json::as_str);
    // A request the page made by itself, with GET, answered 200 in JSON.
    let (Some("GET"), Some("json"), Some(200), Some(page)) = (
        text("method"),
        text("content"),
        found.get("status").and_then(Json::as_u64),
        text("made_by"),
    ) else {
        return Ok(out);
    };
    let Subject::Address(address) = finding.subject() else {
        return Ok(out);
    };
    let (Some(target), Some(surface)) = (cx.target(), crate::draft::surface(cx, address.origin()))
    else {
        return Ok(out);
    };
    let path = address.path();
    if path.chars().count() > LONGEST_PATH || page.chars().count() > LONGEST_PATH {
        return Ok(out);
    }
    let (Some((words, holes)), Some((_, page_holes))) =
        (words_and_holes(path), words_and_holes(page))
    else {
        return Ok(out);
    };
    // Every placeholder of the page is one of the request's: the page's key
    // is the request's, and the read's inputs are those.
    if page_holes.iter().any(|hole| !holes.contains(hole))
        || holes
            .iter()
            .enumerate()
            .any(|(at, hole)| holes[..at].contains(hole))
    {
        return Ok(out);
    }
    // A placeholder named as a secret would be an input that is a secret:
    // nothing is drafted (decision 0302).
    if holes
        .iter()
        .any(|hole| crate::draft::named_as_secret(hole, &crate::draft::snake(hole)))
    {
        return Ok(out);
    }
    let Some(values) = found.get("response").and_then(values) else {
        return Ok(out);
    };
    // A value named as a secret is never an output, and the entry does not
    // declare it: an answer may hold names an entry does not state
    // (decision 0302).
    let (secrets, values): (Vec<Value>, Vec<Value>) = values.into_iter().partition(|value| {
        crate::draft::named_as_secret(&value.name, &crate::draft::snake(&value.name))
    });
    let secrets: Vec<String> = secrets
        .iter()
        .map(|value| crate::draft::snake(&value.name))
        .filter(|name| crate::draft::is_name(name, LONGEST))
        .collect();
    let operation = format!("{}_{words}_get", target.as_str());
    if words.is_empty()
        || !crate::draft::is_name(&format!("{words}_request"), LONGEST)
        || OperationName::new(&operation).is_err()
    {
        return Ok(out);
    }
    let outputs: Vec<&Value> = values
        .iter()
        .filter(|value| {
            value.output.is_some()
                && crate::draft::is_name(&value.name, LONGEST)
                && !holes.contains(&value.name)
        })
        .collect();
    if outputs.is_empty() {
        return Ok(out);
    }

    let properties = values
        .iter()
        .map(|value| {
            (
                value.name.clone(),
                DraftValue::map(vec![("type", DraftValue::text(value.kind))]),
            )
        })
        .collect();
    let required = values
        .iter()
        .filter(|value| value.required)
        .map(|value| DraftValue::text(&value.name))
        .collect();
    // A request that cannot be made again exactly is drafted with a
    // question in place of its path, and no read over it.
    let unmade = not_reproduced(&found);
    let path_value = match &unmade {
        None => DraftValue::text(path),
        Some((query, left_out, headers)) => {
            const ORDINALS: [&str; 3] = ["first", "second", "third"];
            let query = query.join(", ");
            let headers = headers.join(", ");
            let mut words: Vec<&str> = vec![path];
            let mut sent = Vec::new();
            if !query.is_empty() {
                sent.push(format!(
                    "a query, whose names are quoted {}",
                    ORDINALS[words.len()]
                ));
                words.push(&query);
            }
            if *left_out {
                sent.push("an entry of a query that is a value, not kept".to_owned());
            }
            if !headers.is_empty() {
                sent.push(format!(
                    "headers its script set, whose names are quoted {}",
                    ORDINALS[words.len()]
                ));
                words.push(&headers);
            }
            cx.open_question_quoting(
                &format!(
                    "The page that makes this request sent more with it than a read from inside the page sends: {}. A read that left them out could be answered with another variant of the same shape, so none is drafted. The request's path is quoted first. Which request answers what the page shows, and how is each part of it given?",
                    sent.join("; ")
                ),
                &words,
            )
        }
    };
    out.entries.push(DraftDoc {
        path: RelPath::new(&format!("page_requests/{words}.yaml"))?,
        body: DraftValue::map(vec![
            ("endpoint", DraftValue::text(&words)),
            ("surface", DraftValue::text(&surface)),
            ("method", DraftValue::text("GET")),
            ("path", path_value),
            ("page", DraftValue::text(page)),
            ("effect", DraftValue::text("read")),
            (
                "response_schema",
                DraftValue::map(vec![
                    ("type", DraftValue::text("object")),
                    ("required", DraftValue::List(required)),
                    ("properties", DraftValue::Map(properties)),
                ]),
            ),
        ]),
    });

    if unmade.is_some() {
        return Ok(out);
    }
    let mut doc = vec![
        ("operation", DraftValue::text(&operation)),
        ("effect", DraftValue::text("read")),
    ];
    match holes.as_slice() {
        [] => {}
        [only] => doc.push(("key", DraftValue::text(only))),
        many => doc.push((
            "key",
            DraftValue::List(many.iter().map(|hole| DraftValue::text(hole)).collect()),
        )),
    }
    doc.push((
        "inputs",
        DraftValue::Map(
            holes
                .iter()
                .map(|hole| {
                    (
                        hole.clone(),
                        DraftValue::map(vec![("type", DraftValue::text("text"))]),
                    )
                })
                .collect(),
        ),
    ));
    doc.extend([
        (
            "outputs",
            DraftValue::Map(
                outputs
                    .iter()
                    .map(|value| {
                        (
                            value.name.clone(),
                            DraftValue::map(vec![(
                                "type",
                                DraftValue::text(value.output.unwrap_or(UNTRUSTED_TEXT)),
                            )]),
                        )
                    })
                    .collect(),
            ),
        ),
        (
            "bindings",
            DraftValue::List(vec![DraftValue::map(vec![
                ("mode", DraftValue::text(crate::PAGE_HTTP_MODE)),
                ("verb", DraftValue::text("read")),
                ("endpoint", DraftValue::text(&words)),
            ])]),
        ),
        (
            "description",
            DraftValue::text(&format!(
                "Reads what the page `{page}` asks the application for by itself, with the page's own session: the plain values of the answer to `{path}`. Drafted by rule from the requests a page made; its name is provisional.{}",
                a2m_files::left_out_sentence(&secrets)
            )),
        ),
    ]);
    out.operations.push(DraftDoc {
        path: RelPath::new(&format!("operations/{words}_request.yaml"))?,
        body: DraftValue::map(doc),
    });
    Ok(out)
}

/// From an `exchange` finding a page made to its page request.
pub(crate) struct RequestEntryRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for RequestEntryRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        REQUEST_KIND
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        Ok(drafts(finding, cx)?.entries)
    }
}

/// From an `exchange` finding a page made to a `page_http` read.
pub(crate) struct RequestReadRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for RequestReadRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        Ok(drafts(finding, cx)?.operations)
    }
}
