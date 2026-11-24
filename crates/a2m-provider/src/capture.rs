//! Traffic capture: a capability a pack may offer, and the form of what a
//! capture yields (decisions 0390, point 10, and 0399).
//!
//! A pack that offers it lists the surface kinds it captures under
//! `points.traffic_capture` in its manifest and registers one
//! [`CaptureSource`]; a pack that lists none is asked nothing, and one
//! that registers a source it did not declare is refused. A source opens a
//! [`TrafficCapture`] on a surface: a recording of the requests the
//! system's own client makes while it is observed, such as the requests a
//! web page's script makes by itself.
//!
//! What a capture yields is structure only (decision 0195): a method, an
//! origin, a path whose every segment that is not a plain word is
//! [`CAPTURE_PLACEHOLDER`], the names of a query's parameters, the names of
//! headers that carry a credential or that the client set, a status, a
//! media type, and the names and kinds of an answer's fields. No value of a
//! query, a header, a cookie, or a body has a field to be put in. Every
//! yield passes [`take_capture`], which the contract applies whatever the
//! pack did: each text that holds a secret the redactor knows (rule 4), or
//! that is not of its structural form, is not kept, and what was not kept
//! is counted and said.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use a2m_core::{Failure, Redact, SurfaceKind};

use crate::BoxFuture;
use crate::driver::{Deadline, SurfaceRef};
use crate::name::ProviderName;

/// What stands in a captured path for a segment that is not kept.
pub const CAPTURE_PLACEHOLDER: &str = "{value}";

/// The longest name a capture keeps: of a query's parameter, a header, or
/// one part of a field's name.
pub const LONGEST_CAPTURED_NAME: usize = 48;

/// The longest path segment a capture keeps as text.
pub const LONGEST_CAPTURED_SEGMENT: usize = 64;

/// The kinds a captured field may have.
pub const FIELD_KINDS: [&str; 8] = [
    "string", "integer", "number", "boolean", "null", "object", "array", "mixed",
];

/// One request a capture saw, by structure alone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapturedRequest {
    /// The method, in capitals: `GET`.
    pub method: String,
    /// The origin: a scheme, `://`, and an authority with no user part.
    pub origin: String,
    /// The path, without its query: `/` and segments, each a plain word or
    /// [`CAPTURE_PLACEHOLDER`].
    pub path: String,
    /// The names of the query's parameters, in order, each once.
    pub query: Vec<String>,
    /// Whether the query held an entry that is not kept.
    pub query_left_out: bool,
    /// The names, in lowercase, of the request's headers that carry a
    /// credential or an anti-forgery token. Never their values.
    pub credential_headers: Vec<String>,
    /// The names, in lowercase, of the request's headers the client set.
    /// Never their values.
    pub set_headers: Vec<String>,
    /// Whether a header's name was not kept.
    pub headers_left_out: bool,
    /// The status of the answer.
    pub status: u16,
    /// The answer's media type, in lowercase, without its parameters;
    /// empty when not known.
    pub media_type: String,
    /// The names of a structured answer's fields, as `name`,
    /// `outer.inner`, `[]`, or `[].name`, each with one of
    /// [`FIELD_KINDS`]. `None` when the answer was not read as structure.
    pub fields: Option<BTreeMap<String, String>>,
    /// Whether names of the answer were withheld: read as a dictionary's
    /// keys, which are values, or not of a name's form.
    pub names_withheld: bool,
}

/// What one take of a capture yields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapturedTraffic {
    /// The requests, oldest first.
    pub requests: Vec<CapturedRequest>,
    /// How many requests were left out because the capture held its most.
    pub dropped: usize,
    /// How many requests were still awaiting their answer at the deadline.
    pub awaited: usize,
    /// How many requests were withheld whole by the contract's check: a
    /// method or an origin that held a secret or was not of its form.
    pub withheld: usize,
}

/// A recording of one surface's traffic. A public contract.
pub trait TrafficCapture: Send + Sync {
    /// Starts recording. Nothing before this is recorded.
    fn start(&self);

    /// What was recorded since the last take, waiting until `until` for
    /// answers still on their way. A caller takes it through
    /// [`take_capture`], never by calling this alone.
    fn take_recorded<'a>(&'a self, until: Deadline) -> BoxFuture<'a, CapturedTraffic>;
}

/// What a pack registers to offer traffic capture. A public contract.
pub trait CaptureSource: Send + Sync {
    /// Opens a capture on the surface, recording, by `deadline`.
    ///
    /// # Errors
    ///
    /// The pack's own failure when the surface cannot be observed.
    fn open<'a>(
        &'a self,
        surface: &'a SurfaceRef,
        deadline: Deadline,
    ) -> BoxFuture<'a, Result<Box<dyn TrafficCapture>, Failure>>;
}

/// Takes what `capture` recorded, and keeps of it only what is structure
/// and holds no secret `redact` knows. The one way a capture's yield is
/// read.
pub async fn take_capture(
    capture: &dyn TrafficCapture,
    until: Deadline,
    redact: &dyn Redact,
) -> CapturedTraffic {
    kept_by_structure(capture.take_recorded(until).await, redact)
}

/// The contract's check of a yield: what [`take_capture`] applies.
#[must_use]
pub fn kept_by_structure(raw: CapturedTraffic, redact: &dyn Redact) -> CapturedTraffic {
    let mut kept = CapturedTraffic {
        requests: Vec::with_capacity(raw.requests.len()),
        dropped: raw.dropped,
        awaited: raw.awaited,
        withheld: raw.withheld,
    };
    for one in raw.requests {
        match kept_request(one, redact) {
            Some(request) => kept.requests.push(request),
            None => kept.withheld += 1,
        }
    }
    kept
}

fn kept_request(raw: CapturedRequest, redact: &dyn Redact) -> Option<CapturedRequest> {
    let clean = |text: &str| !redact.contains_secret(text);
    if !(is_method(&raw.method)
        && clean(&raw.method)
        && is_origin(&raw.origin)
        && clean(&raw.origin))
    {
        return None;
    }
    let mut query_left_out = raw.query_left_out;
    let (path, cut) = kept_path(&raw.path, redact);
    query_left_out |= cut;
    let mut query = Vec::new();
    for name in raw.query {
        if is_captured_name(&name) && clean(&name) && !query.contains(&name) {
            query.push(name);
        } else if !query.contains(&name) {
            query_left_out = true;
        }
    }
    let mut headers_left_out = raw.headers_left_out;
    let mut header_names = |names: Vec<String>| -> Vec<String> {
        let mut out = Vec::new();
        for name in names {
            let lower = name.to_ascii_lowercase();
            if is_captured_name(&lower) && clean(&lower) {
                if !out.contains(&lower) {
                    out.push(lower);
                }
            } else {
                headers_left_out = true;
            }
        }
        out
    };
    let credential_headers = header_names(raw.credential_headers);
    let set_headers = header_names(raw.set_headers);
    let media_type = if is_media_type(&raw.media_type) && clean(&raw.media_type) {
        raw.media_type
    } else {
        String::new()
    };
    let mut names_withheld = raw.names_withheld;
    let fields = raw.fields.map(|fields| {
        fields
            .into_iter()
            .filter(|(name, kind)| {
                let keep =
                    is_field_name(name) && clean(name) && FIELD_KINDS.contains(&kind.as_str());
                names_withheld |= !keep;
                keep
            })
            .collect()
    });
    Some(CapturedRequest {
        method: raw.method,
        origin: raw.origin,
        path,
        query,
        query_left_out,
        credential_headers,
        set_headers,
        headers_left_out,
        status: raw.status,
        media_type,
        fields,
        names_withheld,
    })
}

/// The path with every segment that is not a plain word, or that holds a
/// secret, as the placeholder; and whether a query or a fragment was cut
/// off it.
fn kept_path(path: &str, redact: &dyn Redact) -> (String, bool) {
    let (bare, cut) = match path.find(['?', '#']) {
        Some(at) => (&path[..at], true),
        None => (path, false),
    };
    let mut out = String::from("/");
    let rest = bare.strip_prefix('/').unwrap_or(bare);
    let segments: Vec<&str> = rest.split('/').collect();
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            out.push('/');
        }
        if segment.is_empty() {
            continue;
        }
        if (is_segment(segment) || is_placeholder(segment)) && !redact.contains_secret(segment) {
            out.push_str(segment);
        } else {
            out.push_str(CAPTURE_PLACEHOLDER);
        }
    }
    (out, cut)
}

fn is_method(text: &str) -> bool {
    (1..=16).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_uppercase())
}

fn is_origin(text: &str) -> bool {
    let Some((scheme, authority)) = text.split_once("://") else {
        return false;
    };
    !scheme.is_empty()
        && scheme.bytes().all(|b| b.is_ascii_lowercase())
        && !authority.is_empty()
        && authority.len() <= 255
        && authority
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
}

fn is_segment(text: &str) -> bool {
    text.len() <= LONGEST_CAPTURED_SEGMENT
        && !text.bytes().all(|b| b == b'.')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

fn is_placeholder(text: &str) -> bool {
    text.strip_prefix('{')
        .and_then(|inner| inner.strip_suffix('}'))
        .is_some_and(|inner| {
            !inner.is_empty()
                && inner.len() <= LONGEST_CAPTURED_NAME
                && inner
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// A name a capture keeps: ASCII letters, digits, `_`, `-`, and `.`, at
/// most [`LONGEST_CAPTURED_NAME`].
#[must_use]
pub fn is_captured_name(text: &str) -> bool {
    (1..=LONGEST_CAPTURED_NAME).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn is_field_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .split('.')
            .all(|part| part == "[]" || is_captured_name(part))
}

fn is_media_type(text: &str) -> bool {
    text.split_once('/').is_some_and(|(kind, sub)| {
        let token = |t: &str| {
            !t.is_empty()
                && t.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'+' | b'-')
                })
        };
        text.len() <= 64 && token(kind) && token(sub)
    })
}

/// A registered capture source, with the surface kinds its manifest
/// declares and the host's redactor.
#[derive(Clone)]
pub struct CaptureHandle {
    pub(crate) provider: ProviderName,
    pub(crate) kinds: Vec<SurfaceKind>,
    pub(crate) source: Arc<dyn CaptureSource>,
    pub(crate) redact: Arc<dyn Redact>,
}

impl fmt::Debug for CaptureHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaptureHandle")
            .field("provider", &self.provider)
            .field("kinds", &self.kinds)
            .finish_non_exhaustive()
    }
}

impl CaptureHandle {
    /// The provider that offers it.
    #[must_use]
    pub fn provider(&self) -> &ProviderName {
        &self.provider
    }

    /// The surface kinds its manifest declares it captures.
    #[must_use]
    pub fn kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    /// Opens a capture on the surface. A surface of a kind the manifest
    /// does not declare is never handed to the pack.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` for such a surface; the pack's
    /// failure, redacted.
    pub async fn open(
        &self,
        surface: &SurfaceRef,
        deadline: Deadline,
    ) -> Result<Capturing, Failure> {
        if !self.kinds.contains(&surface.surface.kind) {
            return Err(crate::codes::failure(
                &crate::codes::row::EFFECT_NOT_DECLARED,
                format!(
                    "The provider `{}` does not declare traffic capture of `{}` surfaces.",
                    self.provider, surface.surface.kind
                ),
                "Capture with a provider whose manifest declares the surface's kind under `points.traffic_capture`.",
            ));
        }
        match self.source.open(surface, deadline).await {
            Ok(capture) => Ok(Capturing {
                capture,
                redact: Arc::clone(&self.redact),
            }),
            Err(failure) => Err(self.redact.redact_failure(&failure)),
        }
    }
}

/// An open capture, reached through the registry: every take passes the
/// contract's check with the host's redactor.
pub struct Capturing {
    capture: Box<dyn TrafficCapture>,
    redact: Arc<dyn Redact>,
}

impl fmt::Debug for Capturing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Capturing").finish_non_exhaustive()
    }
}

impl Capturing {
    /// What was recorded since the last take, kept by structure.
    pub async fn take(&self, until: Deadline) -> CapturedTraffic {
        take_capture(&*self.capture, until, &*self.redact).await
    }
}
