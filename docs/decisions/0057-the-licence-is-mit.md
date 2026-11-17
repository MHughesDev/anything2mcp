# 0057. The licence is MIT

**Status:** Accepted. Owner decision.

**Date:** 2026-11-17

**Supersedes:** [0018](0018-license-apache-2-0.md).

## Context

The design chose Apache-2.0 for its patent grant, on the owner's delegation. The owner has now decided: the project is "completely open source with an MIT license", and the building agent is to handle the copyright details.

## Decision

- The whole repository is under the MIT licence: the code, the documents, and every package built from it.
- The copyright line is "The Anything2MCP Authors". It names no one person, and it covers the owner and everyone whose work is merged.
- A contribution is offered under the same licence as the project. Opening a pull request says so; nothing is signed. [CONTRIBUTING.md](../../CONTRIBUTING.md) states it.
- The closed list of licences a dependency may carry does not change: MIT, Apache-2.0, BSD-3-Clause, and Unicode-3.0. A new permissive licence is added to `deny.toml` on purpose, with a line in the journal. A licence that is not permissive is not added by an agent.
- Releases built for people to install carry the licence texts of what they contain. That is a task of the first real release.

## Consequences

- MIT gives no explicit patent grant. The owner chose the shorter, more widely understood terms.
- Two placeholder packages were published under the earlier names with an Apache-2.0 notice. They stay as published.
- `LICENSE`, the workspace manifest, and the pip package change in the same commit as this record. The audit already checks that the pip package's copy of the licence is identical.

## Alternatives considered

- Apache-2.0, as before: the patent grant, and a longer text with a notice file to maintain.
- MIT or Apache-2.0 at the user's choice, as most Rust crates do: the most permissive for users, and two texts where the owner asked for one.

## Revisit if

A contributor or a company that wants to use the project asks for a patent grant.

## Spec sections

- [29-repository-crates-and-packaging.md](../archive/spec-2026-11/29-repository-crates-and-packaging.md)
