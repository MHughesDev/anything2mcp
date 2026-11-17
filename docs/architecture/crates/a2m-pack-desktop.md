# `a2m-pack-desktop`

The desktop pack: the `desktop` driver, which reaches a desktop application through the operating system's accessibility interface (UI Automation on Windows, the Accessibility interface on macOS, AT-SPI on Linux), and the discoverers and drafting rules for an observed window. Reads on Linux are built ([decision 0198](../../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)); writes, Windows, and macOS are not. What each back end stands on was settled by slice 8.1 ([research note 19](../../research/19-desktop-accessibility-base.md), [decision 0120](../../decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md)) and, for Linux, changed by decision 0198 after [research note 26](../../research/26-desktop-pack-before-it-is-built.md); this file says what it registers through the provider contract as built, and the steps that build it.

## Facts

| | |
| --- | --- |
| Side | Pack: desktop |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-files` (registration), `a2m-auth` (`SessionLease`), `a2m-provider` (`Pack`, `Driver`, `Discoverer`, `Describer`, `SandboxPort`, `PackContext`, `Registrar`), `a2m-telemetry` (the clock of findings). Outside, on Linux only: the workspace's `zbus` (Tokio, proxy calls), `futures-core` (the signal stream), and `sha2` (a program's digest), with AT-SPI calls of the pack's own (decision 0198). When built: on Windows `windows` with the UI Automation features; on macOS `objc2-application-services` and `objc2-core-foundation` (decision 0120). None of them compiles C. Its stand is `targets/example-desk`, a test dependency only |
| Built in | Slice 8.1, the spike (done); then package 8.3: its first slice, reads on Linux, is built (decision 0198) |
| Carries out | Spec sections [201](../../spec/2-kernel/201-archetypes-and-modes.md) (`desktop`, interface), [204](../../spec/2-kernel/204-supervising-subsystems.md) (one request at a time, starting and attaching), [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (changing what is on a person's screen; attaching to a process), [305](../../spec/3-packs/305-the-desktop-pack.md), [106](../../spec/1-product/106-what-it-latches-onto.md) (the desktop row) |

## What is settled, and what is not

| Settled | Not settled |
| --- | --- |
| The mode `desktop`, archetype interface (decisions 0005, 0050) | How a desktop application's own sign-in is kept, and by which auth provider |
| Elements are found by role and name in the accessibility tree; never by position on a picture of the screen | Where a desktop run may happen on a server |
| The base on each system, the build order Linux, Windows, macOS, and `unsafe` confined to one module per system (decision 0120) | Profiles for toolkits, and what a support tier means here |
| A wait is woken by an event and decided by a fresh read; where a system has no event for a condition, the condition is refused by lint (decision 0120) | Whether the AT-SPI cache interface is needed for large windows |
| A field's name is the platform's accessible name: its own, else its label's (decision 0120) | How two windows of one program with no title are told apart beyond process and role |
| Test-application case 28 is `targets/example-desk`, a tree published over AT-SPI by the stand itself, until a real toolkit's adapter stands on maintained crates (decision 0198, which changes decision 0120) | Whether a browser-runtime application whose inspector is disabled may be served by `desktop` |
| An application built on a browser runtime is a web surface (spec sections 201 and 301) | |

## Owns

- The `desktop` driver, over a private back end per operating system.
- Finding the application the target names, already running in the engine's own session (built); later, starting one through the scoped supervisor, by the path and digest the published plan declares, or attaching to one a person named.
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

## What it registers

Through the provider contract as built ([decision 0089](../../decisions/0089-the-provider-contract-as-building-it-settled-it.md)): a manifest embedded with `include_str!`, and `Pack::register` handing objects to the `Registrar`. What is not built is not declared, as in the web pack's manifest. The table is as built by decision 0198; the right column says what the full pack adds later.

| Registration | As built | Later |
| --- | --- | --- |
| Provider | `desktop`, kind `pack` | |
| Mode | `desktop`, archetype interface, verbs `read` and `list`, steps `goto`, `wait`, `read`, and `snapshot` (sent by the discoverer only), no commit step, `exclusive = "none"`, `session_reads = 1` | The write verbs; `set` and `invoke`, with no `locate` step, as [decision 0112](../../decisions/0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md) settled for `ui`; `exclusive = "screen"` for writes |
| Surface kind | `desktop`, archetype interface, `session = false`. Address key `program`: the application's absolute path. Fields `program`, `application` (the name the application gives itself on the accessibility bus), and `digest`. `SurfaceKindSpec::with_program("program", Some("digest"))`, so that the compiler refuses a path that is not absolute and a malformed digest; the driver starts nothing | |
| Effects | `attach = true` (it reads a running application the target names), `screen = true`, `network = "none"` | `plan_programs` once it starts the program; `write_roots` with file dialogs |
| File kind | `screen`, directory `screens/`, `surface_kinds = ["desktop"]`. A locator is `role` and `name`, both exact, and nothing else: a `css` key is refused by the schema. `reach` is `window: {role, title}` | `test_id` as the interface's automation identifier; `actions`; `variants` |
| Lowering | `read` and `list` into `goto`, `wait`, `read`; a binding on an operation that writes is `desktop.writes_not_built` | `create`, `update`, `upsert`, `submit` with read-back |
| Controls | `text` | `textarea`, `number`, `checkbox` first, then the rest, each when a test passes |
| Compile refusals | `desktop.role_unknown` (a role not in the pack's list), `desktop.unnamed` (an empty name or title), `desktop.condition_invalid`, `desktop.column_unknown`, `desktop.unknown_screen`, `desktop.unknown_field`, `desktop.writes_not_built` | The lint rules `desktop.condition_unsupported` and `desktop.title_only`, when a system cannot signal a condition or a window has an identifier |
| Driver | `desktop` | |
| Discoverer | `desktop_snapshot`, with its describer: one `window` finding for each window of the application | `desktop_probe` |
| Finding kind | `window`, for `desktop` surfaces. Not `screen`, so that the web pack's drafting rules over `screen` findings never meet it | |
| Drafting rules | From a `window` finding: a screen and a `get` read of the values the window labels, and a screen and a `list` read of each table it names; a name two controls share is an open question in a screen apart | |
| Settings | `max_nodes` (2000), `max_depth` (32), `max_read_chars` (65536) | |
| Platforms | None listed: the compiler side works on every system, and on Windows and macOS the driver stops at prepare as `busy`/`system_not_built` | |
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
pub use lower::{GOTO, WAIT, READ, SNAPSHOT};
pub use screens::{CONTROLS, DESKTOP_SURFACE, PACK, SCREEN_KIND};
pub mod codes;                                           // every failure code, with `ALL`
```

Inside, one look-only back end per system, with the same methods; the driver and the discoverer reach the application only through it.

```rust
pub(crate) struct Look;   // linux.rs: AT-SPI; none.rs: every method is `system_not_built`
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

The type has no method that sets a value, invokes, selects, focuses, scrolls, or sends a key or a click, and the Linux module's one sending function sends only the members of its list of reads, which a test holds against the members of AT-SPI that act. A wait subscribes, reads the condition once, and reads it again on each signal from the application, until it holds or the deadline passes; it never decides from a signal's payload. The Windows and macOS back ends, when built, are the only modules with `#[allow(unsafe_code)]`, on each function, with a comment on each block; the macOS back end runs its observer's run loop on a thread it owns, and the Windows back end makes its UI Automation calls from a multithreaded apartment on a thread it owns, both handing events to Tokio through a channel (decision 0120).

### How the mode fills the loop

| Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- |
| As built: the application the surface declares found already running in the engine's own session, never started; its process proven by the bus daemon's answer, the program it runs, the user that runs it, and the digest when stated. Later: the program the plan declares started by path and digest, and the start time in the proof | As built: `goto` finds the screen's window, `wait` its `ready` condition, `read` fields and a table's columns. Later: `set` through the widget driver, preferring the value pattern; `invoke` through the invoke pattern; no synthetic key or click until a test shows one can be bound to a window (decision 0198) | Later: inside the committing `invoke`, as for `ui`: the action's declared `confirm`, or a variant the screen declares | Later: the field again, after the screen is reached afresh |

**Waits are events or nothing.** Which conditions each system can signal is in research note 19. On Linux the application's signals end every condition this version offers (`visible`, `text`, `all`). A condition with no event on a system will be refused there by `desktop.condition_unsupported`, which is not built because no condition needs it yet.

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | `desktop` |
| 2 | Read-back for every write | The element's value after the screen is reached again. A control that exposes no value cannot be written except as `irreversible` |
| 3 | Each identity strategy | As for `ui`: `natural` and `composite` through a screen reached by key, `stamped` and `find` through a list or search window, `none` as `irreversible` |
| 4 | A test stand | Test-application case 28, `targets/example-desk`: a tree published over AT-SPI with the test application's order and line items, and a control interface that counts every member it is asked and every press of Save (decision 0198) |
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
| The connection to the session's accessibility bus, the proven application, the screens of the plan, and the window the last `goto` found, for one prepared binding | Memory, released with the binding |
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
| `busy` | `system_not_built` | The pack has no back end for this operating system: Windows and macOS in this version. Built |
| `input` | `step_not_usable` | A plan this version of the pack cannot carry out. Built |
| `rejected` | `app_validation_error` | A declared variant showing a validation message |
| `possible_commit` | `no_confirmation` | An `invoke` of a committing action was sent and its confirmation did not arrive, or the application exited |
| `busy` | `session_in_use` | A person is using the desktop and the target does not allow sharing it |
| `busy` | `runner_unavailable` | No interactive desktop session |
| `busy` | `accessibility_off` | The macOS Accessibility permission is not granted to the program making the calls, or the Linux session's accessibility switch is off and the application's program runs and publishes no tree. The message names the setting; the pack changes nothing. Built for Linux |
| `busy` | `target_elevated` | A Windows window runs at a higher integrity level than the runner |
| `transient` | `timeout`, `connection_lost` | Before anything was sent. Built |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Nothing a read does can change the application | The back end's type, which has no method that acts; a unit test that its list of members holds none that acts; and the stand's control interface, which counts no press and no member that acts during every read and snapshot. Built |
| Nothing but the declared application is asked anything | The stand, running, with a surface that names another program: it was asked nothing. Built |
| No act reaches a window other than the expected one | A test that opens a second window of the same program before an act |
| No input is sent by screen position | Review, and the back-end trait, which has no such method |
| Nothing waits by polling | Review, and two tests: a condition no event ends stops at its deadline as `unexpected_page`; a value the stand shows a moment after the wait began ends it by its signal. Built for Linux |
| A password field is never read | A test against a password field on each system: the stand counts no read of it, and its value is in no result, finding, draft, or log. Built for Linux |
| A committing action lost to a crash is `possible_commit` | Case 28 with the program killed after `invoke` |
| Discovery changes nothing in the program | Case 28's control interface counts no write and no member that acts during `desktop_snapshot`, and no read of any text. Built |
| Each widget driver sets and reads back its control | The conformance group "Acting", on case 28, on each system whose back end is built |

## Build order

1. Done: slice 8.1, in `spikes/desktop-base/`. On Linux, against a GTK 3 window and an egui window: find the field by role and name, set it, read it back, invoke, and end each wait on an event, with the window's store as ground truth. Windows and macOS type-checked. Decision 0120.
2. Done: case 28, `targets/example-desk`: not the egui program, whose AT-SPI adapter stands on unmaintained crates, but a tree the stand publishes over AT-SPI itself, through the real bus launcher and registry, on a session bus the test starts; with the order, its line items, a control with no name, two controls of one name, a password field, a late value, a Save button, and a control interface. It needs no display. The Linux CI job is written for the lead to adopt (decision 0198).
3. Done: the compiler side, with no system code: the manifest, `DesktopPack`, the `desktop` surface kind with its program field, the `screens/` file kind, the `read` and `list` lowerings, the refusal of a write, the control `text`, and a test that the fixture compiles into the same plan twice. Type-checked for Windows; for macOS the check stops in `a2m-provider`, below this crate. The design's four lint rules are not built (above).
4. Done, but for starting a program and `desktop_probe`: the Linux back end, reads only: find the declared application already running, prove its process against the bus daemon's answer, find its window by an event-ended wait, `read`; `desktop_snapshot` and its drafting rules.
5. Linux writes with read-back, and the fault cases: the program killed after `invoke`, a second window, a password field, a condition no event ends.
6. The Windows back end, steps 4 and 5 again, with a native-controls window beside case 28; CI on the hosted Windows machine.
7. The macOS back end, steps 4 and 5 again, proven on a machine where a person grants the permission; the CI gap written in the journal.

## Open points

- **Starting a program that speaks no protocol.** The scoped supervisor offers `run` (to completion) and `start_foreign` (a program on a pair of pipes). A desktop application is neither, and the proof of its process needs its process number and start time. The supervisor needs a link kind for a program it watches until stopped, or `start_foreign` must allow pipes the program ignores. For `a2m-provider`, by decision record, before the pack starts anything; reads find the application already running instead (decision 0198).
- How two windows of one program with no title are told apart, beyond process and role. The egui window has no name, and this version refuses a screen whose window has no title.
- **Role words across systems.** A locator's role is one of the pack's own words, mapped on Linux from AT-SPI's roles. Whether one list serves UI Automation's control types and macOS's roles is for the Windows and macOS back ends.
- **A stand on a real toolkit.** The stand publishes its tree itself, so no toolkit's own tree is in the loop. It returns when egui's AT-SPI adapter stands on maintained crates, or as a second stand on a CI job that accepts the GTK library (decision 0198).
- **The macOS permission on a hosted CI machine** (research note 26): not decided; the owner's.
- Whether the AT-SPI cache interface is needed: a walk costs one to three milliseconds a node in a debug build.
- Where a desktop run may happen: a user's own machine with an interactive session is the only case designed. A server has no desktop by default.
- The auth provider for a desktop application's own sign-in.
- Whether profiles per toolkit are needed, as platform profiles are for the web, and what a support tier means here. Programs on a Java runtime need a bridge of their own on Windows.
- A browser-runtime application that disables its inspector is reached by neither pack. Its accessibility tree is built when it sees an assistive client, so `desktop` could read it; the design refuses it until a decision says otherwise.
- How much of the code duplicated from the web pack should move into the kernel, once both exist.
