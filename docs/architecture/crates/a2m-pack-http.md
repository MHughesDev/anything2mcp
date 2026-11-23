# `a2m-pack-http`

The HTTP pack: the `http` driver, the endpoint catalog as a file kind, and the import of a published OpenAPI description as a discoverer and a drafting rule. Requests are built from typed templates, sent from code with no browser, and checked against the shapes their endpoints declare. The `api` driver and the GraphQL and OData imports are designed here and not built.

## Facts

| | |
| --- | --- |
| Side | Pack: HTTP |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-files` (file-kind, lowering, lint, and drafting registration), `a2m-auth` (`SessionLease`, `CredentialSink`, `RawSign`), `a2m-provider` (`Driver`, `Discoverer`, `Describer`, `SandboxPort`, `PackContext`, `Pack`), `a2m-telemetry` (`Acting`, `record_headers`, the clock). Outside: `tokio`, `serde`, `serde_json`, and `reqwest` with only its `json` feature, all already in the workspace; `regex`, the command pack's, for the pattern of a text answer; and `quick-xml` with no default features, for an XML answer ([decision 0217](../../decisions/0217-the-http-pack-reads-answers-that-are-not-json.md)). In tests only: `a2m-ledger` and `a2m-vault`, for a real intent token and a real lease, and the test application |
| Roadmap packages | Package 4.1 (`http`, reads then writes): built. Package 4.4: the OpenAPI import and its drafting rules are built; the `api` mode is not. Package E4 and the anti-forgery and edge steps of 4.1: not built |
| Carries out | Spec sections [201](../../spec/2-kernel/201-archetypes-and-modes.md) (`http`), [303](../../spec/3-packs/303-the-http-pack.md), [301](../../spec/3-packs/301-the-web-pack.md) (the request rows), [206](../../spec/2-kernel/206-the-compiler-and-the-plan.md) (typed templates), [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (no input changes a destination), [401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md) and [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) (drafting from descriptions) |

## Owns

- The `endpoints/` file kind: its schema, its lint rules, and its compilation into this crate's own request templates, an opaque payload in the plan. Another pack that reads `endpoints/` compiles the entries it is handed into its own payload.
- The shape language of an endpoint's `request_schema` and `response_schema`: a closed subset of JSON Schema, written inside the entry ([decision 0096](../../decisions/0096-the-http-packs-endpoint-entry-steps-and-payload-as-building-them-settled-them.md)).
- The forms of an answer, an endpoint's `answer`: JSON, the default; XML, read by declared paths in a closed language; text, by its character set and an anchored pattern; bytes, by length and digest ([decision 0217](../../decisions/0217-the-http-pack-reads-answers-that-are-not-json.md)). An answer that is not JSON is read into its **record**, the values its endpoint declares, which is the shape its `extract` paths are checked against.
- The `endpoint` and `described_operation` finding kinds, and the form of their `found`.
- Sending a request: building it position by position from the template, placing the session's credentials, sending it, and holding the answer for the steps that check it.
- Recognizing, in an answer, a refusal, a version conflict, a declared "not now", a status nothing declares, and the signs of expiry. It reports them; it acts on none.
- Reading an OpenAPI description into findings, and the drafting rule that turns those findings into endpoint drafts.

It must never: put a secret into a template, a finding, a trace, or a failure; send to an address the run may not reach; follow a redirect; use a proxy; retry; renew a token, even when the answer says it expired; send an endpoint that writes from a step that is not a commit point; report a write that was sent as settled when the answer is not the application's verdict; send a write that declares a version precondition without knowing the record's version; read a description without bound; draft an endpoint as a read for a method its description does not mark safe.

## What it registers

The pack's name is `http`. Its manifest is `src/anything2mcp-provider.toml`, and what is not built is not declared in it.

| Registration | Content |
| --- | --- |
| Modes | `http`, archetype request, rank 20. `api` is not built |
| Surface kinds | `http`, with the address key `base`, and `unencrypted_credentials: accepted`, the one way to let the session's credential cross plain `http` to an address that is not a loopback address ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)) |
| Step kinds | `request`, `validate`, `extract`, and `probe`, which only a discoverer makes. `token` is not built |
| What saves, for the compiler | `Lowering::commits`: `request` is `AsDeclared`, because the same kind sends a read and a write and which it is is the `effect` of the endpoint it names; the lowering marks it `commits` exactly for a write verb, which it binds only to an endpoint marked `write`. `validate`, `extract`, and `probe` are `Never`. No kind always saves, so the manifest lists no `commit_steps`. `Lowering::least_effect`: `read` for `read`, `list`, and `search`; `irreversible` for `delete` and `submit`, and for any other write whose binding names no `read_back`; `write` otherwise |
| What can describe a write | `FileKind::writes` on an endpoint entry: yes unless the entry is marked `read` and its method is one HTTP marks safe (GET, HEAD, OPTIONS). An effect left as an open question is not known to read. The answer is the same for `http` and `web` surfaces, so it holds for the entries `page_http` reads too |
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
| `http.effect_method` | Error | An endpoint marked `read` whose method HTTP does not mark safe, unless it says `read_asserted: true`; `read_asserted` on any other endpoint ([decision 0249](../../decisions/0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md)); an endpoint marked `read` whose header `X-HTTP-Method-Override`, `X-HTTP-Method`, or `X-Method-Override`, or literal query entry `_method`, names a method HTTP does not mark safe or no method at all, `read_asserted` or not; and, from the lowering, a binding that sets the query entry `_method` for an endpoint marked `read` ([decision 0270](../../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)) |
| `http.signed_endpoint` | Error | An `http` binding on an endpoint marked `signed`, whose requests the front end computes or signs ([section 301](../../spec/3-packs/301-the-web-pack.md)) |
| `http.unencrypted_credentials` | Warning | A surface declares `unencrypted_credentials: accepted`: its credentials cross an unencrypted link to another machine |

The file kind and the lowering refuse, as errors under the pack's own codes: `http.path_invalid`, `http.shape_invalid`, `http.header_invalid` (which includes a header the connection sets, `host`, `content-length`, `transfer-encoding`, or `connection`, as an endpoint's header or its idempotency header), `http.responses_invalid`, `http.answer_invalid` (an `answer` its form does not take: a path outside the closed language, a prefix not declared, a pattern longer than 4,096 bytes, one that does not compile or whose compiled form would take more than 256 KiB, or a field that is not one of its groups, a `response_schema` beside a form that is not JSON), `http.version_invalid` (which includes an endpoint that sends a version, bound with no `read_back` endpoint that reads one), `http.not_built`, `http.unknown_endpoint`, `http.placeholder_unbound`, `http.placeholder_unknown`, `http.position_type`, `http.body_undeclared`, `http.body_field_missing`, `http.extract_unknown`, `http.output_type`, `http.effect_mismatch`, and `http.read_back_never_absent`. Not built: the GraphQL rule and the two rules of the `api` mode.

### Drafting rules

| From a finding about | The drafter writes | Left to the developer |
| --- | --- | --- |
| An operation of a published description | One endpoint entry: its method, its path as a template, its shapes in the pack's own words, the statuses the description lists, `auth: none` when the description states that the operation needs no credential, and `effect: read` only for GET and HEAD. An operation of any other method is drafted as `effect: write`, and the drafter refuses it and writes nothing, because the file kind says it describes a write | The surface, when the target declares several `http` surfaces and the address the description was read for does not tell which; a response shape the description does not state; every write, its endpoint entry included |
| A GET of a published description ([decision 0179](../../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)) | One read operation, `{target}_{object}_{get or list}` from the path's last two plain words: the path's placeholders as its inputs and key; an output for each plain property of the answer, or of each item of a list answer (`verb: list`, taken by `*.name`), text as `untrusted_text`; and, when the description gives one for every placeholder, its own example | Its name in the language of the work; a nested value; a read whose placeholder the description states no parameter for, or that needs a parameter outside the path; the test and its fixture |

**A property a description does not require may be `null`.** In the `response_schema` drafted from a description, a property that the description does not list under its object's `required` is written `nullable`, of any kind and at any depth, because many systems write "absent" as `null` and few descriptions say so ([decision 0297](../../decisions/0297-a-property-a-description-does-not-require-is-drafted-as-possibly-null.md)). A required property is drafted as the description states it, a request's shape and a parameter's are drafted without the rule, and the check of an answer does not change: a `null` where the file does not say `nullable`, and a value of another kind, still stop the read as `response_schema_mismatch`.

**An endpoint belongs to the surface its description was read for.** A discovery is of one surface, and its findings carry that surface's origin and the path its address ends in, whether the description was fetched or handed in as a file. A target that declares one `http` surface has no other to name. With several, the drafter takes the one whose origin is the finding's and whose path the finding's path begins with, the longest when more than one does, and leaves the question open when two tie or none fits.

**No two drafted reads share a name.** A read is named `{target}_{object}_{verb}`, the object being the path's last two plain words. The import, which sees every operation of the description, makes the names unique before any is drafted, and writes each read's object in its finding (`object`), so a rule that sees one finding at a time cannot disagree. Reads of one object and one verb (`list` for an array answer, `get` otherwise) that collide are told apart by the nearest path words that differ: each is named again from the shortest end of its path, placeholders included as `by_` and their name, that no other read's has, and each that still collides takes one more word. A read that collides with none keeps the name its words make, and the names are a function of the set of paths, never of the order they are read in. Paths no word tells apart, such as `/Orders` and `/orders`, take a short digest of the path itself. The file `operations/{object}_{verb}.yaml` follows the same object.

The import reads, for each operation, its parameters with their own examples and whether it needs a credential, and with no path named it looks for the description at `/openapi.json`, then `/api/openapi.json`. Not built: the OData key, and the rule for an endpoint seen in capture.

### Discoverers

| Discoverer | Reads | May send |
| --- | --- | --- |
| `http_description` | An OpenAPI 3.0 or 3.1 description, or a Swagger 2.0 one (read from its own `definitions`, response `schema`, parameter types and `in: body` parameter into the same operations; `basePath` is not read, as an OpenAPI 3 `servers` path is not: the surface address carries the prefix), as JSON or YAML, given as a checked path named `description` or fetched from the path the focus names under `description_path`, `/openapi.json` unless given. A reference inside the document is followed; one to another file is not. The text passes the redactor before it is parsed, whichever way it arrived, and reading it is bounded (below) | One GET |
| `http_surface` | Whether the base address answers, with which status, which methods it allows, and the names of the schemes it challenges with | One GET and one OPTIONS of the root |

**A description is read within bounds.** It is the system's own text, and a few kilobytes of it can name a schema that is millions of parts once its references are followed. The import follows a schema at most 32 levels deep, takes at most 20,000,000 steps (a schema part visited, a byte of a reference compared, an entry passed over while a reference is looked up), and writes at most 16,000,000 bytes of shapes, counted as 64 for each part plus the names and words copied into it. Past any of the three, or past the bytes the discovery may read, the whole import is refused as `input`, `description_too_large`: a part of a description is never read as if it were all of it. Each reference is looked up once. A schema that holds itself is found at once, by the references that are open, and has no shape in this pack's words: its body is left as an open question and the other operations are read. The JSON and YAML readers bound nesting at 64 levels, and the YAML reader refuses anchors and aliases, so a document cannot grow as it is parsed. The work is small enough, at these bounds, to be done where the discoverer runs.

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
pub mod codes;                                         // the pack's failure codes in one failure_codes! table (decision 0258), and PASSED_ON
```

Everything else is inside the crate: the driver, the template types, the JSON reader, the shape checker, the lowering, the discoverers.

What the kernel gives this crate, and what it does not:

- **Packs share data shapes, never code.** There is no kernel request-template type. This crate's template is a surface's name, a method, a path of literal segments and whole-segment holes, literal query entries and headers, the request and response shapes, the declared answers, the idempotency header, and the version rule. It holds no scheme, host, or port: the origin is the surface's address in the run's environment, taken apart once when the driver prepares.
- The engine reaches the driver through a handle that refuses a committing step without the ledger's intent token. Every failure says whether the request was not sent, was answered, or is unknown.
- **The lease is given to `prepare` only** ([decision 0097](../../decisions/0097-what-the-provider-contract-could-not-give-the-http-driver-and-what-the-pack-does-instead.md)). The driver applies it there into its own sink. The credentials are held by the prepared client, in the core's secret wrapper, for one pass; they go into a request only for an endpoint whose `auth` is `session` or `token`, and are wiped at release.
- **The address is checked where it is sent.** The driver puts the surface's origin, the path its address ends in, and the path and query that were built into one address, and sends it only when the URL type reads back that origin and exactly that path and query, and the path built starts at a `/`. A dot segment that would be folded away, plain or encoded, a separator that would start a query, a fragment, or a host, and a byte that would be encoded again all change what is read back, so a plan that held one, whatever the compiler said, is refused as `policy`, `surface_not_declared`, not sent. A header the connection sets is refused the same way when the request is built.
- A step's result has no place for what the driver did. The `request` step returns it as two captures: `acting`, in the form of the telemetry crate's `Acting`, and `exchange`, the request and the answer with headers passed through the allowlist.

### The steps

| Step | Arguments | Does | Fails as |
| --- | --- | --- | --- |
| `request` | `path.<placeholder>`, `query.<name>`, `body.<field>`; its payload lists the query names and the body fields with their JSON types and, for a write whose endpoint sends a version, the endpoint whose read supplies it | Builds one request from the endpoint the step names and sends it to the surface's address. Holds the answer. On a lookup, a status the endpoint declares as `absent` is the record being absent | `input` or `stale`, `version_not_read`, before sending; `transient` when nothing was sent or a read lost its answer; `possible_commit` for a write that was sent and has no answer that is the application's verdict; `session_expired`; for a read, `policy` for a redirect that leaves the surface and `unknown_variant`, `response_too_large` |
| `validate` | None | Judges the held answer: an `ok` status whose content type is one the answer's form takes and whose body, or the record read from it in another form, fits the response shape passes; otherwise the declared answer it matches names the failure | `drift`, `rejected`, `stale`, `transient`, `unknown_variant` |
| `extract` | None; its payload lists each output, its path in the body or the record, and its type | Takes one typed value for each output, exactly, through the redactor; remembers the record's version when the endpoint says where it is read | `drift` |
| `probe` | None; its payload is a safe method and a literal path | One request with no body and no credential; returns the status, the content type, the allowed methods, the names of the challenge schemes, and a successful body as text | As `request` |

### How the mode fills the loop

| Verb | Act | Confirm | Read back |
| --- | --- | --- | --- |
| `read`, `list`, `search` | `request`, `validate`, `extract` | | |
| The write verbs | `request`, marked as the commit point, and as idempotent when the endpoint names an idempotency header | `validate`; and `extract` of the save's own answer with `prefer: response` | `request`, `validate`, `extract` on the endpoint `read_back` names, each a lookup |

### A write that was sent

After the commit step's request may have reached the system, the driver returns a class the ledger takes as settled (`policy`, `input`, `stale`) only for the application's own verdict: `stale`, `version_conflict`, for an answer the endpoint declares as a conflict. Everything else leaves the record in doubt, and the read-back or reconcile decides ([decision 0139](../../decisions/0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md)).

| What came back | The driver reports | Sent |
| --- | --- | --- |
| No complete answer, or the time ran out | `possible_commit`, `no_confirmation` | Unknown |
| A redirect to another origin, however it is written: with a scheme, as `//host/path`, or with backslashes. It is not followed, and the address is not quoted | `possible_commit`, `no_confirmation` | Unknown |
| An answer over the limit on its size, which was not read | `possible_commit`, `no_confirmation` | Unknown |
| A redirect within the surface | A status like any other: accepted when the endpoint declares it under `ok`, and the read-back proves the write; otherwise `unknown_variant`, `unexpected_status`, from `validate` | Answered |
| A sign the session ended: the declared status, the provider's status, or a redirect to the sign-in host | `session_expired`, `needs_login`. The keeper is told, and the ledger keeps the record in doubt | Answered |
| A `validate` or `extract` step that cannot be carried out as the plan wrote it, while a write's answer is held | `possible_commit`, `no_confirmation`: the verdict was never read | Unknown |

For a read, a redirect that leaves the surface stays `policy`, `surface_not_declared`, and an answer over the limit stays `unknown_variant`, `response_too_large`. Where a redirect leads is read as a browser reads it, against the address of the request.

### The version precondition

An endpoint that declares `version: {send: ...}` writes with the record's version as a precondition. What a pass knows of a record's version is kept by the endpoint that read the record and the record's key, as one of two things: the version, or that the record was looked up and does not exist. The write's step names the endpoint whose read supplies the version, which is the one its binding reads the record back through; the lowering refuses a binding where that endpoint reads no version.

- The version stays until a request may have reached the system. An attempt that could not connect was not sent and leaves it, so the engine's retry of that attempt carries the same precondition.
- A record this pass looked up and found absent is written with no version: the write creates it. A precondition that the record still does not exist (`If-None-Match: *`) is not built.
- A write for a record of which the pass knows nothing is refused as `stale`, `version_not_read`, not sent: no read in this pass, a read whose answer held no version, a step for no record, or a version read through another endpoint. A version is never kept or taken under no key.
- A version that went out with a request is used up. The read-back after the write reads the new one.

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

### After the kernel's second pass (decisions 0161 and 0162)

- Every step input is built with `StepInput::new` and `for_record`. The `acting` capture is made with `Capture::acting`; its bytes are the ones written before.
- A refusal the endpoint declares under `responses.refused` carries the body's own words, the first of the top-level `message`, `detail`, and `error` that is text, through `StepFailure::with_said`. The failure's sentence never holds it.
- `Lowering::evidence` returns `acknowledged` for a `submit` whose binding names no `read_back`, and nothing else; the operation is then `irreversible`. Every other write keeps its read-back, or the save's answer under `prefer: response`.
- `delete` and `submit` are `irreversible`. An endpoint's effect that is an open question already counts as a write: `FileKind::writes` is true unless the effect is `read` and the method is safe.
- Not moved to expressions: the version precondition (`version: {send, read}` in the endpoint file), kept because moving it would change the file format. It could become `if_version: current.version` in the binding, with the endpoint's `version` kept only for where the version is read.
- Not used: `Prepared::recorded()`. No step of this pack needs what an earlier step recorded, because the read-back is by key.

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
| One pass's credentials, its held answer, and what it knows of each record's version, by the endpoint that read it and the record's key | The prepared client; dropped at release |
| Cookies and tokens beyond one pass | Never here. The keeper holds them |

## Failures

| Class | Codes | When | Sent |
| --- | --- | --- | --- |
| `rejected` | `app_validation_error` | An answer the endpoint declares under `refused` | Answered |
| `stale` | `version_conflict` | An answer the endpoint declares under `conflict`, with the version sent and the version the system now reports | Answered |
| `stale` | `version_not_read` | A write whose endpoint sends a version, for a record of which this pass read no version | Not sent |
| `session_expired` | `needs_login` | A status the session's provider takes for a refusal and the endpoint does not declare; a status and path the target declares; a redirect to a host the target declares. Also an endpoint that takes a credential in a run with no session, which is not sent | Answered, or not sent |
| `drift` | `response_schema_mismatch` | The body of an accepted JSON answer is not JSON, whatever shape the endpoint declares, `any` included, or does not fit the shape. Only an empty body is read as nothing. An answer whose content type is not one its form takes; an XML answer that is not well formed UTF-8, or where a required path matches nothing or a path for one value matches more than one place; a text that does not decode by its character set or does not match its pattern; a value that is not of its declared type. The message names the place or the field and never the value | Answered |
| `unknown_variant` | `document_type_refused` | An XML answer that declares a document type. Nothing of it is read | Answered |
| `unknown_variant` | `answer_holds_secret` | A `bytes` answer whose body holds a registered secret, in any form the redactor knows, read as UTF-8 or as UTF-16 of either byte order: neither its length nor its digest is returned ([decision 0249](../../decisions/0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md), [decision 0270](../../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)) | Answered |
| `unknown_variant` | `unexpected_status`, `response_too_large` | A status, or a body, that nothing declared matches, a redirect within the surface included; an answer to a read over the limit; an XML answer past its `max_depth` or `max_elements`, past 64 namespaces at once, with a namespace's address over 2,048 bytes, a name over 256, or an attribute's value over 65,536, or whose reading takes more work than its size allows; a text answer with a pattern whose text is over 1,048,576 bytes, refused before the match | Answered |
| `possible_commit` | `no_confirmation` | A write was sent, or may have been, and what came back is not the application's verdict: no complete answer, a redirect to another origin, an answer over the limit, or a judging step that cannot be carried out | Unknown |
| `policy` | `read_only_posture` | Outside a live run, a request that may write, by its method, by a method override it carries that names a method HTTP does not mark safe, or by its endpoint's `effect`, that is not the commit point of a write to a declared test record, nor a read the endpoint asserts with `read_asserted`, carrying no such override, in a published version's plan, against an environment of kind `test` in it; any such request in a discovery. The provider contract's code ([decision 0270](../../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)) | Not sent |
| `policy` | `product_header_refused` | The session's provider placed a header whose name begins `a2m-`, one of the product's own carriers between its parts; refused at prepare, and the header is never kept ([decision 0270](../../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)) | Not sent |
| `busy` | `reads_still_running` | Four reads of this surface's answers, or sixteen of the pack's, are still running after their steps stopped waiting for them; refused before a request is sent, or, if the count rose after it was sent, before its answer is read (a write's is then `possible_commit`) ([decision 0270](../../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)) | Not sent, answered |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable` | A read that timed out or lost its connection; an answer not read by the step's deadline (a write's is `possible_commit`); a connection that could not be made, for a read or a write; an answer declared under `unavailable` | Unknown, not sent, answered |
| `policy` | `surface_not_declared` | A surface address the run may not reach, refused when the driver prepares; an address built that is not the surface's origin and exactly the path built; for a read, a redirect to another origin | Not sent, answered |
| `policy` | `link_not_encrypted` | A pass with a session, whose plan has an endpoint on the surface that takes the credential, to a plain `http` address that is not a loopback address written plainly as an IP address (`localhost` is a name, not one; `::ffff:127.0.0.1` is not one), on a surface that does not declare `unencrypted_credentials: accepted`; refused at prepare, and again in `send` for each request that carries a credential | Not sent |
| `input` | `path_value_refused`, `step_not_usable`, `tls_not_enabled`, `auth_scheme_not_built`, `description_unreadable`, `description_too_large` | A path value that is empty, a dot segment, or holds a separator or a control character; a step the driver cannot carry out as written, a header the connection sets included; an HTTPS surface; a session whose scheme needs an exchange that is not built; a description that is not OpenAPI 3.0, OpenAPI 3.1, or Swagger 2.0; a description past the bounds of the import | Not sent |
| `unknown_variant` | `certificate_untrusted`, `certificate_name_mismatch`, `certificate_expired`, `certificate_not_verified`, `tls_handshake_failed` | An `https` surface whose certificate does not lead to a trusted authority, is for another name, or has expired, or is refused otherwise, or whose handshake fails. The session keeper's codes, given by `tls::refusal` of `a2m-auth`, declared in this pack's manifest | Not sent |
| `input` | `tls_setup_failed` | No TLS configuration could be made on this machine | Not sent |

The edge's answers are not built: `challenge_page`, `rate_limited_by_edge`, and `forbidden_by_edge` are not declared, and such an answer stops as `unexpected_status`.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No input can change the origin or the path's shape, and no plan can make the driver send anywhere but the surface's origin and the path built | `no_input_changes_where_a_request_goes`, against the test application; `no_value_can_change_the_shape_of_the_path`; `the_address_sent_to_is_the_surfaces_origin_and_exactly_the_path_that_was_built`; `a_header_the_connection_sets_is_refused_where_it_would_be_sent` |
| A write whose answer is lost is `possible_commit`, and the test application shows exactly one write | `a_write_whose_answer_is_lost_is_a_possible_commit_and_is_not_sent_again` |
| A write that was sent is never reported in a settled class unless the application gave its verdict, and the test application shows the write stored once | `a_write_whose_answer_is_not_the_applications_verdict_is_in_doubt_and_never_settled`, with the fault "redirect after commit" |
| A write's version precondition survives an attempt that was not sent, and a write with no version read is not sent | `a_write_that_was_not_sent_keeps_its_version_and_one_with_no_version_read_is_refused`, with the other caller's change still stored |
| A property a description does not require is drafted `nullable` in an answer's shape, and the check of an answer is unchanged ([decision 0297](../../decisions/0297-a-property-a-description-does-not-require-is-drafted-as-possibly-null.md)) | `a_property_the_description_does_not_require_is_drafted_as_possibly_null`, `a_drafted_shape_takes_null_and_missing_for_an_optional_property_and_not_a_wrong_kind`, `a_read_drafted_from_a_description_takes_null_and_missing_for_an_optional_property` |
| Reading a description is bounded, and past the bounds it is refused by name | `a_description_that_grows_past_the_bounds_is_refused_by_name_and_not_expanded` |
| A write reaches the system only with the ledger's token | `a_write_without_the_ledgers_token_never_reaches_the_application` |
| No discovery sends a request that could write | `discovery_observes_and_cannot_write`, with the control interface counting writes |
| A drafted read exists only for a method the description marks safe, and no drafted entry describes a write | `a_published_description_becomes_claims_and_a_read_is_drafted_only_for_a_safe_method`, where the drafter refuses the DELETE; `an_entry_describes_a_write_unless_it_is_a_read_of_a_safe_method` |
| A write never compiles as a read or as less than its verb does | `a_write_never_compiles_as_less_than_what_its_binding_does` |
| A credential never appears in a value, a capture, or a failure, and reaches only an endpoint that takes it | `a_credential_reaches_only_the_request_it_is_for_and_appears_nowhere_else`, with the token planted in a response header and body |
| A registered secret in a description reaches no finding, whether the description was fetched or given as a file | `a_description_given_as_a_file_passes_the_redactor_before_anything_is_made_of_it` |
| A held answer prints no header and no body | `a_held_answer_prints_no_header_and_no_body` |
| The same target compiles to the same bytes | `the_fixture_targets_compile_twice_to_the_same_bytes` |
| Over HTTPS, with the feature `tls`, a read and a verified write work against a named root; an untrusted authority, a wrong name, and an expired certificate are each named and nothing is sent; a write lost after it was sent is in doubt; a redirect off TLS is not followed | `tests/tls.rs`: `a_read_and_a_verified_write_over_https_trust_the_named_authority`, `each_certificate_made_wrong_is_named_and_nothing_is_sent`, `a_write_lost_after_it_was_sent_over_https_is_in_doubt`, `a_redirect_from_https_to_plain_http_is_not_followed` |
| An answer that is not JSON is read into exactly its declared record, typed as JSON values of those types are, against the test application, and a JSON write reads back through an XML view of the record the application stored | `tests/forms.rs`: `each_form_is_read_end_to_end_and_gives_what_the_application_stores`, `a_json_write_is_read_back_through_its_xml_view_and_the_application_stored_it` |
| A path that matches nothing where required, or more than once for one value; a wrong content type; a text that does not decode; a document type; a bound passed: each stops the read by name and quotes no value | `a_path_that_matches_nothing_or_more_than_once_is_the_response_shape_failure`, `an_answer_of_another_content_type_or_that_does_not_decode_is_the_response_shape_failure`, `an_answer_that_declares_a_document_type_is_refused_before_anything_is_read`, `the_bounds_on_an_xml_answer_are_named`; the reader's own rules in `xml.rs` and `forms.rs` |
| A registered secret in an XML or a text answer comes out redacted | `a_registered_secret_in_an_xml_or_a_text_answer_comes_out_redacted` |
| A hostile answer holds no engine thread and is refused within a fixed count of work: the audit's 4 MB namespace, a long name, a long value, many namespaces, many attributes; JSON with many names | `xml.rs`: `a_namespace_address_past_its_bound_is_refused_at_once`, `names_are_compared_with_the_declared_ones_and_never_copied`, `a_long_name_a_long_value_and_many_namespaces_are_each_refused_by_name`, `many_attributes_cost_work_in_proportion_to_their_number`, `a_read_stops_past_its_budget_and_once_its_flag_is_set`; `json.rs`: `many_names_are_read_in_time_that_grows_with_their_number`; `driver.rs`: `work_that_outlasts_the_deadline_is_left_and_told_to_stop`; `tests/answers.rs`: `an_answer_whose_namespace_address_is_past_its_bound_is_refused_in_time` |
| Outside a live run a request that may write reaches the system only as a test record's commit point or as an asserted read of a published version against a `test` environment; a method override is judged as the method it names | `tests/answers.rs`: `a_request_that_may_write_is_sent_only_where_the_posture_lets_it`, `a_method_override_is_judged_as_the_method_it_names`, `a_write_to_a_declared_test_record_is_sent_and_no_other_outside_a_live_run`, asking the test application what it heard and stored; `driver.rs`: `a_request_is_judged_by_its_method_and_every_override_in_it`; `tests/compile.rs`: `a_read_whose_method_override_names_a_method_that_may_write_is_refused` |
| Reads left running past their deadline are bounded, for the surface and the pack, and a read past the ceiling is refused before its work; a pattern and the text it is matched against are bounded before the match | `driver.rs`: `reads_left_past_their_deadline_are_bounded_and_the_next_is_refused_busy`, `a_read_whose_waiting_is_dropped_is_counted_until_it_ends`; `forms.rs`: `a_text_longer_than_a_pattern_is_matched_over_is_refused_before_the_match`; `tests/compile.rs`: `a_pattern_longer_than_its_bound_is_refused` |
| A header of the product's own, `a2m-` in any case, is never sent | `tests/session.rs`: `a_header_of_the_products_own_is_never_sent`, asking the service what it heard |
| Two attributes for one path, a disagreement about the character set, and CDATA or a reference outside the root each stop the read; a value of any form with a control character other than a tab or a line break stops it; a body with a registered secret gives no digest | `xml.rs`, `forms.rs` unit tests; `tests/answers.rs`: `a_value_is_bounded_data_and_a_control_character_in_any_form_stops`, `a_body_that_holds_a_registered_secret_is_not_confirmable_through_its_digest` |
| A JSON endpoint that declares nothing of its answer compiles as before | `the_forms_compile_through_the_public_entry_twice_to_the_same_bytes_and_json_is_unchanged` |
| Overhead stays under 5 ms beyond the application's time | Not measured yet |

## Build order

1. Done: the `endpoints/` schema and its compilation; the `http` driver's reads against case 1; writes with read-back and the version precondition, cases 2 to 4; the lost answer, case 17.
2. Done: credentials through the lease for `api_key`, `bearer_static`, and `http_basic`; sign-in signals; limits on size and time; redirects refused; HTTPS refused by name in a build without the feature `tls`.
3. Done: the OpenAPI import, `http_description`; `http_surface`; the drafting rule for endpoint entries.
4. Not built: the anti-forgery bootstrap and the `token` step, case 5; the edge's answers, case 23; cases 16 and 18 to 22 and 24, which the test application does not serve yet.
5. Done: the read operation drafted for each GET a description states, with the description's own example ([decision 0179](../../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)), proven against the test application by `the_test_applications_description_drafts_reads_that_read_what_it_stores`. Not built: the `api` mode; GraphQL and OData; the drafting rule for endpoints seen in capture, package E4.
6. Not built: uploads and downloads (an answer of the form `bytes` gives its length and digest, and saving it to a file is the downloads package), request bodies that are neither JSON nor a form, pagers, long-running jobs, identity by a stamp or a search, the digest and integrated Windows exchanges, a client certificate.
7. Done: the eight findings of the safety review, and three more found on the way ([decision 0139](../../decisions/0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md)): a write that was sent is never settled by an answer that is not the application's verdict; the version precondition; the bounds on reading a description; the address checked where it is sent.

8. Done: the declarations the compiler holds a lowering to ([decision 0153](../../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)): which step kinds save, the least effect of a binding, and which endpoint entries describe a write. The driver refuses a write endpoint from any step not marked `commits`, in every posture; and, since [decision 0249](../../decisions/0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md), keeps the posture and applies its own read-only guard outside a live run.
9. Done, behind the feature `tls`: `https` surfaces, through the module `tls` of `a2m-auth`, with the test application served over TLS for the tests ([decision 0157](../../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)). A client for an `https` origin is made with `https_only`. Discovery takes an `https` surface in such a build.
10. Done: no credential crosses plain `http` to another machine unless the surface accepts it, in `src/link.rs`: the refusal at prepare and in `send`, the surface field, the lint rule `http.unencrypted_credentials` (a warning in every mode), and the `link` detail in the `acting` evidence of each request that carried a credential over an accepted link ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)). Proven against the test application's bearer routes by `a_credential_crosses_an_unencrypted_link_to_another_machine_only_where_the_surface_accepts_it`, in `tests/link.rs`. The surface kind registers the notice `SurfaceNotice::UnencryptedCredentials` for that field (`with_notice`), so a door that asks `CompilerSet::surface_notices` shows its fixed words ([decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md); `a_surface_that_accepts_an_unencrypted_link_carries_its_notice_and_no_other_does`).

11. Done: answers that are not JSON ([decision 0217](../../decisions/0217-the-http-pack-reads-answers-that-are-not-json.md)), in `src/forms.rs` and `src/xml.rs`: the `answer` of an endpoint, its content-type check, the XML reader by declared paths, the text form with its character set and anchored pattern, the bytes form, and outputs of the type `date` taken from text for every form. Proven against the test application's views by `tests/forms.rs`. Not built: drafting `answer` from an OpenAPI operation whose answer is `application/xml` or `text/plain`, which needs the `described_operation` finding to carry the media type (such an operation is drafted, as before, with its `response_schema` an open question); an `each` inside an `each`; the text of a repeated element beside its attributes; outputs of the type `moment`, for any form.

12. Done: `link::plain_to_another_machine` asks `ScopedHost::may_leave_this_machine` of `a2m-core` of the host the URL parser writes, the one question every check of a plain link asks; pinned over the shared corpus of address spellings, with its old form kept to show nothing it refused is allowed now ([decision 0240](../../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)).

13. Done: the safety audit's findings S1, S2, and M1 to M5 ([decision 0249](../../decisions/0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md)): names compared without copying and bounded, every answer read on a blocking thread under the step's deadline, the XML read bounded by a count of work, JSON objects read in linear time; the driver's read-only guard and `read_asserted`, with `http.effect_method` an error; two attributes for one path, the character set agreed on, nothing outside the root element, control characters refused in values; a `bytes` body checked by the redactor. Not built: a keyed digest of a `bytes` body, which needs a purpose in `a2m-core` and a keyed digest that reaches a driver; a read binding that sends a body.

14. Done: the second reading's findings C1 to C4, and the HTTP half of A1 ([decision 0270](../../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)): an asserted read is sent outside a live run only from a published version's plan, told by its version, against an environment of kind `test` in it; a method override in a header or the query is judged as the method it names, at compile time and in the driver; reads left running past their deadline are counted, four for a surface and sixteen for the pack, and a read past either is `busy`; a pattern is at most 4,096 bytes and 256 KiB compiled, and is matched against at most 1,048,576 bytes; a `bytes` body is also read as UTF-16; a header whose name begins `a2m-` is refused at prepare. Not built: a mark on the prepare context that the plan is a published version's, which would replace the version as the signal; a match that can be stopped midway; a bound on the blocking pool shared with the other packs.

15. Done ([decision 0264](../../decisions/0264-the-publish-page-renders-the-typed-review-sign-ins-first-from-one-keeper-call.md)): `Lowering::statement` for the review summary: a binding starts nothing, sets nothing, and sends to its surface's address and no other, since an endpoint's path is relative to the surface, the driver refuses any other address, follows no redirect, and uses no proxy. Test: `the_review_states_what_the_http_pack_runs_and_where_it_sends` (`tests/compile.rs`).

## Open points

- How the schema of `endpoints/` and of the `endpoint` finding is versioned, so that `a2m-pack-web-ui` and `a2m-pack-web-probe` are held to the version they read or write by a test. The payload carries the format number 1, and each finding's `found` carries `format`.
- Two packs compile the same entries into two payloads. A test that both build the same request from the same entry would catch drift between them.
- The `endpoint` finding's `found` is what `http_surface` wrote first: a method, a path, a status, a content type, the allowed methods, and the challenge schemes. The probe of the web pack needs template tiers in it, and will widen it.
- Whether the lease should reach `act`, so that a credential is placed for each request and the provider is asked about each sign as it is seen (decision 0097).
- The read-back of a `delete`, where absent is the expected answer, is the engine's to judge; the pack reports it as absent.
- A pass that is replaced between the read of a record and its write knows no version, and the write is refused as `stale`, `version_not_read`. The engine takes a record as fresh by time alone; it should read the record again when the pass that read it is gone.
- A record that was absent is created with no precondition. `If-None-Match: *`, or the system's own form of "only if it does not exist", is not designed.
- A sign that the session ended, on a write that was sent, is left as `session_expired`, so that the keeper is told. That is safe only while the ledger does not take `session_expired` as settled; a test in the ledger should hold that.
- gRPC reflection, which spec section 201 lists under `api`. Not in the first release; it needs its own stand.
- Settled in part: the TLS stack is rustls with ring, the platform's verifier, and bundled roots as the fallback, every licence on the list ([decision 0157](../../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)). Run on Linux; the job with the feature on must still pass on Windows and macOS.
- Whether the `api` mode needs to be a mode at all, or is `http` with a different source of templates. Hypothesis H-19 asks the same of `page_http`. The import built here drafts ordinary endpoint entries, which is evidence for the second.
- The overhead of decoding the plan's endpoints at every `prepare`, to be measured against the 5 ms budget.
