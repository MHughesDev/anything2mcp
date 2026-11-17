//! The driver of the `http` mode: one request per `request` step, built
//! from the endpoint's template, sent to the surface's own address and
//! nowhere else, with the answer held for the `validate` and `extract`
//! steps that follow it.
//!
//! What the driver never does: follow a redirect, retry, renew a session,
//! send to an address the run may not reach, or put a credential anywhere
//! but the one request it was placed for.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_auth::{ChallengeScheme, CredentialSink, RawSign, SessionLease};
use a2m_core::{
    Failure, FailureClass, FailureCode, Redact, Secret, Step, SurfaceIndex, Value, Values,
};
use a2m_provider::{
    BoxFuture, Capture, Cleanup, Driver, DriverHealth, Observed, PrepareContext, Prepared,
    StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use a2m_telemetry::{Acting, Detail, record_headers};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::codes;
use crate::endpoints::{ENDPOINT_KIND, PACK};
use crate::json::Json;
use crate::lower::{EXTRACT, PROBE, REQUEST, VALIDATE};
use crate::template::{
    Auth, Built, EndpointSection, EndpointTemplate, ExtractStep, Extras, Method, PAYLOAD_FORMAT,
    ProbeStep, RequestStep, Take, VersionSource, build, parse_path,
};

/// The most warm clients the driver keeps.
const MOST_CLIENTS: usize = 64;
/// The most text of a probe's answer that is handed to a discoverer.
const MOST_PROBE_TEXT: usize = 4_000_000;

/// The address of a surface, taken apart once, when the driver prepares.
/// Every request of the pass goes here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Origin {
    /// `http://host` or `http://host:port`, as the URL type writes it.
    pub text: String,
    /// The path the surface's address ends in, without a trailing slash.
    pub prefix: String,
}

impl Origin {
    /// Takes a surface's address apart.
    pub(crate) fn of(address: &str) -> Result<Self, Failure> {
        let unusable = |why: &str| {
            codes::failure(
                FailureClass::Input,
                codes::STEP_NOT_USABLE,
                format!("The surface's address cannot be used by the HTTP pack: {why}."),
                "Write the surface's `base` as `http://host`, with a port and a path if it has them.",
            )
        };
        let url = reqwest::Url::parse(address).map_err(|_| unusable("it is not an address"))?;
        match url.scheme() {
            "http" => {}
            "https" => return Err(codes::tls_not_enabled()),
            _ => return Err(unusable("it is not an HTTP address")),
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(unusable("it holds a user name or a password"));
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(unusable("it holds a query or a fragment"));
        }
        if url.host_str().is_none() {
            return Err(unusable("it names no host"));
        }
        let prefix = url.path().trim_end_matches('/').to_owned();
        if !prefix.is_empty() && parse_path(&prefix).is_err() {
            return Err(unusable("its path is not plain segments"));
        }
        Ok(Self {
            text: url.origin().ascii_serialization(),
            prefix,
        })
    }
}

/// The credentials the lease placed for one pass. They are held in memory
/// by the prepared client only, are wiped when it is released, and go into
/// a request only for an endpoint that takes the session's credential.
#[derive(Default)]
struct Held {
    headers: Vec<(String, Secret)>,
    cookies: Vec<(String, String, Secret)>,
    /// A credential the session placed that this driver cannot use.
    cannot: Option<Failure>,
}

impl fmt::Debug for Held {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Held")
            .field("headers", &self.headers.len())
            .field("cookies", &self.cookies.len())
            .finish()
    }
}

impl CredentialSink for Held {
    fn header(&mut self, name: &str, value: &Secret) {
        self.headers
            .push((name.to_owned(), Secret::new(value.expose())));
    }

    fn cookie(&mut self, origin: &str, name: &str, value: &Secret) {
        self.cookies.push((
            origin.trim_end_matches('/').to_owned(),
            name.to_owned(),
            Secret::new(value.expose()),
        ));
    }

    fn tls_identity(&mut self, _certificate: &[u8], _key: &Secret) {
        self.cannot = Some(codes::tls_not_enabled());
    }

    fn challenge_credential(&mut self, scheme: ChallengeScheme, _user: &str, _secret: &Secret) {
        self.cannot = Some(codes::failure(
            FailureClass::Input,
            codes::AUTH_SCHEME_NOT_BUILT,
            format!(
                "The session signs in by the `{}` scheme, whose exchange this version of the HTTP pack does not carry out.",
                scheme.as_str()
            ),
            "Use a provider that places a header or a cookie, such as `api_key`, `bearer_static`, or `http_basic`.",
        ));
    }
}

/// What tells the driver that the session has ended.
#[derive(Debug, Default)]
struct Expiry {
    /// Statuses the session's provider takes for a refusal of its credential.
    provider_statuses: Vec<u16>,
    /// Statuses the target declares, each with the start of the paths it
    /// holds for.
    declared_statuses: Vec<(u16, String)>,
    /// Hosts the target declares it is sent to in order to sign in.
    redirect_hosts: Vec<String>,
}

/// An answer, held between the `request` step and the steps that check it.
struct Answer {
    endpoint: String,
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// The body, read as JSON, once `validate` accepted it.
    json: Option<Json>,
    /// Whether the status is one the endpoint declares for a record that
    /// does not exist, on a lookup.
    absent: bool,
    /// The version that was sent with the request, as text.
    sent_version: Option<String>,
    /// Whether the request was a commit point: a write that was sent, whose
    /// verdict the steps that follow are to read.
    wrote: bool,
}

/// An answer's headers and body can hold a cookie or a token. What is
/// printed of one is its endpoint, its status, and sizes.
impl fmt::Debug for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Answer")
            .field("endpoint", &self.endpoint)
            .field("status", &self.status)
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .field("absent", &self.absent)
            .field("wrote", &self.wrote)
            .finish_non_exhaustive()
    }
}

impl Answer {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value.as_str())
    }
}

/// What `prepare` acquires: the client, the surface's address, the
/// endpoints of the plan, and the pass's own state.
struct Session {
    client: reqwest::Client,
    client_key: String,
    origin: Origin,
    surface: String,
    sections: BTreeMap<usize, EndpointSection>,
    held: Option<Held>,
    expiry: Expiry,
    answer: Option<Answer>,
    /// What this pass knows of a record's version: by the endpoint that
    /// read the record and the record's key.
    versions: BTreeMap<(String, Vec<u8>), Known>,
}

/// What a pass knows of one record's version, from reading the record
/// through one endpoint. Nothing is held for a record the pass did not
/// read, and a write that sends a version is not sent for such a record.
#[derive(Clone, PartialEq, Eq)]
enum Known {
    /// The record was read, and this is its version.
    Version(Json),
    /// The record was looked up and does not exist: a write creates it,
    /// and has no version to send.
    Absent,
}

/// The driver of the `http` mode.
pub(crate) struct HttpDriver {
    redact: Arc<dyn Redact>,
    max_response_bytes: u64,
    /// Warm clients, by session and origin.
    clients: Mutex<BTreeMap<String, reqwest::Client>>,
}

fn failure_of(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next: &str,
) -> Failure {
    Failure::new(class, code, message, next)
}

fn needs_login(why: &str) -> Failure {
    failure_of(
        FailureClass::SessionExpired,
        a2m_auth::codes::NEEDS_LOGIN,
        format!("The session cannot be used: {why}."),
        "Sign in again, then run the tool again or resume the run.",
    )
}

/// Where a pass keeps what one endpoint read of the record a step is for.
/// `None` for a step that is for no record: a version is never kept, or
/// taken, under no key.
fn version_slot(endpoint: &str, input: &StepInput<'_>) -> Option<(String, Vec<u8>)> {
    input
        .key
        .map(|key| (endpoint.to_owned(), key.canonical_bytes()))
}

impl HttpDriver {
    pub(crate) fn new(redact: Arc<dyn Redact>, max_response_bytes: u64) -> Self {
        Self {
            redact,
            max_response_bytes,
            clients: Mutex::new(BTreeMap::new()),
        }
    }

    /// The warm client of a session and an origin, or a new one.
    fn client(&self, key: &str) -> Result<reqwest::Client, Failure> {
        let mut clients = self
            .clients
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(client) = clients.get(key) {
            return Ok(client.clone());
        }
        // No redirect is followed, and no proxy is used: either would send
        // the request somewhere the plan does not name.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .referer(false)
            .user_agent(concat!("anything2mcp/", env!("CARGO_PKG_VERSION")))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| {
                failure_of(
                    FailureClass::Input,
                    codes::STEP_NOT_USABLE,
                    "The HTTP client could not be set up on this machine.",
                    "Report this to the HTTP pack's author.",
                )
            })?;
        while clients.len() >= MOST_CLIENTS {
            if clients.pop_first().is_none() {
                break;
            }
        }
        clients.insert(key.to_owned(), client.clone());
        Ok(client)
    }

    fn forget_client(&self, key: &str) {
        self.clients
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key);
    }

    fn prepared(
        &self,
        cx: &PrepareContext<'_>,
        lease: Option<&SessionLease>,
    ) -> Result<Session, StepFailure> {
        let unsent = StepFailure::not_sent;
        let surface = &cx.surface.surface;
        let origin = Origin::of(&surface.address).map_err(unsent)?;
        if !cx.destinations.contains(&surface.address) {
            return Err(unsent(codes::surface_not_declared(&format!(
                "The address of the surface `{}`",
                surface.name
            ))));
        }
        if cx.deadline.passed() {
            return Err(unsent(failure_of(
                FailureClass::Transient,
                a2m_provider::codes::TIMEOUT,
                "The time for preparing the HTTP client had passed before it began.",
                "Run the tool again.",
            )));
        }

        let mut sections = BTreeMap::new();
        for (position, section) in cx.plan.packs.iter().enumerate() {
            let own = section.pack == PACK
                && section.kind == ENDPOINT_KIND
                && section.surface_kind == surface.kind;
            if !own {
                continue;
            }
            if section.format != PAYLOAD_FORMAT {
                return Err(codes::not_usable(format!(
                    "its endpoints are in format {}, and this pack reads format {PAYLOAD_FORMAT}",
                    section.format
                )));
            }
            sections.insert(
                position,
                section
                    .payload
                    .decode::<EndpointSection>()
                    .map_err(|_| codes::not_usable("its endpoints do not decode"))?,
            );
        }

        let mut expiry = Expiry::default();
        let mut held = None;
        let mut client_key = format!("- {}", origin.text);
        if let Some(lease) = lease {
            let mut placed = Held::default();
            lease.apply(&mut placed).map_err(unsent)?;
            held = Some(placed);
            client_key = format!("{} {}", lease.session(), origin.text);
            for code in [401_u16, 403] {
                let sign = RawSign::Status {
                    code,
                    challenge: None,
                };
                if lease.detect_expiry(&sign).is_some() {
                    expiry.provider_statuses.push(code);
                }
            }
            // What the target's auth file declares for this surface.
            let index = cx
                .plan
                .surfaces
                .iter()
                .position(|known| known.name == surface.name)
                .and_then(|position| u32::try_from(position).ok())
                .map(SurfaceIndex::new);
            let signals = index
                .and_then(|index| cx.plan.auth_of(index))
                .map(|auth| auth.expiry_signals.as_slice())
                .unwrap_or_default();
            for signal in signals {
                let Ok(signal) = Json::parse(signal.as_bytes()) else {
                    continue;
                };
                if let Some(host) = signal.get("redirect_host").and_then(Json::as_text) {
                    expiry.redirect_hosts.push(host.to_ascii_lowercase());
                }
                let status = signal
                    .get("status")
                    .and_then(Json::scalar_text)
                    .and_then(|text| text.parse::<u16>().ok());
                if let Some(status) = status {
                    let path = signal.get("path").and_then(Json::as_text).unwrap_or("/");
                    expiry.declared_statuses.push((status, path.to_owned()));
                }
            }
        }

        Ok(Session {
            client: self.client(&client_key).map_err(unsent)?,
            client_key,
            origin,
            surface: surface.name.as_str().to_owned(),
            sections,
            held,
            expiry,
            answer: None,
            versions: BTreeMap::new(),
        })
    }
}

/// The endpoint a step names.
fn endpoint<'a>(
    session: &'a Session,
    step: &Step,
) -> Result<(&'a str, &'a EndpointTemplate), StepFailure> {
    let named = step
        .refers_to
        .as_ref()
        .ok_or_else(|| codes::not_usable("it names no endpoint"))?;
    session
        .sections
        .get(&named.section.get())
        .and_then(|section| section.get_key_value(&named.entry))
        .map(|(name, template)| (name.as_str(), template))
        .ok_or_else(|| codes::not_usable("the endpoint it names is not in the plan"))
}

/// When a step must be done: the earlier of its own timeout and the
/// deadline it was handed.
fn until(step: &Step, input: &StepInput<'_>) -> tokio::time::Instant {
    let own = tokio::time::Instant::now() + Duration::from_millis(u64::from(step.timeout_ms));
    own.min(input.deadline.at())
}

/// How a request ended when no complete answer came back.
enum Lost {
    /// Nothing left this machine.
    BeforeSending,
    /// The request may have been sent, wholly or in part.
    AfterSending,
    /// The time ran out; the request may have been sent.
    TimedOut,
}

/// A write that was sent, with no answer that is the application's
/// verdict on it. Whether it was made is not known: the record is in
/// doubt, and reading it back decides. The class is never one the ledger
/// takes as settled.
fn in_doubt(endpoint: &str, why: &str) -> StepFailure {
    StepFailure::unknown(failure_of(
        FailureClass::PossibleCommit,
        a2m_provider::codes::NO_CONFIRMATION,
        format!("The write to the endpoint `{endpoint}` was sent, and {why}."),
        "Reconcile the record: read it back before anything is written again.",
    ))
}

fn lost(how: &Lost, commits: bool, endpoint: &str) -> StepFailure {
    match (how, commits) {
        (Lost::BeforeSending, _) => StepFailure::not_sent(failure_of(
            FailureClass::Transient,
            a2m_provider::codes::CONNECTION_LOST,
            format!("No connection could be made for the endpoint `{endpoint}`; nothing was sent."),
            "Check that the system is reachable, then run the tool again.",
        )),
        (Lost::AfterSending | Lost::TimedOut, true) => {
            in_doubt(endpoint, "no complete answer came back")
        }
        (Lost::AfterSending, false) => StepFailure::unknown(failure_of(
            FailureClass::Transient,
            a2m_provider::codes::CONNECTION_LOST,
            format!(
                "The connection was lost before the endpoint `{endpoint}` had answered in full."
            ),
            "Run the tool again.",
        )),
        (Lost::TimedOut, false) => StepFailure::unknown(failure_of(
            FailureClass::Transient,
            a2m_provider::codes::TIMEOUT,
            format!("The endpoint `{endpoint}` did not answer in the time the step allows."),
            "Run the tool again, or raise the step's timeout in the target's defaults.",
        )),
    }
}

/// An answer as it came off the connection.
struct Received {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Why no answer was received.
enum NotReceived {
    Lost(Lost),
    TooLarge,
    Refused(Failure),
}

/// What an answer means for the session or the surface, when it means
/// anything before its status is judged.
enum Stop {
    /// A sign the target or the session's provider declares: the session
    /// has ended.
    Expired(Failure),
    /// A redirect to an address that is not on the surface. It is never
    /// followed.
    LeftSurface,
}

impl Session {
    /// The address of a built request: the surface's origin, the path its
    /// address ends in, and the path and the query that were built.
    ///
    /// Checked again before sending: what the URL type reads back is the
    /// surface's origin and exactly the path and the query that were
    /// built. A dot segment that would be folded away, a separator that
    /// would start a query, a fragment, or a host, and a byte that would be
    /// encoded again all change what is read back, and are refused.
    fn address(&self, built: &Built) -> Result<reqwest::Url, Failure> {
        let path = format!("{}{}", self.origin.prefix, built.path);
        let mut address = format!("{}{path}", self.origin.text);
        if !built.query.is_empty() {
            address.push('?');
            address.push_str(&built.query);
        }
        let query = (!built.query.is_empty()).then_some(built.query.as_str());
        reqwest::Url::parse(&address)
            .ok()
            .filter(|url| {
                url.origin().ascii_serialization() == self.origin.text
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.path() == path
                    && url.query() == query
                    && url.fragment().is_none()
            })
            .ok_or_else(|| codes::surface_not_declared("The address of the request"))
    }

    /// Sends one built request to the surface's address and reads the
    /// answer, up to `most` bytes, by `until`.
    async fn send(
        &self,
        built: &Built,
        credentials: Option<&Held>,
        most: u64,
        until: tokio::time::Instant,
    ) -> Result<Received, NotReceived> {
        let refused = |why: &str| {
            NotReceived::Refused(failure_of(
                FailureClass::Input,
                codes::STEP_NOT_USABLE,
                format!("The HTTP driver cannot carry out this step as the plan wrote it: {why}."),
                "Compile the target again with the version of the HTTP pack that is enabled.",
            ))
        };
        let url = self.address(built).map_err(NotReceived::Refused)?;

        let mut headers = HeaderMap::new();
        for (name, value) in &built.headers {
            let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) else {
                return Err(refused("a header of the endpoint cannot be sent"));
            };
            headers.insert(name, value);
        }
        if let Some(held) = credentials {
            if let Some(cannot) = &held.cannot {
                return Err(NotReceived::Refused(cannot.clone()));
            }
            let sensitive = |secret: &str| {
                HeaderValue::from_str(secret).map(|mut value| {
                    value.set_sensitive(true);
                    value
                })
            };
            let cannot_carry = || refused("the session's credential cannot be carried in a header");
            for (name, secret) in &held.headers {
                let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    sensitive(secret.expose()),
                ) else {
                    return Err(cannot_carry());
                };
                headers.insert(name, value);
            }
            let mut cookies = String::new();
            for (origin, name, secret) in &held.cookies {
                if *origin != self.origin.text {
                    continue;
                }
                if !cookies.is_empty() {
                    cookies.push_str("; ");
                }
                cookies.push_str(name);
                cookies.push('=');
                cookies.push_str(secret.expose());
            }
            if !cookies.is_empty() {
                let value = sensitive(&cookies);
                // The joined text is a copy of secrets: wipe it now.
                drop(Secret::new(cookies));
                headers.insert(reqwest::header::COOKIE, value.map_err(|_| cannot_carry())?);
            }
        }

        let method = match built.method {
            Method::Get => reqwest::Method::GET,
            Method::Head => reqwest::Method::HEAD,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Patch => reqwest::Method::PATCH,
            Method::Delete => reqwest::Method::DELETE,
            Method::Options => reqwest::Method::OPTIONS,
        };
        let mut request = self.client.request(method, url).headers(headers);
        if let Some(body) = &built.body {
            request = request.body(body.clone());
        }
        let request = request
            .build()
            .map_err(|_| refused("the request could not be built"))?;

        let sent = tokio::time::timeout_at(until, self.client.execute(request)).await;
        let mut response = match sent {
            Err(_) => return Err(NotReceived::Lost(Lost::TimedOut)),
            Ok(Err(error)) if error.is_connect() => {
                return Err(NotReceived::Lost(Lost::BeforeSending));
            }
            Ok(Err(_)) => return Err(NotReceived::Lost(Lost::AfterSending)),
            Ok(Ok(response)) => response,
        };
        let status = response.status().as_u16();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_ascii_lowercase(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let mut body = Vec::new();
        loop {
            match tokio::time::timeout_at(until, response.chunk()).await {
                Err(_) => return Err(NotReceived::Lost(Lost::TimedOut)),
                Ok(Err(_)) => return Err(NotReceived::Lost(Lost::AfterSending)),
                Ok(Ok(None)) => break,
                Ok(Ok(Some(chunk))) => {
                    let size = u64::try_from(body.len() + chunk.len()).unwrap_or(u64::MAX);
                    if size > most {
                        return Err(NotReceived::TooLarge);
                    }
                    body.extend_from_slice(&chunk);
                }
            }
        }
        Ok(Received {
            status,
            headers,
            body,
        })
    }

    /// What a redirect or a refusing status means, when it means anything.
    fn stopped_by(
        &self,
        template: Option<&EndpointTemplate>,
        built: &Built,
        received: &Received,
    ) -> Option<Stop> {
        let signed_in = self.held.is_some();
        let location = received
            .headers
            .iter()
            .find(|(name, _)| name == "location")
            .map(|(_, value)| value.as_str());
        if let (300..=399, Some(location)) = (received.status, location) {
            // A redirect is never followed. Where it leads is read as a
            // browser would read it, against the address of the request:
            // with a scheme, with none (`//host/path`), or as a path. One
            // that leaves the surface is a sign-in request when the target
            // says so, and otherwise a destination the plan does not list.
            // One that stays on the surface is a status like any other.
            let elsewhere = self
                .address(built)
                .ok()
                .and_then(|request| request.join(location).ok())
                .filter(|url| url.origin().ascii_serialization() != self.origin.text);
            if let Some(url) = elsewhere {
                let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
                if signed_in && self.expiry.redirect_hosts.contains(&host) {
                    return Some(Stop::Expired(needs_login(
                        "the system sent the request on to its sign-in host",
                    )));
                }
                return Some(Stop::LeftSurface);
            }
        }
        if !signed_in {
            return None;
        }
        let declared = self.expiry.declared_statuses.iter().any(|(status, start)| {
            *status == received.status && built.path.starts_with(start.as_str())
        });
        let by_provider = self.expiry.provider_statuses.contains(&received.status)
            && !template.is_some_and(|template| template.answers.declares(received.status));
        (declared || by_provider).then(|| {
            Stop::Expired(needs_login(&format!(
                "the system refused its credential with status {}",
                received.status
            )))
        })
    }
}

impl HttpDriver {
    /// The exchange as evidence: what was done, in a form two runs that did
    /// the same thing share, and what was measured. Header values pass the
    /// allowlist and the redactor; a credential's header is recorded as
    /// present and masked.
    fn captures(
        &self,
        endpoint: &str,
        template_path: &str,
        built: &Built,
        credentials: Option<&Held>,
        received: &Received,
    ) -> Vec<Capture> {
        let acting = Acting {
            means: vec![
                Detail::new("endpoint", endpoint),
                Detail::new("method", built.method.word()),
                Detail::new("path", template_path),
                Detail::new("status", received.status.to_string()),
            ],
            measured: vec![Detail::new(
                "response_bytes",
                received.body.len().to_string(),
            )],
        };
        let redact = self.redact.as_ref();
        let mut sent: Vec<(&str, &str)> = built
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        // A credential's header is listed by name with no value at all.
        let names: Vec<String> = credentials
            .map(|held| {
                let mut names: Vec<String> = held
                    .headers
                    .iter()
                    .map(|(name, _)| name.to_ascii_lowercase())
                    .collect();
                if !held.cookies.is_empty() {
                    names.push("cookie".to_owned());
                }
                names
            })
            .unwrap_or_default();
        sent.retain(|(name, _)| !names.iter().any(|credential| credential == name));
        let mut request_headers = record_headers(sent, redact);
        request_headers.extend(names.iter().map(|name| a2m_telemetry::RecordedHeader {
            name: name.clone(),
            value: None,
        }));
        let exchange = serde_json::json!({
            "request": {
                "method": built.method.word(),
                "path": template_path,
                "headers": request_headers,
            },
            "response": {
                "status": received.status,
                "headers": record_headers(
                    received.headers.iter().map(|(name, value)| (name.as_str(), value.as_str())),
                    redact,
                ),
            },
        });
        let json = |name: &str, bytes: Vec<u8>| Capture {
            name: name.to_owned(),
            media_type: "application/json".to_owned(),
            bytes,
        };
        vec![
            json("acting", serde_json::to_vec(&acting).unwrap_or_default()),
            json(
                "exchange",
                serde_json::to_vec(&exchange).unwrap_or_default(),
            ),
        ]
    }

    async fn request(
        &self,
        session: &mut Session,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        session.answer = None;
        let (name, template) = endpoint(session, step)?;
        let (name, template) = (name.to_owned(), template.clone());
        if template.surface != session.surface {
            return Err(StepFailure::not_sent(codes::surface_not_declared(
                &format!("The surface of the endpoint `{name}`"),
            )));
        }
        // An endpoint that writes is sent only by a step the plan marks as
        // a commit point, which the handle lets through only with the
        // ledger's intent token.
        if template.writes && !step.commits {
            return Err(codes::not_usable(format!(
                "the endpoint `{name}` writes, and the step is not marked as a commit point"
            )));
        }
        let payload: RequestStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;

        // The version precondition. A write whose endpoint sends one goes
        // out with the version this pass read for the record, through the
        // endpoint the step names; or, for a record this pass looked up and
        // found absent, with none, because the write creates it. It never
        // goes out with none because nothing is known. The version is kept
        // until a request may have reached the system: an attempt that was
        // not sent leaves it for the next.
        let mut version = None;
        let mut taken_from = None;
        if template.version.send.is_some() {
            let reader = payload.version_from.as_deref().ok_or_else(|| {
                codes::not_usable(format!(
                    "the endpoint `{name}` sends the record's version, and the step names no endpoint that reads it"
                ))
            })?;
            taken_from = version_slot(reader, input);
            match taken_from
                .as_ref()
                .and_then(|slot| session.versions.get(slot))
            {
                Some(Known::Version(read)) => version = Some(read.clone()),
                Some(Known::Absent) => {}
                None => return Err(StepFailure::not_sent(codes::version_not_read(&name))),
            }
        }
        let built = build(
            &template,
            &payload,
            input.args,
            Extras {
                stamp: input.stamp,
                version: version.as_ref(),
            },
        )
        .map_err(StepFailure::not_sent)?;

        let credentials = match template.auth {
            Auth::None => None,
            Auth::Session | Auth::Token => Some(session.held.as_ref().ok_or_else(|| {
                StepFailure::not_sent(needs_login(
                    "the endpoint takes the session's credential, and the run has no session",
                ))
            })?),
        };
        let most = template
            .max_response_bytes
            .unwrap_or(self.max_response_bytes);
        // What this endpoint read of the record before is no longer what
        // the pass knows: the steps that follow say what it knows now.
        let own = version_slot(&name, input).filter(|_| !template.writes);
        if let Some(own) = &own {
            session.versions.remove(own);
        }
        let outcome = session
            .send(&built, credentials, most, until(step, input))
            .await;
        let may_have_arrived = !matches!(
            outcome,
            Err(NotReceived::Lost(Lost::BeforeSending) | NotReceived::Refused(_))
        );
        if may_have_arrived && let Some(slot) = &taken_from {
            // The version went out with a request: it is used up.
            session.versions.remove(slot);
        }
        let received = outcome.map_err(|not| match not {
            NotReceived::Lost(how) => lost(&how, step.commits, &name),
            NotReceived::Refused(failure) => StepFailure::not_sent(failure),
            // A write was sent and its answer was not read: in doubt.
            NotReceived::TooLarge if step.commits => in_doubt(
                &name,
                &format!("its answer was larger than {most} bytes and was not read"),
            ),
            NotReceived::TooLarge => StepFailure::answered(failure_of(
                FailureClass::UnknownVariant,
                codes::RESPONSE_TOO_LARGE,
                format!("The endpoint `{name}` answered with more than {most} bytes."),
                "Raise `max_response_bytes` on the endpoint if the answer is meant to be that large.",
            )),
        })?;

        match session.stopped_by(Some(&template), &built, &received) {
            // The session ended. For a write this is `session_expired` and
            // not a settled class: the ledger keeps the record in doubt,
            // and the keeper is told, so the run can sign in and reconcile.
            Some(Stop::Expired(failure)) => return Err(StepFailure::answered(failure)),
            // A write the system answered by sending the caller elsewhere
            // may have been made. The redirect is not the application's
            // verdict, and it is not followed: the record is in doubt.
            Some(Stop::LeftSurface) if step.commits => {
                return Err(in_doubt(
                    &name,
                    "the system answered with a redirect to an address outside the surface, which was not followed",
                ));
            }
            Some(Stop::LeftSurface) => {
                return Err(StepFailure::answered(codes::surface_not_declared(
                    "The address the system redirected the request to",
                )));
            }
            None => {}
        }
        let captures = self.captures(&name, &template.path_text(), &built, credentials, &received);
        let absent = step.lookup && template.answers.absent.contains(&received.status);
        if absent && let Some(own) = own {
            session.versions.insert(own, Known::Absent);
        }
        session.answer = Some(Answer {
            endpoint: name,
            status: received.status,
            headers: received.headers,
            body: received.body,
            json: None,
            absent,
            sent_version: version.as_ref().and_then(Json::scalar_text),
            wrote: step.commits,
        });
        let observed = Observed {
            captures,
            ..Observed::default()
        };
        Ok(if absent {
            StepOutcome::Absent(observed)
        } else {
            StepOutcome::Done(observed)
        })
    }
}

/// A step that judges a held answer and cannot be carried out as written.
/// When the answer held is a write's, the write was sent and its verdict
/// stays unread: the record is in doubt, never "not written".
fn cannot_judge(held: Option<&Answer>, why: String) -> StepFailure {
    match held {
        Some(answer) if answer.wrote => in_doubt(
            &answer.endpoint,
            &format!("the step that was to judge its answer cannot be carried out: {why}"),
        ),
        _ => codes::not_usable(why),
    }
}

/// The answer a `validate` or `extract` step works on: the one its own
/// endpoint's `request` step received.
fn held_answer<'a>(
    answer: &'a mut Option<Answer>,
    name: &str,
) -> Result<&'a mut Answer, StepFailure> {
    if !answer.as_ref().is_some_and(|held| held.endpoint == name) {
        return Err(cannot_judge(
            answer.as_ref(),
            format!("no answer of the endpoint `{name}` is held; a `request` step comes first"),
        ));
    }
    answer.as_mut().ok_or_else(|| codes::not_usable("no answer is held"))
}

fn validate(session: &mut Session, step: &Step) -> Result<StepOutcome, StepFailure> {
    let (name, template) = endpoint(session, step)?;
    let (name, template) = (name.to_owned(), template.clone());
    let answer = held_answer(&mut session.answer, &name)?;
    let answered = StepFailure::answered;
    if answer.absent && step.lookup {
        return Ok(StepOutcome::Absent(Observed::default()));
    }
    let status = answer.status;
    if template.answers.ok.contains(&status) && !answer.absent {
        let shape = &template.response;
        let drift = |why: String| {
            answered(failure_of(
                FailureClass::Drift,
                codes::RESPONSE_SCHEMA_MISMATCH,
                format!(
                    "The answer of the endpoint `{name}` does not fit the shape the endpoint declares: {why}."
                ),
                "Compare the system's answer with the endpoint's `response_schema`, and correct the one that is wrong.",
            ))
        };
        // A body that is not JSON is never read as nothing, whatever the
        // shape: a page answered in place of the data stops here.
        let json = if answer.body.is_empty() {
            Json::Null
        } else {
            Json::parse(&answer.body).map_err(|error| {
                drift(format!(
                    "the body is not JSON at byte {}, where {}",
                    error.at, error.what
                ))
            })?
        };
        shape.check(&json).map_err(drift)?;
        answer.json = Some(json);
        return Ok(StepOutcome::Done(Observed::default()));
    }

    // Not accepted. Which of the answers the endpoint declares is it?
    let body = Json::parse(&answer.body).ok();
    let is = |answers: &[crate::template::Answer]| {
        answers
            .iter()
            .find(|declared| declared.matches(status, body.as_ref()))
            .cloned()
    };
    if is(&template.answers.unavailable).is_some() {
        let wait = answer
            .header("retry-after")
            .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
            .map(|seconds| format!(" It asks to wait {seconds} seconds."))
            .unwrap_or_default();
        return Err(answered(failure_of(
            FailureClass::Transient,
            codes::UPSTREAM_UNAVAILABLE,
            format!(
                "The endpoint `{name}` answered that the system is not available now, with status {status}.{wait}"
            ),
            "Run the tool again later.",
        )));
    }
    if let Some(conflict) = is(&template.answers.conflict) {
        let plain = |text: String| {
            let fine = !text.is_empty()
                && text.len() <= 64
                && text
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
            fine.then_some(text)
        };
        let sent = answer.sent_version.clone().and_then(plain);
        let now = conflict
            .current
            .as_ref()
            .and_then(|field| body.as_ref()?.get(field)?.scalar_text())
            .and_then(plain);
        let versions = match (sent, now) {
            (Some(sent), Some(now)) => format!(" Version {sent} was sent; the record is at {now}."),
            (Some(sent), None) => format!(" Version {sent} was sent."),
            (None, Some(now)) => format!(" No version was sent; the record is at {now}."),
            (None, None) => String::new(),
        };
        return Err(answered(failure_of(
            FailureClass::Stale,
            codes::VERSION_CONFLICT,
            format!(
                "The record was changed since it was read, and the endpoint `{name}` refused the write.{versions}"
            ),
            "Preview the change again, so that it is made against the record as it is now.",
        )));
    }
    if is(&template.answers.refused).is_some() {
        return Err(answered(failure_of(
            FailureClass::Rejected,
            codes::APP_VALIDATION_ERROR,
            format!(
                "The system refused the request to the endpoint `{name}` with status {status}, in the shape the endpoint declares for a refusal."
            ),
            "Correct the input the system refused, then run the tool again.",
        )));
    }
    Err(answered(failure_of(
        FailureClass::UnknownVariant,
        codes::UNEXPECTED_STATUS,
        format!(
            "The endpoint `{name}` answered with status {status}, in a form the endpoint does not declare."
        ),
        "Find out what the answer means, and declare it under the endpoint's `responses`.",
    )))
}

/// One value at the end of a path, as the type the plan asks for. `None`
/// when the value is there and is not of that type.
fn taken(json: &Json, path: &[String], take: Take) -> Option<Value> {
    let Some((first, rest)) = path.split_first() else {
        return match (take, json) {
            (_, Json::Null) => Some(Value::Null),
            (Take::Text, Json::Text(text)) => Some(Value::Text(text.clone())),
            (Take::Bool, Json::Bool(flag)) => Some(Value::Bool(*flag)),
            (Take::Int, number @ Json::Number(text)) if number.is_whole() => {
                text.parse().ok().map(Value::Int)
            }
            (Take::Decimal, Json::Number(text)) if !text.contains(['e', 'E']) => {
                a2m_core::Decimal::parse(text).ok().map(Value::Decimal)
            }
            (Take::Decimal, Json::Text(text)) => {
                a2m_core::Decimal::parse(text).ok().map(Value::Decimal)
            }
            _ => None,
        };
    };
    if first == "*" {
        return match json {
            Json::List(items) => items
                .iter()
                .map(|item| taken(item, rest, take))
                .collect::<Option<Vec<_>>>()
                .map(Value::List),
            Json::Null => Some(Value::Null),
            _ => None,
        };
    }
    match json {
        Json::Object(_) => match json.get(first) {
            Some(inner) => taken(inner, rest, take),
            None => Some(Value::Null),
        },
        Json::Null => Some(Value::Null),
        _ => None,
    }
}

impl HttpDriver {
    fn extract(
        &self,
        session: &mut Session,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, template) = endpoint(session, step)?;
        let (name, template) = (name.to_owned(), template.clone());
        let answer = held_answer(&mut session.answer, &name)?;
        if answer.absent && step.lookup {
            return Ok(StepOutcome::Absent(Observed::default()));
        }
        let Some(json) = &answer.json else {
            return Err(cannot_judge(
                Some(answer),
                format!(
                    "the answer of the endpoint `{name}` was not checked; a `validate` step comes first"
                ),
            ));
        };
        let payload: ExtractStep = step.payload.decode().map_err(|_| {
            cannot_judge(Some(answer), "its payload does not decode".to_owned())
        })?;
        let mut values = Values::new();
        for wanted in &payload.values {
            let value = taken(json, &wanted.path, wanted.take).ok_or_else(|| {
                StepFailure::answered(failure_of(
                    FailureClass::Drift,
                    codes::RESPONSE_SCHEMA_MISMATCH,
                    format!(
                        "The answer of the endpoint `{name}` holds a value at `{}` that is not of the type the operation's output `{}` declares.",
                        wanted.path.join("."),
                        wanted.name
                    ),
                    "Compare the system's answer with the endpoint's `response_schema` and the operation's outputs.",
                ))
            })?;
            values.insert(wanted.name.clone(), value);
        }
        // What the pass now knows of the record's version, read through
        // this endpoint: the version, or nothing when the answer holds none.
        if let (Some(source), Some(slot)) = (&template.version.read, version_slot(&name, input)) {
            let version = match source {
                VersionSource::Field(field) => json.get(field).cloned(),
                VersionSource::Header(header) => answer
                    .header(header)
                    .map(|text| Json::Text(text.to_owned())),
            };
            match version.filter(|version| *version != Json::Null) {
                Some(version) => session.versions.insert(slot, Known::Version(version)),
                None => session.versions.remove(&slot),
            };
        }
        Ok(StepOutcome::Done(Observed {
            values: self.redact.redact_values(&values),
            ..Observed::default()
        }))
    }

    /// One safe request with no body and no credential, to a literal path
    /// of the surface: what a discoverer may send.
    async fn probe(
        &self,
        session: &mut Session,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let probe: ProbeStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let literal = parse_path(&probe.path)
            .ok()
            .filter(|(segments, _)| {
                segments
                    .iter()
                    .all(|segment| matches!(segment, crate::template::Segment::Literal(_)))
            })
            .is_some();
        if !probe.method.is_safe() || step.commits || !literal {
            return Err(codes::not_usable(
                "a probe is a GET, HEAD, or OPTIONS request to a literal path, and never a commit point",
            ));
        }
        let built = Built {
            method: probe.method,
            path: probe.path.clone(),
            query: String::new(),
            headers: vec![(
                "accept".to_owned(),
                "application/json, application/yaml;q=0.8, */*;q=0.1".to_owned(),
            )],
            body: None,
        };
        let most = self.max_response_bytes;
        let received = session
            .send(&built, None, most, until(step, input))
            .await
            .map_err(|not| match not {
                NotReceived::Lost(how) => lost(&how, false, &probe.path),
                NotReceived::Refused(failure) => StepFailure::not_sent(failure),
                NotReceived::TooLarge => StepFailure::answered(failure_of(
                    FailureClass::UnknownVariant,
                    codes::RESPONSE_TOO_LARGE,
                    format!("The system answered a probe with more than {most} bytes."),
                    "Give the description as a file instead.",
                )),
            })?;
        let header = |name: &str| {
            received
                .headers
                .iter()
                .find(|(known, _)| known == name)
                .map(|(_, value)| value.as_str())
        };
        let mut values = Values::new().with("status", i64::from(received.status));
        // Only what describes the surface: never a cookie or a token.
        if let Some(kind) = header("content-type") {
            values.insert("content_type", kind);
        }
        if let Some(allow) = header("allow") {
            values.insert("allow", allow);
        }
        // Of a challenge, only the names of the schemes it offers.
        let schemes: Vec<Value> = received
            .headers
            .iter()
            .filter(|(name, _)| name == "www-authenticate")
            .filter_map(|(_, value)| value.split_whitespace().next())
            .map(|scheme| Value::from(scheme.to_ascii_lowercase()))
            .collect();
        if !schemes.is_empty() {
            values.insert("challenges", schemes);
        }
        if (200..300).contains(&received.status)
            && received.body.len() <= MOST_PROBE_TEXT
            && let Ok(text) = String::from_utf8(received.body)
        {
            values.insert("body", text);
        }
        Ok(StepOutcome::Done(Observed {
            values: self.redact.redact_values(&values),
            ..Observed::default()
        }))
    }
}

impl Driver for HttpDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move { self.prepared(cx, lease).map(Prepared::new) })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let Some(session) = prepared.state::<Session>() else {
                return Err(codes::not_usable("it was not prepared by the HTTP driver"));
            };
            if step.kind == REQUEST {
                self.request(session, step, input).await
            } else if step.kind == VALIDATE {
                validate(session, step)
            } else if step.kind == EXTRACT {
                self.extract(session, step, input)
            } else if step.kind == PROBE {
                self.probe(session, step, input).await
            } else {
                Err(codes::not_usable(format!(
                    "the `http` mode has no step of the kind `{}`",
                    step.kind
                )))
            }
        })
    }

    fn health(&self, surface: &SurfaceRef) -> DriverHealth {
        match Origin::of(&surface.surface.address) {
            Ok(_) => DriverHealth::Ready,
            Err(failure) => DriverHealth::Unavailable(failure.code().clone()),
        }
    }

    fn release<'a>(&'a self, prepared: Prepared, poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            // Dropping the state wipes the credentials held for the pass. A
            // poisoned client is not kept warm for the next one.
            if let Ok(session) = prepared.into_state::<Session>()
                && poisoned
            {
                self.forget_client(&session.client_key);
            }
            Cleanup::complete()
        })
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::{Decimal, Value};

    use std::collections::BTreeMap;

    use super::{Answer, Expiry, Origin, Session, taken};
    use crate::codes;
    use crate::json::Json;
    use crate::template::{Built, Method, Take};

    #[test]
    fn an_address_is_taken_apart_once_and_https_is_refused_by_name() {
        let origin = Origin::of("http://127.0.0.1:4010").unwrap();
        assert_eq!(
            (origin.text.as_str(), origin.prefix.as_str()),
            ("http://127.0.0.1:4010", "")
        );
        let origin = Origin::of("http://app.example.test/v2/").unwrap();
        assert_eq!(
            (origin.text.as_str(), origin.prefix.as_str()),
            ("http://app.example.test", "/v2")
        );
        assert_eq!(
            Origin::of("https://app.example.test").unwrap_err().code(),
            &codes::TLS_NOT_ENABLED
        );
        for bad in [
            "app.example.test",
            "ftp://app.example.test",
            "http://user:PLANTED@app.example.test",
            "http://app.example.test/?x=1",
            "http://app.example.test/a%20b",
        ] {
            let failure = Origin::of(bad).unwrap_err();
            assert_eq!(failure.code(), &codes::STEP_NOT_USABLE, "{bad}");
            assert!(!failure.message().contains("PLANTED"), "{failure}");
        }
    }

    fn session(address: &str) -> Session {
        let origin = Origin::of(address).unwrap();
        Session {
            client: reqwest::Client::new(),
            client_key: format!("- {}", origin.text),
            origin,
            surface: "api".to_owned(),
            sections: BTreeMap::new(),
            held: None,
            expiry: Expiry::default(),
            answer: None,
            versions: BTreeMap::new(),
        }
    }

    fn built(path: &str, query: &str) -> Built {
        Built {
            method: Method::Get,
            path: path.to_owned(),
            query: query.to_owned(),
            headers: Vec::new(),
            body: None,
        }
    }

    #[test]
    fn the_address_sent_to_is_the_surfaces_origin_and_exactly_the_path_that_was_built() {
        let session = session("http://app.example.test/v2");
        let url = session
            .address(&built("/orders/A%2F1/items", "q=a%26b"))
            .unwrap();
        assert_eq!(
            url.as_str(),
            "http://app.example.test/v2/orders/A%2F1/items?q=a%26b"
        );
        // What the compiler and the builder refuse is refused again here,
        // where a plan that held it all the same would be sent: a dot
        // segment that leaves the surface's path, plain or encoded; a
        // separator that starts a query, a fragment, or another host; a
        // byte that would be encoded again.
        for path in [
            "/../admin",
            "/orders/../../admin",
            "/orders/%2e%2e/%2E%2E/admin",
            "/orders/./x",
            "/orders?x=1",
            "/orders#x",
            "/orders\\x",
            "/orders x",
            "/orders/\u{e9}",
            "@other.example.test/x",
            ":8080/x",
            ".other.example.test/x",
        ] {
            let failure = session.address(&built(path, "")).unwrap_err();
            assert_eq!(failure.code(), &codes::SURFACE_NOT_DECLARED, "{path}");
            assert_eq!(failure.class(), a2m_core::FailureClass::Policy, "{path}");
        }
        for query in ["x=1#y", "x=a b", "x=\u{e9}"] {
            let failure = session.address(&built("/orders", query)).unwrap_err();
            assert_eq!(failure.code(), &codes::SURFACE_NOT_DECLARED, "{query}");
        }
        // A second `//` after the origin stays a path on the surface's own
        // host, and is sent as written.
        let url = session.address(&built("//other.example.test/x", "")).unwrap();
        assert_eq!(url.host_str(), Some("app.example.test"));
        assert_eq!(url.path(), "/v2//other.example.test/x");
    }

    #[test]
    fn a_held_answer_prints_no_header_and_no_body() {
        let answer = Answer {
            endpoint: "order_get".to_owned(),
            status: 200,
            headers: vec![(
                "set-cookie".to_owned(),
                "session=secret-for-tests-cookie-0010".to_owned(),
            )],
            body: br#"{"token":"secret-for-tests-body-0011"}"#.to_vec(),
            json: Json::parse(br#"{"token":"secret-for-tests-body-0011"}"#).ok(),
            absent: false,
            sent_version: Some("7".to_owned()),
            wrote: false,
        };
        let printed = format!("{answer:?} {answer:#?}");
        assert!(printed.contains("order_get") && printed.contains("200"));
        assert!(!printed.contains("secret-for-tests"), "{printed}");
    }

    #[test]
    fn a_value_is_taken_exactly_or_not_at_all() {
        let json = Json::parse(
            br#"{"a":{"n":"150.50","i":7,"f":1.25},"list":[{"c":"x"},{"c":"y"}],"t":true}"#,
        )
        .unwrap();
        let path = |text: &str| -> Vec<String> { text.split('.').map(str::to_owned).collect() };
        let decimal = |text: &str| Some(Value::Decimal(Decimal::parse(text).unwrap()));
        assert_eq!(taken(&json, &path("a.n"), Take::Decimal), decimal("150.5"));
        assert_eq!(taken(&json, &path("a.f"), Take::Decimal), decimal("1.25"));
        assert_eq!(taken(&json, &path("a.i"), Take::Int), Some(Value::Int(7)));
        assert_eq!(taken(&json, &path("a.f"), Take::Int), None);
        assert_eq!(taken(&json, &path("a.i"), Take::Text), None);
        assert_eq!(
            taken(&json, &path("t"), Take::Bool),
            Some(Value::Bool(true))
        );
        assert_eq!(
            taken(&json, &path("a.missing"), Take::Text),
            Some(Value::Null)
        );
        assert_eq!(
            taken(&json, &path("list.*.c"), Take::Text),
            Some(Value::List(vec![Value::from("x"), Value::from("y")]))
        );
        assert_eq!(taken(&json, &path("t.x"), Take::Text), None);
    }
}
