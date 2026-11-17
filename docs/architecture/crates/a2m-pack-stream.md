# `a2m-pack-stream`

The stream pack: the `stream` driver over queues, topics, and logs. Every read ends within its limit, a look takes nothing from anyone, a publish carries the record's token so that a resumed publish delivers one message, and discovery never consumes. The pack is designed in [spec section 309](../../spec/3-packs/309-the-stream-pack.md); this document says how its crate is put together. It was built as packages J2 and J3; what building it settled is in decisions [0104](../../decisions/0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md) and [0105](../../decisions/0105-the-stream-mode-and-the-stream-pack-as-building-it-settled-it.md), what a safety audit of the code changed is in [decision 0150](../../decisions/0150-a-take-records-its-message-before-it-acknowledges-and-an-answer-with-no-rule-leaves-a-write-in-doubt.md), its first standard protocol, NATS with JetStream, is [decision 0167](../../decisions/0167-the-stream-pack-speaks-nats-with-jetstream-through-a-client-of-its-own.md), and what it took up of the kernel's second pass is [decision 0173](../../decisions/0173-the-stream-and-file-packs-take-up-the-kernels-second-pass.md).

## Facts

| | |
| --- | --- |
| Side | Pack: stream |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (the plan, steps, values, findings), `a2m-files` (file kinds, lowering, lint, and drafting registration), `a2m-auth` (`SessionLease`, `CredentialSink`), `a2m-provider` (`Pack`, `Driver`, `Discoverer`, `Describer`, `SandboxPort`), `a2m-telemetry` (the clock that stamps a finding). Outside: `tokio` and `serde`. No broker client crate: both wire clients are the pack's own |
| Built in | Packages J2, J3 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [309](../../spec/3-packs/309-the-stream-pack.md); the entry bar of section [202](../../spec/2-kernel/202-kernel-and-packs.md); section [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (no input changes a destination) |

## Owns

- The `channels/` file kind: one document per channel, with its shape, role, key, read paths, ownership, consumers, idempotency header, waits, and limits. It compiles to one section of the plan that only this crate decodes.
- The `schemas/` file kind: one document per message schema, in the part of JSON Schema the pack checks.
- The lowering of the verbs `read`, `list`, `await`, `publish`, and `take` into steps, with the pack's own details as each step's opaque payload.
- The driver of the mode `stream`: bounded looks and waits, a publish checked against its schema before it is sent, and the mapping of every failure to one class and code with whether the request left.
- The wire interface, `Protocol` and `Link`, and its two implementations: `nats` (`src/nats_wire.rs`), the NATS client protocol with JetStream, and `example-broker/1` (`src/example_wire.rs`), the test broker's line protocol.
- The discoverer `broker_look`, its describer, and three drafting rules.

It must never: choose a channel from a step argument; keep a subscription open after its run; subscribe during a discovery; acknowledge on a queue the target does not declare the runtime owns; acknowledge a message it has not first given out to be recorded; say a message is absent from a log it has not covered to the end; report a publish or an acknowledgement as refused when the broker's answer has no rule; send a publish twice; renew a session; open a connection without part of a session's credentials; put what a broker said into a result or a finding before the redactor has seen it, or cut it before the redactor has; create a subscription on a broker.

## What it registers

| Registration | Content |
| --- | --- |
| Mode | `stream`, archetype stream; rank 60; four reads and one write at a time in a session; a write holds the link |
| Surface kind | `broker`, with a session. Fields: `address`, one `host:port`, and `protocol`, one of the names in the crate's `PROTOCOLS` constant: `nats` or `example-broker/1` |
| Step kinds | `latest`, `browse`, `subscribe`, `await`, `publish`, `acknowledge`, `scan`, `position`, `taken` from lowerings; `channels` and `describe` from the discoverer only |
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
| `await` | `subscribe`, `await` | `read_through` lists `own_subscription`; not a queue; a wait within the channel's longest and the settings' ceiling | `arrived` and `message`; or `arrived: false`, `waited_ms`, `examined`, `coverage`, `limitation` |
| `publish` | `publish`, which commits; read back by `latest` on a retained channel, by `scan` on a log that declares an idempotency header, after a `position` step that records where the log ends; none elsewhere, and its evidence is then `acknowledged` | The effect and hazard the channel requires | `message_id`, `position`, `duplicate` |
| `take` | `subscribe`, `await`, then `acknowledge`, which commits; read back by `taken`, a look at the queue for the recorded message. Its evidence is `acknowledged` | A queue with `ownership: runtime_owned`; `effect: irreversible`, no identity | The message, which the `await` step also gives for reconcile so that it is recorded before the acknowledgement; then `acknowledged` |

### Lint rules

The first seven are raised where a file is compiled or a binding lowered; the last three are registered rules that look across files.

| Code | Level | Rule |
| --- | --- | --- |
| `stream.channel_invalid` | Error | A channel file states a wait above its longest or above the settings' ceiling, a retained channel without a key, a read path its shape cannot have, or a name the broker could not carry |
| `stream.schema_unsupported` | Error | A message schema asks for more than the pack checks |
| `stream.unknown_channel` | Error | A binding names a channel no file defines, or one whose own file does not compile |
| `stream.binding_invalid` | Error | A binding lacks what its verb needs, states what its verb cannot use, names the idempotency header itself, or gives the body a field named `message_meta` |
| `stream.verb_needs_shape` | Error | `read` off a retained channel; `list` without a look; `await` on a queue or without the runtime's own subscription |
| `stream.wait_unbounded` | Error | A wait that is no length of time, or longer than the channel and the settings allow |
| `stream.ack_not_owned` | Error | A `take` on a channel that is not a queue the runtime owns |
| `stream.publish_effect` | Error | A publish or a `take` whose operation does not declare the effect, hazard, and identity the channel requires |
| `stream.unknown_schema` | Error | A channel names a schema no file under `schemas/` defines |
| `stream.consumers_unknown` | Warning | A channel that is published to does not say who reads it |
| `stream.untrusted_unmarked` | Error | An output of an operation bound through the mode is or holds text, its field type is not marked `untrusted`, and the channel's schema does not hold the field of that name to an `enum` |
| `stream.protocol_unfit` | Error | On a surface that speaks `nats`: an address that is not `STREAM/subject` for a channel that keeps messages, or not a bare subject for a topic; a consumer name the server refuses; a named subscription on a topic; a take on a queue that names no consumer; a `broker_dedupe` whose header is not `Nats-Msg-Id` or that states no `window` |

**Text a broker returns is untrusted.** Every value of the mode is a broker's: a message another producer wrote, or the broker's own words about one. The last rule makes an output that holds such text carry a field type marked `untrusted`, as [section 502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) requires of free text from a system. A field the schema holds to an `enum` is not free text, because the driver refuses a message whose field holds anything else.

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
pub(crate) enum Credential { Token(Secret), UserPassword { user: String, password: Secret } }
pub(crate) trait Protocol: Send + Sync {
    fn name(&self) -> &'static str;
    fn carries(&self, credential: &Credential) -> bool;
    fn connect(&self, address: &str, credential: Option<&Credential>, deadline: Deadline) -> Wire<Box<dyn Link>>;
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

Every call ends by its deadline. Every `WireError` says whether the request was not sent, was answered, or has an unknown fate. An answer the protocol has no rule for has an unknown fate, and a refusal under a code the protocol does not have is such an answer. A look says whether the channel holds more than it returned; one that does not say is no answer. A refusal of a repeated token is a `Receipt` marked `duplicate`, not an error. A `WireError` carries the broker's words whole; the driver redacts them and only then cleans and cuts them for a message.

What a step is given and gives:

- **The channel comes from the plan.** A step's `refers_to` names an entry of the `channel` section, which `prepare` decoded. Arguments fill typed positions only: `key`, `correlation`, `from`, `header.<name>`, `body.<name>`. A `from` that is not a whole number is refused unsent.
- **Over `nats`, an address is `STREAM/subject` or a bare subject,** a key is the subject's last token, and a key, header, or address the protocol cannot carry is refused unsent (`value_out_of_range`): no input can move a message to another subject. Looks use the stream's own get; a wait on a log an ephemeral consumer with no acknowledgement, deleted at release; a take the consumer the channel names, acknowledged with a reply whose empty answer is the confirmation. Plain TCP goes to a loopback address only, and never to a server that asks for TLS (`transport_refused`).
- **The token is the ledger's.** Where the channel declares an idempotency header and the step input carries a stamp, the stamp travels in that header. The compiler refuses a binding that names the header.
- **A message as values.** The body's fields, by name, and `message_meta` with the broker's identifier, the key, the position, and of the headers only the one the channel declares for correlation. `message_meta` is always the broker's: a body that has a field of that name is `drift`, `response_schema_mismatch`, when read, and is never sent. A header another producer set is not shown. A whole number is an integer; any other number is an exact decimal read from its text; a number with an exponent is `drift`. A decimal is sent as text.
- **A write is refused only in words the protocol defines for it.** For a publish and an acknowledgement, a lost answer, an answer that cannot be read, and a refusal with no rule are all `possible_commit`, `no_confirmation`: nothing says the request was not carried out.
- **A take gives its message out before it acknowledges.** The `await` step of a take returns the redacted message under `message`, with `message_id`, as values for reconcile, which the engine stores durably before it sends the committing step. The stored form may be no larger than the channel's `limits.message_bytes`; a larger message is given back to the queue and the run stops with `take_too_large`, unacknowledged. The `acknowledge` step acknowledges only the message the `await` step gave out.
- **A scan says "absent" only of a log it covered.** A look that says more is held is followed from the place after its last message. A look that says more is held and gives no such place, or says that was all while the log's own end lies further on, is `unexpected_reply`.
- **A wait that saw nothing says what it covered.** `coverage: complete` where the subscription stands at a position the broker reported, as on a log. Elsewhere `coverage: partial` with `limitation: starts_at_subscription`: what was published before the wait subscribed was not seen. A fetch that hands over more than the one message asked for is `unexpected_reply`, and the connection is given up, which gives the messages back.
- **Everything a broker said passes the redactor** the pack was given when it registered, before it leaves `act`. A broker's words in a refusal or in an answer of no rule are the failure's `said`, redacted, then cleaned and cut; the sentence is the product's own.
- **What a write offers as its evidence** (`Lowering::evidence`): `acknowledged` for a `take` and for a `publish` with no read-back, to a queue, a topic, or a log that declares no idempotency header; `read_back` for every other publish. A keyed publish to a log is read back by the scan for its token, never by its key, which another producer may also use.
- **What earlier steps recorded** (`Prepared::recorded`). A `scan` starts at this run's own confirmed position; else at the `scan_from` its record's `position` step recorded before the publish; else at the log's start. The trace says which (`from`). A `taken` look reads the recorded `message_id`, and the message's recorded position where the protocol gives one: over `nats` the one place is read, elsewhere the whole queue, and "gone" is said only of a queue the look covered. Handed nothing of that form, it stops with `nothing_recorded`, unsent.
- **How each step was done** is its `acting` capture: `means` are the protocol, whether the channel's shape was compared in this step or earlier, where a scan began, how a take was looked for, and whether a token was sent; `measured` are counts of pages, messages, bytes, and channels.
- **No step uses `StepInput::key`.** A key travels as the `key` argument; a take's record has no key in the system and needs none.

### How the mode fills the loop

| Part | What the driver does |
| --- | --- |
| Prepare | Checks the surface's address against the run's destinations; decodes the pack's two sections; places the session's token, and stops with `credential_not_carried` when the session's provider places anything a protocol cannot present (a second header, a cookie, a client identity, a user and password); takes an idle connection of the same session and address that is still alive, or opens one |
| Act | One step. On the first step that names a channel, compares its declared shape and key with what the broker reports, once per run |
| Confirm | No steps. The broker's confirmation is the answer to `publish`; a wait's match is checked against the schema in the `await` step |
| Read back | `latest` for the key, or `scan` for the token: at the position this run's publish was confirmed at, or, in another run, from the start of the log, a bounded number of looks |
| Release | Lets the run's subscriptions go, which gives back what it held unacknowledged, and keeps the connection for the session's next run. After a failure of any step of the run the connection is dropped, whether or not the engine marks the run's context as spoiled |

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | One step, one request, except a wait, which fetches one message at a time until its limit. A failure whose fate is unknown gives the connection up |
| 2 | Read-back for every write | By the table of section 309. Where there is none the operation is `irreversible` with no identity, and the compiler says so |
| 3 | Each identity strategy | `natural` on a retained state channel. `stamped` on a log or a retained channel that declares an idempotency header. `composite` and `find` are refused. `none` elsewhere |
| 4 | A test stand | `targets/example-broker`, cases 1 to 16; `targets/example-nats` for the protocol `nats`, with a test against a real server when `A2M_TEST_NATS` names one |
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
| `policy` | `transport_refused` | Plain text to an address that is not this machine's own, or to a server that asks for TLS | Not sent |
| `input` | `protocol_unknown`, `credential_not_carried`, `payload_unreadable` | The surface names a protocol this build does not speak; the session holds a credential the protocol cannot present; the plan was compiled by another build of the pack | Not sent |
| `input` | `nothing_recorded` | A `taken` look or a `scan` was handed nothing recorded of the form it needs | Not sent |
| `policy` | `surface_not_declared`, `ack_not_owned`, `read_only_posture` | An address outside the run's destinations; an acknowledgement on a queue not owned; a publish, a subscription, or an acknowledgement where nothing may be written | Not sent |
| `policy` | `scan_limit` | A log or a queue holds more than a scan may read, so the driver cannot say whether the message is there | Answered |
| `policy` | `take_too_large` | A message taken is larger than may be recorded before it is acknowledged; it was given back | Answered |
| `session_expired` | `needs_login` | The broker refuses the credentials | Answered |
| `rejected` | `broker_refused` | The broker refuses the message, with its reason | Answered |
| `drift` | `channel_missing`, `shape_changed`, `response_schema_mismatch`, `position_out_of_range` | The channel or the runtime's subscription is gone; the shape or key differs; a message fails its schema; a position is no longer held | Answered |
| `unknown_variant` | `unexpected_reply` | An answer the protocol has no rule for, on a read; a look at a log that does not say how far it reached; a fetch that hands over more than was asked | Unknown, or answered |
| `transient` | `upstream_unavailable`, `connection_lost`, `timeout`, `nothing_arrived` | No connection; a connection that ended; a deadline on a read; no match for a binding that needs one | Not sent, unknown, or answered |
| `possible_commit` | `no_confirmation` | A publish or an acknowledgement left, and no answer came or the answer has no rule | Unknown |
| `busy` | `subscription_in_use` | Another run holds the runtime's subscription | Answered |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A registry with only this pack builds, and its digest is stable | `tests/compile.rs` |
| The manifest declares exactly the codes the driver returns | `tests/compile.rs` |
| The example target compiles twice to the same bytes; an unknown key or word is refused with a file, a line, and a fix | `tests/compile.rs` |
| A write never compiles as a read; a drafting rule that drafts a channel the runtime owns is refused | `tests/compile.rs`; `src/draft.rs` |
| A wait above the channel's longest or the settings' ceiling does not compile; `take` compiles only on a queue the runtime owns; a publish must declare what its channel makes of it | `tests/compile.rs` |
| Five reads of one retained key give one trace signature and one result, and the broker shows no delivery | `tests/driver.rs` |
| A look is bounded and says its coverage; a queue that was looked at holds what it held | `tests/driver.rs` |
| A wait ends with a match or with nothing, and no subscription outlives its run | `tests/driver.rs` |
| A message delivered twice is returned once | `tests/driver.rs` |
| A publish is exactly one message, by the broker's count, and reads back; a message that does not fit is never sent | `tests/driver.rs` |
| A lost confirmation is `possible_commit`, and the broker shows no second publish | `tests/driver.rs` |
| A resumed publish with the record's token delivers one message | `tests/driver.rs` |
| A message is acknowledged only with the record's intent token, and one that is not acknowledged goes back | `tests/driver.rs` |
| A take whose acknowledgement landed and whose answer was lost is `possible_commit`, and what was given for reconcile before the acknowledgement is the whole message the broker gave up | `tests/driver.rs`, `a_take_whose_acknowledgement_is_lost_has_its_message_in_the_record` |
| A message too large to record is given back and never acknowledged | `tests/driver.rs`, `a_message_too_large_to_record_is_given_back_and_never_acknowledged` |
| A scan never says "absent" of a log it did not cover | `tests/driver.rs`, `a_scan_that_did_not_cover_the_log_never_says_absent` |
| A publish or an acknowledgement refused under a code the protocol does not have is `possible_commit`; a refusal the protocol defines is still the broker's answer | `tests/driver.rs`, `a_refusal_the_protocol_does_not_define_leaves_a_write_in_doubt`; the unit test of `example_wire` |
| A body cannot take the name `message_meta`, read or sent | `tests/driver.rs`, `a_body_cannot_take_the_name_of_the_brokers_own_facts`; `tests/compile.rs` |
| A fetch that hands over more than was asked is a stop, and what it handed over goes back | `tests/driver.rs`, `a_fetch_that_hands_over_more_than_was_asked_is_a_stop` |
| A wait that saw nothing says what it did not cover | `tests/driver.rs`, `a_wait_that_saw_nothing_says_what_it_did_not_cover` |
| Only the header a channel declares for correlation reaches a caller | `tests/driver.rs`, `only_the_headers_a_channel_declares_reach_a_caller` |
| A connection is not kept after a failure; a credential the protocol cannot present is never dropped silently | `tests/driver.rs`, `a_connection_is_not_kept_after_a_failure`; the unit test of the driver |
| A secret that lies across the place where a broker's words are cut is redacted whole | `tests/driver.rs`, `a_refusal_is_the_brokers_own_and_a_topic_keeps_nothing` |
| Text a broker returns is an output only under a field type marked untrusted, or a field the schema holds to an `enum` | `tests/compile.rs`, `text_a_broker_returns_is_an_output_only_as_untrusted_text` |
| Drift, an unknown answer, a refused session, a lost connection, and an undeclared address are each named | `tests/driver.rs` |
| Over `nats`: five reads of a retained key over one connection give one result and the server shows no delivery, consumer, or publish | `tests/nats.rs`, `a_retained_key_reads_the_same_five_times_over_one_connection_and_takes_nothing` |
| Over `nats`: a look is bounded and a work queue looked at holds what it held | `tests/nats.rs`, `a_list_is_one_bounded_look_and_a_queue_looked_at_holds_what_it_held` |
| Over `nats`: a wait's own consumer is deleted when the run lets go; a topic is read through the connection's own subscription | `tests/nats.rs`, `a_wait_on_a_log_reads_through_a_cursor_the_server_discards_and_a_topic_through_its_own` |
| Over `nats`: a repeated token within the window stores nothing and names the first; past the window a scan still finds the first, and a resend would store twice | `tests/nats.rs`, `a_publish_carries_the_records_token_and_the_window_bounds_what_the_server_remembers` |
| Over `nats`: a stream whose window is shorter than the file's is drift, and nothing is sent | `tests/nats.rs`, `a_stream_that_remembers_a_token_for_less_than_the_file_says_is_drift` |
| Over `nats`: a lost, cut off, or unreadable acknowledgement of a publish is `possible_commit` and the token finds the one message stored; a refusal the protocol defines stores nothing | `tests/nats.rs`, `a_lost_or_unreadable_acknowledgement_is_a_write_in_doubt_and_a_refusal_is_the_servers` |
| Over `nats`: a key that would change the subject, and a message over the server's limit, are never sent | `tests/nats.rs`, `nothing_a_step_sends_can_move_a_message_to_another_subject_or_past_the_servers_limit` |
| Over `nats`: a take gives its message out, then acknowledges, and the server says it is gone; one let go is given back | `tests/nats.rs`, `a_take_records_the_message_then_acknowledges_and_the_server_answers_the_acknowledgement` |
| Over `nats`: an acknowledgement whose answer is lost is `possible_commit` with the whole message in the record; one nobody listens for acknowledged nothing | `tests/nats.rs`, `an_acknowledgement_whose_answer_is_lost_is_in_doubt_and_its_message_is_in_the_record`, `an_acknowledgement_nobody_listens_for_acknowledged_nothing` |
| Over `nats`: a pull that hands over more than asked, and a line the protocol does not have, stop; after a write was sent, in doubt | `tests/nats.rs` |
| Over `nats`: a refused session, a server that asks for TLS, and an address off this machine are each named, and nothing is sent to the last | `tests/nats.rs`; the unit test of `nats_wire` for a token and a user and password reaching `CONNECT` |
| Over `nats`: a discovery asks only for lists and reads, and makes no consumer | `tests/nats.rs`, `a_discovery_lists_streams_and_consumers_through_reads_only` |
| What the protocol cannot carry does not compile | `tests/nats.rs`, `a_channel_states_only_what_the_protocol_can_carry` |
| A secret in what the broker says reaches no result, failure, finding, or trace | `tests/driver.rs`, `tests/discover.rs` |
| The broker's words are the failure's `said`, and the sentence is the product's | `tests/driver.rs`, `a_refusal_is_the_brokers_own_and_a_topic_keeps_nothing` |
| Five reads give the same `means`, and the signature with them | `tests/driver.rs`, `a_retained_key_reads_the_same_five_times_and_takes_nothing`; `tests/nats.rs`, `a_retained_key_reads_the_same_five_times_over_one_connection_and_takes_nothing` |
| A take and a publish with no read-back offer `acknowledged`, and are `irreversible`; every other publish offers its read-back | `tests/compile.rs`, `the_example_target_compiles_twice_to_the_same_bytes` |
| A take in doubt is settled by a look at the queue for its recorded message: gone after its acknowledgement landed, absent when its run ended first, never "gone" of a queue not covered, and nothing sent without a recorded message | `tests/driver.rs`, `a_take_in_doubt_is_settled_by_a_look_at_the_queue_for_the_message_it_recorded`; `tests/nats.rs`, `a_take_in_doubt_is_settled_by_a_look_at_the_sequence_it_recorded` |
| A publish in doubt is found by a scan from where the log ended before it, in one look; past the duplicate window over `nats` too | `tests/driver.rs`, `a_publish_in_doubt_is_found_by_a_scan_from_where_the_log_ended_before_it`; `tests/nats.rs`, `a_publish_in_doubt_past_the_window_is_found_by_a_scan_from_the_recorded_position` |
| A channel file whose ownership is an open question describes a write, and is never drafted | `src/draft.rs`, `a_drafting_rule_that_drafts_a_channel_the_runtime_owns_or_may_own_is_refused` |
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
8. Done: what the kernel holds the pack to after the safety audit ([decision 0155](../../decisions/0155-four-packs-state-what-saves-what-writes-and-when-their-guard-lifts.md)). `Lowering::commits`: `publish` and `acknowledge` are `Always`; `subscribe`, `await`, `latest`, `browse`, `scan`, `channels`, and `describe` are `Never`. The manifest lists `commit_steps = ["publish", "acknowledge"]`. `Lowering::least_effect`: `take` is `irreversible`, `publish` is `write`, the rest `read`. `FileKind::writes`: a channel file describes a write when it states an `ownership` other than `shared`; a schema file never does. The driver's guard: in `Sandbox { test_records: true }` a step writes or takes only if it is marked `commits`, so a `take`'s `subscribe` is refused there (it is not marked), and a `take` runs only live. Test: `a_sandbox_with_test_records_lifts_the_guard_only_for_the_step_that_commits`.

9. Done: the protocol `nats`, NATS with JetStream, through a client of the pack's own, with the stand `targets/example-nats` ([decision 0167](../../decisions/0167-the-stream-pack-speaks-nats-with-jetstream-through-a-client-of-its-own.md)). The channel file gained `idempotency.window`, checked against the stream; `subscription` on a queue the runtime owns; `subscription` left out on a topic. The lease's credential is a token or a user and password, which `Protocol::carries` accepts or refuses.

10. Done: the kernel's second pass ([decision 0173](../../decisions/0173-the-stream-and-file-packs-take-up-the-kernels-second-pass.md)). Step inputs by `StepInput::new` and `for_record`; an `acting` capture on every step; `Prepared::recorded` read by `scan` and the new `taken`; a broker's words as `said`; `acknowledged` evidence for a take and a publish with no read-back; the new step kinds `position` and `taken` and the code `nothing_recorded`. Point 8 (expressions) needed nothing: no mapping key stands in for `current` or `record.key`. Point 7: the pack has no delete; a channel file's `ownership` that is an open question already counts as a write.

Not built:

- TLS for `nats`. Plain TCP to a loopback address only.
- The expected-sequence headers of a publish, which would make a publish to a state channel a compare-and-set. The refusal they cause is mapped.
- The direct get, batches of the stream's get, and key-value buckets. A look is one request a message.
- The `text` encoding, and any binary schema format. A channel file accepts `encoding: json` only.
- A certificate auth provider. A lease that places a client certificate, a cookie, a challenge scheme, or a second header stops the run with `credential_not_carried`; `example-broker/1` also refuses a user and password.
- Settling a take in doubt through the engine. The pack's `taken` step settles it, through the handle; the engine stops every record in doubt whose operation has no identity before it reads anything, so it does not run the step yet, and a person settles the record with the recorded message before them.
- Listing more headers for a caller. A channel file has no key for it; only the correlation header is given.
- Captures other than `acting`. A step returns values and no stored evidence.
- Offline fixtures for the example target's operations. The crate's example target compiles as a draft.

Not proven:

- Over `nats`, anything against a real server. The test `the_same_client_against_a_real_server` runs when `A2M_TEST_NATS` names one and was not run: no server could be had on the machine it was built on. The stand was written from the same documents and source as the client. Decision 0167 lists what only a real server can show.
- Redelivery after the acknowledgement wait on `nats`: the stand redelivers only when a test says so. A run that drops its connection holding a message leaves it pending on the server until the wait (30 s on the stand's consumer) runs out.

- The session path with a real lease. Tests run with no lease; the token reaches the wire only through `connect`, which the stand's token case exercises from the refusing side. Read, not run: `SessionLease::apply` registers every secret with the vault's redactor before the driver's sink is given it, so the token is known to the redactor before `connect`; the refusal of a credential that cannot be carried is tested on the sink alone.
- A take, or a publish with no read-back, through the engine. Their bindings offer `acknowledged`, and the record ends `acted` only through `Control::acknowledged`, which the ledger does not yet implement; what the engine stores before the commit point, and hands back through `Prepared::recorded`, was read in its code and played by the tests through `DriverHandle::act_recorded`.
- The engine loop over these steps. The tests call the handle directly.
- Windows and macOS. The crate has no code that differs by system, and its tree compiles C below it, so it was linted and run on Linux only.

## Open points

- Whether MQTT 5 comes second, for retained and topic channels ([research note 22](../../research/22-stream-wire-protocol.md)).
- A look over `nats` is one request a message, up to `browse_count`. The batch form of the direct get would be fewer, at the cost of a stream setting and of reads a follower may answer.
- A wait fetches one message at a time, so that a message that does not match is never taken in a batch. On a busy channel this is one request per message; a protocol with a server-side filter would do better.
- A wait through the runtime's named subscription consumes the messages that do not match. They are the runtime's own, and they are gone.
- `key.type` in a channel file is checked as a field type and not used. A lint that compares it with the type of the binding's key input would use it.
- Whether waits need their own count in the scheduler's budgets (section 309). The mode declares four reads a session; a wait holds one for up to its limit.
- The shape check costs one `describe` per channel per run. It could be kept per connection.
