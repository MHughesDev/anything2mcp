# 0025. pip and npm packages ship the binary with a thin SDK; no in-process bindings yet

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

The owner wants a pip module that a coding agent can install and use. The engine must stay in Rust. The wheel-building tool can package a Rust binary directly, and warns that shipping a binary and a native library together doubles the package.

## Decision

The pip package holds the compiled binary and a pure-Python SDK; the npm package is a launcher with one binary package per platform and a TypeScript SDK. SDKs are generated, hold no engine logic, and reach a local engine through a bridge subprocess or a remote server over HTTPS. In-process Python and Node bindings are deferred. npm packages run no install script.

## Consequences

- `pip install web2mcp` needs no Rust toolchain and gives the command, the engine, and the import.
- The same SDK code runs against a laptop engine and a company server.
- Hosts that need the engine in-process use the Rust crate until bindings exist.

## Alternatives considered

- Bindings that load the engine into Python: faster calls, larger package, engine state tied to one process.
- An SDK that downloads the binary on first use: breaks offline and locked-down installs.

## Revisit if

A host application needs in-process calls and the Rust crate does not serve it.

## Spec sections

- [41-packaging-and-release.md](../archive/spec-2026-11/41-packaging-and-release.md)
- [22-front-doors.md](../archive/spec-2026-11/22-front-doors.md)
