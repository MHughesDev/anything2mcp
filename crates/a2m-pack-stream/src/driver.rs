//! The driver of the `stream` mode (spec section 309, "The driver, part by
//! part"). It carries out one step at a time over a [`Link`], retries
//! nothing, and says of every failure whether the request left.
//!
//! What it never does: choose a channel from a step argument, keep a
//! subscription open after its run, acknowledge on a queue the target does
//! not declare the runtime owns, renew a session, or put what a broker
//! said into a result before the redactor has seen it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use a2m_auth::{ChallengeScheme, CredentialSink, SessionLease};
use a2m_core::{
    Failure, FailureClass, FailureCode, PackPayload, RecordKey, Redact, Secret, Step, Value, Values,
};
use a2m_provider::{
    BoxFuture, Cleanup, Deadline, Driver, DriverHealth, Observed, Posture, PrepareContext,
    Prepared, Sent, StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use serde::Deserialize;

use crate::channel::{ChannelEntry, MessageSchema, Ownership, SECTION_FORMAT, Shape};
use crate::codes::{self, shown, stopped};
use crate::json::Json;
use crate::lower::{ARG_BODY, ARG_CORRELATION, ARG_FROM, ARG_HEADER, ARG_KEY, StepDetail};
use crate::names::{
    BROKER, CHANNEL, MESSAGE_SCHEMA, PACK, STEP_ACKNOWLEDGE, STEP_AWAIT, STEP_BROWSE,
    STEP_CHANNELS, STEP_DESCRIBE, STEP_LATEST, STEP_PUBLISH, STEP_SCAN, STEP_SUBSCRIBE,
};
use crate::wire::{
    ChannelFacts, Link, Message, Outgoing, Protocol, Receipt, WireError, WireErrorKind,
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

/// What a surface of kind `broker` states beside its address.
#[derive(Debug, Deserialize)]
struct SurfaceDetail {
    protocol: String,
}

/// Which idle connections a run may take: those to the same address, over
/// the same protocol, opened with the same session's credentials.
type PoolKey = (String, String, Option<(String, u64)>);

/// Where the session's token is put: into the one `hello` that opens a
/// connection. It is dropped, and so wiped, when that is sent.
#[derive(Default)]
struct TokenSink(Option<Secret>);

impl CredentialSink for TokenSink {
    fn header(&mut self, _name: &str, value: &Secret) {
        if self.0.is_none() {
            self.0 = Some(Secret::new(value.expose()));
        }
    }

    fn cookie(&mut self, _origin: &str, _name: &str, _value: &Secret) {}

    fn tls_identity(&mut self, _certificate: &[u8], _key: &Secret) {}

    fn challenge_credential(&mut self, _scheme: ChallengeScheme, _user: &str, _secret: &Secret) {}
}

/// A subscription the run opened.
struct Open {
    handle: String,
}

/// A message delivered to the run and not yet acknowledged.
struct Held {
    channel: String,
    handle: String,
    delivery: String,
    message_id: String,
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
}

/// The driver.
pub(crate) struct StreamDriver {
    redact: Arc<dyn Redact>,
    protocols: Vec<Arc<dyn Protocol>>,
    idle: Mutex<BTreeMap<PoolKey, Vec<Box<dyn Link>>>>,
    idle_limit: usize,
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

/// The failure of a call on the wire, as the failure table of section 309
/// names it. `what` says what was being done, for the sentence.
fn wire_failure(error: &WireError, what: &str) -> StepFailure {
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
            format!("The broker refused the message: {}", shown(&error.detail)),
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
        WireErrorKind::Unexpected => (
            codes::UNEXPECTED_REPLY,
            format!(
                "The broker answered in a way the driver has no rule for while it tried to {what}: {}.",
                shown(&error.detail)
            ),
            "Check that the surface's `protocol` is the one the broker speaks; then report this.",
        ),
    };
    stopped(error.sent, code, message, next)
}

/// A doubtful write: sent, and nothing says whether it happened.
fn doubtful(error: &WireError, what: &str) -> StepFailure {
    if error.sent == Sent::Unknown {
        stopped(
            Sent::Unknown,
            codes::NO_CONFIRMATION,
            format!("The request to {what} was sent and the broker's confirmation did not arrive."),
            "Reconcile the record before anything is sent again: the message may have been stored.",
        )
    } else {
        wire_failure(error, what)
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

    fn writes_allowed(&self) -> bool {
        matches!(
            self.posture,
            Posture::Live | Posture::Sandbox { test_records: true }
        )
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

    /// Compares, once per run, the channel's declared shape and key with
    /// what the broker reports.
    async fn check_shape(
        &mut self,
        name: &str,
        entry: &ChannelEntry,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        if self.checked.contains(name) {
            return Ok(());
        }
        let facts = self
            .link()?
            .describe(&entry.address, deadline)
            .await
            .map_err(|error| wire_failure(&error, "describe the channel"))?;
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
        self.checked.insert(name.to_owned());
        Ok(())
    }

    /// Checks a message against the channel's schema, and gives it as the
    /// values a caller sees: the body's fields, and `message_meta` with
    /// what the broker says about the message.
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
        if !values.contains("message_meta") {
            let headers: Values = message
                .headers
                .iter()
                .map(|(name, value)| (name.clone(), Value::from(value.as_str())))
                .collect();
            values.insert(
                "message_meta",
                Values::new()
                    .with("id", message.id.as_str())
                    .with(
                        "key",
                        message.key.as_deref().map_or(Value::Null, Value::from),
                    )
                    .with("position", position_value(message.position))
                    .with("headers", headers),
            );
        }
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
        run.check_shape(&name, &entry, input.deadline).await?;
        let found = run
            .link()?
            .latest(&entry.address, key, input.deadline)
            .await
            .map_err(|error| wire_failure(&error, "read the latest value"))?;
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
        run.check_shape(&name, &entry, input.deadline).await?;
        let page = run
            .link()?
            .browse(&entry.address, from, entry.browse_count, input.deadline)
            .await
            .map_err(|error| wire_failure(&error, "look at the channel"))?;
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
        let count = i64::try_from(messages.len()).unwrap_or(i64::MAX);
        Ok(self.done(
            Values::new()
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
            .map_err(|error| wire_failure(&error, "look at the channel"))?;
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
        run.check_shape(&name, &entry, input.deadline).await?;
        // Where this run's own publish was confirmed, read that one place.
        let mut from = run.receipts.get(stamp).and_then(|receipt| receipt.position);
        let known = from.is_some();
        for _ in 0..SCAN_PAGES {
            let max = if known { 1 } else { entry.browse_count };
            let page = run
                .link()?
                .browse(&entry.address, from, max, input.deadline)
                .await
                .map_err(|error| wire_failure(&error, "scan the log"))?;
            if let Some(found) = page
                .messages
                .iter()
                .find(|message| message.headers.get(header).map(String::as_str) == Some(stamp))
            {
                return Ok(self.done(run.record(&entry, found)?));
            }
            let next = page
                .messages
                .last()
                .and_then(|last| last.position)
                .map(|position| position + 1);
            if known || !page.more || next.is_none() {
                return Ok(StepOutcome::Absent(Observed::default()));
            }
            from = next;
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
        if run.posture == Posture::Discovery || (detail.take && !run.writes_allowed()) {
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
        run.check_shape(&name, &entry, input.deadline).await?;
        let subscription = if detail.take {
            None
        } else {
            entry.subscription.as_deref()
        };
        let opened = run
            .link()?
            .subscribe(&entry.address, subscription, from, input.deadline)
            .await
            .map_err(|error| wire_failure(&error, "open the runtime's subscription"))?;
        let position = position_value(opened.position);
        run.open.insert(
            name,
            Open {
                handle: opened.handle,
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
        run: &mut Run,
        step: &Step,
        detail: &StepDetail,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = run.declared(step)?;
        let Some(handle) = run.open.get(&name).map(|open| open.handle.clone()) else {
            return Err(wrong_plan("a wait before its subscription was opened"));
        };
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
        let mut passed_over: Vec<String> = Vec::new();
        let mut outcome: Result<Option<Message>, StepFailure> = Ok(None);
        let mut delivery_held = None;
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
                    .map_err(|error| wire_failure(&error, "wait for a message")),
                Err(failed) => Err(failed),
            };
            let delivered = match fetched {
                Ok(delivered) => delivered,
                Err(failed) => {
                    outcome = Err(failed);
                    break;
                }
            };
            let Some(delivered) = delivered.into_iter().next() else {
                continue;
            };
            examined += 1;
            if !run.seen.insert(delivered.message.id.clone()) {
                // The same message again: at least once is not exactly once.
                continue;
            }
            let message = delivered.message;
            let matches = key.is_none_or(|key| message.key.as_deref() == Some(key))
                && correlation.is_none_or(|wanted| {
                    correlation_of(&entry, &message).as_deref() == Some(wanted)
                });
            if !matches {
                if detail.take {
                    passed_over.push(delivered.delivery);
                }
                continue;
            }
            delivery_held = Some(delivered.delivery);
            outcome = Ok(Some(message));
            break;
        }
        for delivery in passed_over {
            if let Ok(link) = run.link() {
                // Best effort: what is not given back here goes back when
                // the subscription is let go.
                let _ = link
                    .release(&handle, &delivery, Deadline::after(RELEASE_LIMIT))
                    .await;
            }
        }
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
            return Ok(self.done(
                Values::new()
                    .with("arrived", false)
                    .with("waited_ms", i64::from(detail.wait_ms))
                    .with("examined", examined)
                    .with("coverage", "complete"),
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
        let values = Values::new()
            .with("arrived", true)
            .with("examined", examined)
            .with("message", record);
        let mut observed = Observed {
            values: self.redact.redact_values(&values),
            ..Observed::default()
        };
        if let (true, Some(delivery)) = (detail.take, delivery_held) {
            // Record, then acknowledge: the engine stores these before the
            // acknowledging step is sent.
            observed.assigned_key = RecordKey::new(vec![Value::from(message.id.as_str())]).ok();
            observed.for_reconcile = Values::new().with("message_id", message.id.as_str());
            run.held = Some(Held {
                channel: name,
                handle,
                delivery,
                message_id: message.id,
            });
        }
        Ok(StepOutcome::Done(observed))
    }

    async fn acknowledge(
        &self,
        run: &mut Run,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let (name, entry) = run.declared(step)?;
        if !run.writes_allowed() {
            return Err(read_only("acknowledge a message"));
        }
        if entry.shape != Shape::Queue || entry.ownership != Ownership::RuntimeOwned {
            return Err(not_owned(&name));
        }
        let Some(held) = run.held.take_if(|held| held.channel == name) else {
            return Err(wrong_plan("an acknowledgement with no message taken"));
        };
        run.link()?
            .acknowledge(&held.handle, &held.delivery, input.deadline)
            .await
            .map_err(|error| doubtful(&error, "acknowledge the message"))?;
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
        if !run.writes_allowed() {
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
                body.insert(
                    field.to_owned(),
                    Json::from_value(value).map_err(unsendable)?,
                );
            } else if let Some(header) = arg.strip_prefix(ARG_HEADER) {
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
        run.check_shape(&name, &entry, input.deadline).await?;
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
            .map_err(|error| doubtful(&error, "publish the message"))?;
        let values = Values::new()
            .with("message_id", receipt.id.as_str())
            .with("position", position_value(receipt.position))
            .with("duplicate", receipt.duplicate);
        if let Some(stamp) = input.stamp {
            run.receipts.insert(stamp.to_owned(), receipt);
        }
        Ok(self.done(values))
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
                .map_err(|error| wire_failure(&error, "describe the channel"))?;
            return Ok(self.done(facts_values(&facts)));
        }
        let listed = run
            .link()?
            .channels(input.deadline)
            .await
            .map_err(|error| wire_failure(&error, "list the broker's channels"))?;
        let channels: Vec<Value> = listed
            .iter()
            .map(|facts| Value::Record(facts_values(facts)))
            .collect();
        Ok(self.done(Values::new().with("channels", channels)))
    }
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
            let mut sink = TokenSink::default();
            let session = match lease {
                Some(lease) => {
                    lease.apply(&mut sink).map_err(StepFailure::not_sent)?;
                    Some((lease.session().to_string(), lease.epoch()))
                }
                None => None,
            };
            let key: PoolKey = (surface.address.clone(), protocol.name().to_owned(), session);
            let link = match self.take_idle(&key) {
                Some(link) => link,
                None => protocol
                    .connect(&surface.address, sink.0.as_ref(), cx.deadline)
                    .await
                    .map_err(|error| wire_failure(&error, "open a connection"))?,
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
            let Some(run) = prepared.state::<Run>() else {
                return Err(wrong_plan("the run was prepared by another driver"));
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
                self.scan(run, step, input).await
            } else if step.kind == STEP_SUBSCRIBE {
                self.subscribe(run, step, &detail, input).await
            } else if step.kind == STEP_AWAIT {
                self.wait(run, step, &detail, input).await
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
            if let Err(failed) = &result
                && failed.sent == Sent::Unknown
            {
                // The connection cannot be trusted to pair answers with
                // requests any more. Dropping it also gives back whatever
                // the run held unacknowledged.
                run.link = None;
            }
            result
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
                link, key, open, ..
            } = *run;
            // A connection that is dropped takes its subscriptions with it,
            // and the broker takes back what they held.
            let Some(mut link) = link.filter(|_| !poisoned) else {
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
