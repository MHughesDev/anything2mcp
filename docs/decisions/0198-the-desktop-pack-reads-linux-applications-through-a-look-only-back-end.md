# 0198. The desktop pack's first slice reads Linux applications through a look-only back end

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** points 1 and 9 of [decision 0120](0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md) and the note added to it when it was merged; the registration table, failures, and build order of the [desktop pack's design](../architecture/crates/a2m-pack-desktop.md); [spec section 305](../spec/3-packs/305-the-desktop-pack.md) where it says otherwise. Carries out the first slice that [research note 26](../research/26-desktop-pack-before-it-is-built.md) recommends.

## Context

Package 8.3 starts with reads on Linux, proven against a stand with no person at the machine. Note 26 recommended AT-SPI calls of our own over the workspace's `zbus` 6, in place of the `atspi` crates, which require `zbus` 5; and an egui program as the stand, test-application case 28.

## Decision

1. **Linux back end: AT-SPI over the workspace's `zbus` 6.3.0, through calls of the pack's own.** No new crate: `zbus`, `sha2`, and `futures-core` were in the workspace already, each confined by `cfg` to Linux. Every call passes one function that sends only the members on a fixed list, each of which asks: `GetAddress`, `GetConnectionUnixProcessID`, `Properties.Get` of `Name` and `IsEnabled` only, `GetChildren`, `GetRole`, `GetState`, `GetRelationSet`, `GetInterfaces`, `Text.GetText`, and `RegisterEvent`/`DeregisterEvent` so that a toolkit that sends events only to listeners sends them. A test holds the list against the members of AT-SPI that act. *Instead of:* the `atspi` crates, as decision 0120 chose, which bring a second major version of `zbus` and the three crates that decision 0123 found unmaintained.
2. **The handle is look-only by its type.** The back end's `Look` has no method that sets a value, invokes, selects, focuses, scrolls, or sends a key or a click. A read cannot press Save by any route, whatever a target file holds. The application's own writes, such as autosave, cannot be stopped this way (note 26); the stand's write count proves it for the stand only.
3. **The stand is `targets/example-desk`, a program the test hosts that publishes its tree over AT-SPI itself,** through the system's real bus launcher and registry, on a session bus of the test's own. It is not egui: egui's AT-SPI adapter, `accesskit_unix` 0.24.1, requires `atspi` 0.29 and `zbus` 5.19, whose tree holds `zvariant` 5, `zvariant_derive` 5, and `zbus_names` 4, flagged unmaintained (RUSTSEC-2026-0370, -0371, -0372). In the workspace, it would fail CI's advisory check, which is never weakened; outside the workspace it would dodge that check. *Rejected as well:* `gtk4`, whose system library every Linux build of the workspace would then need, and whose table support was not checked. The cost: no real toolkit is in the loop. The evidence that toolkits answer these calls is the spike's seven runs against GTK 3 and egui (note 19). A stand on a real toolkit returns when egui's adapter moves to `zbus` 6, or as a second stand on one CI job that accepts the GTK library.
4. **No display, and nothing of the person's.** The tests need no display: a bus daemon of their own, with a configuration of their own, and the at-spi2-core programs, which it starts on demand. Every program the bus starts gets no display variable, a runtime folder of the test's own, and settings kept in memory, so nothing reaches a person's display, accessibility bus, or settings. A machine without the programs skips each test and says so; with `A2M_REQUIRE_DESKTOP=1`, as the data pack's `A2M_REQUIRE_POSTGRES`, it fails.
5. **The application is found, never started.** A surface of the kind `desktop` states `program`, the absolute path of the application's program, which is its address; `application`, the name it gives itself on the accessibility bus; and optionally `digest`. The pack lists the applications the registry holds and asks the bus daemon, not the application, which process owns each connection; it compares the program that process runs, by the user that runs it, with `program`; only then does it ask that application its name. None: `busy`/`application_not_running`. The program running, publishing nothing, with the session's accessibility switch off: `busy`/`accessibility_off`, and no setting is changed. Several: `unknown_variant`/`application_ambiguous`. Another digest: `unknown_variant`/`application_not_proven`. The process table is read to tell the first two apart; nothing else of another application is read.
6. **Unknown means stop.** A locator is a role from the pack's closed list of role words and an accessible name, both exact: the element's own name, else what labels it, in the same application only. None: `drift`/`locator_not_found`; several: `drift`/`locator_ambiguous`. A screen's window is matched by role and title among the application's own windows, exactly. A walk is bounded by depth, nodes, and the step's deadline; a window that does not fit is not read.
7. **A wait listens, then looks.** The driver subscribes to the application's signals, reads the condition, and reads it again on each signal until it holds or the step's deadline passes. It never decides from a signal's payload, and never sleeps.
8. **A secret is present and never read.** An element of the role `password_text` is marked secret; its text is never asked for. A read of it is `policy`/`password_read_refused`; a snapshot says it is there.
9. **Writes are refused by name.** The mode serves `read` and `list`; a desktop binding on an operation that writes is `desktop.writes_not_built`.
10. **Discovery names its finding `window`,** not `screen`, so that the web pack's rules over `screen` findings never meet it. A finding keeps roles, structure, and the names that are the window's shape: a control's name, a table's name, a column's heading, a label that labels something. It never keeps a cell, a row, a paragraph, or a label that labels nothing, and never a value read through the text interface.

## Alternatives considered

- **The `atspi` crates.** Above.
- **egui as the stand, outside the workspace.** It would keep a real toolkit, and keep unmaintained crates out of sight of the check.
- **Launching the application from the surface's program.** The supervisor has no link kind for a program watched until stopped (the design's first open point), and a launch is an act on a person's machine. It waits for the write half.

## Not built

Writes (set, invoke, a commit point, a confirmation, a read-back); the Windows and macOS back ends, where the driver stops at prepare with `busy`/`system_not_built` and the compiler side works; keys and clicks; starting an application; the discoverer `desktop_probe`; the design's lint rules (a `css` locator is refused by the schema, an empty name by `desktop.unnamed`, every condition this slice offers is signalled on Linux); controls beyond `text`; the process's start time in its proof.

## Open, for the owner

Granting the accessibility permission on a hosted macOS CI machine by writing the system's permission database, as note 26 describes, is not decided and not written. It is the owner's to decide.

## What was shown

On Linux, against the stand: the drafted reads compile once the file that holds a question is left out, and return the stand's order and line items; the late value is waited for and read once the stand signals it; the two controls named `Reference` stop as `locator_ambiguous`; the password field stops as `password_read_refused`, and its value is in no result, finding, draft, or log; the stand counted no press and no member that acts; a program nothing runs is `application_not_running`. The suite runs in about one second.
