# `a2m-pack-stream`

The stream pack: the `stream` driver over queues, topics, and logs. Every read ends within its limit, a look takes nothing from anyone, a publish carries the record's token so that a resumed publish delivers one message, and discovery never consumes. The pack is designed in [spec section 309](../../spec/3-packs/309-the-stream-pack.md); this document says how its crate is put together. It was built as packages J2 and J3; what building it settled is in decisions [0104](../../decisions/0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md) and [0105](../../decisions/0105-the-stream-mode-and-the-stream-pack-as-building-it-settled-it.md).

## Facts

| | |
| --- | --- |
| Side | Pack: stream |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (the plan, steps, values, findings), `a2m-files` (file kinds, lowering, lint, and drafting registration), `a2m-auth` (`SessionLease`, `CredentialSink`), `a2m-provider` (`Pack`, `Driver`, `Discoverer`, `Describer`, `SandboxPort`), `a2m-telemetry` (the clock that stamps a finding). Outside: `tokio` and `serde`. No broker client crate |
| Built in | Packages J2, J3 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [309](../../spec/3-packs/309-the-stream-pack.md); the entry bar of section [202](../../spec/2-kernel/202-kernel-and-packs.md); section [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (no input changes a destination) |

## Owns

- The `channels/` file kind: one document per channel, with its shape, role, key, read paths, ownership, consumers, idempotency header, waits, and limits. It compiles to one section of the plan that only this crate decodes.
- The `schemas/` file kind: one document per message schema, in the part of JSON Schema the pack checks.
- The lowering of the verbs `read`, `list`, `await`, `publish`, and `take` into steps, with the pack's own details as each step's opaque payload.
- The driver of the mode `stream`: bounded looks and waits, a publish checked against its schema before it is sent, and the mapping of every failure to one class and code with whether the request left.
- The wire interface, `Protocol` and `Link`, and its first implementation, a client of the test broker's line protocol.
- The discoverer `broker_look`, its describer, and three drafting rules.

It must never: choose a channel from a step argument; keep a subscription open after its run; subscribe during a discovery; acknowledge on a queue the target does not declare the runtime owns; send a publish twice; renew a session; put what a broker said into a result or a finding before the redactor has seen it; create a subscription on a broker.

## What it registers

| Registration | Content |
| --- | --- |
| Mode | `stream`, archetype stream; rank 60; four reads and one write at a time in a session; a write holds the link |
| Surface kind | `broker`, with a session. Fields: `address`, one `host:port`, and `protocol`, one of the names in the crate's `PROTOCOLS` constant |
| Step kinds | `latest`, `browse`, `subscribe`, `await`, `publish`, `acknowledge`, `scan` from lowerings; `channels` and `describe` from the discoverer only |
| Driver | One, for `stream` |
| File kinds | `channel` in `channels/`, `message_schema` in `schemas/`, both for `broker` surfaces |
| Binding fields | `channel`, `key`, `headers`, `body`, `correlation`, `from`, `wait`, `on_nothing`, `read_back` |
| Verb lowerings | The table below |
| Finding kinds | `channel`, `message_schema`, `message_shape`; each `found` is one line of JSON |
| Settings, under `packs.stream` | `wait_ceiling_seconds` (60), `sample_count` (5), `idle_links` (2) |
| Auth providers | None. The neutral providers of `a2m-auth` hold the token; the driver places it in the request that opens a connection |

| Verb | Steps | Needs | Returns |
| --- | --- | --- | --- |
| `read` | `latest`, a lookup | A retained channel and its key | The body's fields and `message_meta`, or absent |
| `list` | `browse` | `read_through` lists `browse`; on a log, an optional `from` | `messages`, `count`, `more`, `coverage`, `next` |
| `await` | `subscribe`, `await` | `read_through` lists `own_subscription`; not a queue; a wait within the channel's longest and the settings' ceiling | `arrived` and `message`; or `arrived: false`, `waited_ms`, `examined`, `coverage` |
| `publish` | `publish`, which commits; read back by `latest` on a retained channel, by `scan` on a log that declares an idempotency header; none elsewhere | The effect and hazard the channel requires | `message_id`, `position`, `duplicate` |
| `take` | `subscribe`, `await`, then `acknowledge`, which commits | A queue with `ownership: runtime_owned`; `effect: irreversible`, no identity | The message; then `acknowledged` |

### Lint rules

The first seven are raised where a file is compiled or a binding lowered; the last two are registered rules that look across files.

| Code | Level | Rule |
| --- | --- | --- |
| `stream.channel_invalid` | Error | A channel file states a wait above its longest or above the settings' ceiling, a retained channel without a key, a read path its shape cannot have, or a name the broker could not carry |
| `stream.schema_unsupported` | Error | A message schema asks for more than the pack checks |
| `stream.unknown_channel` | Error | A binding names a channel no file defines, or one whose own file does not compile |
| `stream.binding_invalid` | Error | A binding lacks what its verb needs, states what its verb cannot use, or names the idempotency header itself |
| `stream.verb_needs_shape` | Error | `read` off a retained channel; `list` without a look; `await` on a queue or without the runtime's own subscription |
| `stream.wait_unbounded` | Error | A wait that is no length of time, or longer than the channel and the settings allow |
| `stream.ack_not_owned` | Error | A `take` on a channel that is not a queue the runtime owns |
| `stream.publish_effect` | Error | A publish or a `take` whose operation does not declare the effect, hazard, and identity the channel requires |
| `stream.unknown_schema` | Error | A channel names a schema no file under `schemas/` defines |
| `stream.consumers_unknown` | Warning | A channel that is published to does not say who reads it |

### Drafting rules

| From a finding about | The drafter writes | Left to the developer |
| --- | --- | --- |
| A channel the broker lists | A channel file: address, shape, the key field and the idempotency header where the broker states them, `read_through: browse` where a look worked | `role`, as an open question; `consumers`, `ownership`, every correlation field, every subscription name |
| The same finding | One operation that reads: `read` of a retained key the broker states, else `list` where a look worked | Every other operation |
| A schema the broker holds | A schema file, whole when the schema is within what the pack checks; else its fields as an open question | Confirming it |

No rule writes a publish, a `take`, or a subscription's name.

### Discoverers

| Discoverer | Reads | May send |
| --- | --- | --- |
| `broker_look` | The channels the broker lists, the schema it holds for each, and the names and JSON types of the fields in a bounded sample | `channels`, and `browse` of at most `sample_count` messages per channel. Nothing else |

**Discovery cannot write, and cannot consume.** A discoverer is given a `SandboxPort` and reaches the broker only through steps. The steps it builds are `channels` and `browse`; it has no code that builds a `subscribe`. The driver refuses `subscribe`, `publish`, and `acknowledge` in the discovery posture, whatever sent them. A sample returns field names and types from inside the driver, so no message value reaches a finding.

## Interface

```rust
pub fn register(builder: &mut RegistryBuilder);       // the one call the product crate makes
pub struct StreamPack;                                // impl Pack
pub const MANIFEST: &str;                             // the manifest's text
pub const PROTOCOLS: &[&str];                         // the wire protocols this build speaks
pub mod codes;                                        // every failure code, with ALL and CONTRACT_CODES
```

Inside the crate, the wire interface is the only code that knows a protocol:

```rust
pub(crate) trait Protocol: Send + Sync {
    fn name(&self) -> &'static str;
    fn connect(&self, address: &str, token: Option<&Secret>, deadline: Deadline) -> Wire<Box<dyn Link>>;
}
pub(crate) trait Link: Send {
    fn alive(&self) -> bool;                                               // from memory
    fn channels(&mut self, deadline) -> Wire<Vec<ChannelFacts>>;           // takes nothing
    fn describe(&mut self, address, deadline) -> Wire<ChannelFacts>;       // takes nothing
    fn latest(&mut self, address, key, deadline) -> Wire<Option<Message>>; // takes nothing
    fn browse(&mut self, address, from, max, deadline) -> Wire<Page>;      // takes nothing
    fn subscribe(&mut self, address, name, from, deadline) -> Wire<Opened>;
    fn fetch(&mut self, handle, max, wait, deadline) -> Wire<Vec<Delivered>>;  // bounded
    fn acknowledge(&mut self, handle, delivery, deadline) -> Wire<()>;
    fn release(&mut self, handle, delivery, deadline) -> Wire<()>;
    fn unsubscribe(&mut self, handle, deadline) -> Wire<()>;
    fn publish(&mut self, message: &Outgoing, deadline) -> Wire<Receipt>;  // headers carry the token
}
```

Every call ends by its deadline. Every `WireError` says whether the request was not sent, was answered, or has an unknown fate. A refusal of a repeated token is a `Receipt` marked `duplicate`, not an error.

What a step is given and gives:

- **The channel comes from the plan.** A step's `refers_to` names an entry of the `channel` section, which `prepare` decoded. Arguments fill typed positions only: `key`, `correlation`, `from`, `header.<name>`, `body.<name>`. A `from` that is not a whole number is refused unsent.
- **The token is the ledger's.** Where the channel declares an idempotency header and the step input carries a stamp, the stamp travels in that header. The compiler refuses a binding that names the header.
- **A message as values.** The body's fields, by name, and `message_meta` with the broker's identifier, the key, the position, and the headers. A whole number is an integer; any other number is an exact decimal read from its text; a number with an exponent is `drift`. A decimal is sent as text.
- **Everything a broker said passes the redactor** the pack was given when it registered, before it leaves `act`.

### How the mode fills the loop

| Part | What the driver does |
| --- | --- |
| Prepare | Checks the surface's address against the run's destinations; decodes the pack's two sections; places the session's token; takes an idle connection of the same session and address that is still alive, or opens one |
| Act | One step. On the first step that names a channel, compares its declared shape and key with what the broker reports, once per run |
| Confirm | No steps. The broker's confirmation is the answer to `publish`; a wait's match is checked against the schema in the `await` step |
| Read back | `latest` for the key, or `scan` for the token: at the position this run's publish was confirmed at, or, in another run, from the start of the log, a bounded number of looks |
| Release | Lets the run's subscriptions go, which gives back what it held unacknowledged, and keeps the connection for the session's next run. After a failure the connection is dropped |

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | One step, one request, except a wait, which fetches one message at a time until its limit. A failure whose fate is unknown gives the connection up |
| 2 | Read-back for every write | By the table of section 309. Where there is none the operation is `irreversible` with no identity, and the compiler says so |
| 3 | Each identity strategy | `natural` on a retained state channel. `stamped` on a log or a retained channel that declares an idempotency header. `composite` and `find` are refused. `none` elsewhere |
| 4 | A test stand | `targets/example-broker`, cases 1 to 15 |
| 5 | A discoverer | `broker_look` |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Network to declared surfaces, and a session on `broker` surfaces, both enforced by the driver's own client. Hazards come from operations; the compiler requires `sends_outside` wherever something outside may read |
| 8 | A record per new mode | [Decision 0105](../../decisions/0105-the-stream-mode-and-the-stream-pack-as-building-it-settled-it.md) |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| The product crate | `register`, only |
| `a2m-engine`, through the registry | The driver, through its handle |
| `a2m-files`, through the gathered set | The two file kinds, the lowering, the lint rules, the drafting rules |
| `a2m-actions`, through the registry | The discoverer and the describer |

## State

| State | Where |
| --- | --- |
| Idle connections, by address, protocol, and session | Memory, in the driver; at most `idle_links` for each; checked alive before reuse |
| A run's connection, subscriptions, the message it holds unacknowledged, the identifiers it was delivered, and the confirmations of its publishes | Memory, in the run's `Prepared`; gone at release |
| The session's token | Never here. It is placed into one opening request and dropped |
| A position to wait from across runs | Not here: it is an input of the operation, or the broker's own record in the runtime's named subscription |

## Failures

| Class | Codes | When | Sent |
| --- | --- | --- | --- |
| `input` | `schema_invalid`, `value_out_of_range`, `message_too_large` | The message fails its schema, a value has no form in the encoding, a position is no whole number, the body is over the channel's limit | Not sent |
| `input` | `protocol_unknown`, `payload_unreadable` | The surface names a protocol this build does not speak; the plan was compiled by another build of the pack | Not sent |
| `policy` | `surface_not_declared`, `ack_not_owned`, `read_only_posture` | An address outside the run's destinations; an acknowledgement on a queue not owned; a publish, a subscription, or an acknowledgement where nothing may be written | Not sent |
| `policy` | `scan_limit` | A log holds more than a scan may read, so the driver cannot say the message is absent | Answered |
| `session_expired` | `needs_login` | The broker refuses the credentials | Answered |
| `rejected` | `broker_refused` | The broker refuses the message, with its reason | Answered |
| `drift` | `channel_missing`, `shape_changed`, `response_schema_mismatch`, `position_out_of_range` | The channel or the runtime's subscription is gone; the shape or key differs; a message fails its schema; a position is no longer held | Answered |
| `unknown_variant` | `unexpected_reply` | An answer the protocol has no rule for | Unknown, or answered |
| `transient` | `upstream_unavailable`, `connection_lost`, `timeout`, `nothing_arrived` | No connection; a connection that ended; a deadline on a read; no match for a binding that needs one | Not sent, unknown, or answered |
| `possible_commit` | `no_confirmation` | A publish or an acknowledgement left and no answer came | Unknown |
| `busy` | `subscription_in_use` | Another run holds the runtime's subscription | Answered |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A registry with only this pack builds, and its digest is stable | `tests/compile.rs` |
| The manifest declares exactly the codes the driver returns | `tests/compile.rs` |
| The example target compiles twice to the same bytes; an unknown key or word is refused with a file, a line, and a fix | `tests/compile.rs` |
| A wait above the channel's longest or the settings' ceiling does not compile; `take` compiles only on a queue the runtime owns; a publish must declare what its channel makes of it | `tests/compile.rs` |
| Five reads of one retained key give one trace signature and one result, and the broker shows no delivery | `tests/driver.rs` |
| A look is bounded and says its coverage; a queue that was looked at holds what it held | `tests/driver.rs` |
| A wait ends with a match or with nothing, and no subscription outlives its run | `tests/driver.rs` |
| A message delivered twice is returned once | `tests/driver.rs` |
| A publish is exactly one message, by the broker's count, and reads back; a message that does not fit is never sent | `tests/driver.rs` |
| A lost confirmation is `possible_commit`, and the broker shows no second publish | `tests/driver.rs` |
| A resumed publish with the record's token delivers one message | `tests/driver.rs` |
| A message is acknowledged only with the record's intent token, and one that is not acknowledged goes back | `tests/driver.rs` |
| Drift, an unknown answer, a refused session, a lost connection, and an undeclared address are each named | `tests/driver.rs` |
| A secret in what the broker says reaches no result, failure, finding, or trace | `tests/driver.rs`, `tests/discover.rs` |
| A discovery asks only for looks, and every channel holds afterwards what it held | `tests/discover.rs` |
| A draft states what the broker stated; no draft holds a publish, a `take`, or a subscription name | `tests/discover.rs` |

## Build order

1. Done: the `channels/` and `schemas/` schemas, the lint rules, the `broker` surface kind, the manifest and registration.
2. Done: `read` of a retained key, with one trace signature across five runs.
3. Done: `list` by browse, and `await`, including the answer when nothing arrives.
4. Done: the discoverer, the describer, and the drafting rules, against the test stand.
5. Done: `publish` to a state channel as a `write` with read-back, with the lost-confirmation, duplicate, and changed-value tests.
6. Done: `publish` as `irreversible`, with a scan of a log for the record's token.
7. Done: `take` on a runtime-owned queue.

Not built:

- A standard wire protocol. Only `example-broker/1` exists ([decision 0104](../../decisions/0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md)).
- The `text` encoding, and any binary schema format. A channel file accepts `encoding: json` only.
- A certificate auth provider. The lease's token is carried; a client certificate is ignored.
- Captures. A step returns values and no stored evidence.
- Offline fixtures for the example target's operations. The crate's example target compiles as a draft.
- The trace details a driver could report (`means`, `measured`): the contract gives a step outcome no place for them.

Not proven:

- The session path with a real lease. Tests run with no lease; the token reaches the wire only through `connect`, which the stand's token case exercises from the refusing side.
- The engine loop over these steps. The tests call the handle directly.
- Windows and macOS. The crate has no code that differs by system, and its tree compiles C below it, so it was linted and run on Linux only.

## Open points

- Which standard protocol first, and whether a log needs a second ([decision 0104](../../decisions/0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md)).
- A position recorded at intent. The contract lets a step return values for reconcile and gives a later step no way to receive them, so a scan in another run starts at the start of the log and is bounded by `scan_limit`. See the kernel gaps in decision 0105.
- A wait fetches one message at a time, so that a message that does not match is never taken in a batch. On a busy channel this is one request per message; a protocol with a server-side filter would do better.
- A wait through the runtime's named subscription consumes the messages that do not match. They are the runtime's own, and they are gone.
- `key.type` in a channel file is checked as a field type and not used. A lint that compares it with the type of the binding's key input would use it.
- Whether waits need their own count in the scheduler's budgets (section 309). The mode declares four reads a session; a wait holds one for up to its limit.
- The shape check costs one `describe` per channel per run. It could be kept per connection.
