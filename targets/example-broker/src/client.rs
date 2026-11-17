//! A small ordinary client, for a test that acts as something outside the product:
//! another producer, another consumer, or the test itself.
//!
//! The stream pack does not use this client on the path of a run; it has its own.

use std::collections::BTreeMap;
use std::fmt;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::model::{ChannelSpec, HeldState, Message};

/// Why a request did not get a good answer.
#[derive(Debug)]
pub enum ClientError {
    /// The connection failed in some other way than closing.
    Io(std::io::Error),
    /// The broker closed the connection, or dropped it, before answering.
    Closed,
    /// The broker answered and refused.
    Refused {
        /// The error code, such as `channel_missing`.
        code: String,
        /// The broker's reason.
        message: String,
        /// The whole answer, for the fields some refusals add.
        reply: Value,
    },
    /// The broker answered with something this client has no rule for.
    Protocol(String),
}

impl ClientError {
    /// The broker's error code, if the broker refused.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Refused { code, .. } => Some(code),
            _ => None,
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "the connection to the broker failed: {e}"),
            Self::Closed => write!(f, "the broker closed the connection before answering"),
            Self::Refused { code, message, .. } => write!(f, "the broker refused: {code}: {message}"),
            Self::Protocol(what) => write!(f, "the broker's answer was not understood: {what}"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        // A dropped connection shows up differently on each operating system.
        match e.kind() {
            ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof => Self::Closed,
            _ => Self::Io(e),
        }
    }
}

/// One message to publish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Publish {
    /// The channel.
    pub channel: String,
    /// The message key.
    pub key: Option<String>,
    /// Headers, by name.
    pub headers: BTreeMap<String, String>,
    /// The body.
    pub body: Value,
    /// On a retained channel, whether the value is kept. The broker keeps it if absent.
    pub retain: Option<bool>,
}

impl Publish {
    /// A message with a body and nothing else.
    #[must_use]
    pub fn new(channel: &str, body: Value) -> Self {
        Self {
            channel: channel.to_owned(),
            key: None,
            headers: BTreeMap::new(),
            body,
            retain: None,
        }
    }

    /// The same message, with this key.
    #[must_use]
    pub fn key(mut self, key: &str) -> Self {
        self.key = Some(key.to_owned());
        self
    }

    /// The same message, with this header.
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_owned(), value.to_owned());
        self
    }

    /// The same message, kept or not kept as the retained value.
    #[must_use]
    pub fn retain(mut self, retain: bool) -> Self {
        self.retain = Some(retain);
        self
    }

    fn fields(&self) -> Value {
        json!({
            "channel": self.channel,
            "key": self.key,
            "headers": self.headers,
            "body": self.body,
            "retain": self.retain,
        })
    }
}

/// The broker's confirmation of a publish.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Published {
    /// The identifier the broker gave the message.
    pub id: String,
    /// Its position, on a log.
    #[serde(default)]
    pub position: Option<u64>,
}

/// A channel as the broker describes it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ChannelInfo {
    /// How it is declared.
    #[serde(flatten)]
    pub spec: ChannelSpec,
    /// How many messages it holds now.
    pub held: u64,
    /// On a log, the first position still held.
    #[serde(default)]
    pub start: Option<u64>,
    /// On a log, the position the next message gets.
    #[serde(default)]
    pub end: Option<u64>,
    /// The durable subscriptions declared on it.
    pub subscriptions: Vec<String>,
}

/// One message seen by a browse.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Browsed {
    /// The message.
    #[serde(flatten)]
    pub message: Message,
    /// On a queue, where it stands.
    #[serde(default)]
    pub state: Option<HeldState>,
    /// On a queue, how many times the broker counts it as delivered.
    #[serde(default)]
    pub attempts: Option<u32>,
}

/// What a browse saw. Nothing was taken.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Browse {
    /// The messages, in the channel's order, up to the number asked for.
    pub messages: Vec<Browsed>,
    /// Whether the channel holds more than were returned.
    pub more: bool,
    /// How many messages the channel holds.
    pub held: u64,
    /// On a log, the first position still held.
    #[serde(default)]
    pub start: Option<u64>,
    /// On a log, the position the next message gets.
    #[serde(default)]
    pub end: Option<u64>,
}

/// An open subscription.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Subscribed {
    /// The handle to fetch with: `s-1`, `s-2`, and so on.
    pub subscription: String,
    /// The subscription's name on the broker, or on a queue the consumer's name.
    pub name: String,
    /// On a log, the position it reads next.
    #[serde(default)]
    pub position: Option<u64>,
}

/// One message handed over by a fetch.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Delivered {
    /// The delivery tag, to acknowledge or give back with.
    pub delivery: String,
    /// How many times the broker counts the message as delivered.
    pub attempt: u32,
    /// Whether it was the retained value when the subscription opened.
    pub retained: bool,
    /// The message.
    pub message: Message,
}

#[derive(Deserialize)]
struct ChannelList {
    channels: Vec<ChannelInfo>,
}

#[derive(Deserialize)]
struct OneChannel {
    channel: ChannelInfo,
}

#[derive(Deserialize)]
struct Latest {
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Fetched {
    messages: Vec<Delivered>,
}

fn typed<T: DeserializeOwned>(reply: Value) -> Result<T, ClientError> {
    serde_json::from_value(reply).map_err(|e| ClientError::Protocol(e.to_string()))
}

/// One connection to a broker.
#[derive(Debug)]
pub struct Client {
    lines: Lines<BufReader<OwnedReadHalf>>,
    write: OwnedWriteHalf,
    next_id: u64,
}

impl Client {
    /// Connects and says `hello` under this name.
    ///
    /// # Errors
    ///
    /// Returns an error if the broker cannot be reached or refuses.
    pub async fn connect(address: SocketAddr, name: &str) -> Result<Self, ClientError> {
        Self::connect_with(address, name, None).await
    }

    /// Connects and says `hello` under this name, presenting a token.
    ///
    /// # Errors
    ///
    /// Returns an error if the broker cannot be reached or refuses.
    pub async fn connect_with(
        address: SocketAddr,
        name: &str,
        token: Option<&str>,
    ) -> Result<Self, ClientError> {
        let (read, write) = TcpStream::connect(address).await?.into_split();
        let mut client = Self {
            lines: BufReader::new(read).lines(),
            write,
            next_id: 0,
        };
        client
            .request("hello", json!({"client": name, "token": token}))
            .await?;
        Ok(client)
    }

    /// Sends one request and does not wait for the answer. Returns the request's id.
    ///
    /// # Errors
    ///
    /// Returns an error if the request could not be written.
    pub async fn send(&mut self, op: &str, fields: Value) -> Result<u64, ClientError> {
        self.next_id += 1;
        let mut request = Map::new();
        request.insert("id".to_owned(), json!(self.next_id));
        request.insert("op".to_owned(), json!(op));
        if let Value::Object(fields) = fields {
            // An absent field is left out, as another client would leave it out.
            request.extend(fields.into_iter().filter(|(_, v)| !v.is_null()));
        }
        let mut line = Value::Object(request).to_string();
        line.push('\n');
        self.write.write_all(line.as_bytes()).await?;
        self.write.flush().await?;
        Ok(self.next_id)
    }

    /// Reads the next answer, whatever request it answers.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Closed`] if the broker closed the connection first.
    pub async fn receive(&mut self) -> Result<Value, ClientError> {
        let line = self.lines.next_line().await?.ok_or(ClientError::Closed)?;
        serde_json::from_str(&line).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    /// Sends one request and returns its answer, or the broker's refusal.
    ///
    /// # Errors
    ///
    /// Returns an error if the broker refuses, closes the connection, or answers
    /// another request than this one.
    pub async fn request(&mut self, op: &str, fields: Value) -> Result<Value, ClientError> {
        let id = self.send(op, fields).await?;
        let reply = self.receive().await?;
        if reply.get("re").and_then(Value::as_u64) != Some(id) {
            return Err(ClientError::Protocol(format!(
                "expected the answer to request {id}, got {reply}"
            )));
        }
        match reply.get("ok").and_then(Value::as_bool) {
            Some(true) => Ok(reply),
            Some(false) => Err(ClientError::Refused {
                code: reply["error"].as_str().unwrap_or_default().to_owned(),
                message: reply["message"].as_str().unwrap_or_default().to_owned(),
                reply,
            }),
            None => Err(ClientError::Protocol(format!("no ok field in {reply}"))),
        }
    }

    /// Lists the channels.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn channels(&mut self) -> Result<Vec<ChannelInfo>, ClientError> {
        let reply = self.request("channels", json!({})).await?;
        Ok(typed::<ChannelList>(reply)?.channels)
    }

    /// Describes one channel.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn describe(&mut self, channel: &str) -> Result<ChannelInfo, ClientError> {
        let reply = self.request("describe", json!({"channel": channel})).await?;
        Ok(typed::<OneChannel>(reply)?.channel)
    }

    /// Publishes one message and waits for the confirmation.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn publish(&mut self, message: &Publish) -> Result<Published, ClientError> {
        typed(self.request("publish", message.fields()).await?)
    }

    /// The retained value for a key, if there is one.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn latest(&mut self, channel: &str, key: &str) -> Result<Option<Message>, ClientError> {
        let fields = json!({"channel": channel, "key": key});
        Ok(typed::<Latest>(self.request("latest", fields).await?)?.message)
    }

    /// Looks at up to `max` messages a channel holds. On a log, from a position.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn browse(
        &mut self,
        channel: &str,
        max: u64,
        from: Option<u64>,
    ) -> Result<Browse, ClientError> {
        let fields = json!({"channel": channel, "max": max, "from": from});
        typed(self.request("browse", fields).await?)
    }

    /// Opens a subscription of this connection's own, or attaches a consumer to a queue.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn subscribe(&mut self, channel: &str) -> Result<Subscribed, ClientError> {
        typed(self.request("subscribe", json!({"channel": channel})).await?)
    }

    /// Opens a subscription with every option: a declared name, a log position to start
    /// from, and whether a retained channel's values are handed over first.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn subscribe_with(
        &mut self,
        channel: &str,
        name: Option<&str>,
        from: Option<u64>,
        with_retained: Option<bool>,
    ) -> Result<Subscribed, ClientError> {
        let fields =
            json!({"channel": channel, "name": name, "from": from, "with_retained": with_retained});
        typed(self.request("subscribe", fields).await?)
    }

    /// Reads at most `max` messages, waiting at most `wait` for the first.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn fetch(
        &mut self,
        subscription: &str,
        max: u64,
        wait: Duration,
    ) -> Result<Vec<Delivered>, ClientError> {
        let wait_ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        let fields = json!({"subscription": subscription, "max": max, "wait_ms": wait_ms});
        Ok(typed::<Fetched>(self.request("fetch", fields).await?)?.messages)
    }

    /// Acknowledges a delivery from a queue.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn ack(&mut self, subscription: &str, delivery: &str) -> Result<(), ClientError> {
        let fields = json!({"subscription": subscription, "delivery": delivery});
        self.request("ack", fields).await.map(|_| ())
    }

    /// Gives a delivery back to its queue.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn release(&mut self, subscription: &str, delivery: &str) -> Result<(), ClientError> {
        let fields = json!({"subscription": subscription, "delivery": delivery});
        self.request("release", fields).await.map(|_| ())
    }

    /// Closes a subscription.
    ///
    /// # Errors
    ///
    /// As for [`Client::request`].
    pub async fn unsubscribe(&mut self, subscription: &str) -> Result<(), ClientError> {
        let fields = json!({"subscription": subscription});
        self.request("unsubscribe", fields).await.map(|_| ())
    }
}
