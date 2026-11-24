# Research: what a program can reach in another, and what each route asks of the person

**Status:** partial, written on 2026-11-23 by reading sources and registries. Nothing was installed, built, or run, and no consent prompt was pressed. Several hosts did not resolve from this session: `learn.microsoft.com`, `docs.rs`, `dbus.freedesktop.org`, `scriptingosx.com`, and a googlesource mirror of the D-Bus specification. Apple's pages came back in part. Every claim carries a tag:

- **[read]**: the page or file was opened in this session. Where the fetch tool summarised the page, the note says "as summarised".
- **[search]**: taken from a web-search summary; the page itself was not opened. Treat as a lead.
- **[memory]**: stated from knowledge of the system, not opened here. Treat as unverified.
- **[note 19]** or **[note 26]**: taken from those notes, which ran or read their own sources.

It answers part (c) of brief 35 of [open research](11-open-research.md) in full, and the accessibility and consent parts for package 8.3's Windows and macOS back ends. Parts (a), (b), (d), and (e) of the brief are not touched; see "What was not done". It builds on [note 19](19-desktop-accessibility-base.md) and [note 26](26-desktop-pack-before-it-is-built.md) and does not repeat them. It feeds [spec section 305](../spec/3-packs/305-the-desktop-pack.md), [decision 0276](../decisions/0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md), [decision 0214](../decisions/0214-ways-through-the-refusals-that-keep-a-person-from-their-own-machine.md) (row 6), and hypothesis H-41. This note records facts and candidate rules; the design is another agent's.

## Question

From outside another program, on Windows, macOS, and the Linux desktop, what can a program running as an ordinary user reach: through the accessibility interface, and through the other program's own scripting interface? What does each route describe about whether a call only reads? What does each ask of the person: a consent prompt, a permission in settings, an entitlement, or nothing? Rule 5 binds: a block is a named stop, never something to get past.

## Answer in short

- **The accessibility routes need no new design for the two back ends.** Note 19's table stands. This note adds three facts. Windows: the interface itself asks for no consent, and the one boundary is the integrity level; the single documented way across it needs a signed program in a protected folder, which the pack does not use [read, as summarised]. macOS: the only gate is the Accessibility listing, and the gate is the program that makes the calls, so a program started from another is judged as that other [note 26]. Linux: nothing is asked.
- **Each system has a scripting route, and each describes the other program in a different way.** Windows: COM automation, described by a type library. macOS: Apple events, described by a scripting dictionary (sdef). Linux: D-Bus, described by introspection XML. All three describe names and types. None describes, for every call, whether it only reads.
- **What each describes about reading is weaker than decision 0276 assumes.** A COM type library marks a member as a property get, a property put, or a plain function; a plain function has no read-only mark, so a method named like a getter can still change state [read]. An sdef marks a property `r` or `rw`, but Apple's own guide says Cocoa does not enforce it [read]. D-Bus introspection marks a property `read`, `write`, or `readwrite` and says nothing about a method [search, memory]. So the finding that says "this call only reads" can honestly be at most "the publication says so", never "verified".
- **Only macOS asks the person at the scripting route, and it asks for each pair.** The first Apple event from one program to another program raises a system prompt that names both. The answer is kept per pair, in a store the person manages in System Settings. A refusal is kept too, and later events fail with error -1743 without a prompt [search]. Windows asks nothing at the same integrity level. Linux asks nothing on the session bus; the person's session is the boundary [memory].
- **A scripting route can start the other program, and that must not happen.** An Apple event sent by name launches an application that is not running [memory]. COM can start a server by class identifier [memory]. Reaching only what is already running is a choice the pack must make: attach through a running-object lookup or by process number; never create. D-Bus asks the bus to start a service on a first call [memory]; decision 0276 already says introspection is asked so that a service is never started.
- **Crates exist for each route, and one is stable.** `objc2-foundation` (NSAppleScript), `objc2-osa-kit`, and `objc2-scripting-bridge` are from the project the workspace already uses, all 0.3.2, licence on the closed list for the last one checked. The `windows` crate carries COM and type-information interfaces. `zbus` and `zbus_xml` read D-Bus and its XML. The two crates named `scriptingbridge-rs` and `osakit-rs` are new, with under 200 downloads each; they are not candidates (see "Crates").
- **Microsoft is building a consent route of its own for agents** on Windows 11: a registry of tools that apps offer, and a separate agent session and account. It is in preview in the sources found, and it changes what "reach an application on Windows" may mean later. It is not a route this pack can use today (see "A route Microsoft is adding").

## What was read, and what could not be reached

| Source | Reached | How |
| --- | --- | --- |
| UI Automation, assistive-technology page and the Value pattern's `SetValue` | Yes, as summarised | Microsoft's documentation source in `MicrosoftDocs/win32` and `MicrosoftDocs/sdk-api` on GitHub. `learn.microsoft.com` did not resolve |
| COM: `GetActiveObject`, `INVOKEKIND`, `FUNCFLAGS` | Yes, full text | `MicrosoftDocs/sdk-api` |
| Apple's scripting definition guide | Yes, as summarised | `developer.apple.com` archive page |
| Apple's pages on the Apple events entitlement and `AEDeterminePermissionToAutomateTarget` | No | The first returned only its title; the second was not found. The entitlement and prompt text below are [memory] and [search] |
| D-Bus specification | No | Not resolved. Access values and annotations come from implementations' source named in search results [search] |
| Flatpak and the portal's remote-desktop interface | Search only | Flatpak documentation and a mailing-list thread appeared in results; neither was opened |
| Microsoft's pages on the agent workspace and on MCP on Windows | Search only | Titles and summaries in results; `learn.microsoft.com` did not resolve |
| Crate records | Yes | crates.io API, 2026-11-23 |

## Part 1. The accessibility routes, and what each asks of the person

Note 19 has the table. This part adds what that table did not carry.

### Windows: UI Automation

- **No consent step is documented.** The assistive-technology page describes an assistive program as running at medium integrity, and names the user-account-control dialog as the one surface that is shielded from other programs, "so that malicious software cannot simulate user input" [read, as summarised; `uiauto-securityoverview.md`](https://github.com/MicrosoftDocs/win32/blob/docs/desktop-src/WinAuto/uiauto-securityoverview.md). Nothing is shown to the person when a medium-integrity program reads a medium-integrity window.
- **The one boundary is the integrity level.** To reach a window at a higher level, the program needs the `uiAccess` flag in its manifest, must be signed with a code-signing certificate, and must be installed in a secure location such as Program Files. The page also says `uiAccess` is not enough to cross a level on its own, has no effect against system-level processes, and is unavailable to packaged (UWP) applications [read, as summarised, same page]. Note 26 and decision 0209 already refuse this route and name the stop `target_elevated`. Nothing here argues for changing that. Rule 5 agrees: a program that acts in another's protected window by a route the system reserves for signed assistive programs is getting past a protection.
- **A write is conditional on the element, and the element says so.** The Value pattern's `SetValue` requires the element to be enabled and not read-only (`CurrentIsEnabled` true, `CurrentIsReadOnly` false) [read, as summarised; `iuiautomationvaluepattern-setvalue.md`](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/uiautomationclient/nf-uiautomationclient-iuiautomationvaluepattern-setvalue.md). This is the one place where an interface says, per element, whether a value may be set. The back end for writes can read both properties before it acts and stop with a named failure if either is wrong. The Windows write slice should do so.
- **Whether a pattern is read-only is not stated by the pattern list.** The overview of control patterns says methods manipulate a control and properties and events describe it; it does not say which patterns only read [read, as summarised]. A call to `Invoke` is a write by any reading; no property marks it otherwise.

### macOS: the accessibility interface

- **The gate is a listing in System Settings, Privacy and Security, Accessibility, and the person alone makes it.** Nothing in the program grants it [note 26]. A program started from a terminal is judged as the terminal [note 26]. This decides how the engine must be started on macOS if the person is to have one entry to manage. It repeats the finding on Local Network permission in [note 28](28-how-a-network-can-be-seen-from-one-machine.md) (part 5): the one that started a program is the one the system judges.
- **The system's own prompt can be raised by asking for it, and the pack must not ask during a run.** Note 26 already says so for `AXIsProcessTrustedWithOptions`. Nothing new was found.
- **A per-attribute "is this settable?" exists in the interface** (`AXUIElementIsAttributeSettable`) [memory; the Apple page did not load]. If it holds, it is the macOS counterpart of the Windows read-only property and the write slice should ask it before every set. Check it against Apple's page before relying on it.
- **A hosted CI machine allowed the grant by an administrator's write** ([decision 0278](../decisions/0278-what-the-first-runs-on-windows-and-macos-showed-about-the-desktop-pack-and-the-check-of-programs.md), point 4). That is the test fixture's act on a throwaway machine. It is not a route for the product; rule 5 forbids the product from writing the permission store, and the same holds for the Automation store in part 2.

### Linux: AT-SPI

Nothing new. No consent is asked; some toolkits publish only when the session's accessibility switch is on, which the person owns [note 19].

## Part 2. The scripting routes

For each route the same four questions: how a program names what it reaches; what is described about it; whether the description says a call only reads; what it asks of the person. A fifth question applies to all: can a call start the other program.

### Windows: COM automation

| Question | Answer |
| --- | --- |
| How is the other program reached? | By class identifier, through the registry's COM registrations, or, for a program already running that registered itself, through the running-object lookup. `GetActiveObject` "retrieves a pointer to a running object that has been registered with OLE" and takes a class identifier [read; `getactiveobject.md`](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/oleauto/nf-oleauto-getactiveobject.md). Its failure when nothing is running is the safe answer for a pack that must not start anything |
| What is described? | A type library: interfaces, members, parameter types, and for each member an invocation kind and flags. The kinds are plain function, property get, property put, and property put by reference [read; `INVOKEKIND`](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/oaidl/ne-oaidl-invokekind.md) |
| Does it say a call only reads? | Only for a property get. A plain function has no such mark. The member flags (restricted, bindable, hidden, and others) are about display and binding, not effects; the restricted flag means a member "should not be accessible from macro languages" [read; `FUNCFLAGS`](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/oaidl/ne-oaidl-funcflags.md). A property get can still have side effects, since the library only declares the shape |
| What does it ask of the person? | Nothing at the same integrity level and the same user. A program reaching an object another user or an elevated program owns is a boundary the system enforces through COM's security settings [memory]; there is no prompt |
| Can it start the program? | Yes: creating an object by class identifier starts a registered local server [memory]. The lookup of a running object does not |
| Rust | The `windows` crate, 0.62.2, published by Microsoft, already in the workspace with only `std` [note 26]. It carries the Automation interfaces and the type-information interfaces behind features [memory; `docs.rs` did not load, so the exact feature names are unchecked]. The `com` crate, 0.6.0, was last released in 2021 and is not a candidate [read; registry] |

Two things to carry into the design. First, reading a type library as a file, as decision 0276 point 1 says, gives names and kinds without running the program, and that is the only part of this route that is free of effect. Second, calling a member through `Invoke` runs the other program's code with the person's rights; it is a write unless proven otherwise, and the type library cannot prove otherwise for a function.

### macOS: Apple events and scripting dictionaries

| Question | Answer |
| --- | --- |
| How is the other program reached? | An Apple event is addressed to a running application by process, by bundle identifier, or by name. The scripting language, and the system's `osascript` command, resolve a name and **launch the application if it is not running** [memory] |
| What is described? | The scripting definition, an XML file with the extension `.sdef`: classes, properties, elements, commands, enumerations, and the four-character codes that become Apple events. Cocoa loads it once per launch and registers a handler per command [read, as summarised; Apple's scripting guide](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ScriptableCocoaApplications/SApps_about_apps/SAppsAboutApps.html). The application names it in its own property list |
| Does it say a call only reads? | For a property, yes: it can be marked read-only or read-write. For a command, no. And Apple's guide states the limit: "Cocoa does not currently enforce the read/write status declared in an sdef. Marking a property read-only does not stop it from being written" [read, as summarised, same page]. The mark is the author's statement, not the system's guarantee |
| What does it ask of the person? | A consent prompt, **per pair of programs**: the sender and the target. The first event raises the system's prompt, whose text includes a sentence the sender supplies in its property list under `NSAppleEventsUsageDescription`. The text reads, roughly, that the sender "wants to control" the target and that allowing it "will provide access to documents and data" [memory; the Apple entitlement page returned only its title]. The answer is kept and the person changes it in System Settings, Privacy and Security, Automation [search, [forum thread on the entitlement](https://developer.apple.com/forums/thread/751802) and [the permission preflight call](https://developer.apple.com/forums/thread/130949)]. When the person refuses, the refusal is kept and later events fail with error -1743 and no new prompt; the person must change the setting [search, summary of a scripting-community source in the results of the same query]. |
| The preflight call | `AEDeterminePermissionToAutomateTarget` says before sending whether the pair is allowed, and can be told to ask the person or not [search; Apple's page did not load, and one developer reported that the asking form could hang [forum thread](https://developer.apple.com/forums/thread/666528)]. The pack should use the form that does not ask, as with the accessibility check, and treat an answer of "not yet decided" as a stop with a named code that tells the person what to do |
| Entitlement | A program with the hardened runtime, which notarisation requires, needs `com.apple.security.automation.apple-events` to send Apple events at all. Without it the system's privacy daemon logs an error and the event fails [search, [forum thread](https://developer.apple.com/forums/thread/751802)]. A plain command-line program that is not hardened does not need it [memory]. The product's own signing is a release matter; the question is open for [section 605](../spec/6-running-it/605-packaging-and-release.md) |
| Who is judged? | The program the system takes as responsible, as for Accessibility [memory]. A run that starts from a terminal is judged as the terminal. The same advice as for Accessibility holds: start the engine the one way, so the person manages one entry |
| Can it start the program? | Yes, by name or by `osascript`. Addressing by process number of a running program does not start it, and a send to a program that is not running fails [memory]. The pack must attach by process, proved the way the accessibility back end proves it (decision 0244) |
| Rust | `objc2-foundation` 0.3.2 (the workspace has it) holds `NSAppleScript`, behind a feature [memory; unchecked]. `objc2-osa-kit` 0.3.2 and `objc2-scripting-bridge` 0.3.2 are from the same project and release; the latter's licence is `Zlib OR Apache-2.0 OR MIT` and it depends on `objc2`, `objc2-foundation`, `libc`, and `objc2-core-services` [read; registry]. `scriptingbridge-rs` 0.4.2 and `osakit-rs` 0.3.2 are by one other author, first seen in 2026, with 169 and 138 downloads [read; registry]: not candidates. All of this is `unsafe` at the surface for the same reasons as in note 19 |

Two things to carry into the design. First, a script run by `osascript` receives the sender's identity from the program that runs it, so decision 0214 row 6's fixed pair of interpreter and script means the person's Automation entry is for the interpreter, not for the engine. That is not a problem; it means the one grant covers every script the engine ever hands that interpreter, which is wider than a grant for one target. The wording of the consent in the product's own review must say so. Second, there is nothing that lets a program check the sdef's read marks against what the application does; the check is a test against the application, run by a person who may use the application, which is a real application and so rule 10's matter, not this note's.

### Linux: D-Bus

| Question | Answer |
| --- | --- |
| How is the other program reached? | By a well-known name or a unique name on the session bus (the person's own services) or the system bus. The session bus has one user's programs on it [memory] |
| What is described? | Introspection: the standard `Introspect` method returns XML with interfaces, methods, signals, and properties; each property carries a type and an access value, `read`, `write`, or `readwrite`. Annotations carry more, for example whether a property emits a change signal [search; the Emacs manual and the source of two D-Bus libraries named in the search results]. An application may also ship the interface files, and a service may implement only part of the introspection |
| Does it say a call only reads? | For a property, by its access value. For a method, no: nothing in the format says a method is free of effect [search, memory]. The standard properties interface has `Get`, `GetAll`, and `Set`, so a property can be read without calling the service's own methods [memory] |
| What does it ask of the person? | Nothing for a program in the person's session on the session bus: any program on the bus may send to any name unless the bus's policy files forbid it [memory]. A sandboxed program (Flatpak) is the exception: by default it may own only its own names and talk to its own namespace, the bus daemon, and the portal interfaces; wider access is a permission the person or the package sets [search, [Flatpak sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)]. A program the engine reaches inside a sandbox may therefore not be on the bus the engine sees |
| Can it start the program? | Yes: a call to a name nobody owns can start a service that has an activation file [memory]. Decision 0276 point 1 already says introspection is asked so that a service that is not running is never started; the bus has a call that lists the names that can be activated and another that lists the names now owned, and the pack should read only the second [memory] |
| Input without an accessibility route on Wayland | Faking input on Wayland goes through the desktop portal, which asks the person, or through the compositor's input-emulation service reached by an explicit permission [search; a mailing-list thread on the input-emulation library]. The pack does not fake input (decision 0198), so this is only context |
| Rust | `zbus` 6.5.0 (the workspace pins 6.3.0, features `tokio` and `proxy`) and `zbus_xml` 6.0.0, both from the same project [read; registry and root manifest]. `zbus_xml` reads introspection XML; whether it is already in the lockfile was not checked |

## Part 3. What each route asks of the person, in one table

| Route | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Accessibility tree | Nothing, at the same integrity level | A listing under Accessibility, made by the person, for the program the system judges | Nothing; sometimes the session's switch |
| Scripting route | Nothing, at the same integrity level and user | A prompt per sender and target pair, kept; a refusal is kept; an entitlement for a hardened sender | Nothing on the session bus; a sandbox narrows what is visible |
| The system's own elevation | A consent prompt each time (decision 0214 row 5) | An authorization prompt each time | A polkit prompt each time |
| What reads the prompt's answer back | Not needed | The preflight call, in its form that does not ask [search] | Not needed |

The pack should not state these permissions in prose in a hundred places. They are one table, per system, in the pack's design, with the failure each missing grant ends in.

## A route Microsoft is adding

Sources found in search results describe, for Windows 11 in preview as of late 2025: agent connectors, which are MCP servers that apps register in an on-device registry so that agents can find them; a proxy through which that traffic passes; a declared minimum of capabilities per connector; and an agent workspace, which in the first preview runs in a separate session, possibly under an account of its own for each agent [search; [Windows developer blog](https://blogs.windows.com/windowsexperience/2025/05/19/securing-the-model-context-protocol-building-a-safer-agentic-future-on-windows/) and [the support page on experimental agentic features](https://support.microsoft.com/en-us/windows/ai/ai-features/experimental-agentic-features)]. A first-party connector for files asks the person's consent for the six known folders [search]. What was not found: whether agents there drive other applications through UI Automation, and how that is consented.

Two consequences, both for later. The pack's idea that an agent's session is a separate desktop is partly met by the system itself. And an application that registers a connector offers an MCP server that the MCP pack could reach; that is a different route from this note's and needs its own note when it leaves preview.

## What this means for package 8.3

1. **Windows writes.** Read the element's enabled and read-only properties before every set; stop with a named code if either is wrong. A control with no value pattern is written only as `irreversible` (section 305 says so already).
2. **macOS writes.** Check, before the first set, whether the attribute is settable [memory; verify]. Keep note 26's rule: the pack never raises the system's prompt in a run.
3. **No scripting route in 8.3.** The accessibility back ends do not need one, and each scripting route has a start-the-program path the pack must not take and a consent shape the pack does not yet carry. Decision 0214 row 6 (a fixed pair of interpreter and script) already reaches Apple events and COM for the first release through the command pack. When a pack of its own is built, it attaches to a running program only.
4. **A finding from a scripting dictionary, type library, or introspection says where its "only reads" came from**, as decision 0276 point 1 already requires. It must not be raised above the level of a published claim. The three formats cannot support more (part 2).

## What was not done

- Parts (a), (b), (d), and (e) of brief 35: the folders where programs are installed and who may change them, whether `osascript` and `-File` pass arguments as literal text (hypothesis H-32), whether browser-runtime frameworks take a data directory and a debugging pipe, and how each system's elevation prompt can be asked to start one declared program. They remain open.
- No run on any system. In particular, no consent prompt was seen, and no sdef or type library of a real program was read.
- The `AEDeterminePermissionToAutomateTarget` return codes were not read from Apple. The names `errAEEventNotPermitted` (-1743) and `procNotFound` come from [memory] and from community reports.

## What could not be verified

- The text and behaviour of macOS's Automation prompt, the entitlement's page, and `NSAppleEventsUsageDescription` (Apple's pages did not load; results are from forum threads and memory).
- That an Apple event sent by name launches a closed application, and that one sent by process number does not.
- That COM object creation starts a local server, and the `windows` crate's feature names for the Automation and type-information interfaces.
- The D-Bus specification's wording on access values, annotations, and bus policy; only implementations were seen.
- The `AXUIElementIsAttributeSettable` function's behaviour.
- The current status of Microsoft's agent workspace and MCP on Windows, which the sources describe as preview in 2025.
- That `zbus_xml` is or is not already in the lockfile, and that its licence is on the closed list (the `zbus` family's was checked in earlier notes; this one's was not).

## What this changes in the design

Each item is a proposal. The lead applies, adjusts, or drops it.

1. **Decision 0276, point 1 (new decision record that supersedes in part).** Replace "where the publication says which calls only read" with: "A reader of a publication records what the publication says, and for each call records one of three: the publication marks it as a read (a property get, a property marked read-only or read, a property read through the standard properties interface); it marks it as a change; or it says nothing. A call that the publication says nothing about is never treated as a read. A mark that the publication makes and the system does not enforce, as an sdef's read-only mark, is a claim of the author and is recorded at the level of a published claim, never above it." Reason: part 2 above.
2. **Hypothesis H-41 (register entry).** Add to its test: "For each system, take one declared member the publication marks as a read and one it marks as a change, run both against the test stand, and compare the stand's own record of what was stored." Add to its expected failure: "An sdef's read-only mark is not enforced by Cocoa." Status unchanged.
3. **Spec section 305, "What it reaches, and what it refuses".** Add a row to the refused table: "Starting an application by sending it a message addressed by name | The system starts a closed application when sent one by name, in several of its scripting routes. The pack attaches to a running process it has proved, and starts a program only through the supervisor, by path and digest."
4. **Spec section 305, a new short section "What the person must grant, by system".** "On Windows, nothing at the same integrity level; a window at a higher level is refused as `target_elevated`, and the pack does not use the signed, installed-in-a-protected-place route the system reserves for assistive programs. On macOS, the person lists the program the system judges under Accessibility; a program started by another is judged as that other; the pack asks the system for the answer without its prompt and never raises the prompt during a run. On Linux, nothing, except that some toolkits publish only when the session's accessibility switch is on, which the person owns and the pack never changes. A scripting route is not part of this pack; see decision 0214, row 6."
5. **Spec section 305, Windows and macOS writes.** Add: "Before every set, the back end reads whether the element is enabled and whether it is read-only, and stops with a named failure code, before sending, if the element is disabled or read-only. On macOS the back end first asks the interface whether the attribute is settable." Mark the macOS sentence "to verify against Apple's page when the write slice is built".
6. **Decision 0214, row 6, "Consequence" for macOS.** Add: "The Automation permission the person grants on macOS belongs to the program the system judges as the sender, which is the interpreter or the program that started it. One grant therefore covers every script that sender runs, not one target. The review summary must say this in the person's words before a script is declared." Also add: "A script that addresses an application by name may start it. A declared script names its target by bundle identifier, and the declaration says whether the target must already be running; where it must, the script fails if it is not."
7. **Failure codes for `a2m-pack-desktop` (design table).** Propose one name for a missing Automation grant, if a scripting route is ever built: `busy`/`automation_not_granted`, next step naming System Settings, Privacy and Security, Automation, and the sender. Not for 8.3; record in the design as reserved.
8. **Open research, brief 35.** Mark part (c) answered by this note, with the unverified items above carried as open; parts (a), (b), (d), and (e) stay open. Update the index row of note 33 in [README.md](README.md) and the count of answered briefs.
9. **SCOPE.md, line 31 ("Applications' own scripting interfaces ... Not yet").** Keep the status. Add the reason in kind: "not built; its descriptions cannot say which calls only read, so each call is a write until a person declares otherwise."
10. **Note 26, the Windows and macOS permission table.** Add a cross-reference to this note for the Windows `uiAccess` conditions and the Automation consent shape. No change of fact.
