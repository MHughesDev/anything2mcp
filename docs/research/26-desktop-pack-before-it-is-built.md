# Research: the desktop pack before it is built

**Status:** completed on 2026-11-17 by reading sources and by checking registries. Nothing was compiled or run for this note. It extends [note 19](19-desktop-accessibility-base.md) and [decision 0120](../decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md), which ran a spike on Linux and type-checked Windows and macOS. It answers what those left open: whether each test stand can run on GitHub's hosted machines, how the ten rules apply to a window on a person's screen, whether a read can be made safe from writing, and what the first slice and the scope question look like. It feeds [spec section 305](../spec/3-packs/305-the-desktop-pack.md), [section 106](../spec/1-product/106-what-it-latches-onto.md), and the design of [`a2m-pack-desktop`](../architecture/crates/a2m-pack-desktop.md). It follows up brief 19 of [open research](11-open-research.md); the RPA brief (21) is not touched.

## Question

Before package 8.3 starts: (1) what does each operating system's accessibility interface need, read, do, signal, and hide; (2) which crates, or which bindings already in the workspace, reach each; (3) what test stand can the project build, and can CI run it without a person; (4) how do rules 3, 4, 5, and 7 apply to a desktop application, and what is a look-only run here; (5) what is the thinnest true slice, and should the pack be in the first release for one machine.

## Answer in short

- **Nothing in note 19 was overturned.** One fact changes the Linux route: the `atspi` crates (latest 0.30.0, 2026-05-06) still require `zbus` 5, and the workspace is on `zbus` 6.3.0. Taking the crates in puts two major versions of `zbus` in the tree, and brings back the three crates that [decision 0123](../decisions/0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md) says were declared unmaintained. Writing the few AT-SPI proxies by hand over the workspace's own `zbus` 6 needs no new crate. Recommended, at a cost named below.
- **Windows and macOS need almost no new crate.** The `windows` 0.62.2 crate is in the workspace with only `std`; UI Automation is more features of the same crate. macOS needs one new crate, `objc2-application-services` 0.3.2, from the same project as the `objc2` crates already used. All licences are on the closed list.
- **CI.** Linux works on a hosted machine with a virtual display, a session bus, and the AT-SPI daemons; a published project runs egui, GTK, Qt, and Electron fixtures that way. Windows is reported to work with no setup. macOS is reported to work only by writing a row into the system's permission database with `sudo` on the hosted machine; a 2021 statement by the image maintainers said it could not be done, a 2026 project does it. That is a CI-only act on a throwaway machine, never the product's. It needs a record by the lead (see "Rule 5").
- **Look-only:** a desktop has no equivalent of the web pack's tab that cannot write. The operating system's permission is all or nothing for the program. What can be done is a back-end handle whose type has no set or invoke method. What cannot be done is to stop the application writing by itself.
- **Recommendation:** Linux first, as note 19 and decision 0120 already say, in a first slice of reads only against the egui stand. Size: the whole pack is large; the first slice is medium.

## What was read, and what could not be reached

| Source | Reached | How |
| --- | --- | --- |
| Crate metadata and dependency lists | Yes | crates.io API, 2026-11-17 |
| UI Automation documentation | Yes, through its source repository | The Microsoft documentation source on GitHub; `learn.microsoft.com` itself was not reachable from this session |
| macOS accessibility documentation | Three pages | Apple's documentation data files for `AXIsProcessTrustedWithOptions`, `AXUIElementPerformAction`, `kAXSecureTextFieldSubrole`, and two notification names. The rest is from memory, marked below |
| AT-SPI documentation on `gnome.pages.gitlab.gnome.org` | No | Host not reachable. Linux facts rest on note 19's run and on the `atspi` and `xa11y` repositories |
| GitHub's own page on hosted runners (`docs.github.com`) | No | Host not reachable. Runner facts rest on the `actions/runner-images` repository's readme files and issues, and on other projects' workflows |
| Other projects' CI | Yes | `xa11y` workflow file and scripts, read on 2026-11-17. What they did was read; their results were not seen |

## 1. The accessibility interface of each system

Everything below is for a program that reads and acts by role and name, never by position and never by image.

| | Windows UI Automation | macOS Accessibility API | Linux AT-SPI |
| --- | --- | --- | --- |
| What the program must be granted | Nothing at the same integrity level. A normal program cannot reach an elevated window. `uiAccess` lets a signed program installed in a protected folder reach more, and an elevated window only if launched by an administrator. Nothing reaches the logon or consent screen ([security overview](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-securityoverview.md)) | The person lists the program under Accessibility in the privacy settings. `AXIsProcessTrustedWithOptions` says whether the calling process is trusted ([Apple](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions)). A program started from a terminal is judged as the terminal, from memory and note 19's forum sources | Nothing. The session needs a bus with the accessibility registry. Some toolkits publish nothing until the session's accessibility switch is on (egui through AccessKit: seen in note 19). Reported by `xa11y`'s action: egui through AccessKit publishes its tree only once a window manager has mapped the window. Note 19's run, under a bare virtual display and no window manager, passed seven times, so this is a conflict between two sources |
| Session | The person's interactive desktop. A service session has no desktop (note 19, from documentation). A locked or disconnected session: from memory, unverified | The person's graphical login | The person's graphical session and its bus |
| What it reads | Name, ControlType, AutomationId, IsEnabled, `IsPassword`, RuntimeId, and the tree ([property list](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-automation-element-propids.md)) | Role, subrole, title, description, value, enabled, children, through attributes. Secure text fields carry the subrole `AXSecureTextField` ([Apple](https://developer.apple.com/documentation/applicationservices/kaxsecuretextfieldsubrole)) | Role, name, description, states, relations (labelled-by), attributes, the tree. A password field has its own role (seen) |
| What it does | Patterns: Invoke, Value, Selection, Toggle, ExpandCollapse, Text ([patterns](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-controlpatternsoverview.md)). Writing a value needs the control enabled and not read-only ([Value pattern](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-implementingvalue.md)) | Set an attribute; perform an action ([set](https://developer.apple.com/documentation/applicationservices/1460434-axuielementsetattributevalue), [perform](https://developer.apple.com/documentation/applicationservices/1462091-axuielementperformaction)) | Action, EditableText, Value, Selection interfaces (seen: EditableText and Action) |
| A trap in doing | Invoke is meant to return at once, but a control that opens a modal dialog keeps the calling client blocked until the dialog closes ([Invoke guidance](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-implementinginvoke.md)) | An action can return a "cannot complete" error while the application is still handling it, and that does not mean it failed (Apple's page on perform action) | None found |
| How a change is observed | Five event kinds: property changed, element action, structure changed, desktop-wide (focus, window), notification ([events](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-eventsoverview.md)). Some fire with no real change, so a fresh read decides. Handlers run on a thread that is not the UI thread; use a multithreaded apartment; add and remove on the same thread ([threading](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-threading.md)) | An observer added per process and notification, delivered through a run loop (note 19). Value changed and created are documented ([value](https://developer.apple.com/documentation/applicationservices/kaxvaluechangednotification), [created](https://developer.apple.com/documentation/applicationservices/kaxcreatednotification)); the rest is from note 19 | Bus events: text changed, name changed, state changed, children changed (seen). One confirmation came as a different event from each toolkit |
| What it cannot see | Elevated windows; the secure desktop; custom-drawn controls with no provider ([providers](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-providersoverview.md): a custom control needs its own provider); other sessions | Applications that do not publish a tree; custom-drawn views; other users' sessions; some Chromium-based applications build a tree only when asked (from memory, unverified) | Toolkits with the switch off; custom-drawn views; other sessions' buses |
| Speed | The documentation has a caching mechanism for properties and patterns ([caching](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-cachingforclients.md)); not tried | Not measured | One to three milliseconds a node in a debug build (note 19) |

Two rules follow that note 19 did not state. **A back-end call that can block on the application must run where a block cannot stall the engine:** on Windows an Invoke on a Save that shows a dialog does not return until the dialog is closed, so the call runs on a thread the back end owns and the step is already counted as sent. **A "cannot complete" or timeout from an act is never a failure that nothing was sent:** it is `possible_commit` for a committing action, as the spec already says for a lost confirmation.

## 2. Crates, and the bindings the workspace already has

Dates and licences are from crates.io on 2026-11-17, the licence string exactly as the registry gives it. Every one passes the closed list in `deny.toml` (MIT, Apache-2.0, BSD-3-Clause, Unicode-3.0, Zlib, ISC, CDLA-Permissive-2.0), which for an `OR` expression needs one allowed choice. None needs a nightly toolchain: the highest minimum Rust version below is 1.95, and the workspace pins 1.100.0.

| Crate | Repository | Licence | Latest, date | What it wraps | Notes |
| --- | --- | --- | --- | --- | --- |
| `atspi`, `atspi-proxies`, `atspi-connection`, `atspi-common` | [odilia-app/atspi](https://github.com/odilia-app/atspi) | Apache-2.0 OR MIT | 0.30.0 and 0.14.0, 2026-05-06; five releases in 14 months; 8.5 million recent downloads | AT-SPI over D-Bus, through `zbus`, in Rust; `deny(unsafe_code)` is a stated goal | `atspi-proxies` and `-connection` require `zbus` ^5.5. An open pull request in July 2026 on its lockstep crate and an open issue on its minimum Rust version show the project is working on dependencies; neither was read to a conclusion |
| `zbus` | [z-galaxy/zbus](https://github.com/z-galaxy/zbus/) | MIT | 6.3.0, 2026-11-13 | D-Bus in Rust | In the workspace already, at 6.3.0, for the presence prompt |
| `windows` | [microsoft/windows-rs](https://github.com/microsoft/windows-rs) | MIT OR Apache-2.0 | 0.62.2, 2025-10-06 | Windows APIs, generated bindings | In the workspace and lock at 0.62.2 with `std` only. Note 19: every UI Automation method is `unsafe fn` |
| `uiautomation` | [leexgone/uiautomation-rs](https://github.com/leexgone/uiautomation-rs) | Apache-2.0 | 0.25.1, 2026-09-04; one maintainer; 0.6 million recent downloads | UI Automation, through `windows` | Rejected in note 19 (needs its input feature to build, its matcher sleeps in a loop, brings a date crate) |
| `objc2-application-services` | [madsmtm/objc2](https://github.com/madsmtm/objc2) | Zlib OR Apache-2.0 OR MIT | 0.3.2, 2025-10-04 | The `AXUIElement` and `AXObserver` functions | Not in the lock. Same project and release date as the `objc2` crates already used |
| `objc2-core-foundation` | same | Zlib OR Apache-2.0 OR MIT | 0.3.2, 2025-10-04 | Core Foundation types | Already in the lock at 0.3.2 |
| `accessibility` | [eiz/accessibility](https://github.com/eiz/accessibility) | MIT / Apache-2.0 | 0.2.0, 2025-03-22 | macOS accessibility over deprecated bindings | The licence string uses a slash, which `cargo-deny` may read only in a lax mode: not tried. Rejected in note 19 |
| `xa11y` | [xa11y/xa11y](https://github.com/xa11y/xa11y) | MIT | 0.16.0, 2026-11-13; 12.6 thousand recent downloads | All three | Rejected in note 19 for polling waits, synthetic input, and churn |

**Recommended route per system, and what the alternative costs.**

| System | Route | Why | The alternative, and its cost |
| --- | --- | --- | --- |
| Linux | Proxies for the few AT-SPI interfaces the pack uses, written by hand over the workspace's `zbus` 6.3.0 with Tokio; no new crate | One `zbus` in the tree; no return of the three crates of decision 0123; no `p2p` hazard that note 19 hit. The interfaces used are: the registry, Accessible, Application, Action, EditableText, Text, Value, Selection, and the event match rules | The `atspi` crates as decision 0120 chose: less code, but a second `zbus` major, a failing advisory check unless the three crates have since been fixed (not rechecked), and a cost to remove later. The record's own note says that if the advisory check fails, Windows goes first. This note says the proxies are a third way, not a reason for Windows first |
| Windows | `windows` 0.62.2 with UI Automation features and our `unsafe` layer, as decision 0120 | No new crate; the workspace already calls Windows this way in `a2m-local` | `uiautomation`: no `unsafe` of ours, but the three faults of note 19 |
| macOS | `objc2-application-services` and `objc2-core-foundation`, a layer of our own, observer on a thread of our own | One new crate from a project already trusted for the workspace's presence prompt | `accessibility`: deprecated bindings beneath it |

The workspace already marks system calls with `#[allow(unsafe_code)]` and a comment on each (`a2m-local`'s Windows and macOS presence code), so the rule of decision 0120 point 5 is the same practice.

## 3. The test stand, and whether CI can run it

### What the stand must have

Case 28 of the [test application plan](../plan/test-application.md) is "a small native desktop program with the same data". The spike's stand is a form with one field, a password field, a check box, Save, and a status label. For the pack's invariants the stand needs more, and each item is small:

| Part | Why |
| --- | --- |
| A form: text, number, check box, a select, Save | Set, read back, commit |
| A table of the line items | `list` and `read` of rows |
| A confirmation dialog before saving | `unexpected_dialog`, and the Windows Invoke that blocks |
| A control that is slow: its value changes a moment after it is set | A wait ended by an event; a wait no event ends fails at its deadline |
| A control with no accessible name | `desktop_unnamed`; and a name found through a label |
| A password field | The refusal to read |
| A second window of the same program with the same title | "No act reaches another window" |
| A store file written on Save, and a count of writes | The test asks the stand what it stored (AGENTS.md) |
| A switch that kills the process after Save is invoked | `possible_commit` |

### Toolkits, by licence and by what each exposes

| Toolkit | Licence in its manifest | Exposes | Verdict |
| --- | --- | --- | --- |
| `eframe` 0.37.0 (egui) with `accesskit`, without default fonts | MIT OR Apache-2.0 | Through AccessKit: UIA on Windows, NSAccessibility on macOS, AT-SPI on Linux ([AccessKit](https://github.com/AccessKit/accesskit); crates `accesskit` 0.25.2, `accesskit_unix` 0.24.1, `accesskit_windows` 0.35.2, `accesskit_macos` 0.27.2, all MIT OR Apache-2.0, released 2026-10-25). The project calls the adapters "at rough feature parity", without rich text | The chosen one; seen on Linux. One program for three systems |
| A Win32 window made with the `windows` crate | MIT OR Apache-2.0 | The system's own controls, which have UI Automation providers built in | Best second stand on Windows: no new crate, no OpenGL |
| An AppKit window made with `objc2-app-kit` 0.3.2 | Zlib OR Apache-2.0 OR MIT | The system's own controls | Best second stand on macOS: no new crate beyond what the pack needs |
| GTK 3 through Python (the spike's) or `gtk4` 0.11.5 | `gtk4`: MIT. The GTK library itself is LGPL and is a system package, not a crate | Native AT-SPI, seen with GTK 3 | A second stand on Linux; Python is not allowed on a product path but a stand is not on one, and rule 6 needs a record for a new use: avoid, use `gtk4` if a second Linux stand is wanted |
| Slint 1.19.0 | GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0 | | Not on the closed list |
| iced 0.14.0 | MIT | A search result titled an open issue about accessibility; not read | Not recommended without reading it |

### Can CI run it, per system

| System | Known to work | Reported to work | Unknown |
| --- | --- | --- | --- |
| **Linux** (`ubuntu-latest`) | Under a virtual display, a private session bus, and `at-spi2-core`: seven runs of two windows in note 19, on a machine of ours, not on a hosted one | The `xa11y` project's workflow runs AccessKit, egui, GTK, Qt, Tauri, and Electron fixtures on `ubuntu-latest`, with a virtual display at 1280x1024x24, `dbus-run-session`, the bus launcher, the registry daemon, and a window manager (fluxbox), and sets the registry's status flags by `dbus-send`. Their action page and script were read; results were not. The Ubuntu 24.04 image's readme (version 20261111.370.1) lists Xvfb and D-Bus and does not list `at-spi2-core` or fluxbox, so the job installs them | Whether the window manager is needed (the two sources conflict, above); the software OpenGL libraries the egui window needs on the hosted image; the cost in minutes |
| **Windows** (`windows-latest`) | | The `xa11y` workflow runs egui, WinForms, WPF, UWP, Qt, and Tauri fixtures on `windows-latest`. Their setup action says it does nothing on Windows and UI Automation works as it is. A contributor on an `actions/runner-images` issue (14049, closed 2026-08-07) states the Windows 11 ARM image has an interactive session on the window station `WinSta0`, and reports only that the process cannot take the system foreground (its form's `ActiveForm` is null); the reporter says the same test passes on `windows-latest`. The Windows Server 2025 image readme (version 20261111.331.1) lists WinAppDriver 1.2.2009.02003 | Whether the egui window opens: egui's default OpenGL back end needs more than the plain Windows software OpenGL, and the hosted machine's graphics are not documented in the pages read. A stand built with Win32 needs none of that. Whether an elevated-window test (for `target_elevated`) can run: the hosted user is an administrator, from memory, unverified |
| **macOS** (`macos-latest`) | | The `xa11y` workflow grants Accessibility by `sudo sqlite3` into `/Library/Application Support/com.apple.TCC/TCC.db`, for one client path, with retries on "database is locked", a read-back that the value is "allowed", and a restart of the permission daemon. The same project runs its egui and Cocoa fixtures on `macos-latest`. It does **not** run its Rust accessibility tests in CI, because grants to Cargo-built binary paths "were fragile" | A 2021 answer on `actions/runner-images` (issue 3286) by a maintainer says it cannot be enabled at run time because the hosted runners have SIP on. A 2023 answer (issue 8214) shows a similar database write worked on `macos-13` while it was in beta and failed on `macos-latest` then. Both contradict the 2026 workflow, and nothing here shows which is true of the image today. Whether a granted path survives a rebuild of our binary; whether a window appears at all on the macOS virtual machine |

**What a person must do:** nothing on Linux and (reported) Windows. On macOS the hosted route is a script that writes the permission row. That act is acceptable only on a machine the project throws away after the run, and it must never be a thing the product does or tells a person to do. On a real Mac only the person's click in the settings counts, and no agent can do it.

## 4. The rules applied to a desktop application

### Rule 5, operate openly

The web pack's engine owns its browser's data directory and never touches the person's. The desktop has no such boundary: the person's applications and windows share one session and one accessibility tree. What the pack should require:

| Require | Why, and how |
| --- | --- |
| **A named application.** The target file declares the program by absolute path and digest; the pack starts it through the supervisor, as the design says | The pack drives only what the plan declared. Attaching to a running process only when a person named it, as the spec says |
| **A window the person can see.** The started program's window is shown in the person's session. The pack does not start it on a second hidden desktop (a Windows desktop object made for the purpose), a virtual display, or minimized | A hidden desktop would be stealth. A virtual display is for CI and servers, and is stated as such on the target |
| **A refusal to touch any window of another application.** The back end hands the driver a reference to one proven window of one proven process, never the root of the desktop. Every call carries that reference and checks the element's process before acting | This is the design's invariant. It matters more here than on the web because the accessibility root holds every application |
| **No synthetic input in the first slice.** The spec allows a key or click "only after" the patterns fail. A key goes to whichever window has focus when it arrives, so it cannot be proven to land in the expected window. Refuse it until a test shows it can be bound to a window | Rules 5 and 7. A control with no value pattern then fails as `locator_not_found` or `unknown_variant`, not as typed input |
| **A person using the session stops the run.** `busy`, `session_in_use`, as the spec says; how it is detected on each system is not researched | |
| **A single-instance application is a risk.** A program that on launch hands its work to an already running copy and exits cannot be proven to be the process the supervisor started. The pack fails it as not provable. Nothing was run to see how common this is | |

### Rule 3, every write is verified

| | Desktop meaning |
| --- | --- |
| Intent | Recorded in the ledger before the first `set`, as everywhere |
| Commit point | The `invoke` of the action the screen declares with `commits: true` (the Save button), as for `ui` ([decision 0112](../decisions/0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md)). Typing into a field is not a commit |
| Applications with no Save | Many save on leaving a field or on Enter. Without synthetic input there is no commit action to invoke, so the write is refused or declared `irreversible`. Stated as a limit |
| Confirmation | A declared element, window, or value, ended by an event and decided by a fresh read (decision 0120). A modal confirmation dialog is a declared variant |
| Read-back | The field read again after the screen is reached afresh: the person's flow leaves the record and opens it again. Reading the widget just set proves nothing, since it holds the typed text before any save. As with the web ([decision 0144](../decisions/0144-the-web-drivers-and-discoverers-after-their-safety-audit.md), U4), an application that keeps edits in memory could answer from its copy. Where the stand holds the store, the test asks the store |
| A send whose answer did not come | Windows Invoke can block on a dialog, macOS can return "cannot complete" while it works: both are sent, so `possible_commit` |

### Rule 7, unknown means stop

A role-and-name locator must match exactly one element in the proven window. Zero is `locator_not_found`; two is `locator_ambiguous`. Three further cases to name in the design, none run:

- The name depends on the toolkit (note 19) and on the application's language. The target file should pin the language, and a changed name is drift.
- A list or table that draws only the rows on screen cannot prove a record is absent. A lookup that finds nothing in an unrealized list is `unknown_variant`, not "absent".
- A tree not yet built. Finding the window and its controls is itself a wait on events (note 19); a deadline ends it as `timeout` before anything is sent.

### Rule 4, secrets never reach a caller

| Place | Rule |
| --- | --- |
| Password fields | Known by `IsPassword` on Windows (the property exists in the documentation's list; what a read of its value returns was not read), by the subrole `AXSecureTextField` on macOS, and by the password role on Linux (seen). The back end refuses to read the value, to put it in a tree, and to put it in an event payload. Setting one is allowed only from a secret reference, and the value is registered with the redactor before it is sent |
| The clipboard | The pack never reads or writes it, and never sets a value by paste |
| Window titles and names | A document name can hold personal text. They are redacted whole, then cut, as decision 0143 point 14 did for the web, in findings and errors |
| A tree snapshot | Values of fields that are secret by role are omitted; the rest are evidence, not instructions |
| An application that shows a secret in a plain label | Cannot be known. The redactor catches only registered values |

### What a look-only run means

The web pack's observing tab holds every request of the page that could change the system, so a page that writes on load is stopped by the browser. **No such lock exists for a desktop application.**

| Question | Answer |
| --- | --- |
| Does the operating system offer a read-only grant? | Not found. Windows asks nothing; macOS grants the whole interface; Linux has none |
| What the pack can do | A look-only posture whose back end handle is a different type with find, read, tree, and watch and **no** set, invoke, or expand. The engine then cannot write through it by a mistake, and discovery ([section 305](../spec/3-packs/305-the-desktop-pack.md): "Both read the tree and never invoke, expand, or focus anything") uses only that type |
| What it cannot stop | The application writing by itself: autosave, a write when a window opens or loses focus, a write when a tree is requested. Case 28's store counts writes during a snapshot, which proves it for the stand only |
| Reads that are acts | On Windows expanding a node is a pattern call and must not be used by a read. On macOS, asking a Chromium-based application to build its tree is done by setting an attribute on it (from memory, unverified), which is a write to the application, so a look-only run does not do it |
| A desktop read in a writing session | Necessarily. There is one session and the program is in it. The protection is the type of the handle, the lint on steps, and the stand's write count, not a lock |
| Stronger, not researched | Running the program under an operating-system file sandbox with no write access, so that its own writes fail |

## 5. Recommendation

**The first slice: Linux, reads only, against the egui stand.** It is the system whose stand can run in CI with no person, and the one where the steps are already proven. The slice is steps 2 to 4 of the design's build order: the stand with the parts above, a Linux CI job, the pack's compiler side, and the Linux back end for `read`, `list`, `desktop_snapshot`, and `desktop_probe`.

| Part | Size | What it is |
| --- | --- | --- |
| The stand `targets/example-desktop` with the table, dialog, slow control, nameless control, second window, kill switch, store | Small to medium | One Rust program of a few hundred lines on a crate already checked; the second-window and kill pieces are new |
| The Linux CI job | Small | Install the display, bus, daemons, and software OpenGL; start; run. Whether a window manager is needed is the one experiment |
| The pack's compiler side with no system code | Medium | As the web pack's: manifest, surface kind, `screens/`, lowerings, four lints, a digest test, on three systems |
| A record on the supervisor's missing link kind for a program that is watched until stopped (the design's first open point) | Small | A change to `a2m-provider` by decision |
| The Linux back end, hand-written proxies, reads | Medium | The proxies, the event waits, the process proof, the password refusal |
| Linux writes with read-back and the fault cases | Medium | Set, invoke, confirm, kill after invoke, second window |
| The Windows back end with a Win32 stand beside the egui one | Medium to large | The `unsafe` layer, a multithreaded-apartment thread, the blocking Invoke; CI on the hosted machine |
| The macOS back end | Medium to large | The layer, the observer thread, the permission probe; CI only if the permission script holds |

The whole pack is **large**: about three times the Linux part. **The first slice is medium.**

**What cannot be proven without a person at a machine:** the macOS permission as a person meets it (the prompt, the settings, the "yes but fails" state); `session_in_use` against a real person typing; a locked or disconnected session on Windows; a real toolkit's quality of tree (Qt, WinForms, WPF, Electron, Java), since the stands exercise only AccessKit and one native toolkit each; Wayland sessions; a real application's own sign-in and its dialogs. The owner's go-ahead is needed for any real application (rule 10).

### In or after the first release, five sentences each

**For including it.** [Section 106](../spec/1-product/106-what-it-latches-onto.md) names desktop applications as one of the kinds of system the product latches onto, and the first release is for one machine, which is where a desktop application runs. It is the one pack whose natural home is the person's own session, so it needs no server mode. Linux is provable in CI today, so a Linux-only first release could carry it with a stated platform list, as the manifest already allows. Leaving it out leaves one of the product's named kinds with nothing behind it. Whatever the kernel does wrong for a pack that is not a browser is found sooner.

**For following it.** It is the least designed pack, and the roadmap gates it on package D4, the command pack's count of kernel changes. Two of its three systems cannot be proven by a machine: macOS needs a person at the settings, and Windows is reported, not seen. The first release's bar is that nothing unproven ships, and a pack at "Linux reads only" is a weaker claim than the web pack's. It is one more surface of effects (screen focus, process start) whose consent and `busy` behaviour are not yet in the approval flow. The packaging and the first release have more to finish, and a desktop pack that is late costs less than one that is wrong on a person's own machine.

(Neither case is decided here. The owner has not spoken on it.)

### Changes this note proposes, for the lead

None is made. Each needs a record by an agent with the lead's go-ahead, or is noted for the design.

1. Linux back end on hand-written proxies over `zbus` 6, not the `atspi` crates (supersedes point 1 and the build-order caveat of decision 0120).
2. No synthetic key or click in the first slice; the sentence "a synthetic key or click only after those" in spec section 305 and in the design is tightened.
3. A look-only posture as a type with no set or invoke, and the statement that the application's own writes cannot be stopped.
4. The macOS CI permission script: a record that says it is for a throwaway CI machine only and is never a product act, so that it is not read as a way past a device's protection (rule 5).
5. The stand's parts, added to test-application case 28.

## Not verified

- Nothing was run. All crate dates, licences, and dependencies are from the registry; none was built.
- The `atspi` crates' plan for `zbus` 6 was not read to a conclusion (an open pull request and an open issue, both July 2026, were seen only as titles). The advisory status of the three `zbus` 5 crates was not rechecked; the statement comes from decision 0123.
- Whether any hosted runner image today lets `sudo sqlite3` change the macOS permission database. Two older reports say no or "only on a beta image"; one 2026 workflow says yes.
- Whether the hosted Windows machine can open an egui (OpenGL) window; whether a Linux window manager is needed.
- Everything about the Windows `IsPassword` read result, a locked or disconnected Windows session, the hosted Windows user being an administrator, and how Chromium-based applications are asked to build a tree. All from memory.
- Apple's and Microsoft's pages were read for only the facts cited; the macOS event list for each condition is note 19's, not re-read.
- The AT-SPI documentation host and `docs.github.com` were not reachable from this session.
- Whether `cargo-deny` reads the `accessibility` crate's slash-separated licence string.

## Sources

- crates.io: [atspi](https://crates.io/crates/atspi), [atspi-proxies](https://crates.io/crates/atspi-proxies), [atspi-connection](https://crates.io/crates/atspi-connection), [atspi-common](https://crates.io/crates/atspi-common), [zbus](https://crates.io/crates/zbus), [zvariant](https://crates.io/crates/zvariant), [windows](https://crates.io/crates/windows), [uiautomation](https://crates.io/crates/uiautomation), [objc2-application-services](https://crates.io/crates/objc2-application-services), [objc2-core-foundation](https://crates.io/crates/objc2-core-foundation), [objc2-app-kit](https://crates.io/crates/objc2-app-kit), [accessibility](https://crates.io/crates/accessibility), [xa11y](https://crates.io/crates/xa11y), [eframe](https://crates.io/crates/eframe), [accesskit](https://crates.io/crates/accesskit), [accesskit_unix](https://crates.io/crates/accesskit_unix), [accesskit_windows](https://crates.io/crates/accesskit_windows), [accesskit_macos](https://crates.io/crates/accesskit_macos), [gtk4](https://crates.io/crates/gtk4), [slint](https://crates.io/crates/slint), [iced](https://crates.io/crates/iced). Read from the registry's API on 2026-11-17.
- Repositories: [odilia-app/atspi](https://github.com/odilia-app/atspi) (readme, and its issue list), [AccessKit](https://github.com/AccessKit/accesskit) (readme), [xa11y](https://github.com/xa11y/xa11y) (its [CI workflow](https://github.com/xa11y/xa11y/blob/main/.github/workflows/ci.yml), [macOS grant script](https://raw.githubusercontent.com/xa11y/xa11y/main/scripts/grant_macos_tcc.sh), and [Linux test script](https://raw.githubusercontent.com/xa11y/xa11y/main/scripts/run_integ_tests.sh)), [xa11y/setup-a11y](https://github.com/xa11y/setup-a11y) and its [action file](https://raw.githubusercontent.com/xa11y/setup-a11y/main/action.yml). These are the work of a project that benefits from its own claims; what they ran was read, not repeated.
- GitHub's runner images: [Ubuntu 24.04 readme](https://raw.githubusercontent.com/actions/runner-images/main/images/ubuntu/Ubuntu2404-Readme.md), [Windows 2025 readme](https://raw.githubusercontent.com/actions/runner-images/main/images/windows/Windows2025-Readme.md), and the issues [3286](https://github.com/actions/runner-images/issues/3286) (2021, accessibility permission, SIP), [8214](https://github.com/actions/runner-images/issues/8214) (2023, a database write on `macos-13`), and [14049](https://github.com/actions/runner-images/issues/14049) (2026, interactive session on Windows 11 ARM).
- UI Automation, from the documentation's source repository: [security overview](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-securityoverview.md), [events overview](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-eventsoverview.md), [control patterns](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-controlpatternsoverview.md), [Invoke](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-implementinginvoke.md), [Value](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-implementingvalue.md), [threading](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-threading.md), [property identifiers](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-automation-element-propids.md), [providers](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-providersoverview.md), [caching](https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/WinAuto/uiauto-cachingforclients.md).
- macOS: [AXIsProcessTrustedWithOptions](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions), [AXUIElementSetAttributeValue](https://developer.apple.com/documentation/applicationservices/1460434-axuielementsetattributevalue), [AXUIElementPerformAction](https://developer.apple.com/documentation/applicationservices/1462091-axuielementperformaction), [kAXSecureTextFieldSubrole](https://developer.apple.com/documentation/applicationservices/kaxsecuretextfieldsubrole), [kAXValueChangedNotification](https://developer.apple.com/documentation/applicationservices/kaxvaluechangednotification), [kAXCreatedNotification](https://developer.apple.com/documentation/applicationservices/kaxcreatednotification), [AXObserver](https://developer.apple.com/documentation/applicationservices/axobserver).
