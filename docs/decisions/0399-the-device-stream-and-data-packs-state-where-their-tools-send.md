# 0399. The device, stream, and data packs state where their tools send

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** `Lowering::statement` of `a2m-pack-device`, `a2m-pack-stream`, and `a2m-pack-data`; the designs of those three crates; [section 403](../spec/4-building-tools/403-testing-and-publish-gates.md), `SCOPE.md`, and the roadmap row P2. **Carries out** the part of package P2 that belongs to these three packs, as [decision 0252](0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md) and [decision 0264](0264-the-publish-page-renders-the-typed-review-sign-ins-first-from-one-keeper-call.md) set it up. No kernel crate, driver interface, provider manifest field, or file format changes. The desktop pack's part is not here.

## Context

The publish page shows, for each tool, what it starts, what it sets, and every address it can send a request to. A pack that does not answer `Lowering::statement` is shown as stating nothing, and the page says that a tool of such a pack "may reach others". The command, HTTP, MCP, file, and web packs answer. The device, stream, and data packs did not, though each of them dials one address and no other.

## Alternatives

| | What it costs | What the person approving is told |
| --- | --- | --- |
| **(a) State what each driver already holds to, and test it** | A statement is a promise about the driver. Where the driver might dial something else, the statement must be withheld or the driver must change | `Reach::Stated`: the surface's address in each environment, or none |
| **(b) Leave the three unstated until a driver has a second reader of "only"** | Nothing | "May reach others" on every tool of three packs, which is true of nothing they do |
| **(c) State the address from the target's files for every surface kind alike** | Nothing | Wrong for a store in a local file, which sends to no address, and for a path on another machine, which the operating system reaches |

**Chosen: (a).** (b) leaves the person to read a warning that is not true, and trains them to ignore it. (c) would say a local file sends to an address.

## Decision

1. **Each of the three starts no program and sets no variable.** None starts a child process or passes an environment; the statement says so as an empty list, not as silence.
2. **The device pack sends only over its link.** `Sends::Only { surface: true }`. The driver parses the address as a socket address without a lookup, refuses an address that is not among the run's destinations (`surface_not_declared`), and an input never names an address. A firmware update is frames over the same link. The page shows the address of each environment of the surface.
3. **The stream pack sends only to its broker.** `Sends::Only { surface: true }`. A channel's address is a subject, never a network address. The driver dials only a surface among the run's destinations, over plain TCP to this machine's loopback address, and does not follow the broker's own list of other servers.
4. **The data pack states by the kind of surface.** A store in a file (`database`) sends to no address: `Sends::Only { surface: false }`, stated where the file's address is a local path in form. A path that begins with two separators (`\\host\share`, `//host/share`) is reached over the network by the operating system, so it is `NotStated`, as for the file pack. A store on a server (`database_server`) sends to its surface's address, which is where the driver connects and the only address it may reach; its statement is `Sends::Only { surface: true }`. A kind this build does not serve is `NotStated`. The address shown for a server store is the host the surface declares; its port is the surface's own field and is not part of the address the review holds.
5. **Each statement is proven by a compile test.** The test compiles the pack's example target and reads the review: every tool's reach is stated, its addresses are exactly the surface's, and no binding runs a program or sets a variable. The data pack also proves a local file reaches no address, a path of another machine is not stated, and, with the feature `postgres`, a server store reaches its host only. The stream pack's real-server test ran against a real NATS server with the change; the data pack's against a real PostgreSQL server.

## What this leaves alone

The desktop pack: its tools drive another program's windows, so what that program sends is the program's, and its statement needs the pack's own design (below). Where a link or a broker leads once the connection is made (a device that forwards, a broker that bridges to other servers) is the system's, as for an HTTP server that proxies; the statement is about what the engine dials.

## What the desktop pack needs

Written down for the worker who owns it; not built here. The driver's own header (`crates/a2m-pack-desktop/src/driver.rs`) says nothing in it starts a program or reaches another application: it asks the platform's accessibility interface about a program that is already running, and proves it is the program the surface names (the surface's address is that program, and its `digest` field pins it) before it asks anything. So the part is, in the pack's `Lowering::statement`: (1) a `BindingStatement` with no `runs` and no `environment`, because the pack starts no program and sets no variable; (2) `sends`: the engine itself sends no request, but the program it drives may, and what a program reaches is its own (decision 0252), so `Sends::NotStated` is the honest answer unless the desktop pack can hold the program's traffic, which nothing in it does. `BindingStatement` has no way to say "starts nothing, sends not stated" apart from `Statement::Stated` with the default `sends`, which is `Sends::NotStated`; use that, so the page shows an empty `runs` and `environment` with the fixed notice that the reach is not stated. The proof is the compile test used for the other three packs, reading `compiled.review` of the desktop example target: each binding is `BindingStates::Stated` with empty lists and each tool's reach is `Reach::Partly`. Whether the page should say, beside that, that the program is attached to and not started is a wording for the review's fixed sentences in `a2m-files` and is left to its owner.

## What would reopen it

A driver that dials a second address (a broker's discovered servers followed, a hostname resolved, a redirect followed) makes the statement false; its change must withdraw the statement in the same change, and the test above fails first where it does not.
