# `a2m-pack-desktop`

The desktop pack: the `desktop` driver, which reaches a desktop application through the operating system's accessibility interface (UI Automation on Windows, the Accessibility interface on macOS, AT-SPI on Linux), and the discoverers and drafting rules for an observed window. Reads on Linux are built ([decision 0198](../../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)), and reads on Windows are built and type-checked, unproven until CI's Windows runner runs them ([decision 0209](../../decisions/0209-the-desktop-pack-reads-windows-applications-through-ui-automation-on-a-thread-of-its-own.md)); writes on Linux are built, `update` and `upsert` with a read-back of what is stored ([decision 0218](../../decisions/0218-the-desktop-pack-writes-on-linux-through-a-second-closed-list-and-reads-back-what-is-stored.md)); reads on macOS are built and type-checked, unproven until CI's macOS runner runs them, and every act proves again, just before it acts, the record and that no window the screens do not name is open ([decision 0244](../../decisions/0244-the-desktop-pack-reads-macos-applications-and-proves-the-record-before-every-act.md)); writes on Windows and macOS are built and linted for both targets, each through a closed list of its own, unproven until CI's runners run them ([decision 0399](../../decisions/0399-the-desktop-pack-writes-on-windows-and-macos-through-a-closed-list-of-its-own-on-each.md)). What each back end stands on was settled by slice 8.1 ([research note 19](../../research/19-desktop-accessibility-base.md), [decision 0120](../../decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md)) and, for Linux, changed by decision 0198 after [research note 26](../../research/26-desktop-pack-before-it-is-built.md); this file says what it registers through the provider contract as built, and the steps that build it.

## Facts

| | |
| --- | --- |
| Side | Pack: desktop |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (with `CompiledTarget` and `Step`), `a2m-files` (registration), `a2m-auth` (`SessionLease`), `a2m-provider` (`Pack`, `Driver`, `Discoverer`, `Describer`, `SandboxPort`, `PackContext`, `Registrar`), `a2m-logs` (the clock of findings). Outside, on Linux only: the workspace's `zbus` (Tokio, proxy calls), `futures-core` (the signal stream), and `sha2` (a program's digest), with AT-SPI calls of the pack's own (decision 0198). On Windows only: the workspace's `windows` with the UI Automation, COM, management (WMI), security, threading, and window features, and `sha2` (decisions 0209 and 0318). On macOS only: the workspace's `objc2-application-services` (the accessibility interface), `objc2-core-foundation`, `libc` (the process table and the wait on an application's exit), and `sha2` (decision 0244). None of them compiles C. On every system, `quick-xml` (the readers of decisions 0316 and 0317) and `msft-typelib` 0.2.0 (a type library's bytes, decision 0318). Its stand is `targets/example-desk`, a test dependency only |
| Roadmap packages | Slice 8.1, the spike (done); then package 8.3: reads on three systems and writes on Linux are built (decisions 0198, 0209, 0218, 0244), and writes on Windows and macOS are written (decision 0399) |
| Carries out | Spec sections [201](../../spec/2-kernel/201-archetypes-and-modes.md) (`desktop`, interface), [204](../../spec/2-kernel/204-supervising-subsystems.md) (one request at a time, starting and attaching), [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (changing what is on a person's screen; attaching to a process), [305](../../spec/3-packs/305-the-desktop-pack.md), [106](../../spec/1-product/106-what-it-latches-onto.md) (the desktop row) |

## What is settled, and what is not

| Settled | Not settled |
| --- | --- |
| The mode `desktop`, archetype interface (decisions 0005, 0050) | How a desktop application's own sign-in is kept, and by which auth provider |
| Elements are found by role and name in the accessibility tree; never by position on a picture of the screen | Where a desktop run may happen on a server |
| The base on each system, the build order Linux, Windows, macOS, and `unsafe` confined to one module per system (decision 0120) | Profiles for toolkits, and what a support tier means here |
| A wait is woken by an event and decided by a fresh read; where a system has no event for a condition, the condition is refused by lint (decision 0120) | Whether the AT-SPI cache interface is needed for large windows |
| A field's name is the platform's accessible name: its own, else its label's (decision 0120) | How two windows of one program with no title are told apart beyond process and role |
| Test-application case 28 is `targets/example-desk`, a tree published over AT-SPI by the stand itself, until a real toolkit's adapter stands on maintained crates (decision 0198, which changes decision 0120); on Windows a window of plain Win32 controls and on macOS a window of AppKit's controls, each in a process of its own, through one harness (decisions 0209 and 0244) | Whether a browser-runtime application whose inspector is disabled may be served by `desktop` |
| An application built on a browser runtime is a web surface (spec sections 201 and 301) | |

## Owns

- The `desktop` driver, over a private back end per operating system.
- Finding the application the target names, already running in the engine's own session (built); later, starting one through the scoped supervisor, by the path and digest the published compiled target declares, or attaching to one a person named.
- Locating an element in a window's accessibility tree; acting on it through the interface's own patterns, such as setting a value or invoking, before any synthetic key.
- Its widget drivers, its snapshot builder, its discoverers, its drafting rules, and the `screens/` schema for surfaces of kind `desktop`.
- Proving, before every act, that the element is in the window and the process the step expects.

It must never: click or type by screen position; act in a window it cannot prove is the expected one; read a password field's value, known by its role or flag on each system, whatever its text holds; attach to a process a person did not name; act while a person is using the same desktop session, unless the target says it may; show the macOS permission prompt during a run, or switch the Linux session's accessibility setting.

## Sharing with the web pack without depending on it

A pack may not depend on another pack's crates, so this crate cannot use `a2m-pack-web-ui`'s widget drivers or `a2m-pack-web-browser`'s locating code. What the two share is held in three places, none of them code.

| Shared | Held by |
| --- | --- |
| The names of controls, locator kinds, conditions, steps, and the `screens/` fields | The [widget drivers of section 301](../../spec/3-packs/301-the-web-pack.md#widget-drivers), the [file formats](../../reference/file-formats.md#screensyaml), and the [steps reference](../../reference/steps-snapshots-and-diffs.md). Each pack's schema is checked against them by a test |
| The snapshot's text form | The steps reference. An agent reads a window and a page the same way |
| What a driver must do | The conformance suite's groups for locating, waiting, acting, and snapshots, run against case 28 as against the web cases |

The code is written twice, each over its own element handle. That is the price of the rule, and it is measured: if the second copy grows large, the archetype-level parts (the `screens/` skeleton, locator ordering, condition evaluation over an abstract tree) are a candidate to move into the kernel by decision record. That would be a finding for hypotheses H-26 and H-37, counted the way package D4 counts the command pack's.

**A column or a control named as a secret is never drafted as an output.** Beside the rule that a control the tree marks as holding a secret is never read, the drafter asks the compiler's own test (`names_a_secret` in `a2m-files`) of each table column's heading and each labelled control's name, as the window writes it and as the output name it becomes. One so named is left out of the screen and of the read's outputs, and the last sentence of the drafted description tells the names left out. A drafted read has no input ([decision 0302](../../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)).

## What it registers

Through the provider contract as built ([decision 0089](../../decisions/0089-the-provider-contract-as-building-it-settled-it.md)): a manifest embedded with `include_str!`, and `Pack::register` handing objects to the `Registrar`. What is not built is not declared, as in the web pack's manifest. The table is as built by decision 0198; the right column says what the full pack adds later.

| Registration | As built | Later |
| --- | --- | --- |
| Provider | `desktop`, kind `pack` | |
| Mode | `desktop`, archetype interface, verbs `read`, `list`, `update`, `upsert`; steps `goto`, `wait`, `read`, `set`, `invoke`, and `snapshot` (sent by the discoverer only), with no `locate` step, as [decision 0112](../../decisions/0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md) settled for `ui`; no kind always commits (an `invoke` commits as its action declares); `exclusive = "screen"`, `session_reads = 1` (decision 0218) | `create`, `delete`, `submit`, `upload` |
| Surface kind | `desktop`, archetype interface, `session = false`. Address key `program`: the application's absolute path. Fields `program`, `application` (the name the application gives itself on the accessibility bus), and `digest`. `SurfaceKindSpec::with_program("program", Some("digest"))`, so that the compiler refuses a path that is not absolute and a malformed digest; the driver starts nothing | |
| Effects | `attach = true` (it reads a running application the target names), `screen = true`, `network = "none"` | `compiled_target_programs` once it starts the program; `write_roots` with file dialogs |
| File kind | `screen`, directory `screens/`, `surface_kinds = ["desktop"]`. A locator is `role` and `name`, both exact, and nothing else: a `css` key is refused by the schema. `reach` is `window: {role, title}`; the title may hold one placeholder `{name}`, which the operation's required input of that name fills, and `rows` may leave out `locate`, its table then the one whose headings are `header` ([decision 0462](../../decisions/0462-a-desktop-table-of-cells-is-read-by-the-column-count-it-states-and-a-title-two-windows-show-to-differ-becomes-a-pattern.md)). `actions`: `locate`, `perform` (the element's action by name, `click` unless stated), `confirm`, `commits`; `variants`: `when` and `handling: stop`, the application's refusal, whose element's text is carried as `said` (decision 0218). A screen with an action that commits describes a write | `test_id` as the interface's automation identifier; a variant for a record that is not there |
| Lowering | `read` and `list` into `goto`, `wait`, `read`; `update` and `upsert` into `goto`, `wait`, one `set` per field, the `invoke` of the binding's `action` as the commit point, and a read-back `goto`, `wait`, `read` on the `read_back` screen. Each `wait` carries the record's key, which the window must show (decision 0218) | `create`, `submit` |
| Controls | `desktop_text` | `textarea`, `number`, `checkbox` first, then the rest, each when a test passes |
| Compile refusals | `desktop.role_unknown` (a role not in the pack's list), `desktop.unnamed` (an empty name or title), `desktop.condition_invalid`, `desktop.column_unknown`, `desktop.unknown_screen`, `desktop.unknown_field`, `desktop.writes_not_built` (a verb not built), `desktop.read_acts`, `desktop.action_missing`, `desktop.unknown_action`, `desktop.value_type`, `desktop.password_set` (a password field set), `desktop.unreadable_write` (a field written that is no output, unless `irreversible`: entry bar item 2), `desktop.key_unshown` (a key the screen does not show), `desktop.title_invalid` (two placeholders in a title, or one alone), `desktop.title_unbound` (a placeholder no required input fills), `desktop.title_write` (a write through a screen whose title has a placeholder) (decision 0462) | The lint rules `desktop.condition_unsupported` and `desktop.title_only`, when a system cannot signal a condition or a window has an identifier |
| Driver | `desktop` | |
| Discoverer | `desktop_snapshot`, with its describer: one `window` finding for each window of the application, and one for the windows of one role and shape whose titles differ in exactly one plain word, its title a pattern (decision 0462). A table's element states the column count its system states, and a control's whether it gives its value as text. `desktop_service` (Linux), with its describer: one `dbus_service` finding for the service the surface's `service` names, as it answers (`discovered`), and one for each `.service` file or interface description handed under `service_file` and `interface_file` (`inferred`); a service that is not running is a finding that says so, and nothing is started (decision 0316). `desktop_bundle`, with its describer: reads `Contents/Info.plist` and the dictionary it names from a bundle folder handed under `bundle`, as bytes, on every system, and refuses a dictionary that includes a file outside the bundle (decision 0317). `desktop_management` (Windows): the definition of the class the surface's `management_class` names, through the `schema` step. `desktop_com`: a type library handed under `type_library`, read as bytes by `msft-typelib`, never loaded by the system (decision 0318) | `desktop_probe` |
| Finding kind | `management_class`, for `desktop` surfaces: one class's definition (Windows). `com_type_library`, for `desktop` surfaces: a type library's interfaces and each member's declared way of being invoked (decision 0318). `application_bundle`, for `desktop` surfaces: one file of a macOS bundle, read: its property list's identity, URL schemes, document types, and services; or its scripting dictionary's suites, commands, events, classes, and each class property with the access the dictionary states and whether it states one (decision 0317). `dbus_service`, for `desktop` surfaces: a service's objects, interfaces, members, and each property with the access its document states. `window`, for `desktop` surfaces. Not `screen`, so that the web pack's drafting rules over `screen` findings never meet it | |
| Drafting rules | From a `management_class` finding of a singleton class: one `read` for each property the definition does not mark writable; from a `com_type_library` finding: one `read` for each property get of a dispatch interface that takes nothing; they compile and are not run, the steps `management_get` and `com_get` being a named stop in this version (decision 0318). From a running service's `dbus_service` finding: one `read` for each property whose document states `access="read"`, of a form the `property` step gives as text, on an interface that one object answers; never a property that states `readwrite` or `write`, a method, or a signal (decision 0316). From a `window` finding: a screen and a `get` read of the values the window labels (an entry, a text, or a spin button that gives its value as text), and a screen and a `list` read of each table it names and of the one it gives no name, by the column count it states when its cells are its own children; over a title with a placeholder, each read takes it as its required input and key; a finding drafted from nothing is listed with the reason (decision 0462); a name two controls share is an open question in a screen apart | |
| Settings | `max_nodes` (2000), `max_depth` (32), `max_read_chars` (65536) | |
| Platforms | None listed: the compiler side works on every system; the driver reads on Linux, Windows, and macOS, and on any other system stops at prepare as `busy`/`system_not_built` | |
| Auth providers | None. A desktop application's own sign-in lives inside the application ([section 215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md)) | |

## Interface

As built. The product crate makes one call; nothing else is public but the codes and the step and file kind names.

```rust
pub fn add_to(builder: &mut RegistryBuilder);            // the product's one call
pub fn pack() -> DesktopPack;                            // reads the session the engine runs in
impl DesktopPack { pub fn on_session_bus(address: &str) -> Self; }   // for a test with a session of its own
pub const DESKTOP_MODE: &str = "desktop";
pub const ADDRESS_KEY: &str = "program";
pub const SNAPSHOT_DISCOVERER: &str = "desktop_snapshot";
pub use lower::{GOTO, WAIT, READ, SET, INVOKE, SNAPSHOT};
pub use screens::{CONTROLS, DESKTOP_SURFACE, PACK, SCREEN_KIND};
pub mod codes;                                           // the pack's failure codes in one failure_codes! table (decision 0258), and PASSED_ON
```

Inside, one look-only back end per system, with the same methods; the driver and the discoverer reach the application only through it.

```rust
pub(crate) struct Look;   // linux.rs: AT-SPI; windows.rs: UI Automation; macos.rs: the accessibility interface; none.rs (any other system): every method is `system_not_built`
impl Look {
    async fn connect(session: Option<&str>) -> Result<Look, AxError>;
    async fn application(&self, program: &str, name: &str, digest: Option<&str>) -> Result<App, AxError>;
    async fn windows(&self, app: &App) -> Result<Vec<Window>, AxError>;
    async fn tree(&self, root: &El, limits: &Limits, deadline: Deadline) -> Result<Tree, AxError>;
    async fn text(&self, node: &Node, limits: &Limits) -> Result<String, AxError>;   // refuses a secret
    async fn watch(&self, app: &App) -> Result<Watch, AxError>;                      // the application's signals
    async fn close(self);
}
```

The type has no method that sets a value, invokes, selects, focuses, scrolls, or sends a key or a click, and the Linux module's reading function sends only the members of its list of reads, which a test holds against the members of AT-SPI that act. On Linux the acting half is apart (decision 0218): `Look::hands(&Proof)` gives a `Hands`, made only by the driver's `set` and `invoke`, with `prove` (the element is the application's, and the connection's process, start time, program, and digest are those proven at prepare), `settable`, `set_text` (`EditableText.SetTextContents`), and `perform` (`Action.DoAction` of the action named); it alone calls `act_call`, the one function that sends a member of the second list, `ACT_MEMBERS`, which tests hold apart from the first, against the members that act, and against the module's source. A `Watch` also hears the bus say the application's connection ended. A wait subscribes, reads the condition once, and reads it again on each signal from the application, until it holds or the deadline passes; it never decides from a signal's payload. The Windows and macOS back ends are the only modules with `#[allow(unsafe_code)]`, on each function, with a comment on each block. The Windows back end makes every UI Automation call from a multithreaded apartment on a thread each `Look` owns, where every COM object is made and released, through one function that makes only the members of its list of reads, which two tests hold (the list against the members that act, and the module's source against the list); a wait listens to the system's accessibility events for the application's process through an out-of-context hook on a thread of the watch's own, handed to Tokio through a channel (decision 0209). The macOS back end first asks the system whether this process may use the accessibility interface (`AXIsProcessTrusted`, which never prompts) and otherwise stops as `accessibility_off`, naming where in System Settings the person allows the program that started the engine; it proves the application from the process table (the program's path in the resolved form, the user, the start time, and the digest of the running program's file) before it asks it anything, and proves it again before each later question; every accessibility call is made on a thread each `Look` owns, through one function that makes only the calls of its list of reads, and every attribute is copied through one function that asks only the attributes of its list, which two tests hold; a wait listens through an observer of the application's notifications on a run loop of a thread of the watch's own, and through a kernel queue that reports the application's exit (decision 0244). Every act (`set`, `invoke`) calls the driver's `before_act` on the walk it acts on: the record the `wait` proved is proved again (each `set` and `invoke` step carries the key arguments of its `wait`), and a window of the application that no screen of the target names is a stop, nothing sent. It lives in the driver, above the back ends, so the Windows and macOS acting halves inherit it. On Windows and macOS the acting half is apart as on Linux ([decision 0399](../../decisions/0399-the-desktop-pack-writes-on-windows-and-macos-through-a-closed-list-of-its-own-on-each.md)): `Look::hands(&Proof)` gives a `Hands` with the same four methods, which run as jobs on the back end's own thread and prove there, before every act, the process (its number, start time, program, and stated digest) and on Windows the element's process; each calls `act`, the one function that makes a member of the second list, `ACT_MEMBERS` on Windows (the value pattern's `SetValue`, the invoke pattern's `Invoke`) and `ACT_CALLS` on macOS (`AXUIElementSetAttributeValue` of `AXValue`, `AXUIElementPerformAction` of `AXPress`), which tests hold against the members that act and against the module's source. There the action a screen names must be `click`, the one press each system has.

### How the mode fills the loop

| Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- |
| As built: the application the surface declares found already running in the engine's own session, never started; its process proven by the bus daemon's answer, the program it runs, the user that runs it, the digest when stated, and on Linux its start time, proved again before every act. Later: the program the compiled target declares started by path and digest | As built: `goto` finds the screen's window, `wait` its `ready` condition and the record's key, `read` fields and a table's columns; on Linux `set` sets a field's whole text through its editable-text interface and reads the field again, and `invoke` does the element's named action, both in a fresh walk, in a posture that may act, holding the driver's one desktop; no synthetic key or click (decision 0218). On Windows `set` goes through the value pattern and `invoke` through the invoke pattern; on macOS through `AXValue` and `AXPress` (decision 0399) | As built: inside the committing `invoke`, as for `ui`: the action's declared `confirm`, or a variant the screen declares, ended only by the application's events or the end of its connection (on macOS its process's exit; Windows hears none, and a walk that fails ends the wait) | As built: the `read_back` screen, reached afresh, read where the window shows what is stored, never the field typed into |

**Waits are events or nothing.** Which conditions each system can signal is in research note 19. On Linux the application's signals end every condition this version offers (`visible`, `text`, `all`). A condition with no event on a system will be refused there by `desktop.condition_unsupported`, which is not built because no condition needs it yet.

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | `desktop` |
| 2 | Read-back for every write | The element's value after the screen is reached again. A control that exposes no value cannot be written except as `irreversible` |
| 3 | Each identity strategy | As for `ui`: `natural` and `composite` through a screen reached by key, `stamped` and `find` through a list or search window, `none` as `irreversible` |
| 4 | A test stand | Test-application case 28, `targets/example-desk`: a tree published over AT-SPI with the test application's order and line items, edit fields apart from the values that show what is stored, and a control interface that counts every member it is asked and every press of Save, and reports what is stored, every store, and every value set into each field, with faults a test switches on (decisions 0198 and 0218); on Windows and macOS the same window of the system's own controls, with the same edit fields, store, and record of sets, and five of the faults (decision 0399) |
| 5 | A discoverer | `desktop_snapshot`, built; `desktop_probe`, not. They read the tree and never invoke, expand, or focus anything; what is hidden behind a collapsed node is a limitation, `not_exercised` |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Starts the declared program by path and digest; attaches to a process a person named; changes what is on a person's screen; reads and writes a staging folder for file dialogs. Hazards come from operations |
| 8 | A record per new mode | None: decision 0005 |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-engine`, through the registry | The `desktop` driver, only |
| `a2m-files`, through the gathered set | The `desktop` surface kind, the `screens/` schema for it, lowerings, lint and drafting rules, control names |
| A runner, inside the engine's sandbox | The discoverers, for discovery jobs that `a2m-actions` submits for `target.probe`, `screen.snapshot`, `discover.run` |
| `a2m-actions`, through the registry | The describers |

As built it starts nothing and uses no supervisor: it reads the process table to prove which program owns an application's connection. Later it uses its scoped supervisor to start an application, and to prove a process is the one it started or the one a person named.

## State

| State | Where |
| --- | --- |
| The connection to the session's accessibility bus, the proven application, the screens of the compiled target, and the window the last `goto` found, for one prepared binding | Memory, released with the binding |
| The application's own sign-in | Inside the application, in the runner's desktop session. Never copied |

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `drift` | `locator_not_found`, `fingerprint_mismatch`, `locator_ambiguous` | As for `ui`; `locator_ambiguous` also when two windows of the application have the screen's role and title, and `fingerprint_mismatch` when a table's headings or rows are not as its screen declares. Built |
| `unknown_variant` | `unexpected_page` | The screen's window did not appear, or did not become ready, by the step's deadline. Built |
| `unknown_variant` | `application_ambiguous`, `application_not_proven`, `read_too_large` | Several running applications are the surface's program and name; the program has another digest than the surface states; a value or a window larger than the pack reads. Built |
| `unknown_variant` | `unexpected_dialog` | A modal window |
| `policy` | `password_read_refused` | A step would read a control that holds a secret. Built |
| `busy` | `application_not_running` | No application of the session is the surface's program and name and publishes a tree; the pack starts nothing. Built |
| `busy` | `system_not_built` | The pack has no back end for this operating system: any but Linux, Windows, and macOS. Built |
| `input` | `step_not_usable` | A compiled target this version of the pack cannot carry out. Built |
| `rejected` | `app_validation_error` | A declared variant showing the application's refusal after the committing action, or before it, when it is not done. Its words are `said`. Built on Linux; written for Windows and macOS (decision 0399) |
| `possible_commit` | `no_confirmation` | An `invoke` of a committing action was sent and its confirmation did not arrive by the deadline, or the application exited, or the window could no longer be read. Built on Linux; written for Windows and macOS |
| `unknown_variant` | `field_read_only`, `unexpected_control`, `value_not_held` | A field takes no value today; an element offers no editable text or no action of the declared name (on Windows and macOS, any action but `click`); a field holds, after `set`, another value than the one set, before anything is saved. Built on Linux; written for Windows and macOS |
| `unknown_variant` | `application_not_proven` | Also when, before an act, the application's connection (on Windows and macOS its process) is no longer the proven process. Built on Linux; written for Windows and macOS |
| `policy` | `password_set_refused` | A step would set a control that holds a secret. Built |
| `busy` | `writes_not_built` | A `set` or `invoke` on a system with no back end, nothing sent. Built |
| `policy` | `remote_namespace_refused` | On Windows, the surface's `management_namespace` names another machine; the driver's `schema` step asks nothing (decision 0399) |
| `busy` | `session_in_use` | Another write held the driver's desktop past the step's deadline, nothing sent (built); later, a person is using the desktop and the target does not allow sharing it |
| `busy` | `runner_unavailable` | No interactive desktop session |
| `busy` | `accessibility_off` | The macOS Accessibility permission is not granted to the program making the calls, or the Linux session's accessibility switch is off and the application's program runs and publishes no tree. The message names the setting, and on macOS the program to allow (the one that started the engine, and the engine's own program); the pack changes nothing and never shows the system's prompt. Built for Linux; written for macOS (decision 0244) |
| `busy` | `target_elevated` | A Windows application runs elevated when the engine does not, or its token cannot be opened. Built for Windows (decision 0209) |
| `policy` | `application_not_owned` | A program the surface states runs as another user, or on Windows in another session. Built for Windows (decision 0209); written for macOS (decision 0244) |
| `transient` | `timeout`, `connection_lost` | Before anything was sent. Built |

**Reserved, not built.** `automation_not_granted` (class `busy`): a scripting route the person has not allowed, the sender and target named, with the setting to change (on macOS, System Settings, Privacy and Security, Automation). It stays out of the table of codes, and out of the manifest, until a route that produces it is built ([decision 0336](../../decisions/0336-the-desktop-pack-states-that-it-sends-to-no-address-and-checks-that-a-field-is-enabled-before-it-sets-it.md); research note 33).

**The statement.** `Lowering::statement` answers `Stated` with no program started, no variable set, and `Sends::Only { surface: false, addresses: [] }`; it answers `NotStated` where the surface's `management_namespace` begins with two separators, which names another machine, and the driver itself refuses to ask one, as `remote_namespace_refused` (fail closed, decision 0399). A compile test of the example target reads the review. A driver that dials any address withdraws the statement in the same change.

**Before a set.** The Linux back end's `settable` reads the element's interfaces and its state: editable text offered, enabled, editable, and not read-only. Not enabled and read-only both stop as `field_read_only` with nothing sent; the message tells them apart. On Windows `settable` reads the value pattern's presence, `CurrentIsEnabled`, and the pattern's `CurrentIsReadOnly`; on macOS the role (a text field that holds no secret), `AXEnabled`, and `AXUIElementIsAttributeSettable` of `AXValue` (decision 0399).

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Nothing a read does can change the application | The back end's type, which has no method that acts; a unit test that its list of members holds none that acts; and the stand's control interface, which counts no press and no member that acts during every read and snapshot. Built |
| Nothing but the declared application is asked anything | The stand, running, with a surface that names another program: it was asked nothing. Built |
| No act reaches a window other than the expected one | The stand opens a second window of the same program before an act: with the same title, the write stops as `locator_ambiguous` and nothing is set; with another, every set is in the screen's window. Built for Linux. On all three, the stand moves to another record or raises a dialog once the key is read, and nothing is set or pressed; written for Windows and macOS, unproven until CI runs them |
| Nothing a write reads back is the field it typed into | The stand's silent Save: the fields hold the new values, and the read-back reads the store, which does not. Built for Linux; written for Windows and macOS |
| Two writes never type into one desktop at once | Two concurrent writes through one driver: the stand's record of sets shows one whole write, then the other. Built for Linux; written for Windows and macOS |
| The acting half is reachable only through one function | Tests of the two closed lists and of the module's source. Built for Linux; written for Windows and macOS, whose tests run on CI's runners |
| No input is sent by screen position | Review, and the back-end trait, which has no such method |
| Nothing waits by polling | Review, and two tests: a condition no event ends stops at its deadline as `unexpected_page`; a value the stand shows a moment after the wait began ends it by its signal. Built for Linux; written for Windows and macOS, unproven until CI runs them |
| A password field is never read | A test against a password field on each system: the stand counts no read of it, and its value is in no result, finding, draft, or log. Built for Linux; written for Windows, where the stand counts every message that asks the password edit for its text, and for macOS, where the stand's secure text field counts each time AppKit's accessibility asks it for its value, unproven until CI runs them |
| A committing action lost to a crash is `possible_commit` | Case 28 with the application leaving the bus after Save stored, and with Save storing nothing and saying nothing: each `possible_commit`; the resume reads first and presses Save again only where nothing was stored. Built for Linux |
| Discovery changes nothing in the program | Case 28's control interface counts no write and no member that acts during `desktop_snapshot`, and no read of any text. Built |
| Each widget driver sets and reads back its control | The conformance group "Acting", on case 28, on each system whose back end is built |

## Build order

1. Done: slice 8.1, in `spikes/desktop-base/`. On Linux, against a GTK 3 window and an egui window: find the field by role and name, set it, read it back, invoke, and end each wait on an event, with the window's store as ground truth. Windows and macOS type-checked. Decision 0120.
2. Done: case 28, `targets/example-desk`: not the egui program, whose AT-SPI adapter stands on unmaintained crates, but a tree the stand publishes over AT-SPI itself, through the real bus launcher and registry, on a session bus the test starts; with the order, its line items, a control with no name, two controls of one name, a password field, a late value, a Save button, and a control interface. It needs no display. The Linux CI job is written for the lead to adopt (decision 0198).
3. Done: the compiler side, with no system code: the manifest, `DesktopPack`, the `desktop` surface kind with its program field, the `screens/` file kind, the `read` and `list` lowerings, the refusal of a write, the control `desktop_text`, and a test that the fixture compiles into the same compiled target twice. Type-checked for Windows; for macOS the check stops in `a2m-provider`, below this crate. The design's four lint rules are not built (above).
4. Done, but for starting a program and `desktop_probe`: the Linux back end, reads only: find the declared application already running, prove its process against the bus daemon's answer, find its window by an event-ended wait, `read`; `desktop_snapshot` and its drafting rules.
5. Done for Linux: writes with read-back, `update` and `upsert`, and the fault cases (Save that stores nothing, the application exiting after Save, a late confirmation, a read-only field, a value the field changes, a refusal, a second window, another record, a password field, two writes at once), with the stand learning to be written (decision 0218). On Windows and macOS, steps 6 and 7. Not built: the value interface, `create`, a variant for a record that is not there.
6. The Windows back end, steps 4 and 5 again, with a native-controls window beside case 28; CI on the hosted Windows machine. Done for step 4, reads, and type-checked only: `backend/windows.rs` and the stand's window of plain Win32 controls in `targets/example-desk` behind `cfg(windows)`, with the same tests as Linux through one harness, a back-end test against the stand that prints its walk, and the CI job written for the lead (decision 0209). Unproven until that job runs. Step 5, writes, written: the acting half through the value pattern and `Invoke`, with the stand's window learning to be written (decision 0399); unproven until CI runs it.
7. The macOS back end, steps 4 and 5 again. Done for step 4, reads, and type-checked only: `backend/macos.rs` and the stand's window of AppKit's controls in `targets/example-desk` behind `cfg(target_os = "macos")`, with the same tests as Linux and Windows through one harness, a back-end test against the stand that prints its walk, and the CI job written for the lead, which grants the permission on the runner as decision 0242 allows (decision 0244). Unproven until that job runs. Step 5, writes, written: the acting half through `AXValue` and `AXPress`, with the stand's window learning to be written (decision 0399); unproven until CI runs it.
8. Done for Linux: every act proves the record and the absence of a window no screen names on the walk it acts on, just before it acts; two faults of the stand (the window moves to another record, the application raises a dialog, each once the record's key is read) and a test that failed before (decision 0244).
9. Done for Linux, from field test 10's findings 57 to 60: a table whose cells are its own children read by the column count AT-SPI's `Table` interface states, and never without it; a table with no name located by its headings; a labelled spin button drafted; a reason for every finding not drafted; and a title two windows show to differ in one word drafted as a pattern with an input. The stand opens windows of one record in GTK 3's shape (decision 0462).

## Open points

- **Starting a program that speaks no protocol.** The scoped supervisor offers `run` (to completion) and `start_foreign` (a program on a pair of pipes). A desktop application is neither, and the proof of its process needs its process number and start time. The supervisor needs a link kind for a program it watches until stopped, or `start_foreign` must allow pipes the program ignores. For `a2m-provider`, by decision record, before the pack starts anything; reads find the application already running instead (decision 0198).
- How two windows of one program with no title are told apart, beyond process and role. The egui window has no name, and this version refuses a screen whose window has no title.
- **Role words across systems.** A locator's role is one of the pack's own words, mapped on Linux from AT-SPI's roles and on Windows from UI Automation's control types, in one table each (decision 0209: the list served Windows with two refinements, a password edit and a cell of a row). On macOS, a third table maps the interface's roles, with refinements for a secure text field, a dialog window, a cell of a row, and a table's heading row (decision 0244); unproven until a run shows the roles a window of AppKit gives.
- **A stand on a real toolkit.** The stand publishes its tree itself, so no toolkit's own tree is in the loop. It returns when egui's AT-SPI adapter stands on maintained crates, or as a second stand on a CI job that accepts the GTK library (decision 0198).
- **The macOS permission on a hosted CI machine**: decided by the owner (decision 0242, point 14): granted on the runner only, by writing its permission settings as an administrator; never by product code (decision 0244 writes the step).
- Whether the AT-SPI cache interface is needed: a walk costs one to three milliseconds a node in a debug build.
- Where a desktop run may happen: a user's own machine with an interactive session is the only case designed. A server has no desktop by default.
- The auth provider for a desktop application's own sign-in.
- Whether profiles per toolkit are needed, as platform profiles are for the web, and what a support tier means here. Programs on a Java runtime need a bridge of their own on Windows.
- A browser-runtime application that disables its inspector is reached by neither pack. Its accessibility tree is built when it sees an assistive client, so `desktop` could read it; the design refuses it until a decision says otherwise.
- How much of the code duplicated from the web pack should move into the kernel, once both exist.
