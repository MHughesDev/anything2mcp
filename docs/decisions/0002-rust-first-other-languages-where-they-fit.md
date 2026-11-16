# 0002. Rust first, other languages where they fit

**Status:** Accepted. Decided by the project owner.

**Date:** 2026-11-15

## Context

The system must be fast, install as one file, run on Windows, macOS, and Linux, and scale from a laptop to a server. An earlier draft chose Python. Some parts cannot or should not be Rust: pages that run in a browser, scripts evaluated inside a target page, and SDKs for callers.

## Decision

Rust is the default language and is required for everything on the path of a run: control plane, runners, drivers, ledger, policy, and vault. The system ships as one binary. Another language is used where it is clearly the better tool: TypeScript for the web pages, small JavaScript for in-page scripts, Python and TypeScript for SDKs, shell and PowerShell for installers, SQL for migrations, and any language for plug-ins.

## Consequences

- No official Playwright or MSAL library exists for Rust, so the browser engine and parts of sign-in are new work. See ADR 0004.
- Exact decimal arithmetic, spreadsheet reading, and CEL evaluation need Rust crates chosen in Phase 0.
- A new use of another language needs its own decision record, so the choice stays deliberate.

## Alternatives considered

- Python core: faster to start, slower to run, harder to ship as one file.
- TypeScript core: first-class Playwright and MCP support, but a Node runtime everywhere.
- Rust only: rules out browser pages and in-page scripts, and makes SDKs harder to adopt.

## Revisit if

The Phase 0 browser-engine spike fails on all three candidate bases.

## Spec sections

- [29-repository-crates-and-packaging.md](../spec/29-repository-crates-and-packaging.md)
- [09-performance.md](../spec/09-performance.md)
