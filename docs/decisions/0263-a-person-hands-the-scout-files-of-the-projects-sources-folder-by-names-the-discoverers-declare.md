# 0263. A person hands the scout files of the project's sources folder, by names the discoverers declare

**Status:** Accepted. Design decision. It changes the provider manifest, a public contract. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the [design of `a2m-provider`](../architecture/crates/a2m-provider.md) (the manifest and the registry), the [design of `a2m-engine`](../architecture/crates/a2m-engine.md) (the sandbox and `Control`), and the manifests of `a2m-pack-http` and `a2m-pack-device`.

## Context

A person often holds a file that describes a system: an HTTP interface's published description, or a device's published register map. The HTTP pack's description reader and the device pack's map reader both read such a file from `DiscoveryScope::paths`. The engine handed a discovery job only the project folder, so in the product neither reader could be given a file (round 20 of the idea rounds). Handing a file is an input that names a path, so it must hold to [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md): no input may choose a file outside what the person set aside.

## Decision

1. **A discoverer declares the names it reads.** `[[discoverers]]` gains `files`, a list of names of lowercase letters, digits, and underscores, starting with a letter, at most 32 bytes, each listed once; left out, none. `DiscovererDecl::files` carries it. `Registry::files_taken(&SurfaceDescription)` gives every name that the discoverers suiting a surface declare. This is a change to the provider manifest: a build before this one refuses a manifest that lists `files`, because the manifest has no unknown keys. Old manifests read as before, so `CONTRACT` stays 1; no plug-in exists yet that could be refused.
2. **The two packs declare one name each.** The HTTP pack's `http_description` reads `description`. The device pack's `device_register_map` reads `register_map`, and now reads only the path handed under that name, when its file name ends `.csv`; before, it read every `.csv` path it was handed.
3. **The engine checks each file again, before anything runs.** A discovery job is handed its files by `Control::discovery_files`: each name, to the path as the call wrote it, `sources/` and a file below it. Before policy is asked or a driver is held, the engine makes a root of the project's `sources` folder for reading only, joins each path under it by `CheckedPath::join` (an absolute path, `..`, and a link out of the folder are refused), refuses a part that starts with a dot, and requires a regular file. Each failure is `input`, `path_not_allowed`. A name that no discoverer of the surface declares is `input`, `job_invalid`, because a file nobody reads is a mistake (rule 7). One refusal ends the job; no discoverer runs and nothing is read. A checked file is added to the scope's paths under its name. Nothing else can add a path: not a discoverer, not the focus, not a finding.
4. **The HTTP pack's fix** for a description it cannot read tells the person to put the file in the project's `sources` folder and run `scout run` with `--files '{"description": "sources/<file>"}'`.

## Alternatives weighed

- **A field `JobSpec.files`**, as the plan named it. `JobSpec` is data of the job, and the files are part of the stored call. But `JobSpec` is built field by field in `a2m-actions` (`runner.rs`, `offline.rs`), which another agent holds now, and stable Rust has no default value for a field, so a field would break both. The project folder reaches the engine through `Control` for the same reason, and the files are its neighbours: data the control plane checked about the request. `Control::discovery_files` has a default, so every caller compiles unchanged. If `JobSpec` is ever built through a constructor, the files can move there.
- **The device reader keeps reading every `.csv` path.** Its declaration would then not say what it reads. It reads only its declared name.
- **No name check in the engine,** leaving it to the action. The action checks first; the engine checks again because the check costs nothing, and a name nobody reads would otherwise end as a quiet run.
- **The discovery focus names the file.** The focus is handed to every discoverer as data, and a discoverer would then turn an input into a path. Not taken: paths are made by the kernel alone.

## Consequences

- Owed to `a2m-actions`: the `files` input of `scout.run` and `discover.run`, its check under `sources/`, the refusal of a name `files_taken` does not give, keeping the files in the stored call and the scout's state, and `discovery_files` on its `Control`. Then `spec.import` as `scout.run` with `files = {description: file}`.
- The fix text of point 4 names an option that exists once `a2m-actions` has the `files` input.
- The file pack's discoverer declares no `files`, so no file is handed to it; it reads its surface's folder (decision 0255).

## Three systems

| Matter | Proven here | Only CI proves |
| --- | --- | --- |
| A link out of `sources` is refused | Linux | Windows, where the test makes a link only for a user who may, and skips the case otherwise |
| Paths with forward slashes, joined by `CheckedPath::join` | Linux; the code compiled and linted for Windows and macOS | Both, run |

## The registry's half, as built

Added on 2026-11-18, when `a2m-actions` took the files the consequences above owed it.

1. **`files` on `scout.run` and `discover.run`.** A map from a name a discoverer reads to a file's path from the project folder, as the call writes it: `--files '{"description": "sources/example-openapi.json"}'`. Each path is checked when asked, by the same code that checks a source file of `tool.call`: it must be text, inside `sources/` with no part that starts with a dot, joined under a checked root of the project folder (an absolute path, `..`, and a link out of the project are refused), a regular file (a link is not followed at its end), and at most the smaller of `files.max_upload_bytes` and the engine's source limit. A path that fails is `input`, `path_not_allowed`; a file that is not there or is too large, `input`, `file_unavailable`. Nothing is read.
2. **A name nobody reads is refused before anything starts.** The names taken are the union of `Registry::files_taken` over the surfaces the call would observe: those whose kind an enabled provider serves and that a discoverer suits. A name outside it is `input`, `discovery_file_not_taken`, a new code of this crate, and the fix lists the names that are taken. Both checks come before a scout is stored or a run is submitted, so a refused call leaves nothing behind.
3. **Each surface is handed only what it reads.** A target with two surfaces of different kinds may be handed files for both; each surface's discovery is handed the files under the names its own discoverers declare, so the engine's own check never meets a name its surface does not read.
4. **The files are part of the stored call.** A discovery run's stored input holds them as a record `files`, so they are part of its input hash: a discovery handed other files is another call. A discovery handed none stores none, so its hash is what it was before this change. The runner decodes the record, and `InProcessControl::discovery_files` returns it to the engine unchanged; a record this build cannot read ends the run as unreadable, never as a run with no files.
5. **A scout keeps its files.** Stage 0 of a scout stores them, and the scout's report shows them under `files`. An unfinished scout goes on with the files it began with, each checked again. A call that names other files than the unfinished scout began with starts a new scout instead of resuming it.
6. **`spec.import` is a scout.** It is `scout.run` with `fresh` and `files = {description: <file>}`, and has no logic of its own. Its catalog entry now says what `scout.run` says of itself: it observes, runs long, takes `wait_seconds`, and never writes to the system; before, it was defined as a write that no handler served. It reaches only a discoverer that reads `description`, which today is the HTTP pack's description reader; an upstream MCP server's tool list ([section 304](../spec/3-packs/304-the-mcp-pack.md)) is not reached through it.

Weighed and not taken:

- **Resuming the unfinished scout with the call's new files.** Its stages after the first were made from what it was handed then, so the report would mix two inputs. **Refusing the call** instead would make a person pass `fresh` to say what the files already say.
- **Refusing a name that one observed surface does not read**, rather than one that none reads. A target with an HTTP surface and a device surface could then be handed no file at all.
- **Checking the names in the engine alone.** The engine checks them, but only once a run exists; the registry refuses before one does, and its refusal can name the names taken.

| Matter | Proven here | Only CI proves |
| --- | --- | --- |
| A description handed as a file is read by the scout and drafted from; a register map handed as a file is read by the device scout; `spec.import` drafts the same way | Linux, `scout_files.rs` | Windows and macOS, run |
| A path outside `sources`, an absolute path, `..`, a hidden file, a link out, and a link to a file of the project are refused, and no run starts | Linux; the code compiled and linted for Windows and macOS | Windows, where the test makes a link only for a user who may, and skips the two link cases otherwise |
