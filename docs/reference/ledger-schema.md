# Ledger schema

Table definitions for the run ledger described in [spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md) and the scheduling tables used by [spec section 223](../spec/2-kernel/223-the-runner-protocol.md). The tables of `a2m-ledger` and the first two of `a2m-policy` are built; their definitions in force are the migration files in `crates/a2m-ledger/migrations/`, and where this page and a file differ the file is right. The other tables here are a design draft until their owner writes the migration. Any change after a release is a numbered migration.

Types are written generically. `id` is a time-ordered UUID. `json` is `jsonb` on Postgres and text on SQLite. `ref` names a blob by a keyed digest of its content: a digest made with a key the vault holds, so that someone who can read the store cannot confirm a guessed value by hashing it. `ts` is a timestamp in UTC.

## runs

| Column | Type | Notes |
| --- | --- | --- |
| `id` | id | Primary key; shown to callers as `r_…` |
| `workspace_id` | id | Isolation boundary |
| `principal_id` | id | The caller |
| `client_label` | text | Which client made the call |
| `toolset_id`, `toolset_version` | id, int | Null for local use |
| `tool_name`, `tool_kind` | text, enum | `operation` or `pipeline` |
| `target`, `target_version_hash`, `environment` | text, text, text | The exact published version that ran |
| `identity_ref` | id | The session or identity the target saw |
| `effect` | enum | `read`, `write`, `irreversible` |
| `input_hash`, `input_ref` | text, ref | Inputs, stored once |
| `dedupe_key` | text | Unique among unfinished runs |
| `state`, `state_class`, `state_code` | enum, enum, text | Run state and, when stopped, the failure class and code |
| `attempt` | int | Increases on each resume |
| `lease_epoch` | bigint | Increases on each lease; the fencing number |
| `diff_hash`, `diff_ref` | text, ref | Set when the preview is built |
| `approval_id`, `decision_id` | id, id | The approval that was used, or the recorded decision that let the run write without a person |
| `counts` | json | Planned, unchanged, verified, failed |
| `result_ref`, `trace_ref` | ref, ref | Result and trace |
| `created_at`, `started_at`, `finished_at` | ts | Timings |
| `retention_class`, `legal_hold` | enum, bool | Retention |

Indexes: unique on `dedupe_key` where the state is not final; on `(workspace_id, state)`; on `(principal_id, created_at)`.

## run_steps

| Column | Type | Notes |
| --- | --- | --- |
| `run_id`, `seq` | id, int | Primary key |
| `step_id`, `operation` | text, text | From the pipeline file |
| `state` | enum | `pending`, `running`, `done`, `stopped`, `skipped` |
| `binding_mode`, `binding_reason` | enum, text | Which binding was chosen and why |
| `started_at`, `finished_at` | ts | Timings |

## run_records

| Column | Type | Notes |
| --- | --- | --- |
| `run_id`, `step_seq`, `record_seq` | id, int, int | Primary key |
| `record_key`, `record_key_hash` | text, text | Normalized key; the hash is what locks use |
| `operation` | text | The operation that writes this commit point |
| `state` | enum | `planned`, `unchanged`, `intent`, `acted`, `verified`, `failed` |
| `intended_hash`, `intended_ref` | text, ref | What should be stored |
| `before_ref` | ref | Values seen before the write; used by undo and by resume |
| `after_ref`, `read_back_hash` | ref, text | Values read back, and their digest; a record is `verified` only when the digest equals `intended_hash` |
| `idempotency_token` | text | Stable for the run and key; sent where the application accepts one; the stamp for `stamped` identity |
| `assigned_key` | text | The identifier the application assigned, stored as soon as it is known |
| `outputs_ref` | ref | The record's result, for later steps that depend on it |
| `skip_reason` | text | Such as `blocked_by_parent` |
| `binding_mode` | enum | Mode that performed the write |
| `intent_epoch`, `intent_seq` | bigint, bigint | Lease number under which the intent was stored, and the number of its intent event |
| `attempts` | int | Write attempts |
| `failure_class`, `failure_code` | enum, text | When failed |
| `evidence_ref` | ref | Trace fragment for this record |
| `intent_at`, `acted_at`, `verified_at` | ts | Timings |

Indexes: on `(run_id, state)`; on `record_key_hash`.

## run_events

| Column | Type | Notes |
| --- | --- | --- |
| `run_id`, `seq` | id, bigint | Primary key; `seq` has no gaps within a run |
| `epoch` | bigint | Lease number of the writer |
| `at` | ts | Database time |
| `kind` | enum | `state_changed`, `step_started`, `binding_chosen`, `intent`, `acted`, `verified`, `failed`, `reconcile_read`, `approval_bound`, `lease_granted`, `lease_lost`, `need`, `late_evidence`, `planned`, `diff_stored`, `lock` |
| `payload` | json | Small; large data goes in a blob |
| `evidence_ref` | ref | Optional |

Append-only. An insert carrying an `epoch` other than the run's current `lease_epoch` is refused, except for `late_evidence`.

## approvals

| Column | Type | Notes |
| --- | --- | --- |
| `id` | id | Primary key |
| `scope` | enum | `run` or `standing` |
| `run_id` | id | For run scope |
| `pattern`, `caller`, `window_start`, `window_end` | text, id, ts, ts | For standing scope |
| `diff_hash`, `tool_version_hash` | text, text | What was approved |
| `approver_id` | id | The person |
| `channel`, `presence_check` | enum, enum | How the approval arrived and which user-presence check was passed |
| `granted_at`, `expires_at`, `used_at`, `revoked_at` | ts | A run-scope approval is used once |

## record_locks

| Column | Type | Notes |
| --- | --- | --- |
| `workspace_id`, `target`, `environment`, `record_key_hash` | id, text, text, text | Primary key |
| `run_id`, `epoch` | id, bigint | Holder |
| `state` | enum | `held` or `quarantined` |
| `acquired_at` | ts | For diagnostics |

## jobs

| Column | Type | Notes |
| --- | --- | --- |
| `id` | id | Primary key |
| `seq` | bigint | Order of arrival |
| `run_id` | id | Null for system jobs. At most one job of a run is `pending` or `leased` |
| `workspace_id`, `principal_id` | id, id | Whose job; the caller, for its cap and its turn |
| `kind` | enum | `preview`, `execute`, `try`, `discover`, `reconcile`, `verify_after_loss`, `canary`, `session_keepalive` |
| `target`, `environment` | text, text | The queue the job waits in |
| `requirements` | json | Target, environment, modes, operating system, the person it acts as, session and its limits, exclusive resources, longest step |
| `session_key`, `home_runner_id` | id, id | The live session the job needs, and the runner holding it, when there is one |
| `writes` | bool | Whether the job may write |
| `priority`, `available_at`, `queue_until` | int, ts, ts | Ordering, the moment from which it may be handed out, and its queue limit |
| `state` | enum | `pending`, `leased`, `done`, `lost`, `dead` |
| `epoch` | bigint | Lease number given to the runner; it only grows |
| `runner_id`, `lease_mode`, `lease_until`, `heartbeat_at` | id, text, ts, ts | Current lease, and the mode whose slot it took |
| `attempts` | int | Dispatch attempts |
| `directive` | enum | `continue` or `cancel`: what the next heartbeat tells the runner |
| `end_state`, `end_class`, `end_code` | text, text, text | The run's state when the job ended, and the failure it ended with |
| `created_at`, `finished_at` | ts, ts | |

Indexes: on `(priority, seq)` where the state is `pending`; on `(runner_id, state)`; on `(session_key)` where the state is `leased`; unique on `(run_id)` where the state is `pending` or `leased`. A job that has ended never changes.

## runners

| Column | Type | Notes |
| --- | --- | --- |
| `id` | id | Primary key |
| `owner_principal_id` | id | Set for user-device runners |
| `workspace_id` | id | Scope |
| `capabilities` | json | Operating system, modes, targets |
| `protocol_version` | int | For compatibility |
| `state` | enum | `enrolled`, `online`, `draining`, `offline`, `revoked` |
| `last_seen_at` | ts | Liveness |

The runner's kind, version, public key, and certificate expiry arrive with the runner link and are not columns yet.

## resource_holds, caller_turns, scheduler_holds, budgets

The scheduler's other tables ([`a2m-sched`](../architecture/crates/a2m-sched.md)).

| Table | Columns | Notes |
| --- | --- | --- |
| `resource_holds` | `resource_key` (primary key), `job_id`, `acquired_at` | Who holds each exclusive resource: one holder for one key |
| `caller_turns` | `workspace_id`, `principal_id` (primary key), `served` | The number of the dispatch that last gave this caller a job; the lowest goes first |
| `scheduler_holds` | `scope`, `workspace_id`, `name` (primary key), `held_at` | Scopes no job is handed out for: `everything`, `workspace`, `target`, `caller` |
| `budgets` | `budget_key` (primary key), `capacity`, `refill`, `per_ms`, `level`, `updated_at` | Token buckets. `level` is in requests times `per_ms`, so no fraction is lost |

## sessions

| Column | Type | Notes |
| --- | --- | --- |
| `id` | id | Primary key |
| `identity_ref`, `target`, `environment` | id, text, text | Whose session, on what |
| `owner_principal_id` | id | The person a user session belongs to; null for a service account |
| `state` | enum | `absent`, `signing_in`, `valid`, `suspect`, `renewing`, `needs_person`, `expired`, `revoked` |
| `home_runner_id` | id | Where the live session is |
| `movable` | bool | False for device-bound sessions |
| `survives_restart` | bool | False where the session ends when the browser closes |
| `exclusive` | bool | True where the application allows one session per account |
| `learned` | json | Observed idle timeout and maximum age |
| `last_used_at` | ts | Start of the keep-alive window |
| `vault_ref` | ref | Encrypted saved state; null when the state never leaves a device |
| `epoch` | bigint | Increases when the session moves or is replaced |
| `expires_at`, `checked_at` | ts | Health |

As built by migration `0006`, which `a2m-auth` owns: the table also has `workspace_id`, `provider`, `profile`, `state_since`, `signed_in_at`, `stored_attempts`, and `wants`; saved state is a row of `session_state` and not a `vault_ref` column; `survives_restart`, `exclusive`, and `learned` are not columns yet. The same migration makes `session_events` (the timeline), `session_state` (sealed saved state), and `session_credentials` (stored credentials, sealed).

## blobs

| Column | Type | Notes |
| --- | --- | --- |
| `hash` | text | Primary key; the keyed digest of the plain data |
| `workspace_id`, `owner_kind`, `owner_id` | id, enum, text | The workspace whose digest key named it, and the owner it is sealed for |
| `kind` | enum | `values`, `diff`, `snapshot`, `trace`, `screenshot`, `input`, `result`, `exchange`, `window_tree` |
| `size`, `location`, `sealed` | int, text, bytes | Inline, file, or object-storage key; the sealed bytes when inline |
| `key_ref` | text | Which vault key encrypts it |
| `retention_class`, `created_at` | enum, ts | Retention |

## outbox

| Column | Type | Notes |
| --- | --- | --- |
| `id` | id | Primary key |
| `kind`, `destination` | enum, text | Webhook, chat approval, notification |
| `payload_ref` | ref | Content |
| `attempts`, `next_at`, `done_at` | int, ts, ts | Delivery with backoff |

Rows are inserted in the same transaction as the change they announce, so a message is never lost and never sent for a change that did not commit.

## caller_idempotency

| Column | Type | Notes |
| --- | --- | --- |
| `principal_id`, `key` | id, text | Primary key; the caller's own idempotency key on REST calls |
| `request_hash`, `run_id` | text, id | The same key with a different request is refused |
| `expires_at` | ts | Cleanup |

## audit

Unchanged from the technical reference: an append-only, hash-chained log of who did what. Ledger events are evidence about runs; the audit log is the record of decisions and administration.
