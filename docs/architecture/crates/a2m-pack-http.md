# `a2m-pack-http`

The HTTP pack: the `http` and `api` drivers, the endpoint catalog as a file kind, and the import of published descriptions (OpenAPI, GraphQL schemas, OData metadata) as discoverers and drafting rules. Requests are built from typed templates, sent from code with no browser, and checked against their schemas.

## Facts

| | |
| --- | --- |
| Side | Pack: HTTP |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-expr` (template values), `a2m-files` (file-kind, lowering, lint, and drafting registration), `a2m-auth` (`SessionLease`, `inject`), `a2m-provider` (`Driver`, `Discoverer`, `Describer`, `SandboxPort`, `PackContext`, registration), `a2m-telemetry`. Outside: `tokio`, `serde_json`, an HTTP client (`reqwest` is verified for the test harness; its TLS choice is still open), a JSON Schema validator and parsers for the description formats, outside crates to be verified |
| Built in | Package 4.1 (`http`, reads then writes), package 4.4 (descriptions and the `api` mode), with the drafting rules in E4. Package 2.6 touches it for sign-in signals on HTTP surfaces |
| Carries out | Spec sections [6](../../spec/06-modes-and-drivers.md) (`http`, `api`), [14](../../spec/14-edge-cases-and-large-applications.md) (the request rows), [46](../../spec/46-target-compiler-and-plan.md) (typed templates), [51](../../spec/51-defending-against-a-manipulated-agent.md) (no input changes a destination), [54](../../spec/54-interaction-archetypes.md) (request), [60](../../spec/60-overlaying-tools-on-an-existing-system.md) and [67](../../spec/67-the-scout-and-who-does-what.md) (drafting from descriptions and from captured endpoints) |

## Owns

- The `endpoints/` file kind: its schema, its lint rules, and its compilation into this crate's own request templates, an opaque step payload. Another pack that reads `endpoints/` compiles the entries it is handed into its own payload.
- The `api` binding: an operation in an imported description, compiled into the same templates.
- The `endpoint` finding kind and the schema of its `found`, which `a2m-pack-web-probe` produces.
- Sending a request: building it position by position from the template, injecting credentials through the lease, sending it on a warm connection, checking status and schema, extracting values.
- Carrying out an authentication scheme on the connection, such as the digest exchange or integrated Windows authentication. The auth provider in `a2m-auth` holds and injects the credential; the scheme's round trips are this driver's work.
- Recognizing, in a response, an edge block, a validation refusal, a version conflict, and the signals of expiry the target declared. It reports them; it acts on none.
- Reading OpenAPI, GraphQL, and OData descriptions into findings, and the drafting rules that turn those findings, and endpoint findings from capture, into drafts.

It must never: hold a credential outside one request; put a secret into a template, a finding, or a trace; send to an origin the plan did not resolve; follow a redirect to another origin; retry; renew a token, even when the response says it expired; draft an operation for a method its description does not mark safe.

## What it registers

| Registration | Content |
| --- | --- |
| Modes | `http` and `api`, archetype request |
| Surface kinds | `http` |
| Step kinds | `token`, `request`, `validate`, `extract` for `http`; `call` for `api`; checked names, registered per mode |
| Drivers | `HttpDriver`, `ApiDriver` |
| File kinds | Owns `endpoints/` ([file formats](../../reference/file-formats.md#endpointsyaml)), for `http` surfaces and, for requests made from inside a page, `web` surfaces; `a2m-pack-web-ui` declares that it reads it, and is handed the validated entries as data. Owns a pinned copy of an imported description, with its digest, which the `api` binding names |
| Binding fields | `http`: `endpoint`, `path_params`, `query`, `body`. `api`: `operation_id`, `args` |
| Verb lowerings | `http`: each verb of the [verbs by mode](../../reference/technical-reference.md#verbs-by-mode) table into `token`, `request`, `validate`, `extract`. `api`: into `call`. Each write request is marked as a commit point; each declared lookup as a lookup |
| Finding kinds | `endpoint`, with the schema of its `found`, which `a2m-pack-web-probe` produces and this crate's drafting rule reads; `described_operation` from descriptions |
| Lint rules, drafting rules, discoverers | Below |
| Describer | `http_describer`: endpoints as method, template, and shapes |
| Auth providers | None. Every provider for this pack's surfaces is in `a2m-auth`, `http_digest` and `windows_integrated` included |

### Lint rules

| Code | Level | Rule |
| --- | --- | --- |
| `http_secret_literal` | Error | A header, query, or body template holds a literal that looks like a credential |
| `http_no_response_schema` | Error | An endpoint without a response schema |
| `http_effect_method` | Warning | An endpoint marked `read` whose method is not one its description, or HTTP itself, marks safe |
| `http_signed_endpoint` | Error | An `http` binding on an endpoint marked as computed or signed in the browser ([section 14](../../spec/14-edge-cases-and-large-applications.md)) |
| `http_placeholder_unbound` | Error | A path placeholder with no input and no `path_params` entry |
| `http_graphql_unpinned` | Error | A GraphQL endpoint without an operation name and a query hash |
| `api_operation_missing` | Error | An `operation_id` not in the pinned description |
| `api_description_changed` | Error | The pinned description's digest differs from the one the binding was written against |

### Drafting rules

| From a finding about | The drafter writes | Left to the developer |
| --- | --- | --- |
| A published description | Endpoint entries with field types from its schema; one read operation for each method the description marks safe: GET and HEAD in OpenAPI, a query in GraphQL, a read of an entity set in OData | Which become tools, and their names; every write |
| An entity set with a declared key, in OData | The key as the operation's `natural` identity | Nothing more about identity |
| An endpoint seen in capture, with no description | An endpoint entry with its template tiers marked, at `inferred` | Confirming every template; all operations |

### Discoverers

| Discoverer | Action | Reads | May send |
| --- | --- | --- | --- |
| `http_description` | `spec.import`, `discover.run` | A description given as a file, or fetched from the surface | GET of the description's address. For GraphQL, the one fixed introspection query, which the GraphQL contract makes a read. Nothing else |
| `http_surface` | `target.probe`, `auth.detect` | Whether the base address answers; challenges in `WWW-Authenticate`; a request for a client certificate; links to a description | GET, HEAD, OPTIONS |

**Discovery cannot write.** Discovery runs as a job on a runner, inside the engine's sandbox, and a discoverer is given only what `SandboxPort` offers. Within that, it is given a `ReadClient`, a type that can build only GET, HEAD, and OPTIONS requests and the fixed introspection request, and that refuses a destination outside the declared surfaces. It has no way to send a body of the caller's choosing. The drivers' client is a different type that the discoverers are never given.

## Interface

A sketch.

```rust
pub fn register(reg: &mut Registration, ctx: &PackContext) -> Result<(), Failure>;

pub struct HttpDriver { clients: Clients }
impl Driver for HttpDriver {                                       // mode and capabilities are in the manifest
    async fn prepare(&self, ctx: &RunContext, lease: &SessionLease) -> Result<Prepared, Failure>;
    async fn act(&self, p: &mut Prepared, step: &Step, input: &Values) -> Result<ActOutcome, Failure>;
    async fn health(&self, surface: &Surface) -> Health;          // a cheap declared read, or a HEAD
    async fn release(&self, p: Prepared);
}
pub struct ApiDriver { clients: Clients }                          // the same, mode "api"

/// Inside the crate. Warm connections per session and surface; the lease's credentials are
/// injected into one request at a time and never held.
pub(crate) struct Clients { /* pools by (session, origin) */ }

/// Inside the crate. One per description format.
pub(crate) trait DescriptionFormat {
    fn name(&self) -> &'static str;                                // openapi, graphql, odata
    fn parse(&self, bytes: &[u8]) -> Result<Description, ImportError>;
    fn operations(&self, d: &Description) -> Vec<DescribedOperation>;   // with safe or not, by the format's own rule
}

pub(crate) struct ReadClient { /* GET, HEAD, OPTIONS, and the fixed introspection request */ }
```

What the kernel gives this crate:

- **Packs share data shapes, never code.** There is no kernel request-template type. This crate's template, a resolved origin, a path of literal segments and typed holes, query, header, and body templates, the response schema, the anti-forgery source, the idempotency header, and the version precondition, is its own opaque step payload inside the core's `Plan`, stored and hashed by the kernel and decoded only here. The typed-template guarantee of spec section 46 is kept by each pack that compiles `endpoints/` entries.
- The engine reaches the driver through a handle that refuses a committing step without the ledger's intent token. Every failure says whether the request was not sent, was answered, or is unknown. An outcome may carry outputs known before the commit point.
- `SessionLease::inject` places a token or the session's cookies into one outgoing request without returning them, and the lease takes the report of an expiry signal.
- A step marked as a lookup may report a record as absent, and a 404 on a declared lookup is that answer, not a failure.

### How each mode fills the loop

| Mode | Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- | --- |
| `http` | A warm connection to the surface; the anti-forgery bootstrap if declared | `token`, then `request` from the template | `validate`: the status is one the endpoint declares and the body fits its schema | The endpoint the binding names; or the save response with `prefer: response` |
| `api` | The same, for the description's server | `call`: the described operation, with `args` typed by its schema | The same | The described read operation the binding names |

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | The two drivers. A redirect is not followed across origins; a redirect to an identity provider is an expiry signal |
| 2 | Read-back for every write | The named read endpoint or operation. A write without one is `irreversible` |
| 3 | Each identity strategy | `natural`: a GET by key. `composite`: a nested path, parent first. `stamped`: the record's spare field, or the `idempotency_header` where the service honors one, then a list request filtered on it. `find`: a search endpoint with `settle`. `none`: `irreversible` |
| 4 | A test stand | The test application's JSON interface, cases 1 to 5 and 16 to 24, with a published OpenAPI description of it to be added. GraphQL and OData stands are small additions when package 4.4 needs them |
| 5 | A discoverer | The two above |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Network: the declared `http` and `api` surfaces only, `enforced`, because the plan resolves every origin and the client refuses any other. Holds sessions for those surfaces. Reads a staging folder for uploads, writes one for downloads, each as a checked path from its context. Hazards come from operations |
| 8 | A record per new mode | None: decision 0005 |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-engine`, through the registry | The two drivers, only |
| `a2m-files`, through the gathered set | The `endpoints/` schema, its compilation, lowerings, lint rules, drafting rules |
| `a2m-actions`, through the registry | The discoverers and the describer |
| `a2m-pack-web-ui` and `a2m-pack-web-probe`, through data only | The validated `endpoints/` entries, which the compiler hands `page_http`'s lowering; the `endpoint` finding schema |
| A runner, inside the engine's sandbox | The discoverers, for discovery jobs |

## State

| State | Where |
| --- | --- |
| Warm connections, by session and origin | Memory, closed when the session ends or the pool idles |
| A pinned description | The project folder, as a target file, with its digest |
| Cookies and tokens | Never here. The keeper holds them; one request borrows them |

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `rejected` | `app_validation_error` | A refusal in the shape the endpoint declares for validation |
| `stale` | `version_conflict` | The version precondition failed |
| `session_expired` | `needs_login` | A status and path matching the target's declared expiry signal, or a redirect to its identity host. Reported through the lease |
| `edge_blocked` | `challenge_page`, `rate_limited_by_edge`, `forbidden_by_edge` | The edge's own answer, recognized by status and header, never retried |
| `drift` | `response_schema_mismatch` | The body does not fit the schema |
| `unknown_variant` | `unexpected_status` | A status the endpoint does not declare |
| `possible_commit` | `no_confirmation` | A write request was sent, wholly or in part, and no complete answer came back |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable` | Before any byte of a write was sent; a declared maintenance answer, with its retry time |
| `policy` | `surface_not_declared` | A template that would resolve outside the declared surfaces; refused at compile time and checked again before sending |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No input can change the origin, the path's shape, or which header a value goes in | The manipulation cases of section 51: a path input holding separators, a host, an encoded newline |
| A write whose connection drops after sending is `possible_commit`, and the test application shows at most one write after reconcile | Case 17 over `http` |
| No discovery sends a request that could write | A test with the control interface counting writes during `spec.import` and `target.probe` |
| A drafted read exists only for a method the description marks safe | Drafting tests in package E4 |
| A credential never appears in a template, a finding, a trace, or a failure | The planted-secret test, with a token in a response header and body |
| Overhead stays under 5 ms beyond the application's time | Benchmarks |

## Build order

1. Package 4.1: the `endpoints/` schema and its compilation; `HttpDriver` reads against case 1; then writes with read-back, cases 2, 3, 16 to 21.
2. Then the anti-forgery bootstrap, case 5, and edge answers, case 23.
3. Package 4.4: OpenAPI import, the `api` mode, its drafting rules; then GraphQL and OData.
4. Package E4: the drafting rule for endpoint findings from capture.

## Open points

- How the schema of `endpoints/` and of the `endpoint` finding is versioned, so that `a2m-pack-web-ui` and `a2m-pack-web-probe` are held to the version they read or write by a test.
- Two packs compile the same entries into two payloads. A test that both build the same request from the same entry would catch drift between them.
- gRPC reflection, which spec section 6 lists under `api`. Not in the first release; it needs its own stand.
- Which OpenAPI versions are accepted, and what happens to a description that references files outside itself.
- The HTTP client and its TLS stack, verified on three operating systems with a licence on the list in `deny.toml`.
- Whether the `api` mode needs to be a mode at all, or is `http` with a different source of templates. Hypothesis H-19 asks the same of `page_http`.
