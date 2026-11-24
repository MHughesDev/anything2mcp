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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_auth::{ChallengeScheme, CredentialSink, RawSign, SessionLease};
use a2m_core::EnvironmentKind;
use a2m_core::{Failure, Redact, Secret, Step, SurfaceIndex, Value, Values};
use a2m_provider::{
    BoxFuture, Capture, Cleanup, Driver, DriverHealth, Observed, Posture, PrepareContext, Prepared,
    StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use a2m_telemetry::{Acting, Detail, record_headers};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::codes;
use crate::endpoints::{ENDPOINT_KIND, PACK};
use crate::forgery;
use crate::forms::{AnswerSpec, Charset, Form, Unread, ascii_if_said, refused_control};
use crate::json::Json;
use crate::link;
use crate::lower::{EXTRACT, PROBE, REQUEST, TOKEN, VALIDATE};
use crate::shape::Shape;
use crate::template::{
    Auth, Built, EndpointSection, EndpointTemplate, ExtractStep, Extras, Method, PAYLOAD_FORMAT,
    ProbeStep, RequestStep, Take, TokenSink, VersionSource, build, build_token_page,
    is_override_field, is_override_header, override_may_change, parse_path,
};

/// The most warm clients the driver keeps.
const MOST_CLIENTS: usize = 64;
/// The most text of a probe's answer that is handed to a discoverer.
const MOST_PROBE_TEXT: usize = 4_000_000;

/// The address of a surface, taken apart once, when the driver prepares.
/// Every request of the pass goes here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Origin {
    /// `http://host` or `https://host:port`, as the URL type writes it.
    pub text: String,
    /// The path the surface's address ends in, without a trailing slash.
    pub prefix: String,
    /// Whether the address is plain `http` to an address that is not a
    /// loopback address: no credential goes there unless the surface
    /// accepts it.
    pub plain_remote: bool,
}

impl Origin {
    /// Takes a surface's address apart.
    pub(crate) fn of(address: &str) -> Result<Self, Failure> {
        let unusable = |why: &str| {
            codes::failure(
                &codes::row::STEP_NOT_USABLE,
                format!("The surface's address cannot be used by the HTTP pack: {why}."),
                "Write the surface's `base` as `http://host` or `https://host`, with a port and a path if it has them.",
            )
        };
        let url = reqwest::Url::parse(address).map_err(|_| unusable("it is not an address"))?;
        match url.scheme() {
            "http" => {}
            #[cfg(feature = "tls")]
            "https" => {}
            #[cfg(not(feature = "tls"))]
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
            plain_remote: link::plain_to_another_machine(&url),
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
    /// The name of the first header the session placed whose name begins
    /// `a2m-`: one of the product's own carriers between its parts, never
    /// kept and never sent (decision 0270).
    product_header: Option<String>,
}

/// Whether a header's name is one of the product's own: it begins `a2m-`,
/// in any letter case.
pub(crate) fn is_product_header(name: &str) -> bool {
    name.get(..4)
        .is_some_and(|start| start.eq_ignore_ascii_case("a2m-"))
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
        if is_product_header(name) {
            self.product_header.get_or_insert_with(|| name.to_owned());
            return;
        }
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
        #[cfg(not(feature = "tls"))]
        {
            self.cannot = Some(codes::tls_not_enabled());
        }
        #[cfg(feature = "tls")]
        {
            self.cannot = Some(codes::failure(
                &codes::row::AUTH_SCHEME_NOT_BUILT,
                "The session signs in with a client certificate, which this version of the HTTP pack does not present.",
                "Use a provider that places a header or a cookie, such as `api_key`, `bearer_static`, or `http_basic`.",
            ));
        }
    }

    fn challenge_credential(&mut self, scheme: ChallengeScheme, _user: &str, _secret: &Secret) {
        self.cannot = Some(codes::failure(
            &codes::row::AUTH_SCHEME_NOT_BUILT,
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
    /// The anti-forgery token the request carried, when the answer's body
    /// holds it in any letter case. What the answer says is then never
    /// quoted, and no value taken from it may hold the token (decision
    /// 0399).
    echoed: Option<Secret>,
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

    /// Whether the answer says it is a page (`text/html`), not data.
    fn is_a_page(&self) -> bool {
        self.headers_named("content-type").iter().any(|value| {
            let kind = value.split(';').next().unwrap_or("").trim();
            kind.eq_ignore_ascii_case("text/html")
                || kind.eq_ignore_ascii_case("application/xhtml+xml")
        })
    }

    /// Every value of a header the answer carries, in order.
    fn headers_named(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(known, _)| known == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }
}

/// What `prepare` acquires: the client, the surface's address, the
/// endpoints of the plan, and the pass's own state.
struct Session {
    client: reqwest::Client,
    client_key: String,
    origin: Origin,
    surface: String,
    /// Whether the surface accepts that its credentials cross an
    /// unencrypted link to another machine.
    unencrypted_accepted: bool,
    sections: BTreeMap<usize, EndpointSection>,
    held: Option<Held>,
    expiry: Expiry,
    answer: Option<Answer>,
    /// What this pass knows of a record's version: by the endpoint that
    /// read the record and the record's key.
    versions: BTreeMap<(String, Vec<u8>), Known>,
    /// The anti-forgery token each write's page gave this pass, by the
    /// endpoint and the record's key. A token is sent once and then gone.
    tokens: BTreeMap<(String, Vec<u8>), Secret>,
    /// What the run may do to the system: the driver's own read-only guard
    /// (decision 0249).
    posture: Posture,
    /// Whether a read an endpoint asserts may be sent outside a live run:
    /// the plan is a published version's, and the run's environment is of
    /// kind `test` in it (decision 0270). Under the drafts both facts are
    /// the drafts' own word, and it is never sent outside a live run.
    asserted_reads: bool,
    /// Where this pass's reads are counted while they run past their
    /// deadline: the surface's name at its origin.
    reading_place: String,
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
    /// The reads of answers still running past their step's deadline.
    overruns: Arc<Overruns>,
}

fn failure_of(row: &a2m_core::CodeEntry, message: impl Into<String>, next: &str) -> Failure {
    row.failure(message, next)
}

fn needs_login(why: &str) -> Failure {
    failure_of(
        &a2m_auth::codes::row::NEEDS_LOGIN,
        format!("The session cannot be used: {why}."),
        "Sign in again, then resume the run, which reconciles any write it began before it writes again.",
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
            overruns: Arc::new(Overruns::default()),
        }
    }

    /// The warm client of a session and an origin, or a new one. A client
    /// for an `https` origin sends nothing that is not `https`.
    fn client(&self, key: &str, https: bool) -> Result<reqwest::Client, Failure> {
        let mut clients = self
            .clients
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(client) = clients.get(key) {
            return Ok(client.clone());
        }
        // No redirect is followed, and no proxy is used: either would send
        // the request somewhere the plan does not name. The client trusts
        // what the engine trusts, and states its own TLS provider.
        let trust = a2m_auth::tls::installed()?;
        let client = a2m_auth::tls::client_builder(&trust)
            .https_only(https)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .referer(false)
            .user_agent(concat!("anything2mcp/", env!("CARGO_PKG_VERSION")))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| {
                failure_of(
                    &codes::row::STEP_NOT_USABLE,
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
        let unencrypted_accepted = link::SurfaceFields::of(surface)
            .map_err(|_| codes::not_usable("the surface's fields do not decode"))?
            .accepts_unencrypted();
        if !cx.destinations.contains(&surface.address) {
            return Err(unsent(codes::surface_not_declared(&format!(
                "The address of the surface `{}`",
                surface.name
            ))));
        }
        if cx.deadline.passed() {
            return Err(unsent(failure_of(
                &a2m_provider::codes::row::TIMEOUT,
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

        // A credential never crosses an unencrypted link to another machine
        // unless the surface accepts it: a pass with a session whose plan
        // has an endpoint on this surface that takes the credential is
        // refused here, and each request is checked again as it is sent.
        let takes_credential = sections
            .values()
            .flat_map(BTreeMap::values)
            .any(|template| {
                template.surface == surface.name.as_str() && template.auth != Auth::None
            });
        if lease.is_some() && takes_credential && origin.plain_remote && !unencrypted_accepted {
            return Err(unsent(link::not_encrypted(surface.name.as_str())));
        }

        let mut expiry = Expiry::default();
        let mut held = None;
        let mut client_key = format!("- {}", origin.text);
        if let Some(lease) = lease {
            let mut placed = Held::default();
            lease.apply(&mut placed).map_err(unsent)?;
            if let Some(name) = &placed.product_header {
                return Err(unsent(codes::product_header_refused(name)));
            }
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
            client: self
                .client(&client_key, origin.text.starts_with("https://"))
                .map_err(unsent)?,
            client_key,
            surface: surface.name.as_str().to_owned(),
            unencrypted_accepted,
            sections,
            held,
            expiry,
            answer: None,
            versions: BTreeMap::new(),
            tokens: BTreeMap::new(),
            posture: cx.posture,
            asserted_reads: published(cx.plan)
                && cx
                    .plan
                    .environment_named(&cx.surface.environment)
                    .is_some_and(|environment| environment.kind == EnvironmentKind::Test),
            reading_place: format!("{} {}", surface.name.as_str(), origin.text),
            origin,
        })
    }
}

/// The endpoint a step names, or why it names none.
fn endpoint(session: &Session, step: &Step) -> Result<(String, EndpointTemplate), &'static str> {
    let named = step.refers_to.as_ref().ok_or("it names no endpoint")?;
    session
        .sections
        .get(&named.section.get())
        .and_then(|section| section.get_key_value(&named.entry))
        .map(|(name, template)| (name.clone(), template.clone()))
        .ok_or("the endpoint it names is not in the plan")
}

/// The endpoint a step that judges a held answer names.
fn judged_endpoint(
    session: &Session,
    step: &Step,
) -> Result<(String, EndpointTemplate), StepFailure> {
    endpoint(session, step).map_err(|why| cannot_judge(session.answer.as_ref(), why.to_owned()))
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
        &a2m_provider::codes::row::NO_CONFIRMATION,
        format!("The write to the endpoint `{endpoint}` was sent, and {why}."),
        "Reconcile the record: read it back before anything is written again.",
    ))
}

fn lost(how: &Lost, commits: bool, endpoint: &str) -> StepFailure {
    match (how, commits) {
        (Lost::BeforeSending, _) => StepFailure::not_sent(failure_of(
            &a2m_provider::codes::row::CONNECTION_LOST,
            format!("No connection could be made for the endpoint `{endpoint}`; nothing was sent."),
            "Check that the system is reachable, then run the tool again.",
        )),
        (Lost::AfterSending | Lost::TimedOut, true) => {
            in_doubt(endpoint, "no complete answer came back")
        }
        (Lost::AfterSending, false) => StepFailure::unknown(failure_of(
            &a2m_provider::codes::row::CONNECTION_LOST,
            format!(
                "The connection was lost before the endpoint `{endpoint}` had answered in full."
            ),
            "Run the tool again.",
        )),
        (Lost::TimedOut, false) => StepFailure::unknown(failure_of(
            &a2m_provider::codes::row::TIMEOUT,
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
    /// Checked again before sending: the path built starts at a `/`, and
    /// what the URL type reads back is the surface's origin and exactly
    /// the path and the query that were built. A dot segment that would be
    /// folded away, a separator that would start a query, a fragment, or a
    /// host, and a byte that would be encoded again all change what is
    /// read back, and are refused.
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
                built.path.starts_with('/')
                    && url.origin().ascii_serialization() == self.origin.text
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
        secret_header: Option<&str>,
    ) -> Result<Received, NotReceived> {
        let refused = |why: &str| {
            NotReceived::Refused(failure_of(
                &codes::row::STEP_NOT_USABLE,
                format!("The HTTP driver cannot carry out this step as the plan wrote it: {why}."),
                "Compile the target again with the version of the HTTP pack that is enabled.",
            ))
        };
        let url = self.address(built).map_err(NotReceived::Refused)?;

        let mut headers = HeaderMap::new();
        for (name, value) in &built.headers {
            let (Ok(name), Ok(mut value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) else {
                return Err(refused("a header of the endpoint cannot be sent"));
            };
            if secret_header.is_some_and(|secret| name.as_str() == secret) {
                value.set_sensitive(true);
            }
            headers.insert(name, value);
        }
        if let Some(held) = credentials {
            if let Some(cannot) = &held.cannot {
                return Err(NotReceived::Refused(cannot.clone()));
            }
            if self.origin.plain_remote && !self.unencrypted_accepted {
                return Err(NotReceived::Refused(link::not_encrypted(&self.surface)));
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
                // A certificate or a handshake that failed: named, and
                // nothing was sent.
                let what = format!("the surface `{}`", self.surface);
                if let Some(failure) = a2m_auth::tls::refusal(&error, &what) {
                    return Err(NotReceived::Refused(failure));
                }
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
        (endpoint, template_path): (&str, &str),
        built: &Built,
        credentials: Option<&Held>,
        unencrypted: bool,
        received: &Received,
        secret_header: Option<&str>,
    ) -> Vec<Capture> {
        let mut means = vec![
            Detail::new("endpoint", endpoint),
            Detail::new("method", built.method.word()),
            Detail::new("path", template_path),
            Detail::new("status", received.status.to_string()),
        ];
        // A credential that crossed an unencrypted link, as the surface
        // accepts, is said in the evidence of every request that carried it.
        if unencrypted && credentials.is_some() {
            means.push(Detail::new("link", "unencrypted_credentials_accepted"));
        }
        let acting = Acting {
            means,
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
        sent.retain(|(name, _)| {
            !names.iter().any(|credential| credential == name) && Some(*name) != secret_header
        });
        let mut request_headers = record_headers(sent, redact);
        request_headers.extend(
            names
                .iter()
                .map(String::as_str)
                .chain(secret_header)
                .map(|name| a2m_telemetry::RecordedHeader {
                    name: name.to_owned(),
                    value: None,
                }),
        );
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
        vec![
            Capture::acting(&acting),
            Capture {
                name: "exchange".to_owned(),
                media_type: "application/json".to_owned(),
                bytes: serde_json::to_vec(&exchange).unwrap_or_default(),
            },
        ]
    }

    /// Fetches the page which gives a write's anti-forgery token, and keeps
    /// the one text it holds where the endpoint says, for the `request`
    /// step that follows. It is a GET with no body, sent where the
    /// endpoint's own requests go, and it writes nothing. A page that holds
    /// no token, or two, stops the write before it is sent (decision 0357).
    async fn token(
        &self,
        session: &mut Session,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        session.answer = None;
        let (name, template) = endpoint(session, step).map_err(codes::not_usable)?;
        if template.surface != session.surface {
            return Err(StepFailure::not_sent(codes::surface_not_declared(
                &format!("The surface of the endpoint `{name}`"),
            )));
        }
        let Some(forgery) = template.anti_forgery.clone() else {
            return Err(codes::not_usable(format!(
                "the endpoint `{name}` declares no anti-forgery bootstrap"
            )));
        };
        if step.commits {
            return Err(codes::not_usable(
                "a token is fetched by a step that is not a commit point",
            ));
        }
        let built = build_token_page(&template, input.args).map_err(StepFailure::not_sent)?;
        self.overruns
            .check(&session.reading_place)
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
        let received = session
            .send(&built, credentials, most, until(step, input), None)
            .await
            .map_err(|not| match not {
                // Nothing of the write was sent, whatever became of the page.
                NotReceived::Lost(_) => StepFailure::not_sent(failure_of(
                    &a2m_provider::codes::row::CONNECTION_LOST,
                    format!(
                        "The page that gives the token of the endpoint `{name}` could not be fetched; the write was not sent."
                    ),
                    "Check that the system is reachable, then run the tool again.",
                )),
                NotReceived::Refused(failure) => StepFailure::not_sent(failure),
                NotReceived::TooLarge => StepFailure::not_sent(failure_of(
                    &codes::row::RESPONSE_TOO_LARGE,
                    format!(
                        "The page that gives the token of the endpoint `{name}` is larger than {most} bytes; the write was not sent."
                    ),
                    "Raise `max_response_bytes` on the endpoint if the page is meant to be that large.",
                )),
            })?;
        match session.stopped_by(Some(&template), &built, &received) {
            Some(Stop::Expired(failure)) => return Err(StepFailure::not_sent(failure)),
            Some(Stop::LeftSurface) => {
                return Err(StepFailure::not_sent(codes::surface_not_declared(
                    "The address the system redirected the page of the token to",
                )));
            }
            None => {}
        }
        let held = Answer {
            endpoint: name.clone(),
            status: received.status,
            headers: received.headers,
            body: received.body,
            json: None,
            absent: false,
            sent_version: None,
            wrote: false,
            echoed: None,
        };
        // Nothing of the write was sent, so none of these is in doubt.
        if let Some(edge) = edge_failure(&name, held.status, &held) {
            return Err(StepFailure::not_sent(edge));
        }
        let gone = |why: &str| {
            StepFailure::not_sent(failure_of(
                &codes::row::ANTI_FORGERY_NOT_FOUND,
                format!(
                    "The page that gives the token of the endpoint `{name}` {why}; the write was not sent."
                ),
                "Look at where the page holds its token now, and correct the endpoint's `anti_forgery`.",
            ))
        };
        if held.status != 200 {
            return Err(gone(&format!("answered with status {}", held.status)));
        }
        let cookies = held.headers_named("set-cookie");
        let token = match forgery::read(&forgery.from, &held.body, &cookies) {
            forgery::Found::Token(token) => token,
            forgery::Found::Missing => return Err(gone("holds no token where the endpoint says")),
            forgery::Found::Unreadable => {
                return Err(StepFailure::not_sent(failure_of(
                    &codes::row::ANTI_FORGERY_UNREADABLE,
                    format!(
                        "The page that gives the token of the endpoint `{name}` holds two different tokens where the endpoint says, or one that cannot be sent; the write was not sent."
                    ),
                    "Look at the page; a token is read from one place that holds one text.",
                )));
            }
        };
        session
            .tokens
            .insert(token_slot(&name, input), Secret::new(token));
        let captures = vec![Capture::acting(&Acting {
            means: vec![
                Detail::new("endpoint", name.as_str()),
                Detail::new("method", "GET"),
                Detail::new(
                    "path",
                    crate::template::path_text_of(&forgery.page, forgery.page_trailing_slash),
                ),
                Detail::new("status", held.status.to_string()),
            ],
            measured: vec![Detail::new("response_bytes", held.body.len().to_string())],
        })];
        Ok(StepOutcome::Done(Observed {
            captures,
            ..Observed::default()
        }))
    }

    async fn request(
        &self,
        session: &mut Session,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        session.answer = None;
        let (name, template) = endpoint(session, step).map_err(codes::not_usable)?;
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
        // The anti-forgery token the page gave for this write. A write that
        // sends one is never sent without it, and a token is used once: it is
        // kept until a request may have reached the system.
        let token_slot = template
            .anti_forgery
            .as_ref()
            .map(|_| token_slot(&name, input));
        let token = match &token_slot {
            Some(slot) => Some(
                session
                    .tokens
                    .get(slot)
                    .map(|held| held.expose().to_owned())
                    .ok_or_else(|| StepFailure::not_sent(codes::token_not_read(&name)))?,
            ),
            None => None,
        };
        let secret_header = match template.anti_forgery.as_ref().map(|forgery| &forgery.to) {
            Some(TokenSink::Header(header)) => Some(header.clone()),
            _ => None,
        };
        let built = build(
            &template,
            &payload,
            input.args,
            Extras {
                stamp: input.stamp,
                version: version.as_ref(),
                token: token.as_deref(),
            },
        )
        .map_err(StepFailure::not_sent)?;
        may_send(session, &name, &template, &built, step)?;
        // A read that could not be begun is refused now, before anything
        // is sent, and not once the answer is in (decision 0270).
        self.overruns
            .check(&session.reading_place)
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
            .send(
                &built,
                credentials,
                most,
                until(step, input),
                secret_header.as_deref(),
            )
            .await;
        let may_have_arrived = !matches!(
            outcome,
            Err(NotReceived::Lost(Lost::BeforeSending) | NotReceived::Refused(_))
        );
        if may_have_arrived && let Some(slot) = &taken_from {
            // The version went out with a request: it is used up.
            session.versions.remove(slot);
        }
        if may_have_arrived && let Some(slot) = &token_slot {
            // So is the token.
            session.tokens.remove(slot);
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
                &codes::row::RESPONSE_TOO_LARGE,
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
        let captures = self.captures(
            (&name, &template.path_text()),
            &built,
            credentials,
            session.origin.plain_remote,
            &received,
            secret_header.as_deref(),
        );
        let absent = step.lookup && template.answers.absent.contains(&received.status);
        if absent && let Some(own) = own {
            session.versions.insert(own, Known::Absent);
        }
        let echoed = token
            .filter(|sent| forgery::mentions(&received.body, sent))
            .map(Secret::new);
        session.answer = Some(Answer {
            endpoint: name,
            status: received.status,
            headers: received.headers,
            body: received.body,
            json: None,
            absent,
            sent_version: version.as_ref().and_then(Json::scalar_text),
            wrote: step.commits,
            echoed,
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

/// Whether a plan is a published version's, as far as a driver can tell:
/// a draft is always compiled as the draft version, and a published
/// version under any other (decision 0161). A version published as the
/// draft version's number is taken for a draft, which is the safe side.
fn published(plan: &a2m_core::Plan) -> bool {
    plan.header.version != a2m_files::DRAFT_VERSION
}

/// Percent-decoded text of a query's name or value, `+` read as a space,
/// as a server reads it. An escape that is not two hexadecimal digits is
/// kept as written.
fn query_text(written: &str) -> String {
    let bytes = written.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        let digit = |offset: usize| {
            bytes
                .get(at + offset)
                .and_then(|b| char::from(*b).to_digit(16))
        };
        match (byte, digit(1), digit(2)) {
            (b'%', Some(high), Some(low)) => {
                out.push(u8::try_from(high * 16 + low).unwrap_or(b'%'));
                at += 3;
            }
            (b'+', _, _) => {
                out.push(b' ');
                at += 1;
            }
            _ => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a built request may change the system by its method: its own
/// method is one HTTP does not mark safe, or an override in a header or in
/// the query names one that may change it (decision 0270). An override is
/// judged as the method it names, and never makes a request safer than
/// its own method: whether a system honours an override cannot be known.
/// A body's `_method` is not looked at: only a method HTTP does not mark
/// safe carries a body.
fn may_change_by_method(built: &Built) -> bool {
    !built.method.is_safe() || overrides_to_unsafe(built)
}

/// Whether an override in a built request, in a header or in the query,
/// names a method that may change the system.
fn overrides_to_unsafe(built: &Built) -> bool {
    let by_header = built
        .headers
        .iter()
        .any(|(name, value)| is_override_header(name) && override_may_change(value));
    let by_query = built.query.split('&').any(|entry| {
        let (name, value) = entry.split_once('=').unwrap_or((entry, ""));
        is_override_field(&query_text(name)) && override_may_change(&query_text(value))
    });
    by_header || by_query
}

/// The driver's own read-only guard (decisions 0249 and 0270). A request
/// that may change something, by its method, by a method override it
/// carries, or by its endpoint's `effect`, is sent in a live run; outside
/// one it is sent only as the commit point of a write to a declared test
/// record, in the context the engine prepared for that write, or, for an
/// endpoint whose `read_asserted` says that a method HTTP does not mark
/// safe only reads, when the plan is a published version's and the run's
/// environment is of kind `test` in it. An asserted read is never sent
/// outside a live run with an override that may change the system: the
/// assertion is about the endpoint's own method. A discovery sends none.
/// Refused, nothing is sent.
fn may_send(
    session: &Session,
    name: &str,
    template: &EndpointTemplate,
    built: &Built,
    step: &Step,
) -> Result<(), StepFailure> {
    let overridden = overrides_to_unsafe(built);
    let may_change = template.writes || may_change_by_method(built);
    if !may_change {
        return Ok(());
    }
    let allowed = match session.posture {
        Posture::Live => true,
        Posture::Sandbox { test_records } => {
            if step.commits {
                test_records
            } else {
                template.read_asserted && !overridden && session.asserted_reads
            }
        }
        Posture::Discovery => false,
    };
    if allowed {
        return Ok(());
    }
    let by = if overridden {
        format!(
            "{} with a method override that names a method HTTP does not mark safe",
            built.method.word()
        )
    } else {
        built.method.word().to_owned()
    };
    Err(StepFailure::not_sent(codes::failure(
        &a2m_provider::codes::row::READ_ONLY_POSTURE,
        format!(
            "The endpoint `{name}` is reached by {by}, which may change the system, and this run may not change it; nothing was sent."
        ),
        if template.read_asserted {
            "Run the operation live, or try a published version of it against an environment of kind `test`."
        } else {
            "Run the operation live, or against a declared test record."
        },
    )))
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
    answer
        .as_mut()
        .ok_or_else(|| codes::not_usable("no answer is held"))
}

/// What the system said when it refused a request: the text of the first of
/// the body's top-level `message`, `detail`, and `error` that is text.
/// Redacted here, as the failure's own words never carry it; the contract
/// bounds it again.
fn said_by_refusal(body: Option<&Json>, redact: &dyn Redact) -> Option<String> {
    let body = body?;
    ["message", "detail", "error"]
        .iter()
        .find_map(|field| match body.get(field)? {
            // Text with a control character this pack never returns is
            // left out whole (decision 0249).
            Json::Text(text) if !refused_control(text) => Some(redact.redact(text)),
            _ => None,
        })
}

/// What reading an answer gave, off the engine's async threads.
enum Read {
    /// An answer of a status the endpoint accepts: its record, checked
    /// against the declared shape, or why it is refused.
    Accepted(Result<Json, Unread>),
    /// An answer of any other status: its body, when it is JSON, to match
    /// the declared answers against.
    Other(Option<Json>),
}

/// Reads an answer's body: parses it in its declared form and checks its
/// record against the declared shape, for an accepted status; parses it as
/// JSON, for any other. All of it runs on a blocking thread, never on the
/// engine's async threads, and stops once `stop` is set.
fn read_body(
    spec: &AnswerSpec,
    shape: &Shape,
    accepted: bool,
    said: Option<Charset>,
    body: &[u8],
    redact: &dyn Redact,
    stop: &AtomicBool,
) -> Read {
    if !accepted {
        return Read::Other(Json::parse_until(body, Some(stop)).ok());
    }
    let record = if spec.form != Form::Json {
        spec.read(body, said, redact, stop)
    } else if body.is_empty() {
        Ok(Json::Null)
    } else {
        ascii_if_said(body, said).and_then(|()| {
            Json::parse_until(body, Some(stop)).map_err(|error| {
                Unread::Shape(format!(
                    "the body is not JSON at byte {}, where {}",
                    error.at, error.what
                ))
            })
        })
    };
    Read::Accepted(record.and_then(|json| shape.check(&json).map(|()| json).map_err(Unread::Shape)))
}

/// How work handed to a blocking thread ended.
enum OffThread<T> {
    Done(T),
    /// The deadline came first. The work is told to stop and is not waited
    /// for.
    OutOfTime,
    /// The thread ended without an answer.
    Lost,
    /// So many reads are still running past their deadline that this one
    /// was not begun.
    Busy(Failure),
}

/// The most reads left running past their step's deadline, on one surface.
const MOST_LATE_READS_PER_SURFACE: usize = 4;
/// The most reads left running past their step's deadline, in the pack.
const MOST_LATE_READS: usize = 16;

/// The reads of answers that are still running after their step stopped
/// waiting for them, counted for the pack and for each surface (decision
/// 0270). A read is counted from the moment its step stops waiting, at its
/// deadline or because the step itself was dropped, until its thread ends;
/// at either ceiling a new read is not begun. Each such read holds a
/// blocking thread of the engine and up to the ceiling of an answer in
/// memory, and a pattern's compile and match cannot be stopped midway.
pub(crate) struct Overruns {
    counts: Mutex<LateCounts>,
    most_per_surface: usize,
    most: usize,
}

#[derive(Default)]
struct LateCounts {
    all: usize,
    by_place: BTreeMap<String, usize>,
}

impl Default for Overruns {
    fn default() -> Self {
        Self::with_ceilings(MOST_LATE_READS_PER_SURFACE, MOST_LATE_READS)
    }
}

/// A read's place in the count: still awaited, left running, or ended.
const AWAITED: u8 = 0;
const LEFT: u8 = 1;
const ENDED: u8 = 2;

/// One read, as the count knows it. Its state changes only under the
/// count's lock.
struct Reading {
    overruns: Arc<Overruns>,
    place: String,
    state: std::sync::atomic::AtomicU8,
}

/// Held by the step that waits: when it stops waiting before the read has
/// ended, at its deadline or because the step was dropped, the read is
/// told to stop and is counted as left running.
struct Awaiting {
    reading: Arc<Reading>,
    stop: Arc<AtomicBool>,
}

impl Drop for Awaiting {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.reading.overruns.left(&self.reading);
    }
}

/// Held by the blocking thread: when the work ends, or is dropped unrun,
/// a read left running is no longer counted.
struct Ending(Arc<Reading>);

impl Drop for Ending {
    fn drop(&mut self) {
        self.0.overruns.ended(&self.0);
    }
}

impl Overruns {
    pub(crate) fn with_ceilings(most_per_surface: usize, most: usize) -> Self {
        Self {
            counts: Mutex::new(LateCounts::default()),
            most_per_surface,
            most,
        }
    }

    fn counts(&self) -> std::sync::MutexGuard<'_, LateCounts> {
        self.counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `busy`/`reads_still_running` when a new read on `place` may not be
    /// begun.
    pub(crate) fn check(&self, place: &str) -> Result<(), Failure> {
        let counts = self.counts();
        let here = counts.by_place.get(place).copied().unwrap_or(0);
        if here < self.most_per_surface && counts.all < self.most {
            return Ok(());
        }
        Err(codes::failure(
            &codes::row::READS_STILL_RUNNING,
            format!(
                "{} answers of this surface, and {} of the HTTP pack, are still being read after their steps ran out of time; no new one is begun, and nothing was sent.",
                here, counts.all
            ),
            "Run the tool again once those reads have ended, or ask the endpoint's owner why its answers take so long to read.",
        ))
    }

    fn left(&self, reading: &Reading) {
        let mut counts = self.counts();
        if reading
            .state
            .compare_exchange(AWAITED, LEFT, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            counts.all += 1;
            *counts.by_place.entry(reading.place.clone()).or_insert(0) += 1;
        }
    }

    fn ended(&self, reading: &Reading) {
        let mut counts = self.counts();
        if reading.state.swap(ENDED, Ordering::SeqCst) == LEFT {
            counts.all = counts.all.saturating_sub(1);
            if let Some(here) = counts.by_place.get_mut(&reading.place) {
                *here = here.saturating_sub(1);
                if *here == 0 {
                    counts.by_place.remove(&reading.place);
                }
            }
        }
    }

    /// How many reads are counted as left running.
    #[cfg(test)]
    pub(crate) fn running_late(&self) -> usize {
        self.counts().all
    }
}

/// Runs `work` on a blocking thread, and waits for it until `until`. At the
/// deadline the flag `work` is handed is set, and the step goes on without
/// waiting: an async thread of the engine is never held by an answer. A
/// read that is not waited for to its end is counted against `place` until
/// it ends, and none is begun past the ceilings (decision 0270).
async fn off_thread<T: Send + 'static>(
    overruns: &Arc<Overruns>,
    place: &str,
    until: tokio::time::Instant,
    work: impl FnOnce(&AtomicBool) -> T + Send + 'static,
) -> OffThread<T> {
    if let Err(failure) = overruns.check(place) {
        return OffThread::Busy(failure);
    }
    let reading = Arc::new(Reading {
        overruns: Arc::clone(overruns),
        place: place.to_owned(),
        state: std::sync::atomic::AtomicU8::new(AWAITED),
    });
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let awaiting = Awaiting {
        reading: Arc::clone(&reading),
        stop: Arc::clone(&stop),
    };
    let ending = Ending(reading);
    let task = tokio::task::spawn_blocking(move || {
        let _ending = ending;
        work(&flag)
    });
    let outcome = tokio::select! {
        biased;
        joined = task => joined.map_or(OffThread::Lost, OffThread::Done),
        () = tokio::time::sleep_until(until) => {
            stop.store(true, Ordering::Relaxed);
            OffThread::OutOfTime
        }
    };
    drop(awaiting);
    outcome
}

async fn validate(
    session: &mut Session,
    step: &Step,
    input: &StepInput<'_>,
    redact: &Arc<dyn Redact>,
    overruns: &Arc<Overruns>,
) -> Result<StepOutcome, StepFailure> {
    let (name, template) = judged_endpoint(session, step)?;
    let answer = held_answer(&mut session.answer, &name)?;
    let answered = StepFailure::answered;
    if answer.absent && step.lookup {
        return Ok(StepOutcome::Absent(Observed::default()));
    }
    let status = answer.status;
    let accepted = template.answers.ok.contains(&status) && !answer.absent;
    let drift = |why: String| {
        answered(failure_of(
            &codes::row::RESPONSE_SCHEMA_MISMATCH,
            format!(
                "The answer of the endpoint `{name}` does not fit the shape the endpoint declares: {why}."
            ),
            "Compare the system's answer with the endpoint's `response_schema`, and correct the one that is wrong.",
        ))
    };
    // The answer's content type is held to its declared form: a page
    // answered in place of the data stops here. JSON with no content type
    // declared is not checked, as before (decision 0217).
    let said = if accepted {
        template
            .answer
            .check_content_type(&answer.headers_named("content-type"))
            .map_err(drift)?
    } else {
        None
    };
    let body = std::mem::take(&mut answer.body);
    let (spec, shape, clean) = (
        template.answer.clone(),
        template.response.clone(),
        Arc::clone(redact),
    );
    let read = match off_thread(
        overruns,
        &session.reading_place,
        until(step, input),
        move |stop| {
            let read = read_body(&spec, &shape, accepted, said, &body, clean.as_ref(), stop);
            (body, read)
        },
    )
    .await
    {
        OffThread::Done((body, read)) => {
            answer.body = body;
            read
        }
        OffThread::OutOfTime if answer.wrote => {
            return Err(in_doubt(
                &name,
                "its answer was not read in the time the step allows",
            ));
        }
        OffThread::OutOfTime => {
            return Err(answered(failure_of(
                &a2m_provider::codes::row::TIMEOUT,
                format!(
                    "The answer of the endpoint `{name}` was not read in the time the step allows."
                ),
                "Run the tool again, or raise the step's timeout in the target's defaults.",
            )));
        }
        OffThread::Lost => {
            return Err(cannot_judge(
                Some(answer),
                "its answer could not be read".to_owned(),
            ));
        }
        // The count rose between the request and its reading: the answer
        // is in and unread. A write's is in doubt.
        OffThread::Busy(_) if answer.wrote => {
            return Err(in_doubt(
                &name,
                "its answer was not read: too many reads are still running past their deadline",
            ));
        }
        OffThread::Busy(failure) => return Err(answered(failure)),
    };
    let body = match read {
        Read::Accepted(record) => {
            // A body that is not JSON is never read as nothing, whatever the
            // shape: a page answered in place of the data stops here.
            let json = record.map_err(|unread| match unread {
                Unread::Shape(why) => drift(why),
                Unread::DocumentType => answered(failure_of(
                    &codes::row::DOCUMENT_TYPE_REFUSED,
                    format!(
                        "The answer of the endpoint `{name}` declares a document type, which is never read; no value was taken from it."
                    ),
                    "Ask the system's owner for the answer without a document type, or read it another way.",
                )),
                Unread::TooLarge(why) => answered(failure_of(
                    &codes::row::RESPONSE_TOO_LARGE,
                    format!(
                        "The answer of the endpoint `{name}` is past a bound of what is read: {why}."
                    ),
                    "Raise `max_depth` or `max_elements` under the endpoint's `answer` if the answer is meant to be that large; the other bounds are fixed.",
                )),
                Unread::HoldsSecret => answered(failure_of(
                    &codes::row::ANSWER_HOLDS_SECRET,
                    format!(
                        "The answer of the endpoint `{name}` holds a registered secret, so neither its length nor its digest is returned."
                    ),
                    "Ask the system's owner why the answer carries the credential, or read it another way.",
                )),
                // The flag is set only at the deadline, which was met here.
                Unread::Stopped => cannot_judge(
                    Some(answer),
                    "its reading was stopped".to_owned(),
                ),
            })?;
            answer.json = Some(json);
            return Ok(StepOutcome::Done(Observed::default()));
        }
        Read::Other(body) => body,
    };

    // Not accepted. Which of the answers the endpoint declares is it?
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
            &codes::row::UPSTREAM_UNAVAILABLE,
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
            &codes::row::VERSION_CONFLICT,
            format!(
                "The record was changed since it was read, and the endpoint `{name}` refused the write.{versions}"
            ),
            "Preview the change again, so that it is made against the record as it is now.",
        )));
    }
    if is(&template.answers.refused).is_some() {
        let refused = answered(failure_of(
            &codes::row::APP_VALIDATION_ERROR,
            format!(
                "The system refused the request to the endpoint `{name}` with status {status}, in the shape the endpoint declares for a refusal."
            ),
            "Correct the input the system refused, then run the tool again.",
        ));
        // A refusal that quotes the token the write carried, in any
        // letter case, is not quoted back (decision 0357).
        let said = if answer.echoed.is_some() {
            None
        } else {
            said_by_refusal(body.as_ref(), redact.as_ref())
        };
        return Err(match said {
            Some(said) => refused.with_said(said),
            None => refused,
        });
    }
    // The edge's own answers, recognized by status and header, never
    // retried (decision 0357). Only what the endpoint does not declare
    // gets here, so an endpoint that declares 429 or 403 keeps its meaning.
    if let Some(edge) = edge_failure(&name, status, answer) {
        return Err(answered(edge));
    }
    Err(answered(failure_of(
        &codes::row::UNEXPECTED_STATUS,
        format!(
            "The endpoint `{name}` answered with status {status}, in a form the endpoint does not declare."
        ),
        "Find out what the answer means, and declare it under the endpoint's `responses`.",
    )))
}

/// The failure of an answer that comes from the edge in front of the
/// system, recognized by its status and its content type: a limit (429), or
/// a refusal that is a page (403). The page's own words are never quoted.
fn edge_failure(endpoint: &str, status: u16, answer: &Answer) -> Option<Failure> {
    let next = "Look at the surface in a browser, and ask its owner before trying again. The answer is never retried.";
    if status == 429 {
        let wait = answer
            .header("retry-after")
            .filter(|text| {
                !text.is_empty() && text.len() <= 9 && text.bytes().all(|b| b.is_ascii_digit())
            })
            .map(|seconds| format!(" It asks to wait {seconds} seconds."))
            .unwrap_or_default();
        return Some(failure_of(
            &codes::row::RATE_LIMITED_BY_EDGE,
            format!(
                "The endpoint `{endpoint}` answered with status 429, which it does not declare: a limit set in front of the system.{wait}"
            ),
            next,
        ));
    }
    (status == 403 && answer.is_a_page()).then(|| {
        failure_of(
            &codes::row::FORBIDDEN_BY_EDGE,
            format!(
                "The endpoint `{endpoint}` answered with status 403 and a page in place of data, which it does not declare: a refusal in front of the system."
            ),
            next,
        )
    })
}

/// Where a pass keeps the token the page of one write gave: by the endpoint
/// and the record's key, as a version is kept.
fn token_slot(endpoint: &str, input: &StepInput<'_>) -> (String, Vec<u8>) {
    (
        endpoint.to_owned(),
        input
            .key
            .map(a2m_core::RecordKey::canonical_bytes)
            .unwrap_or_default(),
    )
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
            (Take::Date, Json::Text(text)) => text.parse().ok().map(Value::Date),
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

/// Whether a value taken, or an entry of it, is `null`.
fn holds_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::List(items) => items.iter().any(holds_null),
        _ => false,
    }
}

/// Whether a value taken, or an entry of it, is text that holds a control
/// character this pack never returns.
fn holds_control(value: &Value) -> bool {
    match value {
        Value::Text(text) => refused_control(text),
        Value::List(items) => items.iter().any(holds_control),
        _ => false,
    }
}

/// Whether a value taken, or an entry of it, is text that holds the token,
/// in any letter case.
fn holds_token(value: &Value, token: &str) -> bool {
    match value {
        Value::Text(text) => forgery::mentions(text.as_bytes(), token),
        Value::List(items) => items.iter().any(|item| holds_token(item, token)),
        _ => false,
    }
}

impl HttpDriver {
    fn extract(
        &self,
        session: &mut Session,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, template) = judged_endpoint(session, step)?;
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
        let payload: ExtractStep = step
            .payload
            .decode()
            .map_err(|_| cannot_judge(Some(answer), "its payload does not decode".to_owned()))?;
        let mut values = Values::new();
        for wanted in &payload.values {
            let value = taken(json, &wanted.path, wanted.take).ok_or_else(|| {
                StepFailure::answered(failure_of(
                    &codes::row::RESPONSE_SCHEMA_MISMATCH,
                    format!(
                        "The answer of the endpoint `{name}` holds a value at `{}` that is not of the type the operation's output `{}` declares.",
                        wanted.path.join("."),
                        wanted.name
                    ),
                    "Compare the system's answer with the endpoint's `response_schema` and the operation's outputs.",
                ))
            })?;
            // A `null` for a value the endpoint makes certain to be there,
            // and the operation does not declare absent, is no answer the
            // description allows (decision 0305).
            if !wanted.may_be_absent && holds_null(&value) {
                return Err(StepFailure::answered(failure_of(
                    &codes::row::RESPONSE_SCHEMA_MISMATCH,
                    format!(
                        "The answer of the endpoint `{name}` holds no value at `{}`, which the endpoint declares as always there and the operation's output `{}` does not allow to be absent.",
                        wanted.path.join("."),
                        wanted.name
                    ),
                    "Compare the system's answer with the endpoint's `response_schema`; or declare the output `may_be_absent`.",
                )));
            }
            // Of every form, a value is data, and never a control
            // character but a tab or a line break (decision 0249).
            if holds_control(&value) {
                return Err(StepFailure::answered(failure_of(
                    &codes::row::RESPONSE_SCHEMA_MISMATCH,
                    format!(
                        "The answer of the endpoint `{name}` holds a control character at `{}`, which is never returned as the operation's output `{}`.",
                        wanted.path.join("."),
                        wanted.name
                    ),
                    "Ask the system's owner why the value carries a control character; a tab and a line break are kept.",
                )));
            }
            // An answer that quotes the token the write carried returns no
            // value that holds it (decision 0357).
            if let Some(token) = &answer.echoed
                && holds_token(&value, token.expose())
            {
                return Err(StepFailure::answered(failure_of(
                    &codes::row::ANSWER_HOLDS_SECRET,
                    format!(
                        "The answer of the endpoint `{name}` holds the anti-forgery token the write carried, so the value at `{}` is not returned.",
                        wanted.path.join(".")
                    ),
                    "Ask the system's owner why the answer quotes the token, or read the record through another endpoint.",
                )));
            }
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
            .send(&built, None, most, until(step, input), None)
            .await
            .map_err(|not| match not {
                NotReceived::Lost(how) => lost(&how, false, &probe.path),
                NotReceived::Refused(failure) => StepFailure::not_sent(failure),
                NotReceived::TooLarge => StepFailure::answered(failure_of(
                    &codes::row::RESPONSE_TOO_LARGE,
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
            if step.kind == TOKEN {
                self.token(session, step, input).await
            } else if step.kind == REQUEST {
                self.request(session, step, input).await
            } else if step.kind == VALIDATE {
                validate(session, step, input, &self.redact, &self.overruns).await
            } else if step.kind == EXTRACT {
                self.extract(session, step, input)
            } else if step.kind == PROBE {
                self.probe(session, step, input).await
            } else {
                Err(cannot_judge(
                    session.answer.as_ref(),
                    format!("the `http` mode has no step of the kind `{}`", step.kind),
                ))
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

    use super::{
        Answer, Expiry, OffThread, Origin, Overruns, Session, holds_token, off_thread, taken,
    };
    use crate::codes;
    use crate::json::Json;
    use crate::template::{Built, Method, Take};

    #[test]
    fn an_address_is_taken_apart_once_and_https_is_refused_without_tls() {
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
        #[cfg(not(feature = "tls"))]
        assert_eq!(
            Origin::of("https://app.example.test").unwrap_err().code(),
            &codes::TLS_NOT_ENABLED
        );
        #[cfg(feature = "tls")]
        assert_eq!(
            Origin::of("https://app.example.test:8443/v2").unwrap().text,
            "https://app.example.test:8443"
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

    #[test]
    fn a_value_that_quotes_the_token_is_found_in_any_letter_case_and_in_a_list() {
        let token = "example-token-0001";
        assert!(holds_token(
            &Value::from("The token was EXAMPLE-TOKEN-0001."),
            token
        ));
        assert!(holds_token(
            &Value::List(vec![Value::from("a"), Value::from("example-token-0001")]),
            token
        ));
        assert!(!holds_token(&Value::from("nothing here"), token));
        assert!(!holds_token(&Value::Int(1), token));
    }

    fn session(address: &str) -> Session {
        let origin = Origin::of(address).unwrap();
        Session {
            client: a2m_auth::tls::client_builder(&a2m_auth::tls::installed().unwrap())
                .build()
                .unwrap(),
            client_key: format!("- {}", origin.text),
            origin,
            surface: "api".to_owned(),
            unencrypted_accepted: false,
            sections: BTreeMap::new(),
            held: None,
            expiry: Expiry::default(),
            answer: None,
            versions: BTreeMap::new(),
            tokens: BTreeMap::new(),
            posture: a2m_provider::Posture::Live,
            asserted_reads: false,
            reading_place: "api http://app.example.test".to_owned(),
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
        let bare = session("http://app.example.test");
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
        // With no path of its own, a surface's origin is followed directly
        // by what was built: it starts at a `/`, or it is not sent.
        for path in [
            "@other.example.test/x",
            ".other.example.test/x",
            ":81/x",
            "",
        ] {
            let failure = bare.address(&built(path, "")).unwrap_err();
            assert_eq!(failure.code(), &codes::SURFACE_NOT_DECLARED, "{path}");
        }
        // A second `//` after the origin stays a path on the surface's own
        // host, and is sent as written.
        let url = session
            .address(&built("//other.example.test/x", ""))
            .unwrap();
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
            echoed: None,
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

    /// Work handed off the async threads that does not end by the deadline
    /// is left there, and told to stop: on a runtime of one thread, the
    /// deadline still fires while the work runs, so the thread was never
    /// held (decision 0249).
    #[tokio::test(flavor = "current_thread")]
    async fn work_that_outlasts_the_deadline_is_left_and_told_to_stop() {
        use std::sync::atomic::Ordering;
        let overruns = std::sync::Arc::new(Overruns::default());
        let (told, heard) = std::sync::mpsc::channel();
        let until = tokio::time::Instant::now() + std::time::Duration::from_millis(50);
        let outcome = off_thread(&overruns, "api", until, move |stop| {
            while !stop.load(Ordering::Relaxed) {
                std::hint::spin_loop();
            }
            told.send(()).unwrap();
        })
        .await;
        assert!(matches!(outcome, OffThread::OutOfTime));
        heard
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        let done = off_thread(
            &overruns,
            "api",
            until + std::time::Duration::from_secs(30),
            |_| 7,
        )
        .await;
        assert!(matches!(done, OffThread::Done(7)));
    }

    /// Reads left running past their deadline are counted for each surface
    /// and for the pack. At either ceiling a new read is refused `busy`
    /// before its work begins; once the reads left behind end, the count
    /// falls and reads run again (decision 0270). The work here ends only
    /// when the test lets it, as a match that cannot be stopped would.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reads_left_past_their_deadline_are_bounded_and_the_next_is_refused_busy() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;
        let overruns = Arc::new(Overruns::with_ceilings(2, 3));
        let mut holds = Vec::new();
        let mut left = |surface: &'static str| {
            let (hold, wait) = std::sync::mpsc::channel::<()>();
            holds.push(hold);
            let overruns = Arc::clone(&overruns);
            async move {
                let soon = tokio::time::Instant::now() + Duration::from_millis(20);
                off_thread(&overruns, surface, soon, move |_| {
                    // Ends when its sender is dropped, never by the flag.
                    let _ = wait.recv();
                })
                .await
            }
        };
        assert!(matches!(left("a").await, OffThread::OutOfTime));
        assert!(matches!(left("a").await, OffThread::OutOfTime));
        assert_eq!(overruns.running_late(), 2);
        let began = Arc::new(AtomicBool::new(false));
        let far = tokio::time::Instant::now() + Duration::from_secs(30);
        let try_on = |surface: &'static str| {
            let began = Arc::clone(&began);
            let overruns = Arc::clone(&overruns);
            async move {
                off_thread(&overruns, surface, far, move |_| {
                    began.store(true, Ordering::SeqCst);
                })
                .await
            }
        };
        // The surface's ceiling: refused before any work.
        match try_on("a").await {
            OffThread::Busy(failure) => {
                assert_eq!(failure.class(), a2m_core::FailureClass::Busy);
                assert_eq!(failure.code(), &codes::READS_STILL_RUNNING);
            }
            _ => panic!("a third read on the surface was not refused"),
        }
        assert!(!began.load(Ordering::SeqCst));
        // Another surface has room until the pack's ceiling.
        assert!(matches!(left("b").await, OffThread::OutOfTime));
        assert_eq!(overruns.running_late(), 3);
        assert!(matches!(try_on("c").await, OffThread::Busy(_)));
        assert!(!began.load(Ordering::SeqCst));
        // The reads left behind end; their places return.
        drop(holds);
        let by = tokio::time::Instant::now() + Duration::from_secs(10);
        while overruns.running_late() > 0 && tokio::time::Instant::now() < by {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(overruns.running_late(), 0);
        assert!(matches!(try_on("a").await, OffThread::Done(())));
        assert!(began.load(Ordering::SeqCst));
    }

    /// A read whose waiting is dropped, as a shadow's is at its budget, is
    /// counted as left running until its thread ends.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_read_whose_waiting_is_dropped_is_counted_until_it_ends() {
        use std::sync::Arc;
        use std::time::Duration;
        let overruns = Arc::new(Overruns::with_ceilings(1, 1));
        let (hold, wait) = std::sync::mpsc::channel::<()>();
        let far = tokio::time::Instant::now() + Duration::from_secs(30);
        let dropped = tokio::time::timeout(
            Duration::from_millis(20),
            off_thread(&overruns, "a", far, move |_| {
                let _ = wait.recv();
            }),
        )
        .await;
        assert!(dropped.is_err());
        assert_eq!(overruns.running_late(), 1);
        assert!(matches!(
            off_thread(&overruns, "b", far, |_| ()).await,
            OffThread::Busy(_)
        ));
        drop(hold);
        let by = tokio::time::Instant::now() + Duration::from_secs(10);
        while overruns.running_late() > 0 && tokio::time::Instant::now() < by {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(overruns.running_late(), 0);
    }

    /// The method a request is judged by: its own, and every method an
    /// override in it names. An override never makes a request safer than
    /// its own method, and one that names no known method may change the
    /// system (decision 0270).
    #[test]
    fn a_request_is_judged_by_its_method_and_every_override_in_it() {
        let with = |method: Method, headers: &[(&str, &str)], query: &str| Built {
            method,
            path: "/x".to_owned(),
            query: query.to_owned(),
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            body: None,
        };
        assert!(!super::may_change_by_method(&with(Method::Get, &[], "")));
        assert!(!super::may_change_by_method(&with(
            Method::Get,
            &[("x-http-method-override", "head")],
            "a=1&_method=GET"
        )));
        for request in [
            with(Method::Get, &[("X-HTTP-Method-Override", "DELETE")], ""),
            with(Method::Get, &[("x-http-method", "PUT")], ""),
            with(Method::Get, &[("x-method-override", "MERGE")], ""),
            with(Method::Get, &[], "q=1&_method=patch"),
            with(Method::Get, &[], "_METHOD=POST"),
            with(Method::Get, &[], "%5Fmethod=DELETE"),
            with(Method::Head, &[], "_method=DELETE&x=1"),
            with(Method::Post, &[("x-http-method-override", "GET")], ""),
            with(Method::Post, &[], "_method=GET"),
        ] {
            assert!(super::may_change_by_method(&request), "{request:?}");
        }
    }
}
