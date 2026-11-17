# 0063. The kernel's interfaces, as the crate designs settle them

**Status:** Accepted. Design decision, taken while writing a design for every crate. Nothing here is built; the slice that builds each interface confirms or changes it by its own record. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open points under "Driver interface" in the technical reference; the open point of spec section 55 on where auth providers live; several open points of spec section 42.

## Context

The design said what the system does. It did not say which crate defines each interface, who implements it, and who joins the two. Writing that down for all 24 crates, in parallel and then reconciled, forced a set of choices. They are recorded together because they depend on each other. [The architecture](../architecture/README.md) holds the result; this record holds the alternatives.

## Decision

**The engine and the control plane**

- The engine reaches everything through one interface, `Control`: the plan by its hash, the policy decision at guard, a session lease, a record lock, a rate budget, the approval check, recording an intent, and sending events and evidence. It never opens the store, the policy engine, or the compiler. *Instead of:* the engine calling the store directly, which works only in one process.
- The plan's types live in `a2m-core`, as [decision 0036](0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md) says, and the audit refuses a dependency from the engine on the compiler. *Instead of:* plan types beside the compiler.
- Discovery is a job. A runner runs discoverers inside the engine's sandbox, and a discoverer is given only what the sandbox offers. *Instead of:* the action registry calling a discoverer, which would let the control plane touch a target.

**The driver**

- Four calls: prepare, act, health, release. Confirming and reading back are lists of steps that `act` carries out. *Instead of:* a separate read-back call taking a rule.
- A driver is reached only through a handle that refuses a committing step without the ledger's intent token. *Instead of:* trusting each driver to check.
- Every failure says whether the request was not sent, was answered, or is unknown. A step marked as a lookup may report a record absent. A step's outcome may carry values known before the commit point, which the engine records first.
- A mode's place in the default order, and its limits on parallel work, are data in its provider's manifest. *Instead of:* an order written into the engine, which would have to name modes.

**The store**

- One store, one numbered sequence of migrations, one owning crate per table. No crate's SQL names another's table, with one declared exception: when writes begin, the ledger checks the approval that policy recorded, in the same transaction.
- Record locks belong to the ledger, because an intent must check the lock in the same transaction. The scheduler only queues the wait.
- Evidence is named by a keyed digest of its content. *Instead of:* a plain hash, which would let anyone who can read the store confirm a guessed value.
- The vault seals and opens and stores nothing itself; its wrapped keys are kept through an interface the store implements.

**People**

- Recording a person's approval is not an action and has no generated face. The approval surface holds a handle that can record an approval or a refusal and nothing else. *Instead of:* an approve action hidden from agents by a setting.
- A front door reads a caller's credential; the action registry checks it. Role crates never use each other; the product crate joins them.

**Sign-in**

- An auth provider lives in `a2m-auth` when it only holds or exchanges a credential, and in a pack when it needs a surface of the system. Sign-in forms and single sign-on are in the web pack; authorization to an upstream MCP server is in the MCP pack; the rest are in `a2m-auth`.
- A provider outside the web pack that needs a person's consent in a browser gives the person a link. Nothing drives that browser.
- Only the session keeper renews, across processes as well: it takes the renewal by a compare-and-set on the session's row.

**Providers**

- The manifest is a TOML file. A first-party pack embeds its own.
- The registry is an immutable snapshot, replaced whole when a provider is enabled or disabled.
- Anything that starts a process does so through the supervisor, scoped to what the manifest and the published plan declare.
- File access for packs is a path that can only be made by the check against the allowed roots.

**Packaging**

- One binary is shipped. `a2m` is a link or a launcher on each install route. The workspace builds two small targets from one module for convenience.

## Consequences

- The technical reference's driver sketch, plug-in protocol table, and auth provider table are replaced by pointers to the crate designs and a table of where each provider lives.
- Several spec sections now lag the designs: section 46 on the order of the compiler's stages, section 57 on children that perform no handshake, section 62 on reaching another machine, section 40 on the size of the build toolset, and others. They are corrected when the specification is restructured; the list is in the journal.
- Eight hypotheses are added, H-32 to H-39, for the choices here that rest on judgment.

## Alternatives considered

Given beside each choice above.

## Revisit if

Slice 1.1 builds the driver interface and `Control` and finds either wrong. That is what the slice is for.

## Spec sections

- [The architecture](../architecture/README.md) and the designs in [crates](../architecture/crates)
- [The technical reference](../reference/technical-reference.md)
