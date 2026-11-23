# 0304. A field build stands in for the person in a field test

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** a new program in the workspace that is never packaged, `tools/field_test.py`, the packaging checks, roadmap packages FT4 and FT9. The product's program does not change.

## Context

A field test ([decision 0295](0295-a-field-test-is-run-from-a-recipe-kept-outside-the-repository.md)) runs the product's program from outside. Two things in the product need a person and are built so that a program cannot stand in: a write is approved at a presence prompt, and a credential is typed on the engine's own page. The program ships no presence prompt a test can pass, which is right. So today a field test cannot approve a write or sign in. The product's own tests do both by running the engine inside the test with a prompt of their own.

## Decision

1. **A field build is a second program, built from the product crate's library.** It is the product in every part but one: its presence prompt is the field prompt of point 2. It is a crate of the workspace with `publish = false`, outside the product crate. The crate is named, and added to the layout of section 605 and to the layering table, by the package that builds it (FT4).
2. **The field prompt passes only an approval the runner names exactly.** The runner writes the approval's own hash, as the approval page shows it, to a file in the field build's home folder; the prompt passes when that hash is the one it is asked about, and removes the file. Anything else is refused. So a field test still approves one stated change, and a run with no such file rests as the product does.
3. **The field build refuses to start outside a field test.** It starts only with a home folder that holds the runner's marker file, and it serves only targets whose every address is on this machine. Either check failing is a named stop.
4. **Signing in needs nothing new.** The runner opens the engine's own page for the sign-in and posts the invented credential to it, as a person's browser would. The step is `person` in a recipe, with `signs_in` or `approves`.
5. **The field build is never packaged.** The wheel and the npm package are built from the product crate alone, and the packaging check fails when either holds a file named for the field build.
6. **Every recipe step that runs the product runs the field build.** One program is under test in a whole run, so that a read and a write of one recipe are the same build.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A setting of the product that turns the presence prompt off | It is the one door rule 3's approval rests on. A setting that opens it ships to every machine |
| Write the field tests as Rust tests inside the workspace | A recipe names existing software and lives outside the repository (rule 9); a Rust test in the repository cannot |
| A real operating-system prompt, driven by the runner | Different on each system, and it would be the test driving the person's own prompt, which is the thing the prompt exists to prevent |

## What this risks

A second program that can approve a write exists in the repository. Points 3 and 5 keep it to this machine's own test copies and out of every package, and a test holds each.

## Proof

Roadmap FT4: through the fault proxy, a write whose answer is lost is reconciled, and the software's own database holds it once. Three tests of the field build: it does not start without the marker, it refuses a target with an address off this machine, and an approval the runner did not name rests. The packaging check finds no field build in a wheel.
