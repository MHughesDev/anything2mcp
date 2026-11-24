# 0374. A session's mark that it was made under drafts is kept in its record, and a record that does not say reads as drafted

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), from finding 7 of the [sessions-and-secrets review](../reviews/sessions-and-secrets-review-2026-11-24.md). The owner had not read this record when it was merged. The number 0399 is a placeholder the lead renumbers on merge.

**Date:** 2026-11-24

**Changes:** the design of [`a2m-auth`](../architecture/crates/a2m-auth.md), and migration `0027` of `a2m-ledger`, which `a2m-auth` owns. Closes an item of the open list of [decision 0248](0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md).

## Context

Decision 0248 says a session whose last sign-in was declared by the target's drafts is never renewed without its person, under the drafts or under a published version. The session keeper held that mark in memory only. The local engine stops when idle and starts on demand, so after a restart a session made under drafts, restored from its saved state, was renewed silently by the first run under a published entry. The addresses were the published ones, which a person was shown when the version was published, so nothing went anywhere new; what was lost was the rule itself.

## Decision

1. **The mark is a column of the session's record.** Migration `0027` adds `signed_in_under` to `sessions`: `published` or `drafts`, held to those two words by a check. The keeper writes it in the same compare-and-set that moves a session to `valid` after a sign-in, so the mark and the credentials it is about are written together. A renewal keeps the mark it found; the renewal is refused before it starts when the mark says drafts, so a renewal never writes a published mark.
2. **Fail closed on reading.** Only the word `published` reads as published. Null, which every row kept before migration `0027` holds, reads as made under drafts, as does any other value. A session from such a store waits for its person at its first renewal; the person's sign-in under a published version writes `published`, and the session renews without anyone from then on. A new record, before any sign-in, is written as `drafts`.
3. **The refusal is the one the rule already gave.** A renewal of a session made under drafts stops with `session_expired`/`needs_login`, whose next step tells the person to sign in with `anything2mcp session login` or publish the target, and the session moves to `needs_person`. Nothing is sent.
4. **The allowances stay in memory.** The addresses a person allowed a session made under drafts are not kept in the record. After a restart there are none, so a run under drafts is refused until the person signs in again; that is already the closed side, and keeping the list would widen what a restored session may do without its person. Nothing about them needs to survive.

## Alternatives weighed

- **Keep the mark in the saved state's sealed envelope.** It would go with the credentials, but a session that is not movable has no envelope, and the decide half of the keeper does not open envelopes. A column is read where the decision is made.
- **Read a missing mark as published, to spare a person one sign-in after the upgrade.** It is the very gap the finding named, for every session in every store kept before the change. Rejected under rule 7.
- **Persist the allowances too.** See point 4.

## Consequences

- Tests in `crates/a2m-auth/src/tests/destinations.rs`: `a_session_made_under_drafts_is_not_renewed_under_a_published_version_after_a_restart`, `a_session_made_under_a_published_version_renews_after_a_restart`, and `a_store_from_before_the_mark_opens_and_its_sessions_count_as_made_under_drafts`, which takes the column and the migration's record out of a store file, opens it, and finds the session drafted and its renewal refused, with the provider counting no renewal. The tests use a provider that renews by signing in again, because that rung needs nothing held in memory.
- Found on the way and not changed here: after a restart, a lease that finds a valid session's credentials due starts the renewal before it restores the saved state, so a silent renewal (`refresh`), which needs the held material, finds nothing held and the session waits for its person. It is fail closed, and it costs a person a sign-in that restoring first would save. It is an open point of the design of `a2m-auth`.
- A person whose engine is upgraded signs in once more to each session that would otherwise have renewed silently.

## What would reopen it

A session made under drafts that must renew without its person, which decision 0248 forbids, or a store where the column cannot be added.
