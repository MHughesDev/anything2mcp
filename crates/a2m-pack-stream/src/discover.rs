//! Discovery of a broker (spec section 309, "Discovery and drafting"): what
//! it lists, the schemas it holds, and the shape of the messages a look
//! shows. Every finding comes from a step that takes nothing: `channels`,
//! `describe`, and a bounded `browse`. The discoverer never subscribes, so
//! it cannot consume, whatever a channel file declares.

use std::collections::BTreeMap;
use std::sync::Arc;

use a2m_core::{
    Address, Coverage, EvidenceLevel, FailsAs, Failure, FailureClass, Finding, LimitationCode,
    ProducedBy, Source, Step, StepKind, Subject, UnknownReason, Value, Values,
};
use a2m_provider::{
    BoxFuture, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, FindingSink, SandboxPort, StepInput, StepOutcome, SurfaceDescription,
};
use a2m_telemetry::Clock;

use crate::channel::{JsonType, MessageSchema};
use crate::codes;
use crate::json::Json;
use crate::lower::StepDetail;
use crate::names::{BROKER, Names, PACK, STEP_BROWSE, STEP_CHANNELS};

/// How long one step of a discovery may take, in milliseconds.
const STEP_TIMEOUT_MS: u32 = 10_000;

/// The shapes a look that takes nothing can be asked of.
const LOOKABLE: &[&str] = &["retained", "log", "queue"];

/// The discoverer `broker_look`.
pub(crate) struct BrokerLook {
    names: Names,
    sample: u32,
    version: String,
    clock: Arc<dyn Clock>,
}

impl BrokerLook {
    pub(crate) fn new(names: Names, sample: u32, version: String, clock: Arc<dyn Clock>) -> Self {
        Self {
            names,
            sample,
            version,
            clock,
        }
    }

    fn step(kind: StepKind, detail: &StepDetail) -> Step {
        Step {
            kind,
            refers_to: None,
            args: Vec::new(),
            timeout_ms: STEP_TIMEOUT_MS,
            fails_as: FailsAs {
                class: FailureClass::UnknownVariant,
                code: codes::UNEXPECTED_REPLY,
            },
            commits: false,
            lookup: false,
            idempotent: false,
            when: None,
            payload: detail.encode(),
        }
    }

    fn produced_by(&self) -> ProducedBy {
        ProducedBy {
            provider: PACK.to_owned(),
            version: self.version.clone(),
            component: None,
        }
    }
}

/// The subject of a finding about one channel: the broker's address and
/// the channel's own name, where the two make a plain address.
fn subject(broker: &str, channel: &str) -> Subject {
    Address::new(&format!("broker://{broker}"), &format!("/{channel}")).map_or(
        Subject::Unknown {
            reason: UnknownReason::NotSupported,
        },
        Subject::Address,
    )
}

fn text_json(value: Option<&Value>) -> Json {
    value
        .and_then(Value::as_text)
        .map_or(Json::Null, Json::text)
}

impl Discoverer for BrokerLook {
    fn name(&self) -> DiscovererName {
        self.names.discoverer.clone()
    }

    fn suits(&self, surface: &SurfaceDescription) -> bool {
        surface.kind == BROKER
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one pass over the broker's list; each channel gives up to three findings in a fixed order"
    )]
    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            let broker = scope.surface.surface.address.as_str();
            let no_args = Values::new();
            let input = StepInput::new(&no_args, scope.deadline);
            let listed = port
                .act(&Self::step(STEP_CHANNELS, &StepDetail::default()), &input)
                .await
                .map_err(|failed| failed.failure)?;
            let (StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = listed;
            let channels = observed
                .values
                .get("channels")
                .and_then(Value::as_list)
                .unwrap_or_default();

            let now = self.clock.now();
            let mut report = DiscoveryReport {
                coverage: Coverage::Complete,
                ..DiscoveryReport::default()
            };
            let mut bytes = 0_u64;
            let stop = |report: &mut DiscoveryReport, bound: &str, left: usize| {
                report.coverage = Coverage::Partial;
                report.bounds_hit.push(bound.to_owned());
                report.dropped += u64::try_from(left).unwrap_or(u64::MAX);
            };
            for (at, channel) in channels.iter().enumerate() {
                let left = channels.len() - at;
                if scope.deadline.passed() {
                    stop(&mut report, "seconds", left);
                    break;
                }
                if bytes > scope.bounds.bytes {
                    stop(&mut report, "bytes", left);
                    break;
                }
                let Some(facts) = channel.as_record() else {
                    report.dropped += 1;
                    continue;
                };
                let Some(address) = facts.get("address").and_then(Value::as_text) else {
                    report.dropped += 1;
                    continue;
                };
                let shape = facts.get("shape").and_then(Value::as_text);

                // The look, where the shape allows one. A broker that says
                // its channel cannot be looked at is an answer: no sample.
                let mut look = "none";
                let mut shape_finding = None;
                // The fields the look saw, each with the one plain JSON type
                // every message gave it: what a drafted list returns where
                // the broker holds no schema the pack reads.
                let mut sampled_fields = Json::Null;
                if self.sample > 0 && shape.is_some_and(|shape| LOOKABLE.contains(&shape)) {
                    let detail = StepDetail {
                        address: Some(address.to_owned()),
                        sample: self.sample,
                        ..StepDetail::default()
                    };
                    match port.act(&Self::step(STEP_BROWSE, &detail), &input).await {
                        Ok(StepOutcome::Done(sample) | StepOutcome::Absent(sample)) => {
                            look = "browse";
                            let fields: BTreeMap<String, Json> = sample
                                .values
                                .get("fields")
                                .and_then(Value::as_record)
                                .map(|fields| {
                                    fields
                                        .iter()
                                        .map(|(name, types)| {
                                            let types = types
                                                .as_list()
                                                .unwrap_or_default()
                                                .iter()
                                                .filter_map(Value::as_text)
                                                .map(Json::text)
                                                .collect();
                                            (name.to_owned(), Json::List(types))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            let sampled = sample
                                .values
                                .get("sampled")
                                .and_then(Value::as_int)
                                .and_then(|count| u64::try_from(count).ok())
                                .unwrap_or(0);
                            bytes += sample
                                .values
                                .get("bytes")
                                .and_then(Value::as_int)
                                .and_then(|count| u64::try_from(count).ok())
                                .unwrap_or(0);
                            if sampled > 0 {
                                sampled_fields = Json::Object(
                                    fields
                                        .iter()
                                        .filter_map(|(name, types)| match types.as_list()? {
                                            [one] => {
                                                let one = one.as_str()?;
                                                ["string", "integer", "number", "boolean"]
                                                    .contains(&one)
                                                    .then(|| (name.clone(), Json::text(one)))
                                            }
                                            _ => None,
                                        })
                                        .filter(|(name, _)| !name.is_empty())
                                        .collect(),
                                );
                                let more = sample.values.get("more").and_then(Value::as_bool)
                                    == Some(true);
                                shape_finding = Some((
                                    Json::object([
                                        ("address", Json::text(address)),
                                        ("fields", Json::Object(fields)),
                                        ("sampled", Json::number(sampled)),
                                    ]),
                                    more,
                                ));
                            }
                        }
                        Err(failed) if failed.failure.class() == FailureClass::Drift => {}
                        Err(failed) => return Err(failed.failure),
                    }
                }

                let subscriptions = facts
                    .get("subscriptions")
                    .and_then(Value::as_list)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Value::as_text)
                    .map(Json::text)
                    .collect();
                let schema = facts.get("schema").and_then(Value::as_text);
                // What a read drafted from this one finding returns: the
                // plain fields the broker's registered schema states, by
                // name and JSON type, where the pack reads the schema. A
                // drafting rule sees one finding at a time, so the channel
                // finding carries them; they are the broker's claim about
                // its messages, as the schema finding is, and no value.
                let schema_fields = schema
                    .and_then(|text| Json::parse(text).ok())
                    .and_then(|json| MessageSchema::from_registered(&json))
                    .map_or(Json::Null, |read| {
                        Json::Object(
                            read.fields
                                .iter()
                                .filter(|(_, rule)| {
                                    !matches!(rule.ty, JsonType::Array | JsonType::Object)
                                })
                                .map(|(name, rule)| (name.clone(), Json::text(rule.ty.as_str())))
                                .collect(),
                        )
                    });
                let found = Json::object([
                    ("address", Json::text(address)),
                    ("shape", text_json(facts.get("shape"))),
                    ("key_field", text_json(facts.get("key_field"))),
                    ("dedupe_header", text_json(facts.get("dedupe_header"))),
                    (
                        "dedupe_window_ms",
                        facts
                            .get("dedupe_window_ms")
                            .and_then(Value::as_int)
                            .and_then(|ms| u64::try_from(ms).ok())
                            .map_or(Json::Null, Json::number),
                    ),
                    ("subscriptions", Json::List(subscriptions)),
                    ("look", Json::text(look)),
                    ("schema", Json::Bool(schema.is_some())),
                    ("schema_fields", schema_fields),
                    ("sampled_fields", sampled_fields),
                ])
                .to_line();
                bytes += u64::try_from(found.len()).unwrap_or(u64::MAX);
                let listed = Finding::about(
                    subject(broker, address),
                    self.names.finding_channel.clone(),
                    scope.redact.redact(&found),
                    Source::LiveSystem,
                    self.produced_by(),
                    now,
                )
                .level(EvidenceLevel::Discovered)
                .coverage(Coverage::Complete)
                .build()?;

                if let Some(schema) = schema.and_then(|text| Json::parse(text).ok()) {
                    let found =
                        Json::object([("address", Json::text(address)), ("schema", schema)])
                            .to_line();
                    bytes += u64::try_from(found.len()).unwrap_or(u64::MAX);
                    // A registered schema is a claim about messages, not an
                    // observation of one.
                    let claimed = Finding::about(
                        subject(broker, address),
                        self.names.finding_schema.clone(),
                        scope.redact.redact(&found),
                        Source::LiveSystem,
                        self.produced_by(),
                        now,
                    )
                    .level(EvidenceLevel::Inferred)
                    .coverage(Coverage::Complete)
                    .derived_from(&listed)
                    .build()?;
                    out.found(listed.clone())?;
                    out.found(claimed)?;
                } else {
                    out.found(listed.clone())?;
                }

                if let Some((found, more)) = shape_finding {
                    let seen = Finding::about(
                        subject(broker, address),
                        self.names.finding_shape.clone(),
                        scope.redact.redact(&found.to_line()),
                        Source::LiveSystem,
                        self.produced_by(),
                        now,
                    )
                    .level(EvidenceLevel::Discovered)
                    .coverage(if more {
                        Coverage::Partial
                    } else {
                        Coverage::Complete
                    })
                    .limitation(
                        LimitationCode::Sampled,
                        "The names and types are those of the messages one bounded look returned; other messages may differ.",
                    )
                    .derived_from(&listed)
                    .build()?;
                    out.found(seen)?;
                }
            }
            Ok(report)
        })
    }
}

/// Puts the discoverer's findings so that an agent can read them.
pub(crate) struct BrokerDescriber {
    names: Names,
}

impl BrokerDescriber {
    pub(crate) fn new(names: Names) -> Self {
        Self { names }
    }
}

impl Describer for BrokerDescriber {
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description {
        let mut by_shape: BTreeMap<String, i64> = BTreeMap::new();
        let (mut channels, mut schemas, mut sampled) = (0_i64, 0_i64, 0_i64);
        let mut untrusted = Vec::new();
        for finding in findings {
            if *finding.kind() == self.names.finding_channel {
                channels += 1;
                let found = Json::parse(finding.found()).unwrap_or(Json::Null);
                let shape = found
                    .get("shape")
                    .and_then(Json::as_str)
                    .filter(|shape| crate::channel::Shape::from_word(shape).is_some())
                    .unwrap_or("unknown");
                *by_shape.entry(shape.to_owned()).or_default() += 1;
                if form == DescribeForm::Full
                    && let Some(address) = found.get("address").and_then(Json::as_str)
                {
                    untrusted.push(address.to_owned());
                }
            } else if *finding.kind() == self.names.finding_schema {
                schemas += 1;
            } else if *finding.kind() == self.names.finding_shape {
                sampled += 1;
            }
        }
        let shapes: Vec<String> = by_shape
            .iter()
            .map(|(shape, count)| format!("{count} {shape}"))
            .collect();
        let text = format!(
            "The broker lists {channels} channels ({}). It holds a schema for {schemas} of them, and a look that took nothing showed the shape of the messages of {sampled}.",
            if shapes.is_empty() {
                "none".to_owned()
            } else {
                shapes.join(", ")
            }
        );
        Description {
            text,
            data: Values::new()
                .with("channels", channels)
                .with("schemas", schemas)
                .with("sampled", sampled)
                .with("by_shape", by_shape.into_iter().collect::<Values>()),
            untrusted,
        }
    }
}
