# 0052. Effects are declared and enforced, and a hazard raises the approval

**Status:** Accepted by the agent, under the owner's instruction of 2026-11-17 to design the whole system. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

A universal runtime starts programs, opens files, reaches networks, and talks to devices. The effect class of an operation, `read`, `write`, or `irreversible`, says what it changes in the target and says nothing of what a provider does on the machine, or of harm beyond the record named. The REA project declares effects per tool and states that they are hints for the host to enforce ([research note 16](../research/16-rea-methods.md)). Here the runtime is the host.

## Decision

- A provider declares its effects in its manifest: processes it starts, file roots it reads and writes, network destinations, sessions, code from a target, processes it attaches to, changes to a person's screen, changes to a device.
- Each declared effect is shown with how strongly it is held: `enforced`, `confined`, or `declared`. Only first-party packs may claim more than `declared` until confinement has been researched.
- The effect class keeps its three values. An operation also carries one hazard: `none`, `data_elsewhere`, `sends_outside`, `device_function`, or `physical`.
- A hazard other than `none` requires a person every run. `device_function` and `physical` allow no standing approval and no pre-approval by policy.
- Everything an agent is told about a tool's safety is derived from the effect class, the hazard, and the effects, and never from its name.
- A command is a program with typed arguments. Nothing is ever joined into a line a shell reads.
- Consent for capture, bridges, attaching, running a target's code, and device links is a stored record, never inferred.

## Consequences

- Operation files gain a hazard, and the policy engine gains a rule per hazard. Both are built with Phase 3.
- The review summary shows hazards first.
- A plug-in whose effects are only `declared` is trusted code, and the person enabling it is told so.
- Operating-system confinement of plug-ins becomes a research task before any promise is made.

## Alternatives considered

- Effects as hints, as REA has them: right for a tool that relies on a host, wrong for the host.
- A fourth effect class for physical effects: it would mix what changes with what is at risk, and every rule keyed on the class would need revisiting.
- Per-call confirmation flags supplied by the caller: an agent that has been steered would supply them.

## Revisit if

Confinement proves impractical on a major operating system, or the closed list of hazards misses a real case.

## Spec sections

- [61-effects-hazards-and-consent.md](../spec/2-kernel/213-effects-hazards-and-consent.md)
- [18-safety-policy-and-approvals.md](../archive/spec-2026-11/18-safety-policy-and-approvals.md)
