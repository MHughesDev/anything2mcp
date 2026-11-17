//! Every rule of the stand: streams, the duplicate window, consumers, pending
//! deliveries, and what was received, delivered, and acknowledged. The wire is
//! in `server.rs`; nothing here knows a byte of it.

use std::collections::{BTreeMap, VecDeque};

use crate::model::{
    AckKind, AckPolicy, Acknowledgement, ApiCall, Connect, ConsumerSpec, ConsumerTruth, Delivery,
    Fault, Outcome, Received, Retention, StoredMessage, StreamSpec, StreamTruth, Truth,
};

/// The stand's clock starts here, in milliseconds since 1970. It moves only
/// when a test moves it, so a duplicate window is crossed without a wait.
pub(crate) const CLOCK_START_MS: u64 = 1_800_000_000_000;

/// An answer of the JetStream interface that is an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiError {
    pub code: u16,
    pub err_code: u16,
    pub description: String,
}

impl ApiError {
    pub(crate) fn new(code: u16, err_code: u16, description: impl Into<String>) -> Self {
        Self {
            code,
            err_code,
            description: description.into(),
        }
    }
    pub(crate) fn stream_not_found() -> Self {
        Self::new(404, 10059, "stream not found")
    }
    pub(crate) fn consumer_not_found() -> Self {
        Self::new(404, 10014, "consumer not found")
    }
    pub(crate) fn no_message() -> Self {
        Self::new(404, 10037, "no message found")
    }
    pub(crate) fn bad_request() -> Self {
        Self::new(400, 10003, "bad request")
    }
}

pub(crate) struct Stream {
    pub spec: StreamSpec,
    pub messages: BTreeMap<u64, StoredMessage>,
    pub last_seq: u64,
    /// Message ids seen, with the sequence first stored and when.
    pub dedupe: BTreeMap<String, (u64, u64)>,
    pub consumers: BTreeMap<String, Consumer>,
}

pub(crate) struct Consumer {
    pub spec: ConsumerSpec,
    pub ephemeral: bool,
    /// The next stream sequence to look at.
    pub next: u64,
    /// The consumer's own count of deliveries.
    pub dseq: u64,
    /// Delivered and not acknowledged: stream sequence to (delivery
    /// sequence, delivery count).
    pub pending: BTreeMap<u64, (u64, u64)>,
    /// What goes out again first.
    pub redeliver: VecDeque<u64>,
    /// When each pending delivery went out, on the stand's clock.
    pub delivered_at: BTreeMap<u64, u64>,
    /// How many times each stream sequence went out.
    pub counts: BTreeMap<u64, u64>,
}

/// One delivery the state handed out, for the wire to send.
#[derive(Debug, Clone)]
pub(crate) struct Handed {
    pub message: StoredMessage,
    pub ack_subject: String,
}

/// What a publish did.
pub(crate) enum Published {
    /// No stream listens on the subject.
    NoStream,
    /// Stored, or a duplicate of what was: the stream and the sequence.
    Ack {
        stream: String,
        seq: u64,
        duplicate: bool,
    },
    /// Refused, nothing stored.
    Refused { stream: String, error: ApiError },
}

pub(crate) struct State {
    pub streams: BTreeMap<String, Stream>,
    pub now_ms: u64,
    pub faults: Vec<Fault>,
    pub applied: Vec<Fault>,
    pub received: Vec<Received>,
    pub deliveries: Vec<Delivery>,
    pub acks: Vec<Acknowledgement>,
    pub api_calls: Vec<ApiCall>,
    pub connects: Vec<Connect>,
    pub required_token: Option<String>,
    pub required_user: Option<(String, String)>,
    pub max_payload: usize,
    pub tls_required: bool,
    next_ephemeral: u64,
}

/// Whether a subject matches a pattern with `*` and `>`.
pub(crate) fn matches(pattern: &str, subject: &str) -> bool {
    let mut wanted = pattern.split('.');
    let mut given = subject.split('.');
    loop {
        match (wanted.next(), given.next()) {
            (Some(">"), Some(_)) => return true,
            (Some("*"), Some(_)) => {}
            (Some(a), Some(b)) if a == b => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

impl Stream {
    fn new(spec: StreamSpec) -> Self {
        Self {
            spec,
            messages: BTreeMap::new(),
            last_seq: 0,
            dedupe: BTreeMap::new(),
            consumers: BTreeMap::new(),
        }
    }

    pub(crate) fn first_seq(&self) -> u64 {
        match self.messages.keys().next() {
            Some(seq) => *seq,
            None if self.last_seq == 0 => 0,
            None => self.last_seq + 1,
        }
    }

    fn captures(&self, subject: &str) -> bool {
        self.spec.subjects.iter().any(|p| matches(p, subject))
    }

    fn store(&mut self, subject: &str, headers: BTreeMap<String, String>, data: Vec<u8>) -> u64 {
        // A limit per subject evicts that subject's oldest messages, from
        // wherever they stand in the stream.
        if let Ok(keep) = usize::try_from(self.spec.max_msgs_per_subject)
            && keep > 0
        {
            let of_subject: Vec<u64> = self
                .messages
                .iter()
                .filter(|(_, m)| m.subject == subject)
                .map(|(seq, _)| *seq)
                .collect();
            for seq in of_subject
                .iter()
                .take((of_subject.len() + 1).saturating_sub(keep))
            {
                self.messages.remove(seq);
            }
        }
        self.last_seq += 1;
        let seq = self.last_seq;
        self.messages.insert(
            seq,
            StoredMessage {
                seq,
                subject: subject.to_owned(),
                headers,
                data,
            },
        );
        seq
    }

    /// The next message a consumer may be handed, if any.
    fn claim(&mut self, consumer: &str, now_ms: u64, foreign: bool) -> Option<Handed> {
        let stream = self.spec.name.clone();
        let consumer_state = self.consumers.get_mut(consumer)?;
        let mut seq = None;
        while let Some(again) = consumer_state.redeliver.pop_front() {
            if self.messages.contains_key(&again) {
                seq = Some(again);
                break;
            }
        }
        if seq.is_none() {
            let filter = consumer_state.spec.filter.clone();
            let found = self
                .messages
                .range(consumer_state.next..)
                .find(|(s, m)| {
                    !consumer_state.pending.contains_key(s)
                        && filter.as_deref().is_none_or(|f| matches(f, &m.subject))
                })
                .map(|(s, _)| *s);
            if let Some(found) = found {
                consumer_state.next = found + 1;
                seq = Some(found);
            }
        }
        let seq = seq?;
        let message = self.messages.get(&seq)?.clone();
        consumer_state.dseq += 1;
        let count = consumer_state.counts.entry(seq).or_insert(0);
        *count += 1;
        let dc = *count;
        if consumer_state.spec.ack == AckPolicy::Explicit {
            consumer_state
                .pending
                .insert(seq, (consumer_state.dseq, dc));
            consumer_state.delivered_at.insert(seq, now_ms);
        }
        let pending = self.messages.range(consumer_state.next..).count();
        let named = if foreign {
            "another_consumer"
        } else {
            consumer
        };
        Some(Handed {
            ack_subject: format!(
                "$JS.ACK.{stream}.{named}.{dc}.{seq}.{}.{}.{pending}",
                consumer_state.dseq, CLOCK_START_MS
            ),
            message,
        })
    }
}

impl State {
    pub(crate) fn seeded() -> Self {
        let mut state = Self {
            streams: BTreeMap::new(),
            now_ms: CLOCK_START_MS,
            faults: Vec::new(),
            applied: Vec::new(),
            received: Vec::new(),
            deliveries: Vec::new(),
            acks: Vec::new(),
            api_calls: Vec::new(),
            connects: Vec::new(),
            required_token: None,
            required_user: None,
            max_payload: crate::DEFAULT_MAX_PAYLOAD,
            tls_required: false,
            next_ephemeral: 1,
        };
        state.seed();
        state
    }

    fn seed(&mut self) {
        let body = |text: &str| text.as_bytes().to_vec();
        let mut status = Stream::new(
            StreamSpec::new(crate::SEED_STATUS, ["example.orders.status.*"]).one_per_subject(),
        );
        status.store(
            "example.orders.status.A-100",
            BTreeMap::new(),
            body(r#"{"order_id":"A-100","status":"submitted"}"#),
        );
        let mut events = Stream::new(StreamSpec::new(
            crate::SEED_EVENTS,
            ["example.orders.events.*"],
        ));
        for (key, event) in [
            ("A-100", "created"),
            ("A-101", "created"),
            ("A-101", "submitted"),
        ] {
            events.store(
                &format!("example.orders.events.{key}"),
                BTreeMap::new(),
                body(&format!(r#"{{"order_id":"{key}","event":"{event}"}}"#)),
            );
        }
        let mut work =
            Stream::new(StreamSpec::new(crate::SEED_WORK, ["example.orders.work.*"]).work_queue());
        for (key, task) in [("A-100", "pick"), ("A-101", "pack")] {
            work.store(
                &format!("example.orders.work.{key}"),
                BTreeMap::new(),
                body(&format!(r#"{{"order_id":"{key}","task":"{task}"}}"#)),
            );
        }
        work.consumers.insert(
            crate::SEED_CONSUMER.to_owned(),
            new_consumer(
                ConsumerSpec::new(crate::SEED_CONSUMER, AckPolicy::Explicit),
                false,
                1,
            ),
        );
        for stream in [status, events, work] {
            self.streams.insert(stream.spec.name.clone(), stream);
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::seeded();
    }

    pub(crate) fn fault(&mut self, fault: &Fault) -> bool {
        if self.faults.contains(fault) {
            self.applied.push(fault.clone());
            true
        } else {
            false
        }
    }

    /// The first fault switched on that `pick` accepts, recorded as applied.
    pub(crate) fn fault_where(&mut self, pick: impl Fn(&Fault) -> bool) -> Option<Fault> {
        let found = self.faults.iter().find(|f| pick(f)).cloned()?;
        self.applied.push(found.clone());
        Some(found)
    }

    pub(crate) fn stream_for(&self, subject: &str) -> Option<&str> {
        self.streams
            .values()
            .find(|s| s.captures(subject))
            .map(|s| s.spec.name.as_str())
    }

    pub(crate) fn publish(
        &mut self,
        connection: u64,
        subject: &str,
        headers: BTreeMap<String, String>,
        data: Vec<u8>,
    ) -> Published {
        let now = self.now_ms;
        let Some(name) = self.stream_for(subject).map(str::to_owned) else {
            self.received.push(Received {
                connection,
                subject: subject.to_owned(),
                headers,
                data,
                outcome: Outcome::NotStored,
            });
            return Published::NoStream;
        };
        let refusal = self.fault_where(|f| matches!(f, Fault::RefusePublish { .. }));
        let Some(stream) = self.streams.get_mut(&name) else {
            return Published::NoStream;
        };
        let size = data.len()
            + headers
                .iter()
                .map(|(k, v)| k.len() + v.len() + 4)
                .sum::<usize>();
        let refused = if let Some(Fault::RefusePublish { err_code }) = refusal {
            Some(ApiError::new(400, err_code, "refused by the stand's fault"))
        } else if stream.spec.max_msg_size.is_some_and(|limit| size > limit) {
            Some(ApiError::new(
                400,
                10054,
                "message size exceeds maximum allowed",
            ))
        } else {
            None
        };
        if let Some(error) = refused {
            self.received.push(Received {
                connection,
                subject: subject.to_owned(),
                headers,
                data,
                outcome: Outcome::Refused(error.err_code),
            });
            return Published::Refused {
                stream: name,
                error,
            };
        }
        let window = u64::try_from(stream.spec.duplicate_window.as_millis()).unwrap_or(u64::MAX);
        // An id is forgotten once the window has passed, as the server's purge
        // does: `now - stored >= window`.
        stream
            .dedupe
            .retain(|_, (_, at)| now.saturating_sub(*at) < window);
        let id = headers.get(crate::MSG_ID).cloned();
        if let Some(id) = &id
            && let Some((seq, _)) = stream.dedupe.get(id)
        {
            let seq = *seq;
            self.received.push(Received {
                connection,
                subject: subject.to_owned(),
                headers,
                data,
                outcome: Outcome::Duplicate(seq),
            });
            return Published::Ack {
                stream: name,
                seq,
                duplicate: true,
            };
        }
        let seq = stream.store(subject, headers.clone(), data.clone());
        if let Some(id) = id
            && window > 0
        {
            stream.dedupe.insert(id, (seq, now));
        }
        self.received.push(Received {
            connection,
            subject: subject.to_owned(),
            headers,
            data,
            outcome: Outcome::Stored(seq),
        });
        Published::Ack {
            stream: name,
            seq,
            duplicate: false,
        }
    }

    pub(crate) fn create_consumer(
        &mut self,
        stream: &str,
        spec: ConsumerSpec,
        start: Option<u64>,
        deliver_new: bool,
    ) -> Result<String, ApiError> {
        let name = if spec.name.is_empty() {
            let name = format!("eph{}", self.next_ephemeral);
            self.next_ephemeral += 1;
            name
        } else {
            spec.name.clone()
        };
        let stream = self
            .streams
            .get_mut(stream)
            .ok_or_else(ApiError::stream_not_found)?;
        if stream.spec.retention == Retention::WorkQueue && !stream.consumers.is_empty() {
            return Err(ApiError::new(
                400,
                10099,
                "multiple non-filtered consumers not allowed on workqueue stream",
            ));
        }
        let next = match (start, deliver_new) {
            (Some(seq), _) => seq,
            (None, true) => stream.last_seq + 1,
            (None, false) => 1,
        };
        let spec = ConsumerSpec {
            name: name.clone(),
            ..spec
        };
        stream
            .consumers
            .insert(name.clone(), new_consumer(spec, true, next));
        Ok(name)
    }

    /// Hands out at most `batch` messages to a consumer.
    pub(crate) fn pull(
        &mut self,
        connection: u64,
        stream: &str,
        consumer: &str,
        batch: usize,
    ) -> Result<Vec<Handed>, ApiError> {
        let s = self
            .streams
            .get_mut(stream)
            .ok_or_else(ApiError::stream_not_found)?;
        if !s.consumers.contains_key(consumer) {
            return Err(ApiError::consumer_not_found());
        }
        let now = self.now_ms;
        let foreign = self.faults.contains(&Fault::ForeignDelivery);
        let s = self
            .streams
            .get_mut(stream)
            .ok_or_else(ApiError::stream_not_found)?;
        let mut handed = Vec::new();
        while handed.len() < batch {
            let Some(one) = s.claim(consumer, now, foreign) else {
                break;
            };
            handed.push(one);
        }
        for one in &handed {
            let attempt = s
                .consumers
                .get(consumer)
                .and_then(|c| c.counts.get(&one.message.seq))
                .copied()
                .unwrap_or(1);
            self.deliveries.push(Delivery {
                connection,
                stream: stream.to_owned(),
                consumer: consumer.to_owned(),
                seq: one.message.seq,
                attempt,
            });
        }
        Ok(handed)
    }

    /// An acknowledgement on a `$JS.ACK` subject. `None` when no consumer of
    /// that name exists: nobody listens there.
    pub(crate) fn acknowledge(
        &mut self,
        connection: u64,
        subject: &str,
        payload: &[u8],
    ) -> Option<()> {
        let tokens: Vec<&str> = subject.split('.').collect();
        if tokens.len() != 9 || tokens[0] != "$JS" || tokens[1] != "ACK" {
            return None;
        }
        let (stream_name, consumer_name) = (tokens[2], tokens[3]);
        let seq: u64 = tokens[5].parse().ok()?;
        let stream = self.streams.get_mut(stream_name)?;
        let retention = stream.spec.retention;
        let consumer = stream.consumers.get_mut(consumer_name)?;
        let kind = match payload {
            b"" | b"+ACK" => AckKind::Ack,
            b"-NAK" => AckKind::Nak,
            b"+TERM" => AckKind::Term,
            _ => AckKind::Other,
        };
        let was_pending = consumer.pending.contains_key(&seq);
        match kind {
            AckKind::Ack | AckKind::Term => {
                consumer.pending.remove(&seq);
                consumer.delivered_at.remove(&seq);
                consumer.redeliver.retain(|s| *s != seq);
                // As the server does on a work queue: the acknowledged
                // message is removed, whether or not it was still pending.
                if retention == Retention::WorkQueue {
                    stream.messages.remove(&seq);
                }
            }
            AckKind::Nak => {
                if was_pending {
                    consumer.pending.remove(&seq);
                    consumer.delivered_at.remove(&seq);
                    consumer.redeliver.push_back(seq);
                }
            }
            AckKind::Other => {}
        }
        self.acks.push(Acknowledgement {
            connection,
            stream: stream_name.to_owned(),
            consumer: consumer_name.to_owned(),
            seq,
            kind,
            was_pending,
        });
        Some(())
    }

    /// Moves the clock on, and hands out again every delivery whose
    /// acknowledgement wait ran out, as the server does.
    pub(crate) fn advance(&mut self, ms: u64) {
        self.now_ms = self.now_ms.saturating_add(ms);
        let now = self.now_ms;
        for stream in self.streams.values_mut() {
            for consumer in stream.consumers.values_mut() {
                let wait = u64::try_from(consumer.spec.ack_wait.as_millis()).unwrap_or(u64::MAX);
                let due: Vec<u64> = consumer
                    .delivered_at
                    .iter()
                    .filter(|(_, at)| now.saturating_sub(**at) >= wait)
                    .map(|(seq, _)| *seq)
                    .collect();
                for seq in due {
                    consumer.delivered_at.remove(&seq);
                    consumer.pending.remove(&seq);
                    consumer.redeliver.push_back(seq);
                }
            }
        }
    }

    pub(crate) fn redeliver_unacknowledged(&mut self, stream: &str, consumer: &str) -> usize {
        let Some(c) = self
            .streams
            .get_mut(stream)
            .and_then(|s| s.consumers.get_mut(consumer))
        else {
            return 0;
        };
        let pending: Vec<u64> = c.pending.keys().copied().collect();
        c.pending.clear();
        c.delivered_at.clear();
        c.redeliver.extend(pending.iter().copied());
        pending.len()
    }

    pub(crate) fn remove_message(&mut self, stream: &str, seq: u64) -> bool {
        self.streams
            .get_mut(stream)
            .is_some_and(|s| s.messages.remove(&seq).is_some())
    }

    pub(crate) fn declare_stream(&mut self, spec: StreamSpec) {
        self.streams.insert(spec.name.clone(), Stream::new(spec));
    }

    pub(crate) fn declare_consumer(&mut self, stream: &str, spec: ConsumerSpec) -> bool {
        let Some(s) = self.streams.get_mut(stream) else {
            return false;
        };
        let next = if spec.deliver_new { s.last_seq + 1 } else { 1 };
        s.consumers
            .insert(spec.name.clone(), new_consumer(spec, false, next));
        true
    }

    pub(crate) fn truth(&self) -> Truth {
        Truth {
            streams: self
                .streams
                .iter()
                .map(|(name, s)| {
                    (
                        name.clone(),
                        StreamTruth {
                            spec: s.spec.clone(),
                            messages: s.messages.values().cloned().collect(),
                            last_seq: s.last_seq,
                            consumers: s
                                .consumers
                                .iter()
                                .map(|(n, c)| ConsumerTruth {
                                    name: n.clone(),
                                    ephemeral: c.ephemeral,
                                    ack: c.spec.ack,
                                    pending: c.pending.keys().copied().collect(),
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
            now_ms: self.now_ms,
            faults: self.faults.clone(),
            applied: self.applied.clone(),
            received: self.received.clone(),
            deliveries: self.deliveries.clone(),
            acknowledgements: self.acks.clone(),
            api_calls: self.api_calls.clone(),
            connects: self.connects.clone(),
        }
    }
}

fn new_consumer(spec: ConsumerSpec, ephemeral: bool, next: u64) -> Consumer {
    Consumer {
        spec,
        ephemeral,
        next,
        dseq: 0,
        pending: BTreeMap::new(),
        redeliver: VecDeque::new(),
        delivered_at: BTreeMap::new(),
        counts: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_matches_as_the_protocol_says() {
        assert!(matches("a.*", "a.b"));
        assert!(!matches("a.*", "a.b.c"));
        assert!(matches("a.>", "a.b.c"));
        assert!(!matches("a.>", "a"));
        assert!(matches("_INBOX.x.*", "_INBOX.x.7"));
    }
}
