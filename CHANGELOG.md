# Changelog

Everything a user of Anything2MCP would notice, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as [RELEASING.md](RELEASING.md) applies it.

The design's own history is in the [decision records](docs/decisions/README.md), and the work session by session is in the [journal](docs/journal/README.md).

## [Unreleased]

Nothing usable yet. The project is in its foundation phase.

### Added

- A Cargo workspace of 24 crates: a neutral kernel (`a2m-*`), packs that each know one kind of system (`a2m-pack-*`), and the product crate.
- The `anything2mcp` command, and `a2m` as a short name for it. Both report the version and nothing else.
- A test application with a web interface, a JSON interface, a control interface, and injectable faults.
- Continuous checks on Windows, macOS, and Linux: formatting, lints, build, tests, a closed list of dependency licences, and a documentation audit.
- A release workflow that builds the pip package repeatably and publishes it with short-lived credentials, from a version tag or a manual start.
- The design: 67 specification sections, 59 decision records, 16 research notes, and a register of 31 hypotheses.

### Changed

- The project is named Anything2MCP. It was first designed as Web2MCP, a tool for wrapping applications; it is now a toolkit for any kind of system, built on the same core. Crates, the command, the packages, and the built-in tool prefix were renamed to match.
- The licence is MIT. Earlier work in this repository was marked Apache-2.0.

## [0.0.1]

A placeholder to hold the package name. It contains no working software. Not yet published.

[Unreleased]: https://github.com/MHughesDev/anything2mcp/commits/main
[0.0.1]: https://github.com/MHughesDev/anything2mcp/tree/main/bindings/python
