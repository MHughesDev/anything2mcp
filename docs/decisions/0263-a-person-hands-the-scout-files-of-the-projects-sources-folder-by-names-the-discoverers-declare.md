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
