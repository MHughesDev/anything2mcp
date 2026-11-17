//! Faults a test can switch on, so that a failure is reproduced on demand.

use serde::{Deserialize, Serialize};

use crate::model::Op;

/// What a fault does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FaultKind {
    /// Store a publish, then drop the connection before the confirmation is sent.
    StoreThenDrop,
    /// Store a publish, then send no confirmation. The connection stays open.
    StoreThenSilent,
    /// Drop the connection when a publish arrives. Nothing is stored.
    DropBeforeStore,
    /// Refuse a publish with the code `refused` and this reason. Nothing is stored.
    Refuse {
        /// The reason the client is given.
        reason: String,
    },
    /// Wait this long before handling a request, then handle it correctly.
    Delay {
        /// Milliseconds to wait.
        ms: u64,
    },
    /// Hand a message over a second time, on the next fetch, looking like the first time.
    DuplicateDelivery,
    /// A consumer of the stand's own, named `silent`, takes messages from a queue as they
    /// become ready and never acknowledges. `times` is how many it takes. Clearing the
    /// faults makes it let go, and the messages are redelivered.
    SilentConsumer,
    /// Rename a top-level field of every body a read returns. What is stored does not change.
    RenameField {
        /// The field as it is stored.
        from: String,
        /// The name a reader sees.
        to: String,
    },
}

impl FaultKind {
    /// Whether the fault can apply to this request at all.
    #[must_use]
    pub fn applies_to(&self, op: Op) -> bool {
        match self {
            Self::StoreThenDrop | Self::StoreThenSilent | Self::DropBeforeStore | Self::Refuse { .. } => {
                op == Op::Publish
            }
            Self::Delay { .. } => true,
            Self::DuplicateDelivery => op == Op::Fetch,
            Self::SilentConsumer => false,
            Self::RenameField { .. } => matches!(op, Op::Latest | Op::Browse | Op::Fetch),
        }
    }
}

/// One fault and what it applies to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    /// What it does.
    #[serde(flatten)]
    pub kind: FaultKind,
    /// Only this channel is affected. All are, if absent.
    #[serde(default)]
    pub channel: Option<String>,
    /// Only this request is affected. Every request the fault can apply to is, if absent.
    #[serde(default)]
    pub op: Option<Op>,
    /// How many times it applies before it clears itself. Until cleared, if absent.
    #[serde(default)]
    pub times: Option<u32>,
}

impl Fault {
    /// A fault that applies once.
    #[must_use]
    pub fn once(kind: FaultKind) -> Self {
        Self::times(kind, 1)
    }

    /// A fault that applies this many times.
    #[must_use]
    pub fn times(kind: FaultKind, times: u32) -> Self {
        Self {
            kind,
            channel: None,
            op: None,
            times: Some(times),
        }
    }

    /// A fault that applies until the faults are cleared.
    #[must_use]
    pub fn until_cleared(kind: FaultKind) -> Self {
        Self {
            kind,
            channel: None,
            op: None,
            times: None,
        }
    }

    /// The same fault, limited to one channel.
    #[must_use]
    pub fn on_channel(mut self, channel: &str) -> Self {
        self.channel = Some(channel.to_owned());
        self
    }

    /// The same fault, limited to one request.
    #[must_use]
    pub fn on_op(mut self, op: Op) -> Self {
        self.op = Some(op);
        self
    }
}
