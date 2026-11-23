# 0312. A kind of step is marked as never, as declared, or as able to change the system

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Refines point 4 of [decision 0307](0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md).

**Date:** 2026-11-23

**Changes:** the `Lowering` trait in `a2m-files`, which every pack implements, and the compiler's analysis of a read.

## Context

Decision 0307 had a pack state, for each kind of step, whether a step of it can change the system, and had the compiler refuse a read that holds one. Built as a yes or no, it refuses every read of the HTTP, MCP, command, and device packs: a request, a tool call, a started program, and a written frame are the steps of their reads and their writes alike. Whether such a step changes the system is stated by the pack's files (the endpoint's method, the tool's read-only mark, the command's declaration), and the step's `commits` mark carries it into the plan.

## Decision

1. `Lowering::changes(kind)` gives one of three marks, `StepChanges` in `a2m-files`:
   - **`Never`:** a step of the kind cannot change the system. A step of it marked `commits` is refused as `kernel.lowering_invalid`.
   - **`AsDeclared`:** a step of the kind changes the system only where the pack's files declare a write, and the driver holds it to that declaration. A read may hold one. The existing rule that a read holds no step marked `commits` applies to it.
   - **`Can`:** a step of the kind can change the system whatever the files say, such as typing into a page or pressing its buttons. A read never holds one.
2. A kind the pack does not mark is `Can`, so a forgotten kind fails closed.
3. The compiler refuses an operation whose effect is `read` and whose binding holds a step of a `Can` kind, as `kernel.read_acts`, whichever pack lowered it. The web pack's own refusal stays as the nearer message.
4. The mark is on the `Lowering` trait, not in the provider manifest, because the compiler does not see the manifest.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A yes or no for each kind (decision 0307 as written) | It refuses every read of four packs |
| The mark in the provider manifest | `a2m-files` does not depend on `a2m-provider`; the compiler could not read it |
| Hold the rule in `Plan::check` too | The plan does not carry each kind's mark, so a plan the compiler did not make cannot be held to it. Left open |

## Proof

In `a2m-files`, with the test lowering: a read with a `Can` step fails with `kernel.read_acts`; a read with a kind left unmarked fails the same way; a read with an `AsDeclared` step compiles. Every pack marks its kinds, and every pack's reads still compile.
