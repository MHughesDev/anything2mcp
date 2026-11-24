# 0998. A version in which no binding of an operation may act is not published, and a call of one fails for good

**Status:** Accepted. Design decision, made by the worker on field test 7, finding 47, under the lead's brief. The owner had not read this record when it was written. It adds a kernel lint code and an engine failure code; no public interface changes.

**Date:** 2026-11-24

**Changes:** `a2m-files` (`src/codes.rs`, `src/compile.rs`); `a2m-engine` (`src/codes.rs`, `src/run.rs`); the designs of `a2m-files` and `a2m-engine`; [spec section 403](../spec/4-building-tools/403-testing-and-publish-gates.md); the [file formats reference](../reference/file-formats.md).

## Context

In field test 7 a target written by hand stated no `status` on its bindings. A binding with no status is `draft` ([spec section 201](../spec/2-kernel/201-archetypes-and-modes.md)), and a `draft` binding may not act once published. The target linted clean, passed its offline tests (which run a draft binding, as they must to earn its promotion), and was published by the person's act. Its call then stopped as `busy`, `no_binding`, and the agent was told "The run stopped and can be resumed". Nothing would ever let that run go on: a run is held to the compiled target it started on, and a binding's status in it never changes.

## Decision

1. **The compiler owns the check, as it owns every rule of what a version may hold.** A new kernel lint code, `kernel.no_binding_may_act`: an operation of which every binding is `draft` or `retired` is an error in a publish compile, so the version is not published, and a warning in a draft compile (lint, try, test), as a test rule is. The message says that a binding with no `status` is `draft`; the fix names `binding.promote`.
2. **A call that cannot run on its version fails, and offers no resume.** A binding passed over for its status alone gives a new engine failure, `no_binding_may_act`, of the class `policy`, so the run ends `failed`. It is the reason given only where no binding of the operation was passed over for any other reason: a binding not ready, a session ended, a surface blocked or drifted, each of which may pass, still stops the run as before.
3. **What is not changed.** A `shadow` binding with no `verified` binding beside it is not refused at publish: the example target and the packs' test targets keep one as the binding a write is tested through, and the engine already fails such a call for good under point 2. `no_binding` keeps its class and its other causes.

## Alternatives

| Alternative | Why not |
| --- | --- |
| The publish gate in `a2m-actions` refuses the version | The compiler is the schema and lint gate ([spec section 403](../spec/4-building-tools/403-testing-and-publish-gates.md)); a check there also reaches every other caller of a publish compile, and `lint` can warn of it |
| A binding with no `status` defaults to `verified` | A binding that was never proven would act; the default is `draft` on purpose ([decision 0189](0189-a-developers-first-hour-runs-without-hand-steps.md)) |
| Refuse every operation with no `verified` binding, a lone `shadow` too | It is the stricter rule, and the right one once the test targets that keep a lone shadow write are changed; until then point 2 keeps such a call from offering a resume |
| Keep `no_binding` and change the words of the resume in `a2m-actions` | The run would still be `stopped`, which the ledger and every face read as resumable |

## Proof

`an_operation_with_no_binding_that_may_act_is_warned_in_a_draft_and_refused_at_publish`, in `crates/a2m-files/src/tests/example.rs`: a binding with no status, one `draft`, and one `retired` are each a warning in a draft and an error at publish; the example target, with its lone shadow write, still publishes. `every_kernel_lint_code_has_a_case_that_fails_with_it_and_the_example_passes` holds the new code to a case. `a_try_runs_a_draft_read_that_may_not_act_live`, in `crates/a2m-engine/tests/sandbox.rs`: live, a draft read fails as `policy`, `no_binding_may_act`, and the run is `failed`.
