# 0480. The desktop pack starts an application on Windows by the event of a window shown, and not yet on macOS

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** point 4 of [decision 0467](0467-the-desktop-pack-starts-an-application-a-surface-allows-does-an-action-by-a-key-the-element-binds-and-probes-its-reach.md) for Windows; the desktop pack's failure table, invariants, and build order in [its design](../architecture/crates/a2m-pack-desktop.md); [spec section 305](../spec/3-packs/305-the-desktop-pack.md); the stand `targets/example-desk` on Windows; the desktop rows of [SCOPE.md](../../SCOPE.md) and [section 704](../spec/7-project/704-readiness.md); row 8.3 of the [roadmap](../plan/roadmap.md); CI's Windows desktop job. It leaves decision 0467's points 1 to 3 and 5 to 8 as they are, and point 4 for macOS.

## Context

Decision 0467 built a surface's `start: true` on Linux and stopped it on Windows and macOS as `start_not_built`, because neither back end had an event that says a new application's window appeared, and a wait polls nothing. It named the gap on each: on Windows a hook for a window shown by any process, on macOS a notification of a launched application. It built an action by a key on Linux and stopped it on both other systems as `keys_not_built`, because neither interface lists the key a field's action is bound to.

The Windows back end already listens to the system's accessibility events through an out-of-context hook on a thread of its own (decision 0209), filtered to the proven application's process. The same hook, set for every process, hears a window shown by a program that has just started.

## Decision

1. **Windows starts the application a surface allows,** with decision 0467's rules unchanged: only in a run, only when no process of the engine's user runs the program, only a program the provider's check passes, through the scoped supervisor, which checks the digest the surface states; no argument and nothing on its standard input. The driver's code that starts is the one Linux uses.
2. **The wait is by the event of a window shown.** Before the start, a listener sets one out-of-context hook for `EVENT_OBJECT_SHOW` of every process of the desktop but the engine's (`WINEVENT_SKIPOWNPROCESS`). It wakes the wait only when the object shown is a window itself (`OBJID_WINDOW`, `CHILDID_SELF`), and looks at nothing else of the event; the wait then lists the top-level windows again and proves the application exactly as at attach (its program, session, user, level, digest, and name). One that shows no window by the prepare's deadline is stopped, as `busy`/`application_not_started`. An event of another program costs one listing and decides nothing.
3. **Whether the program runs** is read from the process table (`K32EnumProcesses`), each process's image, whether it has exited, and its token's user. A process whose user cannot be read counts as the engine's: no start is made on a doubt.
4. **The variables passed on Windows** are `SystemRoot`, `windir`, `SystemDrive`, `USERPROFILE`, `USERNAME`, `USERDOMAIN`, `HOMEDRIVE`, `HOMEPATH`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, and `TMP`. Not `PATH`: the supervisor holds every folder a `PATH` names to the check of who may change it, a person's `PATH` on Windows commonly names a folder others may change, which would refuse the start, and a desktop application loads its libraries from its own folder and the system's.
5. **macOS still starts nothing,** as `start_not_built`. The interface the back end uses, an observer of one process's accessibility notifications, can be set only once the application answers that interface, so it cannot say when a just-started one begins to; the notification of a launched application is the workspace's, which arrives on the main thread's run loop, and the engine's main thread is not one the pack runs.
6. **Keys on Windows and macOS stay `keys_not_built`,** for decision 0467's reason (point 7): the element that a key commits is not named by either interface. A window's default button (on macOS `AXDefaultButton`, on Windows the default state of a push button) is what the key presses only when the focused field does not take the key itself, and no interface says whether it does: unknown means stop.

*Instead of* a fresh listing of the windows on a timer until the deadline: it is a poll, and decision 0467 refuses one. *Instead of* a new link kind of the supervisor that reports the started process's first window: no kernel contract changes, as decision 0467 point 2 holds.

## Consequences

- `tests/start.rs` runs its four tests of a start on Linux and Windows, against the stand as a program of its own: on Windows a copy of the test binary named `example-desk-app.exe` or `example-desk-mute.exe` in the user's temporary folder, served from a start function in the C runtime's initializer section that `stand_child!` declares, showing the stand's window with no control interface, or nothing. The refusal of a program another user can replace is proven there on a file open to everyone, set with the system's `icacls`, as the MCP pack's test does. On macOS the test of `start_not_built` stays.
- CI's Windows desktop job runs `tests/start.rs` again on its own and fails unless its five tests pass (the four and the stand's child).
- The stand's started copy and the pack's Windows code were written and linted for the Windows target from Linux; neither has yet run on Windows.

## Not built, and what would reopen it

Starting an application on macOS; a key on Windows and macOS; the process number of a started program in the supervisor's answer. A way, on macOS, to hear that a process has begun to answer the accessibility interface without the main run loop reopens point 5. A Windows run in which a started application's window is shown before the hook is set, or is never announced by `EVENT_OBJECT_SHOW`, reopens point 2.
