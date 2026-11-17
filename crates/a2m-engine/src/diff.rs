//! The diff: what a preview produces, what a person approves, and what an
//! approval is bound to. The engine alone computes its hash.

use a2m_core::{
    CanonicalJson, Digest, Environment, Failure, FieldHasher, Identity, OperationName, RecordKey,
    TargetName, Value,
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
}

impl DiffAction {
    /// The action as the diff and its hash write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Unchanged => "unchanged",
        }
    }
}

/// One field of one record in a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffField {
    /// The field's name.
    pub name: String,
    /// The current value after normalization; `None` when the record does
    /// not exist. For a field labelled restricted, the hash of the value.
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
            Failure::new(
                a2m_core::FailureClass::Input,
                a2m_core::FailureCode::PAYLOAD_UNREADABLE,
                "A stored diff cannot be read back.",
                "Preview the run again.",
            )
        })
    }
}
