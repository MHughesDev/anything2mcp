# The test NATS server

A small local server for the imaginary `example` target that speaks the part of the NATS client protocol and of the JetStream interface the stream pack's client uses. The pack is designed in [spec section 309](../../docs/spec/3-packs/309-the-stream-pack.md), and the protocol was chosen in [decision 0167](../../docs/decisions/0167-the-stream-pack-speaks-nats-with-jetstream-through-a-client-of-its-own.md). The rules every test stand follows are in the [test application plan](../../docs/plan/test-application.md).

It imitates what makes this kind of broker hard to drive, and nothing else: an acknowledgement of a publish that can be lost after the message was stored, a duplicate window that forgets a message id after a time, a work queue whose message is gone once acknowledged, an acknowledgement whose answer can be lost, a consumer that can vanish, and a look that must take nothing. It keeps everything in memory. Its clock moves only when a test moves it, so a duplicate window is crossed without waiting.

## Starting it

```rust
use example_nats::TestNats;

let stand = TestNats::start().await?;     // loopback, a free port, the seed data
let address = stand.address();            // give this to the code under test
// ... the code under test talks to `address` ...
let received = stand.received();          // ground truth
```

The stand stops, with every connection, when the `TestNats` is dropped. There is no binary and no fixed port.

## What it was written from, and what it cannot show

It was written from the protocol's public documentation and from the server's public source at commit `582fb8e939a77b6a33ef845955a208bdc222d767` of `nats-io/nats-server` (files `server/stream.go`, `server/consumer.go`, `server/jetstream_api.go`, `server/client.go`, `server/const.go`, `server/errors.json`, `server/jetstream_errors.go`). The pack's client was written from the same sources, so the two can share a misreading. The stream pack has one test, `the_same_client_against_a_real_server` in `crates/a2m-pack-stream/tests/nats.rs`, that runs the client against a real server when `A2M_TEST_NATS` names one. Decision 0167 lists what only that test can show.

| Behaviour the pack's safety rests on | What the source says | What the stand does |
| --- | --- | --- |
| The duplicate window | A message id is kept with the time of the message; an entry is purged when `now - stored >= window`; the default window is two minutes, at least 100 ms, never above the stream's maximum age; after a restart only ids of messages still stored within the window are rebuilt | The same rule on the stand's own clock. No purge timer and no restart |
| What a publish acknowledgement carries | `{"stream":..,"seq":..}`; for a duplicate, `"duplicate": true` and the sequence first stored, written by hand with a space before `true`; an error is an `error` object with `code`, `err_code`, and `description`, beside `stream` | The same text |
| An expected-sequence mismatch | Error 10071, "wrong last sequence", and nothing stored | A refusal under any code, as a fault; the expected-sequence headers themselves are not imitated |
| A pulled message's redelivery and acknowledgement | A pull of `batch` with `expires` delivers on the request's reply subject with a `$JS.ACK.<stream>.<consumer>.<count>.<stream seq>.<consumer seq>.<time>.<pending>` reply; it ends with status `408` when it expires and `404` with `no_wait`; an acknowledgement sent with a reply is answered with an empty message once carried out; on a work queue the acknowledged message is removed, pending or not; `-NAK` gives a pending message back; an unacknowledged message comes back after the acknowledgement wait | The same, except that redelivery after the wait happens only when a test calls `redeliver_unacknowledged`, and nothing comes back when a connection drops |
| Maximum payload | `INFO` states `max_payload` (1 MiB by default); a larger `PUB` is refused with `-ERR 'Maximum Payload Violation'` and the connection is closed; a stream's `max_msg_size` refuses with error 10054 | The same |
| Nobody listening | With `headers` and `no_responders` in `CONNECT`, a request to a subject nobody listens on is answered with status `503` | The same, for a publish to a subject no stream stores, an unknown JetStream request, and an acknowledgement for a consumer that is gone |
| The error codes the client maps | 10059 stream not found, 10014 consumer not found, 10037 no message found, 10071 and 10070 wrong last sequence or id, 10077 store failed, 10054 too large, 10099 a second consumer on a work queue | Each, where the case occurs |

## The seed

| Stream | Subjects | Kind | Holds |
| --- | --- | --- | --- |
| `ORDERS_STATUS` | `example.orders.status.*` | The last message of each subject | A-100 `submitted` at 1 |
| `ORDERS_EVENTS` | `example.orders.events.*` | Every message | Three events, at 1 to 3 |
| `ORDERS_WORK` | `example.orders.work.*` | A work queue, with the pull consumer `example_runtime` (explicit acknowledgement) | Two tasks, at 1 and 2 |

Every stream has the default window of two minutes. The topic `example.orders.notices` is a subject no stream stores.

## The control interface

| Call | Reports or does |
| --- | --- |
| `reset`, `truth`, `stream`, `messages` | Restore the seed; report every stream with what it holds, its consumers, and what each holds pending |
| `received` | Every publish, by which connection, with its headers, and whether it was stored, a duplicate, refused, or stored nowhere |
| `deliveries`, `acknowledgements` | Every message handed to a consumer, with its attempt; every acknowledgement, with its word and whether the delivery was pending |
| `api_calls`, `connects` | Every JetStream request by subject; every `CONNECT`, with how it signed in and whether it was let in, never the credential |
| `advance` | Moves the clock on, for the duplicate window |
| `declare_stream`, `remove_stream`, `declare_consumer`, `remove_consumer`, `redeliver_unacknowledged` | Change the server under the code being tested |
| `require_token`, `require_user`, `set_max_payload`, `set_tls_required` | Settings of the next connection |
| `publish` | Publish as another producer would |
| `set_fault`, `clear_faults` | The faults below, on until cleared |

| Fault | Does |
| --- | --- |
| `DropPublishAck` | Stores a publish and sends no acknowledgement |
| `CloseAfterStore` | Stores a publish and closes the connection |
| `UnknownCodeAfterStore` | Stores a publish and answers with an error code the protocol does not have |
| `RefusePublish { err_code }` | Refuses a publish under the code; stores nothing |
| `DropAckReply` | Carries out an acknowledgement and sends no answer |
| `UnknownAckReply` | Carries out an acknowledgement and answers with a payload that has no rule |
| `UnknownLine` | Answers each request and ping with a line the protocol does not have |
| `DeliverExtra` | Hands over one message more than a pull asked for |

## What it leaves out

TLS, clusters and their leaders, mirrors and sources, push consumers, flow control and heartbeats, the direct get, key-value and object stores, subject mapping, accounts and permissions, and a purge timer. A server that has any of these may behave in ways the stand does not.
