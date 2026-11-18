//! The typed request template: what an endpoint entry compiles to, and how
//! a request is built from it, position by position.
//!
//! Nothing here joins a value into an address. A path is a list of literal
//! segments and holes; a value fills one hole and is encoded as one
//! segment. A query value is encoded as a query value. A body value is
//! written as one JSON value. The scheme, the host, and the port are not in
//! a template at all: they come from the surface when the driver prepares.

use std::collections::BTreeMap;

use a2m_core::{Failure, FailureClass, Value, Values};
use serde::{Deserialize, Serialize};

use crate::codes;
use crate::forms::AnswerSpec;
use crate::json::{Json, is_plain_number};
use crate::shape::{Kind, Shape};

/// The version of the shape of this pack's payloads in a plan.
pub(crate) const PAYLOAD_FORMAT: u16 = 1;

/// The methods an endpoint may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub(crate) enum Method {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

impl Method {
    pub(crate) const WORDS: &'static [&'static str] =
        &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

    pub(crate) fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "GET" => Self::Get,
            "HEAD" => Self::Head,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "OPTIONS" => Self::Options,
            _ => return None,
        })
    }

    pub(crate) const fn word(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }

    /// Whether HTTP itself says the method changes nothing.
    pub(crate) const fn is_safe(self) -> bool {
        matches!(self, Self::Get | Self::Head | Self::Options)
    }

    /// Whether a request of this method carries a body.
    pub(crate) const fn has_body(self) -> bool {
        matches!(self, Self::Post | Self::Put | Self::Patch)
    }
}

/// The headers frameworks read as a request's method in place of its own.
pub(crate) const OVERRIDE_HEADERS: &[&str] = &[
    "x-http-method-override",
    "x-http-method",
    "x-method-override",
];
/// The query or form entry frameworks read the same way.
pub(crate) const OVERRIDE_FIELD: &str = "_method";

/// Whether a header or a query entry is a method override, by its name.
pub(crate) fn is_override_header(name: &str) -> bool {
    OVERRIDE_HEADERS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(name))
}

/// Whether a query or form entry is a method override, by its name.
pub(crate) fn is_override_field(name: &str) -> bool {
    name.eq_ignore_ascii_case(OVERRIDE_FIELD)
}

/// Whether a method an override names may change the system: anything
/// but a method HTTP marks safe, in any letter case, including a word
/// that is no method at all.
pub(crate) fn override_may_change(named: &str) -> bool {
    !Method::from_word(&named.trim().to_ascii_uppercase()).is_some_and(Method::is_safe)
}

/// One segment of a path template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Segment {
    /// Text that is sent as written. Checked when the endpoint compiles.
    Literal(String),
    /// A place one value fills, as one whole segment.
    Hole(String),
}

/// Who supplies the credential of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Auth {
    /// The session's cookies or token, through the lease.
    Session,
    /// A token, through the lease.
    Token,
    /// The endpoint takes no credential, and none is placed.
    None,
}

/// One answer an endpoint declares besides success: a status, and what the
/// body must say for the answer to be this one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Answer {
    pub status: u16,
    /// Top-level fields of the body and the text each must be.
    pub when: BTreeMap<String, String>,
    /// For a conflict: the body field that holds the record's version now.
    pub current: Option<String>,
}

impl Answer {
    /// Whether an answer is this one.
    pub(crate) fn matches(&self, status: u16, body: Option<&Json>) -> bool {
        self.status == status
            && self.when.iter().all(|(field, expected)| {
                body.and_then(|body| body.get(field))
                    .and_then(Json::scalar_text)
                    .is_some_and(|found| found == *expected)
            })
    }
}

/// The answers an endpoint declares.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Answers {
    /// Statuses that mean the request was accepted.
    pub ok: Vec<u16>,
    /// Statuses that mean the record does not exist, on a lookup.
    pub absent: Vec<u16>,
    /// The system's own refusal of the input.
    pub refused: Vec<Answer>,
    /// The version sent is no longer the record's.
    pub conflict: Vec<Answer>,
    /// The system says "not now".
    pub unavailable: Vec<Answer>,
}

impl Answers {
    /// Whether the endpoint declares anything for a status.
    pub(crate) fn declares(&self, status: u16) -> bool {
        self.ok.contains(&status)
            || self.absent.contains(&status)
            || [&self.refused, &self.conflict, &self.unavailable]
                .iter()
                .any(|answers| answers.iter().any(|answer| answer.status == status))
    }
}

/// Where a record's version is found in an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VersionSource {
    /// A top-level field of the body.
    Field(String),
    /// A response header, such as `etag`.
    Header(String),
}

/// Where the version last read is sent back as a precondition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VersionSink {
    /// A top-level field of the body.
    Body(String),
    /// A request header, such as `if-match`.
    Header(String),
}

/// How an endpoint handles a record's version.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VersionRule {
    pub read: Option<VersionSource>,
    pub send: Option<VersionSink>,
}

/// One endpoint, compiled. This is the pack's entry in its section of the
/// plan, keyed by the endpoint's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndpointTemplate {
    /// The surface the endpoint lives on. Its address is the origin.
    pub surface: String,
    pub method: Method,
    pub path: Vec<Segment>,
    /// Whether the path ends in a slash.
    pub trailing_slash: bool,
    /// Query entries sent as written with every request.
    pub query: BTreeMap<String, String>,
    /// Headers sent as written with every request, by lowercase name.
    pub headers: BTreeMap<String, String>,
    pub auth: Auth,
    /// Whether the endpoint changes the system.
    pub writes: bool,
    /// Whether the developer says that an endpoint marked `read`, whose
    /// method HTTP does not mark safe, only reads (decision 0249). Left
    /// out of the payload when false, so other endpoints compile as before.
    #[serde(default, skip_serializing_if = "is_false")]
    pub read_asserted: bool,
    pub request: Option<Shape>,
    /// The shape the answer's record must fit: the body itself for JSON,
    /// the declared fields for any other form.
    pub response: Shape,
    /// The form of the answer. Left out of the payload for JSON with
    /// nothing declared, so such an endpoint compiles as it did before.
    #[serde(default, skip_serializing_if = "AnswerSpec::is_plain_json")]
    pub answer: AnswerSpec,
    pub answers: Answers,
    /// The header that carries the record's idempotency token.
    pub idempotency_header: Option<String>,
    pub version: VersionRule,
    /// Whether the front end computes or signs the request.
    pub signed: bool,
    /// The most bytes of an answer that are read; the pack's setting when
    /// the endpoint states none.
    pub max_response_bytes: Option<u64>,
    /// Request fields ignored when a shadow is compared.
    pub volatile: Vec<String>,
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's `skip_serializing_if` passes a reference"
)]
const fn is_false(flag: &bool) -> bool {
    !*flag
}

impl EndpointTemplate {
    /// The names of the holes of the path, in order.
    pub(crate) fn holes(&self) -> impl Iterator<Item = &str> {
        self.path.iter().filter_map(|segment| match segment {
            Segment::Hole(name) => Some(name.as_str()),
            Segment::Literal(_) => None,
        })
    }

    /// The path as the target file wrote it, for a message or a trace.
    pub(crate) fn path_text(&self) -> String {
        let mut text = String::new();
        for segment in &self.path {
            text.push('/');
            match segment {
                Segment::Literal(literal) => text.push_str(literal),
                Segment::Hole(name) => {
                    text.push('{');
                    text.push_str(name);
                    text.push('}');
                }
            }
        }
        if self.trailing_slash || self.path.is_empty() {
            text.push('/');
        }
        text
    }
}

/// The section of the plan this pack owns: every endpoint of one surface
/// kind, by name.
pub(crate) type EndpointSection = BTreeMap<String, EndpointTemplate>;

/// How one body field is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BodyField {
    pub name: String,
    pub kind: Kind,
}

/// The payload of a `request` step: which arguments it takes, and for which
/// position. The arguments are named `path.<hole>`, `query.<name>`, and
/// `body.<field>`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RequestStep {
    pub query: Vec<String>,
    pub body: Vec<BodyField>,
    /// For a write whose endpoint sends the record's version: the endpoint
    /// whose read of the record supplies it. A version read through any
    /// other endpoint is the version of something else.
    #[serde(default)]
    pub version_from: Option<String>,
}

/// The type a value taken out of an answer must have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Take {
    Text,
    Int,
    Decimal,
    Bool,
    /// A calendar day, written `YYYY-MM-DD`.
    Date,
}

/// One value an `extract` step takes out of an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Extracted {
    /// The name the value is returned under: an output of the operation.
    pub name: String,
    /// Property names from the top of the body; `*` steps into each item
    /// of an array, and the value is then a list.
    pub path: Vec<String>,
    pub take: Take,
}

/// The payload of an `extract` step.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtractStep {
    pub values: Vec<Extracted>,
}

/// The payload of a `probe` step, which only a discoverer makes: a safe
/// method and a literal path on the surface. It carries no body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProbeStep {
    pub method: Method,
    pub path: String,
}

/// Why a path as written is not a template.
pub(crate) fn parse_path(text: &str) -> Result<(Vec<Segment>, bool), String> {
    let Some(rest) = text.strip_prefix('/') else {
        return Err("a path starts with `/`; the address of the system comes from the surface, never from an endpoint".to_owned());
    };
    let mut segments = Vec::new();
    let mut trailing_slash = false;
    let parts: Vec<&str> = rest.split('/').collect();
    for (position, part) in parts.iter().enumerate() {
        let last = position + 1 == parts.len();
        if part.is_empty() {
            if last {
                trailing_slash = !segments.is_empty();
                continue;
            }
            return Err("a path has no empty segment".to_owned());
        }
        if let Some(name) = part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
            let named = name.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !named {
                return Err(format!(
                    "the placeholder `{}` is not a lowercase name",
                    codes::shown(part)
                ));
            }
            if segments.contains(&Segment::Hole(name.to_owned())) {
                return Err(format!("the placeholder `{name}` appears twice"));
            }
            segments.push(Segment::Hole(name.to_owned()));
        } else if *part == "." || *part == ".." {
            return Err("a path has no `.` or `..` segment".to_owned());
        } else if part.bytes().all(is_unreserved) {
            segments.push(Segment::Literal((*part).to_owned()));
        } else if part.contains(['{', '}']) {
            return Err(format!(
                "a placeholder fills one whole segment; `{}` mixes one with text",
                codes::shown(part)
            ));
        } else {
            return Err(format!(
                "the segment `{}` holds a character other than letters, digits, `-`, `.`, `_`, and `~`",
                codes::shown(part)
            ));
        }
    }
    Ok((segments, trailing_slash))
}

/// Whether a header is one the connection sets. No endpoint, idempotency
/// token, or version may write one: `host` decides which system a request
/// on the surface's connection is for, and the others its framing.
pub(crate) fn is_connection_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host" | "content-length" | "transfer-encoding" | "connection"
    )
}

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// Percent-encodes text so that it is one path segment or one query value
/// and nothing else: every byte but letters, digits, `-`, `.`, `_`, and `~`
/// is written as `%XX`.
pub(crate) fn encode(text: &str, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in text.bytes() {
        if is_unreserved(byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
}

/// A value as the text of a path segment or a query value. `None` for a
/// value that has no one text: a list, a record, nothing.
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::Text(text) => Some(text.clone()),
        Value::Int(number) => Some(number.to_string()),
        Value::Decimal(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Date(date) => Some(date.to_string()),
        _ => None,
    }
}

fn refused(position: &str, why: &str) -> Failure {
    codes::failure(
        FailureClass::Input,
        codes::PATH_VALUE_REFUSED,
        format!("The value for {position} was refused: {why}."),
        "Give a value that is one plain name or number, as the system shows it.",
    )
}

/// A request, built: everything but the origin and the credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Built {
    pub method: Method,
    /// The path, encoded, starting with `/`.
    pub path: String,
    /// The query, encoded, without the `?`; empty for none.
    pub query: String,
    /// Headers by lowercase name. None is a credential.
    pub headers: Vec<(String, String)>,
    /// The body, as JSON text.
    pub body: Option<String>,
}

/// What a request is built from besides the template and the arguments.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Extras<'a> {
    /// The record's idempotency token.
    pub stamp: Option<&'a str>,
    /// The version last read for the record.
    pub version: Option<&'a Json>,
}

/// Builds the request of a `request` step from its endpoint's template and
/// the step's arguments.
///
/// # Errors
///
/// `input`/`path_value_refused` for a path value that is empty, a dot
/// segment, or holds a separator or a control character;
/// `input`/`step_not_usable` for an argument that is missing or of a kind
/// its position cannot carry.
pub(crate) fn build(
    template: &EndpointTemplate,
    step: &RequestStep,
    args: &Values,
    extras: Extras<'_>,
) -> Result<Built, Failure> {
    let unusable = |why: String| {
        codes::failure(
            FailureClass::Input,
            codes::STEP_NOT_USABLE,
            format!("The HTTP driver cannot carry out this step as the plan wrote it: {why}."),
            "Compile the target again with the version of the HTTP pack that is enabled.",
        )
    };

    let mut path = String::new();
    for segment in &template.path {
        path.push('/');
        match segment {
            Segment::Literal(literal) => path.push_str(literal),
            Segment::Hole(name) => {
                let position = format!("the path placeholder `{name}`");
                let value = args
                    .get(&format!("path.{name}"))
                    .ok_or_else(|| unusable(format!("no value was given for {position}")))?;
                let text = scalar_text(value)
                    .ok_or_else(|| refused(&position, "it is not one text or number"))?;
                if text.is_empty() {
                    return Err(refused(&position, "it is empty"));
                }
                if text == "." || text == ".." {
                    return Err(refused(&position, "it is a dot segment"));
                }
                if text.contains(['/', '\\']) {
                    return Err(refused(&position, "it holds a path separator"));
                }
                if text.chars().any(char::is_control) {
                    return Err(refused(&position, "it holds a control character"));
                }
                encode(&text, &mut path);
            }
        }
    }
    if template.trailing_slash || template.path.is_empty() {
        path.push('/');
    }

    let mut query = String::new();
    let mut add_query = |name: &str, value: &str| {
        if !query.is_empty() {
            query.push('&');
        }
        encode(name, &mut query);
        query.push('=');
        encode(value, &mut query);
    };
    for (name, value) in &template.query {
        add_query(name, value);
    }
    for name in &step.query {
        match args.get(&format!("query.{name}")) {
            None | Some(Value::Null) => {}
            Some(value) => {
                let text = scalar_text(value).ok_or_else(|| {
                    unusable(format!(
                        "the query value `{name}` is not one text or number"
                    ))
                })?;
                add_query(name, &text);
            }
        }
    }

    let mut headers: Vec<(String, String)> = template
        .headers
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    let header_safe = |text: &str| text.bytes().all(|byte| (0x20..0x7f).contains(&byte));
    if let (Some(header), Some(stamp)) = (&template.idempotency_header, extras.stamp) {
        if !header_safe(stamp) {
            return Err(unusable(
                "the record's idempotency token holds a character a header cannot carry".to_owned(),
            ));
        }
        headers.push((header.clone(), stamp.to_owned()));
    }

    let mut body = None;
    if template.method.has_body() {
        let mut entries: Vec<(String, Json)> = Vec::new();
        for field in &step.body {
            let value = match args.get(&format!("body.{}", field.name)) {
                None | Some(Value::Null) => continue,
                Some(value) => value,
            };
            let json = body_value(value, field.kind).ok_or_else(|| {
                unusable(format!(
                    "the body field `{}` is a {}, and the endpoint declares a {}",
                    field.name,
                    value.kind(),
                    field.kind.word()
                ))
            })?;
            entries.push((field.name.clone(), json));
        }
        if let (Some(VersionSink::Body(field)), Some(version)) =
            (&template.version.send, extras.version)
        {
            entries.retain(|(name, _)| name != field);
            entries.push((field.clone(), version.clone()));
        }
        body = Some(Json::Object(entries).to_text());
        headers.retain(|(name, _)| name != "content-type");
        headers.push(("content-type".to_owned(), "application/json".to_owned()));
    }
    if let (Some(VersionSink::Header(header)), Some(version)) =
        (&template.version.send, extras.version)
    {
        let text = version.scalar_text().filter(|text| header_safe(text));
        let text = text.ok_or_else(|| {
            unusable("the version read for the record cannot be carried in a header".to_owned())
        })?;
        headers.push((header.clone(), text));
    }

    // The compiler refuses a header the connection sets; a plan that holds
    // one all the same is refused here, where it would be sent.
    if let Some((name, _)) = headers.iter().find(|(name, _)| is_connection_header(name)) {
        return Err(unusable(format!(
            "the header `{}` is set by the connection, and no endpoint may write it",
            codes::shown(name)
        )));
    }

    Ok(Built {
        method: template.method,
        path,
        query,
        headers,
        body,
    })
}

/// One value as the JSON a body field of `kind` carries. An exact decimal
/// is written as a number only from its own digits, never through a float.
fn body_value(value: &Value, kind: Kind) -> Option<Json> {
    Some(match (kind, value) {
        (Kind::String | Kind::Any, Value::Text(text)) => Json::Text(text.clone()),
        (Kind::String, Value::Int(number)) => Json::Text(number.to_string()),
        (Kind::String, Value::Decimal(number)) => Json::Text(number.to_string()),
        (Kind::String, Value::Date(date)) => Json::Text(date.to_string()),
        (Kind::Integer | Kind::Number | Kind::Any, Value::Int(number)) => {
            Json::Number(number.to_string())
        }
        (Kind::Number | Kind::Any, Value::Decimal(number)) => {
            let text = number.to_string();
            if !is_plain_number(&text) {
                return None;
            }
            Json::Number(text)
        }
        (Kind::Boolean | Kind::Any, Value::Bool(flag)) => Json::Bool(*flag),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use a2m_core::{Decimal, FailureClass, Value, Values};

    use super::{
        Answers, Auth, BodyField, EndpointTemplate, Extras, Method, RequestStep, Segment,
        VersionRule, build, parse_path,
    };
    use crate::codes;
    use crate::shape::{Kind, Shape};

    fn template(path: &str, method: Method) -> EndpointTemplate {
        let (path, trailing_slash) = parse_path(path).unwrap();
        EndpointTemplate {
            surface: "api".to_owned(),
            method,
            path,
            trailing_slash,
            query: BTreeMap::from([("view".to_owned(), "full list".to_owned())]),
            headers: BTreeMap::from([("accept".to_owned(), "application/json".to_owned())]),
            auth: Auth::None,
            writes: method.has_body(),
            read_asserted: false,
            request: None,
            response: Shape::of(Kind::Any),
            answer: crate::forms::AnswerSpec::default(),
            answers: Answers::default(),
            idempotency_header: Some("idempotency-key".to_owned()),
            version: VersionRule::default(),
            signed: false,
            max_response_bytes: None,
            volatile: Vec::new(),
        }
    }

    #[test]
    fn a_path_template_is_literal_segments_and_whole_segment_holes() {
        let (segments, trailing) = parse_path("/api/orders/{order_id}/items/{item_code}").unwrap();
        assert_eq!(segments.len(), 5);
        assert_eq!(segments[2], Segment::Hole("order_id".to_owned()));
        assert!(!trailing);
        assert_eq!(parse_path("/").unwrap(), (Vec::new(), false));
        assert!(parse_path("/api/").unwrap().1);
        for bad in [
            "api/orders",
            "http://other.example/x",
            "//other.example/x",
            "/a//b",
            "/a/../b",
            "/a/./b",
            "/orders-{id}",
            "/a/{Id}",
            "/a/{id}/{id}",
            "/a?x=1",
            "/a b",
            "/a%2Fb",
            "/a/@b",
        ] {
            assert!(parse_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn no_value_can_change_the_shape_of_the_path() {
        let template = template("/api/orders/{order_id}/items", Method::Get);
        let step = RequestStep::default();
        let with = |value: Value| {
            build(
                &template,
                &step,
                &Values::new().with("path.order_id", value),
                Extras::default(),
            )
        };

        // A value with a separator, a dot segment, an empty value, a control
        // character: each is refused before anything is sent.
        for bad in ["../admin", "a/b", "a\\b", "..", ".", "", "a\nb", "a\u{0}b"] {
            let failure = with(Value::from(bad)).unwrap_err();
            assert_eq!(failure.class(), FailureClass::Input, "{bad:?}");
            assert_eq!(failure.code(), &codes::PATH_VALUE_REFUSED, "{bad:?}");
            assert!(!failure.message().contains("admin"), "{failure}");
        }

        // A host, a query, a fragment, an encoded newline, an encoded slash:
        // each stays one segment, with every special byte encoded again.
        let cases = [
            ("evil.example:80", "evil.example%3A80"),
            ("a?x=1#y", "a%3Fx%3D1%23y"),
            ("a%0d%0aHost: evil", "a%250d%250aHost%3A%20evil"),
            ("a%2fb", "a%252fb"),
            ("@evil", "%40evil"),
            ("é", "%C3%A9"),
        ];
        for (value, encoded) in cases {
            let built = with(Value::from(value)).unwrap();
            assert_eq!(
                built.path,
                format!("/api/orders/{encoded}/items"),
                "{value}"
            );
            assert_eq!(built.path.matches('/').count(), 4, "{value}");
        }
        assert_eq!(with(Value::Int(7)).unwrap().path, "/api/orders/7/items");
        assert!(with(Value::List(Vec::new())).is_err());

        // Dot segments and separators that are already encoded, a second
        // `//`, a user name, a matrix parameter: none is decoded by this
        // pack, so each is encoded once more and stays one segment.
        let cases = [
            ("%2e%2e", "%252e%252e"),
            (".%2e", ".%252e"),
            ("..%2fadmin", "..%252fadmin"),
            ("%5c%5cevil.example", "%255c%255cevil.example"),
            ("user:pw@evil.example", "user%3Apw%40evil.example"),
            ("a;b=c", "a%3Bb%3Dc"),
            ("..;", "..%3B"),
            ("\u{ff0f}evil", "%EF%BC%8Fevil"),
        ];
        for (value, encoded) in cases {
            let built = with(Value::from(value)).unwrap();
            assert_eq!(
                built.path,
                format!("/api/orders/{encoded}/items"),
                "{value}"
            );
        }
        for bad in ["//evil.example", "\\\\evil.example", "a/..", "/"] {
            let failure = with(Value::from(bad)).unwrap_err();
            assert_eq!(failure.code(), &codes::PATH_VALUE_REFUSED, "{bad:?}");
        }
    }

    #[test]
    fn a_header_the_connection_sets_is_refused_where_it_would_be_sent() {
        // The compiler refuses these; a plan that holds one all the same
        // never reaches the connection with it.
        let args = Values::new();
        let step = RequestStep::default();
        for name in ["host", "content-length", "transfer-encoding", "connection"] {
            let mut written = template("/api/items", Method::Get);
            written
                .headers
                .insert(name.to_owned(), "other.example.test".to_owned());
            let mut stamped = template("/api/items", Method::Get);
            stamped.idempotency_header = Some(name.to_owned());
            let mut versioned = template("/api/items", Method::Get);
            versioned.version.send = Some(super::VersionSink::Header(name.to_owned()));
            let version = crate::json::Json::Text("7".to_owned());
            let extras = Extras {
                stamp: Some("stamp-1"),
                version: Some(&version),
            };
            for template in [written, stamped, versioned] {
                let failure = build(&template, &step, &args, extras).unwrap_err();
                assert_eq!(failure.code(), &codes::STEP_NOT_USABLE, "{name}");
            }
        }
    }

    #[test]
    fn query_values_and_body_values_are_encoded_for_their_own_positions() {
        let template = template("/api/items", Method::Put);
        let step = RequestStep {
            query: vec!["q".to_owned(), "skipped".to_owned()],
            version_from: None,
            body: vec![
                BodyField {
                    name: "description".to_owned(),
                    kind: Kind::String,
                },
                BodyField {
                    name: "quantity".to_owned(),
                    kind: Kind::Integer,
                },
                BodyField {
                    name: "length_in".to_owned(),
                    kind: Kind::String,
                },
                BodyField {
                    name: "price".to_owned(),
                    kind: Kind::Number,
                },
            ],
        };
        let args = Values::new()
            .with("query.q", "a&b=c d")
            .with("body.description", "say \"hi\"\n{\"x\":1}")
            .with("body.quantity", 3_i64)
            .with("body.length_in", Decimal::parse("150.50").unwrap())
            .with("body.price", Decimal::parse("0.10").unwrap());
        let built = build(
            &template,
            &step,
            &args,
            Extras {
                stamp: Some("stamp-1"),
                version: None,
            },
        )
        .unwrap();
        assert_eq!(built.query, "view=full%20list&q=a%26b%3Dc%20d");
        let body = built.body.unwrap();
        let back = crate::json::Json::parse(body.as_bytes()).unwrap();
        assert_eq!(
            back.get("description").unwrap().as_text(),
            Some("say \"hi\"\n{\"x\":1}")
        );
        assert!(body.contains("\"quantity\":3"), "{body}");
        assert!(
            built
                .headers
                .contains(&("idempotency-key".to_owned(), "stamp-1".to_owned()))
        );

        // A value of a kind its position cannot carry is refused, never bent.
        let wrong = Values::new().with("body.quantity", "three");
        let failure = build(&template, &step, &wrong, Extras::default()).unwrap_err();
        assert_eq!(failure.code(), &codes::STEP_NOT_USABLE);
    }
}
