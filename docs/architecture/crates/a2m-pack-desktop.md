# `a2m-pack-desktop`

The desktop pack: the `desktop` driver, which reaches a desktop application through the operating system's accessibility interface (UI Automation on Windows, the Accessibility API on macOS, AT-SPI on Linux), and the discoverer and drafting rules for an observed window. It is the least designed of the first packs. This file says what is settled, what is assumed, and what must be learned before code is written.

## Facts

| | |
| --- | --- |
| Side | Pack: desktop |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-expr`, `a2m-files` (registration), `a2m-auth` (`SessionLease`), `a2m-provider` (`Driver`, `Discoverer`, `Describer`, `SandboxPort`, `PackContext` with its scoped supervisor and checked paths, registration), `a2m-telemetry`. Outside: one library over the three accessibility interfaces, or one crate per operating system; slice 8.1 chooses, with brief 19 ([research note 09](../../research/09-rust-building-blocks.md) lists the candidates, none yet verified by use) |
| Built in | Slice 8.1, the spike; package 8.3, the pack, which also waits on D4: the command pack's count of kernel changes says whether the kernel is ready for another pack |
| Carries out | Spec sections [6](../../spec/06-modes-and-drivers.md) (`desktop`), [54](../../spec/54-interaction-archetypes.md) (interface), [57](../../spec/57-supervising-subsystems.md) (one request at a time, starting and attaching), [61](../../spec/61-effects-hazards-and-consent.md) (changing what is on a person's screen; attaching to a process), [62](../../spec/62-catalog-of-surface-kinds.md) (the desktop row) |

## What is settled, and what is not

| Settled by an existing document | Not settled |
| --- | --- |
| The mode `desktop`, archetype interface (decisions 0005, 0050) | The accessibility library, or libraries |
| Elements are found by role and name in the accessibility tree; never by position on a picture of the screen ([section 62](../../spec/62-catalog-of-surface-kinds.md), refused kinds) | How a window is reached: started, attached, matched by title or by process |
| Its steps are those of `ui` except `observe` ([steps](../../reference/steps-snapshots-and-diffs.md#steps)) | Which conditions each operating system can signal as events |
| Its verbs are the `desktop` column of [verbs by mode](../../reference/technical-reference.md#verbs-by-mode) | How a desktop application's own sign-in is kept, and by which auth provider |
| Widget drivers share names with `ui` where the control exists in both | Profiles for toolkits, and what a support tier means here |
| Test-application case 28: a small native program with the same data, on three operating systems | Which toolkit the test program is written with, so that its accessibility data is good on all three |
| Phase 8 | Where a desktop run may happen on a server |

## Owns

- The `desktop` driver, over a private backend per operating system.
- Starting an application through the supervisor its context scopes to the programs the manifest and the published plan declare, or attaching to one a person named.
- Locating an element in a window's accessibility tree; acting on it through the interface's own patterns, such as setting a value or invoking, before any synthetic key or click.
- Its widget drivers, its snapshot builder, its discoverers, its drafting rules, and the `screens/` schema for surfaces of kind `desktop`.
- Proving, before every act, that the element is in the window and the process the step expects.

It must never: click or type by screen position; act in a window it cannot prove is the expected one; read a password field's value; attach to a process a person did not name; act while a person is using the same desktop session, unless the target says it may.

## Sharing with the web pack without depending on it

A pack may not depend on another pack's crates, so this crate cannot use `a2m-pack-web-ui`'s widget drivers or `a2m-pack-web-browser`'s locating code. What the two share is held in three places, none of them code.

| Shared | Held by |
| --- | --- |
| The names of controls, locator kinds, conditions, steps, and the `screens/` fields | The [technical reference](../../reference/technical-reference.md#widget-drivers), the [file formats](../../reference/file-formats.md#screensyaml), and the [steps reference](../../reference/steps-snapshots-and-diffs.md). Each pack's schema is checked against them by a test |
| The snapshot's text form | The steps reference. An agent reads a window and a page the same way |
| What a driver must do | The conformance suite's groups for locating, waiting, acting, and snapshots, run against case 28 as against the web cases |

The code is written twice, each over its own element handle. That is the price of the rule, and it is measured: if the second copy grows large, the archetype-level parts (the `screens/` skeleton, locator ordering, condition evaluation over an abstract tree) are a candidate to move into the kernel by decision record. That would be a finding for hypothesis H-26, and it should be counted the way package D4 counts the command pack's.

## What it registers

| Registration | Content |
| --- | --- |
| Modes | `desktop`, archetype interface |
| Surface kinds | `desktop` |
| Step kinds | `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, `upload` for `desktop`, checked names registered per mode |
| Driver | `DesktopDriver` |
| File kinds | Owns `screens/` for surfaces of kind `desktop`; registration is keyed by file kind and surface kind, so the web pack's registration for `web` surfaces does not collide. Compiled into this crate's opaque step payload. `reach` takes a window match (process, title, or an automation identifier) or `from` another screen with an action. The `css` locator is not accepted; `test_id` means the interface's automation identifier |
| Binding fields | `screen`, `verb`, `fields`, `action`, as for `ui` |
| Verb lowerings | `open`, `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `upload`, `submit`, into `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, `upload` |
| Control names | `text`, `textarea`, `number`, `select`, `combobox`, `checkbox`, `radio`, `date`, `grid`, `tree`, `tabs`, `dialog`, as checked names, where the operating system's interface exposes them; the rest are refused until a test passes. The widget drivers are internal to this crate |
| Lint rules | `desktop_css` (error: a `css` locator), `desktop_unnamed` (error: a role locator with an empty name), `desktop_condition_unsupported` (error: a condition this operating system cannot signal), `desktop_title_only` (warning: a window reached by title alone) |
| Drafting rules | As for the web: from a window's snapshot, a screen model with fields, roles, names, and locators; field types and every action left as open questions |
| Finding kinds | `surface`, `screen`, `field`, `action` for desktop subjects, checked names with this crate's `found` schemas |
| Discoverers | `desktop_probe`: the program, its version and digest, and how many controls have no name, which sets a support tier. `desktop_snapshot`: the window's tree as screen, field, and action findings |
| Describer | `desktop_describer`, in the snapshot text form |
| Auth providers | None yet. A desktop application's own sign-in lives inside the application ([section 44](../../spec/44-session-keeper.md)); a provider that checks it through a probe screen and asks a person to sign in is the likely shape, and is not designed |

## Interface

A sketch, with less certainty than the other packs'.

```rust
pub fn register(reg: &mut Registration, ctx: &PackContext) -> Result<(), Failure>;

pub struct DesktopDriver { desk: Desktop }
impl Driver for DesktopDriver {                                    // mode and capabilities are in the manifest
    async fn prepare(&self, ctx: &RunContext, lease: &SessionLease) -> Result<Prepared, Failure>;
    async fn act(&self, p: &mut Prepared, step: &Step, input: &Values) -> Result<ActOutcome, Failure>;
    async fn health(&self, surface: &Surface) -> Health;
    async fn release(&self, p: Prepared);
}

/// Inside the crate. One queue per desktop session: one request at a time, as section 57 says.
pub(crate) struct Desktop { /* apps started or attached, by surface; the queue */ }

/// Inside the crate. One implementation per operating system, or one over a library that covers all three.
pub(crate) trait AxBackend: Send + Sync {
    async fn window(&self, app: &AppRef, m: &WindowMatch) -> Result<WindowRef, AxError>;
    async fn find(&self, root: &WindowRef, q: &Query) -> Result<AxElement, AxError>;   // exactly one, or an error
    async fn read(&self, el: &AxElement, what: ReadWhat) -> Result<RawValue, AxError>;
    async fn set(&self, el: &AxElement, how: &Input) -> Result<Dispatch, AxError>;
    async fn invoke(&self, el: &AxElement) -> Result<Dispatch, AxError>;
    fn events(&self, root: &WindowRef) -> AxEvents;               // focus, structure, value, window opened
    async fn tree(&self, root: &WindowRef, limits: &TreeLimits) -> Result<AxTree, AxError>;
}
```

What the kernel gives this crate: the same as `a2m-pack-web-ui` has of `Driver`, `Plan`, `Step`, and `SessionLease`, including the handle that refuses a committing step without an intent token and failures that say whether an act was not sent, was answered, or is unknown; checked paths for the staging folder of file dialogs; discovery as a job inside the engine's sandbox, given only what `SandboxPort` offers; and that the scheduler serializes work that changes what is on a person's screen, as [section 61](../../spec/61-effects-hazards-and-consent.md) says, so that two runs never type into one desktop at once.

### How the mode fills the loop

| Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- |
| The application started by path and digest, or attached as a person named it; its window found and its process proven by identifier, start time, and program digest | `locate`, then `set` through the widget driver, preferring the interface's value pattern; `invoke` through its invoke pattern | The action's declared `confirm`: a window, an element, a value | The field again, after the screen is reached afresh |

**Waits are events or nothing.** Each condition is ended by an accessibility event. Where an operating system cannot signal a condition, the condition is refused for that system by the lint rule above, not polled. Whether enough conditions remain is a question for the spike.

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | `DesktopDriver` |
| 2 | Read-back for every write | The element's value after the screen is reached again. A control that exposes no value cannot be written except as `irreversible` |
| 3 | Each identity strategy | As for `ui`: `natural` and `composite` through a screen reached by key, `stamped` and `find` through a list or search window, `none` as `irreversible` |
| 4 | A test stand | Test-application case 28, with the same control interface as the web application, so ground truth comes from the program's store |
| 5 | A discoverer | `desktop_probe` and `desktop_snapshot`. They read the tree and never invoke, expand, or focus anything; what is hidden behind a collapsed node is a limitation, `not_exercised` |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Starts a process by path and digest; attaches to a process a person named; changes what is on a person's screen; reads and writes a staging folder for file dialogs. Hazards come from operations |
| 8 | A record per new mode | None: decision 0005 |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-engine`, through the registry | `DesktopDriver`, only |
| `a2m-files`, through the gathered set | The `screens/` schema for desktop surfaces, lowerings, lint and drafting rules |
| A runner, inside the engine's sandbox | The discoverers, for discovery jobs that `a2m-actions` submits for `target.probe`, `screen.snapshot`, `discover.run` |
| `a2m-actions`, through the registry | The describer |

It uses its scoped supervisor to start an application and to prove a process is the one it started or the one a person named.

## State

| State | Where |
| --- | --- |
| Applications started or attached, and the desktop queue | Memory and the supervisor's listing |
| The application's own sign-in | Inside the application, in the runner's desktop session. Never copied |

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `drift` | `locator_not_found`, `fingerprint_mismatch`, `locator_ambiguous` | As for `ui` |
| `unknown_variant` | `unexpected_dialog`, `unexpected_page` | A modal window or a different window than the screen declares |
| `rejected` | `app_validation_error` | A declared variant showing a validation message |
| `possible_commit` | `no_confirmation` | An `invoke` of a committing action was sent and its confirmation did not arrive, or the application exited |
| `busy` | `session_in_use` | A person is using the desktop and the target does not allow sharing it |
| `busy` | `runner_unavailable` | No interactive desktop session, or the operating system's accessibility permission is not granted |
| `transient` | `timeout`, `connection_lost` | Before anything was sent |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No act reaches a window other than the expected one | A test that opens a second window of the same program with the same title before an act |
| No input is sent by screen position | Review, and the backend trait, which has no such method |
| A committing action lost to a crash is `possible_commit` | Case 28 with the program killed after `invoke` |
| Discovery changes nothing in the program | Case 28's control interface counts no write and no state change during `desktop_snapshot` |
| Each widget driver sets and reads back its control | The conformance group "Acting", on case 28, on three operating systems |

## Build order

1. Slice 8.1, in `spikes/`: on each operating system, read a simple program's tree, find a text field by name, set it, read it back. A decision record on the library.
2. Case 28, with its control interface.
3. Package 8.3: `DesktopDriver` reads, with `desktop_snapshot`; then writes with read-back; then the remaining widget drivers one test at a time.

## Open points

- **Applications built on a browser runtime.** Spec section 6 says the `desktop` mode reaches them "through the browser engine". The browser engine is the web pack's, so in this arrangement such an application is a web surface whose browser the supervisor starts, served by `ui`. The section needs correcting.
- The library or libraries, and whether one interface over three systems hides differences a driver needs to see.
- How a window is named in `reach` so that two windows of the same program are told apart.
- Where a desktop run may happen: a user's own machine with an interactive session is the only case designed. A server has no desktop by default.
- The auth provider for a desktop application's own sign-in.
- Whether profiles per toolkit are needed, as platform profiles are for the web.
- How much of the code duplicated from the web pack should move into the kernel, once both exist.
