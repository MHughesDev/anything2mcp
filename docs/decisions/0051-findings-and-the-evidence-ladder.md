# 0051. Findings, and an evidence ladder from inferred to approved

**Status:** Accepted by the agent, under the owner's instruction of 2026-11-17 to design the whole system. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

A universal runtime spends much of its time finding out what a system can do. Without a rule, something read out of a program's code would look the same as something watched happening, and an agent would build on both alike. The REA project, studied for this purpose, keeps that line carefully ([research note 16](../research/16-rea-methods.md)).

## Decision

- What discovery returns is a finding. A finding is never an operation and can never be run.
- Every finding, draft, and tool carries one of seven levels: `inferred`, `discovered`, `reconstructed`, `schema_verified`, `executed`, `behavior_verified`, `approved`.
- A level is computed from stored evidence by a named part of the system. No file sets it, none can be skipped, and only `approved` tools are listed to ordinary callers.
- A level falls when the evidence under it stops being true.
- Evidence names its subject with one tagged identity, and carries its limitations.
- There are no confidence scores. A level says what kind of proof exists.

## Consequences

- Binding statuses and version stages are unchanged; the ladder sits beside them and the compiler ties them together.
- Core types gain the level and the tagged identity. The long-term plan's package B2.
- The review summary before publishing shows each tool's level and what it rests on.
- Discovery through any provider, however capable, cannot shortcut the publish gates.

## Alternatives considered

- A separate catalog of possible tools: a second source of truth beside the compiler and the gates.
- A numeric confidence: it invites treating 0.9 as proven.
- Two levels only, draft and published: it hides the difference between seen and guessed, which is the difference that matters most during discovery.

## Revisit if

Agents building tools ignore the levels in practice, or seven prove too many to tell apart.

## Spec sections

- [58-discovery-and-findings.md](../spec/2-kernel/218-discovery-and-findings.md)
- [59-the-evidence-ladder.md](../spec/2-kernel/219-the-evidence-ladder.md)
