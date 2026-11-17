//! Building a trace: redaction on entry, the signature, the capture rule,
//! and the check on the way out.

use std::fmt;
use std::sync::Arc;

use a2m_core::{
    Digest, DigestPurpose, Failure, FailureClass, KeyedDigest, Limitation, LimitationCode, Redact,
    Sensitivity, Value,
};

use super::signature::{Canon, Chain};
use super::wire::{
    EndLine, HeaderLine, Held, PassLine, StepLine, TRACE_SCHEMA, TraceLine, ValueLine,
};
use super::{
    Acting, ArtifactOutcome, CaptureLevel, Detail, DropReason, KeptArtifact, Locating, MaskReport,
    OfferedArtifact, StepRecord, TraceIdentity, TraceOutcome, TracedValue,
};
use crate::clock::micros;
use crate::failure::TRACE_HOLDS_SECRET;
use crate::text::{FREE_TEXT_LIMIT, LABEL_LIMIT, clean_text, limit_text};

/// The size, in bytes, a trace is cut at unless the engine names another.
pub const DEFAULT_TRACE_LIMIT: usize = 32 * 1024 * 1024;

/// Builds the trace of one run attempt. Made by the engine, which holds it
/// for the attempt and ends it with [`TraceBuilder::finish`].
///
/// Everything it is given is redacted before it is kept, so nothing
/// unredacted is held longer than the call that brought it.
pub struct TraceBuilder {
    identity: TraceIdentity,
    level: CaptureLevel,
    redact: Arc<dyn Redact>,
    keyed: Arc<dyn KeyedDigest>,
    limit: usize,
    lines: Vec<u8>,
    chain: Chain,
    kept: u32,
    cut: bool,
    limitations: Vec<Limitation>,
}

/// A finished trace: redacted, checked bytes for the store, and the
/// signature for the store to record on the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedTrace {
    /// The version of the schema `bytes` is written in, to store with the
    /// blob.
    pub schema: u32,
    /// The trace, as JSON Lines.
    pub bytes: Vec<u8>,
    /// The trace signature.
    pub signature: Digest,
    /// How many steps the run recorded.
    pub records: u32,
    /// How many of them are in `bytes`.
    pub kept: u32,
    /// What is weak about this trace: a cut, a dropped artifact.
    pub limitations: Vec<Limitation>,
}

/// A finished trace that held a registered secret at the check on the way
/// out. The trace was dropped: there are no bytes to store. The signature is
/// a digest and is still good; the limitation is what the engine records in
/// the trace's place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceRefused {
    /// The trace signature.
    pub signature: Digest,
    /// How many steps the run recorded.
    pub records: u32,
    /// Why there is no trace, for the run's evidence.
    pub limitation: Limitation,
}

impl fmt::Display for TraceRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.limitation.sentence)
    }
}

impl std::error::Error for TraceRefused {}

impl From<TraceRefused> for Failure {
    fn from(refused: TraceRefused) -> Self {
        Self::new(
            FailureClass::Input,
            TRACE_HOLDS_SECRET,
            refused.limitation.sentence,
            "Register every secret with the vault before the run uses it, then run again.",
        )
    }
}

/// A value on its way into a record: the form the trace holds, and the bytes
/// it contributes to the signature.
struct Entered {
    held: Held,
    signed: Vec<u8>,
}

impl TraceBuilder {
    /// Starts the trace of one run attempt at `level`. `redact` is the
    /// vault's redactor and `keyed` its keyed digest; this crate never makes
    /// its own.
    #[must_use]
    pub fn new(
        identity: TraceIdentity,
        level: CaptureLevel,
        redact: Arc<dyn Redact>,
        keyed: Arc<dyn KeyedDigest>,
    ) -> Self {
        let mut builder = Self {
            identity,
            level,
            redact,
            keyed,
            limit: DEFAULT_TRACE_LIMIT,
            lines: Vec::new(),
            chain: Chain::new(),
            kept: 0,
            cut: false,
            limitations: Vec::new(),
        };
        let header = TraceLine::Header(HeaderLine {
            schema: TRACE_SCHEMA,
            run: builder.identity.run,
            attempt: builder.identity.attempt,
            operation: builder.identity.operation.clone(),
            plan: builder.identity.plan,
            engine: builder.identity.engine,
            registry: builder.identity.registry,
            level,
        });
        builder.append(&header);
        builder
    }

    /// Sets the size, in bytes, at which the trace is cut. Records past the
    /// limit are left out of the bytes and still enter the signature.
    #[must_use]
    pub fn with_limit(mut self, bytes: usize) -> Self {
        self.limit = bytes;
        self
    }

    /// The capture level this trace applies.
    #[must_use]
    pub const fn level(&self) -> CaptureLevel {
        self.level
    }

    /// Whether the trace has passed its size limit and is no longer keeping
    /// records.
    #[must_use]
    pub const fn is_cut(&self) -> bool {
        self.cut
    }

    /// Adds the record of one step. Records are given in the order the steps
    /// were carried out; that order is part of the signature.
    pub fn record(&mut self, step: StepRecord) {
        let seq = self.chain.records();
        let record = step.record.as_ref().map(|key| {
            self.keyed.keyed_digest(
                &self.identity.workspace,
                DigestPurpose::RecordKey,
                &key.canonical_bytes(),
            )
        });
        let locating = step.locating.map(|locating| Locating {
            locator: self.safe(&locating.locator),
            position: locating.position,
            score: locating.score,
        });
        let acting = step.acting.map(|acting| Acting {
            means: self.safe_details(acting.means),
            measured: self.safe_details(acting.measured),
        });

        let mut canon = Canon::record();
        canon.number("pass", u64::from(step.pass));
        canon.text("phase", step.phase.as_str());
        canon.number("step", u64::from(step.step));
        canon.text("kind", step.kind.as_str());
        canon.optional(
            "record",
            record.as_ref().map(|digest| &digest.as_bytes()[..]),
        );
        let chosen = step.begins.as_ref();
        canon.optional(
            "operation",
            chosen.map(|begins| begins.operation.as_str().as_bytes()),
        );
        canon.optional("mode", chosen.map(|begins| begins.mode.as_str().as_bytes()));
        canon.optional(
            "locator",
            locating.as_ref().map(|found| found.locator.as_bytes()),
        );
        canon.optional(
            "position",
            locating
                .as_ref()
                .map(|found| found.position.to_le_bytes())
                .as_ref()
                .map(|bytes| &bytes[..]),
        );
        let means = acting.as_ref().map_or(&[][..], |acting| &acting.means[..]);
        canon.number("means", means.len() as u64);
        for detail in means {
            canon.text("name", &detail.name);
            canon.text("value", &detail.value);
        }
        canon.number("values", step.values.len() as u64);
        let values: Vec<ValueLine> = step
            .values
            .into_iter()
            .map(|value| self.enter(&mut canon, value))
            .collect();
        match &step.outcome {
            TraceOutcome::Verified => canon.text("outcome", "verified"),
            TraceOutcome::Done => canon.text("outcome", "done"),
            TraceOutcome::Failed { class, code } => {
                canon.text("outcome", "failed");
                canon.text("class", class.as_str());
                canon.text("code", code.as_str());
            }
        }
        let sig = canon.finish();
        self.chain.push(&sig);

        let line = TraceLine::Step(Box::new(StepLine {
            seq,
            pass: step.pass,
            phase: step.phase,
            step: step.step,
            kind: step.kind,
            record,
            begins: step.begins.map(|begins| PassLine {
                operation: begins.operation,
                mode: begins.mode,
                reason: begins.reason,
                driver: begins.driver,
                queue_us: micros(begins.queue),
                prepare_us: micros(begins.prepare),
            }),
            took_us: micros(step.took),
            locating: locating.map(|found| Locating {
                locator: limit_text(&found.locator, FREE_TEXT_LIMIT),
                ..found
            }),
            acting: acting.map(|acting| Acting {
                means: cut_details(acting.means),
                measured: cut_details(acting.measured),
            }),
            values,
            outcome: step.outcome,
            artifacts: step.artifacts,
            sig,
        }));
        self.append(&line);
    }

    /// Offers an artifact a driver returned. The level decides whether it is
    /// kept. A kept artifact comes back as redacted, checked bytes for the
    /// store; the name the store gives it goes into the step's record.
    pub fn artifact(&mut self, offered: OfferedArtifact) -> ArtifactOutcome {
        if !self.level.keeps(offered.kind, offered.at) {
            return ArtifactOutcome::Dropped(DropReason::Level);
        }
        if offered.kind.is_text() {
            return self.text_artifact(offered);
        }
        let Some(masked) = offered.masked else {
            self.limit_once(
                LimitationCode::Redacted,
                "An image arrived without a report of what was masked, and was dropped.",
            );
            return ArtifactOutcome::Dropped(DropReason::Unmasked);
        };
        let masked = MaskReport {
            fields: masked
                .fields
                .iter()
                .map(|field| limit_text(&self.safe(field), LABEL_LIMIT))
                .collect(),
        };
        ArtifactOutcome::Kept(KeptArtifact {
            kind: offered.kind,
            at: offered.at,
            bytes: offered.bytes,
            masked: Some(masked),
        })
    }

    fn text_artifact(&mut self, offered: OfferedArtifact) -> ArtifactOutcome {
        let Ok(text) = String::from_utf8(offered.bytes) else {
            self.limit_once(
                LimitationCode::Redacted,
                "An artifact that should be text was not, could not be checked, and was dropped.",
            );
            return ArtifactOutcome::Dropped(DropReason::NotText);
        };
        let redacted = self.redact.redact(&text);
        // The check on the way out, for this piece.
        if self.redact.contains_secret(&redacted) {
            self.limit_once(
                LimitationCode::Redacted,
                "An artifact still held a registered secret after redaction, and was dropped.",
            );
            tracing::error!(
                target: "a2m_telemetry",
                run = %self.identity.run,
                "an artifact held a registered secret after redaction and was dropped"
            );
            return ArtifactOutcome::Dropped(DropReason::HeldSecret);
        }
        ArtifactOutcome::Kept(KeptArtifact {
            kind: offered.kind,
            at: offered.at,
            bytes: redacted.into_bytes(),
            masked: None,
        })
    }

    /// Ends the trace: the JSON Lines bytes, already checked, and the
    /// signature.
    ///
    /// # Errors
    ///
    /// [`TraceRefused`] when the finished trace still holds a registered
    /// secret, which happens when a secret was registered after text that
    /// held it was recorded, or when a number is a secret. The trace is
    /// dropped and an alert is logged; nothing is handed back to store.
    pub fn finish(mut self) -> Result<FinishedTrace, TraceRefused> {
        let records = self.chain.records();
        let signature = self.chain.signature();
        if self.cut {
            let sentence = format!(
                "The trace passed its size limit of {} bytes; {} of {records} records are kept.",
                self.limit, self.kept
            );
            self.limit_once(LimitationCode::Truncated, &sentence);
        }
        let end = TraceLine::End(EndLine {
            records,
            kept: self.kept,
            signature,
            limitations: self.limitations.clone(),
        });
        // The end line is written even past the limit: without it a reader
        // cannot tell a cut trace from a broken one.
        write_line(&mut self.lines, &end);

        if self.holds_secret() {
            tracing::error!(
                target: "a2m_telemetry",
                run = %self.identity.run,
                records,
                "a finished trace held a registered secret and was dropped"
            );
            return Err(TraceRefused {
                signature,
                records,
                limitation: Limitation {
                    code: LimitationCode::Redacted,
                    sentence: "The step trace still held a registered secret when it was finished, and was dropped."
                        .to_owned(),
                },
            });
        }
        Ok(FinishedTrace {
            schema: TRACE_SCHEMA,
            bytes: self.lines,
            signature,
            records,
            kept: self.kept,
            limitations: self.limitations,
        })
    }

    /// One field's values: what the trace holds of them, and what they add
    /// to the signature.
    fn enter(&self, canon: &mut Canon, value: TracedValue) -> ValueLine {
        let field = self.safe(&value.field);
        let sensitivity = value.sensitivity;
        let intended = self.hold(value.intended.as_ref(), sensitivity);
        let read_back = self.hold(value.read_back.as_ref(), sensitivity);
        let normalized_intended = self.hold(value.normalized_intended.as_ref(), sensitivity);
        let normalized_read_back = self.hold(value.normalized_read_back.as_ref(), sensitivity);

        canon.text("field", &field);
        canon.number("volatile", u64::from(value.volatile));
        if !value.volatile {
            canon.optional(
                "normalized_intended",
                normalized_intended
                    .as_ref()
                    .map(|entered| &entered.signed[..]),
            );
            canon.optional(
                "normalized_read_back",
                normalized_read_back
                    .as_ref()
                    .map(|entered| &entered.signed[..]),
            );
        }
        canon.optional(
            "matched",
            value
                .matched
                .map(|matched| [u8::from(matched)])
                .as_ref()
                .map(|flag| &flag[..]),
        );

        ValueLine {
            field: limit_text(&field, LABEL_LIMIT),
            sensitivity,
            volatile: value.volatile,
            intended: intended.map(|entered| entered.held),
            read_back: read_back.map(|entered| entered.held),
            normalized_intended: normalized_intended.map(|entered| entered.held),
            normalized_read_back: normalized_read_back.map(|entered| entered.held),
            matched: value.matched,
        }
    }

    /// A value as its label allows it to be held. A value that is not public
    /// enters the signature as its keyed digest, whatever the level, so the
    /// signature never says more about it than the trace does.
    fn hold(&self, value: Option<&Value>, sensitivity: Sensitivity) -> Option<Entered> {
        let safe = self.safe_value(value?);
        let canonical = safe.canonical_bytes();
        if sensitivity == Sensitivity::Public {
            let mut signed = Vec::with_capacity(canonical.len() + 1);
            signed.push(b'c');
            signed.extend_from_slice(&canonical);
            return Some(Entered {
                held: Held::Value(cut_value(safe)),
                signed,
            });
        }
        let digest = self.keyed.keyed_digest(
            &self.identity.workspace,
            DigestPurpose::RestrictedValue,
            &canonical,
        );
        let mut signed = Vec::with_capacity(Digest::LENGTH + 1);
        signed.push(b'k');
        signed.extend_from_slice(digest.as_bytes());
        let as_digest =
            sensitivity == Sensitivity::Restricted || self.level == CaptureLevel::Minimal;
        Some(Entered {
            held: if as_digest {
                Held::Keyed(digest)
            } else {
                Held::Value(cut_value(safe))
            },
            signed,
        })
    }

    /// Free text made safe to keep: redacted, cleaned of markup and hidden
    /// characters, and redacted once more, because cleaning can join the
    /// two halves of a secret that markup had split.
    fn safe(&self, text: &str) -> String {
        let cleaned = clean_text(&self.redact.redact(text));
        self.redact.redact(&cleaned)
    }

    fn safe_value(&self, value: &Value) -> Value {
        match value {
            Value::Text(text) => Value::Text(self.safe(text)),
            Value::List(items) => {
                Value::List(items.iter().map(|item| self.safe_value(item)).collect())
            }
            Value::Record(values) => Value::Record(
                values
                    .iter()
                    .map(|(name, value)| (self.safe(name), self.safe_value(value)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn safe_details(&self, details: Vec<Detail>) -> Vec<Detail> {
        details
            .into_iter()
            .map(|detail| Detail {
                name: self.safe(&detail.name),
                value: self.safe(&detail.value),
            })
            .collect()
    }

    /// Appends a line unless the trace is cut or the line would take it past
    /// its limit.
    fn append(&mut self, line: &TraceLine) {
        if self.cut {
            return;
        }
        let before = self.lines.len();
        write_line(&mut self.lines, line);
        if self.lines.len() > self.limit {
            self.lines.truncate(before);
            self.cut = true;
        } else if matches!(line, TraceLine::Step(_)) {
            self.kept = self.kept.saturating_add(1);
        }
    }

    fn limit_once(&mut self, code: LimitationCode, sentence: &str) {
        let limitation = Limitation {
            code,
            sentence: sentence.to_owned(),
        };
        if !self.limitations.contains(&limitation) {
            self.limitations.push(limitation);
        }
    }

    /// The check on the way out: the bytes as written, and every text in
    /// them as it reads once the JSON escapes are undone.
    fn holds_secret(&self) -> bool {
        let Ok(text) = std::str::from_utf8(&self.lines) else {
            return true;
        };
        if self.redact.contains_secret(text) {
            return true;
        }
        text.lines().any(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .map_or(true, |parsed| self.any_text_holds_secret(&parsed))
        })
    }

    fn any_text_holds_secret(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(text) => self.redact.contains_secret(text),
            serde_json::Value::Array(items) => {
                items.iter().any(|item| self.any_text_holds_secret(item))
            }
            serde_json::Value::Object(entries) => entries.iter().any(|(name, item)| {
                self.redact.contains_secret(name) || self.any_text_holds_secret(item)
            }),
            _ => false,
        }
    }
}

/// Writes one line and its line feed. A line that cannot be written as JSON
/// is left out; none of this crate's lines can fail to be.
fn write_line(out: &mut Vec<u8>, line: &TraceLine) {
    let before = out.len();
    if serde_json::to_writer(&mut *out, line).is_ok() {
        out.push(b'\n');
    } else {
        out.truncate(before);
    }
}

/// A value with every text in it limited in length, for the trace's bytes.
fn cut_value(value: Value) -> Value {
    match value {
        Value::Text(text) => Value::Text(limit_text(&text, FREE_TEXT_LIMIT)),
        Value::List(items) => Value::List(items.into_iter().map(cut_value).collect()),
        Value::Record(values) => Value::Record(
            values
                .into_iter()
                .map(|(name, value)| (limit_text(&name, LABEL_LIMIT), cut_value(value)))
                .collect(),
        ),
        other => other,
    }
}

fn cut_details(details: Vec<Detail>) -> Vec<Detail> {
    details
        .into_iter()
        .map(|detail| Detail {
            name: limit_text(&detail.name, LABEL_LIMIT),
            value: limit_text(&detail.value, FREE_TEXT_LIMIT),
        })
        .collect()
}
