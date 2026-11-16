# 0012. Approvals happen out of reach of the agent and bind to the diff

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

An agent that can approve its own write provides no safety. A person approving must see exactly what will change.

## Decision

Writes are previewed as a diff. A person approves through a channel the calling agent cannot operate: URL-mode elicitation, an approval page, a chat card, or a local prompt. The approval is a single-use server-side record bound to the diff's hash, the tool version, the approver, and an expiry.

## Consequences

- If the application's data changes before the run, the approval no longer matches and the run stops.
- Policy can pre-approve low-risk writes and grant standing approvals to limit fatigue.
- Clients without elicitation still work through a link in the result.

## Alternatives considered

- Agent-confirmed writes. Rejected.

## Revisit if

Never for irreversible operations.

## Spec sections

- [18-safety-policy-and-approvals.md](../spec/18-safety-policy-and-approvals.md)
