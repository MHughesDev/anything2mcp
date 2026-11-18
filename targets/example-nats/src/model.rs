//! What a test declares and what the control interface reports.

use std::collections::BTreeMap;
use std::time::Duration;

/// How a stream keeps what it stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// Until a limit removes it.
    Limits,
    /// Until a consumer acknowledges it.
    WorkQueue,
}

/// One stream, as a person would declare it on a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSpec {
    /// Its name.
    pub name: String,
    /// The subjects it stores, with `*` and `>`.
    pub subjects: Vec<String>,
    /// How it keeps messages.
    pub retention: Retention,
    /// How many messages of one subject it keeps, the oldest evicted first,
    /// from inside the stream; -1 for no limit.
    pub max_msgs_per_subject: i64,
    /// Whether a message may carry its own time to live.
    pub allow_msg_ttl: bool,
    /// How long a message id is remembered.
    pub duplicate_window: Duration,
    /// The largest message, headers included, it stores.
    pub max_msg_size: Option<usize>,
}

impl StreamSpec {
    /// A stream with limits retention and the server's default window of two minutes.
    pub fn new<const N: usize>(name: &str, subjects: [&str; N]) -> Self {
        Self {
            name: name.to_owned(),
            subjects: subjects.iter().map(|s| (*s).to_owned()).collect(),
            retention: Retention::Limits,
            max_msgs_per_subject: -1,
            allow_msg_ttl: false,
            duplicate_window: Duration::from_secs(120),
            max_msg_size: None,
        }
    }

    /// Keeps the last message of each subject only.
    #[must_use]
    pub fn one_per_subject(mut self) -> Self {
        self.max_msgs_per_subject = 1;
        self
    }

    /// Keeps the last `count` messages of each subject.
    #[must_use]
    pub fn per_subject(mut self, count: i64) -> Self {
        self.max_msgs_per_subject = count;
        self
    }

    /// Keeps a message until it is acknowledged.
    #[must_use]
    pub fn work_queue(mut self) -> Self {
        self.retention = Retention::WorkQueue;
        self
    }

    /// Sets the duplicate window.
    #[must_use]
    pub fn window(mut self, window: Duration) -> Self {
        self.duplicate_window = window;
        self
    }

    /// Sets the largest message it stores.
    #[must_use]
    pub fn max_msg_size(mut self, bytes: usize) -> Self {
        self.max_msg_size = Some(bytes);
        self
    }
}

/// How a consumer expects acknowledgements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckPolicy {
    /// None: a delivery is done when it is sent.
    None,
    /// Each delivery is acknowledged, or it goes out again.
    Explicit,
}

/// One pull consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerSpec {
    /// Its name; empty for one the server names.
    pub name: String,
    /// Its acknowledgement policy.
    pub ack: AckPolicy,
    /// Only subjects that match this.
    pub filter: Option<String>,
    /// Starts at the stream's next message rather than its first.
    pub deliver_new: bool,
    /// How long a delivery waits for its acknowledgement before the message
    /// goes out again, on the stand's clock.
    pub ack_wait: Duration,
}

impl ConsumerSpec {
    /// A consumer that starts at the first message, with the server's
    /// default acknowledgement wait of thirty seconds.
    #[must_use]
    pub fn new(name: &str, ack: AckPolicy) -> Self {
        Self {
            name: name.to_owned(),
            ack,
            filter: None,
            deliver_new: false,
            ack_wait: Duration::from_secs(30),
        }
    }

    /// Sets the acknowledgement wait.
    #[must_use]
    pub fn ack_wait(mut self, wait: Duration) -> Self {
        self.ack_wait = wait;
        self
    }
}

/// A message a stream holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    /// Its stream sequence.
    pub seq: u64,
    /// Its subject.
    pub subject: String,
    /// Its headers.
    pub headers: BTreeMap<String, String>,
    /// Its payload.
    pub data: Vec<u8>,
}

impl StoredMessage {
    /// The payload as text.
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }
}

/// What became of one publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Stored at this sequence.
    Stored(u64),
    /// Not stored: a message with its id was stored at this sequence within the window.
    Duplicate(u64),
    /// Refused with this error code; not stored.
    Refused(u16),
    /// No stream listens on the subject.
    NotStored,
}

/// One publish the server received, whatever became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// The connection, numbered from 1.
    pub connection: u64,
    /// The subject.
    pub subject: String,
    /// The headers.
    pub headers: BTreeMap<String, String>,
    /// The payload.
    pub data: Vec<u8>,
    /// What became of it.
    pub outcome: Outcome,
}

/// One message handed to a consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    /// The connection that pulled it.
    pub connection: u64,
    /// The stream.
    pub stream: String,
    /// The consumer.
    pub consumer: String,
    /// The stream sequence.
    pub seq: u64,
    /// Which time it went out, from 1.
    pub attempt: u64,
}

/// What an acknowledgement said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckKind {
    /// Done with it.
    Ack,
    /// Give it back.
    Nak,
    /// Never deliver it again.
    Term,
    /// A word the stand ignores.
    Other,
}

/// One acknowledgement the server processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acknowledgement {
    /// The connection.
    pub connection: u64,
    /// The stream.
    pub stream: String,
    /// The consumer.
    pub consumer: String,
    /// The stream sequence.
    pub seq: u64,
    /// What it said.
    pub kind: AckKind,
    /// Whether the delivery was still pending.
    pub was_pending: bool,
}

/// One request on the JetStream interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiCall {
    /// The connection.
    pub connection: u64,
    /// The subject, under `$JS.API.`.
    pub subject: String,
}

/// One `CONNECT`, with how it signed in. Never the credential itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connect {
    /// The connection.
    pub connection: u64,
    /// `none`, `token`, or `user`.
    pub presented: &'static str,
    /// Whether the server let it in.
    pub accepted: bool,
    /// Whether it asked for headers.
    pub headers: bool,
    /// Whether it asked to be told when nobody listens.
    pub no_responders: bool,
    /// Whether it asked for the acknowledgement of every line.
    pub verbose: bool,
}

/// A fault, switched on until cleared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// Store a publish and send no acknowledgement.
    DropPublishAck,
    /// Store a publish and close the connection before answering.
    CloseAfterStore,
    /// Store a publish and answer with an error code the protocol does not have.
    UnknownCodeAfterStore,
    /// Refuse a publish with this error code; store nothing.
    RefusePublish {
        /// The JetStream error code.
        err_code: u16,
    },
    /// Carry out an acknowledgement and send no answer.
    DropAckReply,
    /// Carry out an acknowledgement and answer with a payload that has no rule.
    UnknownAckReply,
    /// Answer the next request with a protocol line the protocol does not have.
    UnknownLine,
    /// Hand over one message more than a pull asked for.
    DeliverExtra,
    /// Hand over a delivery whose acknowledgement subject names another
    /// consumer than the one pulled, as another client that sees the
    /// inbox could.
    ForeignDelivery,
    /// Store a publish, then send `-ERR 'User Authentication Expired'` and
    /// close the connection before the acknowledgement: the server's timer
    /// on a credential that expires fires while the publish is in flight.
    ExpireAuthAfterStore,
    /// Carry out an acknowledgement, then send `-ERR 'User Authentication
    /// Expired'` and close the connection before its answer.
    ExpireAuthAfterAck,
    /// Describe a consumer without its `ack_wait`.
    ConsumerWithoutAckWait,
}

/// One consumer, as the control interface reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerTruth {
    /// Its name.
    pub name: String,
    /// Whether a client made it and the server would discard it.
    pub ephemeral: bool,
    /// Its acknowledgement policy.
    pub ack: AckPolicy,
    /// Stream sequences delivered and not acknowledged.
    pub pending: Vec<u64>,
}

/// One stream, as the control interface reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamTruth {
    /// What was declared.
    pub spec: StreamSpec,
    /// What it holds, in order.
    pub messages: Vec<StoredMessage>,
    /// The last sequence it gave.
    pub last_seq: u64,
    /// Its consumers.
    pub consumers: Vec<ConsumerTruth>,
}

/// Everything the server received, did, and holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Truth {
    /// The streams, by name.
    pub streams: BTreeMap<String, StreamTruth>,
    /// The stand's clock, in milliseconds since 1970.
    pub now_ms: u64,
    /// The faults switched on.
    pub faults: Vec<Fault>,
    /// Every time a fault changed what the server did.
    pub applied: Vec<Fault>,
    /// Every publish, in order.
    pub received: Vec<Received>,
    /// Every delivery, in order.
    pub deliveries: Vec<Delivery>,
    /// Every acknowledgement, in order.
    pub acknowledgements: Vec<Acknowledgement>,
    /// Every JetStream request, in order.
    pub api_calls: Vec<ApiCall>,
    /// Every `CONNECT`, in order.
    pub connects: Vec<Connect>,
}
