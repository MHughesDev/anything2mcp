# `a2m-pack-desktop`

The desktop pack: the `desktop` driver, which reaches a desktop application through the operating system's accessibility interface (UI Automation on Windows, the Accessibility interface on macOS, AT-SPI on Linux), and the discoverers and drafting rules for an observed window. Nothing is built yet: the crate is empty. What each back end stands on is settled by slice 8.1 ([research note 19](../../research/19-desktop-accessibility-base.md), [decision 0120](../../decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md)); this file says what it will register through the provider contract as built, and the steps that build it.

## Facts

| | |
| --- | --- |
| Side | Pack: desktop |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-expr`, `a2m-files` (registration), `a2m-auth` (`SessionLease`), `a2m-provider` (`Pack`, `Driver`, `Discoverer`, `Describer`, `SandboxPort`, `PackContext` with its scoped supervisor and checked paths, `Registrar`), `a2m-telemetry`. Outside, by operating system, as decision 0120 chose: on Linux `atspi-proxies`, `atspi-common`, `atspi-connection` without `p2p`, and `zbus` with Tokio; on Windows `windows` with the UI Automation features; on macOS `objc2-application-services` and `objc2-core-foundation`. None of them compiles C |
| Built in | Slice 8.1, the spike (done); then package 8.3, which also waits on D4: the command pack's count of kernel changes says whether the kernel is ready for another pack |
| Carries out | Spec sections [201](../../spec/2-kernel/201-archetypes-and-modes.md) (`desktop`, interface), [204](../../spec/2-kernel/204-supervising-subsystems.md) (one request at a time, starting and attaching), [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (changing what is on a person's screen; attaching to a process), [305](../../spec/3-packs/305-the-desktop-pack.md), [106](../../spec/1-product/106-what-it-latches-onto.md) (the desktop row) |

## What is settled, and what is not

| Settled | Not settled |
| --- | --- |
| The mode `desktop`, archetype interface (decisions 0005, 0050) | How a desktop application's own sign-in is kept, and by which auth provider |
| Elements are found by role and name in the accessibility tree; never by position on a picture of the screen | Where a desktop run may happen on a server |
| The base on each system, the build order Linux, Windows, macOS, and `unsafe` confined to one module per system (decision 0120) | Profiles for toolkits, and what a support tier means here |
| A wait is woken by an event and decided by a fresh read; where a system has no event for a condition, the condition is refused by lint (decision 0120) | Whether the AT-SPI cache interface is needed for large windows |
| A field's name is the platform's accessible name: its own, else its label's (decision 0120) | How two windows of one program with no title are told apart beyond process and role |
| Test-application case 28 is one Rust program in egui, with a native window per system added later (decision 0120) | Whether a browser-runtime application whose inspector is disabled may be served by `desktop` |
| An application built on a browser runtime is a web surface (spec sections 201 and 301) | |

## Owns

- The `desktop` driver, over a private back end per operating system.
- Starting an application through the scoped supervisor, by the path and digest the published plan declares, or attaching to one a person named.
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

## What it will register

Through the provider contract as built ([decision 0089](../../decisions/0089-the-provider-contract-as-building-it-settled-it.md)): a manifest embedded with `include_str!`, and `Pack::register` handing objects to the `Registrar`. What is not built is not declared, as in the web pack's manifest.

| Registration | Content |
| --- | --- |
| Provider | `desktop`, kind `pack` |
| Mode | `desktop`, archetype interface. Steps declared at first: `goto`, `wait`, `read`, `set`, `invoke`, with no `locate` step, as [decision 0112](../../decisions/0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md) settled for `ui`: each step finds its own element. `handle` and `upload` come with file dialogs. `exclusive = "screen"`, so that the scheduler serializes writes to one desktop; `session_writes = 1` |
| Surface kind | `desktop`, archetype interface, `session = false` until an auth provider exists. Address key `program`: the application's absolute path. Fields `program`, `digest`, and `window` (a window match: `role`, and optionally `title` or `automation_id`). `SurfaceKindSpec::programs` is one `ProgramField` with `path_key = "program"` and the digest key `digest`, so the compiler copies the program into `SurfacePlan::programs` ([decision 0124](../../decisions/0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md)) |
| Effects | `plan_programs = true` (it starts only the program the published plan declares, by digest); `processes = []`; `screen = true`; `attach = false` until attaching to a named process is built; `network = "none"`; `write_roots` only when file dialogs are built |
| File kind | `screen`, directory `screens/`, `surface_kinds = ["desktop"]`. Registration is keyed by file kind and surface kind, so the web pack's `screen` kind for `web` does not collide. The `css` locator is not accepted; `test_id` means the interface's automation identifier (AutomationId, AXIdentifier, `accessible_id`) |
| Lowerings | `read` and `list` first, then `create`, `update`, `upsert`, `submit` with read-back, into the five step kinds and the pack's opaque step payload |
| Controls | `text`, `textarea`, `number`, `checkbox` first; `select`, `combobox`, `radio`, `date`, `grid`, `tree`, `tabs`, `dialog` each when a test passes on the system that has it |
| Lint rules | `desktop.css` (error: a `css` locator), `desktop.unnamed` (error: a role locator with an empty name), `desktop.condition_unsupported` (error: a condition this system cannot signal), `desktop.title_only` (warning: a window matched by title alone). Dotted, as decision 0112 writes lint codes |
| Driver | `desktop` |
| Discoverers | `desktop_probe` (the program, its version and digest, the accessibility permission or switch, how many controls have no name, which sets a support tier) and `desktop_snapshot` (the window's tree as screen, field, and action findings), each with its describer in the snapshot text form |
| Finding kinds | `surface`, `screen`, `field`, `action` for `desktop` surfaces |
| Drafting rules | From a window's snapshot, a screen model with fields, roles, names, and locators; field types and every action left as open questions |
| Platforms | Only the systems whose back end is built: `linux-x86_64` and `linux-aarch64` first |
| Auth providers | None. A desktop application's own sign-in lives inside the application ([section 215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md)) |

## Interface

A sketch against the traits as built in `a2m-provider`; every async method returns `BoxFuture`.

```rust
pub struct DesktopPack;
impl Pack for DesktopPack {
    fn manifest(&self) -> &'static str;                      // include_str!("anything2mcp-provider.toml")
    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure>;
}

struct DesktopDriver { cx: PackContext, desk: Desktop }
impl Driver for DesktopDriver {
    fn prepare<'a>(&'a self, cx: &'a PrepareContext<'a>, lease: Option<&'a SessionLease>)
        -> BoxFuture<'a, Result<Prepared, StepFailure>>;     // start the declared program, or find the named process; find its window
    fn act<'a>(&'a self, p: &'a mut Prepared, step: &'a Step, input: &'a StepInput<'a>)
        -> BoxFuture<'a, Result<StepOutcome, StepFailure>>;
    fn health(&self, surface: &SurfaceRef) -> DriverHealth;  // from what registration found: permission, switch, platform
    fn release<'a>(&'a self, p: Prepared, poisoned: bool) -> BoxFuture<'a, Cleanup>;
}

/// One queue per desktop session: one request at a time, as section 204 says.
pub(crate) struct Desktop { /* applications started or attached, by surface; the back end */ }

/// One implementation per operating system. No method takes a screen position.
pub(crate) trait AxBackend: Send + Sync {
    fn access(&self) -> Access;                                         // granted, accessibility_off, or unsupported
    fn window<'a>(&'a self, process: ProcessProof, m: &'a WindowMatch, d: Deadline) -> BoxFuture<'a, Result<WindowRef, AxError>>;
    fn find<'a>(&'a self, root: &'a WindowRef, q: &'a Query) -> BoxFuture<'a, Result<AxElement, AxError>>;   // exactly one
    fn name<'a>(&'a self, el: &'a AxElement) -> BoxFuture<'a, Result<String, AxError>>;  // own name, else its label's
    fn read<'a>(&'a self, el: &'a AxElement, what: ReadWhat) -> BoxFuture<'a, Result<RawValue, AxError>>;   // refuses a password field
    fn set<'a>(&'a self, el: &'a AxElement, value: &'a str) -> BoxFuture<'a, Result<Dispatch, AxError>>;
    fn invoke<'a>(&'a self, el: &'a AxElement) -> BoxFuture<'a, Result<Dispatch, AxError>>;
    fn watch<'a>(&'a self, root: &'a WindowRef, on: &'a [EventKind]) -> BoxFuture<'a, Result<AxEvents, AxError>>;
    fn supports(&self, condition: ConditionKind) -> bool;               // feeds `desktop.condition_unsupported`
    fn tree<'a>(&'a self, root: &'a WindowRef, limits: &'a TreeLimits) -> BoxFuture<'a, Result<AxTree, AxError>>;
}
```

`AxEvents` is a stream of events concerning the window. A wait subscribes, reads the condition once, and then reads it again on each event about the element, until it holds or the deadline passes; it never decides from an event's payload. `Dispatch` says whether the act was sent, so that each failure carries `Sent`. The Windows and macOS back ends are the only modules with `#[allow(unsafe_code)]`, on each function, with a comment on each block. The macOS back end runs its observer's run loop on a thread it owns; the Windows back end makes its UI Automation calls from a multithreaded apartment on a thread it owns. Both hand events to Tokio through a channel.

What the kernel gives this crate: the same as `a2m-pack-web-ui` has of `Driver`, `Plan`, `Step`, and `SessionLease`, including the handle that refuses a committing step without an intent token, and fixes a doubtful committing step as `possible_commit`; checked paths for the staging folder of file dialogs; discovery as a job inside the engine's sandbox, given only what `SandboxPort` offers; and the scheduler serializing writes under `exclusive = "screen"`.

### How the mode fills the loop

| Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- |
| The program the plan declares started by path and digest, or a process a person named found; the process proven by identifier, start time, and digest; its window found by an event-ended wait | `read`, `set` through the widget driver, preferring the value pattern; `invoke` through the invoke pattern | Inside the committing `invoke`, as for `ui`: the action's declared `confirm`, or a variant the screen declares | The field again, after the screen is reached afresh |

**Waits are events or nothing.** Which conditions each system can signal is in research note 19. A condition with no event on a system is refused there by `desktop.condition_unsupported`.

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | `desktop` |
| 2 | Read-back for every write | The element's value after the screen is reached again. A control that exposes no value cannot be written except as `irreversible` |
| 3 | Each identity strategy | As for `ui`: `natural` and `composite` through a screen reached by key, `stamped` and `find` through a list or search window, `none` as `irreversible` |
| 4 | A test stand | Test-application case 28, with the same control interface as the web application, so ground truth comes from the program's store |
| 5 | A discoverer | `desktop_probe` and `desktop_snapshot`. They read the tree and never invoke, expand, or focus anything; what is hidden behind a collapsed node is a limitation, `not_exercised` |
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

It uses its scoped supervisor to start an application, and to prove a process is the one it started or the one a person named.

## State

| State | Where |
| --- | --- |
| Applications started or attached, and the desktop queue | Memory and the supervisor's listing |
| The application's own sign-in | Inside the application, in the runner's desktop session. Never copied |

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `drift` | `locator_not_found`, `fingerprint_mismatch`, `locator_ambiguous` | As for `ui` |
| `unknown_variant` | `unexpected_dialog`, `unexpected_page` | A modal window, or a different window than the screen declares |
| `rejected` | `app_validation_error` | A declared variant showing a validation message |
| `possible_commit` | `no_confirmation` | An `invoke` of a committing action was sent and its confirmation did not arrive, or the application exited |
| `busy` | `session_in_use` | A person is using the desktop and the target does not allow sharing it |
| `busy` | `runner_unavailable` | No interactive desktop session |
| `busy` | `accessibility_off` | The macOS Accessibility permission is not granted to the program making the calls, or the Linux session's accessibility switch is off and the application publishes no tree. The message names the setting; the pack changes nothing |
| `busy` | `target_elevated` | A Windows window runs at a higher integrity level than the runner |
| `transient` | `timeout`, `connection_lost` | Before anything was sent |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No act reaches a window other than the expected one | A test that opens a second window of the same program before an act |
| No input is sent by screen position | Review, and the back-end trait, which has no such method |
| Nothing waits by polling | Review, and a test whose condition no event ends: it fails at its deadline, having read the element once |
| A password field is never read | A test against a password field on each system |
| A committing action lost to a crash is `possible_commit` | Case 28 with the program killed after `invoke` |
| Discovery changes nothing in the program | Case 28's control interface counts no write and no state change during `desktop_snapshot` |
| Each widget driver sets and reads back its control | The conformance group "Acting", on case 28, on each system whose back end is built |

## Build order

1. Done: slice 8.1, in `spikes/desktop-base/`. On Linux, against a GTK 3 window and an egui window: find the field by role and name, set it, read it back, invoke, and end each wait on an event, with the window's store as ground truth. Windows and macOS type-checked. Decision 0120.
2. Case 28: `targets/example-desktop`, the egui program, with the test application's line-item data and its control interface; a Linux CI job that runs it under `Xvfb` with `dbus-run-session` and `at-spi2-core`, with the session's accessibility switch set inside that private session only.
3. The crate's compiler side, with no system code: the manifest, `DesktopPack`, the `desktop` surface kind with its program field, the `screens/` file kind for `desktop`, the `read` and `list` lowerings, the four lint rules, the control names, and a test that a registry with this pack builds with a stable digest. Checked on all three systems.
4. The Linux back end, reads only: start the declared program, prove its process against the bus daemon's answer, find its window by an event-ended wait, `read`; then `desktop_snapshot` and `desktop_probe`.
5. Linux writes with read-back, and the fault cases: the program killed after `invoke`, a second window, a password field, a condition no event ends.
6. The Windows back end, steps 4 and 5 again, with a native-controls window beside case 28; CI on the hosted Windows machine.
7. The macOS back end, steps 4 and 5 again, proven on a machine where a person grants the permission; the CI gap written in the journal.

## Open points

- **Starting a program that speaks no protocol.** The scoped supervisor offers `run` (to completion) and `start_foreign` (a program on a pair of pipes). A desktop application is neither, and the proof of its process needs its process number and start time. The supervisor needs a link kind for a program it watches until stopped, or `start_foreign` must allow pipes the program ignores. For `a2m-provider`, by decision record, before step 4.
- How two windows of one program with no title are told apart, beyond process and role. The egui window has no name.
- Whether the AT-SPI cache interface is needed: a walk costs one to three milliseconds a node in a debug build.
- Where a desktop run may happen: a user's own machine with an interactive session is the only case designed. A server has no desktop by default.
- The auth provider for a desktop application's own sign-in.
- Whether profiles per toolkit are needed, as platform profiles are for the web, and what a support tier means here. Programs on a Java runtime need a bridge of their own on Windows.
- A browser-runtime application that disables its inspector is reached by neither pack. Its accessibility tree is built when it sees an assistive client, so `desktop` could read it; the design refuses it until a decision says otherwise.
- How much of the code duplicated from the web pack should move into the kernel, once both exist.
