# 0045. The workspace starts with every crate, and the audit enforces the layering

**Status:** Accepted. The owner was shown the plan and its alternative on 2026-11-16 and told the agent to go ahead.

**Date:** 2026-11-16

## Context

Slice 0.1 creates the Cargo workspace. Spec section 29 lists 23 crates and the technical reference puts them in eight layers. None has code yet. Phase 0 task A3 asked for empty crates "with their dependency edges", but the table gives the edges that are allowed, not the ones that exist.

## Decision

- All 23 crates exist from the first commit, each empty.
- A crate declares a dependency on another crate only when code needs it.
- `tools/docs_audit.py` enforces the layering. The crates on disk, the layering table, and the section 29 layout must name the same crates, and no crate may depend on a crate in its own layer or a higher one, in any dependency table. The table in the technical reference is the only copy of the rule. `tools/test_docs_audit.py` plants each mistake and checks that it is caught.
- The toolchain is pinned to Rust 1.100.0, the stable release on the day. Workspace lints deny `unsafe`, `unwrap`, and `expect`; tests may use the last two.
- CI uses one outside action, `actions/checkout`, pinned by commit at v7.0.1 and licensed under MIT. The toolchain is installed by the runner's own `rustup` from `rust-toolchain.toml`.

## Consequences

- The layout in the documents and the layout on disk are compared by a script, with no list of exceptions.
- The check is Python, like the audit it lives in. It is tooling, off the path of a run, so no new language is introduced.
- An edge the table forbids fails CI and forces a recorded choice: change the code, or change the table. Hypothesis H-25 counts how often that happens.
- Empty crates say nothing about whether the boundaries are right.
- Moving to a newer toolchain is a one-line change that CI proves on three operating systems.

## Alternatives considered

- Start with two or three crates and split when a boundary has proven itself: fewer boundaries drawn too early, but the layering rule would hold nothing until late, and merging two crates is easier than untangling one.
- Declare every allowed edge now: a dense graph that hides which dependencies are real.
- A Rust task for the check: a second copy of the table or a parser for the document, and the first outside dependencies, for no gain yet.

## Revisit if

H-25 shows real code being refused by the table more than a few times before the end of Phase 3, or the check needs facts that only `cargo metadata` can give.

## Spec sections

- [29-repository-crates-and-packaging.md](../spec/29-repository-crates-and-packaging.md)
- [The technical reference](../reference/technical-reference.md)
