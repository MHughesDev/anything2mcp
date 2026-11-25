# 305. The desktop pack

The desktop pack lets a target be a desktop application: a program with windows that a person uses on their own machine. It reaches the application through the operating system's accessibility interface (UI Automation on Windows, the Accessibility API on macOS, AT-SPI on Linux), finds elements by role and name, and acts on them through the interface's own patterns. It brings one mode, `desktop`, in the interface archetype. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

**How far it has got.** The spike of package 8.1 chose its base ([decision 0120](../../decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md)). Reads and writes on Linux are built ([decision 0198](../../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md), [decision 0218](../../decisions/0218-the-desktop-pack-writes-on-linux-through-a-second-closed-list-and-reads-back-what-is-stored.md)); reads on Windows are built ([decision 0209](../../decisions/0209-the-desktop-pack-reads-windows-applications-through-ui-automation-on-a-thread-of-its-own.md)); reads on macOS are built, and rest on the Accessibility permission the person grants in System Settings ([decision 0244](../../decisions/0244-the-desktop-pack-reads-macos-applications-and-proves-the-record-before-every-act.md)); both passed against the stand on CI's hosted runners ([decision 0278](../../decisions/0278-what-the-first-runs-on-windows-and-macos-showed-about-the-desktop-pack-and-the-check-of-programs.md)). Writes on Windows and macOS are written and linted, through UI Automation's value pattern and `Invoke` and through `AXValue` and `AXPress`, each by a closed list of its own, and not yet run ([decision 0399](../../decisions/0399-the-desktop-pack-writes-on-windows-and-macos-through-a-closed-list-of-its-own-on-each.md)). This section says what is settled, what is assumed, and what is still to be learned; where a later step of the build has settled a point, the crate design says so first.

The pack is one crate, `a2m-pack-desktop`. It has no base crate and depends on no other pack. Its [design](../../architecture/crates/a2m-pack-desktop.md) is the exact statement of how the code is arranged; this section says what the pack does and why.

## What is settled, and what is not

| Settled | Not settled |
| --- | --- |
| The mode `desktop`, archetype interface ([decision 0005](../../decisions/0005-seven-modes-behind-one-driver-interface.md), [decision 0050](../../decisions/0050-seven-archetypes-and-modes-that-arrive-with-packs.md)) | The accessibility library, or libraries |
| Elements are found by role and name in the accessibility tree; never by position on a picture of the screen | How a window is reached: started, attached, matched by title or by process |
| Its steps are those of `ui` except `observe` ([steps reference](../../reference/steps-snapshots-and-diffs.md#steps)) | Which conditions each operating system can signal as events |
| Its verbs are the `desktop` column below | How a desktop application's own sign-in is kept, and by which auth provider |
| Widget drivers share their control names with the web pack where the control exists in both | Profiles for toolkits, and what a support tier means here |
| Its test stand is test-application case 28: a small native program with the same data, on three operating systems | Which toolkit the test program is written with, so that its accessibility data is good on all three |
| Phase 8 | Where a desktop run may happen on a server |

## What it reaches, and what it refuses

| Reaches | How |
| --- | --- |
| A desktop application on the machine the engine runs on, in an interactive desktop session | Started through the supervisor the pack's context scopes to the programs the manifest and the published compiled target declare, by path and digest; or attached to one a person named |

| Refused | Why |
| --- | --- |
| Clicking or typing by screen position | Nothing improvises. The backend has no such method |
| Acting in a window it cannot prove is the expected one | Rule seven: unknown means stop |
| Reading a password field's value | Rule four |
| Attaching to a process a person did not name | Consent ([section 213](../2-kernel/213-effects-hazards-and-consent.md)) |
| Acting while a person is using the same desktop session, unless the target says it may | A run and a person typing into one desktop cannot be told apart. It is `busy`, `session_in_use` |
| Starting an application by sending it a message addressed by name | The system starts a closed application when sent a message by name, in several of its scripting routes ([research note 33](../../research/33-what-a-program-can-reach-in-another.md), partly from memory). The pack attaches to a running process it has proved, and starts a program only through the supervisor, by path and digest |
| An application built on a browser runtime | It is a web surface, served by `ui` ([section 301](301-the-web-pack.md)). The browser engine is the web pack's, and this pack cannot depend on it |

## What the person must grant, by system

What each system asks of the person before the pack may read a window, from [research note 33](../../research/33-what-a-program-can-reach-in-another.md). Only what the note read in a source is stated as fact; the rest is marked.

| System | The person grants | A missing grant ends as |
| --- | --- | --- |
| Windows | Nothing, for a window at the same integrity level as the engine. A window at a higher level is refused; the pack does not use the route the system reserves for signed assistive programs installed in a protected place, which would get past the system's protection (rule 5) | `target_elevated` |
| macOS | A listing under Accessibility in System Settings, Privacy and Security, for the program the system judges, which is the program that started the engine when it was started by another (note 26). The pack asks the system for the answer without raising its prompt, and never raises the prompt during a run | `accessibility_off`, whose message says where in System Settings to allow it (decision 0244) |
| Linux | Nothing. Some toolkits publish only when the session's accessibility switch is on; the switch is the person's, and the pack never changes it | `accessibility_off` |

A scripting route is not part of this pack. Where the system asks the person for one, it asks per pair of programs on macOS (kept, and a refusal kept), and not at all on Windows or on the Linux session bus; a failure code, `automation_not_granted`, is reserved in the pack's design for a missing grant on such a route, and is not in the table of codes until something produces it.

Open points of this section, which note 33 marks as not read: the text and behaviour of macOS's Automation prompt; the entitlement a hardened program needs to send Apple events; that an Apple event sent by name starts a closed application; whether `AXUIElementIsAttributeSettable` exists as described (the bindings the pack uses declare it, and the macOS write calls it; what it answers is unproven until CI runs it); the wording of the D-Bus specification on access values and bus policy; the current state of the agent features Microsoft is adding to Windows.

## The mode

| Mode | Archetype | How it reaches the system | Needs | Best for | Main risk |
| --- | --- | --- | --- | --- | --- |
| `desktop` | Interface | The operating system's accessibility tree | The application running on the machine, in an interactive session | Desktop clients | Applications with poor accessibility data |

## What it registers

| Registration | Content |
| --- | --- |
| Surface kinds | `desktop` |
| Modes | `desktop`, archetype interface |
| Step kinds | `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, `upload`, checked names within the mode |
| Driver | One |
| File kinds | Owns `screens/` for surfaces of kind `desktop`. Registration is keyed by file kind and surface kind, so the web pack's registration for `web` surfaces does not collide. `reach` takes a window match (process, title, or an automation identifier) or `from` another screen with an action. The `css` locator is not accepted; `test_id` means the interface's automation identifier |
| Binding fields | `screen`, `verb`, `fields`, `action`, as for `ui` |
| Control names | `desktop_text`, `textarea`, `number`, `select`, `combobox`, `checkbox`, `radio`, `date`, `grid`, `tree`, `tabs`, `dialog`, where the operating system's interface exposes them. The rest are refused until a test passes |
| Finding kinds | `window` for desktop subjects, built: one window's shape, never a value; `surface` with `desktop_probe` |
| Discoverers | `desktop_probe`, `desktop_snapshot` |
| Describer | `desktop_describer`, in the snapshot text form |
| Auth providers | None yet |

## Verbs and how they lower

| Verb | `desktop` |
| --- | --- |
| `open`, `read`, `list`, `search` | Yes |
| `create`, `update`, `upsert`, `delete` | Yes |
| `upload`, `submit` | Yes |
| `download`, `call` | No |

Each verb lowers into `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, and `upload`, compiled with its screens into the pack's opaque step payload. An action with `commits: true` is a commit point.

## The driver, part by part

| Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- |
| The application started by path and digest, or attached as a person named it; its window found, and its process proven by identifier, start time, and program digest. The first slice finds the application already running and starts nothing ([decision 0198](../../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)) | `locate`, then `set` through the widget driver, preferring the interface's value pattern; `invoke` through its invoke pattern; no synthetic key or click until a test shows one can be bound to its window (decision 0198) | The action's declared `confirm`: a window, an element, a value | The field again, after the screen is reached afresh |

- **One request at a time per desktop session.** The scheduler serializes work that changes what is on a person's screen, so two runs never type into one desktop at once ([section 204](../2-kernel/204-supervising-subsystems.md)).
- **Before every act** the driver proves that the element is in the window and the process the step expects.
- **Waits are events or nothing.** Each condition is ended by an accessibility event. Where an operating system cannot signal a condition, the condition is refused for that system by a lint rule, not polled. Whether enough conditions remain is a question for the spike.
- **Every failure says whether the act was sent.** An `invoke` of a committing action that was sent, and whose confirmation did not arrive, or after which the application exited, is `possible_commit`.
- A control that exposes no value cannot be written except as `irreversible`.
- **Before every set, the driver reads whether the control is enabled and whether it is settable** (offers editable text and is not read-only), and stops before sending anything, as `field_read_only`, if it is not. Built on Linux and proven against the stand with a disabled field ([decision 0336](../../decisions/0336-the-desktop-pack-states-that-it-sends-to-no-address-and-checks-that-a-field-is-enabled-before-it-sets-it.md)). On Windows the read is of the element's enabled property and the value pattern's read-only property. On macOS the driver reads `AXEnabled`, then asks the interface whether `AXValue` is settable (`AXUIElementIsAttributeSettable`, which the `objc2-application-services` bindings declare); how AppKit answers it for a field that is not editable is unproven until CI runs it ([decision 0399](../../decisions/0399-the-desktop-pack-writes-on-windows-and-macos-through-a-closed-list-of-its-own-on-each.md)).
- **What the review says.** A binding states that it starts no program, sets no variable, and sends to no address; the pack attaches to a program that is already running, and what the program does after an action is its own (decision 0336).

The backend is private: one implementation per operating system, or one over a library that covers all three. It finds exactly one element or fails, reads, sets, invokes, reports events (focus, structure, value, window opened), and builds the tree for snapshots.

## Sharing with the web pack without depending on it

A pack may not depend on another pack's crates, so this pack cannot use the web pack's widget drivers or locating code. What the two share is held in three places, none of them code.

| Shared | Held by |
| --- | --- |
| The names of controls, locator kinds, conditions, steps, and the `screens/` fields | The [file formats](../../reference/file-formats.md#screensyaml), the [steps reference](../../reference/steps-snapshots-and-diffs.md), and the control table of [section 301](301-the-web-pack.md#widget-drivers). Each pack's schema is checked against them by a test |
| The snapshot's text form | The steps reference. An agent reads a window and a page the same way |
| What a driver must do | The conformance suite's groups for locating, waiting, acting, and snapshots, run against case 28 as against the web cases |

The code is written twice, each over its own element handle. That is the price of the rule, and it is measured: if the second copy grows large, the parts that belong to the archetype (the `screens/` skeleton, locator ordering, condition evaluation over an abstract tree) are a candidate to move into the kernel by decision record. That would be a finding for hypotheses H-26 and H-37.

## Sessions and sign-in

A desktop application's own sign-in lives inside the application, in the runner's desktop session, and is never copied. The identity of a first desktop target is the signed-in desktop user. The rules for sessions are the kernel's ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)); the auth provider is not designed. The likely shape is a provider that checks the sign-in through a probe screen and asks a person to sign in when it fails.

## Discovery and drafting

| Discoverer | Reads | Finding |
| --- | --- | --- |
| `desktop_probe` | The program, its version and digest, and how many controls have no name | A surface, with a support tier |
| `desktop_snapshot` | The window's accessibility tree | Screens, fields, actions |
| `desktop_bundle` | The files of a macOS application's bundle a person hands the scout under `bundle`: `Contents/Info.plist` (XML or binary) and the scripting dictionary it names | The application's identity, URL schemes, document types, services, and its dictionary's suites, commands, events, classes, and each class property with the access the dictionary states |
| `desktop_management` | On Windows, the definition of the management class the surface's `management_class` names, asked of the system's management service | The class's properties, each with its type and whether the definition marks it writable, its methods, and whether it is a singleton |
| `desktop_com` | A COM type library a person hands the scout under `type_library`, read as bytes on every system | The library's interfaces and each member, with how the library declares it to be invoked |
| `desktop_service` | On Linux, the introspection of the service the surface's `service` names on the session bus, a `.service` file, and an interface description | The service's interfaces: methods, signals, and each property with the access its document states |

Both read the tree and never invoke, expand, or focus anything. What is hidden behind a collapsed node is a limitation, `not_exercised`. They run as discovery jobs inside the engine's sandbox ([section 218](../2-kernel/218-discovery-and-findings.md)), for `target.probe`, `screen.snapshot`, and `discover.run`.

`desktop_service` asks a running service for its introspection, with the bus's flag that forbids it to start the service on every message, and calls no method of it. A service that no connection owns is reported as not running and is never started. A `.service` file and an interface description a person hands the scout (`service_file`, `interface_file`) are read as text and never run. Its findings are `dbus_service`: the live service's at `discovered`, a file's at `inferred`. From a running service's finding the pack drafts a `read` of each property whose document states `access="read"`, as a binding `property: {surface, path, interface, name}`; a property that states `readwrite` or `write`, a method, and a signal are never drafted ([decision 0331](../../decisions/0331-the-desktop-pack-reads-a-running-services-introspection-and-drafts-reads-of-its-read-only-properties.md)).

`desktop_bundle` reads two files of the bundle as bytes, on every system, and sends nothing to any application. A dictionary that includes a file outside the bundle is refused whole, by the name `dictionary_include_refused`; an include inside the bundle is listed and not followed. Its findings are `application_bundle`, at `inferred`. Nothing is drafted from them: reading a property of a running application is an Apple event, which the pack does not send ([decision 0332](../../decisions/0332-the-desktop-pack-reads-an-application-bundles-property-list-and-the-scripting-dictionary-it-names.md)).

`desktop_management` asks for the definition of one class and never for an instance; on any other system it stops with `system_not_built`. `desktop_com` reads a type library as bytes with a library taken whole and fuzzed, and never loads it through the system; only the MSFT form is read, and any other is the named stop `type_library_format_unknown`. From a singleton class's property the definition does not mark writable, and from a dispatch interface's property get, the pack drafts a `read`, binding `management: {surface, class, name}` or `com: {surface, interface, name}`. The drafts compile and are not run: this version carries out no `management_get` or `com_get` step, and says so by the named stop `step_not_built` ([decision 0333](../../decisions/0333-the-desktop-pack-reads-a-management-classs-definition-and-a-com-type-library-and-drafts-reads-that-it-does-not-yet-run.md)).

The drafting rules are as for the web: from a window's snapshot, a screen model with fields, roles, names, and locators; field types and every action left as open questions. As built ([decision 0462](../../decisions/0462-a-desktop-table-of-cells-is-read-by-the-column-count-it-states-and-a-title-two-windows-show-to-differ-becomes-a-pattern.md), from copy test 10's findings 57 to 60):

- **Values.** A labelled `entry`, `text`, or `spin_button` the window names once is a field of one `get` read; a spin button only when the snapshot says it gives its value as text, and one that does not is named in the read's description.
- **Tables.** Each table the window names once, and the one it gives no name, is a screen and a `list` read. A table of rows is read as before. A table whose column headers and cells are its own children, as GTK 3 publishes one, is read only by the column count its system states (on Linux, AT-SPI's `Table` interface): that many headings first, then that many cells to a row. One that states no count is never read, by a draft or a screen written by hand: the read stops as `fingerprint_mismatch` and says why. A table with no name is located by its headings alone: its screen's `rows` has no `locate`.
- **Titles.** A window is reached by its role and its exact title. Within one snapshot, windows of one role and one shape whose titles differ in exactly one plain word are one finding, the title a pattern, `Order {order_id}`, named as the web probe names a path's placeholder; its reads take the placeholder as their required input and key, and the `goto` finds the window of exactly that title. One title is never a pattern; a read over a whole title with a digit in it says how to get one. A screen whose title has a placeholder is read only (`desktop.title_write`).
- **Reasons.** Each finding the rules draft nothing from is listed with the reason.

A column or a control whose name the compiler takes for a secret's is never drafted as an output, as for the web, and the last sentence of the drafted description tells the names left out ([decision 0302](../../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)).

## Lint rules

| Code | Level | Rule |
| --- | --- | --- |
| `desktop.css` | Error | A `css` locator. As built, the schema of `screens/` has no `css` key, so none compiles |
| `desktop.unnamed` | Error | A role locator with an empty name. As built, a refusal of the file kind |
| `desktop.condition_unsupported` | Error | A condition this operating system cannot signal. Not built: on Linux every condition offered is signalled |
| `desktop.title_only` | Warning | A window reached by title alone. Not built: a window has no other match yet. A title that names a record becomes a pattern only from two windows (decision 0462) |

## Effects and hazards

| Effect | Declared as |
| --- | --- |
| Starts a process | The application, by path and digest |
| Attaches to a running process | Only one a person named, proven by identifier, start time, and digest |
| Changes what is on a person's screen | Focus, windows, input; serialized by the scheduler |
| Reads and writes files | A staging folder for file dialogs, as a checked path |

Hazards come from operations ([section 213](../2-kernel/213-effects-hazards-and-consent.md)).

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `drift` | `locator_not_found`, `fingerprint_mismatch`, `locator_ambiguous` | As for `ui` |
| `unknown_variant` | `unexpected_dialog`, `unexpected_page` | A modal window, or a different window than the screen declares. Built as `unexpected_page`: before every act, a window that no longer shows the run's record, or any window of the application that no screen names, stops the act with nothing sent (decision 0244); `unexpected_dialog` is not built |
| `rejected` | `app_validation_error` | A declared variant showing a validation message, carried as the application's own words |
| `unknown_variant` | `field_read_only`, `unexpected_control`, `value_not_held` | A field takes no value today; an element offers no editable text or no action of the declared name; a field holds, after `set`, another value than the one set: stopped before anything is saved |
| `policy` | `password_set_refused` | A step would set a control that holds a secret |
| `busy` | `writes_not_built` | A write on a system whose acting half is not built: any but Linux, Windows, and macOS. Nothing is sent |
| `possible_commit` | `no_confirmation` | An `invoke` of a committing action was sent and its confirmation did not arrive, or the application exited. A resume reads the stored value before it writes again |
| `busy` | `session_in_use` | A person is using the desktop and the target does not allow sharing it; or another write of the engine held the desktop past the step's deadline |
| `busy` | `runner_unavailable` | No interactive desktop session |
| `unknown_variant` | `type_library_format_unknown` | A file handed as a COM type library is in a form the reader does not know (decision 0318) |
| `busy` | `step_not_built` | A drafted read of a management class's property or a COM property get compiles, and this version carries out no such step |
| `policy` | `dictionary_include_refused` | A scripting dictionary a person handed the scout includes a file outside the application's bundle (decision 0317) |
| `busy` | `service_not_running` | The service the surface names has no owner on the session bus when a property is read; nothing was started (decision 0316) |
| `unknown_variant` | `property_not_text` | A service's property holds a value that is not text, a flag, a number, or an object path |
| `busy` | `application_not_running`, `accessibility_off`, `system_not_built` | The application the surface names is not running; it runs and publishes nothing while the session's accessibility switch is off, or on macOS the person has not allowed the program that started the engine to use the accessibility interface, and the message says where in System Settings to allow it (decision 0244); the pack has no back end for this system (decision 0198) |
| `busy` | `target_elevated` | On Windows, the application runs at a higher level than the engine, as administrator ([decision 0209](../../decisions/0209-the-desktop-pack-reads-windows-applications-through-ui-automation-on-a-thread-of-its-own.md)) |
| `policy` | `password_read_refused` | A step would read a control that holds a secret |
| `policy` | `application_not_owned` | On Windows, the program the surface states runs as another user or in another session ([decision 0209](../../decisions/0209-the-desktop-pack-reads-windows-applications-through-ui-automation-on-a-thread-of-its-own.md)); on macOS, as another user (decision 0244) |
| `policy` | `remote_namespace_refused` | On Windows, the surface's `management_namespace` names another machine; the driver asks the management service nothing, as the review withdraws the binding's statement (decision 0399) |
| `transient` | `timeout`, `connection_lost` | Before anything was sent |

## The test stand

Test-application case 28: a small native program with the same data as the test application, on three operating systems, with the same control interface, so ground truth comes from the program's store. On Linux it is a tree the stand publishes over AT-SPI (decision 0198); on Windows a window of plain Win32 controls in a process of its own, whose subclassed controls count the messages a client sends them (decision 0209); on macOS a window of AppKit's controls in a process of its own, through the same harness, whose subclassed fields and Save count what AppKit's accessibility asks of them (decision 0244) ([test application plan](../../plan/test-application.md)).

| Must always hold | Proven by |
| --- | --- |
| No act reaches a window other than the expected one | A second window of the same program, with the same title, opened before an act |
| No input is sent by screen position | Review, and the backend, which has no such method |
| A committing action lost to a crash is `possible_commit` | Case 28 with the program killed after `invoke` |
| Discovery changes nothing in the program | The control interface counts no write and no change of state during `desktop_snapshot` |
| Each widget driver sets and reads back its control | The conformance group "Acting", on case 28, on three operating systems |
| A table, a value, and a title as a GTK 3 dialog program publishes them are read, and a table that states no column count is not | On Linux, windows of one record each, opened by the control interface: a table with no name whose headers and cells are its own children, stating its column count or not; a labelled spin button; titles that differ in the record's number (decision 0462) |

## The entry bar

The eight items of [section 202](../2-kernel/202-kernel-and-packs.md).

| # | Item | The desktop pack's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | The `desktop` driver |
| 2 | Read-back for every write | The element's value after the screen is reached again, where the window shows what is stored, never the field typed into. A field written that no output reads back compiles only as `irreversible` (`desktop.unreadable_write`, decision 0218) |
| 3 | Each identity strategy | As for `ui`: `natural` and `composite` through a screen reached by key; `stamped` and `find` through a list or search window; `none` as `irreversible` |
| 4 | A test stand | Case 28 |
| 5 | A discoverer | `desktop_probe` and `desktop_snapshot` |
| 6 | Failures mapped | The table above |
| 7 | Effects and hazards | The table above; hazards come from operations |
| 8 | A record per new mode | None needed: decision 0005 |

## Build order

1. Slice 8.1, in `spikes/`, with brief 19: on each operating system, read a simple program's tree, find a text field by name, set it, and read it back. A decision record on the library ([research note 09](../../research/09-rust-building-blocks.md) lists candidates, none verified by use).
2. Case 28, with its control interface.
3. Package 8.3: reads, with `desktop_snapshot` (built on Linux, decision 0198, and on Windows, [decision 0209](../../decisions/0209-the-desktop-pack-reads-windows-applications-through-ui-automation-on-a-thread-of-its-own.md), where it is proven only once CI's Windows runner has run it, and on macOS, decision 0244, likewise); then writes with read-back, `update` and `upsert`, built on Linux ([decision 0218](../../decisions/0218-the-desktop-pack-writes-on-linux-through-a-second-closed-list-and-reads-back-what-is-stored.md)) and written for Windows and macOS (decision 0399), proven there once CI's runners have run them; then the remaining widget drivers one test at a time. It also waits on package D4: the command pack's count of kernel changes says whether the kernel is ready for another pack.

## Open points

- The library or libraries, and whether one interface over three systems hides differences a driver needs to see.
- How a window is named in `reach` so that two windows of the same program are told apart.
- Which conditions each operating system can signal as events, and whether enough remain.
- Where a desktop run may happen. A user's own machine with an interactive session is the only case designed; a server has no desktop by default.
- The auth provider for a desktop application's own sign-in.
- Whether profiles per toolkit are needed, as platform profiles are for the web, and what a support tier means here.
- How much of the code duplicated from the web pack should move into the kernel, once both exist.
- How a program built on a browser runtime is started so the web pack can drive it ([section 301](301-the-web-pack.md)).
