# 0049. A neutral kernel, and packs behind one provider contract

**Status:** Accepted by the agent, under the owner's instruction of 2026-11-17 to design the whole system. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

[Decision 0048](0048-the-product-becomes-anything2mcp.md) makes the product a universal runtime. The design of sections 1 to 52 mixes what is true of every system with what is true of web and desktop applications. Before any engine code exists is the cheapest moment to separate them.

## Decision

- The system is a kernel and packs. A kernel crate never depends on a pack crate; a pack depends only on the kernel; the product crate wires them. The audit script enforces it.
- A pack registers five things with the kernel and reaches into it no other way: schemas for its file kinds, a lowering from its verbs to steps, lint rules, a driver, and discoverers.
- Everything that supplies capabilities is a provider with one manifest. There are three ways to be one: a first-party pack, a plug-in process, an upstream MCP server.
- The extension points are a fixed list: driver, auth provider, source reader, widget driver, discoverer. Adding one takes a decision record.
- First-party packs are compiled into the one binary and switched on by a setting.
- A new pack must clear the eight-item bar of spec section 55.

## Consequences

- The compiler becomes neutral: it validates against registered schemas and lowers through registered lowerings.
- The crate layout and the layering table change; the long-term plan's Stage A does it.
- A provider cannot decide policy, renew a session, approve anything, or write a ledger row. Those stay in the kernel.
- The plug-in protocol has to carry schemas and lint rules as data, which it did not before.

## Alternatives considered

- Keep one undivided system and add kinds of system where needed: the web assumptions would spread into the engine and every later kind would pay for them.
- An open plug-in system where a provider may add any kind of thing: every extension point is a contract the engine, policy, and ledger must understand, so an open list would make "verified" mean less.
- A package per pack: more ways to be half-installed, for a binary that is small anyway.

## Revisit if

The second pack cannot be added without reshaping the kernel. That is hypothesis H-26.

## Spec sections

- [55-kernel-and-packs.md](../spec/55-kernel-and-packs.md)
- [56-the-provider-contract.md](../spec/56-the-provider-contract.md)
- [65-python-first-packaging.md](../spec/65-python-first-packaging.md)
