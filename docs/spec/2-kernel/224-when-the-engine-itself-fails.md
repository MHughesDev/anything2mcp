# 224. When the engine itself fails

Other sections cover what happens when the target system misbehaves, a session ends, or a runner is lost. This section covers Anything2MCP's own parts failing: its database, its key store, its disk, and the subsystems its packs start, such as the web pack's browser. The rule is the same throughout, and each row below is a test.

## Three rules

1. **No ledger, no write.** If an intent cannot be stored, the write does not start.
2. **Nothing unverified runs.** A plan, policy set, or binary that cannot be checked against its hash or signature is not used.
3. **Reads degrade before writes do, and both fail closed.** Under pressure the engine gives up evidence detail, then capacity, then reads, and writes first of all. It never gives up a check.

## Health states

| State | Meaning | Calls |
| --- | --- | --- |
| `ok` | Everything works | All |
| `degraded` | Something needed for writes is impaired | Reads only; writes return `busy` with the reason |
| `stopped` | Something needed for any call is impaired | None; calls return the reason |

`anything2mcp engine status`, the health endpoint on a server, and the admin console show the state and its cause.

## Failures and responses

| What fails | During a run | New calls | Recovery |
| --- | --- | --- | --- |
| The database cannot commit: full disk, lost connection | A write stops before its next intent. If a write was sent and cannot be recorded, the runner journals it locally and stops; the run is `possible_commit` | `stopped` | When the database returns, journals are uploaded and doubtful records reconciled |
| The evidence store is full | Writes stop, because a record cannot be marked verified without its evidence | `degraded`; reads continue at the lowest capture level | Free space or retention; nothing is lost |
| The key store or key service is unreachable | Sessions already in memory keep working | No session can be restored or saved; calls needing one wait, then fail | Automatic when it returns |
| In the web pack, the browser crashes | Reads retry on a new browser. A write that had been sent is `possible_commit` | Continue | The browser is relaunched from the same profile, so the sign-in usually survives |
| In the web pack, a tab hangs | The step times out with its declared class | Continue | The tab is closed; the browser is recycled after repeated hangs |
| A provider's code panics in a step | The step fails by name: `unknown_variant` for a step that commits nothing, `possible_commit` for a save, because nothing says whether the write left. The driver's context is not used again | Continue | A save is reconciled on resume |
| A call from the engine to the control plane is never answered | The engine gives the call up after its time limit. An intent that was not acknowledged sends nothing; a write whose record was not acknowledged is `possible_commit` | Continue | Resume reconciles first |
| Memory runs short | No new browser work, or other memory-heavy subsystem work, is started; queued runs wait | `busy` with a retry time | In the web pack, the browser is recycled between runs once past a memory limit |
| The engine process dies | Runs in flight are interrupted | The next client starts a new engine | Reconcile, then resume, as in section 208 |
| The machine's clock jumps | Leases are unaffected; they are measured on a steady clock | Approvals and tokens are judged by the database's or issuer's clock, with a small tolerance | None needed |
| A plan fails its hash check | The tool is not run | That tool is unavailable; others are unaffected | Republish from stored sources |
| A new policy version fails to load | The previous version stays in force | Unchanged | Fix and publish again. With no valid version at all, everything is denied |
| The identity provider is down | Running work continues on existing sessions | Server callers cannot be authenticated and are refused; local use is unaffected | Automatic |
| A migration is interrupted | Not applicable | The engine refuses to start on a half-migrated store | Migrations are transactional where the database allows; a backup is taken first and restored otherwise |
| The store is corrupt | Not applicable | `stopped`; an integrity check runs at start | Restore from backup; every run in flight at the backup's time is reconciled |

## Limits that prevent failures

- In the web pack, a browser is recycled after a set number of runs or past a memory limit, always between runs.
- Queues have a length; a full queue answers `busy` at once instead of growing.
- Evidence has retention classes and a size budget per run.
- Disk space is checked before a pipeline starts, and a pipeline that could not store its evidence is not started.

## Rolling out a new version of a target

A bad target version is the likeliest self-inflicted failure, so a published version does not take over at once.

1. **Published.** The version exists and has passed its gates. Nothing uses it yet.
2. **Candidate.** For read operations, calls are answered by the current version while the candidate runs in shadow and its results are compared. Named callers can opt in to the candidate outright.
3. **Current.** After a set number of matching reads, or at once if a publisher says so, the toolset's pin moves to the new version. Writes switch only at this step.
4. **Rollback.** One action moves the pin back. Runs in flight finish on the version they started with.

A toolset always names the exact version it uses, so two toolsets can be on different versions of the same target.

## Proving it

Each row of the failure table is a fault-injection test: the fault is induced against the built-in test application, and the test asserts the health state, what callers were told, and that the test application's stored data shows no duplicate and no unverified record. The tests for the database, the browser, and process death arrive with Phase 3; the rest with Phase 9.
