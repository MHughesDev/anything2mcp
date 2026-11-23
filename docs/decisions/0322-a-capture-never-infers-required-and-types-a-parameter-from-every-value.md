# 0322. A capture never infers `required`, and types a parameter from every value

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Adds to [decision 0308](0308-the-first-capture-records-reads-only-in-the-engines-own-browser-and-drafts-to-the-holding-folder.md), whose points stand.

**Date:** 2026-11-23

**Changes:** endpoint inference in [`a2m-pack-web-probe`](../architecture/crates/a2m-pack-web-probe.md) (its design and its built tier one and shape merge are to follow this record, in the package that next works on them); the held draft's question of decision 0308 point 4; the proof of package E2.

## Context

[Research note 36](../research/36-inferring-endpoints-from-recorded-traffic.md) read the source and documents of the open tools that turn recorded traffic into an interface description, through a fetch tool that summarises. What it found that bears on the design: the tools that mark a field `required` mean "present in every sample", and one of them says in its own text that this does not establish a contract; one tool types a parameter from its first value; the tools take an object whose keys are all identifiers as a dictionary; the one published error rate for traffic-based discovery is high precision and low recall, from a search summary only, and is not built on here. The one tool that links requests to each other uses a model, which rule 1 forbids at run time and which this design does not use at authoring time either.

## Decision

1. **No `required` is inferred from a capture.** A field seen in every answer is recorded as `seen_in_all`, with the number of answers; a field seen in some is optional, with limitation `sampled`, as now. The count of answers is kept.
2. **A parameter's type comes from every value seen, never from the first.** A parameter seen with one value is typed `text`, and the finding says so.
3. **The held draft asks two more questions** (decision 0308 point 4): which of the drafted parameters may be left out, and whether the list this read returns has more pages. Pagination is asked, never assumed.
4. **A short number is not an identifier by format alone.** Tier one makes a run of digits a parameter only when it is at least four characters long; a shorter one is a parameter only when tier two finds it varying, and stays literal otherwise, listed as a limitation. Four is a starting value for the first test to tune, not a measured figure. Decision 0308 point 5, that a placeholder is drafted only from two requests differing in that part, is unchanged.
5. **A plain word is not a parameter because it varied.** Tier two makes a varying segment a parameter only when its values are not all words that the endpoint's other captured paths also use as literal siblings. The finding says how many values were seen; two is the least evidence.
6. **A dictionary is found by its keys.** An object whose keys are all numbers or identifiers by format is a dictionary, kept as one entry of type `map`, whatever its size.
7. **Dependencies are found only by digest.** A request depends on an earlier answer only where the digest of one value is equal in both; no other evidence is taken, and no model is used.
8. **Inference is judged by what it misses.** The proof of package E2 states both the reads drafted that the software does not mean to be called, and the reads the test application's pages make that were not drafted.

## Alternatives

| Alternative | Why not |
| --- | --- |
| `required` as "present in every sample" | The tools that do it say it is not a contract; a person answers it in one question |
| Type from the first value | One value cannot tell a number from text that looks like one |
| Keep the 64-key test for a dictionary | It names the first sixty-four ids of a small list as fields |

## Proof

In the test application: a field present in every answer is `seen_in_all` with its count and not required; a two-digit segment that never varied stays literal; two literal sibling words that vary between captures stay literal; an object keyed by ids is one `map`; the held draft carries both new questions; E2's report states what was missed as well as what was wrong.
