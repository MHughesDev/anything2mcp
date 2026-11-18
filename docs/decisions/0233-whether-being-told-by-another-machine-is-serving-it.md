# 0233. Whether being told by another machine is serving it

**Status:** Proposed. Owner decision: it asks how far owner decision 0208 reaches, which only the owner changes. Written by the breadth review of 2026-11-18; the owner has not read it. Nothing here is in force, and decision 0208 stands as written until the owner answers. It does not ask to reopen 0208; it asks the owner to read two edges of it.

**Date:** 2026-11-18

**Asks about:** [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md), rule 2 there ("Nothing listens for another machine"); [decision 0169](0169-the-first-release-is-for-one-machine.md).

## Context

Decision 0208 has two halves: outward, the product reaches whatever the machine can reach; inward, it offers its tools on that machine and nowhere else, and nothing listens for another machine. Two things an agent would want sit on the line between the halves, and [decision 0230](0230-the-scope-is-written-down-on-both-sides-and-describes-without-ruling.md) asks that a narrowing be made visible so that the owner can see it.

1. **Being told by an outside service.** Many services report a change by calling an address the subscriber gives them (a webhook). To receive one, something on the machine must listen for another machine. Such a listener would serve no tool and answer nothing but a receipt; it would accept only signed deliveries for a subscription the person set up. It is still a listener, and 0208 says nothing listens.
2. **The person's own second machine.** A person with a laptop and a desktop may want the agent on one to use the tools built on the other. That needs the engine on the second machine to listen for the first. A runner on another machine comes after the first release (0169).

What is told without either, inside 0208 as it reads:

- **Polling.** A watch repeats a read on a schedule the person confirmed and reports what changed ([decision 0234](0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md)). It reaches out; nothing listens.
- **An outbound relay.** A broker or relay the person already uses holds the deliveries, and the stream pack reads them over a connection the machine opens.
- **What the second machine already serves.** An HTTP interface, an MCP server, or a database the person runs there is reached like any other surface.

## The questions for the owner

1. Does "nothing listens for another machine" cover a listener that receives an outside service's signed deliveries and serves no tool?
2. Is the person's own second machine "another machine" in 0208's sense, so that the engine on it serves only itself?

## Recommendation

Yes to both, for the first release. A listener is the largest change to what an attacker on the network can reach, and 0208's owed rule test ("no product code opens a listener on any address but loopback") is the simplest guarantee the first release can give. Polling and an outbound relay carry what is told at the cost of delay, and SCOPE.md should say so plainly. Both questions are for the release after, with the server.

What this record infers: that 0208's "nothing listens" was meant as a guarantee about the machine's exposure, not only about where tools are served. If it was meant only for tools, question 1 is a design question and comes back to the agents.

## Alternatives weighed

- **Not asking.** The narrowing would stay invisible: a reader of SCOPE.md would not learn that being told by an outside service is excluded by an owner decision rather than not yet built.
- **Recommending a receiver on loopback behind a tunnel the person runs.** The tunnel is another program listening for another machine on the product's behalf; it moves the exposure without removing it.
