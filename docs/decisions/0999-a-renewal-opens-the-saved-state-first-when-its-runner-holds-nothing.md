# 0999. A renewal opens the saved state first when its runner holds nothing

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the session keeper in [`a2m-auth`](../architecture/crates/a2m-auth.md) (its renewal ladder, and the open point on a renewal after a restart); the proof of [decision 0361](0361-one-renewal-for-ten-runs-through-the-product-and-another-engines-renewal-heard-at-the-tick.md), which is kept and made true.

## Context

The test `two_engines_over_one_store_make_one_renewal` failed in about one run in four, with or without load on the machine: the wait for one refresh with ten calls waiting never ended. In every failing run the second engine had won the store's compare-and-set and started the renewal. Its runner did not hold the session's credentials, which the first engine's runner had signed in, so the silent rung found nothing to refresh, answered `needs_person`, and the session was put to its person, with no refresh at all. The same path is reached by one engine alone: after a restart its runner holds nothing, and a lease that finds the credentials due starts the renewal before it restores the saved state. The design of `a2m-auth` named that as an open point ([decision 0374](0374-a-sessions-mark-that-it-was-made-under-drafts-is-kept-in-its-record.md), consequences).

The test had a second fault of its own: it took for granted that the first engine renews, and waited for that engine's runs without a tick. When the second engine renewed, the first engine's runs heard of it only at their keeper's tick, which the test never made.

## Decision

1. **Before its silent or light rung, a renewal opens the session's saved state on the keeper's own runner when that runner does not hold the credentials at the session's epoch.** It is the restore a call makes after a restart, sent as `Work::Restore` through the keeper's work and noted as `restored` with the cause `renewal`, only while the epoch is still the one the state was saved at. When the session was renewed or ended meanwhile, what was opened is forgotten again and the renewal ends with nothing written. Saved state that is missing or does not open leaves the ladder as it was, so its rungs that need credentials want the person, as before. The drafts' gate of decision 0248 is checked before this, so nothing is opened for a renewal that may not run without its person. *Instead of* restoring in `lease` before the check that the credentials are due: that covers a restart, but not a renewal started by a driver's report or by a failed probe, and the ladder is the one place every renewal passes.
2. **The two-engine test no longer assumes which engine renews.** It waits for both keepers to be idle, learns from the first engine's count of renewals which one renewed, takes the renewer's runs without a tick, checks that the other engine's five runs still wait, and ticks that engine. Every assertion it had is kept.

## Proven

- `an_engine_that_did_not_sign_in_renews_from_the_saved_state` and `a_restarted_engine_renews_an_expired_session_from_the_saved_state`: one refresh, no new sign-in, epoch 2, no prompt and no ask of the person. Both fail without point 1, with `needs_login`.
- `two_engines_over_one_store_make_one_renewal`: before, 13 failures in 50 runs alone and 9 in 100 with every core busy and the rest of the crate's tests looping beside it; after, 200 of 200 pass in each, with the second engine the renewer in 38 and 7 of them.

## Left

A home runner other than the local engine is not built; for a keeper with no local worker the keeper cannot see what the runner holds, and a renewal there goes on as before.

## Revisit if

A home runner other than the local engine is built, or a provider's refresh is found that must not be made from credentials restored from saved state.
