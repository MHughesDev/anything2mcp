# 0033. Policy decisions use Cedar, with grants compiled to it

**Status:** Accepted. Decided in the system design; the owner delegated these choices. Subject to a spike on size and speed.

**Date:** 2026-11-15

## Context

Policy must deny by default, let no permit override a forbid, be deterministic, and be explainable. Writing those semantics in-house means specifying and testing how overlapping rules combine.

## Decision

Authorization decisions are evaluated with Cedar through its Rust crate. Administrators write grants and roles, which compile to Cedar permit policies; built-in guardrails are forbid policies that no configuration removes. Policies are validated against a schema before publishing, and any evaluation error is a denial. Approval and field visibility are asked as separate allow-or-deny questions. Counts, quotas, and kill switches are handled by a limiter beside it.

## Consequences

- Cedar skips policies that error, so validation and the fail-closed wrapper are mandatory.
- One more dependency, whose license, size, and speed are unverified.
- The same engine runs locally with a built-in policy set.

## Alternatives considered

- An in-house grant matcher: smaller, but the semantics become ours to get right. It is the fallback.
- A general policy language such as Rego: more expressive, harder to analyze and explain.

## Revisit if

The spike shows Cedar missing the size or speed budget.

## Spec sections

- [45-policy-engine.md](../archive/spec-2026-11/45-policy-engine.md)
- [20-access-control-toolsets-and-tenancy.md](../spec/2-kernel/214-access-control-toolsets-and-tenancy.md)
