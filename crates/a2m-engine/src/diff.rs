//! The diff: what a preview produces, what a person approves, and what an
//! approval is bound to. The engine alone computes its hash.

use a2m_core::{
    CanonicalJson, Digest, Environment, Failure, FieldHasher, Identity, Mode, OperationName,
    RecordKey, TargetName, Value, WriteEvidence,
};
use serde::{Deserialize, Serialize};

/// What a write would do to one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffAction {
    /// The record does not exist and would be created.
    Create,
    /// The record exists and at least one field would change.
    Update,
    /// The record already holds the intended values.
    Unchanged,
    /// The record has a key, and its binding has nothing to read it with:
    /// its only evidence is the system's acknowledgement and it has no
    /// read-back step, such as a write-only register of a device. It will
    /// be written; what it holds now is not known, and each field's
    /// `current` is `None` for that reason, never because the record is
    /// absent. A door shows [`NOW_CANNOT_BE_READ`] in its place (decision
    /// 0196).
    Write,
}

/// The fixed words a door shows for what a record holds now when its
/// binding cannot read it ([`DiffAction::Write`]).
pub const NOW_CANNOT_BE_READ: &str =
    "cannot be read: this binding has nothing to read the record with";

impl DiffAction {
    /// The action as the diff and its hash write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Unchanged => "unchanged",
            Self::Write => "write",
        }
    }
}

/// One field of one record in a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffField {
    /// The field's name.
    pub name: String,
    /// The current value after normalization; `None` when the record does
    /// not exist, or, in a record whose action is [`DiffAction::Write`],
    /// because nothing can read it. For a field labelled restricted, the
    /// hash of the value.
    pub current: Option<Value>,
    /// The current value as the system gave it. Not in the hash, and never
    /// kept for a restricted field.
    pub current_raw: Option<Value>,
    /// The proposed value after normalization, or its hash when restricted.
    pub proposed: Value,
    /// Whether the write would change the field.
    pub changes: bool,
}

/// One record in a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffRecord {
    /// The pipeline step that writes it, from 0.
    pub step_seq: u32,
    /// Its place within the step, from 0.
    pub record_seq: u32,
    /// The operation that writes it.
    pub operation: OperationName,
    /// The record's key.
    pub key: RecordKey,
    /// What the write would do.
    pub action: DiffAction,
    /// Its fields, in the record's declared order.
    pub fields: Vec<DiffField>,
    /// What will stand as the evidence that its write landed. A record
    /// whose write is `acknowledged` is accepted by the system and not read
    /// back, and the diff a person approves says so. In the hash only when
    /// it is not `read_back`, so a diff of before this field hashes the
    /// same.
    #[serde(default, skip_serializing_if = "reads_back")]
    pub evidence: WriteEvidence,
    /// The binding that writes it, as the preview chose it, with its mode
    /// and its evidence. The engine writes the record through this binding
    /// and no other: one it would write with in its place stops the run,
    /// unsent, as a stale approval (decision 0185). `None` when the binding
    /// is chosen only once the record's key is known, because the key waits
    /// for a record written earlier in the run; then `could_take` names
    /// every binding it may be written by, and `evidence` is the weakest of
    /// theirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<DiffBinding>,
    /// Every binding a record whose binding is not known at the preview
    /// could be written by, each with its evidence. Empty when `binding`
    /// names it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub could_take: Vec<DiffBinding>,
}

/// One binding of an operation, as a diff names it for a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffBinding {
    /// Its position among the operation's bindings, from 0.
    pub position: u32,
    /// Its mode.
    pub mode: Mode,
    /// What will stand as the evidence that a write through it landed.
    pub evidence: WriteEvidence,
}

impl DiffBinding {
    fn hashed(&self, hasher: &mut FieldHasher) {
        hasher.field("binding", &u64::from(self.position).to_le_bytes());
        hasher.field("mode", self.mode.as_str().as_bytes());
        hasher.field("binding_evidence", self.evidence.as_str().as_bytes());
    }
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the form serde's `skip_serializing_if` calls"
)]
fn reads_back(evidence: &WriteEvidence) -> bool {
    *evidence == WriteEvidence::ReadBack
}

impl DiffRecord {
    /// Whether the record's write is accepted by the system and not read
    /// back, or may be, by a binding it could take: what a person who
    /// approves the diff is told of it.
    #[must_use]
    pub fn not_read_back(&self) -> bool {
        self.evidence == WriteEvidence::Acknowledged
    }

    /// Whether the record's binding cannot read what it holds now: the
    /// page shows [`NOW_CANNOT_BE_READ`] for each field's current value.
    #[must_use]
    pub fn now_cannot_be_read(&self) -> bool {
        self.action == DiffAction::Write
    }

    /// Whether the binding that writes the record is chosen only once its
    /// key is known: the approval names every binding it could take.
    #[must_use]
    pub fn binding_unknown(&self) -> bool {
        self.binding.is_none() && !self.could_take.is_empty()
    }

    /// The positions of the bindings the approval lets write the record.
    #[must_use]
    pub fn approved_bindings(&self) -> Vec<usize> {
        self.binding
            .iter()
            .chain(&self.could_take)
            .filter_map(|binding| usize::try_from(binding.position).ok())
            .collect()
    }
}

/// How many records a diff would create, update, and leave alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiffCounts {
    /// Records to create.
    pub create: u32,
    /// Records to update.
    pub update: u32,
    /// Records that already hold the intended values.
    pub unchanged: u32,
    /// Records that will be written and whose binding cannot read what
    /// they hold now.
    pub write: u32,
}

/// A diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diff {
    /// The tool.
    pub tool: OperationName,
    /// The tool's version: the hash of the plan.
    pub plan: Digest,
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// Who the system sees.
    pub identity: Option<Identity>,
    /// The records, in the order the run would write them.
    pub records: Vec<DiffRecord>,
}

fn value_bytes(value: Option<&Value>) -> Vec<u8> {
    // A leading byte keeps "no record" apart from every value, null too.
    match value {
        None => vec![0],
        Some(value) => {
            let mut bytes = vec![1];
            value.write_canonical(&mut bytes);
            bytes
        }
    }
}

impl Diff {
    /// The counts an approver sees first.
    #[must_use]
    pub fn counts(&self) -> DiffCounts {
        let mut counts = DiffCounts::default();
        for record in &self.records {
            match record.action {
                DiffAction::Create => counts.create += 1,
                DiffAction::Update => counts.update += 1,
                DiffAction::Unchanged => counts.unchanged += 1,
                DiffAction::Write => counts.write += 1,
            }
        }
        counts
    }

    /// Whether the diff holds anything to create or update.
    #[must_use]
    pub fn changes_anything(&self) -> bool {
        self.records
            .iter()
            .any(|record| record.action != DiffAction::Unchanged)
    }

    /// The diff hash: SHA-256 over the tool, its version hash, the target,
    /// the environment, the identity, and, for each record in order of its
    /// key, the key, the operation, the action, and each field in order of
    /// its name with its normalized current and proposed values. Raw forms
    /// and moments are left out. Every part is written with its name and
    /// its length, and every value in the canonical form of `a2m-core`, so
    /// the same diff has the same hash on every machine.
    #[must_use]
    pub fn hash(&self) -> Digest {
        let mut hasher = FieldHasher::new();
        hasher.field("purpose", b"anything2mcp diff 1");
        hasher.field("tool", self.tool.as_str().as_bytes());
        hasher.field("plan", self.plan.as_bytes());
        hasher.field("target", self.target.as_str().as_bytes());
        hasher.field("environment", self.environment.as_str().as_bytes());
        let identity = self
            .identity
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        hasher.field("identity", identity.as_bytes());
        let mut records: Vec<(Vec<u8>, &DiffRecord)> = self
            .records
            .iter()
            .map(|record| (record.key.canonical_bytes(), record))
            .collect();
        records.sort_by(|left, right| {
            (&left.0, left.1.step_seq, left.1.record_seq).cmp(&(
                &right.0,
                right.1.step_seq,
                right.1.record_seq,
            ))
        });
        hasher.field("records", &(records.len() as u64).to_le_bytes());
        for (key, record) in records {
            hasher.field("key", &key);
            hasher.field("operation", record.operation.as_str().as_bytes());
            hasher.field("action", record.action.as_str().as_bytes());
            if record.evidence != WriteEvidence::ReadBack {
                hasher.field("evidence", record.evidence.as_str().as_bytes());
            }
            // The binding the approval names is part of what was approved.
            if let Some(binding) = &record.binding {
                binding.hashed(&mut hasher);
            }
            if !record.could_take.is_empty() {
                hasher.field(
                    "could_take",
                    &(record.could_take.len() as u64).to_le_bytes(),
                );
                for binding in &record.could_take {
                    binding.hashed(&mut hasher);
                }
            }
            let mut fields: Vec<&DiffField> = record.fields.iter().collect();
            fields.sort_by(|left, right| left.name.cmp(&right.name));
            hasher.field("fields", &(fields.len() as u64).to_le_bytes());
            for field in fields {
                hasher.field("name", field.name.as_bytes());
                hasher.field("current", &value_bytes(field.current.as_ref()));
                hasher.field("proposed", &value_bytes(Some(&field.proposed)));
            }
        }
        hasher.finish()
    }

    /// The diff as it is stored: canonical JSON.
    ///
    /// # Errors
    ///
    /// `input`, `payload_not_encodable`, which no diff gives.
    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        Ok(CanonicalJson::encode(self)?.as_bytes().to_vec())
    }

    /// Reads a stored diff back.
    ///
    /// # Errors
    ///
    /// `input`, `payload_unreadable`, when the bytes are not a diff.
    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        serde_json::from_slice(bytes).map_err(|_| {
            a2m_core::codes::row::PAYLOAD_UNREADABLE.failure(
                "A stored diff cannot be read back.",
                "Preview the run again.",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::{
        Decimal, Digest, Environment, FieldTypeIndex, OperationName, RecordKey, Sensitivity,
        TargetName, Value, WriteEvidence,
    };

    use super::{Diff, DiffAction, DiffField, DiffRecord};
    use crate::compare::{Field, guarded};

    /// The hash of [`sample`]. It is written here once, and the worked
    /// example in `docs/reference/steps-snapshots-and-diffs.md` prints the
    /// same value: a machine that computed another hash for the same diff
    /// would refuse every approval given on another machine.
    const DIFF_SAMPLE_HASH: &str =
        "1b52a16688289f1af2c3a9b6a0cf2c7b84e468b6c628835ea70dff0e70e09017";

    fn field(name: &str, current: Option<Value>, proposed: Value) -> DiffField {
        DiffField {
            changes: current.as_ref() != Some(&proposed),
            name: name.to_owned(),
            current_raw: current.clone(),
            current,
            proposed,
        }
    }

    /// A restricted field's value as a diff holds it: the hash of its
    /// normalized form, by the engine's own rule.
    fn restricted(value: &str) -> Value {
        let note = Field {
            name: "note".to_owned(),
            field_type: FieldTypeIndex::new(0),
            sensitivity: Sensitivity::Restricted,
        };
        guarded(&note, Value::Text(value.to_owned()))
    }

    fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    fn price(value: &str) -> Value {
        Value::Decimal(Decimal::parse(value).unwrap())
    }

    fn record(seq: u32, key: i64, action: DiffAction, fields: Vec<DiffField>) -> DiffRecord {
        DiffRecord {
            step_seq: 0,
            record_seq: seq,
            operation: OperationName::new("example_items_item_save").unwrap(),
            key: RecordKey::new(vec![Value::Int(key)]).unwrap(),
            action,
            fields,
            evidence: WriteEvidence::ReadBack,
            binding: None,
            could_take: Vec::new(),
        }
    }

    /// A fixed diff: a record to create, one to update, and one unchanged,
    /// with a decimal and a field labelled restricted, whose raw form is
    /// never kept. The records are not in the order of their keys, so the
    /// hash's own order is exercised.
    fn sample() -> Diff {
        let created = record(
            0,
            3,
            DiffAction::Create,
            vec![
                field("name", None, text("Bolt")),
                field("price", None, price("2.5")),
                field("note", None, restricted("keep dry")),
            ],
        );
        let mut updated = record(
            1,
            1,
            DiffAction::Update,
            vec![
                field("name", Some(text("Nut")), text("Nut")),
                field("price", Some(price("0.75")), price("0.8")),
                field("note", Some(restricted("old")), restricted("new")),
            ],
        );
        updated.fields[2].current_raw = None;
        let unchanged = record(
            2,
            2,
            DiffAction::Unchanged,
            vec![
                field("name", Some(text("Washer")), text("Washer")),
                field("price", Some(price("0.1")), price("0.10")),
            ],
        );
        Diff {
            tool: OperationName::new("example_items_item_save").unwrap(),
            plan: Digest::of(b"the sample plan"),
            target: TargetName::new("example").unwrap(),
            environment: Environment::new("test").unwrap(),
            identity: Some("account:example_writer".parse().unwrap()),
            records: vec![created, updated, unchanged],
        }
    }

    #[test]
    fn a_diff_hashes_the_same_on_every_machine() {
        let diff = sample();
        assert_eq!(diff.hash().to_string(), DIFF_SAMPLE_HASH);
        // The stored form reads back to the same diff and the same hash.
        let stored = Diff::decode(&diff.encode().unwrap()).unwrap();
        assert_eq!(stored, diff);
        assert_eq!(stored.hash().to_string(), DIFF_SAMPLE_HASH);
        // Neither the order of the records, nor the order of the fields,
        // nor a raw form enters the hash.
        let mut reordered = diff.clone();
        reordered.records.reverse();
        for item in &mut reordered.records {
            item.fields.reverse();
            for each in &mut item.fields {
                each.current_raw = None;
            }
        }
        assert_eq!(reordered.hash().to_string(), DIFF_SAMPLE_HASH);
        // A proposed value does.
        let mut changed = diff;
        changed.records[0].fields[1].proposed = price("2.6");
        assert_ne!(changed.hash().to_string(), DIFF_SAMPLE_HASH);
    }

    /// The worked example in the reference: the diff it prints is the
    /// sample, and the hash it prints is the sample's.
    #[test]
    fn the_worked_example_in_the_reference_hashes_to_the_value_it_prints() {
        let document = include_str!("../../../docs/reference/steps-snapshots-and-diffs.md");
        let example = document
            .split_once("**A worked example.**")
            .expect("the reference has a worked example")
            .1;
        let json = example
            .split_once("```json\n")
            .and_then(|(_, rest)| rest.split_once("```"))
            .expect("the worked example prints its diff as JSON")
            .0;
        let printed = example
            .split_once("Its hash is `")
            .and_then(|(_, rest)| rest.split_once('`'))
            .expect("the worked example prints its hash")
            .0;
        let diff = Diff::decode(json.as_bytes()).expect("the printed diff reads as a diff");
        assert_eq!(diff, sample());
        assert_eq!(diff.hash().to_string(), printed);
        assert_eq!(printed, DIFF_SAMPLE_HASH);
    }
}
