# `a2m-api`

The REST face of the kernel, and the server's network listener. It serves `/api/v1`, generated from the action registry; describes it in an OpenAPI 3.1 document generated from the same registry; streams a run's events as server-sent events; delivers webhooks from the store's outbox; and, on a server, serves the approval and sign-in pages behind the identity provider's sign-in. The same router is carried on the local channel on one machine.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-actions`, `a2m-logs`. Outside: Tokio, Axum, Hyper with rustls, as spec section 605 lists them, to be verified; an HTTP client for webhooks, to be verified |
| Roadmap packages | Roadmap package 1.4 (the router on the local channel, the action route); 6.4 (the full interface, OpenAPI, events, webhooks); 6.5 (the approval and sign-in pages and the console on a server); 7.3 (the hosted sign-in page) |
| Carries out | Spec sections [222](../../spec/2-kernel/222-front-doors-and-the-local-engine.md) for REST, [601](../../spec/6-running-it/601-deployment-configurations.md), [503](../../spec/5-security/503-approval-and-sign-in-surfaces.md) on a server; the [REST endpoints](../../reference/technical-reference.md#rest-endpoints) of the technical reference |

## Owns

- The REST router under `/api/v1`, built from the registry when the role starts, and the two routes that no action generates: the OpenAPI document and a run's events.
- The OpenAPI 3.1 document, generated from the registry and served at `/api/v1/openapi.json`.
- A run's events: a page of them from a sequence number, and the same as server-sent events, resumable from an event's sequence number.
- Webhook delivery: reading the outbox, signing, posting, retrying, and recording the outcome.
- On a server: the network listener, TLS, proxy headers, the health answer, protected resource metadata, and the mounting of the routes the product crate hands it, such as `a2m-mcp`'s endpoints.
- On a server: `ServerSurface`, its implementation of `PersonSurface`: the console's approval page, the sign-in status page, and later the hosted sign-in page.

It must never: hold a route that the registry did not generate, beyond the two named above; check a credential itself instead of through the kernel's verifier; serve an approval route on the local channel; send a webhook anywhere that settings and the egress rules do not name.

## Interface

```rust
/// Built from the registry. Pure: the caller decides which listener carries it,
/// and puts the verified `Caller` on each request as an extension (decision 0116),
/// and beside it the `ProjectFolder` the client end stated, when it stated one.
pub fn router(kernel: Kernel, settings: ApiSettings) -> Result<axum::Router, Failure>;
// Re-exported from the action layer: `ProjectFolder`, the one type every door reads.
pub struct ApiSettings { pub max_body_bytes: usize, pub body_timeout: Duration, pub request_timeout: Duration }
pub fn status_of(response: &Response) -> StatusCode;      // the one status table
pub fn openapi(registry: &ActionRegistry) -> serde_json::Value;
pub fn served_routes(registry: &ActionRegistry) -> Vec<ServedRoute>;  // what the router serves
pub const OPENAPI_PATH: &str = "/api/v1/openapi.json";
pub const RUN_EVENTS_PATH: &str = "/api/v1/runs/{run_id}/events";   // not an action
pub const KEEP_ALIVE: Duration;                           // 15 s: a silent stream sends a comment
pub mod codes;  // caller_missing, body_too_large, content_type_unsupported, query_invalid, ...

// Not built yet (sketch):
pub enum Carrier { Local, Server }     // comes back with the server's pages
pub struct ServerListener { /* bind address, TLS, trusted proxies, base_url */ }
impl ServerListener {
    pub async fn bind(cfg: &ServerSettings) -> Result<ServerListener, Failure>;
    /// Serves the REST router and every extra mount, such as /mcp/<toolset>.
    pub async fn serve(self, api: axum::Router, mounts: Vec<(PathPrefix, axum::Router)>)
        -> StopReason;
}
pub struct WebhookDeliverer { /* reads the outbox through the Kernel; one task */ }
impl WebhookDeliverer { pub fn start(kernel: Kernel, cfg: WebhookSettings) -> Self; }
pub struct ServerSurface { /* console base address, approval-page signer */ }
impl PersonSurface for ServerSurface { /* see a2m-actions */ }
```

## Routes

Every route is generated from an `ActionDef`, but two that dispatch no action: the OpenAPI document, and a run's events (see "Events"). There are two forms of a generated route.

| Form | Example | From |
| --- | --- | --- |
| The action route | `POST /api/v1/actions/target.probe` | Every action, by name |
| A resource-shaped alias | `POST /api/v1/operations/{name}/calls` for `tool.call` | The action's `rest_alias`: method, path, and which input field each path segment fills |

The aliases in the technical reference map onto actions as follows. Each is the action's own route under another path, and calls the same dispatch.

| Route | Action |
| --- | --- |
| `GET /api/v1/tools` | `tool.search` with no words |
| `POST /api/v1/operations/{name}/calls`, `POST /api/v1/pipelines/{name}/runs` | `tool.call` |
| `GET /api/v1/runs/{id}` | `run.get` |
| `POST /api/v1/runs/{id}/resume`, `/cancel` | `run.resume`, `run.cancel` |
| `GET /api/v1/sessions` | `session.status` |
| `GET /api/v1/openapi.json` | Generated from the registry; served by this crate, not an action |
| `GET /api/v1/runs/{run_id}/events` | A run's events, below. Not an action: served by this crate; the kernel shows a caller only a run the same caller started |

A request is turned into a `Request` with the caller, the toolset if the route names one, the project folder the mount stated, through `Request::in_project_folder`, and the `Idempotency-Key` header as the idempotency key; it is accepted on every call whose effect is a write. The `Response` becomes JSON with a status code chosen by `status_of` from the exit class and the failure class: 200 for success, 202 for a run that rests waiting for a person, 400 for malformed, 401 for no caller, 403 for `policy`, which a kill switch is (`kill_switch_on`), 409 for `stale`, 503 for `busy`, 500 for anything else, and the door's own codes for 404, 405, 408, 413, and 415 ([decision 0116](../../decisions/0116-the-rest-router-takes-its-caller-from-the-mount-and-chooses-its-status-in-one-table.md)). A still-running run is 200.

What a caller that sends too much, or too slowly, sees:

| Request | Answer |
| --- | --- |
| A body over the limit (1 MiB by default), up to four times the limit | 413 `body_too_large`, always: the door reads and discards the rest (for at most 5 s) before it answers |
| Any other refusal before the body is read (no caller, a bad header, a query on a POST, no such route), with a body up to four times the limit | That refusal, always, after the same draining |
| A body declared or found longer than four times the limit | 413 `body_too_large`, or the connection closed while the client is still writing; nothing is dispatched in either case |
| A body that stops arriving | 408 `body_timed_out` when `body_timeout` (30 s) is up; a client still writing then may see the connection closed instead | A `Retry-After` time for `busy` is not sent yet: the response carries none. The body is always the full `Response`, so a client never needs the status code to know what happened.

The `/api/v1/admin/...` row of the technical reference is not a separate surface: the admin actions are reached by their action routes. See the open points.

## OpenAPI

The document is generated from the registry when the router is built: one operation per route, request and response schemas from the action's schemas, the failure body as one shared schema, and the security schemes in use. The route of a run's events is described in it with both of its answers, a page as JSON and a stream, and carries no `x-action`. Every operation of an action carries `x-action`, its name, and `x-sdk`, whether it has an SDK method: an SDK face (`faces.sdk`) and a handler (`ActionDef::built`), as `RestRouteFace::sdk` and `has_sdk_method` in `a2m-actions` say; the clients' generators write a method only where `x-sdk` is true, so an action that answers `action_not_built` has no method until it is built ([decision 0420](../../decisions/0420-the-api-description-says-which-actions-have-a-client-method.md), refined by [decision 0437](../../decisions/0437-what-a-release-publishes-is-kept-and-every-later-build-is-checked-against-it.md), point 5). `tools/contracts.py check` holds the saved description's `x-sdk` to the actions `handler_for` gives a function or the client carries out, the same list it keeps a release's built actions by. It is also written into the action catalog, which the SDK generators read. A toolset can be exported as its own document, listing that toolset's target tools as operations, for platforms that consume OpenAPI instead of MCP (spec section 222).

## Events

`GET /api/v1/runs/{run_id}/events` gives the events of a run ([decision 0151](../../decisions/0151-the-doors-take-up-the-action-registrys-second-pass.md)).

**Where the route lives, and why.** It is fixed in the router beside the route of the OpenAPI document. `served_routes` lists both, last, with an empty action, and the document describes both. No action generates it, because reading a run's events is not an action: `Kernel::events` and `Kernel::next_events` record no decision and show a caller only a run that the same caller started, which the decision about the call that started the run covered. An action for it would record a decision for every page of a stream. The route takes no body and dispatches nothing, so nothing a request carries chooses what runs.

| A request that | Gets |
| --- | --- |
| Accepts `text/event-stream` | A stream of server-sent events |
| Anything else | One page as JSON, in the shape of every answer: `ok`, an empty `action`, and `result` with `run_id`, `events`, `last`, `status`, `working`, `ended`, `settled` |

1. The caller is the one the mount put on the request. The kernel refuses a run the caller did not start as `input`, `not_found`, the same answer as for no run at all. A path that holds no run's identifier is `input`, `schema_invalid`.
2. A page starts after `after` (0 when not given) and holds at most `limit` events, 200 at most. Any other field of the query is refused as `query_invalid`.
3. A stream starts after the `Last-Event-ID` the client sent, or else after `after`. Each event's `id` is its sequence number in the ledger, and its data is the event: `seq`, `lease`, `at` in milliseconds, `kind`, `detail`. A reconnecting client misses nothing and sees nothing twice.
4. A stream ends with one event named `settled`, which says where the run stands, once the run has ended or rests with every event sent. A run that waits for a person may wait for days, so the stream does not stay open across a rest: a client opens it again, with the last identifier, after the run is resumed.
5. The first page is read before anything is answered. A refusal is therefore an ordinary answer with its status, and never the start of a stream. A failure after the stream began is its last event, named `failed`.
6. A comment line is sent when a stream has said nothing for 15 seconds, so that an idle connection is not taken for dead.
7. A run whose state rests while its job is still in a runner's hands settles without a further event. Each wait is `Kernel::next_events_from` with what the last page showed, so the kernel answers the moment the job is recorded as ended, with no timer ([decision 0159](../../decisions/0159-a-lost-answer-is-a-write-in-doubt-at-the-doors-and-in-the-clients.md)).
8. Each line waits at most 60 s for the reader to take it (`READER_PATIENCE`). A reader that keeps the connection open and reads nothing for that long is treated as gone, and the stream ends. Following only reads, so ending it leaves nothing half done.

Events pass the redactor before they are stored, and their detail passes it again as the kernel hands it out, so the stream never holds a secret.

**No redaction pass of its own.** Unlike the MCP door, this crate does not walk what it sends through the redactor. Everything it sends is the dispatcher's response or the kernel's events, both redacted before they are handed over, and this door's own texts quote nothing a caller sent except the HTTP method and, for a JSON error, a line and a column. The MCP door needs its pass because its protocol errors echo a tool's name, a request's id, and a progress token. A route of this crate that comes to quote its input must add the pass.

## A watch's changes

`GET /api/v1/watches/{watch_id}/changes` gives the changes a watch recorded ([decision 0478](../../decisions/0478-a-watch-is-an-mcp-resource-of-watch-changes-and-its-changes-are-followed-without-a-decision-for-each-wait.md), as [decision 0234](../../decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md) planned it). It is built as the route of a run's events is: fixed in the router, listed by `served_routes` with an empty action before the document's own, described in the document with its page taken from `watch.changes`'s own output, and no action's. Following a watch records no decision for each wait: `Kernel::watch_changes` shows a caller only a watch of the person it acts for, which is what every `watch.*` action shows it.

1. A request that accepts `text/event-stream` gets a stream; any other gets one page as JSON, `result` in the shape `watch.changes` returns: `watch_id`, `state`, `changes`, `after`, `complete`.
2. `after`, `limit` (1 to 100), and `Last-Event-ID` are read as for a run's events, by the same function. A watch of another person, or none, is `input`/`not_found`; a path that holds no watch's identifier is `schema_invalid`.
3. Each change is one event, its `seq` as its `id` and its data the change as `watch.changes` shows it, holding no value. A full page is read on at once; otherwise each wait is the kernel's, on its notice that something changed.
4. A watch is meant to be followed for its whole life, so the stream stays open while the watch lasts, unlike a run's, which ends at a rest. Once the watch has ended and every change is sent, the stream ends with one event named `ended`. A failure after the stream began is its last event, `failed`.
5. A comment line after 15 s of silence, and `READER_PATIENCE` for each line, as for a run's events.

## Webhooks

A webhook is delivered from the outbox, a table in the store written in the same transaction as the event it reports, so that no event is reported that did not happen and none that happened is lost.

| Rule | Detail |
| --- | --- |
| At least once | A delivery is retried with growing delays until it succeeds or a limit is reached; the outcome is recorded |
| Signed | A signature over the body and a timestamp, with a key from the vault |
| Deduplicable | Each delivery carries the event's identifier |
| Only where configured | The destination comes from settings, is checked against the egress allowlist, and never from an input |
| Nothing phones home | With no destination configured, nothing is sent |

## The pages on a server

| Page | How it works | Phase |
| --- | --- | --- |
| Approval | The console's page, after the approver signs in to the organization's identity provider. It reads the stored diff, shows the caller and for whom, and records the answer through `ApprovalDesk` with the sign-in as the proof of a person | 6 |
| Sign-in status | Shows that the keeper is waiting for a sign-in, for which application and identity | 6 |
| Hosted sign-in | Shows the server runner's browser to the person; design waits for research brief 17 | 7 |
| A plain link | `give_link`: the link a sign-in provider outside the web pack needs the person to open, shown in the console and returned to the caller; nothing drives the person's browser | 6 |

The pages are the TypeScript of `web/console`, built and handed to this crate by the product crate. Every rule for every surface in spec section 503 applies: equally easy approve and decline, an expiry with the approval window, restricted fields shown only to approvers allowed to see them, a timeout as a decline. An agent's identity can never hold the approver role, so its sign-in is never a proof of a person.

On the local channel none of these pages and no approval route exist; the local surfaces are `a2m-local`'s. Today no router serves any of them: `Carrier` returns with step 6 of the build order.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `anything2mcp` | `router` on the local channel and on the server; `ServerListener`; `WebhookDeliverer`; `ServerSurface` handed to `Kernel::open` |

It calls `a2m-actions` for the registry, `dispatch`, `verify_caller`, `events`, `approvals`, and health.

## State

| State | Where |
| --- | --- |
| The outbox | A table in the store, written by `a2m-ledger` in the same transaction as the event |
| Each delivery's outcome | A table owned by `a2m-actions`, written through the kernel; its migration is in `a2m-ledger` |
| Open event streams | Memory; a dropped stream resumes by `Last-Event-ID` |
| Console sign-in for approvers | A short-lived, signed cookie for the console only; never a target's session |

## Failures

The dispatcher's, rendered. This crate adds, before the dispatcher is reached: `input` / `schema_invalid` for a body that is not one JSON object; `input` / `body_too_large`, `content_type_unsupported`, `query_invalid`, `path_field_conflict`, `idempotency_key_invalid`, `route_not_found`, `method_not_allowed`, `body_timed_out`; `policy` / `caller_missing`. In place of an answer: `busy` / `request_timed_out`, `transient` / `engine_fault`. Later: `transient` / `upstream_unavailable` recorded against a webhook delivery that failed.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Every route is generated from an action | The router binds only `rest_routes`; `served_routes` is its list, and the OpenAPI test holds the document to it |
| The OpenAPI document equals the routes | `the_openapi_document_describes_every_route_and_nothing_else`: equal both ways, and every operation called |
| The router has no approval page and no approval route | `the_door_offers_no_way_to_approve`: over every served route, and a resting write that a resume through the door does not write |
| An event stream resumed from an identifier misses and repeats nothing | `a_runs_events_are_read_from_a_number_and_streamed_until_the_run_settles`: a stream to the rest of a write, the approval, and a second stream from the last identifier to the end |
| A run's events are shown only to its caller | The kernel's check, proven in `a2m-actions` with a second principal. Here: a run's identifier the caller never got, in both forms of the route, and no caller at all. This build has one user, so a second principal cannot be made outside the kernel |
| A kill switch is answered 403 | `a_kill_switch_is_the_policy_engines_refusal_and_status_403`, and the status table's own test |
| A webhook goes only to a configured destination | A test with an input naming another address |
| A caller cannot approve through the server's pages | The exit test of package 6.5 |

## Build order

1. Done: the router with the action route, carried on whatever the product crate mounts it on; the caller comes as a request extension; limits on body size and time. Package 1.4.
2. Done, partly: the aliases the registry declares as `rest_alias` data: `tool.search`, `run.get`, `run.resume`, `run.cancel`, `session.status`. Not built: `POST /operations/{name}/calls` and `/pipelines/{name}/runs` for `tool.call`, because the catalog gives `tool.call` no alias and one action has at most one; `a2m-actions` must give it one (or two) with `{name}` filling `tool`.
3. Done: the OpenAPI document and its test (every served route described, nothing else, every operation called). Not built: a toolset's own document. Package 6.4.
4. Done: a run's events, as a page and as a stream, over `Kernel::events` and `Kernel::next_events`; a watch's changes, the same way, over `Kernel::watch_changes` ([decision 0478](../../decisions/0478-a-watch-is-an-mcp-resource-of-watch-changes-and-its-changes-are-followed-without-a-decision-for-each-wait.md); `a_watchs_changes_are_refused_for_a_watch_that_is_not_the_callers` and the product's watch test). Not built: the outbox and the deliverer; the store has no outbox table and settings no destinations. Package 6.4.
5. Not built: the server listener, its token checks, protected resource metadata; `verify_caller` refuses bearer tokens and API keys in this build. With package 6.2.
6. Not built: `ServerSurface` and the console's approval page; needs a person's sign-in on a server and the console's pages. Package 6.5.

## Open points

- Actions to manage webhook destinations and toolset export: neither is in the action catalog yet.
- The `/api/v1/admin/...` row of the technical reference should be removed or defined as aliases.
- Where a remote caller fetches a kept blob: a generated alias of `trace.show`, most likely.
- `engine_fault` and `request_timed_out` give the exact next step (decision 0160): with an `Idempotency-Key`, the same request again with the same key, which gives back the run the first one started; without one, `GET /api/v1/runs?since=<a minute before>&tool=<tool>`, by this machine's clock. `GET /api/v1/runs` is `run.list`, an alias its definition declares.
- A second principal for this crate's tests, so that the refusal of another caller's run is proven through the door and not only in the kernel.
- The health answer for a load balancer, which has no caller: a reduced, generated alias of `engine.status` that shows only the state.
