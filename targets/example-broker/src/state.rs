//! What the broker holds, and every rule it applies. Nothing here waits or does I/O.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Map, Value, json};

use crate::faults::{Fault, FaultKind};
use crate::model::{
    Acknowledgement, AppliedFault, ChannelContents, ChannelSpec, ConnectionInfo, Delivery,
    DeliveryCause, DeliveryOutcome, Held, HeldState, Message, Op, PublishOutcome, Received,
    RequestRecord, Shape, SubscriptionState, Truth, body_digest, canonical_json,
};

/// The name, in ground truth, of the stand's own consumer that never acknowledges.
pub const SILENT_CONSUMER: &str = "silent";
/// The name of the durable subscription the seed data declares on every channel but the queue.
pub const SEED_SUBSCRIPTION: &str = "example.runtime";
/// The header the seeded queue refuses a repeated value of.
pub const SEED_DEDUPE_HEADER: &str = "idempotency-token";

/// Why the broker would not do what a request asked.
#[derive(Debug)]
pub(crate) struct Refusal {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) extra: Map<String, Value>,
}

impl Refusal {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            extra: Map::new(),
        }
    }

    fn with(mut self, name: &str, value: Value) -> Self {
        self.extra.insert(name.to_owned(), value);
        self
    }
}

fn channel_missing(address: &str) -> Refusal {
    Refusal::new(
        "channel_missing",
        format!("There is no channel {address}."),
    )
}

fn not_supported(what: &str) -> Refusal {
    Refusal::new("not_supported", what)
}

fn unknown_subscription() -> Refusal {
    Refusal::new(
        "unknown_subscription",
        "This connection holds no such subscription.",
    )
}

/// A publish as the server read it from the wire.
pub(crate) struct PublishRequest {
    pub(crate) channel: String,
    pub(crate) key: Option<String>,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Value,
    pub(crate) retain: Option<bool>,
}

/// How the server ends a publish.
pub(crate) enum PublishEnd {
    Reply(Value),
    Refuse(Refusal),
    /// Close the connection without answering.
    Drop,
    /// Keep the connection and send nothing.
    Silent,
}

/// One message handed to a fetch.
struct Handed {
    number: u64,
    attempt: u32,
    cause: DeliveryCause,
    message: Message,
}

/// What waited in a subscription that is not a queue consumer.
struct Taken {
    messages: Vec<(Message, DeliveryCause)>,
    /// On a log, the position the subscription reads next.
    next: Option<u64>,
}

/// What a fetch returns.
pub(crate) struct Fetched {
    pub(crate) messages: Vec<Value>,
    /// On a log, the position the subscription reads next.
    pub(crate) next: Option<u64>,
}

#[derive(Clone)]
struct QueueEntry {
    message: Message,
    attempts: u32,
    cause: DeliveryCause,
}

struct InFlight {
    entry: QueueEntry,
    /// The handle it was delivered through, or [`SILENT_CONSUMER`].
    handle: String,
    consumer: String,
}

struct Subscription {
    durable: bool,
    /// The handle that holds it now.
    held_by: Option<String>,
    pending: VecDeque<(Message, DeliveryCause)>,
    /// On a log, the position read next.
    cursor: u64,
}

struct Channel {
    spec: ChannelSpec,
    ready: VecDeque<QueueEntry>,
    in_flight: BTreeMap<u64, InFlight>,
    retained: BTreeMap<String, Message>,
    log: VecDeque<Message>,
    log_start: u64,
    next_position: u64,
    /// Values of the de-duplication header seen, with the message stored under each.
    tokens: BTreeMap<String, (String, Option<u64>)>,
    subscriptions: BTreeMap<String, Subscription>,
}

impl Channel {
    fn new(spec: ChannelSpec) -> Self {
        Self {
            spec,
            ready: VecDeque::new(),
            in_flight: BTreeMap::new(),
            retained: BTreeMap::new(),
            log: VecDeque::new(),
            log_start: 0,
            next_position: 0,
            tokens: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
        }
    }

    fn held(&self) -> usize {
        match self.spec.shape {
            Shape::Queue => self.ready.len() + self.in_flight.len(),
            Shape::Topic => 0,
            Shape::Retained => self.retained.len(),
            Shape::Log => self.log.len(),
        }
    }

    fn info(&self) -> Value {
        let is_log = self.spec.shape == Shape::Log;
        json!({
            "address": self.spec.address,
            "shape": self.spec.shape,
            "key_field": self.spec.key_field,
            "schema": self.spec.schema,
            "dedupe_header": self.spec.dedupe_header,
            "max_body_bytes": self.spec.max_body_bytes,
            "held": self.held(),
            "start": is_log.then_some(self.log_start),
            "end": is_log.then_some(self.next_position),
            "subscriptions": self
                .subscriptions
                .iter()
                .filter(|(_, s)| s.durable)
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone)]
struct Handle {
    id: String,
    connection: u64,
    channel: String,
    name: String,
}

fn connection_id(number: u64) -> String {
    format!("c-{number}")
}

fn delivery_tag(number: u64) -> String {
    format!("d-{number}")
}

fn delivery_number(tag: &str) -> Option<u64> {
    tag.strip_prefix("d-")?.parse().ok()
}

fn message_value(message: &Message) -> Value {
    json!({
        "id": message.id,
        "channel": message.channel,
        "key": message.key,
        "headers": message.headers,
        "body": message.body,
        "position": message.position,
    })
}

fn rename_field(body: &mut Value, from: &str, to: &str) {
    if let Value::Object(fields) = body
        && let Some(value) = fields.remove(from)
    {
        fields.insert(to.to_owned(), value);
    }
}

/// Everything one running broker holds.
pub(crate) struct State {
    channels: BTreeMap<String, Channel>,
    handles: BTreeMap<String, Handle>,
    connections: BTreeMap<u64, String>,
    faults: Vec<Fault>,
    applied: Vec<AppliedFault>,
    received: Vec<Received>,
    deliveries: Vec<Delivery>,
    acknowledgements: Vec<Acknowledgement>,
    requests: Vec<RequestRecord>,
    required_token: Option<String>,
    next_message: u64,
    next_delivery: u64,
    next_handle: u64,
    next_connection: u64,
    next_seq: u64,
}

impl State {
    /// A broker holding the seed data and nothing else.
    pub(crate) fn seeded() -> Self {
        Self {
            channels: seed_channels(),
            handles: BTreeMap::new(),
            connections: BTreeMap::new(),
            faults: Vec::new(),
            applied: Vec::new(),
            received: Vec::new(),
            deliveries: Vec::new(),
            acknowledgements: Vec::new(),
            requests: Vec::new(),
            required_token: None,
            next_message: 0,
            next_delivery: 0,
            next_handle: 0,
            next_connection: 0,
            next_seq: 0,
        }
    }

    /// Restores the seed data and clears everything recorded and every setting.
    /// Open connections stay open; their subscriptions are gone.
    pub(crate) fn reset(&mut self) {
        let connections = std::mem::take(&mut self.connections);
        let next_connection = self.next_connection;
        *self = Self::seeded();
        self.connections = connections;
        self.next_connection = next_connection;
    }

    fn seq(&mut self) -> u64 {
        self.next_seq += 1;
        self.next_seq
    }

    fn client_name(&self, connection: u64) -> String {
        self.connections
            .get(&connection)
            .cloned()
            .unwrap_or_else(|| "anonymous".to_owned())
    }

    fn channel(&self, address: &str) -> Result<&Channel, Refusal> {
        self.channels
            .get(address)
            .ok_or_else(|| channel_missing(address))
    }

    fn handle(&self, connection: u64, id: &str) -> Result<Handle, Refusal> {
        self.handles
            .get(id)
            .filter(|h| h.connection == connection)
            .cloned()
            .ok_or_else(unknown_subscription)
    }

    // Connections.

    pub(crate) fn connect(&mut self) -> u64 {
        self.next_connection += 1;
        self.connections
            .insert(self.next_connection, "anonymous".to_owned());
        self.next_connection
    }

    pub(crate) fn hello(&mut self, connection: u64, client: &str) -> Value {
        self.connections.insert(connection, client.to_owned());
        json!({"protocol": crate::PROTOCOL, "connection": connection_id(connection)})
    }

    /// Closes everything a connection held. Unacknowledged deliveries go back to their queues.
    pub(crate) fn disconnect(&mut self, connection: u64) {
        let ids: Vec<String> = self
            .handles
            .values()
            .filter(|h| h.connection == connection)
            .map(|h| h.id.clone())
            .collect();
        for id in ids {
            self.close_handle(&id);
        }
        self.connections.remove(&connection);
    }

    pub(crate) fn authorized(&self, presented: Option<&str>) -> bool {
        self.required_token
            .as_deref()
            .is_none_or(|required| presented == Some(required))
    }

    pub(crate) fn set_required_token(&mut self, token: Option<&str>) {
        self.required_token = token.map(str::to_owned);
    }

    /// The channel a request is about, if it is about one.
    pub(crate) fn channel_of(&self, op: Op, request: &Value) -> Option<String> {
        let field = |name: &str| request.get(name).and_then(Value::as_str);
        match op {
            Op::Hello | Op::Channels => None,
            Op::Describe | Op::Publish | Op::Latest | Op::Browse | Op::Subscribe => {
                field("channel").map(str::to_owned)
            }
            Op::Fetch | Op::Ack | Op::Release | Op::Unsubscribe => field("subscription")
                .and_then(|id| self.handles.get(id))
                .map(|h| h.channel.clone()),
        }
    }

    pub(crate) fn record_request(&mut self, connection: u64, op: Op, channel: Option<&str>) {
        let seq = self.seq();
        self.requests.push(RequestRecord {
            seq,
            connection: connection_id(connection),
            client: self.client_name(connection),
            op,
            channel: channel.map(str::to_owned),
        });
    }

    // Faults.

    /// The first fault that `pick` accepts and that applies here, used up by one.
    pub(crate) fn take_fault(
        &mut self,
        connection: Option<u64>,
        op: Option<Op>,
        channel: Option<&str>,
        pick: impl Fn(&FaultKind) -> bool,
    ) -> Option<FaultKind> {
        let index = self.faults.iter().position(|f| {
            pick(&f.kind)
                && f.channel.as_deref().is_none_or(|c| Some(c) == channel)
                && f.op.is_none_or(|o| Some(o) == op)
        })?;
        let fault = self.faults.get_mut(index)?;
        let kind = fault.kind.clone();
        if let Some(times) = fault.times.as_mut() {
            *times = times.saturating_sub(1);
            if *times == 0 {
                self.faults.remove(index);
            }
        }
        let seq = self.seq();
        self.applied.push(AppliedFault {
            seq,
            kind: kind.clone(),
            op,
            channel: channel.map(str::to_owned),
            connection: connection.map(connection_id),
        });
        Some(kind)
    }

    pub(crate) fn set_fault(&mut self, fault: Fault) {
        self.faults.push(fault);
        for address in self.queue_addresses() {
            self.feed_silent(&address);
        }
    }

    /// Switches every fault off. The silent consumer lets go of what it holds.
    pub(crate) fn clear_faults(&mut self) {
        self.faults.clear();
        for address in self.queue_addresses() {
            let numbers = self.in_flight_of(&address, |f| f.handle == SILENT_CONSUMER);
            self.requeue(&address, &numbers, DeliveryOutcome::Returned);
        }
    }

    fn queue_addresses(&self) -> Vec<String> {
        self.channels
            .values()
            .filter(|c| c.spec.shape == Shape::Queue)
            .map(|c| c.spec.address.clone())
            .collect()
    }

    /// Lets the silent consumer take ready messages for as long as its fault allows.
    fn feed_silent(&mut self, address: &str) {
        loop {
            let ready = self
                .channels
                .get(address)
                .is_some_and(|c| c.spec.shape == Shape::Queue && !c.ready.is_empty());
            if !ready {
                return;
            }
            let silent = |k: &FaultKind| matches!(k, FaultKind::SilentConsumer);
            if self.take_fault(None, None, Some(address), silent).is_none() {
                return;
            }
            self.deliver_from_queue(address, None, SILENT_CONSUMER, SILENT_CONSUMER);
        }
    }

    /// Applies the field-renaming fault to what a read is about to return.
    fn reshape(&mut self, connection: u64, op: Op, address: &str, messages: &mut [Message]) {
        if messages.is_empty() {
            return;
        }
        let rename = |k: &FaultKind| matches!(k, FaultKind::RenameField { .. });
        if let Some(FaultKind::RenameField { from, to }) =
            self.take_fault(Some(connection), Some(op), Some(address), rename)
        {
            for message in messages {
                rename_field(&mut message.body, &from, &to);
            }
        }
    }

    // Reading without taking.

    pub(crate) fn channels(&self) -> Value {
        json!({"channels": self.channels.values().map(Channel::info).collect::<Vec<_>>()})
    }

    pub(crate) fn describe(&self, address: &str) -> Result<Value, Refusal> {
        Ok(json!({"channel": self.channel(address)?.info()}))
    }

    pub(crate) fn latest(
        &mut self,
        connection: u64,
        address: &str,
        key: &str,
    ) -> Result<Value, Refusal> {
        let channel = self.channel(address)?;
        if channel.spec.shape != Shape::Retained {
            return Err(not_supported("Only a retained channel has a latest value."));
        }
        let mut found: Vec<Message> = channel.retained.get(key).cloned().into_iter().collect();
        self.reshape(connection, Op::Latest, address, &mut found);
        Ok(json!({"message": found.first().map(message_value)}))
    }

    pub(crate) fn browse(
        &mut self,
        connection: u64,
        address: &str,
        max: usize,
        from: Option<u64>,
    ) -> Result<Value, Refusal> {
        let channel = self.channel(address)?;
        let shape = channel.spec.shape;
        if from.is_some() && shape != Shape::Log {
            return Err(not_supported("Only a log has positions."));
        }
        // Each message, with the state and delivery count a queue adds.
        let mut all: Vec<(Message, Option<(HeldState, u32)>)> = Vec::new();
        let mut bounds = None;
        match shape {
            Shape::Topic => {
                return Err(not_supported("A topic keeps nothing to look at."));
            }
            Shape::Queue => {
                for held in channel.in_flight.values() {
                    let extra = (HeldState::InFlight, held.entry.attempts);
                    all.push((held.entry.message.clone(), Some(extra)));
                }
                for entry in &channel.ready {
                    let extra = (HeldState::Ready, entry.attempts);
                    all.push((entry.message.clone(), Some(extra)));
                }
            }
            Shape::Retained => {
                all.extend(channel.retained.values().map(|m| (m.clone(), None)));
            }
            Shape::Log => {
                let from = from.unwrap_or(channel.log_start);
                check_position(channel, from)?;
                bounds = Some((channel.log_start, channel.next_position));
                all.extend(
                    channel
                        .log
                        .iter()
                        .filter(|m| m.position.is_some_and(|p| p >= from))
                        .map(|m| (m.clone(), None)),
                );
            }
        }
        let held = channel.held();
        let more = all.len() > max;
        all.truncate(max);
        let (mut messages, extras): (Vec<Message>, Vec<_>) = all.into_iter().unzip();
        self.reshape(connection, Op::Browse, address, &mut messages);
        let items: Vec<Value> = messages
            .iter()
            .zip(extras)
            .map(|(message, extra)| {
                let mut item = message_value(message);
                if let (Some(fields), Some((state, attempts))) = (item.as_object_mut(), extra) {
                    fields.insert("state".to_owned(), json!(state));
                    fields.insert("attempts".to_owned(), json!(attempts));
                }
                item
            })
            .collect();
        Ok(json!({
            "messages": items,
            "more": more,
            "held": held,
            "start": bounds.map(|b| b.0),
            "end": bounds.map(|b| b.1),
        }))
    }

    // Publishing.

    pub(crate) fn publish(&mut self, connection: u64, request: PublishRequest) -> PublishEnd {
        let seq = self.seq();
        let (outcome, end) = self.publish_inner(connection, &request);
        let answered = matches!(end, PublishEnd::Reply(_) | PublishEnd::Refuse(_));
        self.received.push(Received {
            seq,
            connection: connection_id(connection),
            client: self.client_name(connection),
            body_digest: body_digest(&request.body),
            channel: request.channel,
            key: request.key,
            headers: request.headers,
            body: request.body,
            outcome,
            answered,
        });
        end
    }

    fn publish_inner(
        &mut self,
        connection: u64,
        request: &PublishRequest,
    ) -> (PublishOutcome, PublishEnd) {
        fn refuse(refusal: Refusal) -> (PublishOutcome, PublishEnd) {
            let code = refusal.code.to_owned();
            (PublishOutcome::Refused { code }, PublishEnd::Refuse(refusal))
        }
        let address = request.channel.as_str();
        let Some(spec) = self.channels.get(address).map(|c| c.spec.clone()) else {
            return refuse(channel_missing(address));
        };
        let on_publish = |k: &FaultKind| {
            matches!(
                k,
                FaultKind::StoreThenDrop
                    | FaultKind::StoreThenSilent
                    | FaultKind::DropBeforeStore
                    | FaultKind::Refuse { .. }
            )
        };
        let fault = self.take_fault(Some(connection), Some(Op::Publish), Some(address), on_publish);
        match &fault {
            Some(FaultKind::DropBeforeStore) => {
                return (PublishOutcome::Dropped, PublishEnd::Drop);
            }
            Some(FaultKind::Refuse { reason }) => {
                return refuse(Refusal::new("refused", reason.clone()));
            }
            _ => {}
        }
        if spec.shape == Shape::Retained && request.key.is_none() {
            return refuse(Refusal::new(
                "key_required",
                "A publish to a retained channel carries a key.",
            ));
        }
        if request.retain.is_some() && spec.shape != Shape::Retained {
            return refuse(not_supported(
                "Only a retained channel takes the retain flag.",
            ));
        }
        if canonical_json(&request.body).len() > spec.max_body_bytes {
            return refuse(
                Refusal::new("message_too_large", "The body is over the channel's limit.")
                    .with("max_body_bytes", json!(spec.max_body_bytes)),
            );
        }
        let token = spec
            .dedupe_header
            .as_ref()
            .and_then(|header| request.headers.get(header))
            .cloned();
        let Some(channel) = self.channels.get_mut(address) else {
            return refuse(channel_missing(address));
        };
        if let Some(token) = &token
            && let Some((id, position)) = channel.tokens.get(token)
        {
            let refusal = Refusal::new(
                "duplicate_token",
                "A message with this token was already stored.",
            )
            .with("id", json!(id))
            .with("position", json!(position));
            return (
                PublishOutcome::Duplicate { of: id.clone() },
                PublishEnd::Refuse(refusal),
            );
        }
        self.next_message += 1;
        let mut message = Message {
            id: format!("m-{}", self.next_message),
            channel: address.to_owned(),
            key: request.key.clone(),
            headers: request.headers.clone(),
            body: request.body.clone(),
            position: None,
        };
        let mut retained = false;
        match spec.shape {
            Shape::Queue => channel.ready.push_back(QueueEntry {
                message: message.clone(),
                attempts: 0,
                cause: DeliveryCause::First,
            }),
            Shape::Topic | Shape::Retained => {
                if spec.shape == Shape::Retained
                    && request.retain.unwrap_or(true)
                    && let Some(key) = &request.key
                {
                    channel.retained.insert(key.clone(), message.clone());
                    retained = true;
                }
                for subscription in channel.subscriptions.values_mut() {
                    subscription
                        .pending
                        .push_back((message.clone(), DeliveryCause::First));
                }
            }
            Shape::Log => {
                message.position = Some(channel.next_position);
                channel.next_position += 1;
                channel.log.push_back(message.clone());
            }
        }
        if let Some(token) = token {
            channel
                .tokens
                .insert(token, (message.id.clone(), message.position));
        }
        if spec.shape == Shape::Queue {
            self.feed_silent(address);
        }
        let outcome = PublishOutcome::Stored {
            id: message.id.clone(),
            position: message.position,
            retained,
        };
        let end = match fault {
            Some(FaultKind::StoreThenDrop) => PublishEnd::Drop,
            Some(FaultKind::StoreThenSilent) => PublishEnd::Silent,
            _ => PublishEnd::Reply(json!({"id": message.id, "position": message.position})),
        };
        (outcome, end)
    }

    // Subscriptions and deliveries.

    pub(crate) fn subscribe(
        &mut self,
        connection: u64,
        address: &str,
        name: Option<&str>,
        from: Option<u64>,
        with_retained: bool,
    ) -> Result<Value, Refusal> {
        let client = self.client_name(connection);
        let id = format!("s-{}", self.next_handle + 1);
        let channel = self
            .channels
            .get_mut(address)
            .ok_or_else(|| channel_missing(address))?;
        let shape = channel.spec.shape;
        if from.is_some() && shape != Shape::Log {
            return Err(not_supported("Only a log has positions."));
        }
        if let Some(from) = from {
            check_position(channel, from)?;
        }
        let mut position = None;
        let name = if shape == Shape::Queue {
            name.map_or(client, str::to_owned)
        } else {
            let name = match name {
                Some(name) => {
                    let subscription = channel.subscriptions.get(name).ok_or_else(|| {
                        Refusal::new(
                            "subscription_missing",
                            format!("There is no subscription {name} on {address}."),
                        )
                    })?;
                    if subscription.held_by.is_some() {
                        return Err(Refusal::new(
                            "subscription_in_use",
                            format!("Another connection holds the subscription {name}."),
                        ));
                    }
                    name.to_owned()
                }
                None => {
                    // The broker discards this one when its connection lets go.
                    let name = format!("ephemeral-{id}");
                    let mut pending = VecDeque::new();
                    if shape == Shape::Retained && with_retained {
                        pending.extend(
                            channel
                                .retained
                                .values()
                                .map(|m| (m.clone(), DeliveryCause::Retained)),
                        );
                    }
                    channel.subscriptions.insert(
                        name.clone(),
                        Subscription {
                            durable: false,
                            held_by: None,
                            pending,
                            cursor: channel.next_position,
                        },
                    );
                    name
                }
            };
            if let Some(subscription) = channel.subscriptions.get_mut(&name) {
                subscription.held_by = Some(id.clone());
                if let Some(from) = from {
                    subscription.cursor = from;
                }
                position = (shape == Shape::Log).then_some(subscription.cursor);
            }
            name
        };
        self.next_handle += 1;
        self.handles.insert(
            id.clone(),
            Handle {
                id: id.clone(),
                connection,
                channel: address.to_owned(),
                name: name.clone(),
            },
        );
        Ok(json!({"subscription": id, "name": name, "position": position}))
    }

    /// Hands over at most `max` messages. An empty answer means nothing is waiting.
    pub(crate) fn fetch(
        &mut self,
        connection: u64,
        handle: &str,
        max: usize,
    ) -> Result<Fetched, Refusal> {
        let handle = self.handle(connection, handle)?;
        let address = handle.channel.as_str();
        let shape = self.channel(address)?.spec.shape;
        let duplicate = |k: &FaultKind| matches!(k, FaultKind::DuplicateDelivery);
        let mut handed: Vec<Handed> = Vec::new();
        let mut copies: Vec<Message> = Vec::new();
        let mut next = None;
        if shape == Shape::Queue {
            while handed.len() < max {
                let Some(one) =
                    self.deliver_from_queue(address, Some(connection), &handle.id, &handle.name)
                else {
                    break;
                };
                handed.push(one);
            }
        } else {
            let taken = self.take_pending(address, &handle.name, max)?;
            next = taken.next;
            for (message, cause) in taken.messages {
                self.next_delivery += 1;
                let number = self.next_delivery;
                let seq = self.seq();
                self.deliveries.push(Delivery {
                    seq,
                    delivery: delivery_tag(number),
                    message: message.id.clone(),
                    channel: address.to_owned(),
                    subscription: handle.name.clone(),
                    connection: Some(connection_id(connection)),
                    attempt: 1,
                    cause,
                    outcome: DeliveryOutcome::NotRequired,
                });
                handed.push(Handed {
                    number,
                    attempt: 1,
                    cause,
                    message,
                });
            }
        }
        for one in &handed {
            if one.cause != DeliveryCause::Duplicate
                && self
                    .take_fault(Some(connection), Some(Op::Fetch), Some(address), duplicate)
                    .is_some()
            {
                copies.push(one.message.clone());
            }
        }
        // A copy goes to the front, so the next fetch hands the same message over again.
        if let Some(channel) = self.channels.get_mut(address) {
            for message in copies.into_iter().rev() {
                if shape == Shape::Queue {
                    let attempts = handed
                        .iter()
                        .find(|h| h.message.id == message.id)
                        .map_or(1, |h| h.attempt);
                    channel.ready.push_front(QueueEntry {
                        message,
                        attempts,
                        cause: DeliveryCause::Duplicate,
                    });
                } else if let Some(subscription) = channel.subscriptions.get_mut(&handle.name) {
                    subscription
                        .pending
                        .push_front((message, DeliveryCause::Duplicate));
                }
            }
        }
        let mut messages: Vec<Message> = handed.iter().map(|h| h.message.clone()).collect();
        self.reshape(connection, Op::Fetch, address, &mut messages);
        let messages = handed
            .iter()
            .zip(&messages)
            .map(|(one, message)| {
                json!({
                    "delivery": delivery_tag(one.number),
                    "attempt": one.attempt,
                    "retained": one.cause == DeliveryCause::Retained,
                    "message": message_value(message),
                })
            })
            .collect();
        Ok(Fetched { messages, next })
    }

    /// Takes what waits in a subscription that is not a queue consumer.
    fn take_pending(&mut self, address: &str, name: &str, max: usize) -> Result<Taken, Refusal> {
        let channel = self
            .channels
            .get_mut(address)
            .ok_or_else(|| channel_missing(address))?;
        let is_log = channel.spec.shape == Shape::Log;
        let (start, end) = (channel.log_start, channel.next_position);
        let subscription = channel
            .subscriptions
            .get_mut(name)
            .ok_or_else(unknown_subscription)?;
        let mut taken = Vec::new();
        while taken.len() < max {
            let Some(one) = subscription.pending.pop_front() else {
                break;
            };
            taken.push(one);
        }
        if is_log {
            if subscription.cursor < start && taken.is_empty() {
                return Err(position_out_of_range(subscription.cursor, start, end));
            }
            while taken.len() < max && subscription.cursor >= start {
                let index = usize::try_from(subscription.cursor - start).ok();
                let Some(message) = index.and_then(|i| channel.log.get(i)) else {
                    break;
                };
                taken.push((message.clone(), DeliveryCause::First));
                subscription.cursor += 1;
            }
        }
        Ok(Taken {
            messages: taken,
            next: is_log.then_some(subscription.cursor),
        })
    }

    fn deliver_from_queue(
        &mut self,
        address: &str,
        connection: Option<u64>,
        handle: &str,
        consumer: &str,
    ) -> Option<Handed> {
        let channel = self.channels.get_mut(address)?;
        let mut entry = channel.ready.pop_front()?;
        let cause = entry.cause;
        if cause != DeliveryCause::Duplicate {
            entry.attempts += 1;
        }
        self.next_delivery += 1;
        let number = self.next_delivery;
        let handed = Handed {
            number,
            attempt: entry.attempts,
            cause,
            message: entry.message.clone(),
        };
        channel.in_flight.insert(
            number,
            InFlight {
                entry,
                handle: handle.to_owned(),
                consumer: consumer.to_owned(),
            },
        );
        let seq = self.seq();
        self.deliveries.push(Delivery {
            seq,
            delivery: delivery_tag(number),
            message: handed.message.id.clone(),
            channel: address.to_owned(),
            subscription: consumer.to_owned(),
            connection: connection.map(connection_id),
            attempt: handed.attempt,
            cause,
            outcome: DeliveryOutcome::Unacknowledged,
        });
        Some(handed)
    }

    fn in_flight_of(&self, address: &str, pick: impl Fn(&InFlight) -> bool) -> Vec<u64> {
        self.channels.get(address).map_or_else(Vec::new, |c| {
            c.in_flight
                .iter()
                .filter(|(_, held)| pick(held))
                .map(|(number, _)| *number)
                .collect()
        })
    }

    /// Puts deliveries back at the front of their queue, in their first order.
    fn requeue(&mut self, address: &str, numbers: &[u64], outcome: DeliveryOutcome) -> usize {
        let mut count = 0;
        if let Some(channel) = self.channels.get_mut(address) {
            for number in numbers.iter().rev() {
                if let Some(held) = channel.in_flight.remove(number) {
                    let mut entry = held.entry;
                    entry.cause = DeliveryCause::Redelivery;
                    channel.ready.push_front(entry);
                    set_outcome(&mut self.deliveries, *number, outcome);
                    count += 1;
                }
            }
        }
        self.feed_silent(address);
        count
    }

    /// The delivery a consumer names, if this handle holds it.
    fn held_delivery(&self, handle: &Handle, tag: &str) -> Result<u64, Refusal> {
        let channel = self.channel(&handle.channel)?;
        if channel.spec.shape != Shape::Queue {
            return Err(not_supported(
                "Only a delivery from a queue is acknowledged or given back.",
            ));
        }
        delivery_number(tag)
            .filter(|n| {
                channel
                    .in_flight
                    .get(n)
                    .is_some_and(|f| f.handle == handle.id)
            })
            .ok_or_else(|| {
                Refusal::new(
                    "unknown_delivery",
                    "This subscription holds no such unacknowledged delivery.",
                )
            })
    }

    pub(crate) fn ack(&mut self, connection: u64, handle: &str, tag: &str) -> Result<Value, Refusal> {
        let handle = self.handle(connection, handle)?;
        let number = self.held_delivery(&handle, tag)?;
        let held = self
            .channels
            .get_mut(&handle.channel)
            .and_then(|c| c.in_flight.remove(&number));
        if let Some(held) = held {
            set_outcome(&mut self.deliveries, number, DeliveryOutcome::Acknowledged);
            let seq = self.seq();
            self.acknowledgements.push(Acknowledgement {
                seq,
                delivery: delivery_tag(number),
                message: held.entry.message.id,
                channel: handle.channel,
                subscription: held.consumer,
                connection: connection_id(connection),
            });
        }
        Ok(json!({}))
    }

    pub(crate) fn release(
        &mut self,
        connection: u64,
        handle: &str,
        tag: &str,
    ) -> Result<Value, Refusal> {
        let handle = self.handle(connection, handle)?;
        let number = self.held_delivery(&handle, tag)?;
        self.requeue(&handle.channel, &[number], DeliveryOutcome::Released);
        Ok(json!({}))
    }

    pub(crate) fn unsubscribe(&mut self, connection: u64, handle: &str) -> Result<Value, Refusal> {
        let handle = self.handle(connection, handle)?;
        self.close_handle(&handle.id);
        Ok(json!({}))
    }

    fn close_handle(&mut self, id: &str) {
        let Some(handle) = self.handles.remove(id) else {
            return;
        };
        let numbers = self.in_flight_of(&handle.channel, |f| f.handle == handle.id);
        self.requeue(&handle.channel, &numbers, DeliveryOutcome::Returned);
        if let Some(channel) = self.channels.get_mut(&handle.channel) {
            match channel.subscriptions.get_mut(&handle.name) {
                Some(subscription) if subscription.durable => subscription.held_by = None,
                Some(_) => {
                    channel.subscriptions.remove(&handle.name);
                }
                None => {}
            }
        }
    }

    // The control interface.

    /// Declares a channel, replacing one of the same address and everything it held.
    pub(crate) fn declare_channel(&mut self, spec: ChannelSpec) {
        self.handles.retain(|_, h| h.channel != spec.address);
        self.channels
            .insert(spec.address.clone(), Channel::new(spec));
    }

    pub(crate) fn remove_channel(&mut self, address: &str) -> bool {
        self.handles.retain(|_, h| h.channel != address);
        self.channels.remove(address).is_some()
    }

    /// Declares a durable subscription. False if the channel is missing or is a queue.
    pub(crate) fn declare_subscription(&mut self, address: &str, name: &str) -> bool {
        let Some(channel) = self.channels.get_mut(address) else {
            return false;
        };
        if channel.spec.shape == Shape::Queue {
            return false;
        }
        let cursor = channel.next_position;
        channel
            .subscriptions
            .entry(name.to_owned())
            .or_insert(Subscription {
                durable: true,
                held_by: None,
                pending: VecDeque::new(),
                cursor,
            });
        true
    }

    pub(crate) fn set_dedupe_header(&mut self, address: &str, header: Option<&str>) -> bool {
        let Some(channel) = self.channels.get_mut(address) else {
            return false;
        };
        channel.spec.dedupe_header = header.map(str::to_owned);
        true
    }

    /// Drops what a retained channel or a log keeps. Returns how many messages went.
    pub(crate) fn expire_retention(&mut self, address: &str) -> usize {
        let Some(channel) = self.channels.get_mut(address) else {
            return 0;
        };
        let dropped = channel.retained.len() + channel.log.len();
        channel.retained.clear();
        channel.log.clear();
        channel.log_start = channel.next_position;
        dropped
    }

    /// Takes every unacknowledged delivery of a queue back, as a broker does when its
    /// time limit for an acknowledgement runs out. Returns how many came back.
    pub(crate) fn redeliver_unacknowledged(&mut self, address: &str) -> usize {
        let numbers = self.in_flight_of(address, |_| true);
        self.requeue(address, &numbers, DeliveryOutcome::Returned)
    }

    pub(crate) fn truth(&self) -> Truth {
        let holder = |handle: &Option<String>| {
            handle
                .as_ref()
                .and_then(|id| self.handles.get(id))
                .map(|h| connection_id(h.connection))
        };
        let channels = self
            .channels
            .iter()
            .map(|(address, channel)| {
                let in_flight = channel.in_flight.iter().map(|(number, held)| Held {
                    message: held.entry.message.clone(),
                    state: HeldState::InFlight,
                    attempts: held.entry.attempts,
                    held_by: Some(held.consumer.clone()),
                    delivery: Some(delivery_tag(*number)),
                });
                let ready = channel.ready.iter().map(|entry| Held {
                    message: entry.message.clone(),
                    state: HeldState::Ready,
                    attempts: entry.attempts,
                    held_by: None,
                    delivery: None,
                });
                let is_log = channel.spec.shape == Shape::Log;
                let contents = ChannelContents {
                    spec: channel.spec.clone(),
                    queue: in_flight.chain(ready).collect(),
                    retained: channel.retained.clone(),
                    log: channel.log.iter().cloned().collect(),
                    log_start: channel.log_start,
                    log_end: channel.next_position,
                    subscriptions: channel
                        .subscriptions
                        .iter()
                        .map(|(name, s)| SubscriptionState {
                            name: name.clone(),
                            durable: s.durable,
                            held_by: holder(&s.held_by),
                            pending: s.pending.iter().map(|(m, _)| m.id.clone()).collect(),
                            position: is_log.then_some(s.cursor),
                        })
                        .collect(),
                };
                (address.clone(), contents)
            })
            .collect();
        Truth {
            received: self.received.clone(),
            deliveries: self.deliveries.clone(),
            acknowledgements: self.acknowledgements.clone(),
            requests: self.requests.clone(),
            channels,
            faults: self.faults.clone(),
            applied: self.applied.clone(),
            connections: self
                .connections
                .iter()
                .map(|(number, client)| ConnectionInfo {
                    id: connection_id(*number),
                    client: client.clone(),
                })
                .collect(),
        }
    }
}

fn set_outcome(deliveries: &mut [Delivery], number: u64, outcome: DeliveryOutcome) {
    let tag = delivery_tag(number);
    if let Some(delivery) = deliveries.iter_mut().rev().find(|d| d.delivery == tag) {
        delivery.outcome = outcome;
    }
}

fn position_out_of_range(position: u64, start: u64, end: u64) -> Refusal {
    Refusal::new(
        "position_out_of_range",
        format!("The log holds positions {start} up to, and not including, {end}; {position} is outside."),
    )
    .with("start", json!(start))
    .with("end", json!(end))
}

fn check_position(channel: &Channel, position: u64) -> Result<(), Refusal> {
    if position < channel.log_start || position > channel.next_position {
        return Err(position_out_of_range(
            position,
            channel.log_start,
            channel.next_position,
        ));
    }
    Ok(())
}

/// The seed data: one channel of each shape, for the imaginary `example` target.
fn seed_channels() -> BTreeMap<String, Channel> {
    let mut number = 0;
    let mut seed = |channel: &str, key: &str, body: Value, position: Option<u64>| {
        number += 1;
        Message {
            id: format!("s-{number}"),
            channel: channel.to_owned(),
            key: Some(key.to_owned()),
            headers: BTreeMap::new(),
            body,
            position,
        }
    };
    let order = |field: &str, values: &[&str]| {
        json!({
            "type": "object",
            "required": ["order_id", field],
            "properties": {"order_id": {"type": "string"}, field: {"type": "string", "enum": values}},
        })
    };

    let mut status = Channel::new(
        ChannelSpec::new("example.orders.status", Shape::Retained)
            .key_field("order_id")
            .schema(order("status", &["draft", "submitted", "shipped"])),
    );
    for (key, value) in [("A-100", "draft"), ("A-101", "submitted")] {
        let body = json!({"order_id": key, "status": value});
        status
            .retained
            .insert(key.to_owned(), seed("example.orders.status", key, body, None));
    }

    let mut events = Channel::new(
        ChannelSpec::new("example.orders.events", Shape::Log)
            .key_field("order_id")
            .schema(order("event", &["created", "submitted", "shipped"])),
    );
    for (key, event) in [("A-100", "created"), ("A-101", "created"), ("A-101", "submitted")] {
        let body = json!({"order_id": key, "event": event});
        let position = events.next_position;
        events
            .log
            .push_back(seed("example.orders.events", key, body, Some(position)));
        events.next_position += 1;
    }

    let mut work = Channel::new(
        ChannelSpec::new("example.orders.work", Shape::Queue)
            .key_field("order_id")
            .dedupe_header(SEED_DEDUPE_HEADER),
    );
    for (key, task) in [("A-100", "pick"), ("A-101", "pack")] {
        let body = json!({"order_id": key, "task": task});
        work.ready.push_back(QueueEntry {
            message: seed("example.orders.work", key, body, None),
            attempts: 0,
            cause: DeliveryCause::First,
        });
    }

    let notices = Channel::new(ChannelSpec::new("example.orders.notices", Shape::Topic));

    let mut channels = BTreeMap::new();
    for mut channel in [status, events, notices] {
        let cursor = channel.next_position;
        channel.subscriptions.insert(
            SEED_SUBSCRIPTION.to_owned(),
            Subscription {
                durable: true,
                held_by: None,
                pending: VecDeque::new(),
                cursor,
            },
        );
        channels.insert(channel.spec.address.clone(), channel);
    }
    channels.insert(work.spec.address.clone(), work);
    channels
}
