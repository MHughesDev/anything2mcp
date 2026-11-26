# 0492. A drift yields a repair bundle, and a person accepts a repair a pack proposed by rule

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Builds** package 9.1c, the repair bundle and the repair proposer of section 217, step 2, and [decision 0323](0323-a-repair-is-proposed-by-a-fixed-scorer-and-drifting-is-seen-before-drift.md), which [decision 0390](0390-the-open-questions-of-2026-11-24-are-settled-by-the-lead-and-the-first-release-is-what-progress-counts-toward.md) made a row. It **keeps** decision 0380's marks, their refusal and their lift, decision 0396's marked listing, and decision 0482's canaries unchanged: nothing here lifts a mark. A repair is a version, as before; this record says how a person gets one without writing it by hand.

**Changes** the provider contract (a new public interface, `RepairProposer`, with `RepairAsk`, `RepairFile`, and `RepairProposal`; `Registrar::repair_proposer`, `Registry::repair_proposer`); the web pack's probe (its proposer for `screen` findings, `a2m-pack-web-probe/src/repair.rs`); the registry (`repair.propose`, `repair.accept`, neither listed; `discover.run` takes a `focus`); the test application (a fifth change of the form, `relabel`); the saved API description and both generated clients; spec section [217](../spec/2-kernel/217-traces-evidence-and-repair.md); the [action catalog](../reference/action-catalog.md), the [technical reference](../reference/technical-reference.md), the [glossary](../reference/glossary.md); the designs of [`a2m-actions`](../architecture/crates/a2m-actions.md), [`a2m-provider`](../architecture/crates/a2m-provider.md), and [`a2m-pack-web-probe`](../architecture/crates/a2m-pack-web-probe.md); the [test application plan](../plan/test-application.md); hypothesis H-49; the roadmap (9.1c done).

## Plan, stated before the build

The row asks for two things: a bundle a drift yields, and a proposer whose repair a person accepts. Three questions decide the design. **Where the proposer lives**, since rule 8 puts knowledge about one kind of system in its pack and `a2m-actions` "must never name a pack". **What the proposer is given**, since the web pack's driver keeps no snapshot when a step fails and the screen model keeps no reference of the element it last matched (decision 0323's owed `reference`). And **what acceptance does**, since rule 1 forbids a repair in a run and section 217 has a person approve the diff.

The alternatives weighed against the documented design (a ranked list of candidates from the failure snapshot, scored under fixed weights the pack declares):

- *A proposer in `a2m-actions` that reads screen models and snapshots.* Fewest crates touched. Not taken: the kernel would then know the web pack's locators and its snapshot's roles, which is rule 8's case exactly, and every interface pack after it would add its own branch to the kernel.
- *A proposer over the failure snapshot alone, scored against the screen model.* What decision 0323 and research note 37 sketch. Not taken for this slice: the failure snapshot does not exist (the driver keeps none), and on the test application's reword the screen model alone cannot tell the renamed quantity box from the item-code box the screen does not name; both are text boxes in the form, unclaimed. A score would have to weigh order or position, and a reorder in the same release makes that the wrong element.
- *A proposer over two snapshots of the page, one from before the change and one from now.* Taken. What changed is then a fact of the two shapes, not a similarity: an element of a role that was there and is not, and one of the same role, in the same section, that is there and was not.

## Decision

1. **The proposer is the pack's, behind a public contract.** `RepairProposer` (in `a2m-provider`) is pure: it is handed a `RepairAsk` (the drifted entry's name, the pack's code the drift stopped on, the draft files of the file kind that holds the knowledge, as text, and two findings of the kind it is registered for, `before` and `now`) and gives a `RepairProposal`: `Proposed`, with the path of the one file it changes, the file's new text, and one line for each change saying what showed it; or `None`, with why. A pack registers at most one per finding kind its manifest owns (`Registrar::repair_proposer`), as it does a key rule (decision 0486). Nothing in the kernel reads a finding's `found`, a file's content, or a locator.

2. **The web pack's first proposer is a rule of exact pairing, not a weighted score.** It is registered by the probe, for the `screen` findings its `web_snapshot` writes, and reads the `screen` files of the drafts. It parses the screen the drift names and pairs the two snapshots by role and name, in the section (the nearest element of a sectioning role) that held each:
   - *An element renamed*: a locator of the screen (`{role, name}`, or `{label}` for a field) finds nothing now; the element it found before is gone; and in its section there was exactly one such element of its role that is gone and there is exactly one of that role that is new. The locator takes the new name. Two or more gone, or two or more new, of one role in one section is not a sure pairing.
   - *A section that moved*: every locator finds its element, and a fingerprint names a `near_text` that, before, was the name of an element inside the fingerprint's section and, now, is not; the nearest section around the field that now holds an element of that name and role becomes the fingerprint's section.
   - *Anything else is unknown, and proposes nothing* (rule 7): a locator of a kind the rule does not read, a fingerprint text the snapshot keeps only as a type, no `before`, a snapshot cut short or of another format, two files that define the screen, or a change that would leave any element of the screen unpaired. A proposal repairs every break the rule sees or none: half a repair still drifts. The edit replaces only the scalars it names, in place, so the file keeps its comments and its form.
   Decision 0323's ranked candidates with scores by property are not built; a pairing that is one to one has no second candidate, and the margin is the rule that there be exactly one. The weights wait for a case the pairing cannot settle that a score would.

3. **The bundle is built on asking, from what is kept.** `repair.propose` (a builder's, not listed) takes the target, the knowledge as `tool.describe`'s `unavailable.knowledge` names it, and the two findings, by identifier, of a discovery of the page (`discover.run` now hands its discoverers a `focus`, such as the web probe's `paths`, which it always could and was never given). It reads the latest drift mark of that knowledge; refuses findings that are not the target's, not of one kind, or not of one subject; finds the pack section the mark's digest names in the published version, and so the file kind; compiles the drafts and refuses when no section of theirs has that digest (the drafts already changed the knowledge, and a proposal over them would repair what was not published); and hands the pack's proposer the drafts of that kind. It keeps nothing. What it gives:
   - **the drift**: the operation, mode, surface, the pack's code, the run, and when it was marked;
   - **what the drift stops**: every binding the mark covers, by tool and mode;
   - **the two shapes**, each finding's identifier and its `found` under `untrusted`: the page's own names of headings, controls, and sections, and every value as a type (the web probe's rule, decision 0195);
   - **the file it touches**, by path and kind, and the bindings it stops, by tool;
   - **the proposal**: `proposed` true with each changed line of the file, before and after, the lines saying why, and a `proposal` digest of the file's path and its text before and after; or `proposed` false with the pack's reason.
   The same inputs give the same bundle.

4. **Only a person accepts, on the engine's page; an agent can only ask.** `repair.accept` is a person's act with presence: an agent's call is sent to the page, which shows the diff, the reasons, and what the change goes on to need. It takes what `repair.propose` took and the `proposal` digest, builds the bundle again, and refuses when the digest differs (the drafts, the mark, or the findings changed since it was shown). Its whole effect is the one draft file written, through `draft.write`'s own checks and lint, and its answer points at `draft.lint`.

5. **Acceptance writes a draft, never a version.** The repaired file is a draft like any other: it is linted, its offline tests and the publish gates run, and a person publishes it as a new version, which has another section digest, so decision 0380's marks no longer stop it. Nothing heals in a run (rule 1), and nothing reaches a published version without its gates and a publish.

## Proof

| Claim | Test |
| --- | --- |
| The web probe's rule: a reword pairs the renamed box and button and changes only those scalars; a wrap moves the fingerprints that named the heading and no other; two boxes renamed at once, no `before`, a locator by test id, and a cut snapshot propose nothing, each with its reason | `crates/a2m-pack-web-probe/src/tests/repair.rs` |
| A proposer is registered once, for a finding kind its manifest owns, and found by the registry for that kind alone | `crates/a2m-provider/tests/repair.rs` |
| In a browser, through the product's commands, the form reworded by `POST /__control/screens`: a canary stops the write; a discovery of the page before and after gives two findings; `repair.propose` gives a bundle with the drift, the bindings it stops, the two shapes, and the diff `Quantity` to `Qty` and `Save` to `Save line item`; an agent's `repair.accept` is sent to the page and changes nothing until the person presses; the press writes the draft, which lints, passes its offline tests and publish gates, and is published by the person; a canary through the changed form then passes and reads the quantity the application holds, and the write is made once | `a_reworded_form_is_repaired_by_a_proposal_a_person_accepts` in `crates/anything2mcp/tests/screen_drift.rs` |
| The same, with the form relabelled (two boxes renamed at once): the bundle proposes nothing and says the pairing is not one to one; `repair.accept` is refused before any person is asked; no draft changes | `a_relabelled_form_proposes_no_repair_and_says_why` |
| The same, with the form wrapped: the two fingerprints that named the heading are proposed the card that holds it now, the repair written by hand in package 9.1, and no other line | `a_wrapped_form_is_proposed_the_section_that_holds_its_heading_now` |
| After the repaired version is published, the bundle is refused: the version published now does not hold the knowledge the mark names | `a_reworded_form_is_repaired_by_a_proposal_a_person_accepts` |

## What this does not do yet

- The failure snapshot: the bundle's shapes come from a discovery a builder runs, not from the failing step. A driver that keeps a snapshot at a failure would let a canary's drift carry its own.
- The step's trace record in the bundle: `run.trace` gives it, by the run the bundle names.
- A `before` the engine keeps: today it is a discovery a builder ran while the version was good. A version's publish, or a canary's pass, keeping the screen's shape is the next step, and is decision 0323's owed `reference`.
- A proposer for an endpoint, a platform-profile rule, or a desktop screen. The contract takes them; none is written.
- Decision 0323's weighted score and ranked candidates; `drifting`.

## What the work showed

- **The pairing was right every time it proposed, and it proposed for four of six broken elements.** In a real browser, against the test application's form: the reword's quantity box and button (two elements), and the wrap's two fingerprints, were each proposed exactly the repair a person wrote by hand in package 9.1; the relabel's two boxes were proposed nothing, which is right, since the page does not say which old label became which new one. No proposal named a wrong element. H-49's second half, for this proposer, is in the [result file](../hypotheses/H-49-result.md).
- **The failure snapshot was not needed, and the screen model alone would not have done.** On the reword the form holds an item-code box the screen does not name; with only the screen model and the page as it is now, the renamed quantity box and the item-code box are two unclaimed text boxes of one form, and a rule that proposes only where it is sure would have proposed nothing. The snapshot from before tells them apart: the item-code box was there before, with that name.
- **The drafts must be compiled under the draft version** to be compared with the mark's digest: a compile under the published version's number is refused, as a draft, and the first build read that refusal as drafts that do not hold the knowledge.
- **A repair published lifts what the bundle is of.** Once the repaired version is current, no section of it has the mark's digest, so `repair.propose` answers that nothing of it is stopped, which is what decision 0380's mark keyed to the knowledge, not the version, gives for free.
- **The focus of a discovery was there, never handed.** The web probe read `paths` from its scope since decision 0114; `discover.run` handed it an empty map. One field made a discovery of one named page possible.
