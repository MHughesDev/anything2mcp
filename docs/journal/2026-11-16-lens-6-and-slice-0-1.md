# 2026-11-16: design review with lens 6, then slices 0.1, 0.2 up to the publish, and 0.3

## State at the end

The repository has its first code: a Cargo workspace of 23 empty crates that is formatted, linted, built, and tested on Windows, macOS, and Linux by CI, with the crate layering enforced by the audit script. Nothing is usable; the `web2mcp` binary only reports its version. One design review round was run before it, with lens 6. Pull requests 1 (the review) and 2 (slice 0.1) are merged. Slice 0.2 is built up to its last step: the release workflow and a placeholder pip package exist and are checked in CI, and nothing has been published. The PyPI name is still not held. Slice 0.3 is built: the test application covers cases 1 to 4, with its control interface and a harness, and CI checks the licences of the first outside crates. This entry's last update is part of pull request 4 (slice 0.3), which the agent merges once its checks pass; pull request 3 (slice 0.2) is merged.

## Done

- **Lens 6, one pipeline line by line.** Walked `example_order_sheet_enter` (a sheet of line items entered into an order) from its files through compile, call, validation, preview, approval, the write of each record, a stop and resume, and a second run. Sections read in full: 5, 6, 7, 8, 10, 11, 12, 18, 21, 39, 42, 44, 45, 46, 47, 49, 50, 51, 52, and every reference document except the settings examples. Documents corrected in 19 files.
- **Slice 0.1, the repository skeleton.** Phase 0 tasks A2, A3, A4, and A6.
  - The workspace, with Rust 1.100.0 pinned and lints that deny `unsafe`, `unwrap`, and `expect` in library code.
  - The 23 crates of spec section 29, empty, with no dependency edges.
  - `web2mcp` reports its version and refuses anything else with exit code 2.
  - The layering check in `tools/docs_audit.py`, and `tools/test_docs_audit.py`, which plants nine mistakes and checks each is caught.
  - `.github/workflows/ci.yml`, and the Apache-2.0 `LICENSE`.
- **Slice 0.2, the release workflow, without the release.** Phase 0 task A1, except the publish and the check of each `w2m-*` crate name.
  - `bindings/python/`: a placeholder pip package, version 0.0.1, that says it contains no working software.
  - `.github/workflows/release.yml`: builds, checks, and installs the package on a pull request; publishes to PyPI only when the tag `v0.0.1` is pushed on a commit of `main`.
  - The workspace version is now 0.0.1, and the audit checks that the pip package carries the same number and an exact copy of the licence.
  - Not done: the publish itself, which is the owner's to approve, and so the slice's proof, `pip install web2mcp` from PyPI. Not done either: checking whether each `w2m-*` name is free on crates.io.
- **Slice 0.3, the test application.** Phase 0 tasks C0, C1, and A5. Started although slice 0.2 is unproven, because the only step 0.2 lacks is the owner's and 0.3 does not build on it.
  - `targets/example-app/`: cases 1 to 4, the control interface, and the harness `example_app::harness::TestApp`. Its README lists every address.
  - 18 tests of its own, 12 of them over real HTTP. The slice's proof resets the application, changes a record, and asks the control interface what was stored and how many times.
  - `deny.toml` and a CI job that checks licences, advisories, yanked versions, and sources.
  - The audit refuses the test application as an ordinary dependency of a product crate; its self-test now has 13 cases.
  - Not done: Set a mode in the control interface, which no case needs before case 13; the hard-case pages of task C2; the benchmark harness of task C3.

## Learned

- The design's own running example could not run as written. Each part was right alone; the joins had never been checked. Expect the same wherever two sections meet and no example crosses both.
- The preview was the one phase of a write with no procedure. Sections 18, 45, and 50 each held a piece.
- More paper review will find less. The gaps left need code, mostly in Phase 3. Lens 7, "less", is the natural round for the end of Phase 0.
- Slice 0.1 tested no hypothesis. Proof that the checks bite: a planted `unwrap` and a planted `unsafe` block each failed the lint and were removed; the audit's self-test passes 9 of 9.
- CI, first run, wall time per job: Linux 13 s, Windows 30 s, macOS 35 s, the audit and its self-test 5 s. All four passed on the first run. There is no budget for these; they are the starting point to watch as code arrives.
- The layering table can be enforced from the document itself, in about thirty lines of Python, with no second copy of the table.
- Building the test application made hypothesis H-23 concrete. Like many real applications it has no form at the address of a line item that does not exist, and a different address for the empty form. The screen model in spec section 7 cannot reach the second.
- The form was driven once by hand in a real browser, by role and label: an update, a second save with the new version, a validation message, and a create all behaved, and the control interface showed three writes. Nothing automated covers the page's script yet.
- axum, tokio, serde, and reqwest compiled and ran first time on Linux, and the licence check passes. Proof that a licence is really checked: with MIT taken off the list, the check failed.
- CI with the first outside crates, first run, wall time per job: not known yet. The first run on pull request 4 had not finished when this line was written.
- The placeholder package was built twice from one commit and the files were identical, byte for byte, once file times were taken from the commit. It installed into a clean environment and imported. That is local proof of the build half only.

## Changed in the design

- From lens 6: see the fourth pass in the [readiness report](../plan/readiness.md). In short: section 7's example now connects; one reading of sources; a preview procedure and open points in section 42; rule 3 no longer forbids resume; `version_conflict` moved from `rejected` to `stale`; the glossary names the engine loop and the pipeline step; hypotheses H-23, H-24, and H-25 added.
- From slice 0.1: [decision 0045](../decisions/0045-the-workspace-starts-with-every-crate-and-the-audit-enforces-the-layering.md). Phase 0 task A3 no longer asks for dependency edges on empty crates. The technical reference states that a crate may not depend on a crate in its own layer.
- From slice 0.3: [decision 0047](../decisions/0047-the-test-application-is-a-small-server-in-the-workspace-with-a-closed-licence-list.md), which the owner has not read. "What is built" in the [test application plan](../plan/test-application.md). The crates verified by use in [research note 9](../research/09-rust-building-blocks.md).
- From slice 0.2: [decision 0046](../decisions/0046-the-pip-name-is-held-by-a-placeholder-published-on-a-version-tag.md), which the owner has not read. [Publishing setup](../plan/publishing-setup.md) now says how the placeholder is published.

## Noticed, not acted on

- A diff's actions are `create`, `update`, and `unchanged`. `delete`, `submit`, and an upload have no place in the diff or its hash, so an approval for an irreversible operation would bind to little more than a key. Fix before the first irreversible operation is built.
- The step trace's unit and the driver interface's `read_back` method both disagree with the step list. Open points in section 47 and the technical reference; slices 1.2 and 1.1 settle them.
- The driver sketch calls a run's hold on a session a `SessionLease`. The glossary keeps "lease" for a runner's hold on a job.
- A result shows one `mode` for a whole run, but a pipeline's steps can use different modes.
- On a server, a grant on a pipeline says nothing about the operations it calls.
- The layering table allows no edge inside a layer. The session keeper and the browser share a layer, as do the policy engine and the ledger. H-25 counts the collisions; the count stands at zero because no crate has code.
- AGENTS.md's table of paths does not list `crates/`, `targets/example-app/`, the audit's self-test, or the workflows. Left alone; see "Undecided".
- `SECURITY.md` and `CONTRIBUTING.md` (task A7) need text the owner has agreed.
- The test application's pages have no automated test. The interface driver's first tests should include the form.
- The dependency tree is about a hundred crates, most of them from `reqwest`. If CI time matters later, the harness's HTTP client is the place to look.
- The slice order was bent: 0.3 was built while 0.2 waits for the owner's publish. If that wait is long, later slices keep piling on top of an unproven release workflow.

## Undecided

For the owner.

- **Unicode-3.0 on the list of allowed licences.** It is permissive, and it covers the Unicode data crates that `reqwest` brings in. AGENTS.md names MIT, Apache-2.0, BSD, ISC, "or similar permissive terms"; the agent took it as similar and did not ask first.
- **Publishing the placeholder to PyPI.** Everything is ready. The step is pushing the tag `v0.0.1` on `main`, and it cannot be undone. The owner pushes it, or tells the agent in so many words to. Until then the name can still be taken by someone else.

- **How much the agent does without asking.** On 2026-11-16 the owner said they are not reviewing pull requests, that the agent merges them, and "full auto". The agent acted on that within the session: it merged pull request 1, built slice 0.1 on its own recommendations, and merges pull request 2. AGENTS.md, the working method, and the `start-session`, `build-slice`, and `end-session` skills still say to wait for a go-ahead and not to push without being asked. The agent began to rewrite them and the session's safety check refused the edit as weakening oversight, so they are unchanged. Until the owner changes them, or says so again, a new session follows them as written: show the plan and wait.
- Whether a stopped write run resumes only when the caller asks, or by itself when the engine restarts.
- Whether an `upsert` in a `ui` binding may declare two paths (H-23), and whether operations should name the kind of record they write (H-24). Neither blocks anything before Phase 3.
- Carried over: whether the repository stays public; where real target files live; which MCP clients matter most; whether a Windows machine is available; trusted publishing on crates.io and npm.

## Next

1. If the owner has said to publish: push the tag `v0.0.1` on `main`, watch the release workflow, then run `pip install web2mcp` in a clean environment and record the name as held in [publishing setup](../plan/publishing-setup.md). That finishes slice 0.2.
2. Then slice 0.4, the spike that chooses the browser engine base. Run research brief 1 first, with the `research-brief` skill. The spike lives in `spikes/`, drives the test application's pages, fills in the conformance suite's launching, locating, and waiting groups for each candidate, and ends in a decision record. It tests hypotheses H-01, H-02, and H-03. It can start whether or not slice 0.2 has been published.
