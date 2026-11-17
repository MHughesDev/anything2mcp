# 201. Archetypes and modes

There are many kinds of system and few ways to reach one. This section names the seven archetypes, the ways a system can be reached, and shows that each fills the engine loop of [section 207](207-the-engine-loop.md) in the same way. It then says what a mode is, which modes the packs bring, how the runtime chooses among an operation's bindings at call time, and how a new binding earns its place.

A mode is one concrete way of reaching a system, such as `ui`, `http`, or `mcp`. It is brought by a pack ([section 202](202-kernel-and-packs.md)) or a plug-in ([section 203](203-the-provider-contract.md)), and it belongs to exactly one archetype. A binding is how one mode carries out one operation; an operation can have several. Operations, pipelines, the ledger, and the policy gate are the same whatever the mode.

## The seven

| # | Archetype | The system is reached by | Examples | Modes |
| --- | --- | --- | --- | --- |
| 1 | Interface | The elements a person would use, found by role and name | A web page, a desktop window, a mobile screen, a terminal screen | `ui`, `desktop`; for planning, `mobile`, `terminal` |
| 2 | Request | A typed request over a protocol, and its answer | HTTP and its descendants, a database query, a directory lookup, a tool on an MCP server | `page_http`, `http`, `api`, `site_tools`, `mcp`; proposed, `sql`; for planning, `rpc` |
| 3 | Command | Starting a program with arguments and reading its exit status and output | A command-line tool, a script the system ships, the same on another machine | Proposed, `cli` |
| 4 | Bridge | Code running inside the host, through an interface the host offers for that purpose | A scripting host, an add-in, an automation interface, a function a library exports | For planning, `bridge`, `library` |
| 5 | Artifact | Reading and writing a file in a known format | A spreadsheet, a configuration file, a document | Proposed, `file` |
| 6 | Stream | Waiting for, or adding to, a flow of events | A queue, a change feed, a log, telemetry | Proposed, `stream` |
| 7 | Device | Exchanging frames with hardware over a bus or a link | A serial console, registers on a fieldbus, a management protocol | Proposed, `device` |

A proposed mode belongs to a pack that is designed and not yet built. A name "for planning" belongs to no designed pack. Either becomes real only when its pack clears the bar in [section 202](202-kernel-and-packs.md) and the mode has its own decision record. [Section 106](../1-product/106-what-it-latches-onto.md) lists the kinds of surface each archetype covers, and those it refuses.

## In the kernel, the archetype is the closed type

The kernel knows the seven archetypes and holds no list of modes ([decision 0060](../../decisions/0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md)).

- `Archetype` is an enum of seven values in `a2m-core`. The engine's handling differs by archetype and never by mode.
- A mode is a checked name, the type `Mode` in `a2m-core`: lowercase letters, digits, and underscores, starting with a letter, at most 32 characters.
- A provider registers each mode it brings, with its archetype, in its manifest. The registry in `a2m-provider` is the only list of modes.
- A binding that names a mode nobody registered does not compile: unknown means stop. The check is made by the target compiler at load time, where the registry is known, and fails with `input`, `mode_unknown`, listing the registered modes.
- Two providers that register one mode must give it the same archetype, or the second manifest is refused with `input`, `mode_conflict`.
- Kernel code cannot branch on a particular mode, because it cannot name one. A test holds the engine's source free of every first-party mode name.

Every word that arrives with a pack is a registered name in the same way: a surface kind, a step kind, a file kind, a lint code, a finding kind, a control name ([decision 0062](../../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)). A step kind is a name within its mode, so two modes may each have a step called `request` or `locate`.

**Registering a mode.** A first-party pack's mode needs a decision record, because modes are public vocabulary. A plug-in's mode needs its manifest approved by an administrator ([section 203](203-the-provider-contract.md)). The driver interface is public, so a plug-in can bring a mode for a kind of system this repository has never seen without any change to the kernel.

**What the manifest says about each mode.** Everything the kernel needs to know about a mode is data in its provider's manifest, so that nothing in the kernel names one ([decision 0063](../../decisions/0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md)). The full manifest is in the [design of `a2m-provider`](../../architecture/crates/a2m-provider.md).

| Field | What it says |
| --- | --- |
| `name` | The mode |
| `archetype` | One of the seven |
| `verbs` | The verbs the mode can carry out; the [verbs by mode](../../reference/technical-reference.md#verbs-by-mode) table collects them |
| `steps` | The step kinds its lowerings may emit |
| `commit_steps` | The step kinds that always save. A step of such a kind that is not marked as the commit point is refused before the driver sees it. May be left out |
| `rank` | Its default place in binding order when an operation states none. Lower is tried first |
| `intent_window` | How many intents may be stored ahead of acting, by default ([section 208](208-runs-the-ledger-and-resume.md)) |
| `session_reads`, `session_writes` | Parallel reads and writes per session, by default ([section 211](211-concurrency.md)) |
| `exclusive` | What one write holds alone: nothing, the surface, a file, a screen, or a link |

**One mode, one driver.** If two ready providers drive the same mode and no setting names one, the registry refuses with `input`, `provider_ambiguous`, listing each candidate. If the named provider is not ready, it fails with `busy`, `provider_unavailable`, and never falls to the other. There is no priority and no silent fallback, and registration order never changes the result.

## How each archetype fills the loop

The loop is guard, choose, prepare, intend, act, confirm, read back, record ([section 207](207-the-engine-loop.md)). Guard, choose, intend, and record belong to the kernel and never differ. The other four are the archetype's. In the driver interface, confirming and reading back are lists of steps that `act` carries out ([decision 0063](../../decisions/0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md)).

| Archetype | Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- | --- |
| Interface | A tab, a window, a screen | Locate an element; set it through its widget driver; invoke an action | A response, an element, a state | The field again, or the save response |
| Request | A client with the session's credentials | Build the request from a typed template; send it | The status and a response that fits its schema | The matching read request |
| Command | A working directory, an allowed environment, the program's verified path | Start the program with arguments built from a typed template; never a shell | The exit status and a declared pattern in the output | A declared read command |
| Bridge | A connection to the bridge inside the host | Call one declared function with typed arguments | The function's typed result | The matching read function |
| Artifact | The file, opened without changing it | Set values at named places; save by writing a new file and replacing the old one in one step | The save succeeded and the file parses | Open the saved file and read the same places |
| Stream | A subscription or a producer | Publish one message, or wait for one that matches | The broker's acknowledgement, or the message arriving within the time limit | Only where the system offers a read: a keyed topic, a stored message. Otherwise none |
| Device | An open link, with exclusive use of it | Write one frame or one declared console command | The device's acknowledgement | Read the same register or parameter again |

What "cannot write" means in the sandbox, where drafts are tried and discovery runs, is stated per archetype by each pack and proven against its test stand. The kernel's half is the intent token: outside a declared test record, no step that commits is ever sent.

| Archetype | What the pack guarantees in the sandbox |
| --- | --- |
| Interface | Only plain navigation; no control whose role or name commits is pressed; requests other than reads are blocked in the browser |
| Request | Only methods the service's own description marks safe, or read endpoints a person marked safe. For a database, a statement runs in a read-only transaction |
| Command | A program is started only in its declared help and version forms |
| Artifact | A file is opened for reading only, and never replaced |
| Stream | A queue is never consumed and nothing is published; a sample is read without acknowledging |
| Device | Only frames the device lists as reads |

Discovery itself is [section 218](218-discovery-and-findings.md).

## Steps each archetype adds

Steps are the fixed instructions a plan is made of ([steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md)). The existing kinds stay. Each archetype beyond interface and request adds a small set, registered by its first pack as step kinds of its mode and fixed in the phase that builds that pack.

| Archetype | Steps |
| --- | --- |
| Command | `spawn`: start a declared program with typed arguments. `expect`: check the exit status and output. `parse`: take typed values out of output in a declared format. `collect`: receive a file the program wrote |
| Bridge | `attach`: connect to a bridge and check its version. `invoke`: call a declared function. `read`: call a declared read function |
| Artifact | `open`, `locate` a named place, `set`, `read`, `save` |
| Stream | `subscribe`, `await` a matching event, `publish`, `acknowledge` |
| Device | `open` a link, `build_frame`, `write_frame`, `read_frame`, `expect` |

A step still carries only its kind, what it refers to, a time limit, and the failure class to raise. A step contains no logic.

## Where read-back is not possible

Three cases recur, and one rule covers all three: a write that cannot be read back is `irreversible`, approved by a person every time, and never resumed automatically ([section 212](212-policy-and-approvals.md)).

| Case | Example | What "confirmed" then means |
| --- | --- | --- |
| The effect leaves the system | A message sent, a payment made, a mail delivered | The system's own acknowledgement, recorded as evidence. It proves acceptance, not the outcome |
| The effect is physical | A valve opened, a motor started | A declared sensor reading where one exists; otherwise the acknowledgement only. Such an operation also carries a hazard ([section 213](213-effects-hazards-and-consent.md)) |
| The system offers no read | A command with no matching query | The exit status and output only |

## Identity in each archetype

Every write declares how its record is found again ([section 209](209-record-identity-and-dependent-records.md)). The strategies do not change; what a "record" is does.

| Archetype | A record is | Usual strategy |
| --- | --- | --- |
| Interface, request | A row, a document, an entity | As section 209 gives |
| Command | Whatever the tool names: a job, a user, a resource | `natural` by the name the caller chooses; `find` through a list command |
| Bridge | An object in the host | `natural` by the host's own identifier |
| Artifact | A named range, a key path, a row with a key column | `natural`, or `composite` of file and place |
| Stream | A message | `stamped` with the idempotency token where the system keeps messages; otherwise `none` |
| Device | A register, a parameter, an object in a dictionary | `natural` by address or name |

## The modes of the first packs

These are the modes of the four packs designed first ([section 202](202-kernel-and-packs.md)). The web pack is built first; the HTTP and MCP packs come in Phase 4, and the desktop pack in Phase 8.

| Mode | Pack | Archetype | How it reaches the system | Needs | Best for | Main risk |
| --- | --- | --- | --- | --- | --- | --- |
| `ui` | Web ([section 301](../3-packs/301-the-web-pack.md)) | Interface | The browser engine drives a Chromium-family browser over the DevTools protocol, acting on elements by role, label, text, or test id; no pixel clicks | A browser session | Writes that must pass the system's own validation; systems with no usable traffic | Layout and label changes |
| `page_http` | Web | Request | Requests sent from inside the signed-in page with the page's own session, cookies, and headers, described in the endpoint catalog that the HTTP pack owns | A browser session | Fast reads and proven writes without driving the interface | Front-end logic is skipped |
| `site_tools` | Web | Request | Calls tools the site registers in the browser (WebMCP). It is the web pack's because it needs a tab that can act | A browser session | Sites that offer them | Early standard, few sites |
| `http` | HTTP ([section 303](../3-packs/303-the-http-pack.md)) | Request | Requests sent directly from code, with no browser at run time | A session cookie or token from the session keeper | Bulk and high-volume work; servers without a browser | Undocumented endpoints change; bot defenses |
| `api` | HTTP | Request | Calls generated from a published description: OpenAPI, a GraphQL schema, OData metadata, or gRPC reflection | A token or key | Systems with official APIs | Coverage gaps |
| `mcp` | MCP ([section 304](../3-packs/304-the-mcp-pack.md)) | Request | Connects to an MCP server the system already provides and re-exposes chosen tools with this system's policy, approval, and audit around them | The server's endpoint and its sign-in | Systems that ship their own MCP server | Its tools may be too broad or too thin |
| `desktop` | Desktop ([section 305](../3-packs/305-the-desktop-pack.md)) | Interface | Drives a desktop application through the operating system's accessibility tree: UI Automation on Windows, the Accessibility API on macOS, AT-SPI on Linux | The application running on the device | Desktop clients | Applications with poor accessibility data |

**An application built on a browser runtime is a web surface.** Its browser is started by the supervisor, and it is served by `ui`. The `desktop` mode does not reach it; the browser engine belongs to the web pack.

## The proposed modes of the designed packs

Five further packs are designed and not built. Each proposes one mode, which becomes real by its own decision record when the pack is built.

| Mode | Pack | Archetype | Reaches |
| --- | --- | --- | --- |
| `cli` | Command ([section 306](../3-packs/306-the-command-pack.md)) | Command | A program started by absolute path with typed arguments, never through a shell. Built second, to test that the kernel is neutral (hypothesis H-26) |
| `sql` | Data ([section 307](../3-packs/307-the-data-pack.md)) | Request | A relational database; reads before writes |
| `file` | File ([section 308](../3-packs/308-the-file-pack.md)) | Artifact | A file in a known format, treated as the system operated on |
| `stream` | Stream ([section 309](../3-packs/309-the-stream-pack.md)) | Stream | A queue, a topic, a change feed, or a log |
| `device` | Device ([section 310](../3-packs/310-the-device-pack.md)) | Device | Registers or objects on a fieldbus, or a console on a framed link |

## How the runtime chooses a binding

The engine takes the plan's bindings in their declared order and picks the first that qualifies ([decision 0006](../../decisions/0006-the-runtime-picks-the-fastest-verified-binding.md)). A binding qualifies when:

- its status is `verified`;
- the registry has a driver for its mode whose health is ready;
- its surface has not drifted and has no edge block;
- its session is valid, or its surface needs none.

**The order is data.** An operation can list its bindings in any order. Where it states none, the compiler sets the order from each mode's `rank` in its provider's manifest ([section 206](206-the-compiler-and-the-plan.md)). Ranks put the cheapest way first: a published API before direct requests, direct requests before requests from inside the page, and those before driving the interface. The engine walks the plan's order and never knows which mode is cheap.

The choice, the reason, and the provider that serves the mode are recorded in the trace. If no binding qualifies, the run stops with the class of the best reason any candidate failed: `session_expired` before `edge_blocked` before `drift` before `busy`.

**Fallback.**

- A read falls back to the next qualifying binding after `drift`, a `transient` failure that outlasted its retries, or a driver that becomes unavailable.
- A write falls back only if nothing was sent for the current record, or after reconcile has shown that nothing landed. A record is never carried by two modes.
- A mode can be pinned for a run. A setting can pin one; a caller can pin one only where policy allows that option ([section 212](212-policy-and-approvals.md)). A pinned mode removes every other binding.

Every verified binding of an operation must give the same normalized result for the same input. The binding-equivalence gate enforces it before a version is published ([section 403](../4-building-tools/403-testing-and-publish-gates.md)).

## Shadow and promotion

Each binding has a status.

| Status | Meaning |
| --- | --- |
| `draft` | Written and not yet proven. It runs only in the sandbox, as a try |
| `shadow` | Predicts what it would have sent and returned, without acting, while a verified binding does the real work |
| `verified` | May be chosen at call time |
| `retired` | Never chosen, in the sandbox or out of it |

A new binding starts in `shadow`. After a set number of exact matches with the verified binding, a person may promote it to `verified` with `binding.promote`; `shadow.report` shows how often it matched and where it differed. This is how an operation that began as interface steps gains a direct request binding safely. Shadow arrives in Phase 4. Runs in the sandbox are recorded as tries, so they change no binding's health or shadow count.

## Why archetypes and not just modes

A mode names one protocol or one technology. An archetype names what the engine and the compiler have to know: how a step is acted, confirmed, and read back. Seven archetypes mean seven shapes of driver to understand, lint, and test, however many modes there come to be. A proposed mode that fits none of them is a reason to examine the kernel, not to add a special case. Whether every system of real interest fits one of the seven is hypothesis H-29.

## Open points

- The exact steps of each new archetype, fixed with its first pack.
- Whether waiting for an event should also be able to start a run. Triggers are not designed ([section 704](../7-project/704-readiness.md)).
- Whether `library` is a mode of its own, or the bridge mode with the host being a language runtime.
- How a shadow binding predicts: a posture in which the driver's handle refuses every step that reaches the system and the driver returns what it would have sent.
- Load-time checking of modes could let through a class of mistake the Rust compiler would have caught. If it does, decision 0060 is revisited.
