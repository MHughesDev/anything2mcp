# 0399. A screen changed on purpose is caught by its locators or its fingerprint, and a named section is a region

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the test application's control interface (`POST /__control/screens`, a new module `releases.rs` of `targets/example-app`); the role the browser base's page helper gives a `section` (`crates/a2m-pack-web-browser/src/helper.js`); the [test application plan](../plan/test-application.md); roadmap package 9.1, which is done; the first results of hypothesis H-49. Nothing in `a2m-engine` or `a2m-actions` changes: [decision 0380](0380-a-drift-marks-the-knowledge-it-was-seen-in-and-stops-every-binding-that-relies-on-it.md) is proven as written.

## Context

Decision 0380 proved through the kernel, with the test pack's screens changed by a hook, that a canary which finds a screen changed stops the write through it, and that a repaired version lets the write run. Its proof left the browser out: the test application had no screen that could be changed on purpose, so nothing showed which changes a real page makes are caught, which the screen model survives, and whether the repair a person writes fits the page that changed.

Two choices were open. How the stand changes a screen: a separate page per change, or one switch over the page the existing `pages` project already models. And which changes to make: research note 37 names a reworded label, a restyle, a reorder, a wrapping element, and a framework upgrade.

## Decision

1. **The stand changes the form of a line item in place, by a switch of its control interface.** `POST /__control/screens` with `{"page": "line_item", "change": ...}` changes `/orders/{order}/items/{item}` until a reset or `"none"`, as `/__control/shipment` changes an answer. The page as first released is unchanged, byte for byte. Each change is what a release makes and changes nothing the form sends or stores: `reword` (the label "Quantity" reads "Qty", the button "Save" reads "Save line item"), `restyle` (class names, a style sheet, every identifier renamed with each label still naming its field), `reorder` (the dimensions, the quantity, then the description), and `wrap` (the form inside a card, named by its heading "Line Item Details", which leaves the form). *Instead of:* four new pages, each with a screen of its own. The product test would then have proven four screens, not one screen that changed under a published version, which is the case a canary is for.

2. **What each change showed, with the screen model of the `pages` project** (role and name first, label second, and a fingerprint of the section each element is in, with a text that section shows):
   - `reword` is caught: no locator of the quantity's list finds "Qty", and the canary stops on `locator_not_found`. The role-and-name locator and the label locator break together, because both are the label's words.
   - `wrap` is caught: every locator still finds its element, but the fingerprint of the description and the quantity names a form that shows "Line Item Details", and the form no longer shows it; the canary stops on `fingerprint_mismatch`. The locators alone would have read the right fields: the fingerprint catches a change of structure that the locators do not, and that is what it is for.
   - `restyle` and `reorder` are survived, with no drift at all: nothing the model names is a class, an identifier, or a position. The canary passes, and the first version takes the write. A selector by identifier would have broken on `restyle`, and one by position on `reorder`.

3. **A named section is a region.** The page helper gave every `section` the role `generic`, so the wrapped form's card had no role a screen model could name, and the repair had to drop the fingerprint's text. The HTML accessibility mapping gives a `section` with a name of its own (`aria-labelledby` or `aria-label`) the role `region`, and one without a name `generic`; the helper now does the same. No page the tests use held a section before. The repair of `wrap` places the fields in `{role: region, name: "Line Item Details"}` with the same text.

4. **The repair is a version, written and published as any version is.** The test edits the screen model in the draft and publishes it as the person; nothing heals in a run (rule 1). A canary through the changed form then passes on the repaired version, and the write is approved, made once, verified, and read back.

## What this does not do yet

- A framework upgrade, the fifth change of research note 37, is not built: it needs pages built with a framework (Phase 8).
- The stand's form has no test id, so H-49's comparison of a test-id locator with a role-and-name one is not made; the repair proposer (decision 0323) is not built, and H-49 stays open.
- Decision 0380's list of what it does not do is unchanged: a stopped tool is not shown unavailable in `tool.list`, canaries have no schedule of their own, and a write's own drift leaves no mark.

## Proof

`targets/example-app/tests/releases.rs`: each change is what this record says, a reset and `none` give back the page as first released, an unknown change or page is refused, and every name and role the form's script reads is kept. `crates/a2m-pack-web-browser/src/tests/looking.rs`, `a_named_section_is_a_region_and_an_unnamed_one_is_not`. `crates/anything2mcp/tests/screen_drift.rs`, in a browser through the product's commands: for `reword` and `wrap`, a watch's check of the read through the form stops as `drifted` with the pack's code; the write through the form is refused `binding_drifted`, naming the tool, its run `stopped`, nobody asked to approve it, and the application heard nothing of the line item and stored nothing; the repaired screen model, published, passes a new canary through the changed form; the write is then made once and read back, the application holds exactly that one write, and no run of the writing tool failed. For `restyle` and `reorder`, the canary passes both times, nothing is marked, and the first version makes one write under each.
