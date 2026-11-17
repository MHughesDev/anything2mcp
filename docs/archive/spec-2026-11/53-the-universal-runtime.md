# 53. Anything2MCP: the universal runtime

> Archived on 2026-11-17. This is section 53 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 106, What it latches onto](../../spec/1-product/106-what-it-latches-onto.md).

Sections 1 to 52 design a system that wraps applications. Sections 53 to 66 widen it to any existing system, from a device's firmware to a hosted service, without changing what makes it trustworthy. They refine the earlier sections and win where they differ. The owner's decision is [record 0048](../../decisions/0048-the-product-becomes-anything2mcp.md); the order of work is in the [long-term plan](../plans-2026-11/anything2mcp-master-plan.md).

## The claim

Given an existing system, Anything2MCP produces tools an AI agent can call, where each tool is a deterministic operation that has been proven to do what it says.

"Existing" means the system is not changed to suit us. We reach it the way it can already be reached.

## The stack, from the bottom up

A system can be reached at many levels. Each level has its own idea of what an operation is.

| Level | Examples | An operation is | Section |
| --- | --- | --- | --- |
| Devices and firmware | A controller on a fieldbus, a board on a serial console, a network device | Reading or setting a parameter, a declared console command, an update through the device's own mechanism | 63 |
| Operating system | Services, packages, scheduled jobs, the registry, files | A declared command or system call with typed arguments | 62 |
| Native programs and libraries | A command-line tool, a shared library, a program with an automation interface | Starting it with arguments, or calling a function it exports | 54, 62 |
| Language runtimes and packages | A Python or Node package, a script the system ships | A function with a typed signature | 62 |
| Data stores | Relational and other databases, files in known formats, object storage, queues | A query, a keyed write, a message | 62 |
| Services and protocols | HTTP and its descendants, remote procedure calls, mail, directories, an MCP server | A typed request and its response | 6, 62 |
| Applications | Web, desktop, mobile, and terminal applications | What a person does on a screen, done by element and not by pixel | 13, 17, 62 |
| Systems of systems | An order that touches an application, its database, and a device | A pipeline over several targets | 60 |

The same target can be reached at several levels at once. An application has an interface, a backend, a database, perhaps an administration tool and a device behind it. Section 60 says how one target holds all of them and which level a tool should use.

## One method, one engine

Two things are the same at every level, and they are the product.

- **One method for building a tool:** attach, observe, model, bind, verify, publish, watch. Section 60.
- **One engine for running it:** the eight-part loop of section 8, with intent before action and read-back after it. The engine does not know what kind of system it is driving. Section 54 shows how each way of reaching a system fills the loop.

Everything that differs between kinds of system lives in a pack (section 55) behind one provider contract (section 56).

## What "anything" does not mean

| It does not mean | Because |
| --- | --- |
| Anything unverified | A kind of system is supported only when a write to it can be read back, or is honestly classed irreversible. Section 55 sets the bar |
| A model deciding at run time | Rule one. Discovery and drafting may use any help; a run uses none |
| A shell | Being able to run a command is not permission to run it. Commands are declared, with typed arguments. Section 61 |
| Getting in where we are not let in | Rule five. Nothing learned by inspection is used to defeat a protection. Sections 61 and 63 |
| Everything at once | One pack is proven before the next begins. The first usable product is still reads and writes on one machine through the web pack |

## The sections that follow

| # | Section | What it settles |
| --- | --- | --- |
| 54 | Interaction archetypes | The seven ways any system is reached, and how each fills the engine loop |
| 55 | Kernel and packs | What is neutral, what is specific, and the bar a new kind of system must clear |
| 56 | The provider contract | One manifest, a fixed list of extension points, three ways to be a provider |
| 57 | Supervising subsystems | Starting, talking to, and stopping outside processes, bridges, and engines |
| 58 | Discovery and findings | How a system is observed, and what an observation is and is not |
| 59 | The evidence ladder | How far each claim has been proven, and what follows from each level |
| 60 | Overlaying tools on an existing system | The method, end to end, including targets with several layers |
| 61 | Effects, hazards, and consent | What a provider and an operation may do, and what needs a person |
| 62 | Catalog of surface kinds | Every kind of system considered, with what each would need |
| 63 | Devices and firmware | What is realistic at the lowest levels, and what is refused |
| 64 | Rules for tools and capability discovery | How an agent finds what is installed, and how tools are shaped for agents |
| 65 | Python-first packaging | The base install, optional parts, and outside components |
| 66 | Readiness of the universal design | What is designed, what is only cataloged, and what must be proven first |

## Open points

- Whether every system of interest fits one of the seven archetypes of section 54. Hypothesis H-29.
- Whether the kernel can stay neutral once a second pack exists. Hypothesis H-26, tested by Stage D of the long-term plan.
