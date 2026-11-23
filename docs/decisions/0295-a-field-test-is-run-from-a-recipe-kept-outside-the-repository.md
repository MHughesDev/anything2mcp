# 0295. A field test is run from a recipe kept outside the repository, by a runner that names no software

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md).

**Date:** 2026-11-23

**Changes:** `tools/field_test.py` (new), [the field test results](../reviews/field-tests.md) (new), the [glossary](../reference/glossary.md), the [roadmap](../plan/roadmap.md).

## Context

[Decision 0293](0293-software-we-run-ourselves-is-tested-against-during-the-build-and-may-be-written-to.md) makes existing software a testing surface. Rule 9 keeps its name out of this repository. A test that cannot be repeated shows a fault once and then cannot show that it was fixed.

## Decision

1. **A field test is one run of the product against one copy of existing software.** It starts the software from a container image pinned by digest, loads invented data, runs the product's commands, reads the software's own store, and writes a report.
2. **The runner is in this repository and names no software.** `tools/field_test.py` reads a recipe and does what it says. It is Python, as the other tools in `tools/` are; nothing on the path of a run uses it.
3. **A recipe is kept outside this repository.** It names the image, the invented data, each command, and what each must show. The runner refuses a recipe folder inside the repository.
4. **The software's own store is the check.** A recipe reads the database file or the files the software keeps, directly and read-only. The product's own read is never the proof of a write, as with the stands.
5. **A known fault is a step marked `gap`.** The step expects what the product does today. When the product is fixed the step fails, and the recipe is changed to expect the right behaviour. A recipe therefore says, at every run, which known faults are still open.
6. **What a field test shows is written in [the field test results](../reviews/field-tests.md) by the kind of system,** and each fault becomes a case in a stand when it is fixed.

## What is not built

- **Faults on purpose.** A stand can lose an answer after a write was stored. Nothing yet does that to existing software. A proxy between the product and the software, which can drop, delay, or cut a connection, is the next part.
- **Three operating systems.** A field test runs where a container engine runs, which today is Linux. The stands still carry Windows and macOS.
- **A durable home for recipes.** They are in the lead's working folder and are handed to the owner as files. A private repository of the owner's is the lasting place.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Recipes in this repository | Rule 9 |
| A Rust test crate | The runner starts containers and reads files; it holds no engine logic, and a recipe changes far more often than the runner |
| The product's own test stands extended to wrap real software | A stand is depended on by product tests on three operating systems; a container is not available on the hosted Windows and macOS runners |
