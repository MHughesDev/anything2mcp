# 303. The HTTP pack

The HTTP pack lets a target be a service reached by HTTP requests, described or not. Requests are built from typed templates, sent from code with no browser at run time, and checked against their schemas. It brings two modes in the request archetype: `http`, for requests sent directly, and `api`, for calls generated from a published description of the service. It owns the endpoint catalog, the `endpoints/` file kind, which the web pack's `page_http` mode also reads.

The pack is one crate, `a2m-pack-http`. It has no base crate and depends on no other pack. Its [design](../../architecture/crates/a2m-pack-http.md) is the exact statement of how the code is arranged; this section says what the pack does and why.

## What it reaches, and what it refuses

| Reaches | How |
| --- | --- |
| An HTTP interface on a surface the target declares | A client in code, on a warm connection per session and origin, with credentials injected through the session's lease |
| A service with a published description: OpenAPI, a GraphQL schema, or OData metadata | Imported as findings, pinned with its digest, and called through `api` |
| A service with no description | Endpoint entries drafted from captured traffic ([section 301](301-the-web-pack.md)), at `inferred` until proven |
| Hosted services, object storage, search indexes, and devices with a management protocol over HTTP | Their HTTP interface, through this pack ([section 106](../1-product/106-what-it-latches-onto.md)) |

| Refused | Why |
| --- | --- |
| Sending to an origin the plan did not resolve, following a redirect, or using a proxy | Section [502](../5-security/502-defending-against-a-manipulated-agent.md): no input changes where a request goes |
| A secret in a template, a finding, or a trace | Rule four. Credentials reach a request only through injection |
| Holding a credential beyond one pass of the loop, or renewing a token, even when a response says it expired | Only the session keeper renews. The driver reports the sign and stops ([decision 0097](../../decisions/0097-what-the-provider-contract-could-not-give-the-http-driver-and-what-the-pack-does-instead.md)) |
| Retrying inside a step | The engine decides what is tried again |
| Drafting an operation for a method its description does not mark safe | A drafted read must be a read |
| An `http` binding on an endpoint whose requests the front end computes or signs | It would skip logic only the page carries out. Such an endpoint is `page_http` or `ui` only |
| Imitating another application's client to obtain a token | Rule five. Where a direct token is wanted, the route is a registration that IT approves, with delegated permission, so tokens carry the user's own rights and appear in sign-in logs |
| Getting past an edge's bot defenses | Rule five. The supported fix is an allowlist from the system's owner: the runner's outbound address, with a secret header or a client certificate |

## Modes

| Mode | Archetype | How it reaches the system | Needs | Best for | Main risk |
| --- | --- | --- | --- | --- | --- |
| `http` | Request | Requests sent directly from code, with no browser at run time | A session cookie or token from the session keeper | Bulk and high-volume work; servers without a browser | Undocumented endpoints change; bot defenses |
| `api` | Request | Calls generated from a published description | A token or key | Systems with an official interface | Coverage gaps |

Both modes are from [decision 0005](../../decisions/0005-seven-modes-behind-one-driver-interface.md). Their ranks in the pack's manifest put them ahead of `page_http` and `ui` in the default order of bindings, so the cheapest verified binding is tried first ([section 201](../2-kernel/201-archetypes-and-modes.md)). gRPC reflection, once listed under `api`, is not in the first release; it needs its own stand.

## What it registers

| Registration | Content |
| --- | --- |
| Surface kinds | `http` |
| Modes | `http` and `api`, archetype request |
| Step kinds | `token`, `request`, `validate`, `extract` for `http`; `call` for `api`. Checked names within each mode |
| Drivers | One per mode |
| File kinds | Owns `endpoints/`, for `http` surfaces and, for requests made from inside a page, `web` surfaces. The web pack declares that it reads the kind and is handed the validated entries as data. Owns a pinned copy of an imported description, with its digest, which an `api` binding names |
| Binding fields | `http`: `endpoint`, `path_params`, `query`, `body`. `api`: `operation_id`, `args` |
| Finding kinds | `endpoint`, with the schema of its `found`, which the web pack's probe produces and this pack's drafting rule reads; `described_operation`, from descriptions |
| Discoverers | `http_description`, `http_surface` (below) |
| Describer | `http_describer`: endpoints as method, template, and shapes |
| Auth providers | None. Every provider for this pack's surfaces is in `a2m-auth` (below) |

## The endpoint catalog

Endpoints live only in the endpoint catalog (rule eight). There is one catalog, owned by this pack and read by two: the `http` mode and the web pack's `page_http`. Each compiles the entries it uses into its own step payload; there is no kernel type for a request ([decision 0062](../../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)).

| Field | Meaning |
| --- | --- |
| `endpoint` (required) | Its name |
| `surface` (required) | An `http` surface, or a `web` surface for requests made from inside the page |
| `method`, `path` (required) | The method and a path template |
| `query`, `headers` | Templates; no secrets |
| `auth` | `session`, `token`, or `none`; supplied by the session keeper |
| `anti_forgery` | Where the token comes from (`cookie`, `meta`, or another endpoint) and the header it goes in |
| `request_schema`, `response_schema` (response required) | The shape of each body, written in the entry in a closed subset of JSON Schema: `type`, `nullable`, `properties`, `required`, `additionalProperties`, `items`, `enum` ([decision 0096](../../decisions/0096-the-http-packs-endpoint-entry-steps-and-payload-as-building-them-settled-them.md)) |
| `responses` | The answers the endpoint declares: `ok` and `absent` statuses, and `refused`, `conflict`, and `unavailable`, each a status with an optional `when` on a field of the body |
| `signed` | True when the front end computes or signs the request; an `http` binding on it does not compile |
| `max_response_bytes` | The most bytes of an answer that are read |
| `effect` (required) | `read` or `write` |
| `idempotency_header` | The header that carries the record's idempotency token, if the service accepts one |
| `version` | Where a record's version is read, and how it is sent back as a precondition. A write that sends one takes it from the endpoint its binding reads the record back through, which must say where it is read |
| `volatile` | Request fields to ignore when comparing with a shadow |

The full format is in the [file formats](../../reference/file-formats.md#endpointsyaml).

**Typed templates.** The pack's template is a surface, whose address in the run's environment is the origin, a path of literal segments and holes that each fill one whole segment, query, header, and body templates, the response schema, the anti-forgery source, the idempotency header, and the version precondition. A request is built position by position from it. No input can change the origin, the shape of the path, or which header a value goes in ([section 206](../2-kernel/206-the-compiler-and-the-plan.md)). A template that would resolve outside the declared surfaces is refused at compile time and checked again before sending: the driver sends only to an address that reads back as the surface's origin and exactly the path and query that were built.

| Behavior of a service | Built-in handling |
| --- | --- |
| Rotating anti-forgery tokens | A declared bootstrap request or page field supplies the token per request, in the `token` step |
| Optimistic locking (ETag, version numbers) | Read the version first and send it with the write; a conflict is `stale`, `version_conflict`, with both versions shown. The version is kept until a request may have reached the system, so an attempt that was not sent is tried again with it. A write for a record whose version the pass did not read is refused as `stale`, `version_not_read`, not sent; a record that was looked up and is absent is created with none |
| GraphQL, including persisted queries | The entry stores the operation name and the query hash |
| An idempotency header the service honors | The record's token goes in it, and a write the plan marks idempotent may be retried by the engine |
| Pagination by page or cursor | The `list` verb with a pager per style |
| Long-running jobs | `call`, then poll a declared status until done or a time limit |
| Uploads and downloads | Declared on the step, through a staging folder given as a checked path |
| A declared maintenance answer | `transient`, `upstream_unavailable`, with its retry time |
| Requests the front end computes or signs | Marked on the endpoint; `http` is refused for it |

**Responses.** A response is accepted when its status is one the endpoint declares and its body fits the response schema. A body that is not JSON fits no schema, `any` included; only an empty body is read as nothing. Values are extracted from it as typed values and normalized by their field types on both sides before any comparison ([section 210](../2-kernel/210-sources-and-value-normalization.md)); no number is compared as a float. A status the endpoint does not declare is `unknown_variant`; a body that does not fit is `drift`.

## Verbs and how they lower

| Verb | `http` | `api` |
| --- | --- | --- |
| `open` | No | No |
| `read`, `list`, `search` | Yes | Yes |
| `create`, `update`, `upsert`, `delete` | Yes | Yes |
| `upload`, `download` | Yes | Yes |
| `submit`, `call` | Yes | Yes |

`http` lowers each verb into `token`, `request`, `validate`, and `extract`. `api` lowers into `call`: the described operation, with `args` typed by its schema, compiled into the same templates. Each write request is marked as a commit point and each declared lookup as a lookup.

## The driver, part by part

| Mode | Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- | --- |
| `http` | A warm connection to the surface; the anti-forgery bootstrap if declared | `token`, then `request` from the template | `validate`: the status is one the endpoint declares and the body fits its schema | The endpoint the binding names; or the save response with `prefer: response` |
| `api` | The same, for the description's server | `call` | The same | The described read operation the binding names |

- **Every failure says whether the request was sent.** A write request sent wholly or in part with no complete answer is `possible_commit`; before any byte of a write was sent, a lost connection is `transient`.
- **A write that was sent is settled only by the application's verdict.** An answer that is not one, a redirect to another origin or an answer too large to read, leaves the write in doubt as `possible_commit`, and the read-back or reconcile decides. The driver never reports such a write in a class the ledger takes as settled ([decision 0139](../../decisions/0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md)).
- **A 404 on a declared lookup is an answer, not a failure:** the record is absent.
- **A redirect is never followed.** One to the target's identity host is a sign of expiry; one to any other origin stops a read as `policy`, `surface_not_declared`, and leaves a write in doubt; one within the surface is a status like any other, accepted only when the endpoint declares it. Where a redirect leads is read as a browser reads it, against the address of the request, with a scheme or without one.
- **HTTPS is behind the build feature `tls`** ([decision 0157](../../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)). With it, an `https` surface is reached through a client the session keeper's crate makes, which checks a certificate against the roots in `network.trust_roots`, then the operating system's, and speaks TLS 1.2 or 1.3. A certificate that does not lead to a trusted authority, is for another name, or has expired, and a handshake that fails, each stop as `unknown_variant` with their own code, before anything is sent. A client for an `https` surface sends nothing that is not `https`. Without the feature, an `https` surface is refused with `input`, `tls_not_enabled`.
- **Edge answers are recognized by status and header,** never retried, and stop as `edge_blocked`.
- Overhead stays under 5 ms beyond the system's own time, a budget until measured ([section 225](../2-kernel/225-performance.md)).

## Sessions and sign-in

The rules for sessions are the kernel's ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)). The division of work here is exact: **the auth provider in `a2m-auth` holds and injects the credential; the HTTP driver carries out the scheme's round trips on the connection.**

| Provider, in `a2m-auth` | Covers | The driver's part |
| --- | --- | --- |
| `http_basic`, `http_digest` | Simple intranet systems | Answering the challenge, for digest the exchange of nonces |
| `windows_integrated` | NTLM and Kerberos | The negotiation on the connection |
| `client_certificate` | Mutual TLS | Presenting the certificate the lease injects |
| `api_key`, `bearer_static` | Interfaces with fixed keys | Placing the key in the declared header |
| `oauth_client_credentials`, `oauth_authorization_code` | Interfaces with OAuth | Sending the bearer token the lease injects |
| `obo_token` | An on-behalf-of exchange from the caller's token | The same |
| `os_broker` | A token from the operating system's sign-in broker, on user devices | The same |

- The lease places a token or the session's cookies into the driver's prepared client without returning them. The client holds them for one pass of the loop, sends them only to an endpoint that takes the session's credential, and wipes them at release. Warm connections are kept by session and origin.
- The keeper hands the driver short-lived tokens in memory only.
- A status and path matching the target's declared expiry signal, or a redirect to its identity host, is reported through the lease as `needs_login`. The driver never renews.
- Where an authorization-code response arrives on a server, when no loopback address reaches the person's browser, is open.

## Discovery and drafting

| Discoverer | Action | Reads | May send |
| --- | --- | --- | --- |
| `http_description` | `spec.import`, `discover.run` | A description given as a file, or fetched from the surface. Either way its text passes the redactor before it is parsed, and reading it is bounded in depth, in steps, and in bytes; past a bound the import is refused as `input`, `description_too_large` | GET of the description's address. For GraphQL, the one fixed introspection query, which the GraphQL contract makes a read. Nothing else |
| `http_surface` | `target.probe`, `auth.detect` | Whether the base address answers; challenges in `WWW-Authenticate`; a request for a client certificate; links to a description | GET, HEAD, OPTIONS |

**Discovery cannot write.** Discovery runs as a job on a runner, inside the engine's sandbox ([section 218](../2-kernel/218-discovery-and-findings.md)). Within that, a discoverer is given a read client: a type that can build only GET, HEAD, and OPTIONS requests and the fixed introspection request, and that refuses a destination outside the declared surfaces. It cannot send a body of the caller's choosing. The drivers' client is a different type that discoverers are never given.

A description that says an endpoint exists is a claim, at `inferred` with source `published_description`. The endpoint answering is an observation, at `discovered`. They are never shown as the same, and comparing them lists what is described and unexercised and what was seen and is nowhere described.

| From a finding about | The drafter writes | Left to the developer |
| --- | --- | --- |
| A published description | Endpoint entries with field types from its schema, for the methods the description marks safe only: an entry for any other method describes a write, and the drafter refuses it; one read operation for each method the description marks safe: GET and HEAD in OpenAPI, a query in GraphQL, a read of an entity set in OData | Which become tools, and their names in the language of the work; every write, its endpoint entry included |
| An entity set with a declared key, in OData | The key as the operation's `natural` identity | Nothing more about identity |
| An endpoint seen in capture, with no description | An endpoint entry with its template tiers marked, at `inferred` | Confirming every template; all operations |

**From capture to a verified binding.** Endpoints inferred from a web application's traffic, with two-tier path templates, come from the web pack's probe ([section 301](301-the-web-pack.md)); the `endpoint` finding kind and its schema are this pack's, and the sink validates every one against it. A drafted `http` binding starts in `shadow` beside the verified `ui` binding of the same operation, predicts what it would send without acting, and is promoted by a person after a set number of exact matches (`shadow.report`, `binding.promote`). Reads are promoted quickly; writes need more evidence. An endpoint the probe marked as computed or signed in the browser is drafted for `page_http` only, and an `http` binding on it does not compile.

## Lint rules

| Code | Level | Rule |
| --- | --- | --- |
| `http.secret_literal` | Error | A header, query, or body template holds a literal that looks like a credential |
| `http.no_response_schema` | Error | An endpoint without a response schema |
| `http.effect_method` | Warning | An endpoint marked `read` whose method is not one its description, or HTTP itself, marks safe |
| `http.signed_endpoint` | Error | An `http` binding on an endpoint marked as computed or signed in the browser |
| `http.placeholder_unbound` | Error | A path placeholder with no input and no `path_params` entry; refused by the lowering |
| `http.graphql_unpinned` | Error | A GraphQL endpoint without an operation name and a query hash. Not built |
| `http.api_operation_missing` | Error | An `operation_id` not in the pinned description. Not built |
| `http.api_description_changed` | Error | The pinned description's digest differs from the one the binding was written against. Not built |

## Effects and hazards

| Effect | Declared as |
| --- | --- |
| Uses the network | The declared `http` and `api` surfaces only. `enforced`, because the plan resolves every origin and the client refuses any other |
| Holds a session | For those surfaces |
| Reads and writes files | A staging folder for uploads and one for downloads, each a checked path from the pack's context |

Hazards come from operations ([section 213](../2-kernel/213-effects-hazards-and-consent.md)). A request that sends something out of the system, such as a message or a payment, is declared by its operation with the hazard `sends_outside`.

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `rejected` | `app_validation_error` | A refusal in the shape the endpoint declares for validation |
| `stale` | `version_conflict` | The version precondition failed |
| `stale` | `version_not_read` | A write that sends a version, for a record whose version the pass did not read. Not sent |
| `session_expired` | `needs_login` | The declared expiry signal, or a redirect to the identity host. Reported through the lease |
| `edge_blocked` | `challenge_page`, `rate_limited_by_edge`, `forbidden_by_edge` | The edge's own answer, recognized by status and header, never retried |
| `drift` | `response_schema_mismatch` | The body does not fit the schema |
| `unknown_variant` | `unexpected_status` | A status the endpoint does not declare |
| `possible_commit` | `no_confirmation` | A write request was sent, wholly or in part, and no complete answer came back, or what came back is not the application's verdict: a redirect to another origin, or an answer over the limit on its size |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable` | Before any byte of a write was sent; a declared maintenance answer, with its retry time |
| `policy` | `surface_not_declared` | A template that would resolve outside the declared surfaces, or, for a read, a redirect to another origin |
| `unknown_variant` | `response_too_large` | An answer to a read over the limit on its size |
| `unknown_variant` | `certificate_untrusted`, `certificate_name_mismatch`, `certificate_expired`, `certificate_not_verified`, `tls_handshake_failed` | An `https` surface whose certificate or handshake fails; nothing was sent. The session keeper's codes, passed on |
| `input` | `path_value_refused`, `step_not_usable`, `tls_not_enabled`, `auth_scheme_not_built`, `description_unreadable`, `description_too_large` | A path value that is empty, a dot segment, or holds a separator; a step the driver cannot carry out as written; an HTTPS surface in a build without TLS; a session scheme whose exchange is not built; a description that cannot be read; a description past the bounds of the import |

## The test stand

The test application's JSON interface ([test application plan](../../plan/test-application.md)), with its control interface for ground truth: cases 1 to 5 and 16 to 24. A published OpenAPI description of it is to be added. GraphQL and OData stands are small additions when package 4.4 needs them.

| Must always hold | Proven by |
| --- | --- |
| No input can change the origin, the path's shape, or which header a value goes in | The manipulation cases of section 502: a path input holding separators, a host, an encoded newline |
| A write whose connection drops after sending is `possible_commit`, and the stand shows at most one write after reconcile | Case 17 over `http` |
| A write that the system made and answered with a redirect to another origin is in doubt, never settled, and the stand shows the write | The stand's fault "redirect after commit" |
| A write that was not sent is tried again with the version it read, and a write with no version read is not sent | A link in front of the stand that refuses the first connection, with another caller's change in between |
| No discovery sends a request that could write | The control interface counts writes during `spec.import` and `target.probe` |
| A drafted read exists only for a method the description marks safe | Drafting tests |
| A credential never appears in a template, a finding, a trace, or a failure | The planted-secret test, with a token in a response header and body |

## The entry bar

The eight items of [section 202](../2-kernel/202-kernel-and-packs.md).

| # | Item | The HTTP pack's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | One per mode. A redirect is not followed across origins |
| 2 | Read-back for every write | The named read endpoint or operation. A write without one is `irreversible` |
| 3 | Each identity strategy | `natural`: a GET by key. `composite`: a nested path, parent first. `stamped`: the record's spare field, or the `idempotency_header` where the service honors one, then a list request filtered on it. `find`: a search endpoint with `settle`. `none`: `irreversible` |
| 4 | A test stand | The test application's JSON interface, as above |
| 5 | A discoverer | `http_description` and `http_surface` |
| 6 | Failures mapped | The table above |
| 7 | Effects and hazards | The table above; hazards come from operations |
| 8 | A record per new mode | None needed: decision 0005 |

## Build order

Package 4.1: the `endpoints/` schema and its compilation, `http` reads against case 1, then writes with read-back, cases 2, 3, and 16 to 21; then the anti-forgery bootstrap, case 5, and edge answers, case 23. Package 4.4: OpenAPI import, the `api` mode, and its drafting rules; then GraphQL and OData. Built so far: the `http` mode against cases 1 to 4 and 17, credentials and sign-in signals, and the OpenAPI import with its endpoint drafts; the [crate's design](../../architecture/crates/a2m-pack-http.md) lists what is not. Package E4: the drafting rule for endpoint findings from capture. Package 2.6 touches the pack for sign-in signals on HTTP surfaces.

## Open points

- How the schemas of `endpoints/` and of the `endpoint` finding are versioned, so that the web pack is held by a test to the version it reads or writes.
- Two packs compile the same entries into two payloads. A test that both build the same request from the same entry would catch drift between them.
- gRPC reflection, and its stand.
- Which OpenAPI versions are accepted, and what happens to a description that references files outside itself.
- The HTTP client and its TLS stack, verified on three operating systems with a licence on the closed list.
- Whether `api` needs to be a mode at all, or is `http` with a different source of templates. Hypothesis H-19 asks the same of `page_http`.
- Where an authorization-code response arrives on a server.
