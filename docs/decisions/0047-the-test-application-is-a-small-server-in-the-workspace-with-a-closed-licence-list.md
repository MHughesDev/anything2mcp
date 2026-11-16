# 0047. The test application is a small server in the workspace, and the licence list is closed

**Status:** Accepted by the agent, under the owner's instruction of 2026-11-16 to keep working without review. The owner had not read this record when it was merged.

**Date:** 2026-11-16

## Context

Slice 0.3 builds the first cases of the test application that every later test depends on ([the test application plan](../plan/test-application.md)). It also brings the first outside crates, so the rule that every dependency is verified and permissively licensed has to become something a machine checks.

## Decision

**The application.**

- It is one crate, `example-app`, in `targets/example-app/`. It is a member of the workspace and outside the crate layering. A product crate may use it as a dev-dependency only, and the audit script refuses anything else.
- It is built on tokio and axum, the stack the product plans to use, with plain pages and no build step.
- Tests start it in-process on a free port through its harness. A binary exists for people and for tools that are not Rust tests.
- Its control interface is ordinary HTTP under `/__control`, so that it is the same from a Rust test, a browser, or another language. Faults never apply to it.
- Everything a test can observe is repeatable. The endpoint that fails some of the time fails every third call since the last reset; no fault is random.
- Lengths are whole sixteenths of an inch inside the application and decimal text in its JSON, so the test application never holds a business value as a float.
- An update must carry the version it read, and a create must carry none.

**The licence list.**

- `cargo-deny` runs in CI against `deny.toml`: licences, security advisories, yanked versions, and sources other than crates.io.
- The list of allowed licences is closed: MIT, Apache-2.0, BSD-3-Clause, and Unicode-3.0. A crate under anything else fails the build until the list is changed on purpose.
- Unicode-3.0 is on the list as a permissive licence in the sense of AGENTS.md. It covers the Unicode data crates that `reqwest` brings in. The owner has not been asked about it separately.
- Versions of outside crates are written once, in the workspace's `Cargo.toml`.

## Consequences

- The product's planned web stack is proven to build and run on three operating systems before the product uses it.
- The dependency tree goes from nothing to about a hundred crates, most of them from `reqwest`. CI takes longer; the first numbers are in the journal.
- A new advisory against a dependency fails CI on an unrelated change. That is intended: someone has to look.
- The pages' script has no automated test until the interface driver exists. It was driven once by hand in a browser.
- The application's shape for a record that does not exist, a separate address for the empty form, makes hypothesis H-23 concrete.

## Alternatives considered

- A server written on the standard library with no outside crates: nothing to verify or license, but hand-written HTTP and JSON handling would be the least trustworthy part of the test equipment.
- A harness that reads the application's memory directly instead of calling the control interface: faster, but then the control interface would be untested and useless from outside Rust.
- A lighter HTTP client than `reqwest` in the harness: fewer crates now, but the product needs an HTTP client of this kind in Phase 4, and this verifies the candidate early.
- An open licence policy that only names what is forbidden: a new, unfamiliar licence would pass unnoticed.

## Revisit if

A case needs something plain pages cannot imitate before Phase 8, or the dependency tree's build time becomes a cost worth cutting.

## Spec sections

- [29-repository-crates-and-packaging.md](../spec/29-repository-crates-and-packaging.md)
- [The test application plan](../plan/test-application.md)
- [Research note 9](../research/09-rust-building-blocks.md)
