# H-26: the kernel is neutral: a second kind of system can be added as a pack without changing kernel crates beyond its contracts

**Status:** supported, with limits (see the last section)

**Tested:** 2026-11-24, package D4, over the command pack as built through package D3 and the packages after it (reads, writes with a read-back, a script the target keeps, the scout's readers, the check of programs). Counted from the decision records and the code at that date, not from a diff of the first and the last commit; the way each change is attributed to the command pack is the records' own words.

## What was assumed

The kernel has no assumption about the kind of system built into it. A second kind, a command-line program, can be added as a pack, by registering what it needs, and the kernel crates change only where a contract was missing something, never to learn what a command is.

The roadmap's rule for this package: if the kernel changed more than slightly, the boundary is redrawn before another pack starts. Section 306 set the line in advance: of the seven changes it listed as likely, if more than two needed a kernel edit, the boundary is redrawn.

## What was done

The command pack was built against the test stand `targets/example-cli`, through the provider contract only. Every kernel change that a decision record attributes to the command pack, or to an audit of it, was listed and sorted by who asked for it. Then the kernel crates were searched for the pack's names: the mode `cli`, the surface kind `program`, the file kind `command`, and the pack's crate.

## What was found

**Section 306's seven likely changes.** Of the seven ([decision 0099](../decisions/0099-the-command-pack-as-building-it-settled-it.md)): two needed nothing because the kernel already takes them as data (surface kinds, steps); one needed a change (the supervisor's programs, item 2); four needed nothing (leases, read-back, the sandbox, target identity not exercised). That is 1 of 7 against a limit of 2.

**Every kernel change the command pack forced or asked for, 6 in all, and 4 more that every pack was asked for.**

| # | Change | Where | Asked by | Decision |
| --- | --- | --- | --- | --- |
| 1 | The plan declares the programs a surface may start; a surface kind marks which of its fields name one; the engine hands them to the supervisor | The plan in `a2m-core`, the registration data of `a2m-files`, the engine | The command pack alone | [0124](../decisions/0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md) |
| 2 | `CheckedPath::create_dir`: make a folder only where nothing stands | `a2m-provider` | The command pack's run folder, and the file pack | [0148](../decisions/0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md) |
| 3 | The handle redacts the values, the values for reconcile, and the captures a step observed, as it already did a failure | `a2m-provider` | The audits of the command and HTTP packs | 0148 |
| 4 | The supervisor takes a program's digest through one opened handle at every start and starts that handle where the system can | `a2m-provider` | The command pack's audit asked; the provider's own audit found it (finding P2, P4) | 0148, [0140](../decisions/0140-the-command-pack-after-its-safety-audit.md) |
| 5 | The check of who may change a program, with the closed tables of interpreters and of variables that load code, moves from the command pack into the contract | `a2m-provider` | The MCP pack, which could not use another pack's code | [0250](../decisions/0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md) |
| 6 | `CheckedPath::create_private_dir` and `create_private_dir_all`: a folder that is the engine's user's alone from the moment it exists | `a2m-provider` | The fifth safety audit of the command pack (finding PR-4) | 0250 |
| 7 to 10 | `Lowering::commits`, `Lowering::least_effect`, `FileKind::writes`, and `commit_steps` in a manifest | `a2m-files`, `a2m-provider` | An audit of every pack that writes; the command pack answers them like the others | [0153](../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md), 0148, [0154](../decisions/0154-the-http-command-and-mcp-packs-state-what-saves-what-writes-and-which-key-is-sent.md) |

So: 1 change that the command pack forced alone, 5 that it forced or asked for with another pack or an audit, 4 that every pack was asked for. Only change 1 altered the plan's shape. The rest are additions: a method with a default, a function moved from a pack into the contract, a new constructor. Each leaves a pack that does not use it as it was.

**What no change was.** No kernel crate branches on the mode `cli`, the surface kind `program`, the file kind `command`, or the command pack. A search of the kernel crates' sources (`a2m-core`, `a2m-files`, `a2m-expr`, `a2m-provider`, `a2m-engine`, `a2m-sched`, `a2m-ledger`, `a2m-policy`, `a2m-auth`, `a2m-vault`, `a2m-runner`, `a2m-local`, `a2m-mcp`, `a2m-api`, `a2m-actions`) for these names finds only: the archetype `command` in `a2m-core`, which is in the closed type by design ([decision 0060](../decisions/0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md)); the word `command` in the provider's closed list of programs that read their whole line as code (the check of programs, a list of program names, not the pack); `"cli"` as a client's label in `a2m-local` (a door, not a mode); one test in `a2m-local` that builds a surface in mode `cli`; and a pack's guide, `guides/pack-command.md`, which `a2m-actions` embeds as text, as it does for other packs, to answer a question about a pack. The engine's loop ran the command pack's steps unchanged: the product test `a_command_line_program_is_read_through_the_command_pack` reads through the same loop as the first read, and its runs give one trace signature.

**What the pack did itself where the kernel could not give it.** Of the thirteen points of decision 0099, the pack kept its own answer to each and made no kernel edit; rows 2 to 5 (byte ceilings for each stream, a stream that passes its ceiling, a grace period, concurrency) are the supervisor's one-shot link and are the place a second command-like pack would meet the same gaps first. This record does not re-count which of these still stand; change 4 answered one request of the pack's audit, not these.

## What changes

- The register entry for H-26 is `supported`, with the limits below, and its dated notes link here.
- The boundary between kernel and packs is not redrawn. One plan change in the first pack of a second kind, and additions after audits, is slight by the roadmap's rule and by section 306's.
- [Decision 0399](../decisions/0344-the-first-result-of-h-26-the-boundary-stays-and-where-a-second-command-like-pack-would-meet-the-contract.md) records this and names what to watch.

## What is still unknown

- A pack whose kind does not start a program at all, with state kept for a long time between steps (a device, a stream, a desktop), was counted in the register's dated notes, not here: those packs forced a collision of shared names (decisions 0202, 0203), a held folder, and a fixed place. This result is for the command pack; it does not settle the others.
- A pack that needs several findings at once to draft a tool is not shown.
- The count rests on what the records say each change was for. A change asked for by one pack and found by an audit is placed by its record; another reader of the same records could place two of the six differently, and the total would not change.
- Whether the same kernel changes would have been found without a second pack, by an audit alone, is not known.
