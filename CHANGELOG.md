# Changelog

Everything a user of Anything2MCP would notice, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as [RELEASING.md](RELEASING.md) applies it.

The design's own history is in the [decision records](docs/decisions/README.md), and the work session by session is in the [journal](docs/journal/README.md).

## [Unreleased]

Nothing usable yet. The project is in its foundation phase.

### Added

- A Cargo workspace of 29 crates: a neutral kernel (`a2m-*`), packs that each know one kind of system (`a2m-pack-*`), and the product crate.
- The `anything2mcp` command, and `a2m` as a short name for it. Both report the version and nothing else.
- The kernel's shared vocabulary in `a2m-core`: archetypes, effect classes, hazards, failure classes, evidence levels, and the states of runs, records, sessions, and bindings, with a secret type that cannot be printed or serialized.
- Findings in `a2m-core`: what discovery returns, with a subject that can be told apart from any other, a level that cannot exceed what its source supports, and an identifier that is a digest of its content.
- Expressions in target files: a typed subset of CEL with no floating-point numbers. A decimal is written `decimal("1.5")` and takes `+`, `-`, `*`, and comparisons; division is `div(a, b, places, rounding)`.
- The vault: saved state is sealed with an authenticated cipher under a key held in the operating system's key store, with a passphrase route for machines that have none. On Linux this build has no operating-system key store and uses the passphrase route ([decision 0123](docs/decisions/0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md)).
- The redactor masks registered secrets in their encoded forms as well as their plain form.
- Target files load, lint, and compile to a hashed plan. The `example` target shows every file kind the kernel owns.
- Source readers for `csv`, `json`, and `xlsx`, with exact decimals and template fingerprints: an input whose structure matches no known template version is refused.
- The policy engine: nothing is allowed unless a rule allows it, a write needs a person's approval unless a pre-approval rule covers it, and a kill switch stops everything, a workspace, a target, or an operation.
- Sessions: one renewer for each session, and built-in sign-in providers for keys, tokens, basic and digest credentials, client certificates, and OAuth.
- A test application with a web interface, a JSON interface, a control interface, and injectable faults.
- The engine loop: a tool runs as a read, or as a verified write that records its intent first and reads the value back. A resume reads before it writes, so a write whose answer was lost is never sent twice.
- The action registry: the first 26 of its actions are served, through one dispatcher that records a policy decision for every call. Settings load in layers, with presets; an unknown key is refused, and no layer above the installation can loosen a protected setting.
- The REST interface: every action at one route, five resource aliases, and a generated OpenAPI 3.1 document.
- The HTTP pack: tools over a JSON interface, with verified writes. An OpenAPI description can be imported to draft endpoint entries. HTTPS is not in this build.
- The command pack: a program can be a target. Arguments are typed, and no shell is used.
- The data pack: the `sql` mode over an embedded database file.
- The file pack: named places in delimited text, JSON, and workbooks. A write replaces the whole file in one step.
- The stream pack: bounded reads, a look that takes nothing, and keyed publishes. No standard broker protocol is in this build yet.
- The device pack: points over a Modbus TCP link, with ranges enforced before a frame is sent.
- The MCP pack: an upstream MCP server is reached through a mapping file, and a changed upstream tool is detected as drift.
- The browser engine: it starts an installed Chrome, Edge, or Chromium over the debugging pipe, locates elements by role and name, and has an observing tab that cannot write.
- Continuous checks on Windows, macOS, and Linux: formatting, lints, build, tests, a closed list of dependency licences, and a documentation audit.
- A release workflow that builds the pip package repeatably and publishes it with short-lived credentials, from a version tag or a manual start.
- The design: a specification of 59 sections in seven parts, 113 decision records, 19 research notes, and a register of 40 hypotheses.
- The design of configuration: eight layers, twelve scopes, presets, a group of settings for every pack, and the rules that no setting can loosen.
- The bar: what being the best of its kind means, as twelve things that can be measured, with a scorecard.
- The architecture: which crate defines, implements, and joins every interface, with a design for each of the 29 crates, the command, data, file, stream, and device packs among them.
- A roadmap of every work package to version 1.0.

### Changed

- Target files: `auth.yaml` gains `settings` and `probe_inputs`, a web surface's viewport is a field of the surface, and a target's time zone is `UTC` or a fixed offset.
- The specification is rebuilt around the product as it is: a neutral kernel and packs. Sections written for the first design were consolidated, and the originals are kept in `docs/archive/`.

- The project is named Anything2MCP. It was first designed as Web2MCP, a tool for wrapping applications; it is now a toolkit for any kind of system, built on the same core. Crates, the command, the packages, and the built-in tool prefix were renamed to match.
- The licence is MIT. Earlier work in this repository was marked Apache-2.0.

## [0.0.1]

A placeholder to hold the package name. It contains no working software. Not yet published.

[Unreleased]: https://github.com/MHughesDev/anything2mcp/commits/main
[0.0.1]: https://github.com/MHughesDev/anything2mcp/tree/main/bindings/python
