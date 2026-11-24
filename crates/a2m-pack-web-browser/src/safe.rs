//! The requests an observing tab lets through although their method could
//! change the system (decision 0394, point 3).
//!
//! An observing tab fails, before it leaves the browser, every request whose
//! method is not `GET`, `HEAD`, or `OPTIONS` (decision 0195), and every
//! request, whatever its method, that carries a header overriding its
//! method (decision 0270, point 2): a `GET` that says `DELETE` is judged by
//! what it says, and passes neither as a read nor as a safe request. A person may
//! say of one request that it changes nothing, and a published version hands
//! the tab that word as a [`SafeRequest`]. A request passes only when it
//! matches one in all four parts:
//!
//! - **the method**, equal;
//! - **the origin**, equal, as [`origin_of`] writes it;
//! - **the path**, against a template segment for segment: a segment written
//!   `{name}` matches one segment of the request's path, and any other is
//!   equal byte for byte. A longer or a shorter path does not match, and a
//!   request whose address carries a query matches nothing;
//! - **the body**: none, or one of the declared encoding whose value fits a
//!   closed [`Shape`], `enum` and all. The bytes are taken from the
//!   interception's own event, decoded only here, and dropped when the
//!   verdict is given: nothing of a body is stored, logged, or put in a
//!   failure.
//!
//! A body is failed when the browser says there is one and does not give it
//! whole, when it is longer than [`LONGEST_SAFE_BODY`], when it is not of the
//! declared media type, when it does not parse in the declared encoding (a
//! name given twice does not parse: a system may read either), or when its
//! value does not fit the shape. A template, a shape, or an origin that is
//! not well formed matches nothing.
//!
//! What the browser gave the interception, measured on Chrome for Testing
//! 153 on Linux: a body of text, of `URLSearchParams`, of a `FormData` of
//! texts and of in-memory files, of a `Blob`, and of an `ArrayBuffer`,
//! sent by `fetch`, by `XMLHttpRequest`, or by `sendBeacon`, came whole in
//! `postDataEntries`, up to two mebibytes and past it; a `ReadableStream`
//! came as one entry with no bytes; an empty body and no body both came as
//! no body. The deprecated `postData` beside it dropped the bytes that are
//! not UTF-8, so it is never read.

use std::fmt;

use a2m_provider::shape::{Kind, Shape};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::origin::{origin_of, path_of};
use crate::page_write::BodyEncoding;
use crate::protocol::{HeldBody, Sent};

/// The longest body of a safe request the tab reads, in bytes: one longer
/// is failed.
pub const LONGEST_SAFE_BODY: usize = 1024 * 1024;

/// The headers that override a request's method, compared without regard to
/// letter case (decision 0270, point 2). A request that carries one is
/// failed, whatever its method and whatever the header names.
const METHOD_OVERRIDES: &[&str] = &[
    "x-http-method-override",
    "x-http-method",
    "x-method-override",
];

/// The body a safe request carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafeBody {
    /// None: a request that carries a body does not match.
    None,
    /// A body of this encoding, whose value fits this shape. The shape's top
    /// level is an object closed by `additionalProperties: false`; any other
    /// matches nothing. A form's values are texts.
    Shaped {
        /// How the body is written, and so its media type.
        encoding: BodyEncoding,
        /// What its value must be.
        shape: Shape,
    },
}

/// One request an observing tab lets through although its method could
/// change the system. Only a person marks a request safe, and only a
/// published version's word reaches a tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeRequest {
    /// The method, in capitals, such as `POST`.
    pub method: String,
    /// The origin, as the browser writes it: `https://app.example.com`, or
    /// `http://127.0.0.1:8080` where the port is not the scheme's own.
    pub origin: String,
    /// The path as a template, from its first `/`, such as
    /// `/api/cases/{case_id}/rows/search`. A segment written `{name}`
    /// matches one segment; it holds no query.
    pub path: String,
    /// The body it carries.
    pub body: SafeBody,
}

/// The requests an observing tab lets through beside GET, HEAD, and
/// OPTIONS. Empty by default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SafeRequests {
    /// Each request marked safe.
    pub allowed: Vec<SafeRequest>,
}

impl SafeRequests {
    /// Whether an observing tab lets this request through: a method that
    /// reads, or a match with one safe request in all four parts. A request
    /// that carries a header overriding its method passes neither way,
    /// whatever method it names: a system that honours the header reads it
    /// as the method it names, and a `GET` that says `DELETE` may write
    /// (decisions 0195 and 0270).
    pub(crate) fn allows(
        &self,
        method: &str,
        address: &str,
        headers: &Map<String, Value>,
        body: &HeldBody,
    ) -> bool {
        if overrides_method(headers) {
            return false;
        }
        if matches!(method, "GET" | "HEAD" | "OPTIONS") {
            return true;
        }
        let candidates: Vec<&SafeRequest> = self
            .allowed
            .iter()
            .filter(|safe| safe.method == method && safe.at(address))
            .collect();
        if candidates.is_empty() {
            return false;
        }
        // The body is decoded once, and only for a request that could pass.
        let sent = body.sent(LONGEST_SAFE_BODY);
        candidates.iter().any(|safe| safe.body.fits(&sent, headers))
    }
}

impl SafeRequest {
    /// Whether `address` is at this request's origin and path, with no
    /// query and no fragment.
    fn at(&self, address: &str) -> bool {
        let Some(origin) = origin_of(address) else {
            return false;
        };
        let after_scheme = address.split_once("://").map_or("", |(_, rest)| rest);
        if origin != self.origin
            || after_scheme.contains(['?', '#'])
            || address.starts_with("blob:")
        {
            return false;
        }
        path_fits(&self.path, path_of(address))
    }
}

impl SafeBody {
    fn fits(&self, sent: &Sent, headers: &Map<String, Value>) -> bool {
        match (self, sent) {
            (Self::None, Sent::Nothing) => true,
            (Self::None, _) | (Self::Shaped { .. }, Sent::Nothing | Sent::NotWhole) => false,
            (Self::Shaped { encoding, shape }, Sent::Whole(bytes)) => {
                if shape.kind != Kind::Object || !shape.closed || shape.nullable {
                    return false;
                }
                if !media_type_is(headers, *encoding) {
                    return false;
                }
                let value = match encoding {
                    BodyEncoding::Json => json_without_twice_named(bytes),
                    BodyEncoding::Form => form_as_texts(bytes),
                };
                value.is_some_and(|value| shape.check(&value).is_ok())
            }
        }
    }
}

/// Whether a request's path fits a template, segment for segment.
fn path_fits(template: &str, path: &str) -> bool {
    let (Some(template), Some(path)) = (template.strip_prefix('/'), path.strip_prefix('/')) else {
        return false;
    };
    if template.contains(['?', '#']) {
        return false;
    }
    let (mut wanted, mut asked) = (template.split('/'), path.split('/'));
    loop {
        match (wanted.next(), asked.next()) {
            (None, None) => return true,
            (Some(want), Some(ask)) if placeholder(want) => {
                if !one_segment(ask) {
                    return false;
                }
            }
            (Some(want), Some(ask)) => {
                if want.contains(['{', '}']) || want != ask {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Whether a template's segment is a placeholder: `{` a name `}`.
fn placeholder(segment: &str) -> bool {
    segment
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

/// Whether a segment of a request's path is one a placeholder takes: not
/// empty, not a step up or a dot alone in any spelling, and with no slash
/// or backslash hidden in an escape, so that no system can read it as more
/// than one segment.
fn one_segment(segment: &str) -> bool {
    let lower = segment.to_ascii_lowercase();
    let dots = lower.replace("%2e", ".");
    !segment.is_empty()
        && !dots.bytes().all(|byte| byte == b'.')
        && !lower.contains("%2f")
        && !lower.contains("%5c")
        && !segment.contains('\\')
}

/// Whether a header overrides the request's method.
fn overrides_method(headers: &Map<String, Value>) -> bool {
    headers
        .keys()
        .any(|name| METHOD_OVERRIDES.contains(&name.to_ascii_lowercase().as_str()))
}

/// Whether the request's one `Content-Type` names the encoding's media type,
/// with no parameter but a UTF-8 `charset`.
fn media_type_is(headers: &Map<String, Value>, encoding: BodyEncoding) -> bool {
    let mut given = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-type"));
    let (Some((_, Value::String(value))), None) = (given.next(), given.next()) else {
        return false;
    };
    let mut parts = value.split(';');
    let media = parts.next().unwrap_or_default().trim();
    let wanted = match encoding {
        BodyEncoding::Json => "application/json",
        BodyEncoding::Form => "application/x-www-form-urlencoded",
    };
    media.eq_ignore_ascii_case(wanted)
        && parts.all(|parameter| {
            parameter.split_once('=').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case("charset")
                    && value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8")
            })
        })
}

/// A JSON body as a value, or `None` when it is not UTF-8, not JSON, or
/// names one member twice in an object.
fn json_without_twice_named(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut reader = serde_json::Deserializer::from_str(text);
    let value = Once::deserialize(&mut reader).ok()?.0;
    reader.end().ok()?;
    Some(value)
}

/// A form body as a flat object of texts, or `None` when an entry has no
/// `=`, an escape is not one, a text is not UTF-8, or a name is given twice.
fn form_as_texts(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut fields = Map::new();
    if text.is_empty() {
        return Some(Value::Object(fields));
    }
    for entry in text.split('&') {
        let (name, value) = entry.split_once('=')?;
        let name = unescape_form(name)?;
        if name.is_empty() || fields.contains_key(&name) {
            return None;
        }
        fields.insert(name, Value::String(unescape_form(value)?));
    }
    Some(Value::Object(fields))
}

/// A form's text as a server reads it: `+` is a space and `%XX` a byte.
fn unescape_form(text: &str) -> Option<String> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => out.push(b' '),
            b'%' => {
                let high = hex(bytes.next()?)?;
                let low = hex(bytes.next()?)?;
                out.push(high << 4 | low);
            }
            other => out.push(other),
        }
    }
    String::from_utf8(out).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// A JSON value read with every object's names checked to be given once.
struct Once(Value);

impl<'de> Deserialize<'de> for Once {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        reader.deserialize_any(OnceVisitor).map(Once)
    }
}

struct OnceVisitor;

impl<'de> serde::de::Visitor<'de> for OnceVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value whose objects name each member once")
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("not a number JSON can hold"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut items: A) -> Result<Value, A::Error> {
        let mut list = Vec::new();
        while let Some(Once(item)) = items.next_element()? {
            list.push(item);
        }
        Ok(Value::Array(list))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut members: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(name) = members.next_key::<String>()? {
            let Once(value) = members.next_value()?;
            if object.insert(name, value).is_some() {
                return Err(serde::de::Error::custom("a member named twice"));
            }
        }
        Ok(Value::Object(object))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::protocol::{PausedRequest, decode_base64};

    const ORIGIN: &str = "https://app.example.com";
    const SEARCH: &str = "https://app.example.com/api/cases/rows/search";

    fn encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for group in bytes.chunks(3) {
            let word = group.iter().enumerate().fold(0u32, |word, (at, byte)| {
                word | u32::from(*byte) << (16 - 8 * at)
            });
            for at in 0..4 {
                if at <= group.len() {
                    out.push(char::from(ALPHABET[(word >> (18 - 6 * at)) as usize & 63]));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// A request as the browser hands it to the interception, as measured.
    fn sent(
        method: &str,
        url: &str,
        content_type: Option<&str>,
        body: Option<&[u8]>,
    ) -> PausedRequest {
        let mut request = json!({"url": url, "method": method, "headers": {}});
        if let Some(kind) = content_type {
            request["headers"]["Content-Type"] = json!(kind);
        }
        if let Some(body) = body {
            request["hasPostData"] = json!(true);
            request["postDataEntries"] = json!([{"bytes": encode(body)}]);
        }
        serde_json::from_value(request).unwrap()
    }

    fn allows(safe: &SafeRequests, request: &PausedRequest) -> bool {
        safe.allows(
            &request.method,
            &request.url,
            &request.headers,
            &request.body,
        )
    }

    fn text() -> Shape {
        Shape::of(Kind::String)
    }

    /// `{"status": text, "op": "search"}`, both required, closed.
    fn search_shape() -> Shape {
        let mut op = text();
        op.one_of = vec!["search".to_owned()];
        Shape {
            properties: BTreeMap::from([("status".to_owned(), text()), ("op".to_owned(), op)]),
            required: vec!["status".to_owned(), "op".to_owned()],
            closed: true,
            ..Shape::of(Kind::Object)
        }
    }

    fn one(path: &str, body: SafeBody) -> SafeRequests {
        SafeRequests {
            allowed: vec![SafeRequest {
                method: "POST".to_owned(),
                origin: ORIGIN.to_owned(),
                path: path.to_owned(),
                body,
            }],
        }
    }

    fn json_search() -> SafeRequests {
        one(
            "/api/cases/rows/search",
            SafeBody::Shaped {
                encoding: BodyEncoding::Json,
                shape: search_shape(),
            },
        )
    }

    const JSON: Option<&str> = Some("application/json");
    const GOOD: &[u8] = br#"{"status":"open","op":"search"}"#;

    #[test]
    fn a_safe_request_is_matched_by_method_origin_and_template_segment_for_segment() {
        let none = |path: &str| one(path, SafeBody::None);
        let post = |url: &str| sent("POST", url, None, None);
        let exact = none("/api/cases/rows/search");
        assert!(allows(&exact, &post(SEARCH)));
        assert!(allows(
            &exact,
            &post("https://APP.example.com:443/api/cases/rows/search")
        ));
        for refused in [
            // A longer path, a shorter one, one that only begins the same.
            "https://app.example.com/api/cases/rows/search/all",
            "https://app.example.com/api/cases/rows",
            "https://app.example.com/api/cases/rows/search-and-archive",
            "https://app.example.com/api/cases/rows/search/",
            "https://app.example.com//api/cases/rows/search",
            // Another origin.
            "http://app.example.com/api/cases/rows/search",
            "https://app.example.com:8443/api/cases/rows/search",
            "https://app.example.com.other.example/api/cases/rows/search",
            "https://other.example/api/cases/rows/search",
            // A query, even an empty one, and a fragment.
            "https://app.example.com/api/cases/rows/search?op=archive",
            "https://app.example.com/api/cases/rows/search?",
            "https://app.example.com/api/cases/rows/search#top",
            "blob:https://app.example.com/api/cases/rows/search",
        ] {
            assert!(!allows(&exact, &post(refused)), "{refused}");
        }
        // Another method.
        assert!(!allows(&exact, &sent("PUT", SEARCH, None, None)));
        assert!(!allows(&exact, &sent("post", SEARCH, None, None)));

        // A placeholder takes one plain segment, and nothing that could be
        // read as more or less than one.
        let held = none("/api/cases/{case_id}/rows");
        assert!(allows(
            &held,
            &post("https://app.example.com/api/cases/C-1/rows")
        ));
        assert!(allows(
            &held,
            &post("https://app.example.com/api/cases/C%201/rows")
        ));
        for refused in [
            "https://app.example.com/api/cases//rows",
            "https://app.example.com/api/cases/C-1/C-2/rows",
            "https://app.example.com/api/cases/rows",
            "https://app.example.com/api/cases/../rows",
            "https://app.example.com/api/cases/%2e%2E/rows",
            "https://app.example.com/api/cases/./rows",
            "https://app.example.com/api/cases/C-1%2Farchive/rows",
            "https://app.example.com/api/cases/C-1%5carchive/rows",
            "https://app.example.com/api/cases/C-1\\x/rows",
        ] {
            assert!(!allows(&held, &post(refused)), "{refused}");
        }
        // A template that is not well formed matches nothing.
        for bad in [
            "api/cases/rows/search",
            "/api/cases/{}/rows",
            "/api/cases/{case id}/rows",
            "/api/cases/x{case_id}/rows",
            "/api/cases/rows/search?op=search",
            "",
        ] {
            let request = post("https://app.example.com/api/cases/C-1/rows");
            assert!(!allows(&none(bad), &request), "{bad}");
            assert!(!allows(&none(bad), &post(SEARCH)), "{bad}");
        }
        // An origin not written as the browser writes it matches nothing.
        let mut loud = none("/api/cases/rows/search");
        loud.allowed[0].origin = "https://app.example.com:443".to_owned();
        assert!(!allows(&loud, &post(SEARCH)));
        // A read needs no mark, and an empty list lets nothing else through.
        assert!(allows(
            &SafeRequests::default(),
            &sent("GET", SEARCH, None, None)
        ));
        // A read that carries a method override is not a read: it fails
        // whatever it names, and whatever is listed (decision 0270).
        for method in ["GET", "HEAD", "OPTIONS"] {
            for name in [
                "X-HTTP-Method-Override",
                "x-http-method",
                "X-METHOD-OVERRIDE",
            ] {
                for named in ["DELETE", "GET"] {
                    let mut over = sent(method, SEARCH, None, None);
                    over.headers.insert(name.to_owned(), json!(named));
                    assert!(
                        !allows(&SafeRequests::default(), &over),
                        "{method} {name}: {named}"
                    );
                    assert!(!allows(&json_search(), &over), "{method} {name}: {named}");
                }
            }
        }
        assert!(!allows(&SafeRequests::default(), &post(SEARCH)));
    }

    #[test]
    fn a_safe_requests_body_passes_only_whole_of_its_media_type_and_fitting_its_closed_shape() {
        let safe = json_search();
        assert!(allows(&safe, &sent("POST", SEARCH, JSON, Some(GOOD))));
        assert!(allows(
            &safe,
            &sent(
                "POST",
                SEARCH,
                Some("application/json; charset=UTF-8"),
                Some(GOOD)
            )
        ));
        // The same text in another spelling is the same value.
        let escaped = br#"{ "op" : "search", "status": "closed" }"#;
        assert!(allows(&safe, &sent("POST", SEARCH, JSON, Some(escaped))));

        for (why, body) in [
            (
                "a value outside the enum",
                &br#"{"status":"open","op":"archive"}"#[..],
            ),
            ("the same, escaped", br#"{"status":"open","op":"archive"}"#),
            (
                "a field the shape does not name",
                br#"{"status":"open","op":"search","all":true}"#,
            ),
            ("a field of another kind", br#"{"status":1,"op":"search"}"#),
            ("a required field missing", br#"{"op":"search"}"#),
            (
                "a name given twice",
                br#"{"status":"open","op":"search","op":"archive"}"#,
            ),
            (
                "a name given twice, the other way",
                br#"{"status":"open","op":"archive","op":"search"}"#,
            ),
            ("not JSON", b"status=open&op=search"),
            ("two values", br#"{"status":"open","op":"search"} {}"#),
            ("not an object", br#"["open","search"]"#),
            ("not UTF-8", b"{\"status\":\"\xff\",\"op\":\"search\"}"),
            ("a lone surrogate", br#"{"status":"\ud800","op":"search"}"#),
            ("empty", b""),
        ] {
            assert!(
                !allows(&safe, &sent("POST", SEARCH, JSON, Some(body))),
                "{why}"
            );
        }

        // Of another media type, of two, or with no type.
        for kind in [
            None,
            Some("text/plain;charset=UTF-8"),
            Some("application/x-www-form-urlencoded"),
            Some("application/json; charset=utf-16"),
            Some("application/json; boundary=x"),
            Some("application/jsonx"),
        ] {
            assert!(
                !allows(&safe, &sent("POST", SEARCH, kind, Some(GOOD))),
                "{kind:?}"
            );
        }
        let mut twice = sent("POST", SEARCH, JSON, Some(GOOD));
        twice
            .headers
            .insert("content-type".to_owned(), json!("application/json"));
        assert!(!allows(&safe, &twice));

        // A method override, whatever it names.
        for name in [
            "X-HTTP-Method-Override",
            "x-http-method",
            "X-Method-Override",
        ] {
            let mut over = sent("POST", SEARCH, JSON, Some(GOOD));
            over.headers.insert(name.to_owned(), json!("POST"));
            assert!(!allows(&safe, &over), "{name}");
        }

        // A body the browser does not give whole: a stream's one entry with
        // no bytes, a flag with no entries, entries with no flag, and a
        // form of event not known.
        for event in [
            json!({"hasPostData": true, "postDataEntries": [{}]}),
            json!({"hasPostData": true}),
            json!({"hasPostData": true, "postDataEntries": []}),
            json!({"postDataEntries": [{"bytes": encode(GOOD)}]}),
            json!({"hasPostData": "yes", "postDataEntries": [{"bytes": encode(GOOD)}]}),
            json!({"hasPostData": true, "postDataEntries": [{"bytes": encode(GOOD)}, {}]}),
            json!({"hasPostData": true, "postDataEntries": [{"bytes": "not base64!"}]}),
            json!({"hasPostData": true, "postDataEntries": {"bytes": encode(GOOD)}}),
            json!({"hasPostData": true, "postData": String::from_utf8_lossy(GOOD)}),
        ] {
            let mut request = json!({"url": SEARCH, "method": "POST", "headers": {"Content-Type": "application/json"}});
            request
                .as_object_mut()
                .unwrap()
                .extend(event.as_object().unwrap().clone());
            let request: PausedRequest = serde_json::from_value(request).unwrap();
            assert!(!allows(&safe, &request), "{event}");
        }
        // The parts the browser gives are read as one body.
        let split = json!({"url": SEARCH, "method": "POST", "headers": {"Content-Type": "application/json"},
            "hasPostData": true, "postDataEntries": [{"bytes": encode(&GOOD[..7])}, {"bytes": encode(&GOOD[7..])}]});
        assert!(allows(&safe, &serde_json::from_value(split).unwrap()));

        // A body past the bound, even of spaces around a fitting value.
        let mut long = vec![b' '; LONGEST_SAFE_BODY];
        long.extend_from_slice(GOOD);
        assert!(!allows(&safe, &sent("POST", SEARCH, JSON, Some(&long))));
        let mut within = vec![b' '; LONGEST_SAFE_BODY - GOOD.len()];
        within.extend_from_slice(GOOD);
        assert!(allows(&safe, &sent("POST", SEARCH, JSON, Some(&within))));

        // No body where one is declared, and one where none is.
        assert!(!allows(&safe, &sent("POST", SEARCH, JSON, None)));
        let bare = one("/api/cases/rows/search", SafeBody::None);
        assert!(!allows(&bare, &sent("POST", SEARCH, JSON, Some(GOOD))));
        assert!(!allows(&bare, &sent("POST", SEARCH, None, Some(b""))));

        // A shape whose top level is not a closed object matches nothing.
        for open in [
            Shape {
                closed: false,
                ..search_shape()
            },
            Shape {
                nullable: true,
                ..search_shape()
            },
            Shape::of(Kind::Any),
            Shape::of(Kind::String),
        ] {
            let safe = one(
                "/api/cases/rows/search",
                SafeBody::Shaped {
                    encoding: BodyEncoding::Json,
                    shape: open,
                },
            );
            assert!(!allows(&safe, &sent("POST", SEARCH, JSON, Some(GOOD))));
        }
    }

    #[test]
    fn a_form_body_is_a_flat_object_of_texts_each_name_once() {
        const FORM: Option<&str> = Some("application/x-www-form-urlencoded;charset=UTF-8");
        let safe = one(
            "/api/cases/rows/search",
            SafeBody::Shaped {
                encoding: BodyEncoding::Form,
                shape: search_shape(),
            },
        );
        assert!(allows(
            &safe,
            &sent("POST", SEARCH, FORM, Some(b"status=open&op=search"))
        ));
        assert!(allows(
            &safe,
            &sent("POST", SEARCH, FORM, Some(b"status=on+hold&op=se%61rch"))
        ));
        for body in [
            &b"status=open&op=archive"[..],
            b"status=open&op=%61rchive",
            b"status=open&op=search&op=archive",
            b"status=open&op=search&all=1",
            b"status=open&op=search&",
            b"status=open&op",
            b"status=open&op=search%",
            b"status=open&op=search%zz",
            b"status=%ff&op=search",
            b"op=search",
            br#"{"status":"open","op":"search"}"#,
        ] {
            let shown = String::from_utf8_lossy(body);
            assert!(
                !allows(&safe, &sent("POST", SEARCH, FORM, Some(body))),
                "{shown}"
            );
        }
        assert!(!allows(
            &safe,
            &sent("POST", SEARCH, JSON, Some(b"status=open&op=search"))
        ));
        // A form's values are texts: a field declared as a number never fits.
        let mut counted = search_shape();
        counted
            .properties
            .insert("status".to_owned(), Shape::of(Kind::Integer));
        let safe = one(
            "/api/cases/rows/search",
            SafeBody::Shaped {
                encoding: BodyEncoding::Form,
                shape: counted,
            },
        );
        assert!(!allows(
            &safe,
            &sent("POST", SEARCH, FORM, Some(b"status=1&op=search"))
        ));
    }

    #[test]
    fn a_held_body_prints_no_byte_of_itself() {
        let request = sent(
            "POST",
            SEARCH,
            JSON,
            Some(br#"{"password":"secret-for-tests"}"#),
        );
        let printed = format!("{request:?}");
        assert!(!printed.contains("secret-for-tests"), "{printed}");
        assert!(
            !printed.contains(&encode(br#"{"password":"secret-for-tests"}"#)),
            "{printed}"
        );
        assert!(printed.contains("HeldBody(1 entries)"), "{printed}");
    }

    #[test]
    fn base64_is_read_as_the_browser_writes_it() {
        for (text, bytes) in [
            ("", &b""[..]),
            ("YWJj", b"abc"),
            ("YWI=", b"ab"),
            ("YQ==", b"a"),
            ("e//+fQ==", &[0x7b, 0xff, 0xfe, 0x7d]),
        ] {
            let mut out = Vec::new();
            assert!(decode_base64(text, &mut out), "{text}");
            assert_eq!(out, bytes, "{text}");
            assert_eq!(encode(bytes), text);
        }
        for bad in ["YWJ", "Y===", "YQ==YWJj", "YW J", "YWJ-"] {
            assert!(!decode_base64(bad, &mut Vec::new()), "{bad}");
        }
    }
}
