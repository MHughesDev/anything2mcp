# `a2m-api`

The REST face of the kernel, and the server's network listener. It serves `/api/v1`, generated from the action registry; describes it in an OpenAPI 3.1 document generated from the same registry; streams a run's events as server-sent events; delivers webhooks from the store's outbox; and, on a server, serves the approval and sign-in pages behind the identity provider's sign-in. The same router is carried on the local channel on one machine.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-actions`, `a2m-telemetry`. Outside: Tokio, Axum, Hyper with rustls, as spec section 605 lists them, to be verified; an HTTP client for webhooks, to be verified |
| Built in | Roadmap package 1.4 (the router on the local channel, the action route); 6.4 (the full interface, OpenAPI, events, webhooks); 6.5 (the approval and sign-in pages and the console on a server); 7.3 (the hosted sign-in page) |
| Carries out | Spec sections [222](../../spec/2-kernel/222-front-doors-and-the-local-engine.md) for REST, [601](../../spec/6-running-it/601-deployment-configurations.md), [503](../../spec/5-security/503-approval-and-sign-in-surfaces.md) on a server; the [REST endpoints](../../reference/technical-reference.md#rest-endpoints) of the technical reference |

## Owns

- The REST router under `/api/v1`, built from the registry when the role starts.
- The OpenAPI 3.1 document, generated from the registry and served at `/api/v1/openapi.json`.
- Server-sent events for a run, resumable from an event's sequence number.
- Webhook delivery: reading the outbox, signing, posting, retrying, and recording the outcome.
- On a server: the network listener, TLS, proxy headers, the health answer, protected resource metadata, and the mounting of the routes the product crate hands it, such as `a2m-mcp`'s endpoints.
- On a server: `ServerSurface`, its implementation of `PersonSurface`: the console's approval page, the sign-in status page, and later the hosted sign-in page.

It must never: hold a route that the registry did not generate; check a credential itself instead of through the kernel's verifier; serve an approval route on the local channel; send a webhook anywhere that settings and the egress rules do not name.

## Interface

```rust
/// Built from the registry. Pure: the caller decides which listener carries it.
pub fn router(kernel: Kernel, cfg: ApiSettings, carrier: Carrier) -> BoxedHttpService;
pub enum Carrier { Local, Server }     // Local leaves out the console and every approval page

/// The server's listener.
pub struct ServerListener { /* bind address, TLS, trusted proxies, base_url */ }
impl ServerListener {
    pub async fn bind(cfg: &ServerSettings) -> Result<ServerListener, Failure>;
    /// Serves the REST router and every extra mount, such as /mcp/<toolset>.
    pub async fn serve(self, api: BoxedHttpService, mounts: Vec<(PathPrefix, BoxedHttpService)>)
        -> StopReason;
}

pub fn openapi(registry: &Registry, toolset: Option<&Toolset>) -> OpenApiDocument;

pub struct WebhookDeliverer { /* reads the outbox through the Kernel; one task */ }
impl WebhookDeliverer { pub fn start(kernel: Kernel, cfg: WebhookSettings) -> Self; }

pub struct ServerSurface { /* console base address, approval-page signer */ }
impl PersonSurface for ServerSurface { /* see a2m-actions */ }
```

## Routes

Every route is generated from an `ActionDef`. There are two forms.

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
| `GET /api/v1/runs/{id}/events` | The event stream below; reading a run's events is authorized as `run.get` |

A request is turned into a `Request` with the caller, the toolset if the route names one, and the `Idempotency-Key` header as the idempotency key; it is accepted on every call whose effect is a write. The `Response` becomes JSON with a status code chosen from the exit class alone: 200 for success, 202 for a run that rests waiting for a person or is still running, 400 for malformed, 401 for no caller, 403 for `policy`, 409 for `stale`, 429 and 503 for `busy` with a retry time, 500 for anything else. The body is always the full `Response`, so a client never needs the status code to know what happened.

The `/api/v1/admin/...` row of the technical reference is not a separate surface: the admin actions are reached by their action routes. See the open points.

## OpenAPI

The document is generated from the registry when the router is built: one operation per route, request and response schemas from the action's schemas, the failure body as one shared schema, and the security schemes in use. It is also written into the recorded catalog, which the SDK generators read. A toolset can be exported as its own document, listing that toolset's target tools as operations, for platforms that consume OpenAPI instead of MCP (spec section 222).

## Events

`GET /api/v1/runs/{id}/events` is a server-sent event stream.

1. The caller is authorized as for `run.get` on that run.
2. The stream starts with every stored event after the `Last-Event-ID` the client sent, read from the store, then follows `Kernel::events` for new ones.
3. Each event's `id` is its sequence number in the ledger, so a reconnecting client misses nothing and sees nothing twice.
4. The stream ends with the run's final state. A comment line is sent at a fixed interval so idle proxies do not cut it.

Events pass the redactor before they are stored, so the stream never holds a secret.

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

On the local channel, with `Carrier::Local`, none of these pages and no approval route exist; the local surfaces are `a2m-local`'s.

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

The dispatcher's, rendered. This crate adds: `input` / `schema_invalid` for a body that is not JSON; `transient` / `upstream_unavailable` recorded against a webhook delivery that failed.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Every route is generated from an action | The parity test in `a2m-actions`, over the router's route list |
| The OpenAPI document equals the routes | A test that calls every operation the document lists |
| `Carrier::Local` has no approval page and no approval route | A test over the router built for the local channel |
| An event stream resumed from an identifier misses and repeats nothing | A test that cuts a stream mid-run |
| A webhook goes only to a configured destination | A test with an input naming another address |
| A caller cannot approve through the server's pages | The exit test of package 6.5 |

## Build order

1. The router with the action route only, carried on the local channel. Package 1.4.
2. The aliases of `tool.call`, `run.get`, `run.resume`, `run.cancel`, and `session.status`, as `rest_alias` data.
3. The OpenAPI document and its test. Package 6.4.
4. The event stream; the outbox and the deliverer. Package 6.4.
5. The server listener, token checks through the kernel, protected resource metadata. With package 6.2.
6. `ServerSurface` and the console's approval page. Package 6.5.

## Open points

- Actions to manage webhook destinations and toolset export: neither is in the action catalog yet.
- The `/api/v1/admin/...` row of the technical reference should be removed or defined as aliases.
- Where a remote caller fetches a kept blob: a generated alias of `trace.show`, most likely.
- The health answer for a load balancer, which has no caller: a reduced, generated alias of `engine.status` that shows only the state.
