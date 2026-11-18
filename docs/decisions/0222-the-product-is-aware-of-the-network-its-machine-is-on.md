# 0222. The product is aware of the network its machine is on

**Status:** Accepted. Owner decision, given in conversation on 2026-11-18 and written down by the lead agent. Only the owner changes it. The design that carries it out is not written yet: research and design rounds are under way, and what they propose will be Proposed records the owner has not read.

**Date:** 2026-11-18

**Extends:** [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md), which said the product reaches what the person's machine reaches and serves on that machine only.

## Context

Decision 0208 set what the product may reach: an application on the person's machine, a website, a public endpoint. In each of those the person already knows the address and gives it to `target.add`. Nothing in the design looks at the network the machine is on and says what is there. The scout starts from one address a person names.

## Decision

The owner's words: "let's start some waves that essentially go through, do research, and think and brainstorm upon areas that need imagination, invention, and better engineering, so that we can make the system network aware, so that essentially you could have the system run from one device where, say, like the scouting software should be able to scout across the network as well. However, the software that will be available in this software suite, on this platform essentially, will be some more tools that coding agents would be able to use, so that they can also inspect the network they are on as well. So not just the machine, but also the network that it has access to. So not just, you know, a website. A website should be part of that. Like a website, you're going from one network to another. We should have that same capability where it's just network aware in general. Generalize that in the system. The system should be generalized to being network aware, is what I mean to say in the end of all this."

Three things follow.

1. **The scout can look across the network.** Run from one device, it can find what is on the networks that device can reach, and not only examine an address a person already knows.
2. **Agents get tools to inspect the network.** The product carries tools, beside the tools a developer builds, with which a coding agent can see the network its machine is on: the machine, and the network the machine has access to.
3. **The network is a general idea in the system, not a special case.** A website reached across networks is one instance of it. The design is to be generalized so that where a thing is on a network, and how the machine reaches it, is something the system knows about everywhere.

## What this does not change

- **The ten rules.** In particular rule 5, operate openly: no stealth, no evading a network's own protections, no guessing of credentials, and a block is a named stop; and rule 7: what cannot be identified is not guessed at. Looking at a network is done as an honest inventory tool does it.
- **Who reaches the product.** The lead agent reads this decision as being about what the product reaches and sees, not about who may reach the product. Decision 0208's second rule stands: the MCP server and every other door stay on the person's machine, and nothing listens for another machine. If the owner means otherwise, that is the owner's to say.
- **Entitlement.** The product works through access the person already has, on networks the person is entitled to look at. How the product knows which networks those are, and what a person must say before anything is sent to an address they did not name, is the first question for the design.
- **An agent may be steered** ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)): no input from an agent may widen where the product looks. The reach is the person's choice.

## Consequences

- Research is owed before design: how what is on a local network announces itself or can be found, on Windows, macOS, and Linux; what each operating system asks of a program that looks at the local network; what existing tools do and what their conduct rules are; which crates.
- A design is owed that says what "a network" is in the kernel's terms, how the scout goes from a network to a host to a surface to a pack's discoverer, what the built-in inspection tools are, and what bounds each.
- [Section 106](../spec/1-product/106-what-it-latches-onto.md), [section 218](../spec/2-kernel/218-discovery-and-findings.md), [section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md), and section 502 will change when the design is accepted. Nothing in them changes by this record alone.

## Alternatives weighed

None by the lead agent: this is the owner's direction. The alternatives between designs are for the records that follow.
