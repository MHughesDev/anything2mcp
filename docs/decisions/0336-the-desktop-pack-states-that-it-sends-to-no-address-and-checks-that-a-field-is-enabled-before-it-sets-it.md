# 0336. The desktop pack states that it sends to no address, and checks that a field is enabled before it sets it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged. It departs in one point from what [decision 0334](0334-the-device-stream-and-data-packs-state-where-their-tools-send.md) wrote down for this pack, and says why.

**Date:** 2026-11-23

**Changes:** `Lowering::statement` of `a2m-pack-desktop`; the driver's `set` on Linux; the stand `example-desk`; [section 305](../spec/3-packs/305-the-desktop-pack.md), the design of [`a2m-pack-desktop`](../architecture/crates/a2m-pack-desktop.md), `SCOPE.md`, and the roadmap row P2. **Carries out** the desktop pack's part of package P2, and the parts of [research note 33](../research/33-what-a-program-can-reach-in-another.md) that [decision 0319](0319-a-publications-mark-that-a-call-only-reads-is-a-published-claim-and-silence-is-not-a-read.md) owed to section 305.

## Context

Decision 0334 left the desktop pack's statement to its owner, and wrote that `Sends::NotStated` is the honest answer, because the program the pack drives may itself send, and what a program reaches is its own ([decision 0252](0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md)). The review's own words for `Sends` are narrower: where the binding sends requests. The question that matters to the person approving is what the engine can be made to dial.

Reading what the driver contacts: it asks the platform's accessibility interface on this machine (on Linux, the session's accessibility bus, a local socket; on Windows, UI Automation; on macOS, the accessibility interface), and the process table or the bus daemon to prove which process owns what it found. The scout's one other read, the definition of a management class on Windows, is asked of this machine's management service in a namespace the surface names. No code in the pack opens a network socket, resolves a name, or follows an address that an input, a screen, or the program gave.

## Alternatives

| | What the person is told |
| --- | --- |
| **(a) `NotStated`, as decision 0334 suggested** | "May reach others" on every desktop tool. True of the program, not of anything the engine dials, and the same warning would then say nothing that tells a desktop tool from a tool that dials a server |
| **(b) State "to no address", and say nothing of the program** | `Reach::Stated` with no address: the engine sends no request to any address. What the program does after an action is the program's, as for an HTTP server that forwards a request, which decision 0334 already leaves alone for the packs it covers |
| **(c) State the program as the surface's address** | Wrong: a surface's address here is a program's path, not a place a request goes |

**Chosen: (b).** It is a statement about what the engine dials, like the three other packs'. (a) would give the page a warning that does not tell the person anything, which decision 0334 argued against.

## Decision

1. **A binding of the desktop pack states that it starts no program, sets no variable, and sends to no address:** `Statement::Stated` with empty `runs` and `environment`, and `Sends::Only { surface: false, addresses: [] }`. It attaches to a program that is already running (decision 0198).
2. **One surface field can make the statement untrue, and then it is withheld.** A management namespace written with two separators first (`\\host\root`, `//host/root`) names another machine's management service, which the system would reach over the network with the person's identity. Where a surface states such a namespace, the statement is `NotStated`, as for a path of another machine in the data and file packs. The case is a compile test.
3. **The proof** is a compile test of the desktop example target, as for the device pack's: every tool's reach is stated with no address, and every binding is stated with empty `runs` and `environment`.
4. **Before every set, the driver reads whether the control is enabled** (the accessibility interface's enabled state), as well as whether it offers editable text and is not read-only, and stops before sending anything if it is not enabled. The stop is `field_read_only`, with a message that says the field is disabled. This is built on Linux, where the acting half is built. The stand has a fault that disables a field, and its own application refuses a value set into a disabled field, so a set that was sent could not come back as an unsent failure. On Windows and macOS the same read before every set is the design for the write slice (section 305), not yet built there.
5. **`automation_not_granted` is reserved in the pack's design** for a missing grant on a scripting route, should one be built. It is not in the code table, since nothing produces it.

## What would reopen it

A driver that opens a network socket, resolves a name, or reaches a management service on another machine, or a surface field that names an address, makes the statement false; the change must withdraw the statement in the same change, and the compile test fails first where it does not. A scripting route (Apple events, COM, D-Bus method calls) is a different route with its own consent and its own statement.
