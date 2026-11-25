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
    /// The connection would carry the session where this build does not
    /// send it: plain text to an address that is not this machine's own,
    /// or to a server that asks for TLS.
    Transport,
    /// The protocol cannot carry what the step would send: an address, a
    /// key, or a header it has no form for.
    Unfit,
}

/// Where a plain TCP connection may go: a loopback address written as an
/// IP address, `127.0.0.0/8` or `::1`, with its port (decision 0197, by the
/// rule of decision 0188). A name, `localhost` included, is never one: the
/// resolver decides where a name leads. `::ffff:127.0.0.1`, `0.0.0.0`,
/// `::`, and an address with a zone (`[::1%1]:4222`) are not one either.
/// The address is judged by `a2m_core::ScopedHost::may_leave_this_machine`,
/// the one question every check of a plain link asks (decision 0240). Every
/// protocol of the pack dials only what this returns, so the session's
/// credential never crosses a plain link to another machine, and nothing is
/// resolved or sent before the check.
pub(crate) fn plain_tcp_address(address: &str) -> Result<std::net::SocketAddr, WireError> {
    match address.parse::<std::net::SocketAddr>() {
        Ok(found) if !a2m_core::ScopedHost::of_socket_address(found).may_leave_this_machine() => {
            Ok(found)
        }
        _ => Err(WireError {
            kind: WireErrorKind::Transport,
            sent: Sent::NotSent,
            detail: "plain TCP is used only to this machine's loopback address, written as an IP address"
                .to_owned(),
        }),
    }
}

/// The session's credential, as a wire protocol presents it when a
/// connection opens. It is used for that and kept nowhere.
pub(crate) enum Credential {
    /// One token.
    Token(Secret),
    /// A user name and a password.
    UserPassword {
        /// The user's name; not a secret.
        user: String,
        /// The password.
        password: Secret,
    },
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
    /// Whether the broker keys a message by the last token of the subject
    /// it is published on, below the channel's own subject: what the
    /// channel's configuration states, as a stream's subjects do. A broker
    /// that keys so takes no field of the body as the key.
    pub key_from_subject: bool,
    /// A schema the broker holds for the body: a claim, not a check.
    pub schema: Option<Json>,
    /// The header whose repeated value the broker refuses.
    pub dedupe_header: Option<String>,
    /// How long the broker remembers a token, in milliseconds; nothing when
    /// it does not say, which a broker that forgets none may leave out.
    pub dedupe_window_ms: Option<u64>,
    /// How many messages it holds now.
    pub held: u64,
    /// A log's first held position.
    pub start: Option<u64>,
    /// A log's next position.
    pub end: Option<u64>,
    /// The subscriptions declared on it, by name.
    pub subscriptions: Vec<String>,
    /// Whether the broker's own configuration of the channel lets it lose a
    /// message from inside its range between a publish and a later look: a
    /// limit per key that evicts, a time to live per message, or a
    /// retention that removes what nobody reads. A look at such a channel
    /// never says a message is absent.
    pub may_lose_inside: bool,
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
    /// On a queue, how long the broker waits for an acknowledgement of a
    /// delivery before it hands the message out again, as the declared
    /// consumer states it. Nothing only where the protocol has no such
    /// wait: the test broker's hands a message out again only when it is
    /// given back or its connection ends. A protocol that has one and a
    /// broker that does not state it is an answer with no rule.
    pub ack_wait: Option<Duration>,
}

/// One delivery of one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Delivered {
    /// The tag an acknowledgement names.
    pub delivery: String,
    /// The message.
    pub message: Message,
    /// How many times the broker has handed this message out, this one
    /// included, as the delivery's own metadata says; nothing where the
    /// protocol does not say. Above one, or unstated, an earlier delivery
    /// may be held by another record, and a take treats it so.
    pub attempt: Option<u64>,
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

    /// Whether the protocol can present this credential.
    fn carries(&self, credential: &Credential) -> bool;

    /// Opens a connection and presents the session's credential, when there
    /// is one. The credential is used for this and kept nowhere.
    fn connect<'a>(
        &'a self,
        address: &'a str,
        credential: Option<&'a Credential>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The corpus of address spellings every check of where an address is
    /// is pinned against (decision 0240).
    const SPELLINGS: &str = include_str!("../../a2m-core/tests/address-spellings.txt");

    /// Each spelling of the corpus, with the verdict of `column` now and
    /// before decision 0240.
    fn corpus(column: &str) -> Vec<(String, String, String)> {
        let mut header: Vec<&str> = Vec::new();
        let mut rows = Vec::new();
        for line in SPELLINGS.lines() {
            if let Some(names) = line.strip_prefix("#spelling") {
                header = std::iter::once("spelling")
                    .chain(names.split_whitespace())
                    .collect();
            }
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let cells: Vec<&str> = line.split_whitespace().collect();
            let at = header.iter().position(|name| *name == column).unwrap();
            let spelling = cells[0].replace("<empty>", "").replace("<sp>", " ");
            let (now, before) = cells[at].split_once('<').unwrap_or((cells[at], cells[at]));
            rows.push((spelling, now.to_owned(), before.to_owned()));
        }
        assert!(rows.len() > 50, "the corpus was not read");
        rows
    }

    /// `plain_tcp_address`'s test of an address as it was before decision
    /// 0240, kept to show that nothing it refused is allowed now.
    fn before(address: &str) -> bool {
        matches!(address.parse::<std::net::SocketAddr>(), Ok(found) if found.ip().is_loopback())
    }

    fn verdict(allowed: bool) -> &'static str {
        if allowed { "here" } else { "away" }
    }

    #[test]
    fn every_spelling_of_the_shared_corpus_is_judged_as_it_is_pinned() {
        for (spelling, now, before_then) in corpus("stream") {
            assert_eq!(
                verdict(plain_tcp_address(&spelling).is_ok()),
                now,
                "{spelling}"
            );
            assert_eq!(verdict(before(&spelling)), before_then, "{spelling}");
        }
    }

    #[test]
    fn no_spelling_refused_before_the_one_question_is_allowed_after_it() {
        for (spelling, _, _) in corpus("stream") {
            if !before(&spelling) {
                assert!(plain_tcp_address(&spelling).is_err(), "{spelling}");
            }
        }
    }

    /// The one tightening of decision 0240 in this pack: a zone on the
    /// loopback address names an interface, and what a connection to `::1`
    /// through another interface does is no standard's that was read here;
    /// unknown means stop.
    #[test]
    fn a_loopback_address_with_a_zone_was_allowed_and_is_refused() {
        assert!(before("[::1%1]:4222"));
        let refused = plain_tcp_address("[::1%1]:4222").unwrap_err();
        assert_eq!(
            (refused.kind, refused.sent),
            (WireErrorKind::Transport, Sent::NotSent)
        );
    }

    #[test]
    fn plain_tcp_goes_only_to_a_loopback_address_written_as_an_ip_address() {
        for good in ["127.0.0.1:4222", "127.5.6.7:1", "[::1]:7400"] {
            assert!(plain_tcp_address(good).is_ok(), "{good}");
        }
        for bad in [
            "localhost:4222",
            "localhost.:4222",
            "broker.internal:7400",
            "[::ffff:127.0.0.1]:4222",
            "0.0.0.0:4222",
            "[::]:4222",
            "127.1:4222",
            "192.0.2.10:4222",
            "127.0.0.1",
            "",
        ] {
            let refused = plain_tcp_address(bad).unwrap_err();
            assert_eq!(
                (refused.kind, refused.sent),
                (WireErrorKind::Transport, Sent::NotSent),
                "{bad}"
            );
        }
    }
}
