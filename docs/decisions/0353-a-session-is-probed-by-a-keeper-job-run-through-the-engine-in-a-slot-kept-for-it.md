# 0353. A session is probed by a keeper job run through the engine, in a slot kept for it

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** `a2m-actions` (the keeper's probe and the runner loop), `a2m-sched` (`Slots`), and `a2m-auth` (a probe's lease, and a probe that answers nothing). Package 2.4 of the [roadmap](../plan/roadmap.md).

## Context

[Section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md) says the keeper's probe runs "as an ordinary read through the engine, with a lease of purpose `probe`", and that keeper jobs have a reserved slot. Neither was built. `a2m-actions` answered every probe as unknown, so no probe ever ran; a session the system ended was found only when a run met the end, and the keeper treated an unknown answer as a failed check.

## Decision

1. **A probe is a run of the person's own read.** When the keeper probes a session, `a2m-actions` submits a run of the tool that runs the entry's `probe` operation, with the inputs the plan gives it (`probe_inputs`, [decision 0121](0121-the-plan-carries-what-the-run-time-needs-from-target-files.md)), as the session's own person, through the scheduler and the runner loop like any read. Its job is of kind `session_keepalive`; the runner hands the engine an ordinary `execute` job, and the job's control leases the session for purpose `Probe`. The engine's guard asks policy about the person, as for a read they call themself. The run's identity carries a purpose of its own, so it is never the person's own call of the same read under ledger rule 6. A probe's run that rests or is still under way when the wait ends is cancelled, so the next probe is a run of its own.
2. **The probe of a session is that of the published version a run last leased it under,** noted in memory as the lease is given. A session no run has leased since the engine started, a probe that is an entry of a pack's file kind, an operation that no tool of the plan runs, a session that is not a person's, a stopped runner loop, and a run that does not end within two minutes each answer `unknown`.
3. **How the run ended is the answer.** Completed is signed in. Resting at `needs_login`, or ended with a failure of class `session_expired`, is signed out. Anything else is unknown.
4. **A probe that answers unknown changes no state.** A valid session stays valid and unchecked, and the next probe is due a whole interval later, kept in memory. A suspect session is renewed as before: a driver's sign of expiry already stands against it, and that sign is not weighed again.
5. **A probe's lease never signs in and never waits.** It is given in `valid`, `suspect`, and `renewing`, as section 215 says; in any other state it is refused with `needs_login` at once, and starts nothing.
6. **The slot kept for the keeper.** `Slots` gains `keeper`, free slots that only a `session_keepalive` job takes. A keeper job takes one before an ordinary slot, and takes an ordinary one when none is kept free. The runner loop serves one kept slot with a claim loop of its own beside the ordinary one, so a run that holds every ordinary slot while it waits for a session's check never holds the slot the check needs.

## Why not

- **The probe through the keeper's worker, without the engine.** The worker would need a driver, and the probe would not be what the spec calls it: a read the engine runs as any read, under the same guard and evidence.
- **A probe's run with no ledger row.** Every run of the engine is fenced by its row and lease number; a run outside the ledger would be the one read nothing records.
- **One claim with both kinds of slot offered.** A claim waits for work. While it waits with only the kept slot free, a freed ordinary slot would sit unused until the wait ended, and ending a claim early can drop leases it was granted. Two loops each wait on their own slots.
- **Treating unknown as a failed check, as before.** A probe that could not be made would start a renewal, and for a stored credential a sign-in, on no evidence.

## What the work showed

The first test of the probe as specified found that a probe's lease, which section 215 grants in three states, fell through in every other state to the rules of a run, and could sign in with a stored credential. Point 5 closes that.

The roadmap's test was named for a session that ends `expired`. With a provider that needs the person, as a sign-in in a window does, the keeper's ladder ends in `needs_person`, which a call meets as `needs_login` as section 215's table says; the tests say `needs_person`. A provider that works from a stored credential is renewed with it, which is package 2.5.

## Proof

- `a_probe_that_finds_the_session_ended_moves_it_to_needs_person_and_the_next_call_returns_needs_login`, `a_probe_runs_once_per_half_idle_and_stops_after_the_keep_alive_window`, and `a_probe_that_cannot_run_changes_nothing`, in `crates/a2m-actions/src/tests/probe.rs`, on the kernel's manual clock.
- `a_keeper_job_takes_the_slot_kept_for_it_and_no_other_job_ever_does` in `crates/a2m-sched/tests/sched.rs`.
- `a_probe_that_could_not_be_made_changes_nothing_and_a_probes_lease_never_signs_in` in `crates/a2m-auth/src/tests/keeper.rs`.
- `an_expired_session_returns_needs_login` in `crates/anything2mcp/tests/expired_session.rs`: a person signs in in a window, a `ui` read works, the test application ends every session, and the next read rests at `needs_login` while the application counts no sign-in after the person's own.

## Revisit if

A runner on another machine serves sessions, when the probe becomes a job addressed to the session's home runner; or a probe that is an entry of a pack's file kind is built.

## Spec sections

- [215. Sessions and the session keeper](../spec/2-kernel/215-sessions-and-the-session-keeper.md)
- [`a2m-actions`](../architecture/crates/a2m-actions.md), [`a2m-sched`](../architecture/crates/a2m-sched.md), [`a2m-auth`](../architecture/crates/a2m-auth.md)
