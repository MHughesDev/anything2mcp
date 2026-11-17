# Research: the first standard wire protocol of the stream pack

**Status:** completed on 2026-11-17 from documentation and source read, and from a licence and size check of each client crate. Nothing was run against a broker. It answers brief 32 of [open research](11-open-research.md), which was added to carry the open point "Which protocol first" of [spec section 309](../spec/3-packs/309-the-stream-pack.md) and [decision 0104](../decisions/0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md). It feeds roadmap packages J2 and J3 (their "no standard protocol yet").

Two hosts were blocked in this session: the OASIS and Apache Kafka documentation sites. What this note says about MQTT, AMQP, and Kafka is therefore from the standards as the author knew them and from client crates read on the registry, and is marked so. What it says about NATS is from the server's source and the project's documentation on GitHub, which could be read.

## Question

Decision 0104 lists nine things any protocol must offer the pack's interface. Which open protocol offers the most of them, with a client that fits the closed licence list and runs on Windows, macOS, and Linux? Should the client be a crate or our own code?

## Answer

**NATS with JetStream.** It is the only candidate that offers all four channel shapes of section 309 and the one thing that is hardest to find in a broker: the broker itself refuses a repeated token and names the message it stored first. Write a small client of our own, as the pack already did for the stand's protocol. Keep `async-nats` as the alternative.

**MQTT 5 is the second protocol, not the first.** It serves the `retained` and `topic` shapes well and nothing else.

## What each protocol offers, against decision 0104's table

"Yes" means read in a source named below. "Std" means known from the standard and not re-read here. "No" means the protocol has no such part.

| Requirement | NATS and JetStream | MQTT 5 | AMQP 1.0 | Kafka |
| --- | --- | --- | --- | --- |
| Retained value per key | Yes. A stream that keeps the last message per subject; a get of the last message for a subject | Std. One retained message per topic | No | Std. A compacted topic, read by scanning |
| Log with a position per message | Yes. Every stored message has a stream sequence, and the publish confirmation carries it | No | No | Std. Offsets |
| Queue with acknowledge and release | Yes. A work-queue stream and an explicit-acknowledge consumer; a negative acknowledgement gives a message back | No. A shared subscription spreads load, nothing more | Std. Accept and release outcomes | No. Offsets are committed, one message is not released |
| A look that takes nothing | Yes. A read of one message by sequence, or the last by subject, through the stream's own API; no consumer is made | Partly. A retained message is delivered on subscribe | Partly. A link in browse mode, if the broker offers it | Std. A fetch from an assigned offset takes nothing |
| A subscription the broker discards with the connection | Yes. A plain subscription is the connection's; an ephemeral consumer is removed after an inactive time | Yes. A clean session | Std | Yes, without a group |
| The broker refuses a repeated token and names the first message | **Yes.** A header, `Nats-Msg-Id`; the confirmation says `duplicate` and carries the stored sequence | No | No | No. The idempotent producer works by producer and sequence, not by a header the pack sets |
| Confirmation of a publish with identifier and position | Yes. `stream`, `seq` | Std. A packet identifier, no position | Std. A disposition | Std. The offset |
| For every failure, whether the request was sent | Partly. A 503 "no responders" status comes back at once when JetStream could not serve; a missing confirmation is unknown | Std. Unknown after a lost acknowledgement | Std | Std |
| A cheap test of an open connection | Yes. PING and PONG | Std | Std | Std |

NATS misses none of the nine. MQTT misses four. Kafka's protocol is a set of versioned binary requests of a size that makes a client of our own a large piece; its one client crate that is complete, `rdkafka`, wraps a C library.

## What was read, for NATS

| Claim | Source and how it was read |
| --- | --- |
| The client protocol is text lines ending in carriage return and line feed, with a short list of operations: `INFO`, `CONNECT`, `PUB`, `HPUB`, `SUB`, `UNSUB`, `MSG`, `HMSG`, `PING`, `PONG`, `+OK`, `-ERR`. `HPUB` and `HMSG` carry headers | [Client protocol](https://github.com/nats-io/nats.docs/blob/master/reference/nats-protocol/nats-protocol/README.md), read in full |
| The JetStream API is requests and JSON answers on subjects under `$JS.API`, so one client of the core protocol reaches it | [JetStream JSON API design](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-1.md), and the server source below |
| A repeated `Nats-Msg-Id` inside the stream's duplicate window is not stored again | [Message deduplication](https://github.com/nats-io/nats.docs/blob/master/using-nats/jetstream/model_deep_dive.md) |
| The confirmation of a publish is `{"stream", "seq", "duplicate"}`, and for a duplicate the sequence is the one stored first | [Server source, `PubAck`](https://github.com/nats-io/nats-server/blob/main/server/stream.go) (struct at line 260; the duplicate branch at line 7113 answers with the stored entry's sequence and `"duplicate": true`) |
| The default duplicate window is 2 minutes, it cannot be below 100 ms, and it cannot exceed the stream's maximum age | Same file: constant `StreamDefaultDuplicatesWindow`, and the configuration checks near line 2060 |
| JetStream's base quality of service is at least once. A publisher that did not see the confirmation may publish again; the duplicate header is how that is made safe | [JetStream overview](https://github.com/nats-io/nats.docs/blob/master/nats-concepts/jetstream/README.md) |
| Consumers can start by all, last, new, last per subject, a start sequence, or a start time; acknowledgement is none, all, or explicit; an ephemeral consumer has no persisted state and is cleaned up after an inactive time | [Consumers](https://github.com/nats-io/nats.docs/blob/master/nats-concepts/jetstream/consumers.md) |
| A work-queue retention keeps a message until it is acknowledged | [Streams](https://github.com/nats-io/nats.docs/blob/master/nats-concepts/jetstream/streams.md) |
| A message can be read by sequence, or the last for a subject, without a consumer ("direct get"), and the 2.11 server adds batch reads and a cursor | [ADR-31](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-31.md); revision table read, the request shapes not yet |
| A key-value bucket is a stream whose name starts `KV_`, with get, put, delete, create-if-absent, and compare-and-set | [Key/value store](https://github.com/nats-io/nats.docs/blob/master/nats-concepts/jetstream/key-value-store/README.md) |
| JetStream answers with a 503 status when no one could serve the publish | [ADR-22](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-22.md) |
| Reads through direct get may be served by followers, so a read right after a write may not see it on a cluster | The key-value document above states that read-your-writes is not guaranteed |
| The server's licence is Apache 2.0 | [LICENSE](https://github.com/nats-io/nats-server/blob/main/LICENSE) |

## The client crates

All checked on the crates.io interface on 2026-11-17. The licence tree and size are from a throwaway project outside the repository, resolved at the exact version, and checked with `cargo-deny` against this repository's `deny.toml`. Nothing was compiled.

| Crate | Version, date | Licence | Crates in the tree | Fits the list | C code | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| `async-nats`, features `ring` and `jetstream` only | 0.53.0, 2026-11-17; 74 releases, the last five in 5 months | Apache-2.0 | 96 | Yes; the tree needs nothing beyond the licences already on the list (ISC for ring, rustls) | `ring` | The best crate; refused for now on size and control |
| `rumqttc`, features `use-rustls-no-provider` | 0.25.1, 2025-11-21; five releases in two years | Apache-2.0 | 43 | Yes | `ring` | The second protocol's client, when it is wanted |
| `paho-mqtt` | 0.14.0, 2026-03-26 | EPL-2.0 | | **No.** Not on the list | A C library | Refused |
| `lapin` (AMQP 0-9-1) | 4.12.3, 2026-10-18 | MIT | | Not resolved | | Not needed: no log, no retained value |
| `rdkafka` | 0.39.0, 2026-01-25 | MIT | 34 | Yes | Builds the `librdkafka` C library | Refused: Rust first (rule 6), and Kafka lacks the token refusal |

Why our own client and not `async-nats`: the pack's interface needs, for every call, a deadline and a three-way answer, "not sent, answered, or unknown fate" (decision 0104). A crate's error type decides what a lost connection means, and the pack must decide that. The protocol's core is a dozen text operations and the JetStream calls the pack needs are about eight JSON requests (stream info, get by sequence, get last by subject, publish, create and delete an ephemeral consumer, fetch, acknowledge, negative acknowledge). The one outside piece that should not be written twice is TLS, and `tokio-rustls` is a two-crate addition (see [note 24](24-turning-tls-on.md)). This is judgment. If the client turns out larger than 1,500 lines, take `async-nats` behind the same private trait and record it.

## What it means for the design

- **Section 309 "Which protocol first"** can be settled: NATS with JetStream, as one `Protocol` behind the pack's private trait. `protocol: nats` is the word on a `broker` surface.
- **`broker_dedupe: true`** has a real implementation: the record's token goes in `Nats-Msg-Id`. A resumed publish gets `duplicate: true` and the first sequence, which is the "reports the message first stored" row of 0104.
- **The duplicate window is a limit the design does not yet state.** A token is remembered for 2 minutes by default, and cannot be remembered longer than the stream keeps messages. A run resumed after the window can publish twice. The channel file needs the window as a declared value (`idempotency.window`), the compiler must refuse a resume past it for a `broker_dedupe` channel, and reconcile past it must use the scan of the log from the recorded position, as for a log without dedupe. This is a correction to the table in section 309 "Delivery and duplicates", which today treats the broker's refusal as unlimited.
- **Retained, log, queue, and topic** map to: a stream keeping the last message per subject, a stream with limits retention, a stream with work-queue retention, and a plain subscription.
- **A look that takes nothing** is the direct get. It needs the stream to allow direct reads, which is a setting of the stream and a person's choice, as the section says of every change to a broker.
- **A queue's browse** is not shown to exist for a work-queue stream, and was not found in the documents read. If direct get is refused on a work-queue stream, `list` on a queue is refused at compile time under 0104's last sentence. This must be tried against a real server before J2's queue case is built.
- **Text-line protocol and a log:** a message body is bytes. The pack's JSON encoding is unchanged.

## What was not verified

- **Nothing was run against a NATS server.** A download of the server binary was declined by the session's permission check, so no duplicate, direct get, or acknowledge was seen on the wire. Everything above is source and documentation.
- Whether direct get works on a work-queue stream, and the exact request and answer of the batch form.
- That a 503 reply always means "not stored". ADR-22 describes it as the reply to a publish JetStream could not serve; the server source was not traced through every path.
- MQTT, AMQP, and Kafka rows: the standards' sites were blocked.
- Windows and macOS: NATS needs only a socket. The crate trees were not built on any system.
- The size of our own client, which is an estimate.

## Recommendation, and the alternative that lost

Choose NATS with JetStream as the first standard protocol; write the client in the pack; add MQTT 5 second, through `rumqttc`, for retained and topic channels only; add an `idempotency.window` field. The alternative that lost is MQTT 5 first, because the section's likely guess ("small open specification, retained values per key") fits it, and it is an international standard. It cannot give a log position, a queue, or a refusal of a repeated token, so the pack would be proven in part, which decision 0104 refused once already. The second alternative, `async-nats`, loses on size and on who decides what a lost connection means, and stays available behind the trait.
