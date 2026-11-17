# 55. Kernel and packs

The system is split in two: a kernel that is the same whatever is being integrated, and packs that each know one kind of system. This section says what is on each side, how the line is held, and what a new pack must bring. Decision: [record 0049](../decisions/0049-a-neutral-kernel-and-packs-behind-one-provider-contract.md).

## The line

**A kernel crate never depends on a pack crate. A pack depends only on the kernel. The product crate wires them together.** The audit script enforces this the way it already enforces the crate layering.

| In the kernel | In a pack |
| --- | --- |
| Core types; target files, their loader, and the compiler; expressions and field types | The file kinds only that pack understands, with their schemas and lint rules |
| The engine loop, the ledger, identity strategies, reconcile | The driver: how each step of its archetype is carried out |
| The policy engine, approvals, limits | How its verbs lower into steps |
| The session keeper and the vault | Its auth providers: how a session of its kind is made and renewed |
| Sources; the action registry; the local engine; the front doors | Its widget drivers or their equivalent |
| Traces and the evidence store | Its discoverers: how a system of its kind is observed |
| The provider registry, the supervisor, the plug-in host | Its test stand: the piece of the test application family that imitates its kind |

## What a pack hands the kernel

A pack is a provider ([section 56](56-the-provider-contract.md)) that lives in this repository. Besides its manifest it registers five things, and nothing reaches into the kernel any other way.

| The pack registers | The kernel uses it to |
| --- | --- |
| Schemas for its file kinds and for the fields its bindings add | Validate target files in the compiler's second stage |
| A lowering from each verb it supports to a list of steps | Build the plan |
| Lint rules, with stable codes | Report problems with the pack's own files |
| A driver for its modes | Run steps |
| Discoverers and a describer | Observe a target and show an agent what was seen |

So the compiler stays neutral: it parses, validates against whatever schemas are registered, resolves names, type-checks expressions, counts commit points, lowers through whatever lowerings are registered, and emits. "Knowledge lives in one place" becomes: knowledge about a kind of system lives in its pack.

## Where today's files go

| File kind | Owner |
| --- | --- |
| `target.yaml`, `auth.yaml`, `types.yaml`, `operations/`, `pipelines/`, `sources/`, `tests/`, `fixtures/` | Kernel |
| `screens/` | The interface packs: web and desktop |
| `endpoints/` | The request packs: HTTP |
| New kinds, such as `commands/` for the command pack | The pack that brings them |

An operation file is the kernel's, and each of its bindings is checked by the pack whose mode it names.

## The first-party packs

| Pack | Modes | State |
| --- | --- | --- |
| Web | `ui`, `page_http`, `site_tools` | Designed in sections 13, 14, 17, 26; first to be built |
| HTTP | `http`, `api` | Designed; Phase 4 |
| MCP | `mcp` | Designed; Phase 4 |
| Desktop | `desktop` | A catalog; Phase 8 |
| Command | `cli` | Proposed as the second pack, to test the boundary. [Section 62](62-catalog-of-surface-kinds.md) |

First-party packs are compiled into the one binary and switched on by a setting ([section 65](65-python-first-packaging.md)).

## The bar a new pack must clear

A kind of system is supported only with all of these.

| # | The pack must bring | Why |
| --- | --- | --- |
| 1 | A driver that implements the driver interface for its verbs, with no retries inside | The engine loop is the same for everyone |
| 2 | A read-back rule for every write it offers, or the write is `irreversible` | Rule three |
| 3 | An answer for each identity strategy: how a record of its kind is found again | Resume without duplicates |
| 4 | A test stand that imitates its kind and reports ground truth through a control interface | A test never proves a write by our own read path alone |
| 5 | A discoverer, if its kind can be inspected, whose findings never claim more than was observed | Section 58 |
| 6 | Its failures mapped to the existing classes | The class alone decides what happens next |
| 7 | Its effects and hazards declared | Section 61 |
| 8 | A decision record for each new mode | Modes are public vocabulary |

A pack that cannot bring item 2 for its writes can still be accepted for reads.

## Packs and plug-ins

| | Pack | Plug-in |
| --- | --- | --- |
| Lives | In this repository, as Rust crates | Anywhere, as a separate process in any language |
| Trust | Reviewed with the product | Signed, or off by default; its manifest approved by an administrator |
| Reaches the kernel | Through the registrations above | Through the plug-in protocol, which carries the same five things |
| Suits | Kinds of system most people need | Rare, heavy, or proprietary kinds |

The contract is one. A plug-in that proves itself can become a pack without the kernel noticing.

## Open points

- Whether the browser-specific auth providers move from the neutral session keeper's crate into the web pack. Package A7 of the long-term plan.
- How a plug-in registers schemas and lint rules across a process boundary: as data at start-up, most likely.
- Whether the neutral part of the capability probe belongs in the kernel.
