# 0112. The `ui` driver as built: five step kinds, and a commit that waits for its confirmation

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the design of [the web pack's drivers](../architecture/crates/a2m-pack-web-ui.md); the `ui` rows of the driver table and the lint codes in [spec section 301](../spec/3-packs/301-the-web-pack.md).

## Context

The design gave the `ui` mode ten step kinds and put an action's `confirm` in the confirm list of a binding. Building the driver against the test application, through the driver handle and with a real intent token, settled what a step is and where a save is judged.

## Decision

1. **Five step kinds are declared: `goto`, `wait`, `read`, `set`, `invoke`.** There is no `locate` step: each `read`, `set`, and `invoke` finds its own element and checks its fingerprint. *Instead of:* a `locate` step that hands an element to the next step. An element is good for one page generation, so a kept element can only go stale, and each extra step is a round trip. `handle`, `upload`, `download`, and `observe` are not built and not declared.
2. **The committing `invoke` is not done until the page shows the action's `confirm`, or a variant the screen declares.** The confirm list of a `ui` binding is empty. *Instead of:* a click as the commit point and a `wait` in the confirm list. With that split the engine marks a record as acted when the click is dispatched, and a refusal the application shows a moment later arrives after it; and a browser lost while the page is saving fails a step that does not commit, where the handle cannot make it `possible_commit`. In one step, a refusal is `rejected` before anything is marked, and every loss after the click is `possible_commit` by the handle's own rule.
3. **A locator that matches several elements, and a match that fails its fingerprint, stop the step.** A later locator of the list is tried only when an earlier one matches nothing. *Instead of:* falling through to the next locator, which would let a weaker locator stand in for one that has just been shown to be wrong.
4. **`health` answers from what was found when the pack registered.** The contract says it does no input or output; the browser engine's check looks at the disk. A browser installed later is seen at the next start.
5. **A read gives the text the page shows,** and a check box gives a truth value. The driver converts nothing: the field type's normalizer, in the engine, reads the application's form of a value.
6. **Evidence:** every step returns what it did as a capture named `acting`, as the HTTP pack does, with the screen, the address template, the locator that won, and the strategy that set a control, and never a value. A write binding keeps a snapshot before it acts and one at the confirmation, as captures named `snapshot`.
7. **A run outside a posture that may write does not touch a control:** `set` and `invoke` are refused with `policy`, `read_only_posture`, whether or not the action commits.
8. **Lint codes are written `web.css_not_last`,** with a dot, because a lint code is the pack's name, a dot, and the rule.

## What was shown

Against the test application, asking its control interface: one read; five runs with one list of means and one result; a table read by its headings; an absent record; one write stored exactly once, with `12.50` sent, `12.5` shown, and the two equal under the field type's rule; a refused write `rejected` with nothing stored; a browser killed while the application held the save `possible_commit`, with no second write; drift by a missing locator, an ambiguous one, and a failed fingerprint; a control with no rule `unknown_variant`.

## Revisit if

- The engine needs the confirmation as a separate part of its trace.
- A platform is found where falling through to a later locator after an ambiguous match is the only way to find a field.
