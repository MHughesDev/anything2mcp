# 0067. The kernel is built layer by layer, in parallel, and the first driver through the loop is the HTTP pack's

**Status:** Accepted. Design decision, taken by the lead agent when the owner asked for the whole system to be built without stopping. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the order of the [roadmap](../plan/roadmap.md) for milestones 0 to 4, and its rule that one slice finishes before the next starts. Nothing in the design of the product changes.

## Context

The roadmap built the kernel as a chain of thin slices, one after another, each ending in a proof, with the web pack's interface driver as the first driver and a spike of the browser engine before it. That order suits one agent working alone. On 2026-11-17 the owner asked for the full system to be built end to end, overnight, by as many agents as could work at once.

Three facts shaped the answer.

- Every crate has a written design with its interface sketched ([record 0063](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md)), so a crate can be built whole against the crates below it without waiting for a slice to pull each piece into being.
- A dependency only points down the eleven layers, so the crates of one layer can be built at the same time once the layer below is in place.
- The browser engine is the largest and least certain piece of the first packs. The engine loop does not need it: a driver that sends requests exercises the same loop, the same ledger, and the same verification.

## Decision

1. **The kernel is built in the order of its dependencies, a layer at a time, with every crate of a layer built in parallel by its own agent.** Each agent builds its crate's whole public interface, in the order of the design's build order, and proves the invariants its design lists. The lead commits a wave when its crates build and pass together, opens a pull request, and merges it when every check is green.
2. **The first driver through the engine loop is the HTTP pack's request driver, against the test application's JSON interface.** The web pack's browser engine and interface driver are built beside it, from the result of the spike, and join the loop when they are ready. The first end-to-end read and the first verified write are therefore proven over requests, and proven again over the interface.
3. **Test stands, the benchmark harness, and the spikes run beside the kernel from the first hour,** because they share no crate with it.
4. **The second kind of system is not held back.** The command pack, and the data, file, stream, device, and MCP packs after it, start as soon as the provider contract exists. Hypothesis H-26, that a new kind of system needs no kernel change, is tested by counting what each pack had to ask of the kernel, as before, and by more packs at once than one.
5. **One SQLite for the workspace.** The store, the data pack, and the test stands all use `rusqlite` with SQLite built in. Two crates that link SQLite cannot be in one build, so the choice has to be made once. Postgres, when package 6.1 adds it, sits behind the store's own interface with its own client.

The proofs do not change. A package still counts as done only when its proof in the roadmap is shown, and the roadmap's rows are marked as each proof is reached. The exit test of each milestone is still run and written in the journal.

## What follows

- The roadmap's "one slice at a time" rule is replaced, for this build, by "one crate, one agent, at a time". Two agents still never change the same crate at once.
- An interface that turns out wrong is found later than in a chain of slices: when the crate above is built, not when the slice is. The cost is accepted. Each agent reports every interface it changed against the design, and the lead carries the change to the crates above.
- The roadmap's package 1.1 is no longer the place where the driver interface is settled. It is settled by the agent that builds `a2m-provider`, recorded, and confirmed by the first two packs.
- Design review rounds are not skipped. One is run when the first verified write works end to end.

## Rejected

- **Keep the chain of slices.** Safer for the design, and far too slow for the request. The designs that make parallel work possible were written for this.
- **Wait for the browser engine before the loop.** It puts the least certain piece on the path of everything else.
- **An SQL crate that speaks both databases, now.** It would compile a second SQLite or force its version on every other user, and Postgres is not needed until a server is built.

## What would reopen it

Interfaces between layers that have to be redone more than once after the crates above are built; or a pack that cannot be built without changes across the kernel. Either means the designs were not ready to be built in parallel, and the chain of slices returns for what remains.
