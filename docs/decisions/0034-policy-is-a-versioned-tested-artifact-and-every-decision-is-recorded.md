# 0034. Policy is a versioned, tested artifact, and every decision is recorded

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

A policy change can open a write to the wrong people or lock approvers out. Runs can wait for hours between a call and its write, during which grants may change.

## Decision

The policy set is immutable, hashed, and versioned. It ships with a table of requests and expected decisions that must pass before it can be published. Every decision stores the request, the answer, the determining rules, and the policy version. A run is judged on the call, again before its write phase, and again on resume.

## Consequences

- A person who loses a grant while their run waits does not get the write.
- Any past decision can be explained, and questions like who can call a tool can be answered ahead of time.
- Storing every decision costs space; reads may be sampled or summarized.

## Alternatives considered

- Settings edited live with no tests: simple, and one mistake is immediate.

## Revisit if

Decision storage becomes a cost problem at scale.

## Spec sections

- [45-policy-engine.md](../spec/45-policy-engine.md)
- [18-safety-policy-and-approvals.md](../spec/18-safety-policy-and-approvals.md)
