//! The one small interface a wire protocol fills inside the pack: connect,
//! describe, look, bounded fetch, acknowledge, release, publish with a
//! token. The driver and the discoverer speak only this; which protocol is
//! behind it is named by the surface (decision 0104).
//!
//! What the interface requires of any protocol:
//!
//! - every call ends by its deadline, and a fetch returns at most `max`
//!   messages after at most `wait`;
//! - a look ([`Link::latest`], [`Link::browse`], [`Link::describe`],
//!   [`Link::channels`]) takes nothing from anyone;
//! - every failure says whether the request was not sent, was answered, or
//!   has an unknown fate; an answer the protocol has no rule for, a refusal
//!   under an unknown code included, has an unknown fate;
//! - a look says whether the channel holds more than it returned;
//! - a publish carries its headers unchanged, so the record's token reaches
//!   the broker in the header the channel declares.

use std::collections::BTreeMap;
use std::time::Duration;

use a2m_core::Secret;
use a2m_provider::{BoxFuture, Deadline, Sent};

use crate::channel::Shape;
use crate::json::Json;

/// What went wrong on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WireErrorKind {
    /// The broker could not be reached.
    Unreachable,
    /// The connection ended.
    Lost,
    /// The deadline passed.
    TimedOut,
    /// The broker refused the credentials.
    NotAuthorized,
    /// The broker has no such channel.
    ChannelMissing,
    /// The request does not fit the channel's shape.
    NotSupported,
    /// The broker refused the message, with its reason.
    Refused,
    /// The message is larger than the channel accepts.
    TooLarge,
    /// The named subscription is not declared on the broker.
    SubscriptionMissing,
    /// Another connection holds the named subscription.
    SubscriptionInUse,
    /// The position is not one the log holds.
    PositionOutOfRange,
    /// An answer the protocol has no rule for.
    Unexpected,
}

/// A failed call, with whether the request left and what the broker said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WireError {
    /// What went wrong.
    pub kind: WireErrorKind,
    /// Whether the request left, and whether its fate is known.
    pub sent: Sent,
    /// The broker's own words where it gave some, whole and as it said
    /// them; else a fixed phrase. Never a credential the pack sent. The
    /// driver redacts this before it cleans and cuts it for a message.
    pub detail: String,
}

/// What a broker reports about one channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChannelFacts {
    /// The channel as the broker names it.
    pub address: String,
    /// Its shape; nothing when the broker names one this pack does not know.
    pub shape: Option<Shape>,
    /// The field of the body the broker says the key is taken from.
    pub key_field: Option<String>,
    /// A schema the broker holds for the body: a claim, not a check.
    pub schema: Option<Json>,
    /// The header whose repeated value the broker refuses.
    pub dedupe_header: Option<String>,
    /// How many messages it holds now.
    pub held: u64,
    /// A log's first held position.
    pub start: Option<u64>,
    /// A log's next position.
    pub end: Option<u64>,
    /// The subscriptions declared on it, by name.
    pub subscriptions: Vec<String>,
}

/// One message as a broker holds or delivers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    /// The identifier the broker gave it.
    pub id: String,
    /// Its key.
    pub key: Option<String>,
    /// Its headers.
    pub headers: BTreeMap<String, String>,
    /// Its body.
    pub body: Json,
    /// Its position, on a log.
    pub position: Option<u64>,
}

/// One bounded look at a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Page {
    /// The messages, in the channel's order.
    pub messages: Vec<Message>,
    /// Whether the channel holds more than was returned.
    pub more: bool,
    /// A log's first held position.
    pub start: Option<u64>,
    /// A log's next position.
    pub end: Option<u64>,
}

/// An open subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Opened {
    /// The handle later calls name it by.
    pub handle: String,
    /// On a log, the position it stands at.
    pub position: Option<u64>,
}

/// One delivery of one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Delivered {
    /// The tag an acknowledgement names.
    pub delivery: String,
    /// The message.
    pub message: Message,
}

/// One message to publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outgoing {
    /// The channel as the broker names it.
    pub address: String,
    /// The key.
    pub key: Option<String>,
    /// The headers, the record's token among them where the channel
    /// declares its header.
    pub headers: BTreeMap<String, String>,
    /// The body.
    pub body: Json,
}

/// The broker's confirmation of a publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Receipt {
    /// The identifier of the stored message.
    pub id: String,
    /// Its position, on a log.
    pub position: Option<u64>,
    /// True when the broker refused this publish as a repeat of a token it
    /// already stored: `id` and `position` are then those of the message
    /// first stored, and nothing was stored again.
    pub duplicate: bool,
}

/// The result of one call on a link.
pub(crate) type Wire<'a, T> = BoxFuture<'a, Result<T, WireError>>;

/// A wire protocol: how a connection to a broker is opened.
pub(crate) trait Protocol: Send + Sync {
    /// The protocol's name, as a surface states it.
    fn name(&self) -> &'static str;

    /// Opens a connection and presents the session's token, when there is
    /// one. The token is used for this and kept nowhere.
    fn connect<'a>(
        &'a self,
        address: &'a str,
        token: Option<&'a Secret>,
        deadline: Deadline,
    ) -> Wire<'a, Box<dyn Link>>;
}

/// One open connection to a broker.
pub(crate) trait Link: Send {
    /// From memory and without waiting: whether the connection can still
    /// carry a request.
    fn alive(&self) -> bool;

    /// Every channel the broker lists. Takes nothing.
    fn channels(&mut self, deadline: Deadline) -> Wire<'_, Vec<ChannelFacts>>;

    /// What the broker reports about one channel. Takes nothing.
    fn describe<'a>(&'a mut self, address: &'a str, deadline: Deadline) -> Wire<'a, ChannelFacts>;

    /// The retained value for a key, or nothing. Takes nothing.
    fn latest<'a>(
        &'a mut self,
        address: &'a str,
        key: &'a str,
        deadline: Deadline,
    ) -> Wire<'a, Option<Message>>;

    /// At most `max` of the messages a channel holds, on a log from the
    /// position `from`. Takes nothing.
    fn browse<'a>(
        &'a mut self,
        address: &'a str,
        from: Option<u64>,
        max: u32,
        deadline: Deadline,
    ) -> Wire<'a, Page>;

    /// Opens a subscription: the declared one called `name`, or, with no
    /// name, one of the connection's own that the broker discards. On a
    /// log, `from` is where it starts. On a queue it attaches a consumer.
    fn subscribe<'a>(
        &'a mut self,
        address: &'a str,
        name: Option<&'a str>,
        from: Option<u64>,
        deadline: Deadline,
    ) -> Wire<'a, Opened>;

    /// At most `max` messages, after waiting at most `wait` for the first.
    /// An empty list is an answer.
    fn fetch<'a>(
        &'a mut self,
        handle: &'a str,
        max: u32,
        wait: Duration,
        deadline: Deadline,
    ) -> Wire<'a, Vec<Delivered>>;

    /// Acknowledges one delivery of a queue: the message is gone.
    fn acknowledge<'a>(
        &'a mut self,
        handle: &'a str,
        delivery: &'a str,
        deadline: Deadline,
    ) -> Wire<'a, ()>;

    /// Gives one delivery of a queue back, for redelivery.
    fn release<'a>(
        &'a mut self,
        handle: &'a str,
        delivery: &'a str,
        deadline: Deadline,
    ) -> Wire<'a, ()>;

    /// Lets a subscription go. What it held unacknowledged goes back.
    fn unsubscribe<'a>(&'a mut self, handle: &'a str, deadline: Deadline) -> Wire<'a, ()>;

    /// Publishes one message and waits for the broker's confirmation.
    fn publish<'a>(&'a mut self, message: &'a Outgoing, deadline: Deadline) -> Wire<'a, Receipt>;
}
