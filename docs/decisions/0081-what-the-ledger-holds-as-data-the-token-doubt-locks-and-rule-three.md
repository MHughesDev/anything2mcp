# 0081. What the ledger holds as data: the token, doubt, locks, and rule 3

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** what the intent token carries; which moves a record may make; when a record is in doubt; what becomes of a lock; what rule 3 reads. Part of the open point of spec section 208 on failure classes and locks.

## Context

Spec section 208 states nine rules and says each is enforced where the data is. Building them forced choices the section leaves open.

## Decision

1. **The rules are held twice.** Each call of the ledger checks the rule and answers with a failure that names it. The triggers of the migrations refuse the same things whoever sends the statement. *Instead of:* checks in code alone, which another program with the file open would pass.
2. **The intent token** carries the run, the commit point (step and record number), the record key, the lease number, the intended digest, the idempotency token, and the number of its intent event. It is made only by `Store::store_intents`, after the commit. `mark_acted` consumes it and refuses a record that is not in `intent` under that lease and that event. **A token rebuilt from an acknowledgement is not offered yet**: it is a public way to make a token, and it waits for the runner link that needs it.
3. **The idempotency token** is 32 hexadecimal characters of a digest over the run, the step, the record number, and the key's canonical bytes. It includes the commit point, so two commit points on one key do not share a token.
4. **A record's moves.** `planned` to `intent` or `failed`; `intent` to `acted`, `verified`, `planned`, or `failed`; `acted` to `verified`, `planned`, or `failed`; `failed` to `verified` or `planned`. `unchanged` is given when the record is planned, and it and `verified` are final. The moves back to `planned` and from `intent` or `failed` to `verified` are made only by `settle`.
5. **In doubt** means `intent`, `acted`, or `failed` after an intent with a class other than `policy`, `stale`, or `input`. While a record is in doubt from a lease below the current one, no intent is stored and the run cannot enter `running`: `possible_commit`, `reconcile_required`, a new code. A `failed` record that never had an intent is listed for reconcile, so it can be returned to `planned`, and does not block.
6. **Locks.** Released with `verified`, and with `failed` when nothing is in doubt. Quarantined with `failed` when the outcome is in doubt; and, for every record then in doubt, when the run moves to `stopped`, `needs_login`, or a final state, and when a new lease is granted. The run's other locks are released at those moments. A quarantined lock is confirmed only by its own run under a new lease. `settle` requires the lock held under the current lease.
7. **Rule 3** reads, of an approval: that it is of scope `run`, names this run, carries the run's diff hash, is not revoked, is marked used, and was changed in this transaction. Of a decision: that it names this run, is a `write_without_person` that was allowed, and carries the run's diff hash. A write run with `planned` records enters `running` only through `begin_writes`; one with none may enter freely and can never store an intent.
8. **`settle` with `Neither` succeeds** and answers `Settled::Stale`, leaving the record `failed` as `stale`, `changed_since_preview`. *Instead of:* returning that failure, which would take the caller's transaction back and lose what was just learned.
9. **Completed means every record is `verified` or `unchanged`**; the ledger refuses the move otherwise. `stopped` and `failed` carry the failure that ended the run.
10. **Three event kinds are added** to the thirteen of the ledger schema: `planned`, `diff_stored`, and `lock`, so that the state can be rebuilt from events alone. The engine's own events (`step_started`, `binding_chosen`, `need`) go through `append_event`, which refuses the kinds the ledger writes itself.
11. **The audit chain** hashes, for each entry, the workspace, its number, its moment, its four texts after redaction, and the hash before it. The first entry names 32 zero bytes. `verify` answers `Intact` with the head's hash, or `Broken` with the first number that does not follow.
12. **An open question** is `open`, `looking`, `blocked`, `settled`, or `out_of_scope`. Settling it needs at least one stored finding or stored blob.

## What follows

- The engine's table of failure classes and run states stays the engine's; the ledger takes the state it is given and sorts the locks by item 6.
- A run that ends final with a record in doubt keeps that lock quarantined until a person acts; the action does not exist yet.

## What would reopen it

The runner link, which needs a token on a machine without the store; or a system for which "in doubt" after `rejected` or `edge_blocked` proves too cautious to be workable.
