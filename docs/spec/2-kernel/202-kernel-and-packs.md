# 202. Kernel and packs

The system is split in two: a kernel that is the same whatever is being integrated, and packs that each know one kind of system. This section says what is on each side, how the line is held, and what a new pack must bring. Decision: [record 0049](../../decisions/0049-a-neutral-kernel-and-packs-behind-one-provider-contract.md).

## The line

**A kernel crate never depends on a pack crate. A pack depends only on the kernel, and sees only its first five layers. The product crate wires them together.** The audit script enforces this the way it already enforces the crate layering.

The crates fall in eleven layers ([decision 0061](../../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md); the binding table is in the [technical reference](../../reference/technical-reference.md#crate-layering)). The kernel is layers 1 to 8: types, foundations, stores, services, the contract, the engine, actions, and roles. A pack's base crate is layer 9 and its other crates layer 10; a pack sees layers 1 to 5, up to the contract in `a2m-provider`, and the base of its own pack, and cannot call the engine, the scheduler, the action registry, or a front door. The product crate, `anything2mcp`, is layer 11 and the only code that names a pack.

| In the kernel | In a pack |
| --- | --- |
| Core types; target files, their loader, and the compiler; expressions and field types | The file kinds only that pack understands, with their schemas and lint rules |
| The engine loop, the ledger, identity strategies, reconcile | The driver: how each step of its archetype is carried out |
| The policy engine, approvals, limits | How its verbs lower into steps |
| The session keeper and the vault; the auth providers that only hold or exchange a credential | Its auth providers that need a surface of its kind: how a session of its kind is made and renewed |
| Sources; the action registry; the local engine; the front doors | Its widget drivers or their equivalent |
| Traces and the evidence store | Its discoverers: how a system of its kind is observed |
| The provider registry, the supervisor, the plug-in host | Its test stand: the piece of the test application family that imitates its kind |

**Where an auth provider lives.** In `a2m-auth` when it only holds or exchanges a credential; in a pack when it needs a surface of the system. Sign-in forms and single sign-on are in the web pack, authorization to an upstream MCP server is in the MCP pack, and a pack whose credential is bound to its own kind of connection brings its own, as the data pack does for a database password. A provider outside the web pack that needs a person's consent in a browser gives the person a link, and nothing drives that browser. The table is in the [technical reference](../../reference/technical-reference.md#auth-provider-interface).

## What a pack hands the kernel

A pack is a provider ([section 203](203-the-provider-contract.md)) that lives in this repository. Besides its manifest it registers six things, and nothing reaches into the kernel any other way.

| The pack registers | The kernel uses it to |
| --- | --- |
| Schemas for its file kinds and for the fields its bindings add | Validate target files in the compiler's second stage |
| A lowering from each verb it supports to a list of steps | Build the plan |
| Lint rules, with stable codes | Report problems with the pack's own files |
| A driver for its modes | Run steps |
| Discoverers and a describer | Observe a target and show an agent what was seen |
| Drafting rules, from its kinds of finding to its kinds of file | Write drafts for what follows by rule ([section 402](../4-building-tools/402-the-scout-and-who-does-what.md)) |

Its manifest also declares the file kinds and finding kinds it reads from other packs. Packs share data shapes and never code: a kind has one owning pack, and a pack that reads it receives the validated entries from the compiler as data ([decision 0062](../../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)).

**A pack is handed a context when it registers:** a supervisor scoped to what it may start, its folders as checked paths, the redactor, and telemetry. It starts a process only through that supervisor and opens a file only by a checked path ([section 204](204-supervising-subsystems.md)). The product crate joins a pack's own crates where there are several, as it does for the web pack's shared browser handle, and does nothing else with a pack.

So the compiler stays neutral: it parses, validates against whatever schemas are registered, resolves names, type-checks expressions, counts commit points, lowers through whatever lowerings are registered, and emits. "Knowledge lives in one place" becomes: knowledge about a kind of system lives in its pack.

## Where today's files go

Every file kind has one owner, and every word that arrives with a pack, a file kind among them, is a registered name. Registration is keyed by the file kind and the surface kind, so two packs can each own `screens/` for their own surfaces. A pack that needs another pack's kind declares it reads the kind, and the compiler hands it the entries as data. If the owning pack is not enabled, a binding that needs its kind does not compile.

| File kind | Owner | Read by |
| --- | --- | --- |
| `target.yaml`, `auth.yaml`, `types.yaml`, `operations/`, `pipelines/`, `sources/`, `tests/`, `fixtures/` | Kernel | Every pack, through the compiler |
| `screens/`, for web surfaces | The web pack | None |
| `screens/`, for desktop surfaces | The desktop pack | None |
| `endpoints/` | The HTTP pack | The web pack, for its `page_http` mode |
| New kinds, such as `commands/` for the command pack | The pack that brings them | Whichever packs declare they read them |

An operation file is the kernel's, and each of its bindings is checked by the pack whose mode it names.

## The first-party packs

| Pack | Modes | State |
| --- | --- | --- |
| Web | `ui`, `page_http`, `site_tools` | Designed in sections 301, 301, 302, 401 and the designs of `a2m-pack-web-browser`, `a2m-pack-web-ui`, and `a2m-pack-web-probe`; first to be built |
| HTTP | `http`, `api` | Designed in `a2m-pack-http`; Phase 4 |
| MCP | `mcp` | Designed in `a2m-pack-mcp`; Phase 4 |
| Desktop | `desktop` | Designed in `a2m-pack-desktop`; Phase 8 |
| Command | `cli`, proposed | Designed in [section 306](../3-packs/306-the-command-pack.md); its crate is `a2m-pack-command`; the second pack to be built, to test the boundary (Stage D) |
| Data | `sql`, proposed | Designed in [section 307](../3-packs/307-the-data-pack.md); its crate is `a2m-pack-data` |
| File | `file`, proposed | Designed in [section 308](../3-packs/308-the-file-pack.md); its crate is `a2m-pack-file` |
| Stream | `stream`, proposed | Designed in [section 309](../3-packs/309-the-stream-pack.md); its crate is `a2m-pack-stream` |
| Device | `device`, proposed | Designed in [section 310](../3-packs/310-the-device-pack.md); its crate is `a2m-pack-device` |

A proposed mode becomes real by its own decision record when its pack is built. The crate designs are in [the architecture](../../architecture/README.md); [section 106](../1-product/106-what-it-latches-onto.md) catalogs the kinds of system still without a pack.

First-party packs are compiled into the one binary and switched on by a setting ([section 605](../6-running-it/605-packaging-and-release.md)).

## The bar a new pack must clear

A kind of system is supported only with all of these.

| # | The pack must bring | Why |
| --- | --- | --- |
| 1 | A driver that implements the driver interface for its verbs, with no retries inside | The engine loop is the same for everyone |
| 2 | A read-back rule for every write it offers, or the write is `irreversible` | Rule three |
| 3 | An answer for each identity strategy: how a record of its kind is found again | Resume without duplicates |
| 4 | A test stand that imitates its kind and reports ground truth through a control interface | A test never proves a write by our own read path alone |
| 5 | A discoverer, if its kind can be inspected, whose findings never claim more than was observed | Section 218 |
| 6 | Its failures mapped to the existing classes | The class alone decides what happens next |
| 7 | Its effects and hazards declared | Section 213 |
| 8 | A decision record for each new mode | Modes are public vocabulary |

A pack that cannot bring item 2 for its writes can still be accepted for reads.

## Packs and plug-ins

| | Pack | Plug-in |
| --- | --- | --- |
| Lives | In this repository, as Rust crates | Anywhere, as a separate process in any language |
| Trust | Reviewed with the product | Signed, or off by default; its manifest approved by an administrator |
| Reaches the kernel | Through the registrations above | Through the plug-in protocol, which carries the same six things |
| Suits | Kinds of system most people need | Rare, heavy, or proprietary kinds |

The contract is one. A plug-in that proves itself can become a pack without the kernel noticing.

## Open points

- How a plug-in registers schemas and lint rules across a process boundary: as data at start-up, most likely.
- Whether the neutral part of the capability probe belongs in the kernel.
