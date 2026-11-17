//! The names the pack registers: its mode, its surface kind, its file kinds,
//! its verbs, its step kinds, and its finding kinds. Each is written once,
//! here.

use a2m_core::{Failure, FileKindName, FindingKind, Mode, StepKind, SurfaceKind, Verb};
use a2m_provider::DiscovererName;

/// The pack's own name, as its manifest states it.
pub(crate) const PACK: &str = "stream";

/// The surface kind the pack serves.
pub(crate) const BROKER: SurfaceKind = SurfaceKind::fixed("broker");

/// The file kind of the files under `channels/`.
pub(crate) const CHANNEL: FileKindName = FileKindName::fixed("channel");
/// The file kind of the files under `schemas/`.
pub(crate) const MESSAGE_SCHEMA: FileKindName = FileKindName::fixed("message_schema");
/// The kernel's file kind of the files under `operations/`.
pub(crate) const OPERATION: FileKindName = FileKindName::fixed("operation");

/// Reads the last retained value for a key.
pub(crate) const VERB_READ: Verb = Verb::fixed("read");
/// Lists what a channel holds, without taking anything.
pub(crate) const VERB_LIST: Verb = Verb::fixed("list");
/// Waits, within a limit, for a message that matches.
pub(crate) const VERB_AWAIT: Verb = Verb::fixed("await");
/// Publishes one message.
pub(crate) const VERB_PUBLISH: Verb = Verb::fixed("publish");
/// Takes and acknowledges one message of a queue the runtime owns.
pub(crate) const VERB_TAKE: Verb = Verb::fixed("take");

/// The last retained value for a key.
pub(crate) const STEP_LATEST: StepKind = StepKind::fixed("latest");
/// A bounded look at what a channel holds.
pub(crate) const STEP_BROWSE: StepKind = StepKind::fixed("browse");
/// Opens the run's own read path on a channel.
pub(crate) const STEP_SUBSCRIBE: StepKind = StepKind::fixed("subscribe");
/// Waits, within a limit, for one match.
pub(crate) const STEP_AWAIT: StepKind = StepKind::fixed("await");
/// Publishes one message.
pub(crate) const STEP_PUBLISH: StepKind = StepKind::fixed("publish");
/// Acknowledges the message the run holds.
pub(crate) const STEP_ACKNOWLEDGE: StepKind = StepKind::fixed("acknowledge");
/// Looks through a log for the message that carries the record's token.
pub(crate) const STEP_SCAN: StepKind = StepKind::fixed("scan");
/// Lists the channels the broker reports. Sent by the discoverer only.
pub(crate) const STEP_CHANNELS: StepKind = StepKind::fixed("channels");
/// Describes one channel. Sent by the discoverer only.
pub(crate) const STEP_DESCRIBE: StepKind = StepKind::fixed("describe");

/// The names that have no constant form: each is checked when the pack
/// registers, so nothing in the pack can fail on a name later.
#[derive(Debug, Clone)]
pub(crate) struct Names {
    /// The mode `stream`.
    pub mode: Mode,
    /// The discoverer `broker_look`.
    pub discoverer: DiscovererName,
    /// A channel the broker lists.
    pub finding_channel: FindingKind,
    /// A schema the broker holds for a channel.
    pub finding_schema: FindingKind,
    /// The shape of the messages a look saw.
    pub finding_shape: FindingKind,
}

impl Names {
    pub(crate) fn new() -> Result<Self, Failure> {
        Ok(Self {
            mode: Mode::new("stream")?,
            discoverer: DiscovererName::new("broker_look")?,
            finding_channel: FindingKind::new("channel")?,
            finding_schema: FindingKind::new("message_schema")?,
            finding_shape: FindingKind::new("message_shape")?,
        })
    }
}
