# 0082. The decision core is the policy crate's own evaluator

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Supersedes:** [decision 0033](0033-policy-decisions-use-cedar-with-grants-compiled-to-it.md), which chose Cedar subject to a spike on licence, size, and speed. This record is that spike, research brief 31, and its result.

## Context

Decision 0033 chose Cedar so that the meaning of overlapping rules would be specified and tested by someone else, and named a small evaluator of our own as the fallback. The spike was run before `a2m-policy` was built, with `cedar-policy` 4.15.0 on Rust 1.100.0, in a throwaway program outside the workspace.

| Measure | Cedar | Our own evaluator |
| --- | --- | --- |
| Licence of the tree against the closed list in `deny.toml` | Fails. The crate itself is Apache-2.0, but its tree brings `ar_archive_writer`, under `Apache-2.0 WITH LLVM-exception`, as a build dependency of `psm`, which `stacker` and so `cedar-policy-core` need. That expression is not on the list | Nothing added: `serde` and `serde_json`, already in the workspace |
| Crates added | 107 compiled, 69 of them linked into the binary, with a parser generator, a lexer generator, and a pretty-printer among them | None |
| Code that is not Rust | `psm` builds assembly through a C compiler on every platform | None |
| Release build of the tree, two jobs | 116 seconds | Seconds |
| Binary size, stripped, against an empty program | 5.8 MB against 0.3 MB | Not measurable apart from the crate |
| One decision, release build | 1.9 microseconds, two policies | 0.07 microseconds, the built-in local set over 1,000 generated requests |
| Skipped policy on an evaluation error | Yes: a forbid that errors is skipped, so a validator and a fail-closed wrapper are both required | No rule can be skipped: guardrails are functions over typed facts, and a failing condition denies |

Both are far inside the budget of 100 microseconds for a decision.

## Decision

The decision core is `a2m-policy`'s own evaluator. It keeps the model decision 0033 wanted: a principal, a question, a resource, and a context; a request is denied unless a permit applies; one matching forbid denies it whatever permits apply.

- **Forbids are the guardrails,** written as functions over the typed request, compiled into the binary, and consulted before any permit. Their names, wording, and a revision number are hashed into every policy version.
- **Permits are data:** role bindings, grants, label and option rules, and pre-approval rules. A pre-approval rule may carry a condition in the expression language of `a2m-expr`, checked when the set is compiled. That is the only place an administrator writes anything like a language.
- **An error is a denial.** A condition that fails when it runs denies the request with `policy_evaluation_failed`.

Nothing outside `a2m-policy` sees the evaluator, so a policy language can still be put behind `decide` later.

## Consequences

- The meaning of overlapping rules is ours to hold. It is two sentences, and a property test over generated requests holds them: the same request on the same version gives the same answer, and a matching guardrail always denies.
- Administrators write grants, roles, and pre-approval rules, not policies in a language. The open point of section 212 on whether to expose a policy language in the first release is settled for now: no.
- The binary stays small, the tree holds no new licence, and the build holds no new C or assembly.
- We lose Cedar's formal specification, its analysis tools, and partial evaluation for listing what a caller may use. Listing is the question asked tool by tool, which at 0.07 microseconds a decision is cheap for a catalog of thousands.

## Alternatives considered

- **Cedar, with the licence expression added to the list.** The exception makes the licence more permissive, not less, so this is a real option. It was not taken because the list is closed on purpose, and because the tree and the binary size are out of proportion to a model whose forbids are five fixed rules and whose permits are rows.
- **Cedar's core crate without the formatter and validator.** It still needs `stacker` and the parser generator, and without the validator the skipped-policy caution has no answer.
- **A general policy language such as Rego.** Rejected by decision 0033 already.

## Revisit if

Administrators need rules that grants, roles, and conditions cannot say, or an outside audit asks for a formally specified policy language.

## Spec sections

- [212](../spec/2-kernel/212-policy-and-approvals.md), "The decision core"
- [The crate's design](../architecture/crates/a2m-policy.md)
