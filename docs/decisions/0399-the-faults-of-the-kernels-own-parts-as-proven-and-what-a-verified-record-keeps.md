# 0399. The faults of the kernel's own parts as proven, and what a verified record keeps

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), in packages 3.10 and 3.7. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** [spec section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md), rows for the key store, a new policy version, and a migration; the designs of [`a2m-ledger`](../architecture/crates/a2m-ledger.md) (`ReadBack` and migrations), [`a2m-policy`](../architecture/crates/a2m-policy.md), and [`a2m-auth`](../architecture/crates/a2m-auth.md); the [roadmap](../plan/roadmap.md) rows 3.7 and 3.10.

## Context

Package 3.10 makes each row of the failure table in section 224 a test, crate by crate, and through the product against the test stand (`crates/anything2mcp/tests/faults.rs`). Three rows did not say what the code does, and one did not hold. Package 3.7's seeded kill test, through the real program, found a run killed after its record was verified that no resume completed.

## Decision

1. **A half-migrated store is refused, by two checks.** A store holds the first migrations this engine carries, in order, and no others: one that holds a later migration without an earlier one is refused. Before a migration is applied, the store holds no table, index, trigger, or view that no recorded migration made: one that does was half migrated outside this engine, or changed by another program, and is refused. Both are `busy`/`store_version_unsupported`, and nothing is applied. An interruption inside this engine leaves no half: each migration runs in one transaction with the row that records it, so the next start applies it whole. *Instead of:* applying every migration not recorded, which re-ran a migration over the objects it had already made.
2. **A new policy version that fails to load is never stored, and the previous one stays in force.** A set whose condition does not compile, or whose test table fails, is refused at publish, and every decision after it is made by the version before, by its digest. A newest stored version that later cannot be read, written by another program, still denies everything (decision 0083): the engine does not fall back to an older version, because the newer one may have taken a grant away, and falling back would loosen the policy on a fault. The row "with no valid version at all, everything is denied" is read as "with no valid newest version".
3. **An unreachable key store fails a call that needs it at once, by name; it does not wait.** Sessions and keys already open in memory keep working. A saved session that must be opened, or an item sealed under a key not yet open, fails `busy`/`key_store_unreachable`; nothing is signed in, sealed under a new key, or thrown away in its place; and the next call after the store answers again restores the session. *Instead of* a wait inside the call: a locked key store is opened by a person, and an agent kept waiting learns nothing it is not told at once. "Calls needing one wait, then fail" is now "calls needing one fail by name, and succeed once it answers".
4. **A record verified by its own read-back keeps the outputs the read-back showed.** `ReadBack` gains `outputs`, a `Values` blob of what the record's steps observed and what the read-back read; `Ledger::verify` keeps it in place of the outputs stored with the `acted` mark, and `None` keeps those. A resume hands back the outputs the run gave before it was killed. *Before:* only the `acted` outputs were kept, so a resume of a run killed between `verified` and `completed` stopped with `output_unexpected` on an output that only the read-back gave, for ever.
5. **A stopped run takes a new failure.** `stopped` may move to `stopped`, with the failure that stopped it again, and the action registry records it when it differs from the one the run shows. *Before:* a run that a later lease stopped for another reason went on showing `runner_lost`, the first failure, and the reason the resume could not go on was in no record.

## Proven

- `a2m-ledger` `tests/store.rs`: `a_half_migrated_store_is_refused_and_left_as_it_was`, `a_damaged_database_file_fails_the_integrity_check_at_start`; the outputs a read-back gave survive a restart; a stopped run takes a new failure. The nine rules of section 208 (`tests/rules.rs`) still pass.
- `a2m-policy` `a_new_version_that_fails_to_load_leaves_the_previous_one_in_force`.
- `a2m-vault` `an_unreachable_key_store_stops_what_needs_it_and_nothing_else_and_recovers_by_itself`; `a2m-auth` `an_unreachable_key_store_keeps_sessions_in_memory_and_restores_none_until_it_answers`.
- `anything2mcp` `tests/random_kill.rs`, no longer ignored: seeds 0 to 59, the real program killed at a seeded step, never write twice and always end completed and verified.

## Left, with reasons

- **A start while the key store is unreachable** leaves the kernel `degraded` until the engine is started again (`a2m-actions` attaches the vault once, at start). Section 224 says recovery is automatic. That is the action registry's to change.
- **A session that ends between a write and its read-back** leaves the run `needs_login`, whose fix says "run the tool again or resume the run", not `possible_commit` with `retry_safe: false`. The resume after a sign-in reconciles first and writes nothing twice; the words told to the caller are the engine's to change.
- **A surface an edge blocks** stops a run as the HTTP pack's `unknown_variant`/`unexpected_status`, not `edge_blocked`: the product has no source of edge blocks yet (`a2m-actions` `runner.rs`). Nothing is written, and a block during a read-back leaves the record in doubt.
- **A connection lost after a write in the `ui` mode** is proven in the web pack's driver tests; no write in that mode is published in the product's test projects yet.

## Revisit if

A migration is shown that must be applied out of order; a policy fault is shown where denying everything is worse than the version before; a key store is found whose unlock comes by itself within a call's wait; or a read-back is shown that gives an output its record should not keep.
