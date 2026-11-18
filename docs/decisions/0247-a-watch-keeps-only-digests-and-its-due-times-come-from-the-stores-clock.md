# 0247. A watch keeps only digests, and its due times come from the store's clock

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Accepts** [decision 0234](0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md), which was Proposed, by building its ledger's and its scheduler's halves, and refines it where the points below say so. What needs the action registry, the engine, `a2m-core`, and the doors waits for them, as the halves of [decision 0207](0207-a-shadow-binding-reads-beside-the-verified-one-and-a-person-promotes-it-on-a-run-of-exact-matches.md) waited for [decision 0219](0219-shadow-bindings-reach-the-product-and-the-first-hour-is-mended.md).

**Changes:** the designs of [`a2m-ledger`](../architecture/crates/a2m-ledger.md) and [`a2m-sched`](../architecture/crates/a2m-sched.md); the [ledger schema](../reference/ledger-schema.md). Migration `0021-ledger-watches-checks-and-changes.sql`, the next free number.

## Plan, stated before the build

The slice: what a watch is, each check, each change, and the digests a check is compared with, in the ledger, with the rules that make a watch unable to write, to run before its person confirms it, to run twice at once, or to change what it reads; the comparison as a pure function; and in the scheduler, a watch's due times, its gaps, its admission as the lowest work, and its share of the budget. All kernel, no pack. The hypothesis it tests is the one decision 0234 proposes for the register (that a digest of normalized values, compared across repeated reads, reports what changed and nothing across quiet checks), here only against the stand of the ledger's tests, with an output that moves on every read left out by `ignore`. The alternative weighed first was decision 0234's own comparison, check against the check before; the table says why it was not taken.

## Decision

| Part | Built | Proven by |
| --- | --- | --- |
| The watch | `watches`: the read and its version, the fixed input's digest and stored input, the outputs left out, the interval, the end, who asked, whose watch it is (`acts_for`), who confirmed it and when, the phase, the state and its reason. `Tx::watches()` gives `Watches`: `ask`, `confirm`, `begin_check`, `end_check`, `missed`, `defer`, `pause`, `resume`, `end`, `version_changed`, and the reads `get`, `list`, `not_ended`, `open`, `check`, `checks`, `check_of_run`, `changes`, `baseline`, `key_of` | `a2m-ledger/tests/watch.rs`, through the calls and a second connection |
| Each check | `watch_checks`: its due time's index and moment, whether it was the catch-up, its run, when it began and ended, `complete`, `cut_short`, `failed`, or `faulted`, the class and code of a failure, the count of records seen and of each kind of change | The same |
| Each change | `watch_changes`, append-only: `confirmed`, `added`, `changed` (with the names of the outputs that differ), `removed`, `unseen`, `gap`, `paused`, `resumed`, `ended`; the record by the keyed digest of its key; the check that found it and the one that last saw the record | The same |
| The baseline | `watch_baseline`: per record seen, the keyed digest of its values together and of each output, and the check that last saw it | The same |
| The comparison | `compare(&Baseline, &CheckDigests) -> Comparison`, pure; `CheckDigests::new` refuses a key twice (`unknown_variant`/`watch_key_repeated`) and a whole result beside keyed records (`watch_shapes_mixed`); `RecordDigests::of` digests normalized values under [`value_purpose`](../architecture/crates/a2m-ledger.md) | Unit tests in `watch_compare.rs`: a check compared with itself reports nothing; the order of records does not matter; a repeated key is a named fault, over generated checks |
| Due times | `WatchLimits` (in `SchedSettings::watch`), `due_at`, `plan`; `Scheduler::ask_watch`, `confirm_watch`, `due_watches`, `next_watch_due`, `submit_check` | `a2m-sched/tests/watch.rs` on a clock that moves only when told |
| Admission | A check is `batch`, whatever its submission says; it writes nothing; a refusal to wait out (hold, full queue, spent budget, no runner) keeps nothing and defers the watch | The same |
| The budget | A check's grant leaves `reserve_percent` (50) of each bucket's capacity; `WatchLimits::fits_budget` keeps all watches on one surface within `budget_share_percent` (10) of its sustained rate | The same |

### The schedule's form, and the floor

The schedule is one interval, a whole number of seconds, from the moment of confirmation. Due time `n` is `confirmed_at + phase + n × interval`. No calendar form is built (Choice, this record): without named time zones, which the breadth review lists as not built, a "daily at 09:00" is the interval of a day from a confirmation at 09:00 in UTC, and nothing else is cheap and exact; a calendar form waits for time zones.

The floor is `watch.shortest_interval_seconds`, 60 by default, and the type never lets it fall below 60 (`SHORTEST_INTERVAL_FLOOR`), whatever the setting says: a setting may raise a floor and never lower it (spec section 226, principle 7). The reason is the review's and decision 0234's: a watch acts while the person is away, so it must never become a fast poll of a system, and a minute is the shortest interval at which a person is told of a change sooner than they would ask. A watch also keeps to the surface's budget: all watches on one surface take at most a tenth of its sustained rate (checked by `fits_budget`, which the registry calls at confirmation), and a check's grant never takes a bucket below half its capacity, so a caller's own reads always find half the burst and nine tenths of the sustained rate.

The phase spreads watches: it is made from the digest of the watch's identifier, below the smaller of a tenth of the interval and five minutes. Decision 0234 says due times never drift and nothing in them is random; that holds, since the phase is fixed at confirmation and the same on every machine.

### Points where this refines decision 0234

| Point | Decision 0234 | Built | Why |
| --- | --- | --- | --- |
| What a check is compared with | The previous check | A baseline: for each record ever seen, its digests when last seen. A check updates what it saw; a record proven removed leaves | Against the previous check, a list cut short makes every record out of its view `added` again when it comes back, and reports `unseen` at every check after. Both break the hypothesis that quiet checks record nothing |
| `unseen` | Each check in which a key is not seen | Once, in the check right after the last one that saw it | The same reason |
| `removed` | Both checks complete | The newer check complete, and a complete check in the baseline | The baseline's own form of "both complete" |
| What a change holds of the record | Its key | The workspace's keyed digest of its key under the purpose `record_key`, the name a record row and a lock give it. A door finds the record in the check run's result with `Watches::key_of` | A key is often business data (an order number); a change holds no value |
| States | `proposed`, `waiting_for_sign_in` | `asked`, `confirmed`, `running`, `paused`, `ended`. A pause or an end has a reason, `WatchReason`, a state of the watch and not a failure code: `needs_login`, `needs_input`, `drifted`, `version_changed`, `on_hold`, `paused_by_person`; `ended_by_person`, `ended_by_caller`, `end_reached`, `version_withdrawn`, `declined`. A pause a check's failure caused keeps that failure's class and code beside its reason | One paused state with a reason covers every wait; `running` is the guard against two checks at once. A reason is not a failure, so it is not a `FailureCode`, and the codes `needs_login` and `on_hold` stay declared once, where they are failures |
| Whose watch | Implied: the person | `asked_by` (the caller) and `acts_for` (the person whose session is used); only `acts_for` confirms, and a check's run must be that person's | "A user's session is theirs alone" |
| The longest | From confirmation | From the ask, which is earlier, so the same or shorter | The end is fixed when the watch is asked, before the person sees it |
| How a check ends | | Read from its run's row, never from the caller: `completed` is compared; `needs_login`, `needs_input`, or a class `session_expired` or `drift` pause the watch; any other end is a `gap` with the run's class and code, or `busy`/`watch_check_not_completed` when it gave none | As the ledger takes an acknowledgement from the run's stored diff (decision 0172) |
| A changed version | Drift pauses | A new published version of the target pauses every confirmed watch tied to another (`version_changed`), and such a pause, like a drift's, never resumes: only a new watch its person confirms | A watch is tied to the plan its person confirmed |
| The condition | An expression that ends the watch | Not built: it needs `a2m-expr` in the registry; when built, it ends the watch with the reason `condition_met` | |

### Each ledger rule, and why a watch keeps it

| Rule | Why it holds |
| --- | --- |
| 1. Intent before action | A watch writes nothing. Its checks are runs of a read, and a read has no records (`plan_records` refuses one), so no record is ever acted |
| 2. Proof before verified | No record of a watch exists to be verified |
| 3. Approval bound to the diff | A watch is made only of a read (`effect = 'read'`, a check constraint) and a check only of its own read run (a trigger). A confirmation is a person's act on a watch, kept in `watches`; it clears no write and is in no table rule 3 reads |
| 4. One writer per run | A check's run is changed only by the engine under its lease. The ledger reads the run's row to end a check and changes nothing in it. A watch has one check under way at most, and only that check may end (`end_check` and the `running` trigger) |
| 5. Reconcile after any doubt | A read holds nothing in doubt |
| 6. One run per identical call | A check's run is opened by the scheduler's admission, through `open_run`. When the same person has the same read with the same input unfinished, the check takes that run, as any identical call does |
| 7. One lock per record | A read takes no record lock |
| 8. Final means final | Nothing of a watch changes a run. An ended watch never changes (trigger); a check ends once (trigger) |
| 9. History is append-only | A change never changes; a check is written when it begins and once when it ends; a watch, its checks, and its changes leave only with the retention job. The baseline is working state, not history, and changes only while a check is under way |

The nine rule tests in `tests/rules.rs` still pass.

### Failure codes

Each code is declared once, with its class, in a table a program reads: the ledger's thirteen in `WATCH_CODES` of the `codes` module of `a2m-ledger` (`watch_not_a_read`, `watch_not_confirmed`, `watch_not_yours`, `watch_ended`, `watch_run_not_its_own`, `watch_needs_confirmation`: `policy`; `watch_check_running`, `watch_paused`, `watch_check_unfinished`, `watch_due_missed`, `watch_check_not_completed`: `busy`; `watch_key_repeated`, `watch_shapes_mixed`: `unknown_variant`), the scheduler's four in `WATCH_CODES` of the `codes` module of `a2m-sched` (`watch_interval_too_short`, `watch_too_long`: `input`; `watch_too_many`, `watch_over_budget_share`: `busy`). No code is in both; a test in each crate holds its table to its constants.

### Retention

Kept: the watch, its checks, and its changes, for the retention class the watch was asked with (its read's runs' class), until the retention job marks the watch `purging`; that job is not built for runs or watches. The baseline: only while the watch is open; ending deletes it, and a newer check replaces the digests of each record it saw. No normalized value is kept in any of the four tables, and so nothing a device reported and no personal data a system holds: a keyed digest cannot be reversed or checked against a guess without the workspace's key. The principals who asked and confirmed are kept, as on every run.

## Alternatives weighed

| Question | Taken | Weighed and not taken |
| --- | --- | --- |
| What a change shows | Kinds, key digests, output names; the values are read from the check's run | Storing each check's values to show a diff: a second copy of the system's data with a lifetime of its own (section 401), for what the check's run result already holds |
| Who digests | The ledger, inside the transaction that ends the check, with the vault's keyed digest it already holds | The engine: it has no key and never opens the store. A plain digest: a reader of the store could confirm a guessed value |
| The purpose of a value's digest | `value_purpose()`, which returns `restricted_value` today, with a domain prefix inside every digest; one line changes when `a2m-core` has `watch_value` | A digest with no purpose of its own: none exists in `a2m-core`, which this package does not own |
| The lowest priority | `batch`, the lowest the queue has | A third priority below batch: `jobs.priority` has a check constraint of two values, and a rebuilt table is a migration of its own. Not yet |
| The job kind of a check | `canary`, which the engine already refuses for a tool that writes | A kind of its own: `jobs.kind` has a check constraint, so a rebuilt table again. Not yet |
| Jitter | A phase made from the identifier | A random offset at each due time: not reproducible, and decision 0234 forbids it |
| The baseline | Merged, record by record | The previous check alone (the table above) |
| Property tests | A small generator of our own, the same on every machine | An outside crate: none is in the workspace, and the three properties need no shrinking |

## Consequences

- Hypothesis: the register owes a row for decision 0234's bet, under the next free number. This slice showed that against the previous check alone the bet fails for any list cut short, and holds with the baseline; it has not met a test stand.
- Not built here, and owed by other crates: the actions and the person's page (`a2m-actions`), the check's job in the engine, the setting keys (section 226), a purpose `watch_value` in `a2m-core`, a `WatchId` in `a2m-core`, the condition, the MCP resource and its subscription, the REST stream, `run_steps`-style retention.
- An engine with a confirmed watch has work: `next_watch_due` answers its next due time, which the sweeping task must wait on beside `next_due`.
