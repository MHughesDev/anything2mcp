# 0294. The owner hands the open questions, and the deciding, to the agents

**Status:** Accepted. Owner decision, given in conversation on 2026-11-23 and written here in the agent's words ([decision 0231](0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md)).

**Date:** 2026-11-23

**Changes:** how [the open questions for the owner](../plan/open-questions-for-the-owner.md) are closed. No other document changes.

## Context

Agents already plan, build, and merge without waiting ([decision 0055](0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md)). Some questions were still set aside for the owner, with a record at status Proposed, among them whether anything on the path of a run may start an interpreter (decision 0282).

## Decision

1. **An open question that was waiting for the owner is now decided by the agent leading the work,** with a record that says so and gives the evidence. The owner reads it afterwards and may reverse it.
2. **The lead tells the owner what was decided,** in the conversation and in the journal, and does not ask first.
3. **When the lead uses other agents, they run on the cheapest model.** The owner's usage is limited, and the lead does the work that needs judgment itself.

## What I inferred, and what this leaves alone

The owner said the agent handles all open questions and decision making from here. I inferred that this does not reach three things, because each acts outside this repository or is the product's own law:

- **The ten rules.** An agent still proposes a change to one and carries on.
- **What an agent does not do alone** in AGENTS.md: touching a real application, account, or credential; handling a registry token; adding a dependency under a licence that is not permissive.
- **Publishing a release.**

If the owner meant these too, the owner says so and this record is superseded.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep asking for each open question | The owner asked not to be asked |
| Read it as reaching the ten rules | A wrong guess there cannot be taken back as cheaply as a design decision can |
