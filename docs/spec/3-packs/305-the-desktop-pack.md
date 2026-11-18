# 305. The desktop pack

The desktop pack lets a target be a desktop application: a program with windows that a person uses on their own machine. It reaches the application through the operating system's accessibility interface (UI Automation on Windows, the Accessibility API on macOS, AT-SPI on Linux), finds elements by role and name, and acts on them through the interface's own patterns. It brings one mode, `desktop`, in the interface archetype.

**It is the least designed of the first four packs.** Much of what the web pack settles by measurement is, for the desktop, still a question for a spike. This section says what is settled, what is assumed, and what must be learned before code is written. It is built in Phase 8, after the command pack has shown whether the kernel is ready for another pack.

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
| A desktop application on the machine the engine runs on, in an interactive desktop session | Started through the supervisor the pack's context scopes to the programs the manifest and the published plan declare, by path and digest; or attached to one a person named |

| Refused | Why |
| --- | --- |
| Clicking or typing by screen position | Nothing improvises. The backend has no such method |
| Acting in a window it cannot prove is the expected one | Rule seven: unknown means stop |
| Reading a password field's value | Rule four |
| Attaching to a process a person did not name | Consent ([section 213](../2-kernel/213-effects-hazards-and-consent.md)) |
| Acting while a person is using the same desktop session, unless the target says it may | A run and a person typing into one desktop cannot be told apart. It is `busy`, `session_in_use` |
| An application built on a browser runtime | It is a web surface, served by `ui` ([section 301](301-the-web-pack.md)). The browser engine is the web pack's, and this pack cannot depend on it |

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

Both read the tree and never invoke, expand, or focus anything. What is hidden behind a collapsed node is a limitation, `not_exercised`. They run as discovery jobs inside the engine's sandbox ([section 218](../2-kernel/218-discovery-and-findings.md)), for `target.probe`, `screen.snapshot`, and `discover.run`.

The drafting rules are as for the web: from a window's snapshot, a screen model with fields, roles, names, and locators; field types and every action left as open questions.

## Lint rules

| Code | Level | Rule |
| --- | --- | --- |
| `desktop.css` | Error | A `css` locator. As built, the schema of `screens/` has no `css` key, so none compiles |
| `desktop.unnamed` | Error | A role locator with an empty name. As built, a refusal of the file kind |
| `desktop.condition_unsupported` | Error | A condition this operating system cannot signal. Not built: on Linux every condition offered is signalled |
| `desktop.title_only` | Warning | A window reached by title alone. Not built: a window has no other match yet |

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
| `unknown_variant` | `unexpected_dialog`, `unexpected_page` | A modal window, or a different window than the screen declares |
| `rejected` | `app_validation_error` | A declared variant showing a validation message |
| `possible_commit` | `no_confirmation` | An `invoke` of a committing action was sent and its confirmation did not arrive, or the application exited |
| `busy` | `session_in_use` | A person is using the desktop and the target does not allow sharing it |
| `busy` | `runner_unavailable` | No interactive desktop session, or the operating system's accessibility permission is not granted |
| `busy` | `application_not_running`, `accessibility_off`, `system_not_built` | The application the surface names is not running; it runs and publishes nothing while the session's accessibility switch is off; the pack has no back end for this system (decision 0198) |
| `policy` | `password_read_refused` | A step would read a control that holds a secret |
| `transient` | `timeout`, `connection_lost` | Before anything was sent |

## The test stand

Test-application case 28: a small native program with the same data as the test application, on three operating systems, with the same control interface, so ground truth comes from the program's store ([test application plan](../../plan/test-application.md)).

| Must always hold | Proven by |
| --- | --- |
| No act reaches a window other than the expected one | A second window of the same program, with the same title, opened before an act |
| No input is sent by screen position | Review, and the backend, which has no such method |
| A committing action lost to a crash is `possible_commit` | Case 28 with the program killed after `invoke` |
| Discovery changes nothing in the program | The control interface counts no write and no change of state during `desktop_snapshot` |
| Each widget driver sets and reads back its control | The conformance group "Acting", on case 28, on three operating systems |

## The entry bar

The eight items of [section 202](../2-kernel/202-kernel-and-packs.md).

| # | Item | The desktop pack's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | The `desktop` driver |
| 2 | Read-back for every write | The element's value after the screen is reached again. A control that exposes no value cannot be written except as `irreversible` |
| 3 | Each identity strategy | As for `ui`: `natural` and `composite` through a screen reached by key; `stamped` and `find` through a list or search window; `none` as `irreversible` |
| 4 | A test stand | Case 28 |
| 5 | A discoverer | `desktop_probe` and `desktop_snapshot` |
| 6 | Failures mapped | The table above |
| 7 | Effects and hazards | The table above; hazards come from operations |
| 8 | A record per new mode | None needed: decision 0005 |

## Build order

1. Slice 8.1, in `spikes/`, with brief 19: on each operating system, read a simple program's tree, find a text field by name, set it, and read it back. A decision record on the library ([research note 09](../../research/09-rust-building-blocks.md) lists candidates, none verified by use).
2. Case 28, with its control interface.
3. Package 8.3: reads, with `desktop_snapshot` (built on Linux, decision 0198); then writes with read-back; then the remaining widget drivers one test at a time. It also waits on package D4: the command pack's count of kernel changes says whether the kernel is ready for another pack.

## Open points

- The library or libraries, and whether one interface over three systems hides differences a driver needs to see.
- How a window is named in `reach` so that two windows of the same program are told apart.
- Which conditions each operating system can signal as events, and whether enough remain.
- Where a desktop run may happen. A user's own machine with an interactive session is the only case designed; a server has no desktop by default.
- The auth provider for a desktop application's own sign-in.
- Whether profiles per toolkit are needed, as platform profiles are for the web, and what a support tier means here.
- How much of the code duplicated from the web pack should move into the kernel, once both exist.
- How a program built on a browser runtime is started so the web pack can drive it ([section 301](301-the-web-pack.md)).
