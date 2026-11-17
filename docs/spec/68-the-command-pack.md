# 68. The command pack

The command pack lets a target be a program: something started with arguments, which ends with an exit status and leaves output behind. It is the second pack to be built. It was chosen because it is as unlike a web page as a kind of system can be while staying small, and its job is to show that the kernel is neutral (hypothesis H-26; Stage D of the [long-term plan](../plan/anything2mcp-master-plan.md)). It brings one mode, `cli`, in the command archetype of [section 54](54-interaction-archetypes.md). The mode is proposed: it becomes real by its own decision record when the pack is built, as item 8 of the bar in [section 55](55-kernel-and-packs.md) requires.

**It is never a shell.** No command line is ever put together from text an agent supplied, and no program that reads a command line as code is started on an agent's behalf ([section 61](61-effects-hazards-and-consent.md)).

## What it reaches, and what it refuses

| Reaches | How |
| --- | --- |
| A program installed on the machine the engine runs on | By absolute path, checked by digest and version before it is started |
| The same program on another machine | Later, through a runner on that machine, which starts it locally by the same rules. See "Another machine" below |

| Refused | Why |
| --- | --- |
| A shell, a command interpreter, or any route through one, including the Windows command interpreter behind batch and command-script files | It reads a line as code; nothing typed can survive that |
| A program whose argument is evaluated as code: an interpreter given inline code, a tool's "execute this" or "run this expression" flag, a template the program expands | A tool that accepts free text and runs it cannot be verified |
| Finding the program on the search path, or a path that comes from an input | Rule seven, and section 51: no input changes what runs |
| A program, or a directory above it, that a user other than the engine's can write to | Anyone who can replace the file can choose what runs |
| A terminal. The program never gets one | A screen of text belongs to the terminal kind of [section 62](62-catalog-of-surface-kinds.md), not here |
| Attaching to a program already running, or reading its memory | Section 62 refuses both |

A script that the system ships is allowed only as a fixed pair: the interpreter and the script file, both named by path and both checked by digest, with no input in any position the interpreter reads as code.

## What the pack brings

| Bar item in section 55 | Here |
| --- | --- |
| 1. A driver with no retries inside | "The driver" |
| 2. A read-back rule for every write, or `irreversible` | "Read-back" |
| 3. An answer for each identity strategy | "Identity" |
| 4. A test stand with a control interface | "The test stand" |
| 5. A discoverer whose findings claim no more than was observed | "Discovery and drafting" |
| 6. Failures mapped to the existing classes | "Failures" |
| 7. Effects and hazards declared | "Effects and hazards" |
| 8. A decision record for the mode | Written by the lead with the pack, for `cli` |

It registers, as every pack does: the schema of `commands/` and of the `cli` binding fields, a lowering for each verb, lint rules, the driver, a discoverer and a describer, and drafting rules. It also needs a surface kind, `program`, which [file formats](../reference/file-formats.md) does not yet let a pack register; see the last section but one.

## The `commands/` file kind

A `commands/` file is to a program what an `endpoints/` file is to an HTTP surface: the one place where a program's commands are described. Operations bind to them by name. The surface in `target.yaml` gives the path, and an environment may override it.

```yaml
# target.yaml (part)
surfaces:
  tool: {kind: program, path: /opt/example/bin/example-tool}
environments:
  test: {kind: test, surfaces: {tool: {path: /opt/example-test/bin/example-tool}}}
```

```yaml
# commands/inventory.yaml
program:
  surface: tool
  digests:                                 # every known build; sha-256 of the file
    linux-x86_64: ["sha256:3f1c...e9"]
    macos-aarch64: ["sha256:71b2...0c"]
    windows-x86_64: ["sha256:9a07...41"]
  version:
    args: ["--version"]
    expect: {exit: [0], stdout: '^example-tool (?<version>\d+\.\d+)\.\d+$'}
    supported: ["2.4", "2.5"]
  help: [["--help"], ["help", "{command}"]]  # the only forms discovery may run
  env:
    set: {LC_ALL: C.UTF-8, TZ: UTC, EXAMPLE_TOOL_HOME: "{session.state_dir}"}
    pass: []                               # nothing is copied from the engine's own environment
  workdir: fresh                           # an empty folder the engine owns, made for each run
  stdin: none
  limits: {timeout: 10s, grace: 2s, stdout: 1MiB, stderr: 64KiB, concurrency: 1}
  prompts: ['^Are you sure\?', '^Password:']
  exits: {0: ok, 2: input, 4: rejected, 5: stale, 6: busy, 75: transient, 77: session_expired}

commands:
  item_show:
    effect: read
    params: {sku: {type: sku}}
    args: [item, show, {param: sku}, --format=json]
    exits: {3: absent}
    output: {json: schemas/item.json}

  item_list:
    effect: read
    params: {location: {type: location_code, required: false}, ref: {type: stamp, required: false}}
    args: [item, list, {flag: --location, param: location}, {flag: --ref, param: ref}]
    output: {table: {separator: tab, header: [sku, name, quantity, location]}}

  item_set_quantity:
    effect: write
    params: {sku: {type: sku}, quantity: {type: whole_count}}
    args: [item, set, {param: sku}, {flag: --quantity, param: quantity, join: "="}, --yes]
    exits: {3: absent}
    output: {pattern: '^updated (?<sku>\S+) quantity=(?<quantity>\d+)$', match: one_line}

  item_add:
    effect: write
    params: {name: {type: item_name}, location: {type: location_code}}
    args: [item, add, {flag: --name, param: name}, {flag: --location, param: location},
           {flag: --ref, stamp: true}, --format=json]
    output: {json: schemas/created.json}
    assigns: {sku: output.sku}

  catalog_push:
    effect: irreversible
    hazard: sends_outside
    reaches: {network: ["catalog.example.com"]}
    args: [catalog, push, --yes]
    output: {pattern: '^pushed (?<count>\d+) items$', match: one_line}
```

**Program.**

| Field | Meaning |
| --- | --- |
| `surface` (required) | A surface of kind `program` |
| `digests` (required) | Known builds, by platform. A file with no listed digest does not run ("Failures") |
| `version` | A command that reports the version, its expected pattern, and the versions the file was written for |
| `help` | The argument lists discovery may run, with `{command}` standing for a listed command name. None are assumed |
| `env` | `set`: variables and values; `pass`: names copied from the engine's environment. Nothing else reaches the program. `{session.state_dir}` is a folder the engine owns for the session ("Secrets") |
| `workdir` | `fresh`, or a named folder inside the allowed roots of section 51 |
| `stdin` | `none` (closed at once), a param, a secret, or a blob. Never a terminal |
| `limits` | Time limit, the grace before a forced stop, byte ceilings for each stream, and how many may run at once on this surface |
| `prompts` | Patterns that mean the program is asking a person something |
| `exits` | What each exit status means, shared by every command: `ok`, `absent`, or a failure class. A command may add to it, never change it |

**Command.** `effect` (required: `read`, `write`, or `irreversible`), `hazard` (default `none`), `params` (typed by field types from `types.yaml`), `args` (required), `stdin`, `exits`, `output` (required), `assigns`, `reaches`, `limits`.

**Argument elements.** `args` is the argument list, element by element. Each entry becomes exactly the elements shown, and no element is ever split, joined to another, or read by anything but the program.

| Entry | Becomes |
| --- | --- |
| `item` | The element `item`, as written |
| `{param: sku}` | One element: the value, formatted by its field type |
| `{flag: --name, param: name}` | Two elements, `--name` and the value; none if the param is optional and absent |
| `{flag: --quantity, param: quantity, join: "="}` | One element, `--quantity=12` |
| `{flag: --archived, when: archived}` | `--archived` if the boolean param is true |
| `{flag: --tag, each: tags}` | The flag and a value for each item of a list |
| `{flag: --ref, stamp: true}` | The flag and the record's stamp ("Identity") |
| `--` | The end-of-options marker, as written |

**Rules on values.** A value never contains a NUL character. A value given as its own element may not begin with `-` unless it comes after a `--` element, so an input cannot become an option. A text param must have a field type with an anchored pattern or a closed list of values; free text without one does not compile.

**Output.** One declared form per command, parsed into typed values the engine normalizes with field types as for any mode. Standard error is kept as evidence and is never parsed for values.

| Form | Rule |
| --- | --- |
| `json` | The whole of standard output is one document, or one per line with `lines: true`, checked against a schema |
| `table` | Rows of named columns, split by a declared separator (`tab`, or fixed column positions); a header, if declared, must match exactly |
| `pattern` | An anchored pattern with named groups, matching one line, every line, or the whole output; any unmatched line is a failure |

Output is decoded as UTF-8 unless the program declares another encoding. The locale and time zone the program sees come from `env`, so that numbers and dates print the same way on every machine.

**Read or write is declared.** Help text does not say which commands change anything, and a name such as `show` proves nothing. `effect` has no default: a command without one does not compile, and the drafter never fills it.

**The binding.** An operation binds a command as it binds an endpoint: `mode: cli`, `command` (file and command name), `args` (a map from param to expression; a param takes the operation input of the same name when not listed), and `read_back` (a read command, required for `write`).

## Verbs and their steps

The pack adds the four step kinds that section 54 lists for the command archetype.

| Verb | Steps |
| --- | --- |
| `read` | `spawn` the read command; `expect` an `ok` or `absent` exit; `parse` |
| `list`, `search` | `spawn`; `expect`; `parse` into rows. A result cut by a ceiling says so (section 64) |
| `create` | `spawn`; `expect`; `parse`, capturing `assigns` at once with the `acted` mark; then the read-back |
| `update`, `delete` | `spawn`; `expect`; `parse`; then the read-back. For `delete` the read-back must answer `absent` |
| `upsert` | The preview's class (section 50) chooses between the binding's `create` and `update` commands; the binding names both |
| `upload` | A blob is copied to a file in the run's own folder and its path passed as an element; a caller never passes a path |
| `download` | `spawn`, then `collect` a file the program wrote into the run's own folder. It becomes a blob and comes back as a resource link (section 51) |
| `call` | `spawn`; `expect`; `parse`. Its effect class is the command's |

`open` and `submit` are not offered. A step carries its kind, the command it refers to, a time limit, and the failure class to raise, as every step does.

## The driver

**Prepare.** Resolve the surface's path for the environment. Refuse it if it is not absolute, if it or any folder above it can be written by another user, or if it is a script run by a command interpreter. Take its digest and look it up for this platform. Run the version command once per lease and check the version. Make the working folder. Build the environment from `env` alone, with the session's injected values. Nothing is inherited.

**Act.** Start the program through the kernel's supervisor ([section 57](57-supervising-subsystems.md)), which checks it against the plan's declared programs and owns the process from then on.

| | POSIX systems | Windows |
| --- | --- | --- |
| Start | The absolute path and the element list handed straight to the system's process-start call; no search path | The absolute path given as the application name, so nothing is searched; the command line built from the elements by the standard quoting rule of the platform's C runtime |
| Group | A new session and process group, with no controlling terminal | A job object that ends its processes when its handle closes |
| Inherited | Standard input, output, and error, and any declared secret handle; every other descriptor is closed | The same three, and declared handles only |
| Window | Not applicable | No console window |

Standard output and error are read as they arrive, at the same time, into buffers with the declared ceilings. A stream that passes its ceiling ends the process. Standard input is written and closed, or closed at once.

On Windows a program that parses its command line by another rule would see different arguments than were sent; such a program is refused until the test stand proves its rule.

**Stopping.** At the time limit, or when a ceiling is passed, the supervisor asks the group to end, waits for exit up to the grace limit, then ends it by force: the job object on Windows, a kill to the group elsewhere. Ownership is checked at that moment by the launch's token, as section 57 says, and cleanup that cannot be confirmed is reported as incomplete. The stop, its cause, and the cleanup are three facts in the trace.

**Confirm.** The `expect` step checks the exit status against `exits` and looks for `prompts` in both streams; then `parse`. **Read back** runs the read command as a step of its own, parsed the same way, and the engine compares. **No retries inside:** the driver starts each program once per step, and whether anything is tried again is the failure class's to decide, in the engine.

**What a run's state is after a stop.** For a read, a timeout is `transient` and the class rule allows a retry. For a write, any stop after the process was started, other than an exit status declared to mean nothing changed, is `possible_commit`: the run stops, and on resume the engine reconciles through the read command before anything else.

## Read-back

Every `write` command names a `read_back` command, which must work from the identity inputs alone and answer `absent` through a declared exit status or an empty result with complete coverage, as [file formats](../reference/file-formats.md) requires of every read-back. A command with no read is `irreversible`, approved by a person every run and never resumed automatically. Writing through the program and reading back through another surface of the same target, where one exists, is allowed and is what H-30 measures.

## Identity

| Strategy | For a program |
| --- | --- |
| `natural` | The caller chooses the name: a job, a user, an item code |
| `composite` | A parent and a child name together, both arguments |
| `stamped` | The program has a field that takes a reference and a way to list by it: the `{flag: ..., stamp: true}` element carries the record's stamp, and a read command finds it |
| `find` | A list command filtered by values the write set, which must match exactly one record |
| `none` | Neither: the command is `irreversible` |

A key the program prints when it creates a record is taken from `assigns` and stored at once, as section 50 says.

## Effects and hazards

| Effect in the pack's manifest | Held |
| --- | --- |
| Starts a process: only programs declared in a published plan's `commands/` files, by path and digest | `enforced` |
| Its arguments, environment, working folder, standard input, time limit, and output ceilings | `enforced` |
| Reads and writes files: the run's own folder and the session's state folder | `enforced` for what the pack does; the program's own file access is `declared` |
| Uses the network | None for the pack; a command's `reaches` is `declared` |

The program does whatever the operating system lets it. Until confinement is researched (section 61), what a command touches beyond its arguments is the developer's declaration, shown in the review summary, and the listing says `declared` in those words. A command that `reaches` outside the target is `sends_outside` unless the developer shows otherwise; a command that removes more than the record it names is `data_elsewhere`. Both make it `irreversible`.

The lint refuses, by rule: a program on the pack's list of shells and interpreters unless it is the fixed pair above; an argument the developer marks as code; a flag value in a position the program's help marks as an expression to run. The last two depend on the developer and the help, so they are shown first in the review summary.

## Secrets

A credential reaches a program only through `inject` ([section 44](44-session-keeper.md)), for one step, and never as an argument, since other programs on the machine can read a process's arguments.

| Channel | When |
| --- | --- |
| Standard input | The program reads the secret from it; preferred |
| An inherited handle | The program reads from a descriptor number it is given |
| An environment variable | The program accepts nothing else. Other processes of the same user can read it, and the listing says so |

A program that keeps its own sign-in is given `{session.state_dir}`, a folder the engine owns for that session, exactly as a browser is given an engine-owned profile. Its everyday folder is never used. Every injected value is registered with the redactor before the program starts, and both streams pass the redactor before anything is stored or returned. Output is untrusted text (section 61).

## Discovery and drafting

**The discoverer** runs only the `help` and `version` forms the target declares, with no input, a fresh empty folder, a bare environment, and a short limit. It runs nothing by default: many programs ignore a flag they do not know and do their ordinary work. Each listed command is a finding of kind `command` at `discovered`, with `not_exercised`; argument names and types read from help are `inferred`. A manual page or completion file at a declared path is read as a file, as `artifact`, and never run. The subject is a `file`: the program's digest and its reported version.

**The drafter** writes, from those findings, a `commands/` draft: the `program` block with the digest and version seen, and one command for each listed command, with its params typed where the help states a type and an open question where it does not. It writes no `effect`, no `exits` beyond `0`, no output form it did not see, no read-back, and no operation (section 67). The scout tries none of the drafted commands, since none has a known effect. A developer who declares a command `read` may try it with `draft.try`; the sandbox cannot make a program unable to write, so for this mode it runs a command only against an environment of kind `test`.

## Failures

| What went wrong | Class | Code |
| --- | --- | --- |
| The path is missing or not executable | `drift` | `program_missing` (proposed) |
| The file's digest is not listed for this platform | `drift` | `program_digest_changed` (proposed) |
| The version is absent or not supported | `drift` | `program_version_unsupported` (proposed) |
| The file or a folder above it is writable by another user | `policy` | `program_path_unsafe` (proposed) |
| A value is NUL-bearing, or would become an option | `input` | `argument_unsafe` (proposed) |
| A value breaks its field type | `input` | `schema_invalid` |
| An exit declared `rejected`; standard error is the untrusted message | `rejected` | `app_validation_error` |
| An exit declared `stale` | `stale` | `version_conflict` |
| An exit declared `busy`, or the surface's `concurrency` is full | `busy` | `program_busy` (proposed) |
| An exit declared `session_expired` | `session_expired` | `needs_login` |
| An exit declared `transient` | `transient` | `upstream_unavailable` |
| A read: an exit status not declared, or an end by signal | `unknown_variant` | `unexpected_exit_status` (proposed) |
| A read: a declared prompt appeared | `unknown_variant` | `unexpected_prompt` (proposed) |
| A read: output that does not parse | `drift` | `output_unparsed` (proposed) |
| A read: a stream passed its ceiling | `policy` | `limit_exceeded`, naming the setting |
| A read: the time limit passed | `transient` | `timeout` |
| A write: any of the four rows above, after the program started | `possible_commit` | `no_confirmation` |
| A `find` read-back matched more than one record | `possible_commit` | `ambiguous_match` |
| The read-back differs | `mismatch` | `read_back_differs` |
| A stopped program's cleanup could not be confirmed; new starts on the surface wait | `busy` | `cleanup_incomplete` (proposed) |

An `input`, `rejected`, `stale`, `busy`, `session_expired`, or `transient` exit on a write is the developer's statement that nothing changed. The publish gates check it against the test stand before a write reaches `behavior_verified`.

## The test stand

A small Rust program, `example-tool`, under `targets/`: an inventory of items kept in a data file inside the folder named by `EXAMPLE_TOOL_HOME`. It follows [the test application plan](../plan/test-application.md): ground truth comes from the tool's own store, every fault is a setting, and it is as real as is cheap. Its harness, like the test application's, starts a tool with a fresh home folder per test.

| Control | Purpose |
| --- | --- |
| Reset | Restore the seed items |
| Inspect | What the data file holds for an item, read directly, not through the tool |
| Count writes | How many times each item was written, and by which start of the program |
| Set a fault | Written to a control file the tool reads at every start; for the next start or until cleared |

| # | The tool does this | Risk it exercises |
| --- | --- | --- |
| C1 | Shows an item as JSON, a list as a tab table, and a result as one line of text | Each output form |
| C2 | Echoes the elements and environment it received, as JSON | Arguments arrive exactly: spaces, quotes, `$`, `;`, newlines, non-ASCII text, a leading dash after `--`; only declared variables arrive |
| C3 | Sets a quantity, with a read command | A write and its read-back, proven by Inspect |
| C4 | Rounds or reformats a stored value when told to | `mismatch` |
| C5 | Commits, then hangs until killed | Timeout after the write; `possible_commit`; reconcile finds the write |
| C6 | Creates an item with an assigned code, with and without a `--ref` field | `stamped` and `find` |
| C7 | Creates a duplicate if the same add runs twice | That a resumed run never runs it twice |
| C8 | Rejects bad input with a message on standard error and a declared exit | `rejected`, with the message untrusted |
| C9 | Asks "Are you sure?" unless given `--yes` | `unexpected_prompt`; no terminal |
| C10 | Exits with an undeclared status, or ends by signal | `unexpected_exit_status` and `no_confirmation` |
| C11 | Prints slowly, and prints more than the ceiling | Reading both streams at once; `limit_exceeded` |
| C12 | Reports a different version, or is rebuilt with a different digest | `program_version_unsupported`, `program_digest_changed` |
| C13 | Prints the injected secret, and text aimed at an agent | Redaction; untrusted output |
| C14 | Starts a child of its own that outlives it | The whole group is stopped, and cleanup is confirmed |

Every case runs on Windows, macOS, and Linux.

## What the kernel must not need to change

Hypothesis H-26 is measured by counting kernel changes. **A kernel change** is any change to a kernel crate's public types, its traits, its file formats, or its behavior that the pack needed. Not counted: registering the pack's mode, file kind, lowerings, lint rules, driver, discoverer, and drafting rules through the existing registration points; and a fix to a kernel bug that the web pack would have met as well, recorded as such. Each change is recorded in D4 with the line of this section that forced it.

Where the kernel is most likely to need one, most likely first:

1. **Surface kinds.** `target.yaml` lists the kinds `web`, `http`, `mcp`, `desktop` as a closed list. `program` needs either a kernel edit or surface kinds registered by packs, which the registrations of section 55 do not include.
2. **The supervisor.** Section 57 describes long-lived subsystems that shake hands. A one-shot program does not, and the list of programs it may start comes from a published plan, not only from a manifest. The architecture's table does not yet show packs calling the supervisor.
3. **Steps.** If the compiled step is a kernel enum of kinds, `spawn`, `expect`, `parse`, and `collect` are kernel edits. They should be a kernel envelope (kind, reference, time limit, failure class) around a payload only the pack reads.
4. **Leases.** Most programs need no session. The engine asks for a `SessionLease` before prepare; a surface with no sign-in must get an empty lease, not a special case.
5. **Read-back.** The two gaps the technical reference already names in the driver interface: read-back as a method or as steps, and reporting a record as absent.
6. **The sandbox.** It promises that a draft cannot write. For a program that promise cannot be enforced, so it must be stated per archetype.
7. **Target identity.** Section 60 takes a target's identity at each start; a program's is its digest and version, which the identity check must accept from the pack.

If more than items 1 and 3 need a kernel edit, the boundary is redrawn before a third pack is designed, as Stage D says.

## Another machine

The usual remote-login protocols run a command by handing one line to the remote account's shell, which this pack refuses. So the same program on another machine is reached through a runner there ([section 43](43-runner-protocol.md)): it is the session's home runner, it claims the job, and it starts the program locally by every rule above. A remote transport that starts a program from an element list with no shell may be added later, with its own decision record.

## Build order

| Step | What | Proof |
| --- | --- | --- |
| 1 | D2: `example-tool`, its control interface, and its harness | Its own tests on three systems |
| 2 | The `commands/` schema, lint rules, and the `read`, `list`, `search` lowerings | Files for C1 compile; files that break each rule above do not |
| 3 | The driver for reads: prepare, `spawn`, `expect`, `parse`, stopping | C1, C2, C9 to C12, and C14 on three systems |
| 4 | D3: a read through the same engine loop | The same trace signature across five runs |
| 5 | D4: the H-26 count | A hypothesis result file |
| 6 | Writes: `create`, `update`, `delete`, read-back, identity, secrets | C3 to C8, C13, and the fault tests of every write |
| 7 | The discoverer and drafting rules | A scout of `example-tool` drafts C1's commands and tries none |

## Open points

- The step payloads, fixed with step 2, and whether `collect` belongs to this archetype or to the file pack of section 70.
- Programs on Windows that parse their command line by their own rule: how to declare it and prove it.
- How long a digest may be trusted between starts. Taking it every start is safe and slow for large programs; starting the opened, checked file itself, where the platform allows it, closes the gap between check and start.
- How a person signs a program in when it keeps its own sign-in and asks for it interactively.
- Whether the terminal kind becomes an interface mode of this pack, as section 62 asks.
- Confinement: which operating-system mechanism could make a command's file and network reach `confined` rather than `declared`.
