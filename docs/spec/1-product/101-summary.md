# 101. Summary

**North Star: latch onto any existing software or system that a person is entitled to use, and expose it to AI agents as deterministic, verified MCP tools.**

**Anything2MCP** is an open-source toolkit. A developer, a person or an agent working for one, installs it, points it at a system that already exists, and builds tools over it. Each tool is a deterministic operation that has been proven to do what it says. Any agent that speaks the Model Context Protocol can call the tools, whoever made it.

"Existing" means the system is not changed to suit the toolkit. It need not cooperate, publish an interface, or know that it is being wrapped. Anything2MCP reaches it the way it can already be reached, under a person's own sign-in or an account of its own. The system can be a web application, an HTTP service, an existing MCP server, a desktop program, a command-line program, a database, a file, a queue, or a device with its firmware ([section 106](106-what-it-latches-onto.md)).

## Who it is for

| It is for | It is not |
| --- | --- |
| A developer, human or agent, who picks it up as they would a library: one install, then pointed at a system | A hosted service, or a product for end users to browse |
| Any agent that speaks the protocol, a home-built one included. No make of agent is preferred, and nothing in the kernel knows which one is calling | Tied to a model provider. None is required |
| Targets the developer keeps: a target is a folder in the developer's own repository, loaded from a path or from a package they built | A catalog. The project publishes no tool definitions for real products and hosts no place to share them ([decision 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md)) |

The judgment needed to build tools, what things mean and which capabilities matter, is done once, at build time, by the developer or their agent. What the toolkit automates and what it leaves to them is in [section 402](../4-building-tools/402-the-scout-and-who-does-what.md).

## The four qualities

Every design decision is weighed against four qualities, in this order when they conflict: **safe, correct, fast, easy.** Speed is a first-class measure, with budgets and regression tests ([section 225](../2-kernel/225-performance.md)).

## What makes it different

1. **Deterministic at run time.** No model runs inside a tool call and nothing improvises. A run carries out a compiled plan; a step that cannot be done as written stops with a named failure class.
2. **Every write verified.** A write is recorded before it is attempted, previewed and approved where policy asks, read back from the system afterwards, and compared after normalization. A run that is interrupted resumes without writing twice.
3. **Honest about evidence.** Everything learned about a system carries an evidence level, from `inferred` to `approved`. A guess never passes as a fact, and only proven tools are listed ([section 219](../2-kernel/219-the-evidence-ladder.md)).
4. **Many ways to one operation.** One operation can have several bindings, each in a different mode: a page's interface, the requests the page makes, a direct request, an MCP server's tool, a declared command. The engine uses the verified binding first in the declared order, and falls to the next when one is unhealthy.
5. **Built by the agent you already have.** Any coding agent builds tools with the toolkit's commands and target files, or through the matching MCP tools. A scout does the mechanical part by itself and never writes to the system.
6. **Identity done properly.** Each person acts with their own account and permissions, or the toolkit acts with an account of its own. Existing single sign-on and multi-factor sign-ins are reused and kept alive by the session keeper, so nobody approves a sign-in per call. A person's session is used only by that person and agents acting for them.
7. **Governed.** Administrators decide which tools are exposed to which people and agents, with grants, approvals, limits, and a tamper-evident audit log.
8. **Runs anywhere, in any shape.** One binary runs on one laptop, as a company server, or as a control plane with runners on other machines, on Windows, macOS, and Linux.
9. **Many ways in.** Agents connect over MCP. Other software uses a REST interface, Python and TypeScript SDKs, or embeds the Rust library.

## How it is built

The core is written in **Rust** and shipped as **one binary**, installed through **pip, crates.io, or npm**; each route delivers the same binary ([section 605](../6-running-it/605-packaging-and-release.md)). Other languages are used where they fit better: TypeScript for the web pages, Python and TypeScript for the SDKs, shell scripts for the installers, and any language for plug-ins.

It is a neutral **kernel** and a set of **packs**. The kernel does what is the same for every kind of system: the engine loop, the ledger, policy, sessions, the compiler, and the front doors. Each pack knows one kind of system and reaches the kernel only through one provider contract ([section 202](../2-kernel/202-kernel-and-packs.md), [section 203](../2-kernel/203-the-provider-contract.md)).

## What it does not do

| It does not | Because |
| --- | --- |
| Run anything unverified | A kind of system is supported only when a write to it can be read back, or is honestly classed `irreversible` |
| Let a model decide at run time | Discovery and drafting may use any help; a run uses none |
| Offer a shell, or a statement built from an agent's text | Being able to run a command is not permission to run it. Commands are declared, with typed arguments ([section 213](../2-kernel/213-effects-hazards-and-consent.md)) |
| Click by position on a picture of the screen | Nothing can be verified against it |
| Get in where it is not let in | No stealth, no getting past a sign-in, a bot defence, a licence check, or a device's protection. A block is a named stop |
| Publish a catalog, or host anything | A target is a folder the developer keeps. The product runs where the developer runs it, and nothing phones home |
| Arrive all at once | One pack is proven before the next begins. The first usable product is reads and writes on one machine through the web pack |
