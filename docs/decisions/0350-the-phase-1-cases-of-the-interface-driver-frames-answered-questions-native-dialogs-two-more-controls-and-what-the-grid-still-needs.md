# 0350. The Phase 1 cases of the interface driver: frames, answered questions, native dialogs, two more controls, and what the grid still needs

**Status:** Accepted. Design decision. The owner had not read this record when it was merged. Carries out package 1.6. Carries out point 3 of [decision 0322](0322-a-capture-never-infers-required-and-types-a-parameter-from-every-value.md) in the drafter of a page's own requests, which [decision 0341](0341-endpoint-inference-as-built-the-counts-the-short-number-the-sibling-words-and-the-map.md) left not done.

**Date:** 2026-11-24

**Changes:** the `screens/` format in [file formats](../reference/file-formats.md#screensyaml) (a `frame`, the handlings `confirm` and `dismiss` with `with`, a field's `options`, an action's `dialog`, the controls `radio` and `date`); the widget driver table and the edge cases in [spec section 301](../spec/3-packs/301-the-web-pack.md); the designs of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md) and [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md); the cases built in [the test application's plan](../plan/test-application.md).

## Context

The proof of package 1.6 is that "the Phase 1 cases of the test application pass". The case table names cases 6, 7, 8, 9, 11, and 12 for Phase 1, case 10 for "1, then 8", and case 33, the browser's own controls, for Phase 1. Of these, 31 and the reads of the form and the table were built; 33 had a page and no test that drove it; 6 to 9, 11, and 12 had no page at all. Writing the pages and driving them showed which parts of the driver were missing.

## Decision

1. **The Phase 1 cases are 6, 7, 8, 9, 11, 12, 31, and 33.** Case 10 is split: its custom controls (a filtering dropdown, a date picker, a rich-text editor, a file input, a toggle, a tree) are package 8.4's, as the table's "then 8" says; the browser's own controls are case 33's.
2. **The test application has a page for each of cases 6 to 9, 11, and 12, under `/cases`,** and says what each page was told: `GET /__control/cases` lists the events a page sent by `POST /cases/events/{name}`, in the order they were told. A test asks the application what its page did and never takes the driver's account of it.
3. **A frame of the page's own origin is a scope of a query.** In the base, a query whose `within` is a frame element is looked for in that frame's document; a frame of another origin has no document to enter and is no scope, so a control of a frame is never taken for one of the page. The base's waits watch such a frame, and a click's middle is given in the page's own coordinates. A screen declares the frame it is in with `frame`, a list of locators, outermost first; each locator of the screen, and each section a fingerprint names, is then looked for inside it.
4. **A page's question is a declared variant, answered once by an action of the screen.** A variant takes the handling `confirm` or `dismiss` and `with`, the name of an action of the screen. `confirm` is a question the page asks of the action just performed: it is answered once and the page's own confirmation is awaited again, so a question the page asks twice stops the run. `dismiss` is a state in the way, such as a notice over the form: it is closed before the step goes on, and one still shown after is a stop. A page that does not ask costs nothing, since a variant is a state that may appear, not one that is waited for.
5. **A dialog of the browser is declared by the action that opens it.** An action may carry `dialog: {says, answer}`. When its click is held by a dialog that says exactly that, the driver accepts or dismisses it and goes on; a dialog that says anything else, or that no action declares, stops the run with `unexpected_dialog`, and after a click that saves it is a write in doubt. The base tells a tab's caller what the open dialog says, redacted and bounded, and answers nothing by itself.
6. **Two controls are added: `radio` and `date`.** A `radio` field's locators find the group; its `options` list each stored code with the label of its radio button; it is set by choosing the radio button of the code's label and read as the code of the one that is on, and a code the options do not list is refused before anything is sent. A `date` is set by the text rule (filling, typing, then the field's own setter) in the form the browser's date field takes, `2026-12-01`, and read as its value. An action that cannot be pressed because the page keeps it disabled is refused before anything is sent.
7. **The drafter of a page's own requests never states `required`.** The held draft of a request's entry holds an open question in place of `required`, which asks which values the answer may leave out and quotes the values every captured answer held; the read drafted over it is held with it. This is point 3 of decision 0322, built.
8. **Case 9, the grid, is proven in the base, and the `ui` driver's read of a grid is not built.** The base finds a row by the key its first cell shows (`By::RowKeyed`) and reads each cell of it (`ReadWhat::Cell`); the proof types the key into the grid's finder and asks the application what the page was told. The `ui` driver reads in a tab that cannot act, which cannot type into a finder or scroll, so a grid that must be filtered to show its row needs either an acting tab for the read or the network. Which of the two is a decision for the `grid` widget driver of package 8.4; this record makes neither.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A frame as a separate kind of locator | A scope that is entered is what a frame is; `within` already existed, and the screen's `frame` is only the words for putting every locator of one screen into one scope |
| Answering any dialog the page opens, as a convenience | Rule 7: an unknown dialog stops. The action says what it expects, and the driver compares the words |
| Waiting for a declared question on every action | A question the page asks on some presses would make the other presses wait out their time. The variant is looked at when the page answers, which is also when a question is asked |
| `date` as its own rule that clicks the picker | The picker is another kind of control and is 8.4's; the browser's date field takes its ISO value from the field's own setter, and the text rule already compares what the field holds with what it was given |
| A `grid` read in an acting tab now | A read that may type into a page is a read that may write by accident (decision 0195); that is not settled by a test page |

## Proof

In `crates/a2m-pack-web-browser/src/tests/cases.rs`, one test for each of cases 6 to 9, 11, and 12 at the base; in `crates/a2m-pack-web-ui/tests/cases.rs`, the driver's tests for cases 6, 7, 8, 11 (a declared question, a notice, and an undeclared dialog), 12, and 33 (each native control set and read back, after a dialog of the browser, with the application asked what it stored and how many times, and the refusals that store nothing); in `crates/a2m-pack-web-ui/tests/page_http.rs`, the held draft that never states `required`.

## What would reopen it

A page of the test application, or of a real application, whose question cannot be told from its confirmation by the page's words alone; or a frame whose origin differs from its page's and that a person has a reason to drive.
