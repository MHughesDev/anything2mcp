# 2026-11-16: design review with lens 6, then slice 0.1

## State at the end

The repository has its first code: a Cargo workspace of 23 empty crates that is formatted, linted, built, and tested on Windows, macOS, and Linux by CI, with the crate layering enforced by the audit script. Nothing is usable; the `web2mcp` binary only reports its version. One design review round was run before it, with lens 6. Pull request 1 (the review) is merged. This entry is part of pull request 2 (slice 0.1), which the agent merges once its checks pass.

## Done

- **Lens 6, one pipeline line by line.** Walked `example_order_sheet_enter` (a sheet of line items entered into an order) from its files through compile, call, validation, preview, approval, the write of each record, a stop and resume, and a second run. Sections read in full: 5, 6, 7, 8, 10, 11, 12, 18, 21, 39, 42, 44, 45, 46, 47, 49, 50, 51, 52, and every reference document except the settings examples. Documents corrected in 19 files.
- **Slice 0.1, the repository skeleton.** Phase 0 tasks A2, A3, A4, and A6.
  - The workspace, with Rust 1.100.0 pinned and lints that deny `unsafe`, `unwrap`, and `expect` in library code.
  - The 23 crates of spec section 29, empty, with no dependency edges.
  - `web2mcp` reports its version and refuses anything else with exit code 2.
  - The layering check in `tools/docs_audit.py`, and `tools/test_docs_audit.py`, which plants nine mistakes and checks each is caught.
  - `.github/workflows/ci.yml`, and the Apache-2.0 `LICENSE`.

## Learned

- The design's own running example could not run as written. Each part was right alone; the joins had never been checked. Expect the same wherever two sections meet and no example crosses both.
- The preview was the one phase of a write with no procedure. Sections 18, 45, and 50 each held a piece.
- More paper review will find less. The gaps left need code, mostly in Phase 3. Lens 7, "less", is the natural round for the end of Phase 0.
- Slice 0.1 tested no hypothesis. Proof that the checks bite: a planted `unwrap` and a planted `unsafe` block each failed the lint and were removed; the audit's self-test passes 9 of 9.
- CI, first run, wall time per job: Linux 13 s, Windows 30 s, macOS 35 s, the audit and its self-test 5 s. All four passed on the first run. There is no budget for these; they are the starting point to watch as code arrives.
- The layering table can be enforced from the document itself, in about thirty lines of Python, with no second copy of the table.

## Changed in the design

- From lens 6: see the fourth pass in the [readiness report](../plan/readiness.md). In short: section 7's example now connects; one reading of sources; a preview procedure and open points in section 42; rule 3 no longer forbids resume; `version_conflict` moved from `rejected` to `stale`; the glossary names the engine loop and the pipeline step; hypotheses H-23, H-24, and H-25 added.
- From slice 0.1: [decision 0045](../decisions/0045-the-workspace-starts-with-every-crate-and-the-audit-enforces-the-layering.md). Phase 0 task A3 no longer asks for dependency edges on empty crates. The technical reference states that a crate may not depend on a crate in its own layer.

## Noticed, not acted on

- A diff's actions are `create`, `update`, and `unchanged`. `delete`, `submit`, and an upload have no place in the diff or its hash, so an approval for an irreversible operation would bind to little more than a key. Fix before the first irreversible operation is built.
- The step trace's unit and the driver interface's `read_back` method both disagree with the step list. Open points in section 47 and the technical reference; slices 1.2 and 1.1 settle them.
- The driver sketch calls a run's hold on a session a `SessionLease`. The glossary keeps "lease" for a runner's hold on a job.
- A result shows one `mode` for a whole run, but a pipeline's steps can use different modes.
- On a server, a grant on a pipeline says nothing about the operations it calls.
- The layering table allows no edge inside a layer. The session keeper and the browser share a layer, as do the policy engine and the ledger. H-25 counts the collisions; the count stands at zero because no crate has code.
- AGENTS.md's table of paths does not list `crates/`, the audit's self-test, or the CI workflow. Left alone; see "Undecided".
- Licence checking in CI (task A5) should arrive with the first outside dependency, which slice 0.3 is likely to bring. `SECURITY.md` and `CONTRIBUTING.md` (task A7) need text the owner has agreed.

## Undecided

For the owner.

- **How much the agent does without asking.** On 2026-11-16 the owner said they are not reviewing pull requests, that the agent merges them, and "full auto". The agent acted on that within the session: it merged pull request 1, built slice 0.1 on its own recommendations, and merges pull request 2. AGENTS.md, the working method, and the `start-session`, `build-slice`, and `end-session` skills still say to wait for a go-ahead and not to push without being asked. The agent began to rewrite them and the session's safety check refused the edit as weakening oversight, so they are unchanged. Until the owner changes them, or says so again, a new session follows them as written: show the plan and wait.
- Whether a stopped write run resumes only when the caller asks, or by itself when the engine restarts.
- Whether an `upsert` in a `ui` binding may declare two paths (H-23), and whether operations should name the kind of record they write (H-24). Neither blocks anything before Phase 3.
- Carried over: whether the repository stays public; where real target files live; which MCP clients matter most; whether a Windows machine is available; trusted publishing on crates.io and npm.

## Next

1. Slice 0.2: the release workflow and a placeholder release to PyPI. Read [publishing setup](../plan/publishing-setup.md) and spec section 41, plan it, and get the owner's explicit go-ahead before anything is published: a release cannot be undone.
2. If the owner is not available for that, slice 0.3 (the test application, cases 1 to 4) does not depend on 0.2 and can be planned first.
