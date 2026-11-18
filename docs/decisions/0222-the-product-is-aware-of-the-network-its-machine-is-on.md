# 0222. The product is aware of the network its machine is on

**Status:** Accepted. Owner decision of 2026-11-18. The direction is the owner's; the wording is the lead agent's, as the owner asked ([decision 0231](0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md)). Only the owner changes it. The design that carries it out is not written yet: research and design rounds are under way, and what they propose will be Proposed records the owner has not read.

**Date:** 2026-11-18

**Extends:** [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md), which said the product reaches what the person's machine reaches and serves on that machine only.

## Context

Decision 0208 set what the product may reach: an application on the person's machine, a website, a public endpoint. In each of those the person already knows the address and gives it to `target.add`. Nothing in the design looks at the network the machine is on and says what is there. The scout starts from one address a person names.

## Decision

The product becomes aware of the network. Until now it has known a machine and the addresses a person hands it. From now on it also knows where its machine stands: which networks the machine is on, what else is on them, and how the machine gets from where it is to anything it reaches. A website was always a thing reached across networks; the design treated it as a special case with an address. It becomes one instance of a general idea.

Three things follow.

1. **The scout looks across the network.** Run from one machine, it finds what is on the networks that machine can reach. It no longer needs a person to know an address first.
2. **Agents can see the network.** Beside the tools a developer builds, the product carries tools of its own with which a coding agent can inspect where it is: the machine, and the networks the machine is on.
3. **The network is a general idea, held everywhere.** Where a thing is, and by what path the machine reaches it, is something every part of the system knows and checks, in one way. Nothing is a special case of it: not a website, not a device, not a service on the machine itself.

The owner also asked that this be approached with invention and good engineering, not as a feature bolted on: research first, then design, in rounds.

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
