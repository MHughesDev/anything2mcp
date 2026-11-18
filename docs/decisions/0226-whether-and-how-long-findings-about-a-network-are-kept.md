# 0226. Whether and how long findings about a network are kept

**Status:** Proposed. **This question is the owner's**, because a list of what is on a network is personal data, often about devices that are not the person's own (a café's other guests, a client's hosts), and keeping it touches more than a retention setting. An agent does not persist network findings past a run until the owner decides. Written by the first network-awareness design wave of 2026-11-18. Nothing here is in force.

**Date:** 2026-11-18

**Rests on:** the network findings of [decision 0223](0223-a-network-is-a-place-a-recognized-network-and-a-reach.md) and [docs/plan/network-awareness.md](../plan/network-awareness.md); the retention rules of [section 217](../spec/2-kernel/217-traces-evidence-and-repair.md).

## Context

The scout across a network produces findings: the hosts it saw, the services that announced themselves, the certificates it read. A device list is personal data. On a café or a client's network, much of it is about third parties. Traces are kept 30 days by default today (section 217); a device list is not an ordinary trace.

## The question

Are findings about a network kept at all, and if so for how long, and is the answer different for a network the person named as theirs and one they did not?

## The lead's recommendation

**Keep network findings only for the life of the scout run that made them, unless the person has saved the network to their reach.** For a network the person named (it is in their reach), findings about *that named network* are kept under the ordinary trace retention, because the person has said it is theirs to look at and may want "what changed since I last looked" (an invention the design proposes). For a network the person has not named, findings are not persisted past the run that made them: the inventory is shown and then dropped. A device list about a café's other guests is not something the product keeps.

This keeps the useful case (my own network, over time) and refuses the dangerous one (a record of who was on a network that is not mine), and it needs no new retention class beyond marking network findings for the shorter life where the network is unnamed.

## Alternatives weighed

- **Keep all network findings under the ordinary trace retention.** Simplest; it gives the diff feature everywhere. Rejected: it builds a kept record of third parties' devices on networks the person does not own, which is the personal-data risk the owner should weigh.
- **Never persist any network finding.** Safest; loses "what changed since I last looked" even on the person's own network, which is one of the design's stronger inventions. Held as the fallback if the owner refuses any keeping.

## Revisit if

The owner decides; or data-protection advice (a research brief) shows the named-network case also needs a shorter class or an explicit opt-in.
