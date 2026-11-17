# Technical reference

This file holds the engineering detail behind the [system design](../spec/README.md): interfaces, data model, protocols, and catalogs. Everything here is a starting design to be refined in the phase that builds it.

## Crate layering

Crates depend only downward, which keeps build times short and the core small. A crate may depend only on the layers its row names, never on a crate in its own layer. `tools/docs_audit.py` reads this table, compares it with the crates on disk, and refuses any other edge. The reasoning behind each layer, and which crate defines each interface, are in [the architecture](../architecture/README.md) ([decision 0061](../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md)).

Layers 1 to 8 are the kernel. Layers 9 and 10 are the packs ([spec section 55](../spec/55-kernel-and-packs.md)).

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
| 10. Packs | `a2m-pack-web-ui`, `a2m-pack-web-probe`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-desktop` | Layers 1 to 5, and the base of its own pack |
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
| `Archetype` | Seven, fixed | [Spec section 54](../spec/54-interaction-archetypes.md) |
| `EffectClass` | Three, ordered from least to most serious | [Spec section 18](../spec/18-safety-policy-and-approvals.md) |
| `Hazard` | Five, ordered from least to most serious | [Spec section 61](../spec/61-effects-hazards-and-consent.md) |
| `FailureClass` | Twelve | "Failure codes" below, and [spec section 8](../spec/08-execution-engine.md) |
| `EvidenceLevel` | Seven, ordered as the ladder; the default is the weakest | [Spec section 59](../spec/59-the-evidence-ladder.md) |
| `Coverage` | Three; the default is `unknown` | [Spec section 58](../spec/58-discovery-and-findings.md) |
| `RunState`, `RecordState` | Twelve and six | [Spec section 42](../spec/42-run-ledger-design.md) |
| `SessionState` | Eight | [Spec section 44](../spec/44-session-keeper.md) |
| `BindingStatus` | Four | [File formats](file-formats.md) |

Two words are checked names and not enums, because their values arrive with packs and crates ([decision 0060](../decisions/0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md)):

| Type | Shape | Who supplies the values |
| --- | --- | --- |
| `Mode` | Lowercase letters, digits, and underscores, starting with a letter; at most 32 characters | A provider registers each mode it brings, with its archetype |
| `FailureCode` | The same shape; at most 64 characters | Each crate names the codes it can produce; a code never changes once released |

Two more types complete the crate:

- **`Failure`** is a class, a code, a plain sentence, and a next step. The sentence and the next step are shown to callers and must never contain a secret. A name that is not one of a vocabulary's values, or is badly formed, becomes a failure of class `input` with the code `schema_invalid`, and its next step lists the values that would have been accepted.
- **`Secret`** wraps a value that must never be shown. Printing it gives `[redacted]`. It cannot be serialized or copied, and the build fails if it ever can. The value is reached only through a method named `expose`, so that every use can be found.

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

## Widget drivers

One driver per control type, shared by the `ui` and `desktop` modes where the control exists in both.

| Control | Set | Read back |
| --- | --- | --- |
| `text`, `textarea` | Fill; escalate to key-by-key typing, then the native value setter with events | Input value |
| `number`, `currency`, `measurement` | As text, formatted by the field type | Input value, normalized |
| `select` | Choose by stored code | Selected code |
| `combobox`, `autocomplete` | Open, type to filter, choose the option whose code or exact label matches | Selected code or label map |
| `date`, `datetime`, `time` | Type in the field's declared format; fall back to the picker | Value, normalized to the field's time-zone rule |
| `checkbox`, `toggle` | Set to the target state only if different | Live checked state |
| `radio` | Choose by stored code | Checked option |
| `slider` | Keyboard steps to the value | Current value |
| `file` | Attach the file to the input directly | File name shown |
| `rich_text` | Editor-specific command, or paste as plain text | Editor content, normalized |
| `grid` | Address a cell by row key and column id; scroll or filter to find the row | Cell value; network response preferred |
| `tree` | Expand along a path of labels | Selected path |
| `tabs`, `accordion` | Activate by name | Active name |
| `dialog` | Declared as expected; confirm or dismiss by named action | Closed state |
| `toast` | Observed only | Text, used as a confirmation |

## Platform profile schema

```yaml
profile: aspnet_webforms
detect:                       # Wappalyzer-style rules
  any:
    - {dom: "input[name='__VIEWSTATE']"}
    - {js: "Sys.WebForms.PageRequestManager"}
ready:
  js: "!Sys.WebForms.PageRequestManager.getInstance().get_isInAsyncPostBack()"
locators:
  priority: [label, role, text, css]
  reject_id_patterns: ["^ctl\\d+_", "^MainContent_"]
input_commit: blur_then_wait_ready
frames: {mode: declared}
drift_risk: low               # how often the vendor ships interface changes
support_tier: full
```

## Auth provider interface

Each provider implements five operations: `sign_in`, `is_signed_in`, `detect_expiry`, `refresh`, `sign_out`, plus `inject`, which places credentials into a driver's context without returning them. Only the session keeper calls a provider.

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
| `totp` | One-time codes for dedicated accounts, where IT allows | `a2m-auth` |

Further packs bring their own where a credential is bound to their kind of connection: the data pack brings a database password provider.

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

Runners open every connection themselves, over mutually authenticated HTTPS; `lease` is the reply to a long-polled `claim`. The full design, including leases, lost-lease handling, and secrets, is in spec section 43.

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
| `policy` | `not_granted`, `identity_not_allowed`, `effect_too_high`, `approval_missing`, `approval_stale`, `limit_exceeded`, `surface_not_declared` |
| `input` | `lint_failed`, `path_not_allowed`, `file_type_mismatch`, `schema_invalid`, `source_template_unknown`, `value_out_of_range` |
| `session_expired` | `needs_login`, `needs_mfa`, `device_required` |
| `edge_blocked` | `challenge_page`, `rate_limited_by_edge`, `forbidden_by_edge` |
| `drift` | `locator_not_found`, `fingerprint_mismatch`, `response_schema_mismatch`, `upstream_tool_changed` |
| `unknown_variant` | `unexpected_dialog`, `unexpected_page`, `unexpected_status` |
| `rejected` | `app_validation_error` |
| `mismatch` | `read_back_differs` |
| `possible_commit` | `no_confirmation`, `runner_lost`, `ambiguous_match` |
| `stale` | `changed_since_preview`, `version_conflict` |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable` |
| `busy` | `queue_full`, `record_locked`, `session_in_use`, `runner_unavailable`, `storage_full`, `engine_degraded` |

## Command line

Every action in the [action catalog](action-catalog.md) is a command of the form `anything2mcp <noun> <verb>`, with a matching MCP tool, REST route, and SDK method. The nouns are `tool`, `run`, `session`, `target`, `auth`, `screen`, `capture`, `endpoints`, `spec`, `draft`, `test`, `fixture`, `shadow`, `plan`, `version`, `publish`, `binding`, `toolset`, `grant`, `policy`, `runner`, `killswitch`, `audit`, `secret`, `vault`, `delegation`, `engine`, `config`, `doctor`, `browser`, `plugin`, `client`, `trace`, `action`, `docs`, `self`, `provider`, `component`, `discover`, `finding`, `unknown`, and `scout`.

| Kind | Commands |
| --- | --- |
| Shortcuts | `init`, `add`, `learn`, `lint`, `try`, `test`, `publish`, `status`, `call`, `run`, `diff`, `bundle`, `rollback`, `migrate`, `connect`, `doctor`, `docs` |
| Process commands | `serve`, `runner`, `bridge` |

On a local machine no command approves a write; see spec section 39. On a server, approvals are given in the console or a chat card after the approver signs in.

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
| `/api/v1/admin/...` | Targets, versions, toolsets, grants, runners, audit |

## Plug-in protocol

A plug-in is a separate process that exchanges messages over standard input and output, started and stopped only by the supervisor. It receives only the inputs for its step. Secrets reach it only through `inject`, and only if an administrator approved the effects its manifest declares. The message list, its versioning, and the launch handshake are in the design of [`a2m-provider`](../architecture/crates/a2m-provider.md).

## Benchmark matrix

Each cell is measured in CI against the built-in test application and compared with the previous release.

| Scenario | Modes measured | Metric |
| --- | --- | --- |
| Single read | All | Overhead beyond app time, at the median and 99th percentile |
| Single write with read-back | All | Overhead; round trips used |
| 100-record pipeline | `ui`, `page_http`, `http` | Total time; records per second |
| 50 callers reading at once | `http`, `ui` | Throughput; queue wait |
| Cold start to first result | Solo profile | Wall time |
| Tool listing with 500 operations | MCP | Response time and size |
| Idle and loaded memory | Control plane, runner with 10 contexts | Resident memory |
