# 0059. The scout drafts and proves reads by itself, and three things stay with the developer

**Status:** Accepted. Design decision. The owner asked the building agent to decide what the system automates and what it leaves to the developer, and had not read this record when it was merged.

**Date:** 2026-11-17

**Refines:** [0051](0051-findings-and-the-evidence-ladder.md). The second rule of discovery is reworded; its meaning holds.

## Context

The design had an agent or a person do every step between a finding and a draft. The owner wants the product to take that work off them wherever it can be automated, and wants a clear line between what the system does, what it assumes, and what the developer decides. The product must still run no model, and a guess must still never pass as a fact.

## Decision

- **A scout.** One action, `scout.run`, identifies a target, runs every discoverer that suits it, compares what the system says about itself with what was seen, drafts what can be drafted mechanically, tries the drafted reads, and reports. It is a fixed pipeline with no model in it. The same system gives the same report.
- **A drafter.** Where a finding maps to a target file by rule, the runtime writes the draft: an endpoint from a published description, a command definition from a program's own help, a record kind from a schema, a screen model from an observed interface. A draft made this way is at `reconstructed`, like any other draft, and passes the same compiler and the same gates.
- **The scout proves reads and never tries a write.** It tries a read only where reading is enforced by the engine or is the system's own stated contract. It never submits a form, presses a button, or runs a command whose effect it does not know.
- **Three things stay with the developer or their agent,** because they are judgment: what things mean and what to call them; everything about writes, including where a test write is allowed; and settling what the scout could not decide.
- **Some things only a person does,** because they are authority: saying that the system may be inspected, signing in, approving, and publishing to others.
- **The second rule of discovery now reads:** a finding cannot run; only target files that pass the compiler and the gates can, whoever or whatever wrote them.

[Section 67](../spec/67-the-scout-and-who-does-what.md) is the design.

## Consequences

- Two actions join the catalog: `scout.run` and `scout.report`.
- The drafter is kernel code; each pack registers the drafting rules for its own file kinds, as one more thing a pack hands the kernel.
- A developer's first minutes change from "write files" to "read a report and decide". Hypothesis H-31 says this saves more than it costs.
- The scout sends requests to a system the moment it runs. It is bounded, polite, and read-only, and it needs the same consent as any discovery.

## Alternatives considered

- Leave drafting to the agent, as before: nothing to build, and every agent repeats the same mechanical translation and makes its own mistakes in it.
- Let a model inside the product draft and name things: better names, and it would break rule 1 and make two runs differ.
- Let the scout try writes against anything that looks like a test system: the scout cannot know what is safe to change.

## Revisit if

Drafted reads fail their first try so often that fixing them costs more than writing them (H-31), or a kind of system turns up where even reading cannot be made safe.

## Spec sections

- [58-discovery-and-findings.md](../spec/58-discovery-and-findings.md)
- [60-overlaying-tools-on-an-existing-system.md](../spec/60-overlaying-tools-on-an-existing-system.md)
- [67-the-scout-and-who-does-what.md](../spec/67-the-scout-and-who-does-what.md)
