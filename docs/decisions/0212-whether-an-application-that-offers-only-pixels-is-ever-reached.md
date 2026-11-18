# 0212. Whether an application that offers only pixels is ever reached

**Status:** Accepted in the form its recommendation gave, by owner decision [0242](0242-the-owner-takes-the-recommended-answer-to-every-open-question.md) of 2026-11-18: an application that offers only pixels is not reached at run time; the scout may keep a screenshot as build-time evidence only. Only the owner changes it.

**Date:** 2026-11-18

**Asks about:** rules one and seven of [AGENTS.md](../../AGENTS.md#rules-that-never-bend); [decision 0003](0003-no-ai-at-run-time.md); [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md).

## Context

Some software on a person's computer offers nothing but pixels: a remote desktop session, a program drawn by a toolkit that publishes no accessibility tree, a game, a virtual machine's console. Decision 0208 asks for whatever the computer can reach. The documents disagreed on these: [section 102](../spec/1-product/102-requirements-and-guarantees.md) said "until a later vision mode is justified", [section 704](../spec/7-project/704-readiness.md) said "not planned", and section 106 refuses. The review pointed all three at this record.

The rules bear in two ways. Rule one: nothing in a run may call a model; reading text from a picture is usually done by a learned model. Rule seven: unknown means stop; a picture that differs by one pixel from the one recorded is a variant with no rule. And clicking by position cannot be bound to the element it was meant for, the reason section 106 gives.

## What it costs

Little, measured by the [coverage map](../reviews/coverage-map.md): most applications that look pixel-only to an agent have another surface (an accessibility tree, files, a local database, a command-line tool, a scripting interface). What stays unreachable is a remote desktop session, a program with no accessibility data and no other surface, and a console of a machine that is not running.

## The safest way through that stays inside the rules

None that reaches the same applications. The nearest:

- A read-only recognition with no learned model: a declared region of a declared window compared, after a fixed normalization, with a recorded image, to tell which of a closed list of states a screen is in. It is deterministic and can stop on any variant. It cannot read a value or make a write, so it adds little but a guard.
- A screenshot taken at build time, as evidence for the scout, never as a step of a run ([research note 27](../research/27-the-public-landscape-of-agent-tools.md), question 3).

## The questions for the owner

1. Are applications that offer only pixels out of scope for good, or only for the first release?
2. If not for good: is a fixed, non-learned recognizer (exact comparison with recorded images, or glyph matching of a declared font) "a model" under rule one?
3. May the scout take a screenshot of a window as build-time evidence, kept redacted like any capture?

## Recommendation

Out of scope for the first release, and section 106's refusal stands. Answer 3 yes, since it changes nothing at run time. Revisit 1 and 2 only if a measured count of real applications with no other surface shows the gap matters.

## Alternatives weighed

- A vision mode behind a setting. Refused: a setting cannot loosen a rule ([section 226](../spec/2-kernel/226-configuration.md)).
