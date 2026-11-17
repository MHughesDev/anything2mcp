# 0167. The stream pack speaks NATS with JetStream, through a client of its own

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point "Which protocol first" of [spec section 309](../spec/3-packs/309-the-stream-pack.md), left open by [decision 0104](0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md), as [research note 22](../research/22-stream-wire-protocol.md) recommended. **Changes** sentences of section 309 and the design of [`a2m-pack-stream`](../architecture/crates/a2m-pack-stream.md). **Adds** the test stand `targets/example-nats` ([README](../../targets/example-nats/README.md)).

## Decision

**The protocol.** `nats`: the NATS client protocol, with the JetStream interface reached as requests on subjects under `$JS.API.`. It is a second implementation of the pack's private `Protocol` and `Link` traits; `example-broker/1` stays. A surface of kind `broker` says which it speaks, and nothing in the kernel names either.

**A client of the pack's own,** in `src/nats_wire.rs`: 1,842 lines before its unit tests, 1,621 of them neither blank nor comment, as the formatter lays them out. That is above the 1,500 lines at which note 22 said to take `async-nats` instead. It is kept: about a third of it is the JetStream mapping and the rule for each answer's fate, which the pack would write over `async-nats` too, and the crate would bring 96 crates and C code for the rest. No outside crate was added. `async-nats` stays the alternative behind the same traits.

**How a channel's address names what it reads.** `STREAM/subject` for a stream: the stream's name, and the subject its messages are published on, a message's key as the subject's last token. A bare subject is a topic, which no stream keeps. A channel is the whole stream: the driver reports another shape when the stream stores a subject that is not the channel's, and when a topic's subject is one a stream stores. A key that is not one token (holding `.`, `*`, `>`, or a space) is refused unsent, because it would move the message to another subject ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)). A new lint rule, `stream.protocol_unfit`, refuses at compile time what the protocol cannot carry.

**Each behaviour the pack's safety rests on, from the server's source** (`nats-io/nats-server` at commit `582fb8e9`):

| Behaviour | Source | What the pack does |
| --- | --- | --- |
| A look that takes nothing | `jsMsgGetRequest` in `jetstream_api.go`: the stream's own get, by sequence, by the next at or after a sequence for a subject filter, or by the last of a subject; answered by the stream leader; no consumer; no retention check, so it reads a work queue too | `latest` and `browse` use it. The direct get was not chosen: it needs a stream setting a person must turn on, and may be answered by a follower that has not seen the last write. An ordered consumer was not chosen: it makes server state for a look |
| The duplicate window | `checkMsgId`, `purgeMsgIds`, `storeMsgIdLocked`, `rebuildDedupe` in `stream.go`; `StreamDefaultDuplicatesWindow` is two minutes, at least 100 ms, never above the maximum age | A channel file states it, `idempotency.window`; the driver stops with `shape_changed` when the stream reports a shorter one, or no refusal of a repeated token in the declared header |
| What a publish acknowledgement carries | `PubAck` and the hand-written answers in `processJetStreamMsgWithBatch`: `stream`, `seq`, and for a duplicate `"duplicate": true` with the sequence first stored; an error object beside `stream` | The receipt is read whole before anything else; one that names another stream, or sequence 0, has an unknown fate |
| An expected-sequence mismatch | Error 10071, and nothing stored | Mapped to `rejected`/`broker_refused`. The pack sends no expected-sequence header yet |
| Redelivery and acknowledgement of a pulled message | `processAck`, `processAckMsgLocked`, `processNak`, `sendAckReply` in `consumer.go`: an acknowledgement sent with a reply is answered with an empty message once carried out; on a work queue it removes the message, pending or not; the answer is skipped when the consumer is closed; an unacknowledged message comes back after the acknowledgement wait | A take pulls one message from the consumer a person made, gives the whole message out to be recorded, and acknowledges with a reply. The empty answer is the confirmation; none is `possible_commit`; a `503` (nobody listens: the consumer is gone) acknowledged nothing. What the run let go unacknowledged is given back with `-NAK` |
| Maximum payload | `maxPayloadViolation` in `client.go`; `MAX_PAYLOAD_SIZE` 1 MiB; error 10054 for a stream's own limit | The `INFO` limit is held before sending: `message_too_large`, unsent |
| The error codes | `errors.json` | 10059, 10014, 10037, 10099, 10071, 10070, 10060, 10109, 10077, 10023, 10002, 10054, 10097, 10076, 10039, 10008 each have a rule. Any other code, any `-ERR` line but the two that close the door, any status but those of a pull, and any line the protocol does not have is `unexpected_reply` on a read and `possible_commit` on a publish or an acknowledgement |

**The duplicate window, and a resume past it.** The server forgets a message id once the window has passed, and after a restart remembers only the ids of messages it still holds, so on a work queue an acknowledged message's id is gone. The refusal of a repeat is therefore a second guard and never the record's proof. Every resume of a stamped publish reconciles first, as rule 3 says: on a log by the scan for the record's token, which finds the message however long ago it was stored; on a queue, where nothing can be read back, the engine stops the record in doubt. The test stand shows the cost of not doing so: the same token sent past the window is stored twice.

**Credentials.** A token is `auth_token` and a user and password are `user` and `pass` of `CONNECT`, from the session's lease only. The neutral holders place both as one header; the driver reads a `Bearer ` token, and a `Basic` value back into its user and password. The example protocol still carries a token only. **Transport.** Plain TCP to an address every resolution of which is a loopback address, and only to a server whose `INFO` does not ask for TLS; anything else is `policy`/`transport_refused`, a new code, before anything is sent. TLS is another package's.

**Waits.** On a log, through an ephemeral pull consumer with no acknowledgement, made at the run's position or at the stream's next, and deleted when the run lets go; the server removes it after 30 s unused in any case. Through a named consumer, which must pull and must acknowledge nothing (a work queue's must acknowledge explicitly). On a topic, through a plain subscription of the connection. The channel file may now leave `subscription` out on a topic, and name one on a queue the runtime owns, which is the consumer a take pulls from; on NATS the lint requires it.

## What the stand proves, and what only a real server can

The stand imitates the protocol from the same documents and source as the client, so a misreading shared by both passes every test. Against the stand, 15 tests of `tests/nats.rs` and a unit test of the client prove each rule above by asking the stand what it stored, delivered, and was told, including every fault: a lost or unreadable acknowledgement of a publish, a connection closed after the store, a refusal, a lost answer to an acknowledgement, a consumer gone before it, a pull that hands over more than asked, a line with no rule, a window shorter than the file's, a refused session, a server that asks for TLS, and an address off this machine.

Only a real server can show: that the answers' exact forms are as read (the base64 of the stored get, the acknowledgement subject's token count, the hand-written duplicate answer, the 408 that ends a pull, the empty answer to an acknowledgement); that `max_msgs_per_subject` and `retention` are reported as read; that an ephemeral consumer's position is `delivered.stream_seq + 1`; and everything the stand leaves out (clusters, a purge timer, restarts, permissions). The test `the_same_client_against_a_real_server` runs when `A2M_TEST_NATS` names a server: it makes its streams through requests of its own, reads, publishes one token twice, and takes one message, and reads the server's own counts through requests of its own. It was not run here: there is no server on this machine. None of the faults can be made on a real server.

## Alternatives weighed

- **`async-nats` 0.53.0.** Complete and Apache-2.0, with 96 crates and C code below it. Its error type decides what a lost connection means, and the pack must decide that per request (decision 0104). Kept as the alternative.
- **The direct get for looks.** Batches, and one request a page; refused for the two reasons in the table.
- **Trusting the duplicate window on a resume.** Simpler, and wrong after two minutes; the scan is already the reconcile of a stamped log.
- **Making an ephemeral consumer for a take.** It works on a work queue only while no other consumer exists, and section 309 says the runtime never makes one on a queue.
- **Keeping every pulled delivery to give back.** On a consumer that acknowledges nothing a `-NAK` means nothing; only a work queue's deliveries are held.

## Consequences

- `PROTOCOLS` is `nats`, `example-broker/1`. A target may say `protocol: nats`.
- New: the failure code `transport_refused` (policy); the lint rule `stream.protocol_unfit`; the channel key `idempotency.window`; `subscription` on a queue the runtime owns.
- The interface: `Protocol::carries`, and `connect` takes a credential, a token or a user and a password. `ChannelFacts` reports the window. A consumer that is gone is a refusal of an acknowledgement, not an answer with no rule.
- The CI owes a job that starts a real server and sets `A2M_TEST_NATS`; until it runs, the real-server lines of the table above are unproven.

## What would change it

- The real-server test failing: the stand and the client are corrected together, from what the server did.
- A cluster as a target: whether the stored get from the leader still holds, and the clustered paths of `processJetStreamMsg`, need reading.
- Writes that should be compare-and-set on a state channel: the expected-sequence headers, which the stand would then imitate.

## Sources

- [Client protocol](https://github.com/nats-io/nats.docs/blob/master/reference/nats-protocol/nats-protocol/README.md)
- [JetStream JSON API design](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-1.md)
- [Server source, `stream.go`](https://github.com/nats-io/nats-server/blob/582fb8e939a77b6a33ef845955a208bdc222d767/server/stream.go)
- [Server source, `consumer.go`](https://github.com/nats-io/nats-server/blob/582fb8e939a77b6a33ef845955a208bdc222d767/server/consumer.go)
- [Server source, `jetstream_api.go`](https://github.com/nats-io/nats-server/blob/582fb8e939a77b6a33ef845955a208bdc222d767/server/jetstream_api.go)
- [Server source, `client.go`](https://github.com/nats-io/nats-server/blob/582fb8e939a77b6a33ef845955a208bdc222d767/server/client.go)
- [Server source, `errors.json`](https://github.com/nats-io/nats-server/blob/582fb8e939a77b6a33ef845955a208bdc222d767/server/errors.json)
