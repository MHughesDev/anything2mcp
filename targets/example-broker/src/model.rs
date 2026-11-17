//! What travels on the wire and what the control interface reports.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::faults::{Fault, FaultKind};

/// What a channel does with a message it accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Each message goes to one consumer and is removed when acknowledged.
    Queue,
    /// Every subscription gets a copy. Nothing is kept.
    Topic,
    /// Every subscription gets a copy, and the last value per key is kept.
    Retained,
    /// Messages are kept in order, each at a position the broker reports.
    Log,
}

/// How a channel is declared on the broker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelSpec {
    /// The name of the channel on the broker.
    pub address: String,
    /// What the channel does with a message.
    pub shape: Shape,
    /// The field of the body the broker says the key is taken from. A report, not a check.
    #[serde(default)]
    pub key_field: Option<String>,
    /// A registered schema for the body. A claim: the broker does not check messages against it.
    #[serde(default)]
    pub schema: Option<Value>,
    /// The header whose repeated value makes the broker refuse a publish. Absent: no refusal.
    #[serde(default)]
    pub dedupe_header: Option<String>,
    /// The largest body the channel accepts, in bytes of its canonical JSON.
    pub max_body_bytes: usize,
}

impl ChannelSpec {
    /// A channel with no key field, no schema, no refusal of repeats, and a 65536-byte limit.
    #[must_use]
    pub fn new(address: &str, shape: Shape) -> Self {
        Self {
            address: address.to_owned(),
            shape,
            key_field: None,
            schema: None,
            dedupe_header: None,
            max_body_bytes: 65536,
        }
    }

    /// The same channel, reporting this key field.
    #[must_use]
    pub fn key_field(mut self, field: &str) -> Self {
        self.key_field = Some(field.to_owned());
        self
    }

    /// The same channel, holding this registered schema.
    #[must_use]
    pub fn schema(mut self, schema: Value) -> Self {
        self.schema = Some(schema);
        self
    }

    /// The same channel, refusing a publish that repeats a value of this header.
    #[must_use]
    pub fn dedupe_header(mut self, header: &str) -> Self {
        self.dedupe_header = Some(header.to_owned());
        self
    }
}

/// One message as the broker holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// The identifier the broker gave it: `m-1`, `m-2`, and so on; seed messages are `s-1` and up.
    pub id: String,
    /// The channel it was published to.
    pub channel: String,
    /// The message key, if it has one.
    #[serde(default)]
    pub key: Option<String>,
    /// Headers, by name.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// The body: any JSON value.
    pub body: Value,
    /// The position in a log. Absent on every other shape.
    #[serde(default)]
    pub position: Option<u64>,
}

/// A request a client can make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Say who is connecting, and present a token.
    Hello,
    /// List the channels.
    Channels,
    /// Describe one channel.
    Describe,
    /// Publish one message.
    Publish,
    /// Read the retained value for a key.
    Latest,
    /// Look at what a channel holds without taking anything.
    Browse,
    /// Open a subscription, or attach a consumer to a queue.
    Subscribe,
    /// Read at most so many messages, waiting at most so long.
    Fetch,
    /// Acknowledge a delivery from a queue.
    Ack,
    /// Give a delivery back to its queue.
    Release,
    /// Close a subscription.
    Unsubscribe,
}

impl Op {
    /// The request named by `name`, if there is one.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "hello" => Self::Hello,
            "channels" => Self::Channels,
            "describe" => Self::Describe,
            "publish" => Self::Publish,
            "latest" => Self::Latest,
            "browse" => Self::Browse,
            "subscribe" => Self::Subscribe,
            "fetch" => Self::Fetch,
            "ack" => Self::Ack,
            "release" => Self::Release,
            "unsubscribe" => Self::Unsubscribe,
            _ => return None,
        })
    }
}

/// What became of one publish.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PublishOutcome {
    /// The broker stored the message.
    Stored {
        /// The identifier the broker gave it.
        id: String,
        /// Its position, on a log.
        position: Option<u64>,
        /// Whether it became the retained value for its key.
        retained: bool,
    },
    /// The broker refused it as a repeat of a message it already holds. Nothing was stored.
    Duplicate {
        /// The identifier of the message first stored under the same token.
        of: String,
    },
    /// The broker refused it. Nothing was stored.
    Refused {
        /// The error code the client was sent.
        code: String,
    },
    /// The connection was dropped before the message was stored.
    Dropped,
}

/// One publish the broker received, whatever became of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Received {
    /// Its place in the order of everything the broker recorded.
    pub seq: u64,
    /// The connection it came in on: `c-1`, `c-2`, and so on.
    pub connection: String,
    /// The name that connection gave in its `hello`.
    pub client: String,
    /// The channel named in the request.
    pub channel: String,
    /// The key sent.
    pub key: Option<String>,
    /// The headers sent.
    pub headers: BTreeMap<String, String>,
    /// The body sent.
    pub body: Value,
    /// [`body_digest`] of the body.
    pub body_digest: String,
    /// What became of it.
    #[serde(flatten)]
    pub outcome: PublishOutcome,
    /// Whether the broker sent an answer. False when a fault dropped the answer or the connection.
    pub answered: bool,
}

/// Why a message was handed to a subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryCause {
    /// Its first delivery.
    First,
    /// It was delivered before and came back unacknowledged.
    Redelivery,
    /// The duplicate-delivery fault handed it over a second time.
    Duplicate,
    /// It was the retained value when the subscription opened.
    Retained,
}

/// What became of one delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryOutcome {
    /// Delivered from a queue and neither acknowledged nor given back yet.
    Unacknowledged,
    /// The consumer acknowledged it. The message is gone from the queue.
    Acknowledged,
    /// The consumer gave it back. It is in the queue again.
    Released,
    /// The broker took it back: the consumer went away, or redelivery was forced.
    Returned,
    /// Not from a queue. No acknowledgement is asked for.
    NotRequired,
}

/// One delivery: which message went to whom.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    /// Its place in the order of everything the broker recorded.
    pub seq: u64,
    /// The delivery tag: `d-1`, `d-2`, and so on.
    pub delivery: String,
    /// The identifier of the message delivered.
    pub message: String,
    /// The channel.
    pub channel: String,
    /// The subscription, or on a queue the consumer, by name.
    pub subscription: String,
    /// The connection it went to. Absent for the stand's own silent consumer.
    pub connection: Option<String>,
    /// How many times the broker counts this message as delivered, this one included.
    pub attempt: u32,
    /// Why it was delivered.
    pub cause: DeliveryCause,
    /// What became of it.
    pub outcome: DeliveryOutcome,
}

/// One acknowledgement the broker accepted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acknowledgement {
    /// Its place in the order of everything the broker recorded.
    pub seq: u64,
    /// The delivery acknowledged.
    pub delivery: String,
    /// The message that delivery carried.
    pub message: String,
    /// The channel.
    pub channel: String,
    /// The consumer, by name.
    pub subscription: String,
    /// The connection that acknowledged.
    pub connection: String,
}

/// One request the broker received.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestRecord {
    /// Its place in the order of everything the broker recorded.
    pub seq: u64,
    /// The connection it came in on.
    pub connection: String,
    /// The name that connection had given.
    pub client: String,
    /// The request.
    pub op: Op,
    /// The channel it was about, where it was about one.
    pub channel: Option<String>,
}

/// One time a fault changed what the broker did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedFault {
    /// Its place in the order of everything the broker recorded.
    pub seq: u64,
    /// The fault.
    pub kind: FaultKind,
    /// The request it applied to. Absent for the silent consumer, which answers no request.
    pub op: Option<Op>,
    /// The channel.
    pub channel: Option<String>,
    /// The connection whose request it applied to.
    pub connection: Option<String>,
}

/// Where a message in a queue stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeldState {
    /// Waiting for a consumer.
    Ready,
    /// Delivered to a consumer and not acknowledged.
    InFlight,
}

/// One message a queue holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    /// The message.
    pub message: Message,
    /// Where it stands.
    pub state: HeldState,
    /// How many times the broker counts it as delivered.
    pub attempts: u32,
    /// The consumer holding it, when in flight.
    pub held_by: Option<String>,
    /// The delivery tag it is held under, when in flight.
    pub delivery: Option<String>,
}

/// One subscription on a channel that is not a queue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionState {
    /// Its name.
    pub name: String,
    /// Whether it outlives the connection that holds it.
    pub durable: bool,
    /// The connection that holds it now.
    pub held_by: Option<String>,
    /// Identifiers of the messages waiting in it, oldest first.
    pub pending: Vec<String>,
    /// On a log, the position it reads next.
    pub position: Option<u64>,
}

/// Everything one channel holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelContents {
    /// How the channel is declared.
    pub spec: ChannelSpec,
    /// A queue's messages, in flight first and then ready, each in order.
    pub queue: Vec<Held>,
    /// The retained value for each key.
    pub retained: BTreeMap<String, Message>,
    /// A log's messages, in order.
    pub log: Vec<Message>,
    /// The first position a log still holds.
    pub log_start: u64,
    /// The position a log gives its next message.
    pub log_end: u64,
    /// The subscriptions on a channel that is not a queue.
    pub subscriptions: Vec<SubscriptionState>,
}

/// One open connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// `c-1`, `c-2`, and so on.
    pub id: String,
    /// The name it gave in its `hello`, or `anonymous`.
    pub client: String,
}

/// Ground truth: everything the broker received, did, and holds, at one moment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Truth {
    /// Every publish received, in order.
    pub received: Vec<Received>,
    /// Every delivery, in order.
    pub deliveries: Vec<Delivery>,
    /// Every acknowledgement accepted, in order.
    pub acknowledgements: Vec<Acknowledgement>,
    /// Every request received, in order.
    pub requests: Vec<RequestRecord>,
    /// What each channel holds, by address.
    pub channels: BTreeMap<String, ChannelContents>,
    /// The faults that are switched on.
    pub faults: Vec<Fault>,
    /// Every time a fault changed what the broker did, in order.
    pub applied: Vec<AppliedFault>,
    /// The connections that are open.
    pub connections: Vec<ConnectionInfo>,
}

impl Truth {
    /// The publishes received for one channel, in order.
    #[must_use]
    pub fn received_on(&self, channel: &str) -> Vec<&Received> {
        self.received
            .iter()
            .filter(|r| r.channel == channel)
            .collect()
    }

    /// The deliveries of one message, in order.
    #[must_use]
    pub fn deliveries_of(&self, message: &str) -> Vec<&Delivery> {
        self.deliveries
            .iter()
            .filter(|d| d.message == message)
            .collect()
    }
}

/// A value as JSON text with object keys in order and no spaces: the same text for the same value.
#[must_use]
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(fields) => {
            let mut names: Vec<&String> = fields.keys().collect();
            names.sort();
            out.push('{');
            for (index, name) in names.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(name.clone()).to_string());
                out.push(':');
                write_canonical(&fields[name], out);
            }
            out.push('}');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// The SHA-256 of [`canonical_json`] of a body, in lowercase hexadecimal.
#[must_use]
pub fn body_digest(body: &Value) -> String {
    let digest = Sha256::digest(canonical_json(body).as_bytes());
    let mut out = String::with_capacity(64);
    for byte in digest {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}
