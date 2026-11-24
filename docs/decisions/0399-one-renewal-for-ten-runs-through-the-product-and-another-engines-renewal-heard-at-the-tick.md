# 0399. One renewal for ten runs through the product, and another engine's renewal heard at the tick

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), in package 2.5. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the design of [`a2m-auth`](../architecture/crates/a2m-auth.md) (work item 4); the [roadmap](../plan/roadmap.md) row 2.5.

## Context

Package 2.5 had the keeper's one renewer proven in one process (`ten_runs_meet_an_expired_session_and_exactly_one_renewal_happens`), the count of attempts kept across a restart, four sign-ins an hour without the person, and the registry telling the keeper that a sign-in is the person's own (`SignInBy::Person` in `session.login`). Four proofs were left: ten runs through the engine against the test application, a wrong stored password and the application's lockout, the person's press against an agent's ask in the action registry, and two engines over one store.

## Decision

1. **The proofs are as the roadmap states them, and hold without a change to the product's code.** Through the product (`crates/anything2mcp/tests/renewal.rs`), ten reads at once after the application ended every session complete under one new session, and the application counts one sign-in form more. A wrong password the person typed is sent once; the application counts one failure; six reads and an agent's `session.login` after it send nothing, so the account the application locks after five failures is not locked. In the action registry, an agent's ask at the attempt limit stops at once with `credential_refused` and sends nothing; the person's press on the engine's page of the same sign-in is tried and holds.
2. **Two keepers over one store make one renewal; a waiter of the keeper that did not renew hears of it at its keeper's next tick.** The store's compare-and-set on the session's state and epoch lets one keeper start the renewal. The other keeper's calls wait on its own signal of change, which only its own work and its `tick` raise, so they read the store again at the next tick, which the kernel's tending task makes a few times a minute, and are then given the renewed epoch from the saved state. *Instead of* a signal that crosses processes, which the store does not offer and the local product does not need: one engine holds a store, by the store's lock on its data folder, so two keepers on one store arise only on a shared server.

## Proven

- `crates/anything2mcp/tests/renewal.rs`: `ten_runs_meet_an_expired_session_through_the_engine_and_the_application_counts_one_sign_in`, `a_wrong_stored_password_locks_nothing_the_stand_counts_one_failure`.
- `a2m-actions` `a_persons_press_retries_a_credential_an_agents_ask_may_not`.
- `a2m-auth` `two_engines_over_one_store_make_one_renewal`: five runs on each of two keepers over one store, one refresh and no new sign-in, the other keeper's runs given epoch 2 after its tick. Without a tick nothing wakes them: a call looks at its own wait only when it is woken, and in the product the kernel's tending task ticks the keeper.

## Left

A renewal that a keeper of another process holds in `renewing` is ended as `interrupted` by this keeper's tick once it has lasted the work limit (ten minutes), as one interrupted by a crash is. On a shared server a renewal that takes that long is ended by the other engine; nothing is written twice, and the next call signs in again. The shared server's store decides whether it signals changes across processes.

## Revisit if

The shared server runs two engines over one store, or a renewal is found that takes longer than the work limit.
