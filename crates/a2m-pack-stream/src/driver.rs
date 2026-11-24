//! The driver of the `stream` mode (spec section 309, "The driver, part by
//! part"). It carries out one step at a time over a [`Link`], retries
//! nothing, and says of every failure whether the request left.
//!
//! What it never does: choose a channel from a step argument, keep a
//! subscription open after its run, acknowledge on a queue the target does
//! not declare the runtime owns, acknowledge a message it has not given
//! out to be recorded, say a message is absent from a log it has not
//! covered, call a write refused when the answer has no rule, renew a
//! session, or put what a broker said into a result before the redactor
//! has seen it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use a2m_auth::{ChallengeScheme, CredentialSink, SessionLease};
use a2m_core::{
    CanonicalJson, Failure, FailureClass, FailureCode, PackPayload, RecordKey, Redact, Secret,
    Step, Value, Values,
};
use a2m_provider::{
    BoxFuture, Capture, Cleanup, Deadline, Driver, DriverHealth, Observed, Posture, PrepareContext,
    Prepared, RecordedElsewhere, Sent, StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use a2m_telemetry::{Acting, Detail};
use serde::Deserialize;

use crate::channel::{ChannelEntry, MessageSchema, Ownership, SECTION_FORMAT, Shape};
use crate::codes::{self, shown, stopped};
use crate::json::Json;
use crate::lower::{ARG_BODY, ARG_CORRELATION, ARG_FROM, ARG_HEADER, ARG_KEY, StepDetail};
use crate::names::{
    BROKER, CHANNEL, MESSAGE_SCHEMA, PACK, STEP_ACKNOWLEDGE, STEP_AWAIT, STEP_BROWSE,
    STEP_CHANNELS, STEP_DESCRIBE, STEP_LATEST, STEP_POSITION, STEP_PUBLISH, STEP_SCAN,
    STEP_SUBSCRIBE, STEP_TAKEN,
};
use crate::wire::{
    ChannelFacts, Credential, Link, Message, Outgoing, Protocol, Receipt, WireError, WireErrorKind,
};

/// How many pages a scan of a log reads before it says it cannot tell.
const SCAN_PAGES: u32 = 20;

/// The most messages a discovery's look returns, whatever it asks for.
const LARGEST_SAMPLE: u32 = 100;

/// What is kept back from a wait so that its last fetch can still be
/// answered before the step's own deadline.
const ANSWER_MARGIN: Duration = Duration::from_millis(250);

/// How long letting go of a subscription may take when a run ends.
const RELEASE_LIMIT: Duration = Duration::from_secs(2);

/// What is kept back from a consumer's acknowledgement wait: an
/// acknowledgement sent later than this before the wait ends could reach
/// the broker after it handed the message to someone else.
const ACK_WAIT_MARGIN: Duration = Duration::from_secs(1);

/// The most marks the driver keeps before it forgets the messages given
/// back.
const MARKS_MOST: usize = 100_000;

/// Which message of which channel of which broker: a mark's key.
type MessageAt = (String, String, String);

/// What the driver knows of a message of a queue it took from, in this
/// engine's life: whether a record may hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    /// A record recorded it as taken, and nothing has settled that record:
    /// its acknowledgement was not answered, or it was never sent past the
    /// broker's wait. No other record takes it.
    Held,
    /// It was handed out and given back with no record holding it, or the
    /// record that took it was settled as not taken: another take may have
    /// it, even delivered again.
    GivenBack,
}

/// Whether a delivery may be taken as new, by this engine's marks and the
/// broker's count of its deliveries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MayTake {
    /// First delivered now, or given back with no record holding it.
    Free,
    /// A record of this engine holds it.
    HeldHere,
    /// Delivered before, or with no count the broker states, and this
    /// engine saw nothing settle it: another record may hold it.
    DeliveredBefore,
}

/// A consumer one run takes from. Two runs of this engine never take from
/// one consumer at once; the mark goes when the run is let go or dropped.
struct ConsumerHold {
    taking: Arc<Mutex<BTreeSet<(String, String)>>>,
    key: (String, String),
}

impl Drop for ConsumerHold {
    fn drop(&mut self) {
        self.taking
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

/// What a surface of kind `broker` states beside its address.
#[derive(Debug, Deserialize)]
struct SurfaceDetail {
    protocol: String,
}

/// Which idle connections a run may take: those to the same address, over
/// the same protocol, opened with the same session's credentials.
type PoolKey = (String, String, Option<(String, u64)>);

/// The name `message_meta`: where a result holds what the broker says
/// about a message. No body may have a field of this name.
const MESSAGE_META: &str = "message_meta";

/// The name under which a `position` step records, before a publish, the
/// place the log ends at. It is no output of any operation: the engine
/// keeps what a step records beside the record's outputs.
const SCAN_FROM: &str = "scan_from";

/// The name under which a take's `await` step records the identifier of
/// the message it gives out to be recorded.
const MESSAGE_ID: &str = "message_id";

/// Where the session's credentials are put while a connection opens.
///
/// A wire protocol presents one token, or a user and a password, and
/// nothing else. The neutral providers place either as one header: a
/// token as the header's value (a `Bearer ` before it is dropped), a user
/// and a password as the value of the `Basic` scheme, which is read back
/// into its two parts. Anything more that a provider places (a second
/// header, a cookie, a client identity, a challenge scheme) cannot be
/// presented, and a connection opened without it would not be the
/// session's: the sink remembers that, and `prepare` stops.
#[derive(Default)]
struct CredentialPlaced {
    header: Option<Secret>,
    not_carried: bool,
}

impl CredentialPlaced {
    /// The one credential placed, as a protocol presents it; `Err` when
    /// what was placed is more than one, or not one this pack can read.
    fn credential(&self) -> Result<Option<Credential>, ()> {
        if self.not_carried {
            return Err(());
        }
        let Some(value) = &self.header else {
            return Ok(None);
        };
        let value = value.expose();
        if let Some(pair) = value.strip_prefix("Basic ") {
            let decoded = crate::nats_wire::base64_decode(pair.trim()).ok_or(())?;
            let decoded = String::from_utf8(decoded).map_err(|_| ())?;
            let (user, password) = decoded.split_once(':').ok_or(())?;
            return Ok(Some(Credential::UserPassword {
                user: user.to_owned(),
                password: Secret::new(password),
            }));
        }
        let token = value.strip_prefix("Bearer ").unwrap_or(value);
        Ok(Some(Credential::Token(Secret::new(token))))
    }
}

impl CredentialSink for CredentialPlaced {
    fn header(&mut self, _name: &str, value: &Secret) {
        if self.header.is_none() {
            self.header = Some(Secret::new(value.expose()));
        } else {
            self.not_carried = true;
        }
    }

    fn cookie(&mut self, _origin: &str, _name: &str, _value: &Secret) {
        self.not_carried = true;
    }

    fn tls_identity(&mut self, _certificate: &[u8], _key: &Secret) {
        self.not_carried = true;
    }

    fn challenge_credential(&mut self, _scheme: ChallengeScheme, _user: &str, _secret: &Secret) {
        self.not_carried = true;
    }
}

/// A subscription the run opened.
struct Open {
    handle: String,
    /// Whether the broker said where in the channel it stands. Only then
    /// can a wait that saw nothing say what it covered.
    placed: bool,
    /// How long the broker waits for an acknowledgement, where it says.
    ack_wait: Option<Duration>,
}

/// A message delivered to the run and not yet acknowledged.
struct Held {
    channel: String,
    handle: String,
    delivery: String,
    message_id: String,
    /// The channel's address, for the message's mark.
    address: String,
    /// When the broker handed it over, and how long it waits for the
    /// acknowledgement.
    delivered_at: tokio::time::Instant,
    ack_wait: Option<Duration>,
}

/// What one run holds between `prepare` and `release`.
struct Run {
    /// The connection. Nothing when it was given up after a failure.
    link: Option<Box<dyn Link>>,
    key: PoolKey,
    posture: Posture,
    channels: BTreeMap<String, ChannelEntry>,
    schemas: BTreeMap<String, MessageSchema>,
    /// The channels whose shape was compared with what the broker reports.
    checked: BTreeSet<String>,
    /// The run's own subscriptions, by channel.
    open: BTreeMap<String, Open>,
    held: Option<Held>,
    /// The identifiers of the messages delivered to this run. A broker
    /// delivers at least once and does not mark a repeat.
    seen: BTreeSet<String>,
    /// The confirmations of this run's publishes, by the record's token.
    receipts: BTreeMap<String, Receipt>,
    /// Whether a step of the run failed. Its connection is then not kept.
    failed: bool,
    /// The wire protocol the run speaks, as the surface names it.
    protocol: &'static str,
    /// How the step being carried out is done, for its trace record: set
    /// up when the step starts, and handed over as its `acting` capture.
    acting: Acting,
    /// The consumers the run takes from, held against every other run.
    holds: Vec<ConsumerHold>,
}

/// The driver.
pub(crate) struct StreamDriver {
    redact: Arc<dyn Redact>,
    protocols: Vec<Arc<dyn Protocol>>,
    idle: Mutex<BTreeMap<PoolKey, Vec<Box<dyn Link>>>>,
    idle_limit: usize,
    /// The consumers some run of this engine takes from now: the broker's
    /// address, and the channel's address with the consumer's name.
    taking: Arc<Mutex<BTreeSet<(String, String)>>>,
    /// What this engine knows of the messages its runs took.
    marks: Mutex<BTreeMap<MessageAt, Mark>>,
}

/// The run a `Prepared` of this driver holds.
fn run_of(prepared: &mut Prepared) -> Result<&mut Run, StepFailure> {
    prepared
        .state::<Run>()
        .ok_or_else(|| wrong_plan("the run was prepared by another driver"))
}

fn wrong_plan(what: &str) -> StepFailure {
    StepFailure::not_sent(Failure::new(
        FailureClass::Input,
        FailureCode::PAYLOAD_UNREADABLE,
        format!("The plan does not fit this build of the stream pack: {what}."),
        "Compile the target again with the versions of the packs that are enabled.",
    ))
}

fn read_only(what: &str) -> StepFailure {
    StepFailure::not_sent(Failure::new(
        FailureClass::Policy,
        a2m_provider::codes::READ_ONLY_POSTURE,
        format!("The stream driver was asked to {what} where nothing may be written or taken."),
        "Run the operation live, or against a declared test record.",
    ))
}

/// What a broker said, for a sentence: redacted first, and only then
/// cleaned and cut, so that a cut never leaves part of a secret behind.
fn said(redact: &dyn Redact, error: &WireError) -> String {
    shown(&redact.redact(&error.detail))
}

/// The failure of a call on the wire, as the failure table of section 309
/// names it. `what` says what was being done, for the sentence.
fn wire_failure(redact: &dyn Redact, error: &WireError, what: &str) -> StepFailure {
    let (code, message, next): (FailureCode, String, &str) = match error.kind {
        WireErrorKind::Unreachable => (
            codes::UPSTREAM_UNAVAILABLE,
            format!("The broker could not be reached to {what}."),
            "Check that the broker is running and that its address is the surface's, then try again.",
        ),
        WireErrorKind::Lost => (
            codes::CONNECTION_LOST,
            format!("The connection to the broker was lost on the way to {what}."),
            "Try again; a read changes nothing.",
        ),
        WireErrorKind::TimedOut => (
            codes::TIMEOUT,
            format!("The broker did not answer in time to {what}."),
            "Try again, or raise the step's time limit if the broker is known to be slow.",
        ),
        WireErrorKind::NotAuthorized => (
            codes::NEEDS_LOGIN,
            "The broker refused the session's credentials.".to_owned(),
            "Sign in to the target again with `anything2mcp session login`.",
        ),
        WireErrorKind::ChannelMissing => (
            codes::CHANNEL_MISSING,
            format!("The broker has no channel at the address the target declares, so it could not {what}."),
            "Check the channel's `address` against the broker, and correct the channel file.",
        ),
        WireErrorKind::SubscriptionMissing => (
            codes::CHANNEL_MISSING,
            "The runtime's own subscription, which the channel file names, is not declared on the broker.".to_owned(),
            "Have the subscription made on the broker, or correct `subscription` in the channel file. The runtime never creates one.",
        ),
        WireErrorKind::NotSupported => (
            codes::SHAPE_CHANGED,
            format!("The broker says its channel does not allow the runtime to {what}: its shape is not the one the target declares."),
            "Run a discovery of the broker and correct the channel file's `shape`.",
        ),
        WireErrorKind::Refused => (
            codes::BROKER_REFUSED,
            "The broker refused the message.".to_owned(),
            "Correct what the broker names, then send the message again.",
        ),
        WireErrorKind::TooLarge => (
            codes::MESSAGE_TOO_LARGE,
            "The broker refused the message as larger than its channel accepts.".to_owned(),
            "Shorten the message, and lower `limits.message_bytes` in the channel file to the broker's limit.",
        ),
        WireErrorKind::SubscriptionInUse => (
            codes::SUBSCRIPTION_IN_USE,
            "The runtime's own subscription is held by another run.".to_owned(),
            "Wait for the other run to end, then try again.",
        ),
        WireErrorKind::PositionOutOfRange => (
            codes::POSITION_OUT_OF_RANGE,
            "The log no longer holds the position the run was to read from.".to_owned(),
            "Start from a position the log still holds; its retention has passed the one given.",
        ),
        WireErrorKind::Transport => (
            codes::TRANSPORT_REFUSED,
            format!("The connection to {what} was not opened: {}.", error.detail),
            "Write the broker's address as this machine's loopback IP address (127.0.0.1 or [::1]), or use a build that speaks TLS to it.",
        ),
        WireErrorKind::Unfit => (
            codes::VALUE_OUT_OF_RANGE,
            format!("The wire protocol cannot carry what the step would send to {what}: {}.", error.detail),
            "Correct the value or the channel file; nothing was sent.",
        ),
        WireErrorKind::Unexpected => (
            codes::UNEXPECTED_REPLY,
            format!(
                "The broker answered in a way the driver has no rule for while it tried to {what}."
            ),
            "Check that the surface's `protocol` is the one the broker speaks; then report this.",
        ),
    };
    let failed = stopped(error.sent, code, message, next);
    // What the broker said is its own, kept apart from the sentence.
    match error.kind {
        WireErrorKind::Refused | WireErrorKind::Unexpected => failed.with_said(said(redact, error)),
        _ => failed,
    }
}

/// The failure of a publish or an acknowledgement. A write is refused only
/// when the broker said so in words the protocol defines for it. A lost
/// answer, an answer that cannot be read, and a refusal with no rule all
/// leave the write in doubt: nothing says it did not happen.
fn doubtful(redact: &dyn Redact, error: &WireError, what: &str) -> StepFailure {
    let refused = match error.kind {
        WireErrorKind::Unreachable
        | WireErrorKind::Lost
        | WireErrorKind::TimedOut
        | WireErrorKind::NotAuthorized
        | WireErrorKind::ChannelMissing
        | WireErrorKind::NotSupported
        | WireErrorKind::Refused
        | WireErrorKind::TooLarge
        | WireErrorKind::Transport
        | WireErrorKind::Unfit
        // A consumer that is gone acknowledged nothing.
        | WireErrorKind::SubscriptionMissing => error.sent != Sent::Unknown,
        // A refusal that belongs to no write, and an answer of no rule.
        WireErrorKind::SubscriptionInUse
        | WireErrorKind::PositionOutOfRange
        | WireErrorKind::Unexpected => error.sent == Sent::NotSent,
    };
    if refused {
        return wire_failure(redact, error, what);
    }
    let unexpected = error.kind == WireErrorKind::Unexpected;
    let message = if unexpected {
        format!(
            "The request to {what} was sent and the broker answered in a way the driver has no rule for, so nothing says whether it happened."
        )
    } else {
        format!("The request to {what} was sent and the broker's confirmation did not arrive.")
    };
    let failed = stopped(
        Sent::Unknown,
        codes::NO_CONFIRMATION,
        message,
        "Reconcile the record before anything is sent again: the broker may have carried the request out.",
    );
    if unexpected {
        failed.with_said(said(redact, error))
    } else {
        failed
    }
}

fn text_arg<'a>(input: &'a StepInput<'_>, name: &str) -> Result<&'a str, StepFailure> {
    input
        .args
        .get(name)
        .and_then(Value::as_text)
        .ok_or_else(|| {
            stopped(
                Sent::NotSent,
                codes::VALUE_OUT_OF_RANGE,
                format!("The step's `{name}` is not text."),
                "Give the value as text.",
            )
        })
}

/// A log position from a step argument: absent, or a whole number that is
/// not negative.
fn position_arg(input: &StepInput<'_>) -> Result<Option<u64>, StepFailure> {
    match input.args.get(ARG_FROM) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_int()
            .and_then(|number| u64::try_from(number).ok())
            .map(Some)
            .ok_or_else(|| {
                stopped(
                    Sent::NotSent,
                    codes::VALUE_OUT_OF_RANGE,
                    "A position in a log is a whole number that is not negative.",
                    "Give a position the log reported.",
                )
            }),
    }
}

fn position_value(position: Option<u64>) -> Value {
    position
        .and_then(|position| i64::try_from(position).ok())
        .map_or(Value::Null, Value::Int)
}

fn mismatch(why: &str) -> StepFailure {
    stopped(
        Sent::Answered,
        codes::RESPONSE_SCHEMA_MISMATCH,
        format!("A message the broker returned does not fit what the target declares: {why}."),
        "Compare the channel's schema with what the broker now sends, and correct the target's files.",
    )
}

impl Run {
    fn link(&mut self) -> Result<&mut Box<dyn Link>, StepFailure> {
        self.link.as_mut().ok_or_else(|| {
            stopped(
                Sent::NotSent,
                codes::CONNECTION_LOST,
                "The run's connection to the broker was given up after an earlier failure.",
                "Start the run again.",
            )
        })
    }

    /// Whether this step may write or take. Live, any step the plan marks
    /// to write. In a sandbox with test records, only the step marked
    /// `commits`: the engine gives that posture to the context that writes
    /// a declared test record, and cannot see what a step that is not
    /// marked does there, so the driver's own guard stays for every other
    /// step.
    fn writes_allowed(&self, step: &Step) -> bool {
        match self.posture {
            Posture::Live => true,
            Posture::Sandbox { test_records: true } => step.commits,
            _ => false,
        }
    }

    /// The channel a step of the plan refers to.
    fn declared(&self, step: &Step) -> Result<(String, ChannelEntry), StepFailure> {
        let Some(refers_to) = &step.refers_to else {
            return Err(wrong_plan("a step names no channel"));
        };
        self.channels
            .get(&refers_to.entry)
            .map(|entry| (refers_to.entry.clone(), entry.clone()))
            .ok_or_else(|| wrong_plan("a step names a channel its section does not hold"))
    }

    /// A detail of how the step was done, which the trace's signature
    /// covers: never a value.
    fn mean(&mut self, name: &str, value: &str) {
        self.acting.means.push(Detail::new(name, value));
    }

    /// A count the step measured, which the signature does not cover.
    fn measure(&mut self, name: &str, count: usize) {
        self.acting
            .measured
            .push(Detail::new(name, count.to_string()));
    }

    /// Compares, once per run, the channel's declared shape and key with
    /// what the broker reports.
    async fn check_shape(
        &mut self,
        redact: &dyn Redact,
        name: &str,
        entry: &ChannelEntry,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        if self.checked.contains(name) {
            self.mean("shape_check", "earlier");
            return Ok(());
        }
        let facts = self
            .link()?
            .describe(&entry.address, deadline)
            .await
            .map_err(|error| wire_failure(redact, &error, "describe the channel"))?;
        self.compare_shape(name, entry, &facts)
    }

    /// What the broker reports about a channel, as a step that needs the
    /// report itself reads it; compared with the file once per run.
    async fn described(
        &mut self,
        redact: &dyn Redact,
        name: &str,
        entry: &ChannelEntry,
        deadline: Deadline,
    ) -> Result<ChannelFacts, StepFailure> {
        let facts = self
            .link()?
            .describe(&entry.address, deadline)
            .await
            .map_err(|error| wire_failure(redact, &error, "describe the channel"))?;
        if self.checked.contains(name) {
            self.mean("shape_check", "earlier");
        } else {
            self.compare_shape(name, entry, &facts)?;
        }
        Ok(facts)
    }

    /// The comparison of the channel's declared shape and key with what the
    /// broker reports.
    fn compare_shape(
        &mut self,
        name: &str,
        entry: &ChannelEntry,
        facts: &ChannelFacts,
    ) -> Result<(), StepFailure> {
        let changed = |what: &str| {
            stopped(
                Sent::Answered,
                codes::SHAPE_CHANGED,
                format!("The channel `{name}` is not what its file declares: {what}."),
                "Run a discovery of the broker and correct the channel file.",
            )
        };
        if facts.shape != Some(entry.shape) {
            return Err(changed("the broker reports another shape"));
        }
        if let (Some(reported), Some(declared)) = (&facts.key_field, &entry.key_field)
            && reported != declared
        {
            return Err(changed("the broker takes the key from another field"));
        }
        if entry.broker_dedupe {
            // The file says the broker refuses a repeated token. Where the
            // broker says otherwise, a resumed publish could store twice.
            if facts.dedupe_header.is_none() || facts.dedupe_header != entry.token_header {
                return Err(changed(
                    "the broker does not refuse a repeated token in the header the file declares",
                ));
            }
            if let (Some(declared), Some(reported)) =
                (entry.dedupe_window_ms, facts.dedupe_window_ms)
                && reported < u64::from(declared)
            {
                return Err(changed(
                    "the broker remembers a token for less time than the file's `idempotency.window`",
                ));
            }
        }
        self.checked.insert(name.to_owned());
        self.mean("shape_check", "made");
        Ok(())
    }

    /// Checks a message against the channel's schema, and gives it as the
    /// values a caller sees: the body's fields, and `message_meta` with
    /// what the broker says about the message. `message_meta` is always
    /// the broker's: a body with a field of that name is refused. Of the
    /// headers, only the one the channel declares for correlation is
    /// given; what other producers put in headers is not the caller's.
    fn record(&self, entry: &ChannelEntry, message: &Message) -> Result<Values, StepFailure> {
        if let Some(name) = &entry.schema {
            let schema = self
                .schemas
                .get(name)
                .ok_or_else(|| wrong_plan("a channel names a schema the plan does not hold"))?;
            schema.check(&message.body).map_err(|why| mismatch(&why))?;
        }
        let mut values = match message.body.to_value().map_err(mismatch)? {
            Value::Record(fields) => fields,
            other => Values::new().with("body", other),
        };
        if values.contains(MESSAGE_META) {
            return Err(mismatch(
                "its body has a field named `message_meta`, the name under which a result holds what the broker says about a message",
            ));
        }
        let declared = entry
            .correlation
            .as_deref()
            .and_then(|field| field.strip_prefix("headers."));
        let headers: Values = message
            .headers
            .iter()
            .filter(|(name, _)| Some(name.as_str()) == declared)
            .map(|(name, value)| (name.clone(), Value::from(value.as_str())))
            .collect();
        values.insert(
            MESSAGE_META,
            Values::new()
                .with("id", message.id.as_str())
                .with(
                    "key",
                    message.key.as_deref().map_or(Value::Null, Value::from),
                )
                .with("position", position_value(message.position))
                .with("headers", headers),
        );
        Ok(values)
    }
}

impl StreamDriver {
    pub(crate) fn new(
        redact: Arc<dyn Redact>,
        protocols: Vec<Arc<dyn Protocol>>,
        idle_limit: usize,
    ) -> Self {
        Self {
            redact,
            protocols,
            idle: Mutex::new(BTreeMap::new()),
            idle_limit,
            taking: Arc::new(Mutex::new(BTreeSet::new())),
            marks: Mutex::new(BTreeMap::new()),
        }
    }

    fn mark(&self, at: MessageAt, mark: Option<Mark>) {
        let mut marks = self.marks.lock().unwrap_or_else(PoisonError::into_inner);
        match mark {
            Some(mark) => {
                // Bounded: forgetting that a message was given back only
                // makes a later take of it stricter, never looser.
                if marks.len() >= MARKS_MOST {
                    marks.retain(|_, kept| *kept == Mark::Held);
                }
                marks.insert(at, mark);
            }
            None => {
                marks.remove(&at);
            }
        }
    }

    fn mark_of(&self, at: &MessageAt) -> Option<Mark> {
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(at)
            .copied()
    }

    /// Whether a delivery may be taken as new: never one a record of this
    /// engine holds, and one the broker delivered before only when this
    /// engine saw it given back with no record holding it, or the engine
    /// answers that no other record holds it. A delivery whose count the
    /// broker does not state counts as delivered before.
    fn may_take(&self, at: &MessageAt, attempt: Option<u64>) -> MayTake {
        match (self.mark_of(at), attempt) {
            (Some(Mark::Held), _) => MayTake::HeldHere,
            (_, Some(1)) | (Some(Mark::GivenBack), _) => MayTake::Free,
            _ => MayTake::DeliveredBefore,
        }
    }

    fn protocol_of(&self, surface: &SurfaceRef) -> Option<&Arc<dyn Protocol>> {
        let detail: SurfaceDetail = surface.surface.payload.decode().ok()?;
        self.protocols
            .iter()
            .find(|protocol| protocol.name() == detail.protocol)
    }

    fn take_idle(&self, key: &PoolKey) -> Option<Box<dyn Link>> {
        let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
        let links = idle.get_mut(key)?;
        while let Some(link) = links.pop() {
            if link.alive() {
                return Some(link);
            }
        }
        None
    }

    fn keep_idle(&self, key: PoolKey, link: Box<dyn Link>) {
        let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
        let links = idle.entry(key).or_default();
        if links.len() < self.idle_limit {
            links.push(link);
        }
    }

    fn done(&self, values: Values) -> StepOutcome {
        StepOutcome::Done(Observed {
            values: self.redact.redact_values(&values),
            ..Observed::default()
        })
    }

    async fn latest(
        &self,
        run: &mut Run,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = run.declared(step)?;
        let key = text_arg(input, ARG_KEY)?;
        run.check_shape(&*self.redact, &name, &entry, input.deadline)
            .await?;
        let found = run
            .link()?
            .latest(&entry.address, key, input.deadline)
            .await
            .map_err(|error| wire_failure(&*self.redact, &error, "read the latest value"))?;
        let Some(message) = found else {
            return Ok(StepOutcome::Absent(Observed::default()));
        };
        if message.key.as_deref() != Some(key) {
            return Err(stopped(
                Sent::Answered,
                codes::UNEXPECTED_REPLY,
                "The broker returned the value of another key than the one asked for.",
                "Report this; the driver has no rule for it.",
            ));
        }
        Ok(self.done(run.record(&entry, &message)?))
    }

    async fn browse(
        &self,
        run: &mut Run,
        step: &Step,
        detail: &StepDetail,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        if step.refers_to.is_none() {
            return self.sample(run, detail, input).await;
        }
        let (name, entry) = run.declared(step)?;
        let from = position_arg(input)?;
        run.check_shape(&*self.redact, &name, &entry, input.deadline)
            .await?;
        let page = run
            .link()?
            .browse(&entry.address, from, entry.browse_count, input.deadline)
            .await
            .map_err(|error| wire_failure(&*self.redact, &error, "look at the channel"))?;
        let next = page
            .messages
            .last()
            .and_then(|last| last.position)
            .map(|position| position + 1);
        let messages = page
            .messages
            .iter()
            .map(|message| run.record(&entry, message).map(Value::Record))
            .collect::<Result<Vec<Value>, StepFailure>>()?;
        run.measure("messages", messages.len());
        let count = i64::try_from(messages.len()).unwrap_or(i64::MAX);
        // Each field of the messages as one list of its values in their
        // order, `null` where a message has none: what a list's outputs are
        // read from, since an output is one value or a list of values. The
        // fields are the channel's schema's, or else those the messages
        // hold; none takes over a name the look states of itself.
        let fields: BTreeSet<String> =
            match entry.schema.as_ref().and_then(|name| run.schemas.get(name)) {
                Some(schema) => schema.fields.keys().cloned().collect(),
                None => messages
                    .iter()
                    .filter_map(Value::as_record)
                    .flat_map(|record| record.iter().map(|(name, _)| name.to_owned()))
                    .collect(),
            };
        let mut values = Values::new();
        for field in fields.iter().filter(|field| {
            field.as_str() != MESSAGE_META && !crate::draft::LOOK_FACTS.contains(&field.as_str())
        }) {
            let list: Vec<Value> = messages
                .iter()
                .map(|message| {
                    message
                        .as_record()
                        .and_then(|record| record.get(field))
                        .cloned()
                        .unwrap_or(Value::Null)
                })
                .collect();
            values.insert(field.clone(), Value::List(list));
        }
        Ok(self.done(
            values
                .with("messages", messages)
                .with("count", count)
                .with("more", page.more)
                .with("coverage", if page.more { "partial" } else { "complete" })
                .with("next", position_value(next.or(page.end))),
        ))
    }

    /// A discovery's look at a channel the broker listed: the names and
    /// JSON types of the fields seen, and never a value.
    async fn sample(
        &self,
        run: &mut Run,
        detail: &StepDetail,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let address = discovery_address(run, detail)?;
        let max = detail.sample.clamp(1, LARGEST_SAMPLE);
        let page = run
            .link()?
            .browse(&address, None, max, input.deadline)
            .await
            .map_err(|error| wire_failure(&*self.redact, &error, "look at the channel"))?;
        let mut fields: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
        let mut bytes = 0_usize;
        for message in &page.messages {
            bytes += message.body.to_line().len();
            match message.body.as_object() {
                Some(map) => {
                    for (name, value) in map {
                        fields
                            .entry(name.clone())
                            .or_default()
                            .insert(value.type_name());
                    }
                }
                None => {
                    fields
                        .entry(String::new())
                        .or_default()
                        .insert(message.body.type_name());
                }
            }
        }
        let fields: Values = fields
            .into_iter()
            .map(|(name, types)| {
                let types: Vec<Value> = types.into_iter().map(Value::from).collect();
                (name, Value::List(types))
            })
            .collect();
        Ok(self.done(
            Values::new()
                .with("fields", fields)
                .with(
                    "sampled",
                    i64::try_from(page.messages.len()).unwrap_or(i64::MAX),
                )
                .with("bytes", i64::try_from(bytes).unwrap_or(i64::MAX))
                .with("more", page.more),
        ))
    }

    async fn scan(
        &self,
        run: &mut Run,
        step: &Step,
        input: &StepInput<'_>,
        recorded: &Values,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = run.declared(step)?;
        let (Some(stamp), Some(header)) = (input.stamp, &entry.token_header) else {
            return Err(stopped(
                Sent::NotSent,
                codes::VALUE_OUT_OF_RANGE,
                format!(
                    "A scan of the channel `{name}` needs the record's token and the header it travels in."
                ),
                "Declare `idempotency.header` in the channel file, and give the operation the `stamped` identity.",
            ));
        };
        // Where the log ended before the publish was sent, as a step of
        // the record recorded it: the message, if it was stored, is there
        // or after it. Checked before anything is sent.
        let recorded_from = match recorded.get(SCAN_FROM) {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_int()
                    .and_then(|position| u64::try_from(position).ok())
                    .ok_or_else(|| {
                        nothing_recorded(&format!(
                            "the place a scan of the log `{name}` starts at is not a whole number"
                        ))
                    })?,
            ),
        };
        // The stream's own configuration, read now: one that can lose a
        // message from inside its range between the publish and this look
        // never lets the look say "absent".
        let facts = run
            .described(&*self.redact, &name, &entry, input.deadline)
            .await?;
        // Where this run's own publish was confirmed, read that one place;
        // else from where the log ended before it; else from the start.
        let confirmed = run.receipts.get(stamp).and_then(|receipt| receipt.position);
        let (mut from, began) = match (confirmed, recorded_from) {
            (Some(position), _) => (Some(position), "confirmed"),
            (None, Some(position)) => (Some(position), "recorded"),
            (None, None) => (None, "start"),
        };
        let known = confirmed.is_some();
        run.mean("from", began);
        // "Absent" rests on two things: a stream that loses nothing from
        // inside its range, and a range read whole, every position present,
        // from where the log ended before the publish to where it ends now.
        // A gap is a message gone from inside, whatever removed it; with no
        // place recorded before the publish, the range has no beginning.
        // With no place recorded, the range has a beginning only where the
        // log has lost nothing from its front: a log read from its first
        // position, which is the first it ever held, is its whole history.
        let mut origin_whole = false;
        let may_say_absent = !facts.may_lose_inside && (known || recorded_from.is_some());
        let mut expect = from;
        let mut whole = true;
        let mut pages = 0_usize;
        let mut looked = 0_usize;
        for _ in 0..SCAN_PAGES {
            let max = if known { 1 } else { entry.browse_count };
            let page = run
                .link()?
                .browse(&entry.address, from, max, input.deadline)
                .await
                .map_err(|error| wire_failure(&*self.redact, &error, "scan the log"))?;
            pages += 1;
            if pages == 1 && from.is_none() && page.start == Some(1) {
                origin_whole = true;
                expect = page.start;
            }
            looked += page.messages.len();
            run.acting.measured.clear();
            run.measure("pages", pages);
            run.measure("messages", looked);
            if let Some(found) = page
                .messages
                .iter()
                .find(|message| message.headers.get(header).map(String::as_str) == Some(stamp))
            {
                return Ok(self.done(run.record(&entry, found)?));
            }
            for message in &page.messages {
                match (expect, message.position) {
                    (Some(wanted), Some(at)) if at == wanted => expect = Some(at + 1),
                    _ => whole = false,
                }
            }
            if known {
                // The one place this run's own publish was confirmed at
                // holds another message, or none: the broker confirmed a
                // message that is not there now.
                return Err(inconclusive(&name));
            }
            // "Absent" is said only of a log that was covered to its end.
            // Where the look says more is held, the scan goes on from the
            // place after the last message; a look that gives no such place,
            // or contradicts itself, is an answer of no rule.
            let next = page
                .messages
                .last()
                .and_then(|last| last.position)
                .map(|position| position + 1)
                .filter(|next| from.is_none_or(|from| *next > from));
            if !page.more {
                let reached = next.or(from).or(page.start).unwrap_or(0);
                if page.end.is_some_and(|end| reached < end) {
                    return Err(uncovered(&name));
                }
                let may_say_absent = may_say_absent || (origin_whole && !facts.may_lose_inside);
                if !may_say_absent || !whole || (page.end.is_some() && expect != page.end) {
                    return Err(inconclusive(&name));
                }
                return Ok(StepOutcome::Absent(Observed::default()));
            }
            let Some(next) = next else {
                return Err(uncovered(&name));
            };
            from = Some(next);
        }
        Err(stopped(
            Sent::Answered,
            codes::SCAN_LIMIT,
            format!(
                "The log `{name}` holds more than a scan may read, so the driver cannot say that the record's message is not there."
            ),
            "Look at the log by hand before anything is sent again, or shorten its retention.",
        ))
    }

    async fn subscribe(
        &self,
        run: &mut Run,
        step: &Step,
        detail: &StepDetail,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = run.declared(step)?;
        if run.posture == Posture::Discovery || (detail.take && !run.writes_allowed(step)) {
            return Err(read_only("open a subscription"));
        }
        if detail.take && entry.ownership != Ownership::RuntimeOwned {
            return Err(not_owned(&name));
        }
        if !detail.take && (entry.shape == Shape::Queue || !entry.own_subscription) {
            return Err(wrong_plan(
                "a wait on a channel that is not read through a subscription of the runtime's own",
            ));
        }
        if run.open.contains_key(&name) {
            return Ok(self.done(Values::new().with("position", Value::Null)));
        }
        let from = position_arg(input)?;
        if detail.take {
            // One run of this engine at a time takes from one consumer: a
            // second would be handed what the first may hold.
            let key = (
                run.key.0.clone(),
                format!(
                    "{}#{}",
                    entry.address,
                    entry.subscription.as_deref().unwrap_or_default()
                ),
            );
            let fresh = self
                .taking
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key.clone());
            if !fresh {
                return Err(stopped(
                    Sent::NotSent,
                    codes::SUBSCRIPTION_IN_USE,
                    format!(
                        "Another run takes from the channel `{name}` through the same subscription now."
                    ),
                    "Wait for the other run to end, then try again.",
                ));
            }
            run.holds.push(ConsumerHold {
                taking: Arc::clone(&self.taking),
                key,
            });
        }
        run.check_shape(&*self.redact, &name, &entry, input.deadline)
            .await?;
        // On a queue the runtime owns, the consumer a person made for its
        // takes; elsewhere the runtime's own subscription, or one the broker
        // discards with the connection.
        let subscription = entry.subscription.as_deref();
        let opened = run
            .link()?
            .subscribe(&entry.address, subscription, from, input.deadline)
            .await
            .map_err(|error| {
                wire_failure(&*self.redact, &error, "open the runtime's subscription")
            })?;
        let position = position_value(opened.position);
        run.open.insert(
            name,
            Open {
                handle: opened.handle,
                placed: opened.position.is_some(),
                ack_wait: opened.ack_wait,
            },
        );
        Ok(self.done(Values::new().with("position", position)))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one wait: each delivery is judged in order, and splitting the loop would hide that order"
    )]
    async fn wait(
        &self,
        prepared: &mut Prepared,
        step: &Step,
        detail: &StepDetail,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        // Borrowed again after the engine is asked about another record,
        // which needs the `Prepared` itself.
        let mut run = run_of(prepared)?;
        let (name, entry) = run.declared(step)?;
        let Some((handle, placed, ack_wait)) = run
            .open
            .get(&name)
            .map(|open| (open.handle.clone(), open.placed, open.ack_wait))
        else {
            return Err(wrong_plan("a wait before its subscription was opened"));
        };
        if detail.take
            && (entry.shape != Shape::Queue || entry.ownership != Ownership::RuntimeOwned)
        {
            return Err(not_owned(&name));
        }
        let key = if detail.by_key {
            Some(text_arg(input, ARG_KEY)?)
        } else {
            None
        };
        let correlation = if detail.by_correlation {
            Some(text_arg(input, ARG_CORRELATION)?)
        } else {
            None
        };
        let limit = Duration::from_millis(u64::from(detail.wait_ms));
        let started = tokio::time::Instant::now();
        let mut examined = 0_i64;
        // Deliveries of a queue that did not match: given back when the
        // wait ends, so that they are not handed to this run again first.
        let mut passed_over: Vec<(String, MessageAt)> = Vec::new();
        let mut outcome: Result<Option<Message>, StepFailure> = Ok(None);
        let mut delivery_held = None;
        let mut delivered_at = tokio::time::Instant::now();
        let broker = run_address(run);
        let at = |id: &str| (broker.clone(), entry.address.clone(), id.to_owned());
        loop {
            let left = limit.saturating_sub(started.elapsed());
            let wait = left.min(input.deadline.remaining().saturating_sub(ANSWER_MARGIN));
            if left.is_zero() || wait.is_zero() {
                if !left.is_zero() {
                    outcome = Err(stopped(
                        Sent::Answered,
                        codes::TIMEOUT,
                        "The step's time limit passed before the wait's own limit.",
                        "Give the step more time than its wait.",
                    ));
                }
                break;
            }
            let fetched = match run.link() {
                Ok(link) => link
                    .fetch(&handle, 1, wait, input.deadline)
                    .await
                    .map_err(|error| wire_failure(&*self.redact, &error, "wait for a message")),
                Err(failed) => Err(failed),
            };
            let delivered = match fetched {
                Ok(delivered) => delivered,
                Err(failed) => {
                    outcome = Err(failed);
                    break;
                }
            };
            if delivered.len() > 1 {
                // One was asked for. The others were handed over and would
                // go unexamined; giving the connection up gives them back.
                outcome = Err(stopped(
                    Sent::Unknown,
                    codes::UNEXPECTED_REPLY,
                    "The broker handed over more messages than the one the wait asked for.",
                    "Check that the surface's `protocol` is the one the broker speaks; then report this.",
                ));
                break;
            }
            let Some(delivered) = delivered.into_iter().next() else {
                continue;
            };
            examined += 1;
            if !run.seen.insert(delivered.message.id.clone()) {
                // The same message again: at least once is not exactly once.
                continue;
            }
            let message = delivered.message;
            // A message delivered before, or one a record of this engine
            // took and has not settled, may be held by that record: it is
            // never taken as new, and nothing is acknowledged, unless the
            // engine answers that no other record holds it (decision 0197).
            let taken_here = at(&message.id);
            let judged = self.may_take(&taken_here, delivered.attempt);
            if detail.take && judged != MayTake::Free {
                let elsewhere = if judged == MayTake::DeliveredBefore {
                    let asked = prepared
                        .recorded_elsewhere(
                            MESSAGE_ID,
                            &Value::from(message.id.as_str()),
                            input.deadline,
                        )
                        .await;
                    run = run_of(prepared)?;
                    asked
                } else {
                    RecordedElsewhere::CannotTell
                };
                match elsewhere {
                    RecordedElsewhere::NotHeld => {}
                    RecordedElsewhere::Held(record) => {
                        outcome = Err(stopped(
                            Sent::Answered,
                            codes::MESSAGE_MAY_BE_HELD,
                            format!(
                                "A message of the channel `{name}` was delivered before, and the record {} of step {} of the run `{}` holds it. Nothing was taken.",
                                record.record_seq, record.step_seq, record.run
                            ),
                            "Settle that record first; the message is then taken by the next run.",
                        ));
                        break;
                    }
                    RecordedElsewhere::CannotTell => {
                        outcome = Err(stopped(
                            Sent::Answered,
                            codes::MESSAGE_MAY_BE_HELD,
                            format!(
                                "A message of the channel `{name}` was delivered before{}, and the record that took it first may hold it. Nothing was taken.",
                                delivered.attempt.map_or_else(
                                    || " (the broker does not say how often)".to_owned(),
                                    |attempt| format!(" ({attempt} deliveries)")
                                )
                            ),
                            "Settle the record that took the message first; or, where none did, remove the message from the queue by hand.",
                        ));
                        break;
                    }
                }
            }
            let matches = key.is_none_or(|key| message.key.as_deref() == Some(key))
                && correlation.is_none_or(|wanted| {
                    correlation_of(&entry, &message).as_deref() == Some(wanted)
                });
            if !matches {
                if detail.take {
                    passed_over.push((delivered.delivery, taken_here));
                }
                continue;
            }
            delivery_held = Some(delivered.delivery);
            delivered_at = tokio::time::Instant::now();
            outcome = Ok(Some(message));
            break;
        }
        for (delivery, message_at) in passed_over {
            if let Ok(link) = run.link() {
                // Best effort: what is not given back here goes back when
                // the subscription is let go. Either way no record holds it.
                let _ = link
                    .release(&handle, &delivery, Deadline::after(RELEASE_LIMIT))
                    .await;
            }
            self.mark(message_at, Some(Mark::GivenBack));
        }
        run.measure("examined", usize::try_from(examined).unwrap_or(usize::MAX));
        let Some(message) = outcome? else {
            if detail.fail_on_nothing {
                return Err(stopped(
                    Sent::Answered,
                    codes::NOTHING_ARRIVED,
                    format!(
                        "No message that matches arrived on the channel `{name}` within the wait."
                    ),
                    "Try again when the message is expected, or raise the wait where the channel allows.",
                ));
            }
            // Only from a place the broker reported is everything up to
            // now covered. Elsewhere the wait saw what came after it
            // subscribed, and says so.
            let (coverage, limitation) = if placed {
                ("complete", Value::Null)
            } else {
                ("partial", Value::from("starts_at_subscription"))
            };
            return Ok(self.done(
                Values::new()
                    .with("arrived", false)
                    .with("waited_ms", i64::from(detail.wait_ms))
                    .with("examined", examined)
                    .with("coverage", coverage)
                    .with("limitation", limitation),
            ));
        };
        let record = match run.record(&entry, &message) {
            Ok(record) => record,
            Err(failed) => {
                if let (true, Some(delivery), Ok(link)) = (detail.take, &delivery_held, run.link())
                {
                    let _ = link
                        .release(&handle, delivery, Deadline::after(RELEASE_LIMIT))
                        .await;
                }
                return Err(failed);
            }
        };
        let record = self.redact.redact_values(&record);
        let mut observed = Observed::default();
        if let (true, Some(delivery)) = (detail.take, delivery_held) {
            // Record, then acknowledge. The engine stores what a step
            // gives for reconcile, durably, before it sends the step that
            // commits; so the whole message goes there, and the
            // acknowledgement is sent only for a message given out this
            // way. One too large to be recorded is given back.
            let for_reconcile = Values::new()
                .with(MESSAGE_ID, message.id.as_str())
                .with("message", record.clone());
            let size = CanonicalJson::encode(&for_reconcile)
                .map(|stored| stored.as_bytes().len())
                .ok();
            let limit = usize::try_from(entry.message_bytes).unwrap_or(usize::MAX);
            if size.is_none_or(|size| size > limit) {
                if let Ok(link) = run.link() {
                    let _ = link
                        .release(&handle, &delivery, Deadline::after(RELEASE_LIMIT))
                        .await;
                }
                return Err(stopped(
                    Sent::Answered,
                    codes::TAKE_TOO_LARGE,
                    format!(
                        "A message taken from the channel `{name}` is larger than the {} bytes that may be recorded before it is acknowledged. It was given back and not acknowledged.",
                        entry.message_bytes
                    ),
                    "Raise `limits.message_bytes` in the channel file, or have the message removed from the queue by hand.",
                ));
            }
            observed.assigned_key = RecordKey::new(vec![Value::from(message.id.as_str())]).ok();
            observed.for_reconcile = for_reconcile;
            // From here a record holds it, until its acknowledgement is
            // answered or the record is settled.
            self.mark(at(&message.id), Some(Mark::Held));
            run.held = Some(Held {
                channel: name,
                handle,
                delivery,
                message_id: message.id,
                address: entry.address.clone(),
                delivered_at,
                ack_wait,
            });
        }
        observed.values = Values::new()
            .with("arrived", true)
            .with("examined", examined)
            .with("message", record);
        Ok(StepOutcome::Done(observed))
    }

    async fn acknowledge(
        &self,
        run: &mut Run,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = run.declared(step)?;
        if !run.writes_allowed(step) {
            return Err(read_only("acknowledge a message"));
        }
        if entry.shape != Shape::Queue || entry.ownership != Ownership::RuntimeOwned {
            return Err(not_owned(&name));
        }
        let Some(held) = run.held.take_if(|held| held.channel == name) else {
            return Err(wrong_plan("an acknowledgement with no message taken"));
        };
        let at: MessageAt = (
            run_address(run),
            held.address.clone(),
            held.message_id.clone(),
        );
        // An acknowledgement that could reach the broker after its wait
        // ran out could remove a message the broker has handed to someone
        // else since. It is not sent; the record is in doubt, and its
        // reconcile looks at the queue.
        if held
            .ack_wait
            .is_some_and(|wait| held.delivered_at.elapsed() + ACK_WAIT_MARGIN >= wait)
        {
            return Err(stopped(
                Sent::Unknown,
                codes::NO_CONFIRMATION,
                format!(
                    "The message taken from the channel `{name}` was held longer than the broker waits for its acknowledgement, which was therefore not sent: the broker may have handed the message to someone else."
                ),
                "Reconcile the record: a look at the queue says whether the message is still there.",
            ));
        }
        let acknowledged = run
            .link()?
            .acknowledge(&held.handle, &held.delivery, input.deadline)
            .await;
        match acknowledged {
            Ok(()) => self.mark(at, None),
            Err(error) => {
                let failed = doubtful(&*self.redact, &error, "acknowledge the message");
                if failed.sent != Sent::Unknown {
                    // Answered as not carried out: no record holds it.
                    self.mark(at, Some(Mark::GivenBack));
                }
                return Err(failed);
            }
        }
        Ok(self.done(
            Values::new()
                .with("acknowledged", true)
                .with("message_id", held.message_id),
        ))
    }

    async fn publish(
        &self,
        run: &mut Run,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = run.declared(step)?;
        if !run.writes_allowed(step) {
            return Err(read_only("publish a message"));
        }
        let unsendable = |why: &str| {
            stopped(
                Sent::NotSent,
                codes::VALUE_OUT_OF_RANGE,
                format!("The message for the channel `{name}` cannot be built: {why}."),
                "Correct the value; nothing was sent.",
            )
        };
        let mut body = BTreeMap::new();
        let mut headers = BTreeMap::new();
        for (arg, value) in input.args.iter() {
            if let Some(field) = arg.strip_prefix(ARG_BODY) {
                if field == MESSAGE_META {
                    return Err(unsendable(
                        "`message_meta` is the name under which a result holds what the broker says about a message, and no body may use it",
                    ));
                }
                body.insert(
                    field.to_owned(),
                    Json::from_value(value).map_err(unsendable)?,
                );
            } else if let Some(header) = arg.strip_prefix(ARG_HEADER) {
                // A header the server acts on would change what the publish
                // does beyond what the channel's effect declares; the record's
                // token is placed below, never from an argument.
                if crate::lower::is_server_header(header)
                    || entry
                        .token_header
                        .as_deref()
                        .is_some_and(|token| token.eq_ignore_ascii_case(header))
                {
                    return Err(unsendable(
                        "a header of the binding is one the broker acts on, or the record's token header",
                    ));
                }
                let text = value
                    .as_text()
                    .ok_or_else(|| unsendable("a header is not text"))?;
                headers.insert(header.to_owned(), text.to_owned());
            }
        }
        let key = match input.args.get(ARG_KEY) {
            None => None,
            Some(value) => Some(
                value
                    .as_text()
                    .ok_or_else(|| unsendable("the key is not text"))?
                    .to_owned(),
            ),
        };
        if let (Some(header), Some(stamp)) = (&entry.token_header, input.stamp) {
            // The record's token, in the one header the channel declares.
            // No argument can name that header: the compiler refuses it.
            headers.insert(header.clone(), stamp.to_owned());
            run.mean("token", "sent");
        } else {
            run.mean("token", "none");
        }
        let body = Json::Object(body);
        if let Some(schema_name) = &entry.schema {
            let schema = run
                .schemas
                .get(schema_name)
                .ok_or_else(|| wrong_plan("a channel names a schema the plan does not hold"))?;
            schema.check(&body).map_err(|why| {
                stopped(
                    Sent::NotSent,
                    codes::SCHEMA_INVALID,
                    format!("The message for the channel `{name}` does not fit its schema: {why}."),
                    "Correct the input; nothing was sent.",
                )
            })?;
        }
        let size = body.to_line().len();
        run.measure("bytes", size);
        if size > usize::try_from(entry.message_bytes).unwrap_or(usize::MAX) {
            return Err(stopped(
                Sent::NotSent,
                codes::MESSAGE_TOO_LARGE,
                format!(
                    "The message for the channel `{name}` is {size} bytes, and the channel allows {}.",
                    entry.message_bytes
                ),
                "Shorten the message; nothing was sent.",
            ));
        }
        run.check_shape(&*self.redact, &name, &entry, input.deadline)
            .await?;
        let outgoing = Outgoing {
            address: entry.address.clone(),
            key,
            headers,
            body,
        };
        let receipt = run
            .link()?
            .publish(&outgoing, input.deadline)
            .await
            .map_err(|error| doubtful(&*self.redact, &error, "publish the message"))?;
        let values = Values::new()
            .with("message_id", receipt.id.as_str())
            .with("position", position_value(receipt.position))
            .with("duplicate", receipt.duplicate);
        if let Some(stamp) = input.stamp {
            run.receipts.insert(stamp.to_owned(), receipt);
        }
        Ok(self.done(values))
    }

    /// Where a log ends now, read before a publish to it and given for
    /// reconcile, so that a scan of a resumed run starts there. Takes
    /// nothing.
    async fn position(
        &self,
        run: &mut Run,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = run.declared(step)?;
        if entry.shape != Shape::Log {
            return Err(wrong_plan("a position read on a channel that is not a log"));
        }
        let facts = run
            .described(&*self.redact, &name, &entry, input.deadline)
            .await?;
        let Some(end) = facts.end else {
            return Err(stopped(
                Sent::Answered,
                codes::UNEXPECTED_REPLY,
                format!("The broker did not say where the log `{name}` ends."),
                "Check that the surface's `protocol` is the one the broker speaks; then report this.",
            ));
        };
        Ok(StepOutcome::Done(Observed {
            for_reconcile: Values::new().with(SCAN_FROM, position_value(Some(end))),
            ..Observed::default()
        }))
    }

    /// Whether the message a take recorded before it acknowledged is gone
    /// from its queue, which settles a take whose acknowledgement's answer
    /// was lost: gone, the acknowledgement landed and the record is found;
    /// still held, nothing was taken and the record is absent. Only a queue
    /// the runtime owns is looked at, so nobody else removes a message from
    /// it. Takes nothing. "Gone" is said only of a queue the look covered.
    async fn taken(
        &self,
        run: &mut Run,
        step: &Step,
        input: &StepInput<'_>,
        recorded: &Values,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = run.declared(step)?;
        if entry.shape != Shape::Queue || entry.ownership != Ownership::RuntimeOwned {
            return Err(not_owned(&name));
        }
        let Some(id) = recorded.get(MESSAGE_ID).and_then(Value::as_text) else {
            return Err(nothing_recorded(&format!(
                "the record holds no identifier of a message taken from the queue `{name}`"
            )));
        };
        // Where the broker gives a message a position, as a stream does,
        // the one place is looked at; else the whole queue.
        let position = match recorded
            .get("message")
            .and_then(Value::as_record)
            .and_then(|message| message.get(MESSAGE_META))
            .and_then(Value::as_record)
            .and_then(|meta| meta.get("position"))
        {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_int()
                    .and_then(|position| u64::try_from(position).ok())
                    .ok_or_else(|| {
                        nothing_recorded("the recorded message's position is not a whole number")
                    })?,
            ),
        };
        let facts = run
            .described(&*self.redact, &name, &entry, input.deadline)
            .await?;
        let held = if let Some(at) = position {
            run.mean("look", "by_position");
            if facts.start.is_some_and(|start| at < start) {
                false
            } else if facts.end.is_none_or(|end| at >= end) {
                return Err(stopped(
                    Sent::Answered,
                    codes::UNEXPECTED_REPLY,
                    format!(
                        "The queue `{name}` does not reach the position of the message the record took."
                    ),
                    "Look at the queue by hand before anything is sent again; then report this.",
                ));
            } else {
                let page = run
                    .link()?
                    .browse(&entry.address, Some(at), 1, input.deadline)
                    .await
                    .map_err(|error| wire_failure(&*self.redact, &error, "look at the queue"))?;
                run.measure("messages", page.messages.len());
                page.messages
                    .first()
                    .is_some_and(|message| message.id == id)
            }
        } else {
            run.mean("look", "whole_queue");
            let page = run
                .link()?
                .browse(&entry.address, None, entry.browse_count, input.deadline)
                .await
                .map_err(|error| wire_failure(&*self.redact, &error, "look at the queue"))?;
            run.measure("messages", page.messages.len());
            if page.messages.iter().any(|message| message.id == id) {
                true
            } else if page.more {
                return Err(stopped(
                    Sent::Answered,
                    codes::SCAN_LIMIT,
                    format!(
                        "The queue `{name}` holds more than one look may read, so the driver cannot say whether the message the record took is gone."
                    ),
                    "Look at the queue by hand before anything is sent again, or raise `limits.browse_count`.",
                ));
            } else if u64::try_from(page.messages.len()).unwrap_or(u64::MAX) < facts.held {
                // Nobody else takes from a queue the runtime owns: a look
                // that shows fewer messages than the queue holds did not
                // cover it, and "gone" is never said of what was not seen.
                return Err(stopped(
                    Sent::Answered,
                    codes::UNEXPECTED_REPLY,
                    format!(
                        "A look at the queue `{name}` showed fewer messages than the broker says it holds, so the driver cannot say whether the message the record took is gone."
                    ),
                    "Look at the queue by hand before anything is sent again; then report this.",
                ));
            } else {
                false
            }
        };
        let at: MessageAt = (run_address(run), entry.address.clone(), id.to_owned());
        if held {
            // Settled as not taken: no record holds it any more.
            self.mark(at, Some(Mark::GivenBack));
            return Ok(StepOutcome::Absent(Observed::default()));
        }
        self.mark(at, None);
        Ok(self.done(
            Values::new()
                .with("acknowledged", true)
                .with(MESSAGE_ID, id),
        ))
    }

    /// What the broker lists, for a discovery.
    async fn channels(
        &self,
        run: &mut Run,
        detail: &StepDetail,
        input: &StepInput<'_>,
        one: bool,
    ) -> Result<StepOutcome, StepFailure> {
        if run.posture == Posture::Live {
            return Err(wrong_plan("a step of a discovery in a live run"));
        }
        if one {
            let address = discovery_address(run, detail)?;
            let facts = run
                .link()?
                .describe(&address, input.deadline)
                .await
                .map_err(|error| wire_failure(&*self.redact, &error, "describe the channel"))?;
            return Ok(self.done(facts_values(&facts)));
        }
        let listed = run
            .link()?
            .channels(input.deadline)
            .await
            .map_err(|error| wire_failure(&*self.redact, &error, "list the broker's channels"))?;
        let channels: Vec<Value> = listed
            .iter()
            .map(|facts| Value::Record(facts_values(facts)))
            .collect();
        run.measure("channels", channels.len());
        Ok(self.done(Values::new().with("channels", channels)))
    }
}

/// The broker's address a run reaches.
fn run_address(run: &Run) -> String {
    run.key.0.clone()
}

/// A scan that cannot say "absent": the stream may lose a message from
/// inside its range, the range read had a gap, or nothing recorded where
/// it began. The record stays in doubt with what it recorded.
fn inconclusive(name: &str) -> StepFailure {
    stopped(
        Sent::Answered,
        codes::SCAN_INCONCLUSIVE,
        format!(
            "A look at the log `{name}` cannot say whether the record's message is there: the stream can lose a message from inside its range (a limit per key, a time to live per message, or a retention that removes what is read), a message is missing from inside the range read, or nothing recorded where the range begins."
        ),
        "Look at the log by hand before anything is sent again: the record stays in doubt.",
    )
}

/// A look at a log that does not say how far it reached. The scan cannot
/// say the record's message is absent, and it does not guess.
fn uncovered(name: &str) -> StepFailure {
    stopped(
        Sent::Answered,
        codes::UNEXPECTED_REPLY,
        format!(
            "A look at the log `{name}` did not say how far it reached, so the driver cannot say that the record's message is not there."
        ),
        "Look at the log by hand before anything is sent again; then report this.",
    )
}

/// A step that needs what an earlier step of its record recorded, and was
/// given nothing of the form it needs. Nothing is sent: a look that has
/// nothing to look for cannot settle the record.
fn nothing_recorded(what: &str) -> StepFailure {
    stopped(
        Sent::NotSent,
        codes::NOTHING_RECORDED,
        format!("The step cannot settle its record: {what}."),
        "Settle the record by hand, with what the run's ledger holds for it.",
    )
}

fn not_owned(name: &str) -> StepFailure {
    stopped(
        Sent::NotSent,
        codes::ACK_NOT_OWNED,
        format!(
            "The channel `{name}` is not a queue the target declares the runtime owns, so nothing on it is taken or acknowledged."
        ),
        "Look at the queue with `list`; declare `ownership: runtime_owned` only when nothing else reads it.",
    )
}

/// The channel a discovery's step names, by the broker's own name for it.
/// Honoured only where nothing may be written: in a live run a channel
/// comes from the target's files and from nowhere else.
fn discovery_address(run: &Run, detail: &StepDetail) -> Result<String, StepFailure> {
    match (&detail.address, run.posture) {
        (Some(address), Posture::Discovery | Posture::Sandbox { .. }) => Ok(address.clone()),
        _ => Err(wrong_plan(
            "a step names a channel outside the target's files",
        )),
    }
}

/// The value a message carries in its channel's correlation field.
fn correlation_of(entry: &ChannelEntry, message: &Message) -> Option<String> {
    let field = entry.correlation.as_deref()?;
    match field.strip_prefix("headers.") {
        Some(header) => message.headers.get(header).cloned(),
        None => message
            .body
            .get(field)
            .and_then(Json::as_str)
            .map(str::to_owned),
    }
}

fn facts_values(facts: &ChannelFacts) -> Values {
    let text = |value: &Option<String>| value.as_deref().map_or(Value::Null, Value::from);
    let subscriptions: Vec<Value> = facts
        .subscriptions
        .iter()
        .map(|name| Value::from(name.as_str()))
        .collect();
    Values::new()
        .with("address", facts.address.as_str())
        .with(
            "shape",
            facts
                .shape
                .map_or(Value::Null, |shape| Value::from(shape.as_str())),
        )
        .with("key_field", text(&facts.key_field))
        .with("dedupe_header", text(&facts.dedupe_header))
        .with(
            "dedupe_window_ms",
            facts.dedupe_window_ms.map_or(Value::Null, |ms| {
                Value::Int(i64::try_from(ms).unwrap_or(i64::MAX))
            }),
        )
        .with(
            "schema",
            facts
                .schema
                .as_ref()
                .map_or(Value::Null, |schema| Value::from(schema.to_line())),
        )
        .with("held", i64::try_from(facts.held).unwrap_or(i64::MAX))
        .with("start", position_value(facts.start))
        .with("end", position_value(facts.end))
        .with("subscriptions", subscriptions)
}

impl Driver for StreamDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move {
            let surface = &cx.surface.surface;
            let Some(protocol) = self.protocol_of(cx.surface) else {
                return Err(stopped(
                    Sent::NotSent,
                    codes::PROTOCOL_UNKNOWN,
                    format!(
                        "The surface `{}` names a wire protocol this build does not speak.",
                        surface.name
                    ),
                    "Use a protocol the stream pack lists, or a build that speaks this one.",
                ));
            };
            if !cx.destinations.contains(&surface.address) {
                return Err(stopped(
                    Sent::NotSent,
                    codes::SURFACE_NOT_DECLARED,
                    format!(
                        "The address of the surface `{}` is not one this run may reach.",
                        surface.name
                    ),
                    "Declare the broker's address on a surface of the target, for this environment.",
                ));
            }
            let mut channels = BTreeMap::new();
            let mut schemas = BTreeMap::new();
            for section in &cx.plan.packs {
                if section.pack != PACK || section.surface_kind != BROKER {
                    continue;
                }
                if section.format != SECTION_FORMAT {
                    return Err(wrong_plan("a section of another format"));
                }
                if section.kind == CHANNEL {
                    let all: BTreeMap<String, ChannelEntry> =
                        section.payload.decode().map_err(StepFailure::not_sent)?;
                    channels.extend(
                        all.into_iter()
                            .filter(|(_, entry)| entry.surface == surface.name.as_str()),
                    );
                } else if section.kind == MESSAGE_SCHEMA {
                    schemas = section.payload.decode().map_err(StepFailure::not_sent)?;
                }
            }
            let mut sink = CredentialPlaced::default();
            let session = match lease {
                Some(lease) => {
                    lease.apply(&mut sink).map_err(StepFailure::not_sent)?;
                    Some((lease.session().to_string(), lease.epoch()))
                }
                None => None,
            };
            let credential = sink
                .credential()
                .ok()
                .filter(|credential| credential.as_ref().is_none_or(|c| protocol.carries(c)));
            let Some(credential) = credential else {
                // A connection opened without part of the session's
                // credentials would not be the session's connection.
                return Err(stopped(
                    Sent::NotSent,
                    codes::CREDENTIAL_NOT_CARRIED,
                    format!(
                        "The session of the surface `{}` holds a credential the wire protocol `{}` cannot present.",
                        surface.name,
                        protocol.name()
                    ),
                    "Sign in to the target with a provider that gives one token, or use a protocol that carries this credential.",
                ));
            };
            let key: PoolKey = (surface.address.clone(), protocol.name().to_owned(), session);
            let link = match self.take_idle(&key) {
                Some(link) => link,
                None => protocol
                    .connect(&surface.address, credential.as_ref(), cx.deadline)
                    .await
                    .map_err(|error| wire_failure(&*self.redact, &error, "open a connection"))?,
            };
            Ok(Prepared::new(Run {
                link: Some(link),
                key,
                posture: cx.posture,
                channels,
                schemas,
                checked: BTreeSet::new(),
                open: BTreeMap::new(),
                held: None,
                seen: BTreeSet::new(),
                receipts: BTreeMap::new(),
                failed: false,
                protocol: protocol.name(),
                acting: Acting::default(),
                holds: Vec::new(),
            }))
        })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            // What this record's earlier steps recorded, read before the
            // driver's own state is borrowed.
            let recorded = prepared.recorded().clone();
            let run = run_of(prepared)?;
            run.acting = Acting {
                means: vec![Detail::new("protocol", run.protocol)],
                measured: Vec::new(),
            };
            let detail: StepDetail = if step.payload == PackPayload::none() {
                StepDetail::default()
            } else {
                step.payload.decode().map_err(StepFailure::not_sent)?
            };
            let result = if step.kind == STEP_LATEST {
                self.latest(run, step, input).await
            } else if step.kind == STEP_BROWSE {
                self.browse(run, step, &detail, input).await
            } else if step.kind == STEP_SCAN {
                self.scan(run, step, input, &recorded).await
            } else if step.kind == STEP_POSITION {
                self.position(run, step, input).await
            } else if step.kind == STEP_TAKEN {
                self.taken(run, step, input, &recorded).await
            } else if step.kind == STEP_SUBSCRIBE {
                self.subscribe(run, step, &detail, input).await
            } else if step.kind == STEP_AWAIT {
                self.wait(prepared, step, &detail, input).await
            } else if step.kind == STEP_PUBLISH {
                self.publish(run, step, input).await
            } else if step.kind == STEP_ACKNOWLEDGE {
                self.acknowledge(run, step, input).await
            } else if step.kind == STEP_CHANNELS {
                self.channels(run, &detail, input, false).await
            } else if step.kind == STEP_DESCRIBE {
                self.channels(run, &detail, input, true).await
            } else {
                Err(wrong_plan("a step of a kind the driver does not carry out"))
            };
            let run = run_of(prepared)?;
            if let Err(failed) = &result {
                // After any failure the connection is not kept for the
                // session's next run.
                run.failed = true;
                if failed.sent == Sent::Unknown {
                    // The connection cannot be trusted to pair answers with
                    // requests any more. Dropping it also gives back
                    // whatever the run held unacknowledged.
                    run.link = None;
                }
            }
            // How the step was done goes to its trace record.
            let acting = Capture::acting(&std::mem::take(&mut run.acting));
            result.map(|outcome| match outcome {
                StepOutcome::Done(mut observed) => {
                    observed.captures.push(acting);
                    StepOutcome::Done(observed)
                }
                StepOutcome::Absent(mut observed) => {
                    observed.captures.push(acting);
                    StepOutcome::Absent(observed)
                }
            })
        })
    }

    fn health(&self, surface: &SurfaceRef) -> DriverHealth {
        if self.protocol_of(surface).is_some() {
            DriverHealth::Ready
        } else {
            DriverHealth::Unavailable(codes::PROTOCOL_UNKNOWN)
        }
    }

    fn release<'a>(&'a self, prepared: Prepared, poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            let Ok(run) = prepared.into_state::<Run>() else {
                return Cleanup::complete();
            };
            let Run {
                link,
                key,
                open,
                failed,
                held,
                holds,
                ..
            } = *run;
            // A message taken and never acknowledged: the acknowledgement
            // was not sent, so no record holds it. It goes back with the
            // subscription, or with the connection.
            if let Some(held) = held {
                self.mark(
                    (key.0.clone(), held.address, held.message_id),
                    Some(Mark::GivenBack),
                );
            }
            // Held until the subscriptions are let go.
            let _holds = holds;
            // A connection that is dropped takes its subscriptions with it,
            // and the broker takes back what they held.
            let Some(mut link) = link.filter(|_| !poisoned && !failed) else {
                return Cleanup::complete();
            };
            for subscription in open.values() {
                if link
                    .unsubscribe(&subscription.handle, Deadline::after(RELEASE_LIMIT))
                    .await
                    .is_err()
                {
                    return Cleanup::complete();
                }
            }
            if link.alive() {
                self.keep_idle(key, link);
            }
            Cleanup::complete()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_the_protocol_cannot_present_is_remembered_and_never_dropped_silently() {
        let secret = Secret::new("secret-for-tests-one");
        let mut token = CredentialPlaced::default();
        token.header("authorization", &Secret::new("Bearer secret-for-tests-one"));
        assert!(
            matches!(token.credential(), Ok(Some(Credential::Token(t))) if t.expose() == "secret-for-tests-one")
        );

        // `example-user:secret-for-tests-one`, as the neutral basic holder places it.
        let mut basic = CredentialPlaced::default();
        basic.header(
            "Authorization",
            &Secret::new("Basic ZXhhbXBsZS11c2VyOnNlY3JldC1mb3ItdGVzdHMtb25l"),
        );
        assert!(matches!(
            basic.credential(),
            Ok(Some(Credential::UserPassword { user, password }))
                if user == "example-user" && password.expose() == "secret-for-tests-one"
        ));
        {
            assert!(
                crate::example_wire::ExampleProtocol.carries(&Credential::Token(Secret::new("x")))
            );
            assert!(
                !crate::example_wire::ExampleProtocol.carries(&Credential::UserPassword {
                    user: "example-user".to_owned(),
                    password: Secret::new("x"),
                })
            );
        }

        let mut second = CredentialPlaced::default();
        second.header("authorization", &secret);
        second.header("x-other", &secret);
        assert!(second.credential().is_err());

        let mut password = CredentialPlaced::default();
        password.challenge_credential(ChallengeScheme::Digest, "example-user", &secret);
        assert!(password.credential().is_err());

        let mut identity = CredentialPlaced::default();
        identity.tls_identity(b"certificate", &secret);
        assert!(identity.credential().is_err());

        let mut cookie = CredentialPlaced::default();
        cookie.cookie("broker.example.test", "session", &secret);
        assert!(cookie.credential().is_err());
    }
}
