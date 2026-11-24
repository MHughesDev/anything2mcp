# 0385. The set step commits a text control as the surface's platform profile says

**Status:** Accepted. Design decision. The owner had not read this record when it was written. Finishes package 8.4a, which [decision 0370](0370-platform-profiles-are-data-in-profiles-and-the-first-one-applied-is-the-identifiers-a-platform-makes.md) began, and replaces its list of commits.

**Date:** 2026-11-24

**Changes:** the `InputCommit` type of the `profiles` module of `a2m-pack-web-ui`, and the commits a profile file may name; the profile `controlled_inputs`; the `ui` driver's `set` of a text control; the surface field `profile` as the driver reads it; the base's `Input::Blur` and `ActTab::wait_redrawn`; the failure code `commit_not_redrawn`; the test application's pages under `/cases/commit`; the "Platform profiles" part of [spec section 301](../spec/3-packs/301-the-web-pack.md).

## Context

A profile states `input_commit`, how a value is committed in a page of its family, and nothing applied it: the driver set every text control by one rule (insert the text, then type it, then use the element's own setter, asking after each), whatever the page. That rule is a ladder of three ways, and a page sees each way it tries. A family of interface is not that loose: a page whose inputs keep their value in a component's state takes only what its own key handlers saw, and records nothing put in another way; a server-rendered forms framework sends the change to the application when the control loses the focus and draws the form again; a plain page needs only its control to say it changed. The commits decision 0370 listed (`none`, `blur`, `enter`, `native_set_then_blur`, `blur_then_wait_ready`) were names with no way behind them, and one of them, setting the value through the element's setter, is the way a page with controlled inputs can tell from a person's typing and ignore.

## Decision

1. **A commit is a closed type, and a profile names one of its members.** `InputCommit` has four: `blur` (insert the text as one edit, then take the focus from the control), `enter` (insert it, then press Enter), `blur_then_wait_ready` (insert it, take the focus away, then wait for the page to draw the form again), and `type_then_blur` (type it key by key, then take the focus away). The loader reads `input_commit` into this type, so a file that names any other commit, `none` and `native_set_then_blur` among them, does not load, with a sentence that names the commits the driver knows. A profile therefore never states a commit the driver has no way for.
2. **`controlled_inputs` commits by `type_then_blur`.** The family's pages take a value only when their key handlers saw it; setting it through the element's setter fires an event a page can tell from typing. `plain` stays `blur`, `aspnet_webforms` `blur_then_wait_ready`, and `virtualized_grid` `enter`.
3. **The driver's `set` of a text control on a surface that names a profile does the profile's commit, and only that.** The commit is read from the surface's `profile` when the tab is set up; a name this version does not hold is `step_not_usable`, with nothing sent. The commit is one way, not a ladder: after it the control is asked what it holds, and a control that does not hold the text is `unknown_variant`, `unexpected_control`, with its fate unknown and the commit named in the message; no other way is tried after it. The step's capture names the commit as its strategy.
4. **`blur_then_wait_ready` waits on the page's own word, and a page that does not draw again is a named stop.** Before the text is put in, the driver notes the count of the tab's loaded documents; after the focus is taken away it waits, at most ten seconds and never past the step's deadline, until the control leaves its document (a part of the page drawn again) or a document loads after that count (the whole page sent and drawn again). Then it waits for the field's locator to find a control, finds the field again, and reads the value from the control drawn anew. A page that does neither in that time is `unknown_variant`, `commit_not_redrawn`, with the fate of the value unknown: the change may have reached the application, and the stop does not say it did not (rule 7). The profile's own `ready` rule, which is script text, is not run: running a profile's script in a page is the probe's, with detection.
5. **A surface that names no profile, or `auto`, keeps the rule of no profile.** `auto` is settled by the probe's detection (package E2), which is not built; until it is, a surface of `auto` sets a text control as one that names none. The base rules of select, check box, and radio, and the widget rules, do not read the commit: a profile's commit is how text is committed.
6. **The browser base gains the two ways the commits need.** `Input::Blur` takes the focus from an element, as a person's moving on would, so that a control that changed says so; `ActTab::wait_redrawn` is the wait of point 4, a watch of the element's document in the page and the tab's count of loaded documents, never a fixed time.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the ladder of three ways, with the profile's commit added at its end | A page sees every way tried; a page with controlled inputs records a set value it ignored, and a post-back page posts a change for each. One way, named by the profile, is what a family needs |
| Keep `native_set_then_blur` for controlled inputs | The page the family is like ignores a value whose input event no key made; typing is what such a page takes |
| Wait for a post-back by a fixed time, or by the profile's `ready` script | A fixed time is a guess (rule 7, and the code conventions); the script is the profile's text run in a page, which belongs with detection in the probe |
| Read the value back from the old control after a post-back | The old control is not the page's any more; a value read from it says nothing of what the application drew |

## Proof

In `crates/a2m-pack-web-ui/tests/commits.rs`, against the test application, each proof asking it what its page was told or was sent: under `plain`, a page the server draws whole is told of the change once, with the value, before anything is saved; under `controlled_inputs`, the input of `/cases/typed-input`, which clears any value no key press made, is told of nothing ignored and saves the value typed; the same input under `plain` stops as `unexpected_control` with its fate unknown and the page told once that it ignored a value; under `aspnet_webforms`, a page that posts its change and draws the form again received the value once and the step reads it from the control drawn anew; the same form when it never draws again stops as `commit_not_redrawn` with its fate unknown, though the application received the value; and a surface of `auto` sets the text by the rule of no profile. In `crates/a2m-pack-web-ui/src/profiles.rs`: each family states its commit, each commit is read by its name, and a file naming `none`, `native_set_then_blur`, or any other word does not load.

## Not built

- `enter` is applied and proven by no page: its one profile, `virtualized_grid`, is read only, so no write through it compiles.
- A whole-page post-back is waited for by the tab's count of loaded documents and proven by no page; the test application's post-back page draws a part of itself again.
- The probe's detection, which settles `auto`, and the profile's `ready` rule (package E2); the frame mode `found`; the locator priority as an order; `drift_risk` as how often drift is checked.

## What would reopen it

A family whose pages need a commit that is not one of these four; a page that commits on a key other than Enter; or a post-back page that draws the form again without replacing its controls, which this wait would not see.
