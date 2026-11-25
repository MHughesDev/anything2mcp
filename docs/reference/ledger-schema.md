# Ledger schema

Table definitions for the run ledger described in [spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md) and the scheduling tables used by [spec section 223](../spec/2-kernel/223-the-runner-protocol.md). The tables of `a2m-ledger`, `a2m-policy`, `a2m-auth`, `a2m-sched`, and `a2m-actions` are built. Their definitions in force are the migration files in `crates/a2m-ledger/migrations/`, numbered in order; a number that was never used is skipped. Where this page and a file differ, the file is right. Two tables here, `run_steps` and `outbox`, have no migration and are not built. Any change after a release is a numbered migration.

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
| `result_coverage` | text | How much of what its read could return the result holds, as the engine stated it with the result: `complete`, `partial`, or `unknown`, the default and every run before migration `0029`. Written with `result_ref` in one statement; a trigger refuses it without a result and any change once the run is final ([decision 0472](../decisions/0472-a-read-declares-its-records-over-its-list-outputs-and-a-driver-states-whether-it-returned-every-record.md)) |
| `created_at`, `started_at`, `finished_at` | ts | Timings |
| `retention_class`, `legal_hold` | enum, bool | Retention |

Indexes: unique on `dedupe_key` where the state is not final; on `(workspace_id, state)`; on `(principal_id, created_at)`.

## run_steps

Not built: no migration creates this table.

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
| `intent_at`, `acted_at`, `verified_at` | ts | Timings. `acted_at` set means the write was sent in the current attempt: the record stays in doubt, whatever it failed with, until it is read back |
| `reconciled_at` | ts | When reconcile last read it back. A record that may have been written returns to `planned` only with it set; a new intent clears it |
| `acknowledged_at` | ts | When the record ended on the system's acknowledgement: its write was sent and answered, and the run's stored diff says that answer is the binding's only evidence. The record stays `acted`, is never `verified`, has no `read_back_hash`, keeps the answer's evidence in `evidence_ref`, and is settled, not in doubt; once set, nothing of the row changes. Migration `0015` ([decision 0172](../decisions/0172-the-ledger-ends-a-record-on-an-acknowledgement-and-keeps-what-the-system-said.md)) |

Indexes: on `(run_id, state)`; on `record_key_hash`.

A run enters `completed` only when every record is `verified`, `unchanged`, or `acted` with `acknowledged_at` set; a trigger holds it.

## run_events

| Column | Type | Notes |
| --- | --- | --- |
| `run_id`, `seq` | id, bigint | Primary key; `seq` has no gaps within a run |
| `epoch` | bigint | Lease number of the writer |
| `at` | ts | Database time |
| `kind` | enum | `state_changed`, `step_started`, `binding_chosen`, `intent`, `acted`, `verified`, `failed`, `reconcile_read`, `approval_bound`, `lease_granted`, `lease_lost`, `need`, `late_evidence`, `planned`, `diff_stored`, `lock` |
| `payload` | json | Small; large data goes in a blob. A record that ends on an acknowledgement gets a second `acted` event whose payload says `"evidence": "acknowledged"`. A `state_changed` event that stops or ends a run, and a `failed` event, carry the failure's `class`, `code`, `message`, `next_step`, and `said`, what the system itself said, redacted and kept apart from the sentence |
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
| `channel`, `presence_check` | enum, enum | How the approval arrived and which user-presence check was passed: `os_prompt`, `polkit`, `local_authentication`, `windows_hello`, `field_harness` (only the copy test harness records it, decision 0310; the word keeps the harness's old name, decision 0402), or `signed_in`, each of which proves presence, or `not_available`, accepted only on the engine's own prompt (`local_prompt`) |
| `granted_at`, `expires_at`, `used_at`, `revoked_at` | ts | A run-scope approval is used once |

## decisions

As built by migrations `0004` and `0005`, which `a2m-policy` owns. One row per answer of the policy engine.

| Column | Type | Notes |
| --- | --- | --- |
| `id` | id | Primary key |
| `workspace_id`, `principal_id` | id, id | Isolation boundary; the caller |
| `run_id` | id | The run the question was about, when there is one |
| `question` | text | The name of what was asked. `write_without_person`, allowed, is what lets a run write with no approval |
| `allowed` | bool | The answer |
| `diff_hash` | text | The run's diff, for a question about a write |
| `reasons` | json | The rules that made the answer |
| `policy_version` | text | The policy set that answered |
| `decided_at` | ts | When |
| `resource`, `request`, `refusal_code` | text, json, text | Added by migration `0005`, for `policy.explain`: the resource's short name, the whole request as canonical JSON, and the code a refused caller was given |

Index: on `run_id`.

## approval_declines

A person's refusal of a run's diff, or an approval window that ended with no answer.

| Column | Type | Notes |
| --- | --- | --- |
| `id` | id | Primary key |
| `workspace_id`, `run_id` | id, id | The run whose diff was declined |
| `declined_by` | id | The person; null when the window ended with no answer |
| `channel` | text | How the refusal arrived |
| `declined_at` | ts | When |

Index: on `run_id`.

## policy_versions

Published policy sets. A row never changes and is never removed; the newest `seq` is the one in force.

| Column | Type | Notes |
| --- | --- | --- |
| `seq` | int | Primary key |
| `version` | text | The digest over the format, the guardrails' digest, and both texts. A set published again after another took its place is a new row with the same version |
| `format` | int | The format of a policy set |
| `guardrails` | text | The digest of the built-in guardrails |
| `layers`, `tests` | json, json | The set's layers and its test table, as canonical JSON |
| `published_by`, `presence_check` | id, text | Who published it, and the user-presence check passed |
| `published_at` | ts | When |

Index: on `version`.

## kill_switches

| Column | Type | Notes |
| --- | --- | --- |
| `scope`, `name` | enum, text | Primary key. `everything`, `workspace`, `target`, or `operation`, and what the scope names; the name is empty for everything |
| `is_on` | bool | Whether the switch is on |
| `epoch` | bigint | Rises with every change, so a runner can tell that one happened |
| `set_by`, `presence_check` | id, text | Who changed it, and the user-presence check passed |
| `set_at` | ts | When |

## quota_counters

Calls counted against a quota.

| Column | Type | Notes |
| --- | --- | --- |
| `workspace_id`, `principal_id`, `period_minutes`, `period_start` | id, id, int, ts | Primary key: one row per caller, period length, and period |
| `calls` | int | Calls counted in that period |

## record_locks

| Column | Type | Notes |
| --- | --- | --- |
| `workspace_id`, `target`, `environment`, `record_key_hash` | id, text, text, text | Primary key |
| `run_id`, `epoch` | id, bigint | Holder |
| `state` | enum | `held` or `quarantined` |
| `acquired_at` | ts | For diagnostics |

A record lock is held, quarantined, or released. A released lock has no row: its row is removed in the transaction that releases it.

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

As built by migrations `0006`, `0024`, and `0027`, which `a2m-auth` owns: the table also has `workspace_id`, `provider`, `profile`, `state_since`, `signed_in_at`, `stored_attempts`, `wants`, `attempted_with` (the mark of the stored credential the count of attempts is about: a digest of its sealed envelopes, never of a value; [decision 0269](../decisions/0269-a-stored-credentials-attempts-outlive-a-restart-and-sign-ins-without-the-person-are-limited.md)), and `signed_in_under` (`published` or `drafts`: what declared the last sign-in; null in a row kept before migration `0027`, and the keeper reads null as `drafts`, so such a session is not renewed without its person; [decision 0374](../decisions/0374-a-sessions-mark-that-it-was-made-under-drafts-is-kept-in-its-record.md)); saved state is a row of `session_state` and not a `vault_ref` column; `survives_restart`, `exclusive`, and `learned` are not columns yet. The same migration makes `session_events` (the timeline), `session_state` (sealed saved state), and `session_credentials` (stored credentials, sealed).

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

Not built: no migration creates this table.

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
| `workspace_id`, `principal_id`, `key_digest` | id, id, text | Primary key; the workspace's keyed digest of the caller's own idempotency key, under the purpose `caller_key`, never the key |
| `request_hash`, `run_id` | text, id | The digest of the call: tool, environment, input and source. The same key with a different request is refused. The run is empty from the moment the key is held, before the run is admitted, until it is bound |
| `held_at` | ts | When the key was held. A bound key holds for as long as its run is kept, and leaves with it |

As built by migration `0013-ledger-caller-idempotency.sql` ([decision 0160](../decisions/0160-an-idempotency-key-gives-back-its-run-and-run-list-shows-a-callers-own-runs.md)).

## actions_installation, actions_toolsets, actions_scout_stages, actions_targets, actions_run_calls, actions_run_sources, actions_scouts, actions_discovery_results, actions_drift_marks

As built by migrations `0009`, `0011`, `0014`, and `0028`, which `a2m-actions` owns and is the only crate to read or write. Moments are milliseconds since 1970, from the store's clock.

| Table | One row is | Key |
| --- | --- | --- |
| `actions_installation` | One fact about the installation, by name: the workspace of one machine, the principal of its user, the digest of the action catalog, the digest of the settings at the last start. Never a secret | `name` |
| `actions_toolsets` | A toolset, as JSON written through `toolset.create` and `toolset.update`, with who updated it and when | `workspace_id`, `name` |
| `actions_scout_stages` | One finished stage of one scout run, stages 0 to 7: the hash of what it was given and its result, already redacted. A stopped scout continues after the last stage stored | `run_id`, `stage` |
| `actions_targets` | A target that has a published version, so that a tool can be found by its name alone. Written by `version.publish` in the transaction that stores the version | `workspace_id`, `target` |
| `actions_run_calls` | What a run was called with: the stored input and the verified facts about the caller, as names and flags and never a credential. Written before the run is admitted | What makes a run the same run (ledger rule 6): `workspace_id`, `principal_id`, `tool_name`, `target_version`, `input_hash` |
| `actions_run_sources` | The source a run was called with: the blob that holds its bytes, whether it was a file of the project or rows given inline, the path inside the project as the caller wrote it, and its size. Written beside the row of `actions_run_calls`, and read by the runner so that every lease of the run reads the same bytes | The same key as `actions_run_calls` |
| `actions_scouts` | One scout of one target by one person: when it started and finished, its report's hash once the last stage is stored, and `project`, the digest of the project folder it was made in or `none` (migration `0018`, [decision 0196](../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md); a scout stored before has none and is never continued). Its identifier keys its stages in `actions_scout_stages` (column `run_id`); one with no `finished_at` is continued by the next `scout.run` in the same project ([decision 0170](../decisions/0170-the-scout-identifies-each-surface-with-one-discovery-job-and-stores-each-stage.md)) | `scout_id` |
| `actions_discovery_results` | What one discoverer reported in one discovery run: the surface, the identifiers of its findings as a JSON list, and as JSON its coverage, the bounds it reached, and what it dropped, or its failure's class and code. Written by the run's control in the transaction that stores the findings | `run_id`, `provider`, `discoverer` |
| `actions_drift_marks` | One piece of knowledge a canary or a run found drifted, on one surface through one mode: `entry`, a pack entry held by its name and the digest of the whole section that holds it, or `binding`, a binding whose drift no step named, held by its digest; with the operation, the drift's code, and the run that saw it. Kept when the engine reports a drift, lifted when a canary's pass vouches for the same knowledge, and read by the runner into each job's conditions. Of the target, not of a version: a version that holds the same digest is stopped by it ([decision 0380](../decisions/0380-a-drift-marks-the-knowledge-it-was-seen-in-and-stops-every-binding-that-relies-on-it.md)) | `workspace_id`, `target`, `surface`, `mode`, `knowledge`, `digest`, `entry` |

## target_versions

| Column | Type | Notes |
| --- | --- | --- |
| `workspace_id`, `target`, `plan_hash` | id, text, text | Primary key: one published version of a target is its compiled target, named by the compiled target's hash. The columns `plan_hash` and `plan` keep the compiled target's old name, so that a store an older build wrote still opens ([decision 0402](../decisions/0402-the-rename-sweep-renames-every-name-and-keeps-what-is-stored.md)) |
| `version` | text | The version the compiled target's header states |
| `plan` | bytes | The compiled target's canonical bytes |
| `published_by`, `published_at` | text, ts | Who published it, and when |
| `is_current` | bool | At most one current version per target, held by a unique partial index |
| `gate_evidence` | text | The evidence of the publish gates the version passed, canonical JSON written once with the version: each gate that ran with its result, and the gates the documents name that did not run (spec section 403, [decision 0180](../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)). Null for a version stored before migration `0016` |

As built by migrations `0003`, `0016-ledger-versions-keep-their-gate-evidence.sql`, and `0017-ledger-versions-gate-evidence-never-changes.sql`, which `a2m-ledger` owns. A version never changes once stored, and is never removed: triggers refuse an update of its compiled target, hash, version, target, workspace, or gate evidence, and every delete.

## shadow_comparisons, shadow_promotions

| Column | Type | Notes |
| --- | --- | --- |
| `seq` | int | Primary key of a comparison, in the order they were kept |
| `workspace_id`, `target`, `environment` | id, text, text | Taken from the run, never from the caller |
| `operation`, `binding_digest`, `mode`, `position` | text, text, text, int | The binding in shadow, named by the digest of what it does ([decision 0200](../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)) |
| `against_digest`, `run_id` | text, id | The binding that did the real work, and the run it was compared beside |
| `writes` | bool | Whether the operation writes; always false while the shadow of a write is not built |
| `outcome` | enum | `match`, `mismatch`, `not_compared` |
| `differing` | text | For a mismatch: the outputs that differed, by name, and how (`differs`, `missing`, `unreadable`), as JSON; never a value |
| `why_class`, `why_code` | enum, text | For `not_compared`: what stopped it |
| `at` | ts | When it was kept |
| `input_digest` | hash | The workspace's keyed digest of the call's inputs, made in the ledger's transaction; the inputs are never stored, and the digest is never shown. Required of every comparison kept from migration `0022` on (trigger); null in one kept before, which counts as no distinct input |
| `input_purpose` | enum | The purpose `input_digest` was made under: `shadow_input` for every comparison kept from migration `0023` on, `caller_key` for one kept before. Only comparisons of `shadow_input` count as distinct inputs, so one input digested under both is never counted twice, and an older comparison counts as none ([decision 0262](../decisions/0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md)) |
| `requests` | int | What the shadow read cost the system, in the rate budget's units, one per step granted. 0 in a comparison kept before migration `0022` |

A promotion (`shadow_promotions`) is one row per workspace, target, operation, and binding digest: who promoted it and when; the fewest distinct inputs that matched in any environment its evidence came from, which is at least the threshold in force; the threshold; the last comparison counted; `against_digest`, the verified binding the counted matches were against; and `evidence`, canonical JSON with, for each environment its evidence came from, its distinct inputs and its matches. The last two are null for a promotion made before migration `0022`, whose `matches` are matches in a row. As built by migrations `0020-ledger-shadow-comparisons-and-promotions.sql`, `0022-ledger-shadow-evidence-names-what-it-covers.sql`, and `0023-ledger-watch-shape-and-shadow-input-purpose.sql`, which `a2m-ledger` owns ([decisions 0207](../decisions/0207-a-shadow-binding-reads-beside-the-verified-one-and-a-person-promotes-it-on-a-run-of-exact-matches.md) and [0251](../decisions/0251-the-evidence-for-promoting-a-shadow-binding-says-what-it-covers-and-a-shadow-costs-what-it-says.md)). Both are append-only by trigger; a comparison is taken only beside a run that is `running`, and leaves only with its run, when retention purges it. Keeping a comparison also appends a `need` event to its run that says a shadow read was made, how it compared, and what it cost in requests, with no value and no digest. Counts are queries over the rows. What makes a binding promotable is, in each environment its evidence must come from, the number of distinct input digests under `shadow_input` whose comparison matched the named verified binding after the last mismatch; a mismatch in any environment, against any verified binding, starts the count again, and `not_compared` neither counts nor starts it again.

## watches, watch_checks, watch_changes, watch_baseline

A watch: one published read with a fixed input, repeated on a schedule its person confirmed, and what changed between its checks. As built by migrations `0021-ledger-watches-checks-and-changes.sql`, `0023-ledger-watch-shape-and-shadow-input-purpose.sql`, `0025-ledger-watch-checks-say-whether-they-opened-their-run.sql`, and `0026-ledger-a-watch-ends-when-its-condition-holds.sql`, which `a2m-ledger` owns ([decisions 0247](../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md), [0262](../decisions/0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md), and [0291](../decisions/0291-a-watch-ends-when-its-condition-holds-on-a-checks-own-result.md)). Migration `0026` built `watches` and `watch_changes` again, with their triggers and indexes, to widen the words of `reason` and `kind`, and copied every row across unchanged. No value of what a check read is kept in any of the four tables: only keyed digests, counts, names of outputs, and times. The values stay in the result of each check's run.

| Column of `watches` | Type | Notes |
| --- | --- | --- |
| `id` | id | `watch_` and 32 hexadecimal characters |
| `workspace_id`, `target`, `environment`, `operation`, `target_version_hash` | id, text, text, text, hash | The published read and the version it was confirmed for. Never changed (trigger) |
| `effect` | enum | Always `read` (check constraint) |
| `input_hash`, `input_ref` | hash, blob ref | The fixed input, as a run's are. Never changed (trigger) |
| `ignore_outputs` | json | Output names left out of the digests; for a list, fields of its records |
| `surface`, `requests_per_check`, `record_shape` | text, int, json | From the published compiled target, never from the caller: the surface whose budget the checks share, how many requests one check sends (at least one), and what its records are, as canonical JSON, `{"whole":true}`, `{"key":[<field>, ...],"list":<output>}`, or, for a read that declares its records over its list outputs, `{"columns":[<output>, ...],"key":[<output>, ...]}` ([decision 0472](../decisions/0472-a-read-declares-its-records-over-its-list-outputs-and-a-driver-states-whether-it-returned-every-record.md)). Required of every watch (trigger); never changed (trigger). Migration `0023` |
| `condition` | json | What ends it early, or null: the compiled target's form of an expression over a check's result that gives true or false, as canonical JSON (`{"source":<text>,"result":<type>,"library":<n>}`), compiled by the registry against the read's outputs and loaded again by the ledger when the watch is asked. Never changed (trigger). Migration `0026` |
| `interval_seconds`, `ends_at` | int, ts | The schedule and the latest end: at least 60 seconds, and at most 366 days after `asked_at`, whoever sends the statement (trigger, migration `0023`). Never changed (trigger) |
| `asked_by`, `acts_for`, `client_label`, `asked_at` | id, id, text, ts | Who asked, whose watch it is, which client, when |
| `confirmed_by`, `confirmed_at`, `phase_ms` | id, ts, int | Set once, while `asked`, by the person it acts for and no one else (check constraint and trigger). Due time `n` is `confirmed_at + phase_ms + n × interval` |
| `state` | enum | `asked`, `confirmed`, `running`, `paused`, `ended`; `running` only from `confirmed`, one check at a time; `ended` never changes |
| `reason` | enum | Why it paused or ended, a state of the watch and not a failure: `needs_login`, `needs_input`, `drifted`, `version_changed`, `on_hold`, `paused_by_person`, `ended_by_person`, `ended_by_caller`, `end_reached`, `version_withdrawn`, `declined`, `condition_met` (migration `0026`: its condition held on a check). Only a watch paused `drifted` or `version_changed` never goes on |
| `reason_failure_class`, `reason_failure_code` | enum, text | The failure of the check that made it pause, when one did |
| `next_due_index`, `not_before` | int, ts | The next due time not yet handled, which only grows; a refused admission's retry moment |
| `running_check`, `checks` | int, int | The check under way; how many were begun |
| `baseline_through`, `baseline_complete` | int, bool | The last check the baseline took; whether a complete check is in it |
| `retention_state` | enum | `kept` or `purging`; a watch leaves only with the retention job, which is not built |

`watch_checks` is one row per check, keyed by watch and number: the due time's index and moment, whether it was the one catch-up check after missed due times, the run of the read (a trigger refuses any run but the watch's own read: its operation, version, and input, for the person it acts for), when it began and ended, the outcome (`running`, `complete`, `cut_short`, `failed`, `faulted`; `cut_short` is a completed run whose result does not prove that the read returned every record: one whose run's `result_coverage` is not `complete`, one of a watch compared whole, or one that saw no record after a baseline that held some), the class and code of a failure or fault (`watch_result_not_its_shape` for a result not of the watch's `record_shape`, `watch_condition_unanswered` for a result on which the watch's condition gave neither true nor false), the count of records seen, and the counts of added, changed, removed, unseen, and unchanged records, and `opened_run` (migration `0025`): 1 when the check's admission opened its run, 0 when it was handed a run that already existed for the same read, input, and person under rule 6, another watch check or the person's own call ([decision 0285](../decisions/0285-two-watches-of-the-same-read-for-one-person-share-a-run-and-ending-one-never-cancels-a-run-another-rests-on.md)). A trigger lets a check open only a run no check names yet; a row from before the migration holds 1. One check per due time (unique index). Written when it begins and once when it ends, then never again; `opened_run` never changes.

`watch_changes` is one row per change, in one sequence: `confirmed`, `added`, `changed`, `removed`, `unseen`, `gap`, `paused`, `resumed`, `ended`, `condition_met` (migration `0026`: the condition held on the check it names, a check of the same watch (trigger); the watch's `ended`, with the reason `condition_met`, follows in the same transaction); the check that found it and the one that last saw the record before; the record's key as the workspace's keyed digest under the purpose `record_key` (as a record row names it), or `whole`; for `changed`, the names of the outputs that differ; for a gap, the class and code of the failure that made it (a failed check, a fault, a check that ending the watch closed, with `watch_ended`, or `watch_due_missed`); for a pause or an end, its `reason`; for due times missed, the first, the last, and how many. Never changed (trigger).

`watch_baseline` is the working state a check is compared with, one row per record the watch has seen: the keyed digest of its compared values together and of each output, under the purpose `watch_value`, with the watch and the record's key inside what is digested, so that equal values of two records or two watches never share a digest; and the check that last saw it. The records are worked out by the ledger from the check run's own stored result and `record_shape`, never handed to it. A newer check replaces the digests of the records it saw; a record proven removed is deleted; the whole baseline is deleted when the watch ends, and a trigger refuses a row for a watch with no check under way.

Kept, and for how long: the watch, its checks, and its changes are kept for the retention class the watch was asked with, the class its read's runs take, and leave together when the retention job marks the watch `purging` (no such job is built, for runs or watches). The baseline lasts only while the watch is open. Nothing a device reported and no personal data a system holds is in any of them: a digest under the workspace's key cannot be reversed, and cannot be checked against a guess without the key. The principals who asked and confirmed are kept, as on every run.

## audit

Unchanged from the technical reference: an append-only, hash-chained log of who did what. Ledger events are evidence about runs; the audit log is the record of decisions and administration.
