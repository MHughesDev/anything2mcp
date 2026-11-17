# 0016. Plug-ins are separate processes

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

New drivers, auth providers, readers, and widget drivers should be addable without touching the core, in any language, without putting the vault at risk.

## Decision

A plug-in is a separate process that speaks a small JSON-RPC protocol over standard input and output. It declares capabilities, receives only the inputs for its step, and never gets raw vault access. Unsigned plug-ins are off by default.

## Consequences

- Isolation and language freedom, at the cost of some speed per call.
- Performance-critical drivers stay in-tree in Rust.

## Alternatives considered

- In-process dynamic libraries: faster, but unsafe and Rust-only.
- WebAssembly components: worth revisiting once the plug-in interface is stable.

## Revisit if

Process overhead makes plug-in drivers impractical.

## Spec sections

- [29-repository-crates-and-packaging.md](../archive/spec-2026-11/29-repository-crates-and-packaging.md)
