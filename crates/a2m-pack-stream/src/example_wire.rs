//! The pack's first wire protocol: `example-broker/1`, the line protocol of
//! the test broker under `targets/example-broker`. One JSON object on one
//! line each way; one answer to each request, in order.
//!
//! This is a small client of the pack's own on Tokio. The pack does not use
//! the stand's client, so that what is proven is what would run.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::time::Duration;

use a2m_core::Secret;
use a2m_provider::{Deadline, Sent};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::channel::Shape;
use crate::json::Json;
use crate::wire::{
    ChannelFacts, Delivered, Link, Message, Opened, Outgoing, Page, Protocol, Receipt, Wire,
    WireError, WireErrorKind,
};

/// The protocol's name, as a surface states it.
pub(crate) const NAME: &str = "example-broker/1";

/// The name the pack gives itself in `hello`.
const CLIENT: &str = "anything2mcp";

/// The longest answer read, in bytes. A longer one is no answer of this
/// protocol.
const LONGEST_ANSWER: usize = 4 * 1024 * 1024;

/// The longest the broker waits in one fetch, in milliseconds.
const LONGEST_FETCH_WAIT_MS: u64 = 60_000;

fn error(kind: WireErrorKind, sent: Sent, detail: impl Into<String>) -> WireError {
    WireError {
        kind,
        sent,
        detail: detail.into(),
    }
}

/// An answer that is not one the protocol describes. The request may have
/// been carried out, so its fate is unknown.
fn unexpected(what: &str) -> WireError {
    error(WireErrorKind::Unexpected, Sent::Unknown, what)
}

/// The protocol.
pub(crate) struct ExampleProtocol;

impl Protocol for ExampleProtocol {
    fn name(&self) -> &'static str {
        NAME
    }

    fn connect<'a>(
        &'a self,
        address: &'a str,
        token: Option<&'a Secret>,
        deadline: Deadline,
    ) -> Wire<'a, Box<dyn Link>> {
        Box::pin(async move {
            let unreachable = || {
                error(
                    WireErrorKind::Unreachable,
                    Sent::NotSent,
                    "the broker did not accept a connection",
                )
            };
            let stream = tokio::time::timeout_at(deadline.at(), TcpStream::connect(address))
                .await
                .map_err(|_| unreachable())?
                .map_err(|_| unreachable())?;
            // The protocol is one short line each way; waiting to batch them
            // would only add delay. A failure to set this changes nothing.
            let _ = stream.set_nodelay(true);
            let mut link = ExampleLink {
                stream: BufReader::new(stream),
                next_id: 1,
                broken: false,
            };
            let mut fields = vec![("client", Json::text(CLIENT))];
            if let Some(token) = token {
                fields.push(("token", Json::text(token.expose())));
            }
            let reply = link.request("hello", fields, deadline).await?;
            if reply.get("protocol").and_then(Json::as_str) != Some(NAME) {
                return Err(unexpected("the broker speaks another protocol"));
            }
            Ok(Box::new(link) as Box<dyn Link>)
        })
    }
}

struct ExampleLink {
    stream: BufReader<TcpStream>,
    next_id: u64,
    /// True from the moment a request starts until its whole answer is
    /// read, and for ever after a failure of the connection. A link whose
    /// request was cut off cannot tell which answer belongs to which
    /// request, so it carries no further one.
    broken: bool,
}

impl ExampleLink {
    /// Reads one line of at most [`LONGEST_ANSWER`] bytes. `None` at the
    /// end of the stream.
    async fn read_line(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        loop {
            let buffer = self.stream.fill_buf().await?;
            if buffer.is_empty() {
                return Ok(None);
            }
            let end = buffer.iter().position(|byte| *byte == b'\n');
            let take = end.unwrap_or(buffer.len());
            if line.len() + take > LONGEST_ANSWER {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    "the answer is too long",
                ));
            }
            line.extend_from_slice(&buffer[..take]);
            self.stream.consume(end.map_or(take, |end| end + 1));
            if end.is_some() {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(line));
            }
        }
    }

    /// Sends one request and reads its answer. Gives the answer when the
    /// broker said `ok`.
    async fn request(
        &mut self,
        op: &str,
        fields: Vec<(&str, Json)>,
        deadline: Deadline,
    ) -> Result<Json, WireError> {
        let reply = self.exchange(op, fields, deadline).await?;
        match reply.get("ok").and_then(Json::as_bool) {
            Some(true) => Ok(reply),
            Some(false) => Err(refusal(&reply)),
            None => Err(unexpected("the broker's answer says neither yes nor no")),
        }
    }

    /// Sends one request and reads the one answer that belongs to it,
    /// whatever it says.
    async fn exchange(
        &mut self,
        op: &str,
        fields: Vec<(&str, Json)>,
        deadline: Deadline,
    ) -> Result<Json, WireError> {
        if !self.alive() {
            self.broken = true;
            return Err(error(
                WireErrorKind::Lost,
                Sent::NotSent,
                "the connection to the broker had ended",
            ));
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut request: BTreeMap<String, Json> = fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect();
        request.insert("id".to_owned(), Json::number(id));
        request.insert("op".to_owned(), Json::text(op));
        let mut line = Json::Object(request).to_line();
        line.push('\n');

        self.broken = true;
        let lost = || {
            error(
                WireErrorKind::Lost,
                Sent::Unknown,
                "the connection to the broker ended before it answered",
            )
        };
        let timed_out = || {
            error(
                WireErrorKind::TimedOut,
                Sent::Unknown,
                "the broker did not answer in time",
            )
        };
        let wrote = tokio::time::timeout_at(deadline.at(), async {
            self.stream.get_mut().write_all(line.as_bytes()).await?;
            self.stream.get_mut().flush().await
        })
        .await;
        match wrote {
            Err(_) => return Err(timed_out()),
            Ok(Err(_)) => return Err(lost()),
            Ok(Ok(())) => {}
        }
        let answer = match tokio::time::timeout_at(deadline.at(), self.read_line()).await {
            Err(_) => return Err(timed_out()),
            Ok(Err(failed)) if failed.kind() == ErrorKind::InvalidData => {
                return Err(unexpected(
                    "the broker's answer is longer than any answer may be",
                ));
            }
            Ok(Err(_) | Ok(None)) => return Err(lost()),
            Ok(Ok(Some(answer))) => answer,
        };
        let reply = std::str::from_utf8(&answer)
            .ok()
            .and_then(|text| Json::parse(text).ok())
            .filter(|reply| reply.as_object().is_some())
            .ok_or_else(|| unexpected("the broker's answer is not one JSON object"))?;
        if reply.get("re").and_then(Json::as_u64) != Some(id) {
            return Err(unexpected("the broker answered another request"));
        }
        // The answer is whole and is this request's: the link can go on.
        self.broken = false;
        Ok(reply)
    }
}

/// The broker's refusal, as this pack names it. The broker's words are
/// passed on whole: the driver redacts them before it cleans and cuts them.
fn refusal(reply: &Json) -> WireError {
    let code = reply
        .get("error")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let said = reply
        .get("message")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let kind = match code {
        "not_authorized" => WireErrorKind::NotAuthorized,
        "channel_missing" => WireErrorKind::ChannelMissing,
        "not_supported" | "key_required" => WireErrorKind::NotSupported,
        "message_too_large" => WireErrorKind::TooLarge,
        "refused" => WireErrorKind::Refused,
        "subscription_missing" => WireErrorKind::SubscriptionMissing,
        "subscription_in_use" => WireErrorKind::SubscriptionInUse,
        "position_out_of_range" => WireErrorKind::PositionOutOfRange,
        // A refusal under a code the protocol does not have says nothing
        // about whether the request was carried out.
        _ => {
            return error(
                WireErrorKind::Unexpected,
                Sent::Unknown,
                format!("the broker refused with the code `{code}`"),
            );
        }
    };
    error(kind, Sent::Answered, said)
}

fn text_of(value: Option<&Json>) -> Option<String> {
    value.and_then(Json::as_str).map(str::to_owned)
}

/// A field that is a whole number, or absent or null.
fn number_of(reply: &Json, key: &str) -> Result<Option<u64>, WireError> {
    match reply.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| unexpected("a count or a position is not a whole number")),
    }
}

fn message_of(value: &Json) -> Result<Message, WireError> {
    let malformed = || unexpected("a message is not in the protocol's form");
    let headers = match value.get("headers") {
        None | Some(Json::Null) => BTreeMap::new(),
        Some(headers) => headers
            .as_object()
            .ok_or_else(malformed)?
            .iter()
            .map(|(name, value)| value.as_str().map(|value| (name.clone(), value.to_owned())))
            .collect::<Option<BTreeMap<String, String>>>()
            .ok_or_else(malformed)?,
    };
    let key = match value.get("key") {
        None | Some(Json::Null) => None,
        Some(key) => Some(key.as_str().ok_or_else(malformed)?.to_owned()),
    };
    Ok(Message {
        id: text_of(value.get("id")).ok_or_else(malformed)?,
        key,
        headers,
        body: value.get("body").cloned().ok_or_else(malformed)?,
        position: number_of(value, "position")?,
    })
}

fn facts_of(value: &Json) -> Result<ChannelFacts, WireError> {
    let malformed = || unexpected("a channel description is not in the protocol's form");
    let subscriptions = match value.get("subscriptions") {
        None | Some(Json::Null) => Vec::new(),
        Some(list) => list
            .as_list()
            .ok_or_else(malformed)?
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(malformed)?,
    };
    Ok(ChannelFacts {
        address: text_of(value.get("address")).ok_or_else(malformed)?,
        shape: value
            .get("shape")
            .and_then(Json::as_str)
            .and_then(Shape::from_word),
        key_field: text_of(value.get("key_field")),
        schema: value
            .get("schema")
            .filter(|schema| !schema.is_null())
            .cloned(),
        dedupe_header: text_of(value.get("dedupe_header")),
        held: number_of(value, "held")?.unwrap_or(0),
        start: number_of(value, "start")?,
        end: number_of(value, "end")?,
        subscriptions,
    })
}

fn list_of<'a>(reply: &'a Json, key: &str) -> Result<&'a [Json], WireError> {
    reply
        .get(key)
        .and_then(Json::as_list)
        .ok_or_else(|| unexpected("a list the protocol promises is missing"))
}

impl Link for ExampleLink {
    fn alive(&self) -> bool {
        if self.broken || !self.stream.buffer().is_empty() {
            return false;
        }
        // The broker sends nothing unasked, so between requests a live
        // connection has nothing to read. Reading nothing means it ended.
        let mut probe = [0_u8; 1];
        matches!(
            self.stream.get_ref().try_read(&mut probe),
            Err(failed) if failed.kind() == ErrorKind::WouldBlock
        )
    }

    fn channels(&mut self, deadline: Deadline) -> Wire<'_, Vec<ChannelFacts>> {
        Box::pin(async move {
            let reply = self.request("channels", Vec::new(), deadline).await?;
            list_of(&reply, "channels")?.iter().map(facts_of).collect()
        })
    }

    fn describe<'a>(&'a mut self, address: &'a str, deadline: Deadline) -> Wire<'a, ChannelFacts> {
        Box::pin(async move {
            let fields = vec![("channel", Json::text(address))];
            let reply = self.request("describe", fields, deadline).await?;
            facts_of(
                reply
                    .get("channel")
                    .ok_or_else(|| unexpected("a description holds no channel"))?,
            )
        })
    }

    fn latest<'a>(
        &'a mut self,
        address: &'a str,
        key: &'a str,
        deadline: Deadline,
    ) -> Wire<'a, Option<Message>> {
        Box::pin(async move {
            let fields = vec![("channel", Json::text(address)), ("key", Json::text(key))];
            let reply = self.request("latest", fields, deadline).await?;
            match reply.get("message") {
                None | Some(Json::Null) => Ok(None),
                Some(message) => message_of(message).map(Some),
            }
        })
    }

    fn browse<'a>(
        &'a mut self,
        address: &'a str,
        from: Option<u64>,
        max: u32,
        deadline: Deadline,
    ) -> Wire<'a, Page> {
        Box::pin(async move {
            let mut fields = vec![
                ("channel", Json::text(address)),
                ("max", Json::number(u64::from(max))),
            ];
            if let Some(from) = from {
                fields.push(("from", Json::number(from)));
            }
            let reply = self.request("browse", fields, deadline).await?;
            Ok(Page {
                messages: list_of(&reply, "messages")?
                    .iter()
                    .map(message_of)
                    .collect::<Result<_, _>>()?,
                // Whether more is held is what a scan's "absent" rests on:
                // a look that does not say is no answer of this protocol.
                more: reply
                    .get("more")
                    .and_then(Json::as_bool)
                    .ok_or_else(|| unexpected("a look does not say whether more is held"))?,
                start: number_of(&reply, "start")?,
                end: number_of(&reply, "end")?,
            })
        })
    }

    fn subscribe<'a>(
        &'a mut self,
        address: &'a str,
        name: Option<&'a str>,
        from: Option<u64>,
        deadline: Deadline,
    ) -> Wire<'a, Opened> {
        Box::pin(async move {
            // A subscription of the run's own never asks for the values a
            // channel retains: a wait starts where the subscription does.
            let mut fields = vec![
                ("channel", Json::text(address)),
                ("with_retained", Json::Bool(false)),
            ];
            if let Some(name) = name {
                fields.push(("name", Json::text(name)));
            }
            if let Some(from) = from {
                fields.push(("from", Json::number(from)));
            }
            let reply = self.request("subscribe", fields, deadline).await?;
            Ok(Opened {
                handle: text_of(reply.get("subscription"))
                    .ok_or_else(|| unexpected("a subscription has no handle"))?,
                position: number_of(&reply, "position")?,
            })
        })
    }

    fn fetch<'a>(
        &'a mut self,
        handle: &'a str,
        max: u32,
        wait: Duration,
        deadline: Deadline,
    ) -> Wire<'a, Vec<Delivered>> {
        Box::pin(async move {
            let wait_ms = u64::try_from(wait.as_millis())
                .unwrap_or(LONGEST_FETCH_WAIT_MS)
                .min(LONGEST_FETCH_WAIT_MS);
            let fields = vec![
                ("subscription", Json::text(handle)),
                ("max", Json::number(u64::from(max))),
                ("wait_ms", Json::number(wait_ms)),
            ];
            let reply = self.request("fetch", fields, deadline).await?;
            list_of(&reply, "messages")?
                .iter()
                .map(|item| {
                    Ok(Delivered {
                        delivery: text_of(item.get("delivery"))
                            .ok_or_else(|| unexpected("a delivery has no tag"))?,
                        message: message_of(
                            item.get("message")
                                .ok_or_else(|| unexpected("a delivery has no message"))?,
                        )?,
                    })
                })
                .collect()
        })
    }

    fn acknowledge<'a>(
        &'a mut self,
        handle: &'a str,
        delivery: &'a str,
        deadline: Deadline,
    ) -> Wire<'a, ()> {
        Box::pin(async move {
            let fields = vec![
                ("subscription", Json::text(handle)),
                ("delivery", Json::text(delivery)),
            ];
            self.request("ack", fields, deadline).await.map(|_| ())
        })
    }

    fn release<'a>(
        &'a mut self,
        handle: &'a str,
        delivery: &'a str,
        deadline: Deadline,
    ) -> Wire<'a, ()> {
        Box::pin(async move {
            let fields = vec![
                ("subscription", Json::text(handle)),
                ("delivery", Json::text(delivery)),
            ];
            self.request("release", fields, deadline).await.map(|_| ())
        })
    }

    fn unsubscribe<'a>(&'a mut self, handle: &'a str, deadline: Deadline) -> Wire<'a, ()> {
        Box::pin(async move {
            let fields = vec![("subscription", Json::text(handle))];
            self.request("unsubscribe", fields, deadline)
                .await
                .map(|_| ())
        })
    }

    fn publish<'a>(&'a mut self, message: &'a Outgoing, deadline: Deadline) -> Wire<'a, Receipt> {
        Box::pin(async move {
            let headers = message
                .headers
                .iter()
                .map(|(name, value)| (name.clone(), Json::text(value.as_str())))
                .collect();
            let mut fields = vec![
                ("channel", Json::text(message.address.as_str())),
                ("headers", Json::Object(headers)),
                ("body", message.body.clone()),
            ];
            if let Some(key) = &message.key {
                fields.push(("key", Json::text(key.as_str())));
            }
            let reply = self.exchange("publish", fields, deadline).await?;
            let duplicate = match reply.get("ok").and_then(Json::as_bool) {
                Some(true) => false,
                // The one refusal that is good news: the broker already
                // holds a message with this token, and stored nothing again.
                Some(false)
                    if reply.get("error").and_then(Json::as_str) == Some("duplicate_token") =>
                {
                    true
                }
                Some(false) => return Err(refusal(&reply)),
                None => return Err(unexpected("the broker's answer says neither yes nor no")),
            };
            Ok(Receipt {
                id: text_of(reply.get("id"))
                    .ok_or_else(|| unexpected("a confirmation names no message"))?,
                position: number_of(&reply, "position")?,
                duplicate,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test stops when its helper cannot work"
    )]

    use tokio::net::TcpListener;

    use super::*;

    /// A broker of one connection that answers each request with the next
    /// of `answers`, after the request's own `re`.
    async fn answering(answers: Vec<&'static str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            for (number, answer) in answers.into_iter().enumerate() {
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                let reply = format!("{{\"re\":{},{answer}}}\n", number + 1);
                stream.get_mut().write_all(reply.as_bytes()).await.unwrap();
            }
        });
        address
    }

    #[tokio::test]
    async fn an_answer_with_no_rule_has_an_unknown_fate() {
        let address = answering(vec![
            r#""ok":true,"protocol":"example-broker/1""#,
            // A look that does not say whether more is held.
            r#""ok":true,"messages":[]"#,
            // A refusal under a code the protocol does not have.
            r#""ok":false,"error":"replica_timeout","message":"later""#,
        ])
        .await;
        let deadline = Deadline::after(Duration::from_secs(10));
        let mut link = ExampleProtocol
            .connect(&address, None, deadline)
            .await
            .unwrap();
        let look = link
            .browse("example.orders.events", None, 5, deadline)
            .await
            .unwrap_err();
        assert_eq!(
            (look.kind, look.sent),
            (WireErrorKind::Unexpected, Sent::Unknown)
        );
        let refused = link.acknowledge("s-1", "d-1", deadline).await.unwrap_err();
        assert_eq!(
            (refused.kind, refused.sent),
            (WireErrorKind::Unexpected, Sent::Unknown)
        );
    }
}
