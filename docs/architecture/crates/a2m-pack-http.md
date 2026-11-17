# `a2m-pack-http`

The HTTP pack: the `http` driver, the endpoint catalog as a file kind, and the import of a published OpenAPI description as a discoverer and a drafting rule. Requests are built from typed templates, sent from code with no browser, and checked against the shapes their endpoints declare. The `api` driver and the GraphQL and OData imports are designed here and not built.

## Facts

| | |
| --- | --- |
| Side | Pack: HTTP |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-files` (file-kind, lowering, lint, and drafting registration), `a2m-auth` (`SessionLease`, `CredentialSink`, `RawSign`), `a2m-provider` (`Driver`, `Discoverer`, `Describer`, `SandboxPort`, `PackContext`, `Pack`), `a2m-telemetry` (`Acting`, `record_headers`, the clock). Outside: `tokio`, `serde`, `serde_json`, and `reqwest` with only its `json` feature, all already in the workspace. In tests only: `a2m-ledger` and `a2m-vault`, for a real intent token and a real lease, and the test application |
| Built in | Package 4.1 (`http`, reads then writes): built. Package 4.4: the OpenAPI import and its drafting rule are built; the `api` mode is not. Package E4 and the anti-forgery and edge steps of 4.1: not built |
| Carries out | Spec sections [201](../../spec/2-kernel/201-archetypes-and-modes.md) (`http`), [303](../../spec/3-packs/303-the-http-pack.md), [301](../../spec/3-packs/301-the-web-pack.md) (the request rows), [206](../../spec/2-kernel/206-the-compiler-and-the-plan.md) (typed templates), [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (no input changes a destination), [401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md) and [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) (drafting from descriptions) |

## Owns

- The `endpoints/` file kind: its schema, its lint rules, and its compilation into this crate's own request templates, an opaque payload in the plan. Another pack that reads `endpoints/` compiles the entries it is handed into its own payload.
- The shape language of an endpoint's `request_schema` and `response_schema`: a closed subset of JSON Schema, written inside the entry ([decision 0096](../../decisions/0096-the-http-packs-endpoint-entry-steps-and-payload-as-building-them-settled-them.md)).
- The `endpoint` and `described_operation` finding kinds, and the form of their `found`.
- Sending a request: building it position by position from the template, placing the session's credentials, sending it, and holding the answer for the steps that check it.
- Recognizing, in an answer, a refusal, a version conflict, a declared "not now", a status nothing declares, and the signs of expiry. It reports them; it acts on none.
- Reading an OpenAPI description into findings, and the drafting rule that turns those findings into endpoint drafts.

It must never: put a secret into a template, a finding, a trace, or a failure; send to an address the run may not reach; follow a redirect; use a proxy; retry; renew a token, even when the answer says it expired; send an endpoint that writes from a step that is not a commit point; draft an endpoint as a read for a method its description does not mark safe.

## What it registers

The pack's name is `http`. Its manifest is `src/anything2mcp-provider.toml`, and what is not built is not declared in it.

| Registration | Content |
| --- | --- |
| Modes | `http`, archetype request, rank 20. `api` is not built |
| Surface kinds | `http`, with the address key `base` |
| Step kinds | `request`, `validate`, `extract`, and `probe`, which only a discoverer makes. `token` is not built |
| Drivers | The driver of `http` |
| File kinds | `endpoint`, in `endpoints/` ([file formats](../../reference/file-formats.md#endpointsyaml)), for `http` surfaces and, for requests made from inside a page, `web` surfaces; `a2m-pack-web-ui` declares that it reads it, and is handed the validated entries as data |
| Binding fields | `endpoint`, `path_params`, `query`, `body`, `extract`, and `read_back` with `endpoint`, `path_params`, `query`, `extract`, and `prefer` |
| Verb lowerings | `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `submit`, `call`. `upload` and `download` are not built |
| Finding kinds | `endpoint`: what a surface answered at its root, as `http_surface` saw it; `a2m-pack-web-probe` will produce the same kind from capture. `described_operation`: one operation of a description |
| Lint rules, drafting rules, discoverers | Below |
| Describer | Endpoints counted by whether their method is safe, with the system's own paths kept apart as untrusted text |
| Auth providers | None. Every provider for this pack's surfaces is in `a2m-auth` |
| Settings | `packs.http.max_response_bytes`: the most bytes of one answer the driver reads; 8,000,000 unless set, and an endpoint may state its own |

### Lint rules

A lint code is the pack's name, a dot, and the rule.

| Code | Level | Rule |
| --- | --- | --- |
| `http.secret_literal` | Error | A header, a query entry, or a literal in a binding's query or body looks like a credential |
| `http.no_response_schema` | Error | An endpoint without a response shape |
| `http.effect_method` | Warning | An endpoint marked `read` whose method is not one HTTP marks safe |
| `http.signed_endpoint` | Error | An `http` binding on an endpoint marked `signed`, whose requests the front end computes or signs ([section 301](../../spec/3-packs/301-the-web-pack.md)) |

The file kind and the lowering refuse, as errors under the pack's own codes: `http.path_invalid`, `http.shape_invalid`, `http.header_invalid`, `http.responses_invalid`, `http.version_invalid`, `http.not_built`, `http.unknown_endpoint`, `http.placeholder_unbound`, `http.placeholder_unknown`, `http.position_type`, `http.body_undeclared`, `http.body_field_missing`, `http.extract_unknown`, `http.output_type`, `http.effect_mismatch`, and `http.read_back_never_absent`. Not built: the GraphQL rule and the two rules of the `api` mode.

### Drafting rules

| From a finding about | The drafter writes | Left to the developer |
| --- | --- | --- |
| An operation of a published description | One endpoint entry: its method, its path as a template, its shapes in the pack's own words, the statuses the description lists, and `effect: read` only for GET and HEAD | The surface, when the target declares none at the description's address; a response shape the description does not state; every operation and every write |

Not built: the read operation drafted for each safe method, the OData key, and the rule for an endpoint seen in capture.

### Discoverers

| Discoverer | Reads | May send |
| --- | --- | --- |
| `http_description` | An OpenAPI 3.0 or 3.1 description, as JSON or YAML, given as a checked path named `description` or fetched from the path the focus names under `description_path`, `/openapi.json` unless given. A reference inside the document is followed; one to another file is not | One GET |
| `http_surface` | Whether the base address answers, with which status, which methods it allows, and the names of the schemes it challenges with | One GET and one OPTIONS of the root |

**Discovery cannot write.** A discoverer is handed only the sandbox port. Within that it uses a `ReadClient`, a type that can build only a `probe` step: a GET, HEAD, or OPTIONS request to a literal path of the surface, with no body and no credential. The driver refuses a `probe` of any other method, and refuses an endpoint that writes from any step that is not a commit point, which the port never lets through.

## Interface

```rust
pub struct HttpPack;                                   // implements the provider contract's `Pack`
pub fn add_to(builder: &mut RegistryBuilder);          // the one call the product crate makes

pub const PACK: &str;                                  // "http"
pub const HTTP_MODE: &str;                             // "http"
pub const ADDRESS_KEY: &str;                           // "base"
pub const ENDPOINT_KIND: FileKindName;                 // "endpoint"
pub const HTTP_SURFACE: SurfaceKind;                   // "http"
pub const WEB_SURFACE: SurfaceKind;                    // "web"
pub const REQUEST: StepKind;                           // and VALIDATE, EXTRACT, PROBE
pub const DESCRIPTION_FILE: &str;                      // the checked path a description is given under
pub const DESCRIPTION_PATH: &str;                      // the focus entry that names where one is served
pub mod codes;                                         // the pack's failure codes, and ALL
```

Everything else is inside the crate: the driver, the template types, the JSON reader, the shape checker, the lowering, the discoverers.

What the kernel gives this crate, and what it does not:

- **Packs share data shapes, never code.** There is no kernel request-template type. This crate's template is a surface's name, a method, a path of literal segments and whole-segment holes, literal query entries and headers, the request and response shapes, the declared answers, the idempotency header, and the version rule. It holds no scheme, host, or port: the origin is the surface's address in the run's environment, taken apart once when the driver prepares.
- The engine reaches the driver through a handle that refuses a committing step without the ledger's intent token. Every failure says whether the request was not sent, was answered, or is unknown.
- **The lease is given to `prepare` only** ([decision 0097](../../decisions/0097-what-the-provider-contract-could-not-give-the-http-driver-and-what-the-pack-does-instead.md)). The driver applies it there into its own sink. The credentials are held by the prepared client, in the core's secret wrapper, for one pass; they go into a request only for an endpoint whose `auth` is `session` or `token`, and are wiped at release.
- A step's result has no place for what the driver did. The `request` step returns it as two captures: `acting`, in the form of the telemetry crate's `Acting`, and `exchange`, the request and the answer with headers passed through the allowlist.

### The steps

| Step | Arguments | Does | Fails as |
| --- | --- | --- | --- |
| `request` | `path.<placeholder>`, `query.<name>`, `body.<field>`; its payload lists the query names and the body fields with their JSON types | Builds one request from the endpoint the step names and sends it to the surface's address. Holds the answer. On a lookup, a status the endpoint declares as `absent` is the record being absent | `input` before sending; `transient` or `possible_commit` when no complete answer came; `session_expired`; `policy` for a redirect that leaves the surface; `unknown_variant`, `response_too_large` |
| `validate` | None | Judges the held answer: an `ok` status whose body fits the response shape passes; otherwise the declared answer it matches names the failure | `drift`, `rejected`, `stale`, `transient`, `unknown_variant` |
| `extract` | None; its payload lists each output, its path in the body, and its type | Takes one typed value for each output, exactly; remembers the record's version when the endpoint says where it is read | `drift` |
| `probe` | None; its payload is a safe method and a literal path | One request with no body and no credential; returns the status, the content type, the allowed methods, the names of the challenge schemes, and a successful body as text | As `request` |

### How the mode fills the loop

| Verb | Act | Confirm | Read back |
| --- | --- | --- | --- |
| `read`, `list`, `search` | `request`, `validate`, `extract` | | |
| The write verbs | `request`, marked as the commit point, and as idempotent when the endpoint names an idempotency header | `validate`; and `extract` of the save's own answer with `prefer: response` | `request`, `validate`, `extract` on the endpoint `read_back` names, each a lookup |

A `request` step has the target's time limit for a request, 30 seconds unless the target says otherwise. A `validate` or `extract` step judges an answer already held, sends nothing, and has a limit of one second: the engine sums a binding's step limits before a commit point, and five request limits would pass the lease of the in-process runner ([decision 0119](../../decisions/0119-the-product-crate-as-building-it-settled-it.md)).

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | The driver of `http`. No redirect is followed |
| 2 | Read-back for every write | The named read endpoint, which must declare a status for a record that does not exist |
| 3 | Each identity strategy | `natural` and `composite`: the read-back endpoint, reached from the identity inputs. `stamped` and `find` are refused when the target compiles, because the lookup they need is not built. `none`: `irreversible` |
| 4 | A test stand | The test application's JSON interface, cases 1 to 4 and 17, and a small signed-in service inside the crate's tests for credentials, sign-in signals, limits, and a served description |
| 5 | A discoverer | The two above |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Network: the declared `http` surfaces only. Holds sessions for those surfaces. Hazards come from operations |
| 8 | A record per new mode | None: decision 0005 |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| The product crate | `add_to`, or `HttpPack` |
| `a2m-engine`, through the registry | The driver, only through its handle |
| `a2m-files`, through the gathered set | The `endpoints/` schema, its compilation, the lowering, the lint rules, the drafting rule |
| `a2m-actions`, through the registry | The discoverers and the describer |
| `a2m-pack-web-ui` and `a2m-pack-web-probe`, through data only | The validated `endpoints/` entries; the `endpoint` finding kind |

## State

| State | Where |
| --- | --- |
| Warm clients, by session and origin, at most 64 | Memory; one is dropped when its pass ends poisoned |
| One pass's credentials, its held answer, and the versions it read | The prepared client; dropped at release |
| Cookies and tokens beyond one pass | Never here. The keeper holds them |

## Failures

| Class | Codes | When | Sent |
| --- | --- | --- | --- |
| `rejected` | `app_validation_error` | An answer the endpoint declares under `refused` | Answered |
| `stale` | `version_conflict` | An answer the endpoint declares under `conflict`, with the version sent and the version the system now reports | Answered |
| `session_expired` | `needs_login` | A status the session's provider takes for a refusal and the endpoint does not declare; a status and path the target declares; a redirect to a host the target declares. Also an endpoint that takes a credential in a run with no session, which is not sent | Answered, or not sent |
| `drift` | `response_schema_mismatch` | The body of an accepted answer is not JSON or does not fit the shape. The message names the place and never the value | Answered |
| `unknown_variant` | `unexpected_status`, `response_too_large` | A status, or a body, that nothing declared matches; an answer over the limit | Answered |
| `possible_commit` | `no_confirmation` | A write was sent, or may have been, and no complete answer came back | Unknown |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable` | A read that timed out or lost its connection; a connection that could not be made, for a read or a write; an answer declared under `unavailable` | Unknown, not sent, answered |
| `policy` | `surface_not_declared` | A surface address the run may not reach, refused when the driver prepares; a redirect to another origin | Not sent, answered |
| `input` | `path_value_refused`, `step_not_usable`, `tls_not_enabled`, `auth_scheme_not_built`, `description_unreadable` | A path value that is empty, a dot segment, or holds a separator or a control character; a step the driver cannot carry out as written; an HTTPS surface; a session whose scheme needs an exchange that is not built; a description that is not OpenAPI 3.0 or 3.1 | Not sent |

The edge's answers are not built: `challenge_page`, `rate_limited_by_edge`, and `forbidden_by_edge` are not declared, and such an answer stops as `unexpected_status`.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No input can change the origin or the path's shape | `no_input_changes_where_a_request_goes`, against the test application; `no_value_can_change_the_shape_of_the_path` |
| A write whose answer is lost is `possible_commit`, and the test application shows exactly one write | `a_write_whose_answer_is_lost_is_a_possible_commit_and_is_not_sent_again` |
| A write reaches the system only with the ledger's token | `a_write_without_the_ledgers_token_never_reaches_the_application` |
| No discovery sends a request that could write | `discovery_observes_and_cannot_write`, with the control interface counting writes |
| A drafted read exists only for a method the description marks safe | `a_published_description_becomes_claims_and_a_read_is_drafted_only_for_a_safe_method` |
| A credential never appears in a value, a capture, or a failure, and reaches only an endpoint that takes it | `a_credential_reaches_only_the_request_it_is_for_and_appears_nowhere_else`, with the token planted in a response header and body |
| The same target compiles to the same bytes | `the_fixture_targets_compile_twice_to_the_same_bytes` |
| Overhead stays under 5 ms beyond the application's time | Not measured yet |

## Build order

1. Done: the `endpoints/` schema and its compilation; the `http` driver's reads against case 1; writes with read-back and the version precondition, cases 2 to 4; the lost answer, case 17.
2. Done: credentials through the lease for `api_key`, `bearer_static`, and `http_basic`; sign-in signals; limits on size and time; redirects refused; HTTPS refused by name.
3. Done: the OpenAPI import, `http_description`; `http_surface`; the drafting rule for endpoint entries.
4. Not built: the anti-forgery bootstrap and the `token` step, case 5; the edge's answers, case 23; cases 16 and 18 to 22 and 24, which the test application does not serve yet.
5. Not built: the `api` mode; the read operation drafted for each safe method; GraphQL and OData; the drafting rule for endpoints seen in capture, package E4.
6. Not built: uploads and downloads, pagers, long-running jobs, identity by a stamp or a search, the digest and integrated Windows exchanges, a client certificate, TLS.

## Open points

- How the schema of `endpoints/` and of the `endpoint` finding is versioned, so that `a2m-pack-web-ui` and `a2m-pack-web-probe` are held to the version they read or write by a test. The payload carries the format number 1, and each finding's `found` carries `format`.
- Two packs compile the same entries into two payloads. A test that both build the same request from the same entry would catch drift between them.
- The `endpoint` finding's `found` is what `http_surface` wrote first: a method, a path, a status, a content type, the allowed methods, and the challenge schemes. The probe of the web pack needs template tiers in it, and will widen it.
- Whether the lease should reach `act`, so that a credential is placed for each request and the provider is asked about each sign as it is seen (decision 0097).
- The read-back of a `delete`, where absent is the expected answer, is the engine's to judge; the pack reports it as absent.
- gRPC reflection, which spec section 201 lists under `api`. Not in the first release; it needs its own stand.
- The TLS stack, verified on three operating systems with a licence on the list in `deny.toml`.
- Whether the `api` mode needs to be a mode at all, or is `http` with a different source of templates. Hypothesis H-19 asks the same of `page_http`. The import built here drafts ordinary endpoint entries, which is evidence for the second.
- The overhead of decoding the plan's endpoints at every `prepare`, to be measured against the 5 ms budget.
