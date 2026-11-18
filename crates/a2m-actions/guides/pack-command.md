name: pack-command

# The command pack, mode `cli`

**Latches onto.** A command-line program installed on the machine the engine runs on. Surface kind: `program`, with `path`. The program is started by absolute path, checked by digest and version first. It gets no shell and no terminal.

**File kind.** `commands/`: one file per program, with its digests, version check, environment, limits, exit codes, and each command.

**Verbs.** `read`, `list`, `search`, `create`, `update`, `delete`, `call`.

**Steps.** `spawn`, `expect`, `parse`.

## A command file, from the `example` target

```yaml
surface: tool
program:
  digests:
    linux-x86_64: ["sha256:..."]      # write the whole digest of each build
  version:
    args: ["--version"]
    expect: {exit: [0], stdout: '^example-cli (?<version>\d+\.\d+)\.\d+$'}
    supported: ["2.4", "2.5"]
  help: [["--help"], ["help", "{command}"]]
  env:
    set: {LC_ALL: C.UTF-8, TZ: UTC}
    pass: []
  workdir: fresh
  stdin: none
  limits: {timeout: 20s, stdout: 1MiB, stderr: 64KiB}
  exits: {0: ok, 2: input, 4: rejected, 5: stale, 6: busy, 75: transient, 77: session_expired}
commands:
  item_show:
    effect: read
    params: {sku: {type: identifier, pattern: '^[A-Z]{2}-\d{4}$'}}
    args: [item, show, {param: sku}, --format=json]
    exits: {3: absent}
    output:
      json:
        fields: {sku: text, quantity: integer, version: integer}
```

Arguments are a list. A value goes in only as `{param: name}` or `{flag: --name, param: name}`. A binding of mode `cli` names a `command`, and a write names `read_back: {command: item_show, key: [sku]}`. A program that assigns the key uses `identity: {strategy: stamped}`, a `{flag: --ref, stamp: true}` argument, a `lookup` command, and `assigns` in the command.

## Rules

- Every exit status the program can give is in `exits`. An unlisted one is `unexpected_exit_status`.
- Output is parsed by a declared shape: `json`, `table`, or a `pattern`. A parse failure is `output_unparsed`.
- A write command should say which exit statuses mean nothing changed (lint `command.write_exit_declared`).
- A command with a hazard other than `none` is `irreversible`, and a command that reaches a network says where with `reaches`.
- Each run gets a fresh working folder, which is removed after.

## Refuses

- A shell, an interpreter, or any route through one.
- A program that runs an argument as code, or one that starts the program its arguments name.
- Finding a program on the search path, or a path from an input.
- A program, or a folder on its way, that another user can write.
- A terminal. Attaching to a running program.
- A prompt it did not expect (`unexpected_prompt`).

## Codes

`drift`: `program_missing`, `program_digest_changed`, `program_version_unsupported`, `output_unparsed`. `policy`: `program_path_unsafe`, `limit_exceeded`. `input`: `argument_unsafe`, `schema_invalid`, `step_not_usable`. `rejected`: `app_validation_error`. `stale`: `version_conflict`, `record_absent`. `busy`: `program_busy`. `transient`: `upstream_unavailable`. `unknown_variant`: `unexpected_exit_status`, `unexpected_prompt`. `possible_commit`: `ambiguous_match`.
