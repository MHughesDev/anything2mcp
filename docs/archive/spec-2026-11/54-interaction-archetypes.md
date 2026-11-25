# 54. Interaction archetypes

> Archived on 2026-11-17. This is section 54 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 201, Archetypes and modes](../../spec/2-kernel/201-access-kinds-and-modes.md).

There are many kinds of system and few ways to reach one. This section names seven, and shows that each fills the engine loop of [section 8](../../spec/2-kernel/207-the-engine-loop.md) in the same way. A mode belongs to exactly one archetype. A pack ([section 55](../../spec/2-kernel/202-kernel-and-packs.md)) supplies one or more modes.

Decision 0005 fixed seven modes. Those seven stay exactly as they are. Further modes arrive with packs, each by decision record ([record 0050](../../decisions/0050-seven-archetypes-and-modes-that-arrive-with-packs.md)).

## The seven

| # | Archetype | The system is reached by | Examples | Modes |
| --- | --- | --- | --- | --- |
| 1 | Interface | The elements a person would use, found by role and name | A web page, a desktop window, a mobile screen, a terminal screen | `ui`, `desktop`; later `mobile`, `terminal` |
| 2 | Request | A typed request over a protocol, and its answer | HTTP and its descendants, a database query, a directory lookup, a tool on an MCP server | `page_http`, `http`, `api`, `site_tools`, `mcp`; later `sql`, `rpc` |
| 3 | Command | Starting a program with arguments and reading its exit status and output | A command-line tool, a script the system ships, the same on another machine | Later `cli` |
| 4 | Bridge | Code running inside the host, through an interface the host offers for that purpose | A scripting host, an add-in, an automation interface, a function a library exports | Later `bridge`, `library` |
| 5 | Artifact | Reading and writing a file in a known format | A spreadsheet, a configuration file, a document | Later `file` |
| 6 | Stream | Waiting for, or adding to, a flow of events | A queue, a change feed, a log, telemetry | Later `stream` |
| 7 | Device | Exchanging frames with hardware over a bus or a link | A serial console, registers on a fieldbus, a management protocol | Later `device` |

"Later" modes are names for planning. Each becomes real only when its pack clears the bar in section 55.

**In the kernel, the archetype is a fixed type and a mode is a registered name** ([record 0060](../../decisions/0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md)). The kernel knows the seven archetypes and holds no list of modes. A provider registers each mode it brings, with its archetype, and a binding that names a mode nobody registered does not compile. The engine's handling differs by archetype and never by mode.

## How each archetype fills the loop

The loop is guard, choose, prepare, intend, act, confirm, read back, record. Guard, choose, intend, and record belong to the kernel and never differ. The other four are the archetype's.

| Archetype | Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- | --- |
| Interface | A tab, a window, a screen | Locate an element; set it through its widget driver; invoke an action | A response, an element, a state | The field again, or the save response |
| Request | A client with the session's credentials | Build the request from a typed template; send it | The status and a response that fits its schema | The matching read request |
| Command | A working directory, an allowed environment, the program's verified path | Start the program with arguments built from a typed template; never a shell | The exit status and a declared pattern in the output | A declared read command |
| Bridge | A connection to the bridge inside the host | Call one declared function with typed arguments | The function's typed result | The matching read function |
| Artifact | The file, opened without changing it | Set values at named places; save by writing a new file and replacing the old one in one step | The save succeeded and the file parses | Open the saved file and read the same places |
| Stream | A subscription or a producer | Publish one message, or wait for one that matches | The broker's acknowledgement, or the message arriving within the time limit | Only where the system offers a read: a keyed topic, a stored message. Otherwise none |
| Device | An open link, with exclusive use of it | Write one frame or one declared console command | The device's acknowledgement | Read the same register or parameter again |

## Steps each archetype adds

Steps are the fixed instructions a plan is made of ([steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md)). The existing kinds stay. Each new archetype adds a small set, to be fixed in the phase that builds its first pack.

| Archetype | Steps |
| --- | --- |
| Command | `spawn`: start a declared program with typed arguments. `expect`: check the exit status and output. `parse`: take typed values out of output in a declared format. `collect`: receive a file the program wrote |
| Bridge | `attach`: connect to a bridge and check its version. `invoke`: call a declared function. `read`: call a declared read function |
| Artifact | `open`, `locate` a named place, `set`, `read`, `save` |
| Stream | `subscribe`, `await` a matching event, `publish`, `acknowledge` |
| Device | `open` a link, `write_frame`, `read_frame`, `expect` |

A step still carries only its kind, what it refers to, a time limit, and the failure class to raise. A step still contains no logic.

## Where read-back is not possible

Three cases recur, and the rule for all three already exists: a write that cannot be read back is `irreversible`, approved by a person every time, and never resumed automatically.

| Case | Example | What "confirmed" then means |
| --- | --- | --- |
| The effect leaves the system | A message sent, a payment made, a mail delivered | The system's own acknowledgement, recorded as evidence. It proves acceptance, not the outcome |
| The effect is physical | A valve opened, a motor started | A declared sensor reading where one exists; otherwise the acknowledgement only. Section 61 adds a hazard to these |
| The system offers no read | A command with no matching query | The exit status and output only |

## Identity in each archetype

Every write declares how its record is found again ([section 50](../../spec/2-kernel/209-record-identity-and-dependent-records.md)). The strategies do not change; what a "record" is does.

| Archetype | A record is | Usual strategy |
| --- | --- | --- |
| Interface, request | A row, a document, an entity | As section 50 |
| Command | Whatever the tool names: a job, a user, a resource | `natural` by the name the caller chooses; `find` through a list command |
| Bridge | An object in the host | `natural` by the host's own identifier |
| Artifact | A named range, a key path, a row with a key column | `natural`, or `composite` of file and place |
| Stream | A message | `stamped` with the idempotency token where the system keeps messages; otherwise `none` |
| Device | A register, a parameter, an object in a dictionary | `natural` by address or name |

## Why archetypes and not just modes

A mode names one protocol or one technology. An archetype names what the engine and the compiler have to know: how a step is acted, confirmed, and read back. Seven archetypes mean seven shapes of driver to understand, lint, and test, however many modes there come to be. A proposal for a mode that fits none of them is a signal to stop and look at the kernel, not to add a special case.

## Open points

- The exact steps of each new archetype, fixed with its first pack.
- Whether waiting for an event should also be able to start a run. Triggers are still not designed ([readiness](../plans-2026-11/readiness.md)).
- Whether `library` is a mode of its own or the bridge mode with the host being a language runtime.
