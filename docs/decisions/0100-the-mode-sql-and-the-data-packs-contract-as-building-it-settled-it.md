# 0100. The mode `sql`, and the data pack's contract as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Carries out:** item 8 of the entry bar of [spec section 307](../spec/3-packs/307-the-data-pack.md), which asks for a record when the mode `sql` becomes real ([decision 0060](0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md)). **Changes:** section 307, where the code differs.

## Decision

1. **`sql` is a registered mode of the request archetype.** `a2m-pack-data` registers it, with the surface kind `database`, the file kind `statement` in `statements/`, the verbs `read`, `list`, `search`, `create`, `update`, and `delete`, and the step kinds `begin`, `execute`, `fetch`, `expect_rows`, and `commit`. It fitted the archetype without bending it: a request, the store's acknowledgement, a read by key.
2. **A sixth step kind, `catalog`.** A discoverer reaches a system only by sending steps through the sandbox port, so reading the catalog is a step of the mode. No lowering emits it. *Instead of:* a second route from the discoverer to the store, which the contract does not give and should not.
3. **The first store is the embedded SQL engine, through `rusqlite`,** the workspace's one binding to it. No outside crate was added. What is specific to the store is behind two traits inside the crate, `Opener` and `Conn`: quoting, opening a connection held to an access and reading that back, beginning, describing, running, committing, the catalog, and what an error means. *Instead of:* calling `rusqlite` from the driver, which would have to be undone for the first server store; *and instead of* a layer over many databases, which section 307 already refuses.
4. **A `database` surface states `store`, `file`, and `root`.** `file` is the address: the path of the store's file below a named root, with forward slashes. The driver takes the file from `PrepareContext::paths`, as a checked path under the surface's name, and only when its root and its path are the ones the surface states. *Instead of:* an address that is a path on the machine, which a pack may never resolve.
5. **A record's `key` lists fields, not columns.** Each key part then has a field type, which a parameter and a typed result need. Section 307 said columns.
6. **Every statement names a record,** and every result column of a read is the column of one of the record's fields, or its version column. A column the record does not declare is drift. *Instead of:* a separate list of result columns on each statement.
7. **The version a guard compares is a parameter named `version`, filled by the binding.** A lowering can read only `input` and `env`, so the version the preview saw has to arrive as an input of the operation. *Instead of:* a way for a lowering to name a value of the preview, which the kernel does not have; see the gaps below.
8. **The binding states its verb,** and the verb must be the one its statement is bound with. `read` is the default of the mode, because the kernel allows one default per mode and cannot take it from the statement.
9. **A statement's shape is held to the plan at first use in a session, not when files are compiled.** The compiler is pure and has no connection. The driver asks the store to describe a statement the first time a step runs it; a different parameter count or an undeclared column is `drift`, `schema_changed`, before the statement runs.
10. **Isolation.** The embedded engine's transactions are serializable, which is at least as strong as any level a target may state, so every stated level is accepted.
11. **The tests live inside the crate.** The fault of a lost answer to `commit` is placed at the private store interface, which a test outside the crate cannot reach. The driver is still reached only through the registry's handle, with a real intent token.

## What the kernel could not express (hypothesis H-26)

No kernel crate was changed. These are the places where the pack built less than section 307 asks, or built around a gap:

- `Observed` has no place for what a driver did (`Acting`), so nothing a driver notes reaches the step trace.
- `StepFailure` carries no capture, so a constraint's name cannot be handed on as untrusted evidence.
- A lowering cannot refer to a value the preview read, which `guard: version` and `guard: before_values` need.
- The identity strategies have no word for a key the store assigns and the statement returns; such a create compiles only with `identity: none`, as `irreversible`.
- A manifest lists roots by name, so a pack cannot declare a root a person added later.
- Nothing says how a surface whose address is a file becomes an entry of `PrepareContext::paths`.
- A discoverer is given no clock; the pack takes telemetry's when it registers.
- A drafting rule writes one file kind, and a pack cannot draft the kernel's `operations/`, so the drafted read has no drafted operation.

## Consequences

- The design of `a2m-pack-data` carries the interface as built, and names what is not built.
- Section 307 is corrected where this record differs from it.
- Hypothesis H-29: `sql` fitted the request archetype.
