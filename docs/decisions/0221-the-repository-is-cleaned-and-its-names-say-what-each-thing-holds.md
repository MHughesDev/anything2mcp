# 0221. The repository is cleaned, and its names say what each thing holds

**Status:** Accepted. Design decision. The owner had not read this record when it was written.

**Date:** 2026-11-18

## Context

The owner asked for a cleanup of the whole repository: stray and unused things removed, things with a poor name renamed, unsound structure rebuilt, and every document in force made true under the standard of [decision 0231](0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md) and the scope rule of [decision 0230](0230-the-scope-is-written-down-on-both-sides-and-describes-without-ruling.md). Five audits listed 115 findings and a verifier checked each against the tree. The verifier turned down every rename on cost. This record weighs each one on merit instead: a rename is made when a newcomer, or an agent that starts cold, finds the thing sooner and reads it more truly under the new name. References are updated in the same change.

The biggest fault was not a name. The documents that say what is built had fallen a day behind the code: [section 704](../spec/7-project/704-readiness.md) said nothing of the engine was built; the scorecard of [section 107](../spec/1-product/107-the-bar.md) said almost every row was designed; the changelog said nothing was usable; four documents said a benchmark gate fails the build when CI does not run it.

## Decision

1. **Moves.**
   - `docs/references/sources.md` moves to `docs/reference/sources.md`, and the folder `docs/references/` is gone. Two folders one letter apart sent readers to the wrong one; the generated list of sources is reference material. The audit's generator writes the new path and skips only that file.
   - The three review reports (coverage, breadth, configurability, all of 2026-11-18) and the coverage map move from `docs/plan/` to a new folder, `docs/reviews/`, with an index of its own. A review report is a recurring kind of document, not a plan; three had gathered among the plans in one day. Every link is repointed, decision records' links included, and a round that writes a report now files it there ([design review lenses](../plan/design-review-lenses.md), step 6).
   - `docs/assets/system-overview.png` moves to `docs/archive/assets/`. It shows the first product, and only an archived section uses it; in `docs/assets/` it read as current.
2. **A label that misled.** The Facts row "Built in" of every crate design becomes "Roadmap packages". It lists the packages that build the crate, done or not, and "Built in" read as "is built". What exists is said in one place in each design, its build order, where every step is marked Done, Done in part, or Not built; the architecture's index now says so. Five designs in flight keep the old label until their packages report.
3. **An index for `docs/plan/` and for `docs/reviews/`.** `docs/plan/` holds the order of the build, plans outside it, methods, and standing descriptions; a new `docs/plan/README.md` sorts them by kind, and the roadmap no longer calls itself the only plan. The audit refuses a document in either folder that its index does not list, and checks the counts the documentation index gives for both.
4. **The state of the code is kept in four places, each by the change that alters it:** the roadmap's State column and "Built so far" notes, each crate design's build order, the README's Status, and SCOPE.md. Section 704 stops keeping a list of its own. Its state table gives each section a "Design" column (how firm) and a coarse "Built" column (built, partly built, not built, or not applicable) that names where the detail is. Each spec section and reference document says the same in one sentence of its first paragraph.
5. **Untrue statements are corrected from the code and the tests**, not patched by guess: the scorecard row by row, the changelog's opening, the roadmap's stale marks (packages 1.1 and 1.2 are done, by the first-read test of the product; 1.3 and 2.1 become ready), the milestone table's new State column, the benchmark gate described as built and not yet enforced, which jobs CI runs on which system, and the "does not" lines of sections 101 and 402 and the README, each now marked as a rule, a choice, or not yet.
6. **Checks that were claimed and missing are added:** the audit now holds the npm launcher's version and its eight platform dependencies to the workspace version, which RELEASING.md already said it did, and counts the journal entries, the crate designs, and the plan documents in the documentation index. Each has a planted mistake in the audit's self-test.
7. **Dead code removed:** the variant `CallArgument::ObjectId` of the browser engine, never built and carrying an `allow(dead_code)`, and a `tokio` entry in the web interface pack's ordinary dependencies that its library never uses.
8. **Nothing tracked is deleted.** Ignored build output on disk was removed; it was never in git.

## Weighed and left alone

- **The three spikes.** AGENTS.md says spikes are thrown away; the question each answered is settled (decisions 0070, 0071 and 0130, 0120). They stay because research notes 17, 18, and 19 cite their results and scripts as evidence, as do section 305 and two crate designs, and a deleted file cannot be checked. The architecture's table of folders now says why they are kept.
- **The names of the test stands** (`targets/example-*`). They are package names used by tests and CI, and each already says which stand it is.
- **`targets/example-app/tests/cases.rs` and `more_cases.rs`.** Each file's first lines say which cases it holds; a longer name would say no more.
- **`docs/hypotheses/TEMPLATE.md`** in upper case marks a file about the folder, as README.md does.
- **Decision record names**, the first record's old product name among them: history.
- **Where a pack keeps its manifest** (the crate root or `src/`): each crate design says where its own is.
- **The two script folders of `bindings/`**: decision 0156 names the paths.

## How the work was done, and what a second reader found

The planner of this cleanup could not start a worker agent: the session offered no tool for it. The planner made the first seven batches itself, and the lead then started a worker, which moved the review reports and checked the planner's rebuilt documents against the code. It found these faults in the planner's work, and each is corrected:

- [Section 704](../spec/7-project/704-readiness.md) said fairness between callers is not designed. Round robin between callers and priority are built and tested in `a2m-sched`; the part not designed is a weighted share for a caller.
- Section 704 and the first paragraph of [section 601](../spec/6-running-it/601-deployment-configurations.md) said the `solo` and `workstation` profiles are built. `solo` is; the local HTTP of `workstation` is read by no code.
- Section 704 listed the closed lists of findings as not designed. They are enums in `a2m-core`; section 218 is corrected the same way.
- The roadmap left package 1.4 `held` when the one package it needs was done, and package F1's note said three things were not built that are built and tested in CI.
- The documentation index counted three diagrams after one had moved to the archive.
- Sections 101 and 402 called the thing decision 0058 refuses "hosting"; it refuses hosting a place to share target files.

The worker's questions also showed one fault of the planner's own making: one edit of section 704 (the judgment-alone paragraph, the bets, the row for configuration, and three stale rows of "not designed yet") had been prepared and never written to the file, so the planner reported it as done when it was not. It is now made.

## Alternatives weighed for the layout as a whole

- **Leave every name and fix only sentences**, as the verifier proposed. Cheaper, and it leaves a newcomer the same wrong turns: the one-letter folder, the label that reads as a state, a diagram of the first product among current ones.
- **A deeper regrouping of `docs/`**: plans, methods, and reviews each in their own top-level folder, and the reference documents split by audience. Turned down for now: links into `docs/plan/` and `docs/reference/` run into the hundreds, many from records and from files in flight, and the index of each folder gives most of the gain. The one regrouping with a clear gain, the review reports, is made.
- **Keep a list of what is built in section 704 and update it more often.** It fell behind within a day of being written, while the roadmap and the crate designs, which each package must touch, stayed current. One place per fact is the rule of AGENTS.md for knowledge in the code; it is applied here to the state of the code.

## Consequences

- An agent reading section 704, the scorecard, or a crate design's Facts table is no longer told that built work is ahead of it.
- A package that changes what is built updates its roadmap row and its crate's build order, as before, and the Built column of section 704 and the first paragraph of its spec section when the coarse state changes.
- The audit fails when a plan or review document is not indexed, when the npm launcher's version differs, and when the index's counts of journal entries, crate designs, plan documents, or review documents are wrong.
