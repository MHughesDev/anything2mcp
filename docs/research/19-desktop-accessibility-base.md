# Research: what the desktop pack stands on

**Status:** completed on 2026-11-17 by a spike and by reading sources. It answers brief 19 of [open research](11-open-research.md) and closes roadmap package 8.1. The spike is in [spikes/desktop-base](../../spikes/desktop-base/README.md). The choice it supports is [decision 0120](../decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md). Feeds [spec section 305](../spec/3-packs/305-the-desktop-pack.md) and the design of [`a2m-pack-desktop`](../architecture/crates/a2m-pack-desktop.md). Updates the desktop entry of [research note 09](09-rust-building-blocks.md).

## Question

To find a control by role and name, read it, act on it, and wait on an event, what does the desktop pack stand on, on each operating system: one library over all three accessibility interfaces, a wrapper crate per system, or bindings of our own over each system's interface? The brief also asks what permission a person must grant, whether a service session can be used, how stable element identifiers are, what a test stand would be, and whether an application built on a browser runtime is a web surface.

## Answer in short

- **Linux: the `atspi` crates over `zbus`.** Safe Rust, no C, no system library at build time, events from the bus. Proven here: against a GTK 3 window and an egui window, the program found the field by role and name, set it, ended the wait on the field's own event in 0 to 5 ms, read it back, invoked Save, ended the wait for the confirmation on an event, and the window's store held exactly one write. Seven runs, seven passes.
- **Windows: Microsoft's `windows` crate, with a thin safe layer of our own.** Every UI Automation method there is an `unsafe fn`; the spike needed 16 `unsafe` blocks for the whole set of steps, events included. The safe wrapper crate `uiautomation` was rejected: it does not compile without its synthetic-input feature, its matcher waits by sleeping in a loop, and it brings a date crate the pack has no use for. Type-checked only.
- **macOS: `objc2-application-services` and `objc2-core-foundation`, with a thin safe layer of our own.** Five crates in all, 16 `unsafe` blocks for the same steps. The older `accessibility` crates stand on deprecated bindings. Type-checked only.
- **One library over three systems, `xa11y`, was rejected.** Its waits poll every 100 ms, it carries synthetic mouse and key input and screen capture in the same crates, it compiles Objective-C on macOS, it is synchronous, and it has had twenty releases in eight months.
- **Three things the run showed that the design must take in.** A field's name is not always its own: GTK 3 gives a labelled field no name and points to its label by a relation, while egui names the field itself. The confirmation of one write came as a different event from each toolkit. And egui publishes nothing until the session's accessibility switch is on, a setting that belongs to the person and outlives the session.
- **The permission is the person's on macOS only.** macOS asks the person to list the program under Accessibility in its privacy settings; nothing can grant it silently. Windows asks nothing of a program at the same integrity level, and refuses an elevated window. Linux asks nothing, but some toolkits stay silent until the session's switch is on.
- **The test stand:** one Rust program in egui for case 28, on all three systems, with a second window of native controls on each system later, so that the pack is not fitted to one provider. CI can run it on Linux under a virtual display; Windows is likely; macOS hosted machines are not expected to grant the permission.
- **An application built on a browser runtime stays a web surface,** as sections 201 and 301 say. The one case neither pack reaches, an application that disables its inspector, is written down below.

## What was run, and what was only read

| | Run here | Type-checked only | Only read |
| --- | --- | --- | --- |
| Linux, `atspi` 0.30.0 over `zbus` 5.19.0 | Built and run against both windows, under `Xvfb` and a private session bus, with `at-spi2-core` 2.52 | | |
| Windows, `windows` 0.62.2 | | `cargo check --target x86_64-pc-windows-msvc` | Its generated bindings, for which methods are `unsafe fn` |
| Windows, `uiautomation` 0.25.1 | | The same target | Its source: the matcher's wait loop, its features |
| macOS, `objc2-application-services` 0.3.2 | | `cargo check --target aarch64-apple-darwin` | Its generated bindings |
| `xa11y` 0.16.0 | | An empty program on all three targets, which builds its whole tree | Its source: waits, input, capture, build script |
| `accessibility` and `accessibility-sys` 0.2.0 | | | Their crates.io metadata and dependencies |
| The egui test window | Run on Linux | Windows and macOS targets | |
| Licences | `cargo-deny check licenses bans sources` with the repository's `deny.toml`, for every spike crate on every target | | |
| Permissions, sessions, hosted CI machines | | | Platform documentation and reports; see the sources. Nothing was tried on Windows or macOS |

## Results

### Linux, the run

Debug build, one D-Bus round trip per property read. From `results/linux-atspi.txt`.

| Step | GTK 3 window (3 runs) | egui window (3 runs) |
| --- | --- | --- |
| Application found on the bus, and its process from the bus daemon | Yes; process number matches the started program | Yes, once the session's accessibility switch is on. With it off, the application never appeared and the wait ended at its deadline |
| Window found | By title and role `Frame` | By role and process only: the frame has no name |
| Locate "Description" by role and name, walking the tree | 11 nodes, 12 to 17 ms. Role `Text`. **Own name empty**; the name comes from the labelled-by relation | 9 nodes, 19 to 28 ms. Role `Entry`, own name "Description" |
| Locate through the Collection interface | No match for a role rule; cause not found | Interface not implemented |
| Set through EditableText, wait for the field's text-changed event | Event in 0 to 1 ms | Event in 3 to 5 ms |
| Read back through Text | "Hinge, steel" | "Hinge, steel" |
| Password field, read | Role `PasswordText`; 16 mask characters, not the value | Not in that window |
| Invoke Save through Action, wait for the confirmation | The status label's **text-changed** event, under 1 ms | The status label's **name** property-change event, 3 to 6 ms |
| The window's store | One write, the value set | One write, the value set |
| A wait no event ends | Ends at its 5 s deadline as a failure | The same |
| Object paths | Numbered in order of creation, so they change with the window | Derived from the toolkit's widget identifiers; the same in each run |
| Automation identifier (`accessible_id`) | Empty | Empty |

Two things went wrong on the way and are part of the result. The `atspi-connection` crate's default `p2p` feature asks every application on the bus for its root when the connection opens, and failed in two of three runs while the egui window was still starting; without that feature, every run passed. And both windows came onto the bus in steps, the application before its window and the window before its controls, so finding a window is itself a wait on the registry's children-changed events.

### The candidates, per operating system

Versions and dates from crates.io on 2026-11-17. "Crates" is the count of distinct crates in the normal tree for that target.

#### Linux

| | `atspi` + `zbus` | `xa11y` | Own bindings over `zbus` |
| --- | --- | --- | --- |
| Version, last release | `atspi` 0.30.0 and its parts 0.14.0, 2026-05-06; `zbus` 5.19.0, maintained, releases in 2026-11 | 0.16.0, 2026-11-13 | |
| Licence tree | Passes; 90 crates with Tokio | Passes; 143 crates | Smaller by the proxies the pack does not use |
| System library or C build | None. D-Bus is spoken in Rust | `xkbcommon` and Wayland client libraries | None |
| Safe at its surface | Yes; the `atspi` crates forbid `unsafe` | Yes | Yes |
| Events | Yes: a stream of typed events, chosen by match rules on the bus | Yes, with locator waits that poll every 100 ms | Yes |
| Against | `atspi-proxies` turns on `zbus`'s own runtime crates beside Tokio's; `p2p` must be off | Polling waits; synthetic input through the X test extension, a virtual input device, and Wayland; screen capture | Writing proxies that already exist |

#### Windows

| | `windows` and a layer of our own | `uiautomation` | `xa11y` |
| --- | --- | --- | --- |
| Version, last release | 0.62.2, 2025-10-06; published by Microsoft | 0.25.1, 2026-09-04; one maintainer | 0.16.0 |
| Licence tree | Passes; 16 crates | Passes; 25 crates | Passes; 51 crates |
| System library or C build | None | None | None |
| Safe at its surface | No: every UI Automation method is `unsafe fn`. 16 blocks for the spike's steps. A `VARIANT` is built by hand: 0.62 has no conversions | Yes | Yes |
| Events | Yes: property-changed, automation, structure, and focus handlers, written as safe trait implementations with the crate's `implement` attribute | Yes, behind its `event` feature | Yes, with polling locator waits |
| Against | The `unsafe` layer is ours to write and test | Needs its `input` feature to compile; its matcher sleeps in a loop; brings `chrono` | As for Linux |

#### macOS

| | `objc2-application-services` and a layer of our own | `accessibility` 0.2.0 | `xa11y` |
| --- | --- | --- | --- |
| Version, last release | 0.3.2, 2025-10-04; part of the `objc2` project | 2025-03-22 | 0.16.0 |
| Licence tree | Passes; 5 crates (Zlib OR Apache-2.0 OR MIT, taken as MIT) | Not checked | Passes; 47 crates |
| System library or C build | None at build; links the system's frameworks | | An Objective-C file compiled by `cc`; links screen capture |
| Safe at its surface | No: calls that write through an out-pointer are `unsafe fn`. 16 blocks. Attribute names are C macros and so are written as strings | Partly | Yes |
| Events | Yes: an AXObserver whose source runs on a run loop, which needs a thread of its own | | Yes, with polling locator waits |
| Against | The layer and its run-loop thread are ours | Stands on the deprecated `cocoa` and `objc` 0.2 crates | As above |

### Which conditions each system can end with an event

Read from each interface's documentation and bindings; only the Linux column was seen.

| Condition | Windows | macOS | Linux |
| --- | --- | --- | --- |
| A window appears or closes | Window opened and closed events | Window created; element destroyed | Window create and destroy; children changed (seen) |
| An element appears or goes | Structure changed | Created; element destroyed; layout changed | Children changed (seen) |
| A value changes | Value property changed; text changed | Value changed | Text changed (seen); value property change |
| A name or label changes | Name property changed | Title changed | Name property change (seen); a label may send text changed instead (seen) |
| Enabled, checked, selected, expanded | A property-changed event for each | Selected children changed; for enabled, none is documented | State changed |
| Focus | Focus changed | Focused element changed | Focus and state changed |

The design's rule holds: where a cell is empty, the condition is refused for that system by lint, never polled. The run adds one rule. **An event wakes the wait; a fresh read decides it.** One confirmation came as text-changed from GTK and as a name change from egui, so a condition is evaluated by reading the element again whenever any event concerning it arrives, and never from the event's own payload.

### Permissions, and how the pack states them

| System | What a person grants | How the pack finds out | What it does |
| --- | --- | --- | --- |
| macOS | The program that makes the calls is listed under Accessibility in the system's privacy settings. A program started from a terminal is judged as the terminal. The record is protected by the system and cannot be written by a script on an ordinary machine | `AXIsProcessTrustedWithOptions` without its prompt option. Reports say it can answer yes while calls still fail, so the probe also makes one real read | `busy`, `runner_unavailable`, naming the setting and the program. The system's own prompt is shown only when a person asks for it through a command, never during a run |
| Windows | Nothing, for windows at the same integrity level. An elevated window cannot be driven from a normal one; crossing that needs a signed program with `uiAccess` installed under Program Files, which the pack does not do | The window's process integrity level, read before the first act | `busy`, `target_elevated`: the person runs the application normally, or the runner at the same level |
| Linux | Nothing. Some toolkits publish their tree only when the session's accessibility switch is on; GTK 3 publishes it regardless. The switch is the person's desktop setting and is kept after the session ends (seen) | The switch's value on the session bus, and whether the application is on the accessibility bus | `busy`, `accessibility_off`, telling the person how to switch it on. The pack never flips it |

### Sessions, identifiers, and other interfaces

- **An interactive session is needed on every system.** UI Automation and the macOS interface reach windows on the desktop of the person's own sign-in; a Windows service runs in a session with no desktop of its own. On Linux the accessibility bus belongs to the person's graphical session. A server has no desktop unless one is made for it, as the spike did with a virtual display.
- **No identifier lasts beyond one reach of a screen.** Object paths changed with GTK and stayed with egui; Windows runtime identifiers are documented as temporary. The interface's automation identifier (Windows AutomationId, macOS AXIdentifier, AT-SPI `accessible_id`) is stable only where the program's author set one; both test windows set none. So `test_id` means that identifier, a locator is otherwise role and name, and a found element is used for one step, as the web pack decided ([decision 0112](../decisions/0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md)).
- **Programs on a Java runtime** reach UI Automation on Windows only through a separate bridge of their own, and AT-SPI on Linux through a toolkit wrapper. Neither was tried. They are a toolkit profile for later, not a back end now.
- **A business client's own scripting interface** is not accessibility. It would be a mode of its own, and is out of scope here.

### The test stand

| Choice | For | Against |
| --- | --- | --- |
| **egui, one Rust program** (chosen for case 28) | One code base and one store on three systems; permissive licences once its bundled fonts are left out; its tree reaches each system through AccessKit; seen working on Linux | AccessKit is one implementation of each system's provider side; it needs the session's switch on Linux; its window has no name; 239 crates |
| A native window per system | Tests the providers real applications use: GTK on Linux (seen), the system's common controls on Windows, AppKit on macOS | Three programs to keep with the same data |
| A cross-platform toolkit with native widgets | One program, native providers | The candidates are C++ libraries or under licences off the closed list |

Case 28 is the egui program; a second window of native controls on each system is added when that system's back end is built, so that the conformance group runs against two providers. CI: Linux under `Xvfb` with `dbus-run-session` and `at-spi2-core`, as here. Windows hosted machines are reported to run desktop UI tests; not verified. macOS hosted machines are not expected to grant the permission, and the record cannot be written there; the macOS back end is proven on a machine where a person grants it, and the gap is written in the journal.

### Applications built on a browser runtime

They stay web surfaces, served by `ui` after the supervisor starts the program, as [section 201](../spec/2-kernel/201-archetypes-and-modes.md) and [section 301](../spec/3-packs/301-the-web-pack.md) say; the design's open point that asked for section 201 to be corrected is stale. One case is left: a packaged application can disable its inspector, and then the web pack fails. Such a runtime builds its accessibility tree only when it sees an assistive client, so the desktop pack could read it. The design keeps refusing it; whether to allow `desktop` there is an open point, not a decision.

## What this means for the design

1. One private back end per system, each over that system's own interface; no library that hides the three. The back-end trait carries the differences the run showed: how a name is computed, which conditions have events.
2. `unsafe` lives in one module per system on Windows and macOS, each function marked as the workspace already does for its system calls, each block explained, and each exercised by a test on its system.
3. Every wait, including finding the application and its window, is ended by an event and decided by a fresh read; nothing polls.
4. A window is matched by the process the supervisor started or a person named, and role; a title alone is a warning, and some windows have none.
5. A password field is known by its role or flag on each system and is never read, whatever its text holds.
6. Two failure codes are added: `accessibility_off` and `target_elevated`, both `busy`.
7. Linux first, then Windows, then macOS ([decision 0120](../decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md)).

## Not verified

- Nothing ran on Windows or macOS. Their programs type-check; whether each call behaves as written, and the event timings there, are unknown.
- Why GTK 3's Collection interface matched nothing for a role rule.
- Whether the AT-SPI cache interface can fetch a whole tree in one call; the walk costs about one to three milliseconds a node in a debug build, which matters for large windows.
- Whether Windows hosted CI machines give a desktop UI Automation can drive, and whether a macOS hosted machine can be given the permission.
- That macOS documents no event for a change of enabled state.
- Whether egui draws text without its bundled fonts.

## Sources

- crates.io: [atspi](https://crates.io/crates/atspi), [zbus](https://crates.io/crates/zbus), [windows](https://crates.io/crates/windows), [uiautomation](https://crates.io/crates/uiautomation), [objc2-application-services](https://crates.io/crates/objc2-application-services), [objc2-core-foundation](https://crates.io/crates/objc2-core-foundation), [accessibility](https://crates.io/crates/accessibility), [xa11y](https://crates.io/crates/xa11y), [eframe](https://crates.io/crates/eframe), [accesskit](https://crates.io/crates/accesskit). Dependency lists and feature tables from the crates.io API on 2026-11-17; sources read from the downloaded crates.
- UI Automation security, integrity levels, and `uiAccess`: [UI Automation security overview](https://learn.microsoft.com/windows/win32/winauto/uiauto-securityoverview); [the policy that limits `uiAccess` programs to secure locations](https://technet.microsoft.com/en-us/library/jj852244.aspx).
- macOS permission from a command-line program: [a platform forum thread](https://developer.apple.com/forums/thread/750905); a trusted answer while calls fail: [a platform forum thread](https://developer.apple.com/forums/thread/794253); the protected permission record on CI machines: [a CI forum thread](https://travis-ci.community/t/xcode-helper-is-not-trusted-for-accessibility/2064).
