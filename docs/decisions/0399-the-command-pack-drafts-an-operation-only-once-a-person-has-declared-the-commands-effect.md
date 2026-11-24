# 0399. The command pack drafts an operation only once a person has declared the command's effect

**Status:** Proposed. Design decision. The owner had not read this record. Written by the agent that took package E4.

**Date:** 2026-11-24

**Changes:** nothing yet. It states why the one part of package E4 still missing cannot be built by rule, and what a later slice could build.

## Context

Package E4's proof is "a draft made by rule compiles, names its findings, and carries no unit or key the system did not state". Every pack that observes a system's data or its own interface drafts reads that meet it. The command pack drafts a `commands/` file for each command it finds, and that file fails to compile only on open questions: the effect, the exit statuses, the output form, and the read-back. It drafts no operation. [Spec section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md) gives the reason: help text does not say which commands read and which write, so no operation is drafted. Section 306 adds that a name such as `show` proves nothing.

## Decision

1. **The gap is not closed by a rule, and the roadmap says so.** A rule that drafted a read over a command would have to state an effect the program never stated. That is a guess, and rule 7 (unknown means stop) and rule 3 (a write is recorded first) forbid it. Package E4 stays `held`; its text names the command pack as the one pack that drafts no operation, and why.
2. **Alternative weighed, and proposed for a later slice.** A person declares `effect: read` for a command in the `commands/` file, which the drafter never fills. A drafting rule over a file with every effect declared could then draft one `read` operation for each command declared `read`, naming the command's finding, with inputs from the declared params and no output beyond what the person declared. It meets the proof because nothing in it is guessed: the person stated the effect. It needs the drafter to read a declared file, which today it does not, and a test against `example-cli`. Not built here.
3. **Alternative rejected.** Inferring `read` from a verb in a command's name, or from a PowerShell `Get-` verb. A name states a convention, not a guarantee; a program may write on any verb.

## Inferred

That the owner's intent for E4 is the proof as written, not a change to section 402. If the owner means the command pack to draft by name, this record is wrong and should be answered.
