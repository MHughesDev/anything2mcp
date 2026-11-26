# H-49: a role-and-name locator with a test-id locator survives more releases than either alone, and a fixed-weight proposer's top candidate is right in nine breaks of ten

**Status:** open. The first half is tested for content locators and fingerprints on one form; the test-id locator is not. The second half is tested for a proposer of exact pairing, not for a weighted one, on three changes of one form.

**Tested:** 2026-11-24, roadmap package 9.1, [decision 0384](../decisions/0384-a-screen-changed-on-purpose-is-caught-by-its-locators-or-its-fingerprint-and-a-named-section-is-a-region.md); 2026-11-25, roadmap package 9.1c, [decision 0492](../decisions/0492-a-drift-yields-a-repair-bundle-and-a-person-accepts-a-repair-a-pack-proposed-by-rule.md), the proposer.

## What was assumed

A locator list that holds a role-and-name locator and a test-id locator survives more releases of an application than either alone, and a fixed-weight repair proposer's top candidate is the right element in at least 9 of 10 breaks and proposes nothing in the rest.

## What was done

The test application's form of a line item was changed on purpose, through `POST /__control/screens`, in four of the five ways [research note 37](../research/37-locator-drift-and-repair.md) names: a reword, a restyle, a reorder, and a wrapping element. The `pages` project's screen model (each field by role and name first and by label second, with a fingerprint of its section and a text the section shows; the button by role and name) was published, and a watch's check of the read through the form was run in a real browser after each change (`crates/anything2mcp/tests/screen_drift.rs`).

## What was found

| Change | Role and name | Label | Fingerprint | The canary |
| --- | --- | --- | --- | --- |
| Reword ("Quantity" to "Qty", "Save" to "Save line item") | broke | broke | not reached | `locator_not_found` |
| Restyle (classes, a style sheet, every identifier renamed) | held | held | held | passed |
| Reorder (dimensions, quantity, description) | held | held | held | passed |
| Wrap (a named card takes the form's heading) | held | held | broke | `fingerprint_mismatch` |

The two content locators break together, on the reword, as research note 37 says: both are the label's words, so a second content locator adds nothing against a reword. Neither breaks on a restyle or a reorder, where a selector by identifier or by position would have. The wrap is caught by the fingerprint and not by any locator: the locators found the right fields, and only the fingerprint saw that the form no longer shows its heading. Every break was caught before a write was sent, and each repair was one line or two of the screen model.

## What changes

The browser base's page helper gives a named `section` the role `region`, as the HTML accessibility mapping does; before, the wrapped form's card had no role a model could name (decision 0384).

## What is still unknown

- Whether a test-id locator holds where the content locators broke: the stand's form has no test id. Adding one to the form, with a change that removes it, is the next case.
- A framework upgrade, which needs pages built with a framework (Phase 8).
- A weighted proposer's score: decision 0323's ranked candidates are not built. The proposer that is built (below) pairs two snapshots and has no second candidate to rank.

## The proposer, 2026-11-25

Decision 0492 built the web pack's first repair proposer, a rule of exact pairing over two `screen` findings of the page, one from before the change and one from now, rather than decision 0323's fixed-weight score over a failure snapshot. Each drift below was caught by a canary in a real browser, the page discovered before and after, and the bundle's proposal compared with the repair a person wrote by hand in package 9.1 (`crates/anything2mcp/tests/screen_drift.rs`).

| Change | Broken | Proposed | Right | Nothing proposed, and why |
| --- | --- | --- | --- | --- |
| Reword ("Quantity" to "Qty", "Save" to "Save line item") | the quantity box, the button | both | both: the hand repair, line for line | |
| Wrap (a named card takes the form's heading) | the fingerprints of the description and the quantity | both | both: the hand repair, line for line | The length's fingerprint text, "inches", is not an element's name in a snapshot, so whether it moved is not known, and it is left alone; it had not moved |
| Relabel ("Description" to "Item" and "Quantity" to "Qty" at once) | the description box, the quantity box | neither | | Two text boxes of the form are gone and two are new, so which became which is not shown by the page |
| Restyle, reorder | nothing | | | No drift |

Of six broken elements, four were proposed and all four were right; two were proposed nothing, and that was right too, since no rule could tell them apart without guessing by order, which a reorder in the same release would make wrong. The bet's threshold, the top candidate right in nine breaks of ten, is about a scorer that always ranks; a rule that proposes only where it is sure trades how often it proposes for never being wrong. On this form it was never wrong. The sample is three changes of one form, and the relabel is the only case where a score would have had something to rank.

Two things the work showed for the design:

- Without the snapshot from before, the reword would have proposed nothing: the form holds an item-code box the screen does not model, so the screen model and the page now show two unclaimed text boxes, and only the earlier snapshot says which was there all along. This is decision 0323's owed `reference` of the matched element, seen from the other side: a kept shape of the screen at a verified run is what lets a proposer be sure.
- A fingerprint's text is checkable only when it is an element's name; text a snapshot keeps as a type cannot be followed.

Still unknown: a test-id locator; a framework upgrade; a weighted score on a case like the relabel; a page with several equal forms, where a section's role and name alone do not tell two sections apart.
