# 0490. The desktop pack still starts nothing on macOS, because no event off the main run loop says an application answers

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** the reason [decision 0480](0480-the-desktop-pack-starts-an-application-on-windows-by-the-event-of-a-window-shown-and-not-yet-on-macos.md) gives in its point 5, and its points 5 and 6 for macOS, made exact: the designs weighed for a wait, why each is refused, and what would reopen it; the open point and the build order of [the desktop pack's design](../architecture/crates/a2m-pack-desktop.md). It changes no code path: on macOS a surface that allows a start, with no application of its program running, still stops as `busy`/`start_not_built`, nothing started, and an action by a key still stops as `busy`/`keys_not_built`, nothing pressed. It leaves Linux and Windows as decisions 0467 and 0480 built them.

## Context

Package 8.3 asked for a surface's `start: true` on macOS by a wait that needs no main run loop and is no fixed sleep. The driver's code that starts is shared (decision 0467, point 1): only a run starts, only when no process of the engine's user runs the program, only a program the provider's check passes (a shell, an interpreter, a script, or a file in a folder another user can replace is refused), through the scoped supervisor, which checks the digest; one that shows nothing by the deadline is stopped. What each system needs of its own is an event that wakes the wait when the started application can be proven. On Linux it is the accessibility bus's signals; on Windows the system's event of a window shown by any process. The macOS back end answers one question about an application once it answers the accessibility interface; the wait needs the moment it begins to.

## Decision

1. **macOS starts nothing in this version,** as `start_not_built`. Each design weighed fails on a fact of the system, not on work left undone:
   - **An observer of the started process's notifications on a thread of the pack's own, with its own run loop.** The thread and the run loop are no obstacle; the back end's watch already runs one so (decision 0244). The observer is not: adding a notification to an application's element asks the application, and an application that has not yet begun to answer the interface refuses it (`kAXErrorCannotComplete`, as reports of a freshly launched application say). The observer can be set only after the moment the wait is for, so it cannot announce that moment. Trying again until it takes is a poll.
   - **A kernel event on the process (`kqueue`, `EVFILT_PROC`).** The kernel says that a process forked, ran a new program, got a signal, or exited. None of these is that its accessibility answers. The exit is worth hearing, to end the wait at once, and the back end already listens for it; it cannot start the wait's reads. A series of probes needs a condition that marks each try, and the kernel gives none between the start and the exit.
   - **The workspace's notification of a launched application, or a running application's `finishedLaunching`.** These are the system's own word, and the platform says both change only while the main thread's run loop runs. The engine's main thread runs the engine's runtime, not the system's run loop, and a pack does not choose that.
   - **Starting the application through the workspace (`openApplication`), whose answer comes on a queue of its own.** It would start the program through the system's launch service, not the supervisor: the digest would not be checked at the start, no process group or job would hold it, and the environment would be the launch service's. Rule 5 and decision 0467, point 1, refuse it.
   - **Listening to another program's accessibility** (the program that shows the dock, or whichever application has the focus) for a sign that a new one appeared. It depends on what another application does, a program started without the focus gives no sign, and it names nothing the system promises.
   - **A fresh probe on a timer until the deadline.** It is a poll, which decision 0467 and the rule against fixed sleeps refuse.
2. **The refusals of a start stay as they are on every system.** No change here touches the discovery's refusal, the digest, the check of who may change a program, or the stop of a program that shows nothing; on macOS none of them is reached, because nothing is started.
3. **Keys on macOS stay `keys_not_built`.** The accessibility interface has an element action named `AXConfirm`, which its header is remembered as describing as pressing Return in the element, and `AXCancel`, a cancel. It would fit decision 0467, point 5: the element's own action bound to the key, never a key event. It is not built here because two facts could not be checked in this session: the header's own words, and whether the standard text field offers `AXConfirm` and does with it what Return does. Building on a meaning remembered, and not read, would be a guess, which rule 7 refuses.

## Consequences

- `tests/start.rs` keeps its test of `start_not_built` on macOS, and CI's macOS desktop job runs it with the rest of the pack's tests. `tests/keys.rs` keeps its test of `keys_not_built` there.
- The macOS row of the roadmap's package 8.3 is unchanged: starting an application, and an action by a key, are not built there.

## Not built, and what would reopen it

Point 1 reopens when either is true. The product runs the system's run loop on its main thread on macOS, with the engine's runtime on a thread of its own: then the workspace's notification of a launched application, filtered to the program the surface declares and checked against the process table as now, wakes the wait, and the application is proven as one found running. That is the product crate's change and needs a record of its own. Or a run on macOS shows an event, heard on a thread of the pack's own, that comes when a just-started application begins to answer the interface.

Point 3 reopens when the header of the accessibility actions has been read, and a run on macOS shows a standard text field that lists `AXConfirm` and saves as Return does. Then `enter` is done as `AXConfirm` on an element that lists it once, through the acting half's closed list, and `escape` waits on the same test of `AXCancel`.
