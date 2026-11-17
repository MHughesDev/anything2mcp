# Technical reference

This file holds the engineering detail behind the [system design](../spec/README.md): interfaces, data model, protocols, and catalogs. Everything here is a starting design to be refined in the phase that builds it.

## Crate layering

Crates depend only downward, which keeps build times short and the core small. A crate may depend only on the layers its row names, never on a crate in its own layer. `tools/docs_audit.py` reads this table, compares it with the crates on disk, and refuses any other edge. The reasoning behind each layer, and which crate defines each interface, are in [the architecture](../architecture/README.md) ([decision 0061](../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md)).

Layers 1 to 8 are the kernel. Layers 9 and 10 are the packs ([spec section 202](../spec/2-kernel/202-kernel-and-packs.md)).

| Layer | Crates | May depend on |
| --- | --- | --- |
| 1. Types | `a2m-core` | Nothing internal |
| 2. Foundations | `a2m-expr`, `a2m-vault`, `a2m-telemetry` | Layer 1 |
| 3. Stores | `a2m-files`, `a2m-ledger` | Layers 1 to 2 |
| 4. Services | `a2m-policy`, `a2m-auth`, `a2m-sources` | Layers 1 to 3 |
| 5. Contract | `a2m-provider` | Layers 1 to 4 |
| 6. Engine | `a2m-engine`, `a2m-sched` | Layers 1 to 5 |
| 7. Actions | `a2m-actions` | Layers 1 to 6 |
| 8. Roles | `a2m-runner`, `a2m-mcp`, `a2m-api`, `a2m-local` | Layers 1 to 7 |
| 9. Pack bases | `a2m-pack-web-browser` | Layers 1 to 5 |
| 10. Packs | `a2m-pack-web-ui`, `a2m-pack-web-probe`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-desktop`, `a2m-pack-command`, `a2m-pack-data`, `a2m-pack-file`, `a2m-pack-stream`, `a2m-pack-device` | Layers 1 to 5, and the base of its own pack |
| 11. Product | `anything2mcp` | Everything |

Three rules follow from the table, and the audit states each one by name when it is broken:

- **A kernel crate never depends on a pack crate.** The engine reaches a driver through the registry in `a2m-provider`, never by naming a pack.
- **A pack crate sees the kernel up to the contract, and nothing above it.** It cannot call the engine, the scheduler, the action registry, or a front door. It registers what it offers and is called.
- **A pack crate depends on another pack crate only when that crate is the base of its own pack.** The pack is the third word of the name: `a2m-pack-web-ui` may use `a2m-pack-web-browser`, and `a2m-pack-http` may not.

Only the product crate names packs, and it does nothing else with them than hand them to the registry.

## Core types

The crate `a2m-core` holds the words every other crate shares. A word with a fixed set of values is an enum whose names are the ones in the documents; a test in the crate reads the documents and fails when the two differ, in content or in order. Each enum reads and writes its values by name, in files, in the ledger, and in JSON.

| Type | Values | Defined in |
| --- | --- | --- |
| `Archetype` | Seven, fixed | [Spec section 201](../spec/2-kernel/201-archetypes-and-modes.md) |
| `EffectClass` | Three, ordered from least to most serious | [Spec section 212](../spec/2-kernel/212-policy-and-approvals.md) |
| `Hazard` | Five, ordered from least to most serious | [Spec section 213](../spec/2-kernel/213-effects-hazards-and-consent.md) |
| `FailureClass` | Twelve | "Failure codes" below, and [spec section 207](../spec/2-kernel/207-the-engine-loop.md) |
| `EvidenceLevel` | Seven, ordered as the ladder; the default is the weakest | [Spec section 219](../spec/2-kernel/219-the-evidence-ladder.md) |
| `Coverage` | Three; the default is `unknown` | [Spec section 218](../spec/2-kernel/218-discovery-and-findings.md) |
| `RunState`, `RecordState` | Twelve and six | [Spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md) |
| `SessionState` | Eight | [Spec section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md) |
| `BindingStatus` | Four | [File formats](file-formats.md) |
| `Source` | Five; each has the strongest level a finding from it can have | [Spec section 218](../spec/2-kernel/218-discovery-and-findings.md) |
| `LimitationCode` | Eight | [Spec section 218](../spec/2-kernel/218-discovery-and-findings.md) |
| `UnknownReason` | Four | [Spec section 218](../spec/2-kernel/218-discovery-and-findings.md) |
| `Availability` | Eight; the default is `unknown`, which is never `ready` | [Spec section 203](../spec/2-kernel/203-the-provider-contract.md) |
| `SubsystemState`, `SubsystemMove` | Four and three | [Spec section 204](../spec/2-kernel/204-supervising-subsystems.md) |
| `DoctorState` | Seven; the default is `unknown` | [Spec section 204](../spec/2-kernel/204-supervising-subsystems.md) |
| `IdentityStrategy` | Five | [Spec section 209](../spec/2-kernel/209-record-identity-and-dependent-records.md) |
| `FieldBase`, `Sensitivity`, `EnvironmentKind`, `TestKind`, `Checkpoint` | Ten, three in order, two, two, two | [File formats](file-formats.md) |
| `Rounding` | Seven: `half_up`, `half_even`, `half_down`, `up`, `down`, `ceiling`, `floor` | [Decision 0068](../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md) |
| `DigestPurpose` | Three: `blob_name`, `restricted_value`, `record_key` | [Decision 0069](../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md) |

Some words are checked names and not enums, because their values arrive with packs, crates, and target files ([decision 0060](../decisions/0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md), [decision 0062](../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)):

| Type | Shape | Who supplies the values |
| --- | --- | --- |
| `Mode` | Lowercase letters, digits, and underscores, starting with a letter; at most 32 characters | A provider registers each mode it brings, with its archetype |
| `FailureCode` | The same shape; at most 64 characters | Each crate names the codes it can produce; a code never changes once released |
| `SurfaceKind`, `StepKind`, `Verb`, `FileKindName`, `FindingKind`, `ControlName` | The same shape; at most 32 characters | A provider registers each one it brings. A step kind is a name within its mode |
| `LintCode` | An owner and a rule, each of that shape, joined by one dot; at most 64 characters | `kernel`, or the provider that registers the rule |
| `TargetName`, `SurfaceName`, `Environment` | The same shape; at most 32 characters | The target's files |
| `OperationName`, `AccountName` | The same shape; at most 64 characters | The target's files, and the settings. The compiler checks the parts of a tool name |

These complete the crate ([decision 0068](../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md), [decision 0069](../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md)); the [crate's design](../architecture/crates/a2m-core.md) has every signature:

- **`Failure`** is a class, a code, a plain sentence, and a next step. The sentence and the next step are shown to callers and must never contain a secret. A name that is not one of a vocabulary's values, or is badly formed, becomes a failure of class `input` with the code `schema_invalid`, and its next step lists the values that would have been accepted.
- **`Finding`** is what discovery returns. It is made only through its builder, which refuses a level above `discovered`, above what the source supports, or above the weakest finding it was derived from. What is not stated takes the weakest value. Its identifier is a `Digest` of its content, so the same observation has the same identifier everywhere, and a stored finding is checked against its identifier when it is read. Its `Subject` is one of six kinds; an address subject carries no credentials, query, or fragment.
- **`Secret`** wraps a value that must never be shown. Printing it gives `[redacted]`. It cannot be serialized or copied, and the build fails if it ever can. The value is reached only through a method named `expose`, so that every use can be found.
- **`Value`** is what a field holds: nothing, true or false, a whole number, text, a decimal, a moment, a date, a length of time, a list, or a record. It is never a float and never a secret, and kinds are never converted. **`Values`** holds named values sorted by name, so the same values have the same bytes and the same digest whoever built them. **`ValueType`** describes a value's type, for the expression checker and the plan alike. **`Decimal`** is exact to 28 significant digits: a result that does not fit is refused, never rounded, and a division takes places and a rounding mode. **`RecordKey`** is one or more plain values in declared order.
- **Identifiers** are 16 bytes, ordered by the moment they were made, written as a one-letter prefix, an underscore, and 32 hexadecimal characters: `RunId` (`r_`), `JobId`, `SessionId`, `ApprovalId`, `DecisionId`, `WorkspaceId`, `PrincipalId`, `RunnerId`. The crate reads no clock and no random source; whoever makes one brings both. **`Identity`** is a person or a named account, never a credential.
- **`Plan`** is the compiled plan as data, with `encode`, `decode`, `check`, and `show`. It is encoded as canonical JSON after one line that names the format; the hash is the digest of all the bytes, and `decode` checks it before it reads anything. A pack's own data is a `PackPayload`, held as canonical JSON text that the kernel never parses.
- **`Redact`** and **`KeyedDigest`** are the two interfaces the vault implements. `Redact` turns text into text with every registered secret removed, and redacts a whole failure or a set of values in one call. A keyed digest is separate for each workspace and each purpose, and has one construction, HMAC-SHA-256.

## Driver interface

The driver interface, the manifest, the registry, the supervisor, and the plug-in protocol are designed in [`a2m-provider`](../architecture/crates/a2m-provider.md), and that design is the reference until slice 1.1 builds the interface and settles it by decision record. In brief:

| Call | Does |
| --- | --- |
| `prepare` | Acquires what the steps need: a page, a client, a connection, a link |
| `act` | Carries out one compiled step. Confirming and reading back are steps like any other, so there is no separate read-back call |
| `health` | A cheap check used by canaries and by the choice of binding |
| `release` | Gives back what `prepare` acquired |

Rules for every driver:

- No retries inside `act`. The engine decides what is tried again.
- Every failure maps to exactly one failure class, and says whether the request was not sent, was answered, or is unknown.
- No secret ever appears in a step's outcome or in a failure.
- A driver is reached only through a handle that refuses a step that commits without the token the ledger gives for a recorded intent.
- A step marked as a lookup may report that a record is absent. No other step may.
- A step's outcome can carry values known before the commit point, such as a key the store assigned. The engine records them before the committing step runs.

## Verbs by mode

| Verb | `ui` | `page_http` | `http` | `api` | `site_tools` | `mcp` | `desktop` |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `open` | Yes | No | No | No | No | No | Yes |
| `read` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `list`, `search` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `create`, `update`, `upsert` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `delete` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `upload` | Yes | Yes | Yes | Yes | No | No | Yes |
| `download` | Yes | Yes | Yes | Yes | No | If offered | No |
| `submit` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `call` | No | Yes | Yes | Yes | Yes | Yes | No |

The columns are the modes of the first four packs. Every further pack adds a column in its own design: the command, data, file, stream, and device packs each state the verbs they offer and how each lowers into steps.

## Widget drivers and platform profiles

Both are the web pack's knowledge and live in its section: [widget drivers](../spec/3-packs/301-the-web-pack.md#widget-drivers) and [platform profiles](../spec/3-packs/301-the-web-pack.md#platform-profiles).

## Auth provider interface

Each provider implements five operations: `sign_in`, `is_signed_in`, `detect_expiry`, `refresh`, `sign_out`, plus `inject`, which places credentials into a driver's context without returning them. Only the session keeper calls a provider. A provider also states what it needs and can do (a person, a silent renewal, a light prompt, state that can move, and whether a successful exchange confirms the credential) and checks an `auth.yaml` entry without signing in. The traits as built are in the [crate design](../architecture/crates/a2m-auth.md).

A provider lives in `a2m-auth` when it only holds or exchanges a credential. It lives in a pack when it needs a surface of the system. A provider that needs a person's consent in a browser, and is not in the web pack, gives the person a link to open in their own browser; nothing drives that browser.

| Provider | Covers | Lives in |
| --- | --- | --- |
| `sso_oidc`, `sso_saml` | Entra ID, Okta, Google Workspace, ADFS, Ping | `a2m-pack-web-ui` |
| `form_login` | Username and password pages | `a2m-pack-web-ui` |
| `os_broker` | Windows Web Account Manager, macOS Enterprise SSO, Linux identity broker; user devices only | `a2m-auth` |
| `obo_token` | OAuth On-Behalf-Of exchange from the caller's token | `a2m-auth` |
| `http_basic`, `http_digest` | Simple intranet systems. The provider holds the credential; the HTTP driver carries out the scheme | `a2m-auth` |
| `windows_integrated` | NTLM and Kerberos, in the same division of work | `a2m-auth` |
| `client_certificate` | Mutual TLS | `a2m-auth` |
| `api_key`, `bearer_static` | Interfaces with fixed keys | `a2m-auth` |
| `oauth_client_credentials`, `oauth_authorization_code` | Interfaces with OAuth | `a2m-auth` |
| `upstream_mcp` | Authorization to a system's own MCP server | `a2m-pack-mcp` |
| `totp` | One-time codes for dedicated accounts, where IT allows. A function another provider calls, not a provider of sessions | `a2m-auth` |

Further packs bring their own where a credential is bound to their kind of connection: the data pack brings a database password provider.

## Vault interface

The design of [`a2m-vault`](../architecture/crates/a2m-vault.md) gives the interface in full. The main types:

- **`Vault`** seals a plain item into a **`Sealed`** envelope under a **`SealContext`**, and opens it again only under the same context. The caller stores the envelope.
- **`KeyStore`** is the back end that holds the root key. `OsKeyStore`, `PassphraseKeyStore`, and `MemoryKeyStore` implement it.
- **`KeyRecords`** is where wrapped keys persist, as one **`KeyRecord`** per key and **`Generation`**. `a2m-ledger` implements it.
- **`Redactor`** implements `Redact`, and **`VaultDigest`** implements `KeyedDigest`, both from `a2m-core`.

## Telemetry interface

The design of [`a2m-telemetry`](../architecture/crates/a2m-telemetry.md) gives the interface in full. The main types:

- **`TraceBuilder`** takes one **`StepRecord`** per step, redacts it, and ends as a **`FinishedTrace`**: the bytes and the trace signature. A trace that still holds a secret ends as a **`TraceRefused`**.
- **`Telemetry`**, made by `start` from **`TelemetrySettings`**, is the handle for spans, metrics, and the engine's own logs.
- **`Clock`** is how time is given, so that a test can fix it.

## Data model

The same schema runs on SQLite and Postgres. Column-level definitions for the run, ledger, scheduling, and evidence tables are in the [ledger schema](ledger-schema.md).

| Table | Holds | Key columns |
| --- | --- | --- |
| `workspaces` | Isolation boundaries | id, name |
| `targets`, `target_versions` | Targets and their immutable published versions | target, version, hash, signature, compiled plan, status |
| `bindings_health` | Health of each binding | operation, mode, status, last check, shadow match counts |
| `toolsets`, `toolset_items` | Named tool selections | toolset, version, operation or pipeline, identity mode, approval policy |
| `principals`, `grants` | Callers and what they may do | principal, scope pattern, max effect, role |
| `policy_versions`, `decisions` | Published policy sets; every decision with the rules that made it | version, hash, tests passed; request, answer, determining rules |
| `sessions` | Signed-in states | identity, target, runner affinity, expiry, vault reference |
| `runs` | One row per run | run id, caller, tool version, input hash, state, mode, timings |
| `run_steps`, `run_records` | Pipeline steps and the ledger, one row per commit point | run id, record key, state, intended and observed values, attempts, evidence reference |
| `run_events` | Append-only history of every run | run id, sequence, lease number, kind |
| `record_locks` | Which run may write a record | target, environment, record key, holder, held or quarantined |
| `blobs`, `outbox` | Evidence stored by content hash; messages owed to the outside | hash, kind, retention; destination, attempts |
| `approvals` | Approval records | run id, diff hash, approver, channel, expiry, used flag |
| `jobs`, `runners` | Scheduling | job, lease, runner, capabilities, heartbeat |
| `audit` | Append-only, hash-chained log | sequence, previous hash, actor, action, subject, hash |

## Runner protocol

Runners open every connection themselves, over mutually authenticated HTTPS; `lease` is the reply to a long-polled `claim`. The full design, including leases, lost-lease handling, and secrets, is in spec section 223.

| Message | Direction | Purpose |
| --- | --- | --- |
| `hello` | Runner to control plane | Version, operating system, modes, targets, identities it can serve |
| `lease` | Control plane to runner | A job, with a deadline; the runner must accept or decline |
| `event` | Runner to control plane | Step started, intent recorded, acted, verified, failed |
| `need` | Runner to control plane | A session secret, an approval check, or a rate-budget grant |
| `heartbeat` | Both | Liveness; a missed heartbeat ends the lease |
| `result` | Runner to control plane | Final outcome and evidence references |
| `cancel`, `drain` | Control plane to runner | Stop a job; stop taking jobs |

Delivery is at-least-once. A job whose lease ends without a result is treated as `possible_commit` and reconciled before it is given to another runner.

## Failure codes

Every failure carries a class, a stable code, a plain sentence, and a next step. The classes are closed. The codes below are examples: each crate names the codes it can produce, and its design in [the architecture](../architecture/README.md) lists them.

| Class | Example codes |
| --- | --- |
| `policy` | `not_granted`, `identity_not_allowed`, `effect_too_high`, `approval_missing`, `approval_stale`, `approval_refused`, `limit_exceeded`, `kill_switch_on`, `policy_evaluation_failed`, `presence_required`, `surface_not_declared` |
| `input` | `lint_failed`, `policy_tests_failed`, `decision_not_found`, `path_not_allowed`, `file_type_mismatch`, `schema_invalid`, `source_template_unknown`, `value_out_of_range`, `expression_invalid`, `expression_type_error`, `value_unparsable`, `value_absent`, `expression_cost_exceeded`, `plan_expression_unloadable`, `trace_holds_secret`, `telemetry_settings_invalid`, `vault_passphrase_mismatch` |
| `session_expired` | `needs_login`, `needs_mfa`, `device_required` |
| `edge_blocked` | `challenge_page`, `rate_limited_by_edge`, `forbidden_by_edge` |
| `drift` | `locator_not_found`, `fingerprint_mismatch`, `response_schema_mismatch`, `upstream_tool_changed` |
| `unknown_variant` | `unexpected_dialog`, `unexpected_page`, `unexpected_status` |
| `rejected` | `app_validation_error` |
| `mismatch` | `read_back_differs` |
| `possible_commit` | `no_confirmation`, `runner_lost`, `ambiguous_match` |
| `stale` | `changed_since_preview`, `version_conflict` |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable` |
| `busy` | `queue_full`, `record_locked`, `session_in_use`, `runner_unavailable`, `storage_full`, `engine_degraded`, `randomness_unavailable` |

## Command line

Every action in the [action catalog](action-catalog.md) is a command of the form `anything2mcp <noun> <verb>`, with a matching MCP tool, REST route, and SDK method. The nouns are `tool`, `run`, `session`, `target`, `auth`, `screen`, `capture`, `endpoints`, `spec`, `draft`, `test`, `fixture`, `shadow`, `plan`, `version`, `publish`, `binding`, `toolset`, `grant`, `policy`, `runner`, `killswitch`, `audit`, `secret`, `vault`, `delegation`, `engine`, `config`, `doctor`, `browser`, `plugin`, `client`, `trace`, `action`, `docs`, `self`, `provider`, `component`, `discover`, `finding`, `unknown`, and `scout`.

| Kind | Commands |
| --- | --- |
| Shortcuts | `init`, `add`, `learn`, `lint`, `try`, `test`, `publish`, `status`, `call`, `run`, `diff`, `bundle`, `rollback`, `migrate`, `connect`, `doctor`, `docs` |
| Process commands | `serve`, `runner`, `bridge` |

On a local machine no command approves a write; see spec section 222. On a server, approvals are given in the console or a chat card after the approver signs in.

### Command output contract

| Item | Rule |
| --- | --- |
| Machine output | One JSON object on standard output with `--json` or when output is not a terminal |
| Success | `{"ok": true, "action", "result", "warnings", "next"}` |
| Failure | `{"ok": false, "error": {"code", "class", "message", "fix"}}` |
| Exit codes | 0 success; 1 action failed; 2 malformed command; 3 a person is needed; 4 engine unreachable |
| Large results | Written under `.anything2mcp/out/`; the result carries path, size, and summary |
| Prompts | None when output is not a terminal |

## Local channel

| Item | Value |
| --- | --- |
| Transport | Unix domain socket (macOS, Linux); named pipe (Windows) |
| Access | Per-user directory or access list, plus a check of the connecting user on every connection |
| Protocol | HTTP carrying the `/api/v1` REST API and the MCP endpoint |
| Start | On demand, guarded by a lock file; `--in-process` for single-command use |
| Stop | After 30 minutes idle by default; session state saved to the vault |
| Version skew | A newer client asks an older engine to drain and restart |

## On-disk layout

Each operating system's own conventions are followed. `A2M_HOME` overrides all of them with one directory. Exact paths are confirmed when the paths module is written in Phase 1.

| What | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Settings | Roaming application data | Application Support | The user's configuration directory |
| Store, blobs, browser profiles, logs | Local application data | Application Support | The user's data directory |
| Local channel and lock file | A named pipe carrying the user's identifier | A per-user runtime directory | The user's runtime directory |
| Pinned browsers and caches | Local application data | Caches | The user's cache directory |

Inside the data directory: `store/` for the database, `blobs/` for evidence, `profiles/<identity>/` for one browser data directory per identity, `journal/` for the runner's local journal, and `logs/`. A project folder holds only target files and working output, never any of these.

## REST endpoints

| Method and path | Purpose |
| --- | --- |
| `GET /api/v1/tools` | List operations and pipelines the caller may use |
| `POST /api/v1/operations/{name}/calls` | Call an operation |
| `POST /api/v1/pipelines/{name}/runs` | Start a pipeline |
| `GET /api/v1/runs/{id}` | Run status and result |
| `POST /api/v1/runs/{id}/resume`, `/cancel` | Resume or cancel |
| `GET /api/v1/runs/{id}/events` | Server-sent event stream |
| `GET /api/v1/sessions` | Session status for the caller |
| `POST /api/v1/actions/{noun}.{verb}` | Run an authoring or admin action from the registry |
| `GET /api/v1/openapi.json` | The generated API description |

## Plug-in protocol

A plug-in is a separate process that exchanges messages over standard input and output, started and stopped only by the supervisor. It receives only the inputs for its step. Secrets reach it only through `inject`, and only if an administrator approved the effects its manifest declares. The message list, its versioning, and the launch handshake are in the design of [`a2m-provider`](../architecture/crates/a2m-provider.md).

## Benchmark matrix

Each cell is measured in CI against the built-in test application and compared with the previous release. The harness in `bench/` writes one JSON result per run (its schema is named "a2m-bench-result", version 1) and has two commands, `run` and `compare`.

| Scenario | Modes measured | Metric |
| --- | --- | --- |
| Single read | All | Overhead beyond app time, at the median and 99th percentile |
| Single write with read-back | All | Overhead; round trips used |
| 100-record pipeline | `ui`, `page_http`, `http` | Total time; records per second |
| 50 callers reading at once | `http`, `ui` | Throughput; queue wait |
| Cold start to first result | Solo profile | Wall time |
| Tool listing with 500 operations | MCP | Response time and size |
| Idle and loaded memory | Control plane, runner with 10 contexts | Resident memory |
