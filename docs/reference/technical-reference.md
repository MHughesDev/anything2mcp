# Technical reference

This file holds the engineering detail behind the [system design](../spec/README.md): interfaces, data model, protocols, and catalogs. Everything here is a starting design to be refined in the phase that builds it.

## Crate layering

Crates depend only downward, which keeps build times short and the core small. A crate may depend on crates in lower layers only, never on one in its own layer. `tools/docs_audit.py` reads this table, compares it with the crates on disk, and refuses any other edge ([decision 0045](../decisions/0045-the-workspace-starts-with-every-crate-and-the-audit-enforces-the-layering.md)).

| Layer | Crates | May depend on |
| --- | --- | --- |
| 1. Types | `w2m-core` | Nothing internal |
| 2. Foundations | `w2m-files`, `w2m-expr`, `w2m-vault`, `w2m-telemetry` | Layer 1 |
| 3. Services | `w2m-ledger`, `w2m-policy`, `w2m-auth`, `w2m-sources`, `w2m-browser` | Layers 1 to 2 |
| 4. Drivers | `w2m-driver-ui`, `w2m-driver-http`, `w2m-driver-mcp`, `w2m-driver-desktop` | Layers 1 to 3 |
| 5. Engine | `w2m-engine`, `w2m-sched`, `w2m-probe` | Layers 1 to 4 |
| 6. Actions | `w2m-actions` | Layers 1 to 5 |
| 7. Roles | `w2m-runner`, `w2m-mcp`, `w2m-api`, `w2m-local` | Layers 1 to 6 |
| 8. Product | `web2mcp` | Everything |

## Driver interface

Every mode implements one trait. The sketch is illustrative, not final.

```rust
#[async_trait]
pub trait Driver: Send + Sync {
    fn mode(&self) -> Mode;
    fn capabilities(&self) -> DriverCapabilities;

    /// Acquire what the step needs: a page, an HTTP client, a desktop handle.
    async fn prepare(&self, ctx: &RunContext, lease: &SessionLease) -> Result<Prepared, Failure>;

    /// Perform one compiled step. Must not retry internally.
    async fn act(&self, p: &mut Prepared, step: &CompiledStep, input: &Values)
        -> Result<ActOutcome, Failure>;

    /// Re-read values for verification, without side effects.
    async fn read_back(&self, p: &mut Prepared, rule: &ReadBackRule) -> Result<Values, Failure>;

    /// Cheap check used by canaries and by mode selection.
    async fn health(&self, surface: &Surface) -> Health;

    async fn release(&self, p: Prepared);
}
```

Rules for every driver: no retries inside `act`; every failure maps to exactly one failure class; no secret ever appears in `ActOutcome`, `Values`, or `Failure`.

Two things in the sketch do not yet match the rest of the design, and slice 1.1 settles both with a decision record, since this is a public interface. First, [steps, snapshots, and diffs](steps-snapshots-and-diffs.md) makes confirm and read-back lists of steps, which `act` could carry out, while the sketch gives read-back a method of its own taking a rule. Second, `read_back` also serves the preview and reconcile ([file formats](file-formats.md)), so its result must be able to say that a record is absent.

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

Each provider implements five operations: `sign_in`, `is_signed_in`, `detect_expiry`, `refresh`, `sign_out`, plus `inject`, which places credentials into a driver's context without returning them.

| Provider | Covers |
| --- | --- |
| `sso_oidc`, `sso_saml` | Entra ID, Okta, Google Workspace, ADFS, Ping |
| `os_broker` | Windows Web Account Manager, macOS Enterprise SSO, Linux identity broker; user devices only |
| `obo_token` | OAuth On-Behalf-Of exchange from the caller's token |
| `form_login` | Username and password pages |
| `http_basic`, `http_digest` | Simple intranet apps |
| `windows_integrated` | NTLM and Kerberos |
| `client_certificate` | Mutual TLS |
| `api_key`, `bearer_static` | Vendor APIs with fixed keys |
| `oauth_client_credentials`, `oauth_authorization_code` | Vendor APIs with OAuth |
| `upstream_mcp` | Authorization to an app's own MCP server |
| `totp` | One-time codes for dedicated accounts, where IT allows |

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

Every failure carries a class, a stable code, a plain sentence, and a next step.

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

Every action in the [action catalog](action-catalog.md) is a command of the form `web2mcp <noun> <verb>`, with a matching MCP tool, REST route, and SDK method. The nouns are `tool`, `run`, `session`, `target`, `auth`, `screen`, `capture`, `endpoints`, `spec`, `draft`, `test`, `fixture`, `shadow`, `plan`, `version`, `publish`, `binding`, `toolset`, `grant`, `policy`, `runner`, `killswitch`, `audit`, `secret`, `vault`, `delegation`, `engine`, `config`, `doctor`, `browser`, `plugin`, `client`, `trace`, `action`, `docs`, and `self`.

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
| Large results | Written under `.web2mcp/out/`; the result carries path, size, and summary |
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

Each operating system's own conventions are followed. `W2M_HOME` overrides all of them with one directory. Exact paths are confirmed when the paths module is written in Phase 1.

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

A plug-in is a separate process that exchanges JSON-RPC messages over standard input and output.

| Method | Purpose |
| --- | --- |
| `describe` | Kind (driver, auth provider, source reader, widget driver), version, capabilities requested |
| `prepare`, `act`, `read_back`, `health`, `release` | The driver interface, for driver plug-ins |
| `read`, `fingerprint` | For source readers |
| `sign_in`, `refresh`, `inject` | For auth providers |

A plug-in receives only the inputs for its step. Secrets reach it only through `inject`, and only if its declared capabilities were approved by an admin.

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
