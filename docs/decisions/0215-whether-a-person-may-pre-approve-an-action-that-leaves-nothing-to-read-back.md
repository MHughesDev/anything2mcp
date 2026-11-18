# 0215. Whether a person may pre-approve an action that leaves nothing to read back

**Status:** Proposed. Owner decision: it trades safety for ease on the approval of `irreversible` operations, which rule two orders. Written by the coverage review of 2026-11-18; the owner has not read it. Nothing here is in force.

**Date:** 2026-11-18

**Asks about:** rules two and three of [AGENTS.md](../../AGENTS.md#rules-that-never-bend); [section 212](../spec/2-kernel/212-policy-and-approvals.md); [section 201](../spec/2-kernel/201-archetypes-and-modes.md), "Where read-back is not possible"; [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md).

## Context

The owner asked that the product wrap software the machine can "read or modify in any way". The verified-write model gives that for anything that leaves a state the system reports: the write is recorded, sent, read back, and compared. Much of what a person does on their own computer is of that kind once it is bound as a write of the state it leaves: a service running, a setting set, a window open, a track playing (hypothesis H-42).

What is left is an action that leaves nothing to read: a notification shown, a page printed, a key sent, a message sent. Section 201 makes it `irreversible`; [decisions 0161](0161-the-plan-and-the-engine-after-the-packs-were-built.md), [0193](0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md), and [0196](0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md) let it end on the system's acknowledgement, "accepted by the system, not read back", and never as verified. That is honest and enough as evidence. The cost is in section 212: an `irreversible` operation is approved by a person every run and never by standing approval. For a payment that is right. For "show me a notification" or "pause the music", asked many times a day on the person's own machine, it makes the tool unusable.

## The safest way through that stays inside the rules

- Bind every action that leaves a reported state as a write of that state, read back by its query, so that it is a `write`, not `irreversible`. This needs no change, only drafting guidance, and the review counts it as the first answer (H-42).
- Leave everything else `irreversible`, approved every run.

## The question for the owner

On one machine, for an operation that is `irreversible` only because nothing can be read back, whose hazard is `none`, and whose target declares it so: may the person grant a standing approval, for a named tool, a named caller, and a time window, revocable and recorded, as section 212 allows for writes? A payment, a send to another person, a delete, any hazard other than `none`, and every server installation would stay as today.

## Recommendation

Yes, narrowly as stated, after H-42 has been measured: if most actions turn out to be state writes, the question shrinks to a few tools and may not be worth asking.

## Alternatives weighed

- A new effect class for such actions. Refused for now: the effect classes are a closed type held to the documents, and the difference is in approval, not in what the action changes.
- Treating the system's acknowledgement as a read-back. Refused: decision 0193 holds that an acknowledgement proves acceptance, not outcome.
