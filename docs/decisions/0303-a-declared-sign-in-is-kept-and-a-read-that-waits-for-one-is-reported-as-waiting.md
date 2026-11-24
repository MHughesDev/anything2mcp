# 0303. A declared sign-in is kept, and a read that waits for one is reported as waiting

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** the scout's `try` stage and report in `a2m-actions`, the trial action's answer, [section 303](../spec/3-packs/303-the-http-pack.md), roadmap package FT9. The drafting rule for a declared sign-in does not change.

## Context

Field test 1, finding 11 ([results](../reviews/field-tests.md)): a system's published description declares a sign-in for the whole service. The drafter reads the operation's own `security`, then the service's, and where either asks for a credential the drafted endpoint takes the session's. The system in fact answers some of those reads with no credential. With no session, each of them stops with `needs_login` before anything is sent, and the report counts it among the reads that failed.

## Decision

1. **The drafter keeps what the description declares.** An endpoint is drafted `auth: none` only where the description says the operation takes no credential.
2. **A read is never sent without a credential to find out whether the system answers anyway.** A system may give a smaller answer to a caller it does not know, and a tool that returned that answer as the whole one would be wrong without saying so. Correct comes before easy.
3. **A person who knows an endpoint is open says so**, by writing `auth: none` on it. The review shows that line as a change a person made.
4. **A read that waits for a sign-in is not a failed read.** The scout's report lists it under `waiting`, apart from `proven` and from `failed`, with the surface to sign in to and the one action that does it. The trial action's answer for such a read names the same action. The counts of the report add up: drafted is proven, waiting, failed, and held.
5. **A sign-in to a described service with a token is the existing one.** The person types the token on the engine's own page, reached through `session.login`; it is stored in the vault and never an input of an action. No action that takes a credential is added.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Try each read with no credential first, and keep `auth: none` where the system answers | Point 2. It also sends requests the description says need a credential, which a system may count against an address |
| A third value, "send the credential when there is one" | The same tool would then return different answers on different days, by whether a session is there |
| Draft every read `auth: none` and let the system refuse | A refusal by the system is a response the product must then classify for every system; the description already says it |

## What this risks

A person meets a sign-in for a read that would have worked without one. The report tells them what to do in one line, and point 3 is the way around it.

## Proof

In the test application, a described service that declares a sign-in and has no session: the report lists its reads under `waiting`, none under `failed`, and the counts add up. In field test 1, after a sign-in with an invented token ([decision 0304](0304-a-field-build-stands-in-for-the-person-in-a-field-test.md)), the same reads return the stored values, and the token is in no result, log, or trace.
