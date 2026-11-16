# 2026-11-16: design review with lens 6, and a plan for slice 0.1

## State at the end

Still a design and no code. One design review round was run, with lens 6, and the documents were corrected in 19 files; the audit reports no findings. Slice 0.1 is planned and was shown to the owner. It is not started: the owner's go-ahead had not been given when this entry was written. The session's changes were committed and pushed to the branch `ccr-4721dcb5-syl1kq` when the repository's stop hook asked for it.

## Done

- **Lens 6, one pipeline line by line.** Walked `example_order_sheet_enter` (a sheet of line items entered into an order) from its files through compile, call, validation, preview, approval, the write of each record, a stop and resume, and a second run. Sections read in full: 5, 6, 7, 8, 10, 11, 12, 18, 21, 39, 42, 44, 45, 46, 47, 49, 50, 51, 52, and every reference document except the settings examples.
- **A plan for slice 0.1,** below under "Next". No product file was created.

## Learned

- The design's own running example could not run as written. Its pipeline passed no inputs, its screen could not reach one line item, its save was not a commit point, and its key left out the order. Each part was right alone; the joins had never been checked. Expect the same wherever two sections meet and no example crosses both.
- The preview was the one phase of a write with no procedure. Sections 18, 45, and 50 each held a piece.
- Returns from this lens were good: about a dozen fixes and as many gaps that paper cannot close. Another paper round would find less. The gaps left are of the kind that need code, mostly in Phase 3, so the next gain is from building. Lens 7, "less", is the natural round for the end of Phase 0.

## Changed in the design

Fixed in the documents, all as clarifications where two documents disagreed or a step was missing. The [readiness report](../plan/readiness.md) has the full table.

- Section 7's worked example now connects end to end and agrees with the file format reference.
- One reading of sources: a source file maps columns to row fields; a pipeline step passes them to inputs with `with`; the sheet arrives as a reserved `source` input.
- File formats: placeholders take inputs by name; `read_back` is also the lookup used by the preview, the freshness check, and reconcile; a list `key` is `composite`; the module part of a tool name is optional; `checkpoint` only places snapshots.
- Section 42: "The preview, step by step"; a run with nothing to change needs no approval; rule 3 no longer forbids resume; rule 6 hashes a file's content; an "Open points" list.
- Sections 10 and 11 follow ledger rule 6. Section 8 names the engine loop's unit and places the record lock in it. Section 39 gives a crashed run a real state. Section 46 counts actions marked `commits`.
- `version_conflict` moved from `rejected` to `stale`. This changes the response to that failure; the owner was told.
- Glossary: "engine loop" and "pipeline step" added; "step" alone is one driver instruction.
- Hypotheses H-23 (one `ui` binding can carry out an upsert), H-24 (a record key needs no record kind), and H-25 (the crate layering holds) added, all open.
- The audit script now refuses four phrases this round replaced, and lets journal entries quote old wording.

No decision record was added. No accepted decision was changed.

## Noticed, not acted on

- A diff's actions are `create`, `update`, and `unchanged`. `delete`, `submit`, and an upload have no place in the diff or its hash, so an approval for an irreversible operation would bind to little more than a key. Outside the pipeline walked; fix before the first irreversible operation is built.
- The step trace's unit and the driver interface's `read_back` method both disagree with the step list. Open points in section 47 and the technical reference; slices 1.2 and 1.1 settle them.
- The driver sketch calls a run's hold on a session a `SessionLease`. The glossary keeps "lease" for a runner's hold on a job.
- A result shows one `mode` for a whole run, but a pipeline's steps can use different modes.
- On a server, a grant on a pipeline says nothing about the operations it calls.
- The layering table allows no edge inside a layer. The session keeper and the browser share a layer, as do the policy engine and the ledger. H-25 will count the collisions.
- Phase 0 task A3 asks for empty crates "with their dependency edges". The table gives allowed edges, not real ones. The slice 0.1 plan declares none until code needs one; correct A3 when the slice is built.

## Undecided

For the owner. New this session:

- The go-ahead for slice 0.1, and its two choices: all 23 crates now or only crates with code; the layering check inside the audit script or as a Rust task.
- Whether a stopped write run resumes only when the caller asks, or by itself when the engine restarts.
- Whether an `upsert` in a `ui` binding may declare two paths, one for a record that exists and one for a record that does not (H-23), and whether operations should name the kind of record they write (H-24). Neither blocks anything before Phase 3.

Carried over, still open: whether the repository stays public; where real target files live; which MCP clients matter most; whether a Windows machine is available; trusted publishing on crates.io and npm.

## Next

1. With the owner's go-ahead, build slice 0.1 with the `build-slice` skill, to this plan:
   - A Cargo workspace: pinned stable toolchain, format and lint settings, workspace lints that deny `unsafe`, `unwrap`, and `expect` in library code, and the Apache-2.0 `LICENSE`.
   - The crates of spec section 29 as empty crates with no dependency edges yet; `web2mcp` prints its version.
   - A layering check that reads the table in the technical reference, compares it with section 29 and with the crates on disk, and refuses an edge that does not point to a lower layer; a self-test plants an upward edge.
   - CI on Windows, macOS, and Linux: format, lint, build, test; the documentation audit and its self-test on Linux.
   - Proof: a green pull request, and the three planted failures shown failing.
   - Not in the slice: the release workflow (0.2), licence checking (with the first dependency), `SECURITY.md` and `CONTRIBUTING.md` (their text needs the owner).
2. If the owner chose differently on either of the two choices, change the plan first and say so.
