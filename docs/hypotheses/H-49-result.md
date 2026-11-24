# H-49: a role-and-name locator with a test-id locator survives more releases than either alone, and a fixed-weight proposer's top candidate is right in nine breaks of ten

**Status:** open. The first half is tested for content locators and fingerprints on one form; the test-id locator and the proposer are not.

**Tested:** 2026-11-24, roadmap package 9.1, [decision 0384](../decisions/0384-a-screen-changed-on-purpose-is-caught-by-its-locators-or-its-fingerprint-and-a-named-section-is-a-region.md).

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
- The proposer's score: decision 0323's proposer is not built, so its top candidate was not scored against the known right element. In these breaks the right element was the one the reword renamed and, for the wrap, the same element under a new section.
