# The test broker

A small local broker for the imaginary `example` target. The stream pack is proven against this broker and never against a real one. The pack is designed in [spec section 309](../../docs/spec/3-packs/309-the-stream-pack.md); the rules every test stand follows are in the [test application plan](../../docs/plan/test-application.md) and in [the other parts](../../docs/architecture/other-parts.md).

It imitates what makes a flow of messages hard to drive, and nothing else: a message that is gone once it is taken, delivery at least once, a confirmation that can be lost after the message was stored, and a look that must take nothing from anyone. It keeps everything in memory, and keeps no clock: nothing it does depends on the time of day.

## Starting it

```rust
use example_broker::harness::TestBroker;

let broker = TestBroker::start().await?;          // loopback, a free port, the seed data
let address = broker.address();                   // give this to the code under test
let mut outside = broker.client("outside").await?; // an ordinary client, to act as someone else
// ... the code under test talks to `address` ...
let received = broker.received();                 // ground truth
```

The broker stops, with every connection it holds, when the `TestBroker` is dropped. Two brokers in one test share nothing. There is no binary and no fixed port.

## The wire protocol, and why it is our own

**The choice.** The broker speaks `example-broker/1`: a line protocol of our own, JSON over TCP. Section 309 names no protocol; which one the pack speaks first is one of its open points, and it says a log with positions may need a second protocol. The stand has to hold a channel of every shape the section lists (retained, log, queue), with a look that takes nothing and a broker that can refuse a repeated token. No one small open protocol offers all of that, so an honest subset of a standard would have covered part of the pack and needed a binary codec to do it. The alternative weighed was exactly that subset; it was put off, not rejected. When the pack's first slice chooses its protocol, that protocol is added here as a second way in to the same broker: `src/server.rs` is the only file that knows the wire, and `src/state.rs` holds every rule.

**Framing.** One JSON object on one line each way, UTF-8, ended by a line feed; a carriage return before it is ignored. The client sends a request; the broker sends exactly one answer to each request, in order, and never sends anything else. A request line over 1,048,576 bytes is refused. Field order carries no meaning.

**A request** has `op`, and should have `id`, any JSON value, which the answer returns as `re`. **An answer** has `re` and `ok`. When `ok` is false it also has `error`, a code from the table below, and `message`.

```text
> {"id":1,"op":"hello","client":"outside"}
< {"re":1,"ok":true,"protocol":"example-broker/1","connection":"c-1"}
> {"id":2,"op":"publish","channel":"example.orders.status","key":"A-100","headers":{"correlation_id":"r-1"},"body":{"order_id":"A-100","status":"shipped"}}
< {"re":2,"ok":true,"id":"m-1","position":null}
> {"id":3,"op":"latest","channel":"example.orders.status","key":"A-999"}
< {"re":3,"ok":true,"message":null}
> {"id":4,"op":"fetch","subscription":"s-9","max":5,"wait_ms":100}
< {"re":4,"ok":false,"error":"unknown_subscription","message":"This connection holds no such subscription."}
```

A message, wherever it appears, is `{"id", "channel", "key", "headers", "body", "position"}`. `key` and `position` may be null; `headers` is an object of text values; `body` is any JSON value. `position` is set only on a log.

| `op` | Fields | Answer | Notes |
| --- | --- | --- | --- |
| `hello` | `client`, `token` | `protocol`, `connection` | Optional unless a token is required. Names the connection in ground truth |
| `channels` | | `channels`: a list of channel descriptions | Takes nothing |
| `describe` | `channel` | `channel`: `address`, `shape`, `key_field`, `schema`, `dedupe_header`, `max_body_bytes`, `held`, `start`, `end`, `subscriptions` | `start` and `end` are a log's first held position and next position. `subscriptions` lists the declared ones |
| `publish` | `channel`, `body`, and optionally `key`, `headers`, `retain` | `id`, `position` | The confirmation. One message per request |
| `latest` | `channel`, `key` | `message`, or null | Retained channels only. Takes nothing |
| `browse` | `channel`, `max`, and on a log `from` | `messages`, `more`, `held`, `start`, `end` | Takes nothing. On a queue each message also has `state` (`ready` or `in_flight`) and `attempts`. A topic answers `not_supported` |
| `subscribe` | `channel`, and optionally `name`, `from`, `with_retained` | `subscription` (the handle), `name`, `position` | See the shapes below |
| `fetch` | `subscription`, `max`, and optionally `wait_ms` | `messages`: each `delivery`, `attempt`, `retained`, `message`; and `next`, a log's next position | The bounded read |
| `ack` | `subscription`, `delivery` | | Queues only |
| `release` | `subscription`, `delivery` | | Queues only: gives the message back for redelivery |
| `unsubscribe` | `subscription` | | Unacknowledged deliveries go back to the queue |

**Every read is bounded.** `fetch` returns at most `max` messages (1 to 1000, required) and waits at most `wait_ms` (0 to 60000, default 0) for the first. It answers as soon as one message can be handed over, and answers with an empty list when the wait runs out: nothing arriving is an answer, not an error. There is no request that keeps delivering after its answer.

| `error` | When |
| --- | --- |
| `bad_request` | The line is not one JSON object, or a field is missing or of the wrong kind |
| `unknown_op` | No such request |
| `not_authorized` | A token is required and this connection's `hello` did not present it |
| `channel_missing` | No such channel |
| `not_supported` | The request does not fit the channel's shape |
| `key_required` | A publish to a retained channel without a key |
| `message_too_large` | The body's canonical JSON is over the channel's `max_body_bytes`. The answer carries the limit |
| `refused` | The refusal fault. `message` is the reason |
| `duplicate_token` | The channel's de-duplication header repeats a value already stored. The answer carries the `id` and `position` of the message first stored |
| `subscription_missing`, `subscription_in_use` | A named subscription is not declared, or another connection holds it |
| `unknown_subscription`, `unknown_delivery` | The handle or the delivery tag is not held by this connection |
| `wait_too_long` | `wait_ms` over 60000 |
| `position_out_of_range` | A log position before `start` or after `end`. The answer carries both |

## The four shapes

| Shape | Publish | Reading | Kept |
| --- | --- | --- | --- |
| `queue` | Joins the end | `subscribe` attaches a consumer; `name` is only its label, and many may attach. Each message goes to one consumer and must be acknowledged. It is delivered again, with `attempt` one higher, after `release`, after `unsubscribe`, when the consumer's connection ends, or when the control interface forces redelivery. No positions | Until acknowledged |
| `topic` | Copied to every subscription open at that moment | `subscribe` without `name` opens a subscription the broker discards when the connection lets go. With `name` it attaches to a declared, durable one, which collects copies while nobody holds it and which one connection at a time may hold. No acknowledgement | Nothing |
| `retained` | As a topic; and unless `retain` is false, the message becomes the value for its key | As a topic; a new unnamed subscription is first handed the retained values, in key order, marked `retained`, unless `with_retained` is false. `latest` reads one key | The last retained value per key |
| `log` | Appended at the next position, which the confirmation carries | A subscription is a cursor. Unnamed, it starts at the end; `from` sets it. No acknowledgement. `browse` reads from any held position | Everything, until retention is expired |

**De-duplication.** A channel may declare a header (`dedupe_header`). A publish whose value for that header was already stored on the channel is refused with `duplicate_token` and stores nothing, so a publish that is sent again after a lost confirmation delivers one message. Without the declaration the broker stores every repeat.

**Identifiers** count up from one after each reset, so a test can name them: messages `m-1`, deliveries `d-1`, subscription handles `s-1`. Seed messages are `s-1` to `s-7`. Connections are `c-1` and up, and are not reset.

## Seed data

| Channel | Shape | Holds | Declares |
| --- | --- | --- | --- |
| `example.orders.status` | `retained` | `A-100` is `draft` (`s-1`); `A-101` is `submitted` (`s-2`) | Key field `order_id`; a schema |
| `example.orders.events` | `log` | Positions 0 to 2: `A-100` created, `A-101` created, `A-101` submitted (`s-3` to `s-5`) | Key field `order_id`; a schema |
| `example.orders.work` | `queue` | `A-100` pick (`s-6`), `A-101` pack (`s-7`) | Key field `order_id`; de-duplication header `idempotency-token`; no schema |
| `example.orders.notices` | `topic` | Nothing | No key field, no schema |

Every channel but the queue has one declared subscription, `example.runtime`, held by nobody. The orders are those of `example-app`. No token is required.

## The control interface

The control interface is the `TestBroker` handle, in the test's own process. It is not on the wire, so the code under test cannot reach it, and no fault ever applies to it. Everything it returns is plain data from `example_broker::model`.

| Call | Reports or does |
| --- | --- |
| `reset()` | Restores the seed data; clears every record, fault, and setting. Open connections stay; their subscriptions are gone |
| `truth()` | Everything below at one moment, as one `Truth` |
| `received()` | Every publish received, in order: connection, client name, channel, key, headers, body, a digest of the body, what became of it (`Stored`, `Duplicate`, `Refused`, `Dropped`), and whether the broker answered |
| `deliveries()` | Every delivery, in order: which message, to which subscription or consumer on which connection, the attempt, the cause (`First`, `Redelivery`, `Duplicate`, `Retained`), and the outcome (`Unacknowledged`, `Acknowledged`, `Released`, `Returned`, `NotRequired`) |
| `acknowledgements()` | Every acknowledgement accepted, in order |
| `requests()` | Every request received, in order: connection, client name, request, channel. This is how a test shows that a discovery never subscribed |
| `inspect(channel)` | Everything one channel holds: its declaration, the queue, the retained values, the log with its bounds, the subscriptions with what waits in each |
| `queue(channel)`, `retained(channel, key)`, `log(channel)` | The same, one part at a time |
| `set_fault(fault)`, `faults()`, `clear_faults()`, `applied_faults()` | Switch a fault on; list those on; switch all off; list every time one changed what the broker did |
| `declare_channel(spec)`, `remove_channel(channel)` | Add, replace, or remove a channel. Replacing drops what it held: this is how a channel changes shape or key |
| `declare_subscription(channel, name)` | Declare a durable subscription. The broker never creates one for a client |
| `set_dedupe_header(channel, header)` | Make the channel refuse a repeated token, or accept it |
| `expire_retention(channel)` | Drop what a retained channel or a log keeps. A log's positions carry on |
| `redeliver_unacknowledged(channel)` | Take back every unacknowledged delivery of a queue, as when a broker's time limit for an acknowledgement runs out |
| `set_required_token(token)` | Require a token in `hello`, change it, or require none. A connection that presented another is refused from its next request |
| `drop_connections()` | Drop every connection, as when the broker restarts. What it holds stays |
| `wait_for(limit, condition)` | Wait until the ground truth satisfies a condition. A test waits on this, never on a sleep |
| `address()`, `client(name)` | Where the broker listens; a new ordinary client |

`example_broker::model::body_digest` gives the digest a test expects: SHA-256 of the body's JSON with object keys in order and no spaces. A token is never part of what the control interface reports.

## Faults

Each fault is a setting: `Fault::once(kind)`, `Fault::times(kind, n)`, or `Fault::until_cleared(kind)`, narrowed with `.on_channel(address)` and `.on_op(op)`. The first fault that fits is the one used.

| `FaultKind` | Applies to | What happens | The failure it lets a test reproduce |
| --- | --- | --- | --- |
| `StoreThenDrop` | `publish` | The message is stored; the connection is dropped before the confirmation | A possible commit |
| `StoreThenSilent` | `publish` | The message is stored; no confirmation is sent; the connection stays open | A possible commit, seen as a time limit |
| `DropBeforeStore` | `publish` | The connection is dropped; nothing is stored | A lost connection that looks the same from outside and is not a commit |
| `Refuse { reason }` | `publish` | Answered `refused` with the reason; nothing is stored | A refused publish |
| `Delay { ms }` | Any request | The broker waits, then does the request correctly | A slow broker; on `fetch`, a slow delivery |
| `UnexpectedReply` | Any request | The request is not carried out; the answer has the request's id and no `ok` | An answer the driver has no rule for |
| `DuplicateDelivery` | `fetch` | The message handed over is handed over again on the next fetch, with a new delivery tag and the same `attempt`, even after it was acknowledged | Delivery at least once |
| `SilentConsumer` | A queue | A consumer of the stand's own, named `silent`, takes messages as they become ready, as many as the fault's `times`, and never acknowledges. `clear_faults()` makes it let go | Another consumer on the queue; a consumer that never acknowledges |
| `RenameField { from, to }` | `latest`, `browse`, `fetch` | A top-level field of every body returned is renamed. What is stored does not change | A changed message shape |
| `StoreThenRefuse { code }` | `publish` | The message is stored; the answer is a refusal under `code`, which need not be a code of the table above | A write in doubt behind an answer that looks like a refusal |
| `AckThenDrop` | `ack` | The message is acknowledged and gone; the connection is dropped before the answer | A take whose acknowledgement is in doubt |
| `AckThenRefuse { code }` | `ack` | The message is acknowledged and gone; the answer is a refusal under `code` | The same, behind an answer that looks like a refusal |
| `EmptyLook { more }` | `browse` | The answer holds no messages, whatever the channel holds, and says `more` as the fault states | A look that does not cover the channel, which must never be read as "nothing is there" |
| `ExtraDelivery` | `fetch` | One message more than `max` is handed over, when something waits | A broker that breaks the bound of a read |

Settings of the control interface reproduce the rest: a channel that is gone or has changed shape (`remove_channel`, `declare_channel`), retention that ran out (`expire_retention`), a repeated token accepted (`set_dedupe_header`), an acknowledgement that came too late (`redeliver_unacknowledged`), credentials that stopped working (`set_required_token`), and a lost connection on a read (`drop_connections`). A stale value, a second producer, and a second consumer need no fault: the test acts as one, with `client(name)`.

## Cases

Each case exists because of a risk named in section 309. Each is a test in `tests/cases.rs`.

| # | The broker does this | Risk it exercises |
| --- | --- | --- |
| 1 | Reports every publish it received, once, with what became of it | That a publish is proven by asking the stand, never by the pack's own read |
| 2 | Lists, describes, browses, and reads the latest value without a delivery | Discovery never consumes; `list` by browse; a topic that cannot be looked at |
| 3 | Hands each queue message to one consumer, and again until acknowledged | `take`; record, then acknowledge; never acknowledging what is another's |
| 4 | Copies a topic message to every open subscription and keeps nothing; lets one connection hold a declared subscription | A publish with no read-back; the runtime's own subscription; `subscription_in_use` |
| 5 | Keeps the last retained value per key; lets an unretained message pass through; hands retained values to a new subscription | `read` of a retained key; read-back by `latest`; a wait that matches an old value |
| 6 | Ends a read with nothing when nothing arrives, and at once when something does; refuses a wait over its limit | Every wait ends; `arrived: false` is an answer |
| 7 | Keeps a log in order at positions it reports; refuses a position that retention took | A wait from a recorded position; reconcile by scanning; an expired position |
| 8 | Refuses a repeated token, or accepts it when told to | A resumed publish that delivers one message; the duplicate when the broker does not help |
| 9 | Stores a message and loses the confirmation, two ways; drops a connection before storing | `possible_commit`; the same sight with nothing stored |
| 10 | Refuses a publish; answers slowly; refuses a body over the limit | `rejected`; time limits; `message_too_large` |
| 11 | Delivers one message twice, on a queue and on a topic | A duplicate is returned once |
| 12 | Lets another consumer hold a message unacknowledged; takes deliveries back on demand | Messages others take; an acknowledgement that came too late |
| 13 | Shows a changed field, a changed shape, a missing channel, and an answer outside the protocol | `drift`; `unknown_variant` |
| 14 | Requires a token, and stops accepting one it accepted | `needs_login`; no credential in any report |
| 15 | Drops every connection; restores the seed data on reset | `connection_lost`; a known starting state |
| 16 | Carries a publish or an acknowledgement out and then loses the answer or answers with an unknown code; answers a look with nothing; hands over more than a fetch asked for | A write in doubt that looks refused; a take whose message is gone; "absent" said of a log that was not covered; an unbounded read |

## What is not built

- **A standard protocol.** See "The wire protocol" above.
- **Partitions.** A log is one partition. Section 309 speaks of the partitions a key maps to; the stand adds them when a slice needs them.
- **The control interface on a second port.** It is in-process only, which is what a Rust test needs. A test in another language would need it on the wire.
- **A binary to run by hand.** The stand runs inside a test.
- **Checking a message against the registered schema.** The schema is a claim the broker reports and does not enforce, as section 309 treats it.
- **Transactions across channels, and encodings other than JSON values.** A text body is a JSON string.
