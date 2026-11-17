# 309. The stream pack

Queues, topics, change feeds, and logs: systems reached by waiting for, or adding to, a flow of events. This is the stream archetype of [section 201](../2-kernel/201-archetypes-and-modes.md). The pack is designed here before it is built, as [section 202](../2-kernel/202-kernel-and-packs.md) requires, and this section answers each item of that section's entry bar. Mode: `stream`, registered when the pack was built ([record 0105](../../decisions/0105-the-stream-mode-and-the-stream-pack-as-building-it-settled-it.md)); the same record lists where building changed this section, and [record 0150](../../decisions/0150-a-take-records-its-message-before-it-acknowledges-and-an-answer-with-no-rule-leaves-a-write-in-doubt.md) lists what a safety audit of the code changed.

The stream archetype is the furthest from what the engine loop was first drawn for. The loop assumes a record that is there to be read before and after a write. A message is often not: it is sent, it is taken, and it is gone. This section says plainly where that bends the loop, and what the pack does about it.

## What it is for, and what it refuses

| For | Refused |
| --- | --- |
| Reading the last retained value for a key: the current state a channel holds | A subscription that never ends. A tool call must end |
| Waiting, within a time limit, for the next message that matches a key or a correlation field | Taking a message from a queue that others expect to read, unless the target declares that the runtime owns it |
| Listing, with a bound, what a channel holds now, where the broker offers a look without taking | Choosing the channel from an input. The channel is fixed in the target's files ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)) |
| Publishing one message built from a typed template | A message body passed through as free text from a caller |
| Taking and acknowledging a message on a queue the runtime owns | Starting a run because a message arrived. Triggers are not designed ([readiness](../7-project/704-readiness.md)) |

## The file kind: `channels/`

One file per channel. It holds everything the pack knows about where messages are and what they look like. An operation names a channel; it never names a topic or a queue directly. Field types stay in `types.yaml` and message schemas in their own files under `schemas/`, as rule eight requires. A schema file is YAML and holds the part of JSON Schema the pack checks: an object, `required`, and `properties` with a `type` and an `enum` of texts.

```yaml
channel: orders.status            # module.name, as screens are named
surface: broker                   # a surface of kind broker in target.yaml
address: example.orders.status    # the topic, queue, or feed as the broker names it
shape: retained                   # retained | log | queue | topic
role: state                       # state | events
schema: order_status              # a message schema that a file under schemas/ defines
encoding: json                    # json; text and others when a reader for them exists
key:
  field: order_id                 # the message key; for retained, what one value is kept per
  type: identifier
correlation:
  field: headers.correlation_id   # how a reply is matched to a request
read_through: own_subscription    # own_subscription | browse | none, or a list of the first two
subscription: example.runtime     # the runtime's own subscription, as the broker names it
ownership: shared                 # shared | runtime_owned
consumers: outside                # outside | none | unknown; default unknown
idempotency:
  header: idempotency-token       # where the record's token travels, if consumers honour it
  broker_dedupe: false            # whether the broker itself refuses a repeated token
  # window: 2m                    # with broker_dedupe: how long the broker remembers one
wait:
  default: 10s
  max: 60s                        # the compiler refuses a wait above the target's ceiling
limits:
  browse_count: 100
  message_bytes: 65536
```

| Field | Meaning |
| --- | --- |
| `shape` | `retained`: the broker keeps the last value per key. `log`: the broker keeps messages in order, each at a position it reports. `queue`: a message is delivered to one reader and removed when acknowledged. `topic`: every open subscription gets a copy, and nothing is kept |
| `role` | `state`: the latest value for a key is the record, and a newer one supersedes it. `events`: each message is a thing that happened and is acted on by whoever reads it. A judgment the developer makes; never drafted |
| `read_through` | How the runtime reads without taking from others: a subscription of its own, the broker's browse, both, or not at all. `list` uses the browse and `await` the subscription |
| `subscription` | The runtime's own subscription. On a log or a topic it may be left out: the run then reads through a cursor or a subscription of its own, which the broker discards. On a queue the runtime owns, the consumer a person made for the runtime's takes |
| `ownership` | `runtime_owned` only when the target declares that nothing else reads this queue |
| `consumers` | Who reads what is published. `unknown` is treated as `outside` |
| `wait` | Default and largest wait for this channel; the settings hold a ceiling for all ([section 226](../2-kernel/226-configuration.md)) |

The surface kind `broker` takes one `address`, a `protocol`, and a `rate`, as other surfaces do. The protocols are `nats`, the NATS client protocol with JetStream ([record 0167](../../decisions/0167-the-stream-pack-speaks-nats-with-jetstream-through-a-client-of-its-own.md)), and `example-broker/1`, the test broker's own. Over `nats` a channel that keeps messages has the address `STREAM/subject`, and a topic is a bare subject; the compiler refuses under `stream.protocol_unfit` what the protocol cannot carry. A binding of mode `stream` names a `channel`, a `verb`, and for a publish a `key`, `headers`, and `body`, each a map of name to expression. No body may have a field named `message_meta`: that is the name under which a result holds what the broker says about a message, and a message that uses it is refused when it is read and never sent.

```yaml
operation: example_order_status_set
effect: write
key: order_id
inputs:
  order_id: {type: identifier, required: true}
  status: {type: order_status, required: true}
bindings:
  - mode: stream
    channel: orders.status
    verb: publish
    key: {order_id: input.order_id}
    body: {order_id: input.order_id, status: input.status}
    read_back: {verb: read, channel: orders.status}
```

## Verbs and lowering

The steps are those [section 201](../2-kernel/201-archetypes-and-modes.md) lists: `subscribe`, `await`, `publish`, `acknowledge`. The pack adds two, `latest` and `browse`, because a look that takes nothing is not a subscription. Two of the steps save, and always: `publish` and `acknowledge`. The manifest lists them as the mode's commit steps, and the pack's driver carries out neither unless the plan marks the step as the one that commits. In a sandbox that writes a test record, the driver lifts its guard for that step alone, so a `take`, whose `subscribe` and `await` are not marked, runs only live.

| Verb | Effect class | Lowers to | Returns |
| --- | --- | --- | --- |
| `read` | `read` | `latest` for the key | The value, or absent |
| `list` | `read` | `browse` up to `browse_count` | Messages held now, with coverage |
| `await` | `read` | `subscribe` from a recorded position, `await` a match | The message, or nothing within the limit |
| `publish` | By the table under "Read-back" | `publish`, then the read-back steps | The broker's acknowledgement and position |
| `take` | `irreversible` on the runtime's own queue | `subscribe`, `await`, `acknowledge` | The message |

**Every wait ends.** The compiler refuses an `await` with no time limit, or one above the channel's `max` or the settings' ceiling. There is no verb that keeps a subscription open after the call returns.

**Nothing arriving is an answer, not a failure.** An `await` that sees no match within its limit returns `arrived: false`, how long it waited, and the coverage of what it watched. A binding that needs a message states `on_nothing: fail`, and then the run stops with the class `transient` and the code `nothing_arrived`. A `take` always needs one.

**A wait starts from a recorded position.** In prepare, the driver records the position each subscribed partition stands at and stores it with the run. A resumed wait starts from the same place, so a reply that came while the runner was lost is still found. On a `queue` with no positions, a wait starts at the moment of subscribing and says so as a limitation. So a wait that saw nothing says `coverage: complete` only where its subscription stands at a position the broker reported; elsewhere it says `coverage: partial` with the limitation `starts_at_subscription`. A wait asks for one message at a time, and a broker that hands over more is stopped as `unexpected_reply`.

## The driver, part by part

| Part | What the driver does |
| --- | --- |
| Prepare | Takes the connection the session's home runner holds for this broker ([section 204](../2-kernel/204-supervising-subsystems.md)); opens a producer, or a subscription of the runtime's own, for this run only; checks that the channel exists and its declared shape matches what the broker reports; records start positions |
| Act | Publishes one message, encoded from the typed template and checked against the schema before it is sent; or waits for one match |
| Confirm | The broker's acknowledgement within the step's time limit. For a `log`, the acknowledgement carries the position. For a wait, a message that fits the schema |
| Read back | Only by the rules below |

Connections are long-lived and shared by the runs of one session. Producers and subscriptions are not shared: each run has its own, as each run has its own browser tab ([section 211](../2-kernel/211-concurrency.md)).

## Read-back, and when a publish is irreversible

Read-back is the hard part of this pack. It exists only when the broker keeps what was sent and lets it be read again by something the runtime knows.

| The channel is | Read back by | Effect class | Hazard |
| --- | --- | --- | --- |
| `retained`, `role: state`, `consumers: none` | `latest` for the key, compared after normalization | `write` | `none` |
| `retained`, `role: state`, consumers `outside` or `unknown` | `latest` for the key | `irreversible` | `sends_outside` |
| `log`, any role | Reading the message at the acknowledged position | `irreversible` | `sends_outside` |
| `queue`, or any channel with no retention | None. The acknowledgement is recorded as evidence, and proves acceptance only | `irreversible` | `sends_outside` |

The reasoning is exact. Read-back proves that the broker holds what was sent. It never proves what a reader did with it. Where something outside reads the channel, the message has already left, and [section 213](../2-kernel/213-effects-hazards-and-consent.md) makes any hazard but `none` `irreversible`. The only publish that is an ordinary `write` is a new value for a key on a state channel whose readers the target declares to be none outside it: there, the latest value is the record, and a later value is the correction.

For a `retained` channel, another producer may publish the same key between the write and the read. That is a `mismatch`, read again once and then a stop, as for any record changed by someone else.

## Delivery and duplicates

Brokers deliver at least once. A publish whose acknowledgement was lost may have landed, and sending it again may send it twice. The same holds for an answer the driver cannot read and for a refusal under a code the protocol does not define: neither says that nothing was stored, so each is `possible_commit`, for a publish and for the acknowledgement of a `take` alike. A write is refused only in words the protocol defines for it.

| What the target offers | How a publish is made safe to resume |
| --- | --- |
| The broker refuses a repeated token (`broker_dedupe: true`) | The record's idempotency token from the ledger travels in the declared header. A resend is refused by the broker, and the driver reports the message first stored. Such a broker may remember a token only within a window: the channel file states it as `idempotency.window`, and the driver stops with `shape_changed` where the broker reports a shorter one. The refusal is never the proof: a resume reconciles first, by the scan on a log, which finds the message however long ago it was stored. Identity `stamped` where the channel also keeps what was sent; on a queue or a topic the operation declares no identity, because nothing can be read back |
| A `retained` state channel | Identity `natural` by the key. Reconcile reads `latest`: the intended value means it landed; the value seen before means it did not; anything else is `stale` |
| A `log` with positions | Identity `stamped`. Reconcile scans from the position recorded at intent, on the partitions the key maps to, for the token. Found means it landed. Not found with `complete` coverage of that range means it did not. A look that says more is held and gives no place to go on from, or says that was all while the log's own end lies further on, has not covered the range: it is `unexpected_reply`, never "not found" |
| Consumers that ignore a repeated token, declared | Recorded and shown at approval; it does not change the class. The runtime cannot see a consumer |
| None of these | Identity `none`. A lost acknowledgement is `possible_commit`, the run stops, and a person decides whether to send again |

Because a publish to an outside reader is `irreversible`, it is never resumed automatically, whatever reconcile finds ([section 208](../2-kernel/208-runs-the-ledger-and-resume.md)). Reconcile still runs, and its result is shown to the person who decides: "the message is in the log at this position" is a better basis than nothing.

## Consuming without stealing

Reading a queue removes messages that others expected. The pack's rules:

1. **Read through the runtime's own subscription or consumer group,** named in the channel file. The runtime does not create one during a run or a discovery; creating it is a change to the broker, done by a person or by a declared `write` operation with its own read-back.
2. **Never acknowledge on a shared queue.** `take` compiles only on a channel with `ownership: runtime_owned`.
3. **Otherwise only look.** Where the broker offers a browse or a peek, `list` uses it. Where it does not, and there is no subscription of the runtime's own, the channel cannot be read, and the compiler says so.
4. **Discovery never consumes,** whatever the channel declares.
5. **Record, then acknowledge.** A `take` stores the message in the ledger before it acknowledges. The step that waits gives the whole message, redacted, as values for reconcile, and the engine stores those durably before it sends the acknowledging step ([section 208](../2-kernel/208-runs-the-ledger-and-resume.md)). A message larger than the channel's `limits.message_bytes` cannot be recorded: it is given back to the queue and the run stops with `take_too_large`, never acknowledged. If the run stops before the acknowledgement is sent, the broker delivers the message again. If the acknowledgement was sent and its answer is lost or has no rule, the take is `possible_commit`: the broker may have dropped the message, the record holds it, and nothing is taken in its place until a person has settled the record. This is the only read-like verb with a ledger row. It is declared `irreversible` with no identity: an acknowledged message cannot be read again or put back.

## Identity, sessions, effects

**Identity strategies.** A record is a message, or the value for a key. `natural` by key on a state channel; `stamped` with the token where the broker keeps messages; `find` is not offered, because a lookup over a flow is the bounded scan above and is written as `stamped`; `none` everywhere else.

**Sessions and secrets.** A broker session is a signed-in connection: a user name and password, a token, or a client certificate, held in the vault and injected when the connection opens. The neutral auth providers of `a2m-auth` cover the first two; a certificate provider is the pack's, if the neutral set lacks one. A wire protocol presents what it can carry, and a session that holds a credential it cannot carry is stopped with `credential_not_carried` before a connection opens; `nats` carries a token or a user and password, `example-broker/1` one token. Plain TCP goes only to this machine's loopback address and to a server that does not ask for TLS; anything else is `transport_refused`, before anything is sent. The session keeper renews; the driver reports signs of expiry, such as an authorization refusal on a connection that worked a moment ago. No credential is ever a message header the caller can see: of a message's headers a result holds only the one the channel declares for correlation. A message body is checked by the redactor before it is stored, and a broker's words are redacted before they are cut for a failure's sentence.

**Text from a message is untrusted.** An output of an operation bound through this mode that is or holds text must be of a field type marked `untrusted` ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)), unless the channel's schema holds the field of that name to an `enum`. The compiler refuses the rest under `stream.untrusted_unmarked`.

**Effects** in the manifest: uses the network, to the broker addresses the target declares; holds a session, for surfaces of kind `broker`. Both `enforced`, since the pack's client performs them.

**Hazards.** `sends_outside` on every publish an outside reader may see, with a person approving every run and no automatic retry ([section 213](../2-kernel/213-effects-hazards-and-consent.md)). One approval still covers a whole pipeline's diff, so forty messages from one sheet are one decision. The diff for a publish shows the message as a create, with the channel, key, and body; for a state channel, with the value it replaces.

## Discovery and drafting

The discoverer reads what [section 218](../2-kernel/218-discovery-and-findings.md) lists for streams, within its bounds.

| It reads | Finding | Level |
| --- | --- | --- |
| Topics, queues, and partitions the broker lists | A channel with its shape | `discovered` |
| Schemas the broker or a schema registry holds | A message schema | `inferred`: a registered schema is a claim about messages |
| A bounded sample, through browse or a subscription that the broker discards on close and that takes nothing | The shape of messages: names and types, never values | `discovered`, with the limitation `sampled` |
| The retention and the key the broker reports | Whether a channel is `retained` or `log` | `discovered` |

Where no such look exists, no sample is taken and coverage is `unknown`. The scout tries a drafted `read` of a retained key and a drafted `list`, five times each. It does not try `await` on a live system, because the next message is not the same twice: reading a flow is not repeatable, and the system says so. A drafted `await` stays at `reconstructed` until the publish gates prove it against the test stand.

| From a finding about | The drafter writes | It leaves to the developer |
| --- | --- | --- |
| A listed channel with a schema | A channel file, with shape, key, and schema; a `read` or `list` operation | `role`, `consumers`, `ownership`, every correlation field |
| A sampled shape with no schema | A schema marked `inferred` and an open question | Confirming the schema; every operation |

The drafter never writes a publish, a `take`, or a subscription name.

## Where the loop fits, and where it strains

| Part of the loop | Fit |
| --- | --- |
| Guard, choose, intend, record | Unchanged |
| Prepare, act, confirm | Fit cleanly. One publish is one commit point |
| Read back | Fits only for retained and logged channels. Everywhere else the write is `irreversible`, and most publishes are |
| Preview | A publish has no current value except on a state channel. The diff is a create, every time |
| Reads | A wait is a read that is not repeatable and that can take as long as its limit. It holds a runner for that time, and the trace signature of two waits differs in what arrived |
| Ledger | A read normally has no record row. `take` needs one, so its acknowledging step commits |

These strains are contained in the pack, and none needs a kernel change. One may: a long wait holding a runner may need the scheduler to count waits apart from other reads. That is recorded as evidence for hypothesis H-26 when the pack is built, and H-29 is tested by whether the stream archetype held without a special case in the engine.

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `input` | `schema_invalid`, `value_out_of_range`, `message_too_large`, `protocol_unknown`, `credential_not_carried` | The message fails its schema or the size limit, the surface names a protocol this build does not speak, or the session holds a credential the protocol cannot present; nothing is sent |
| `policy` | `ack_not_owned`, `surface_not_declared`, `scan_limit`, `take_too_large`, `transport_refused` | A `take` on a shared queue; a broker address not declared; a log longer than a scan for a record's token may read; a message taken that is too large to record before it is acknowledged; plain text to an address off this machine, or to a server that asks for TLS |
| `session_expired` | `needs_login` | The broker refuses the credentials |
| `rejected` | `broker_refused` | The broker refuses the message, with its reason |
| `drift` | `channel_missing`, `shape_changed`, `response_schema_mismatch`, `position_out_of_range` | The channel is gone, its shape or key changed, a message fails its schema, or a position is no longer held |
| `unknown_variant` | `unexpected_reply` | The broker answers a read in a way the driver has no rule for; a look at a log does not say how far it reached; a fetch hands over more than was asked |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable`, `nothing_arrived` | Before a publish was sent, or on a read |
| `possible_commit` | `no_confirmation` | A publish or an acknowledgement was sent, and no answer came or the answer has no rule |
| `mismatch` | `read_back_differs` | The latest value or the message at the position differs |
| `busy` | `subscription_in_use` | The runtime's own subscription is held by another run |

Codes not already in the technical reference are proposed.

## The test stand

A small broker in the test application family ([test application](../../plan/test-application.md)), written in Rust, on a local port. It exists at `targets/example-broker` ([README](../../../targets/example-broker/README.md)). It speaks a small line-delimited JSON protocol of its own, written before a first standard protocol was chosen, and stays as the pack's second way in. It holds four channels: retained, log, queue, and a plain topic that keeps nothing.

| Control call | Reports or does |
| --- | --- |
| Reset, Inspect | Restore known contents; return every message a channel holds, with position, key, and headers |
| Received | Every message published, by which connection, in order, with a digest of its body |
| Delivered | Every delivery: to which subscription, whether acknowledged, how many times |
| Faults | Store and then drop the acknowledgement; delay; refuse; deliver twice; expire retention; refuse a repeated token or accept it; an unexpected reply; a consumer that never acknowledges; a renamed field; a drop before the message is stored; store or acknowledge and then answer with an unknown code; acknowledge and then drop the answer; a look that returns nothing; a fetch that hands over more than was asked |

A test that a publish happened once asks Received, never the pack's own read.

A second stand, `targets/example-nats` ([README](../../../targets/example-nats/README.md)), speaks the part of the NATS client protocol and the JetStream interface the pack's client uses, with the same kind of control interface and faults, and a clock a test moves to cross a duplicate window. It was written from the same documents and source as the client, so one test also runs the client against a real server when `A2M_TEST_NATS` names one; record 0167 says what only that test can show.

## What it needs from outside

The client of the first standard protocol is the pack's own, on Tokio, and adds no crate (record 0167). Still needed, each an outside crate to be verified: TLS for that client, and readers for any binary schema format beyond JSON. Needs no process of its own.

## Build order

1. The `channels/` schema, lint rules, and the `broker` surface kind; the test stand with Reset, Inspect, and Received.
2. `read` of a retained key, through the same engine loop, with an identical trace signature across five runs.
3. `list` by browse, and `await` from a recorded position, including the answer when nothing arrives.
4. The discoverer and drafting rules, against the test stand.
5. `publish` to a state channel with `consumers: none`, as a `write` with read-back, with the fault tests: lost acknowledgement, duplicate, stale value.
6. `publish` as `irreversible` with `stamped` reconcile on a log.
7. `take` on a runtime-owned queue, last.

## Open points

- Settled: the first standard protocol is NATS with JetStream, through a client of the pack's own ([record 0167](../../decisions/0167-the-stream-pack-speaks-nats-with-jetstream-through-a-client-of-its-own.md)), as [research note 22](../../research/22-stream-wire-protocol.md) recommended. MQTT 5 may come second, for retained and topic channels. A protocol is one interface inside the pack, and [record 0104](../../decisions/0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md) says what it must offer.
- Whether waits need their own count in the scheduler's budgets.
- Whether a transactional publish across several channels is one commit point. Not offered until a target needs it.
- Triggers: whether a matching message may start a run. Still not designed.
- The ceiling on a wait, which is a target until measured.
