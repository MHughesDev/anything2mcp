# 0061. The layers are redrawn so that every needed dependency is allowed

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Refines:** [0045](0045-the-workspace-starts-with-every-crate-and-the-audit-enforces-the-layering.md) and the layering table of [0056](0056-one-name-everywhere-anything2mcp.md). The rules of 0056 stand; the numbers change.

## Context

The layering table was drawn before any interface was placed in a crate. Writing out who defines, implements, and calls each interface ([the architecture](../architecture/README.md)) showed four dependencies the design needs and the table forbade, because a crate may not depend on another in its own layer:

- the compiler type-checks expressions, and `a2m-files` shared a layer with `a2m-expr`;
- policy, the session keeper, and the scheduler all persist through the store, and `a2m-policy` and `a2m-auth` shared a layer with `a2m-ledger`;
- the registry holds auth providers and source readers, and `a2m-provider` shared a layer with `a2m-auth` and `a2m-sources`, where those interfaces are defined;
- a driver is handed a session lease and a plan step, so the driver interface must sit above the crates that define them, and a pack must be able to see it.

## Decision

Eleven layers. The kernel is 1 to 8, the packs are 9 and 10, the product is 11.

| Layer | Crates | May depend on |
| --- | --- | --- |
| 1. Types | `a2m-core` | Nothing internal |
| 2. Foundations | `a2m-expr`, `a2m-vault`, `a2m-telemetry` | Layer 1 |
| 3. Stores | `a2m-files`, `a2m-ledger` | Layers 1 to 2 |
| 4. Services | `a2m-policy`, `a2m-auth`, `a2m-sources` | Layers 1 to 3 |
| 5. Contract | `a2m-provider` | Layers 1 to 4 |
| 6. Engine | `a2m-engine`, `a2m-sched` | Layers 1 to 5 |
| 7. Actions | `a2m-actions` | Layers 1 to 6 |
| 8. Roles | `a2m-runner`, `a2m-mcp`, `a2m-api`, `a2m-local` | Layers 1 to 7 |
| 9. Pack bases | `a2m-pack-web-browser` | Layers 1 to 5 |
| 10. Packs | The other `a2m-pack-*` crates | Layers 1 to 5, and the base of its own pack |
| 11. Product | `anything2mcp` | Everything |

- **A pack sees the kernel up to the contract, and nothing above it.** It cannot call the engine, the scheduler, the action registry, or a front door.
- **An interface is defined in the lowest crate that must call it,** implemented where the knowledge is, and joined to its caller by a crate above both. Where two crates in one layer would need each other, one defines a trait and a higher crate supplies the implementation: the engine's `Control`, the keeper's `KeeperWork`, and `Redact` in the core are the three cases.
- **Everything that persists goes through `a2m-ledger`.** A service owns the meaning of its tables; the store owns the connection, the transactions, and the migrations.
- **Every entry into the kernel is an action,** and `a2m-actions` holds the handle to the assembled kernel. No new crate is added for assembly.

## Consequences

- The technical reference's table, the audit's self-test, and AGENTS.md change with this record. No crate has an internal dependency yet, so no code changes.
- The engine and the scheduler stay in one layer with no edge between them. A runner joins them.
- A pack may see the store. It has no reason to use it, and a pack that does is a finding for review, since the audit's rule is by layer.

## Alternatives considered

- Put every extension-point interface in `a2m-core`: no layering problem at all, and the core would have to define sessions, plans, and findings in full, becoming the whole kernel's types in one crate.
- Allow dependencies inside a layer: simpler table, and nothing would stop a cycle forming between two services.
- A separate crate that assembles the kernel: clearer name, and one more crate holding little but a constructor that `a2m-actions` needs anyway.

## Revisit if

A real dependency turns up that this table forbids. That is a finding about the design before it is a reason to move a crate.

## Spec sections

- [The architecture](../architecture/README.md)
- [The technical reference](../reference/technical-reference.md#crate-layering)
- [55-kernel-and-packs.md](../spec/2-kernel/202-kernel-and-packs.md)
