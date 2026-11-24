# 0399. `auth.detect`'s questions are in its answer, and the unknowns ledger is the scout's

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), in package 2.7, from a finding of the [sessions-and-secrets review](../reviews/sessions-and-secrets-review-2026-11-24.md) (finding 3). The owner had not read this record when it was merged. The number 0399 is a placeholder the lead renumbers on merge.

**Date:** 2026-11-24

**Changes:** [section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md) ("Finding out how a target signs in"), which said that where no row matches the recommendation "opens a question in the unknowns ledger".

## Context

Section 215 said the table of recommendations opens a question in the unknowns ledger where no row matches. `auth.detect` is a read-only action: the catalog marks it `Observe`, and it writes nothing, so it cannot open a question in a table of the ledger. When the rules were built ([decision 0399 on the rules](0399-auth-detect-recommends-a-mode-and-a-provider-by-fixed-rules-and-marks-it-inferred.md)) the two sentences could not both stand.

## Decision

1. **`auth.detect` puts its questions in its answer and writes nothing.** The recommendation's `questions` and `unknown` hold what a person has to say: a sign no row covers, a 401 that names no scheme the rules know, an identity mode nothing shows, and a second factor, a passkey, or a link sent by mail that does not show before a sign-in is sent. A caller who asks again gets the same answer from the same findings.
2. **The unknowns ledger is the scout's.** The scout opens a question in the ledger for the stages it runs, once, whatever number of times it is run (it already does, `open_questions`). The scout's report carries each surface's `sign_in` entry, with the recommendation, as `auth.detect` gives it. Raising the questions of a recommendation in the ledger, so that `unknown.list` shows them beside the others, is not built, and the roadmap holds no package for it: no consumer has asked for it.
3. **Section 215 says so.** The sentence now says the answer holds a question instead of a guess.

## Alternatives weighed

| Alternative | Why not |
| --- | --- |
| Let `auth.detect` open the question in the ledger | It would make a read an action that writes, and the action's effect class and the policy over it would change for a convenience. A read that says it observes only must observe only |
| Leave section 215 as it was | It would promise a behaviour nothing builds, and the next agent would look for it |

## What would reopen it

A caller that works only from `unknown.list`, and so misses a question that only the answer of `auth.detect` holds.
