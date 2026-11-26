# 0488. A read's observing tab is kept warm for the next read of its surface and session, made fresh first

**Status:** Accepted. Design decision, made by a worker under the lead's brief. The owner had not read this record when it was merged. It changes no public contract of the provider: the web pack's base gains `Observer::warm_observing_tab`, `Observer::keep_warm`, `Observer::warm_tabs`, and `ObserveTab::reused`; `Observer::observing_tab` is as it was.

**Date:** 2026-11-25

**Changes:** [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), the set of browsers (`browsers.rs`) and the tab (`tab/warm.rs`); [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md), the `ui` driver's prepare and release; the row "Getting a tab on a warm, signed-in browser" of [section 225](../spec/2-kernel/225-performance.md). It carries out the "warm pools" of section 225 for observing tabs, which [decision 0481](0481-a-runs-trace-ends-with-the-time-each-part-of-the-control-plane-took.md), point 7, named as the design that would meet the budget.

## Context

Decision 0481 measured getting a tab on a warm browser at a median of about 143 ms against a budget of 100 ms. Its reason was the product's own: every `ui` read opens a new observing tab, and the browser makes a page and a renderer for it, and the tab's lock is set on with some ten protocol calls, before the read's first step. A read of the same surface and session right after would be given the same tab, set up the same way.

A tab kept for another read must not carry the first read's state into it. A tab's state is of two kinds. What the browser context holds, its cookies, its local storage, and its cache, every tab of the profile's browser shares, so a new tab is given it too: keeping a tab adds nothing of it. What the tab alone holds is its document and the page's script, its history, its session storage, a dialog, a window or a download it began, the page's tools, the requests it refused or awaits, and what the engine recorded of it.

## Decision

1. **Kept only when the read went as written.** On release, the `ui` driver keeps the observing tab warm only when the run was not poisoned, opened no acting tab, every step went as written (none failed, a record found absent counts as written), the tab refused nothing, not even what a page made and a read goes on past ([decision 0477](0477-a-read-goes-on-past-what-an-observing-tab-refused-when-the-page-made-it-and-the-run-says-so.md)), and no step left the tab's browser in doubt. Every other tab is closed, as before.
2. **Made fresh before it is kept.** The browser engine keeps a tab (`Observer::keep_warm`) only when it holds nothing a new tab would not: it is open and answering and not being probed, nothing it refused is untaken, no dialog is open, no window it opened is there, no download was allowed or began, the page's tools were never heard, it never recorded exchanges, and its browser is still its profile's and is not ending. It must also hold no frame, and have one origin of the web it may be on, with its document on it, or on none if it never loaded one. Then the session storage of that origin is cleared while the document is still there, and read back as empty; the document is erased, as at a release, so its leaving runs nothing of the page's; the engine sends the tab to `about:blank` and waits for that document to be parsed; its history is reset, and read back as that one entry; and, once more, nothing was refused and no request of the page awaits its answer. What the engine noted of the page goes. A step that does not hold closes the tab.
3. **Handed only to a read that would open it the same.** A kept tab is handed out by `Observer::warm_observing_tab` only for the same running browser of the same profile, the same fixed environment, the same asserted reads, the same origins, and the same session and epoch, or no session for a read with none. Its interception, its environment, its socket script, and its asserted reads are as they were set when it was opened, so it refuses what a new tab of that setup refuses. Before a tab is handed out, those whose browser is no longer its profile's, or that are no longer as new, are closed.
4. **A kept tab is not a held tab.** It does not count among the tabs held, so a browser with only kept tabs is between two runs: it is measured for memory and recycled as one with none ([decision 0425](0425-a-browsers-memory-is-measured-from-its-own-list-of-its-processes-and-past-a-ceiling-no-new-browser-work-starts.md), [decision 0422](0422-a-tab-that-answers-not-even-a-probe-is-closed-and-three-hangs-recycle-its-browser-between-runs.md)), and its kept tabs are closed with it. At most four tabs are kept in all the browsers together; past that the oldest is closed.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Open a spare tab for the next read once a read ends, and never hand out a used one | Carries nothing between reads by construction, and costs a page and a renderer for every read all the same, opened in the time between two calls; a read that comes before the spare is ready waits for it. Kept as the fallback if a state a tab holds is found that cannot be cleared |
| Clear the origin's storage in the browser context too | A new tab is given that storage, so clearing it is no part of what a fresh tab is; and it would take it from a read of the same profile running beside |
| Keep a tab whose page held a frame, clearing each frame's storage by its storage key | A frame of another site keeps a storage partitioned by its top site, which needs the browser's key for each frame; a page with a frame is rare among the reads measured, so it is closed instead |
| Revise the budget to 150 ms | The number was of a choice the product makes, as decision 0481 said |

## Proof

`a_tab_kept_warm_is_handed_to_the_next_read_of_its_session_fresh_and_refusing_what_a_new_tab_refuses` (`crates/a2m-pack-web-browser/src/tests/warm.rs`): a read leaves session storage, a global of its page, and a history entry; the tab is kept and counts as no tab held; a read of another session or another setup is given a new tab; the next read of the session is given the same tab, finds none of what the first left, still has a write of its page refused, and the stand stores nothing; a tab with an untaken refusal is closed, not kept; a closed browser closes its kept tabs. Without the clearing of session storage the test fails: the next read finds one entry. `a_read_that_went_as_written_leaves_its_tab_warm_for_the_next_and_one_that_did_not_leaves_none` (`crates/a2m-pack-web-ui/tests/driver.rs`): three reads in a row each leave one tab kept and none held; a run released as poisoned, and a read whose locator is not found, leave none; a read that finds its record absent leaves one. The shared-worker read of decision 0477 leaves none.

Measured with `warm_tab` in a release build on Linux x86-64, 40 reads after 5: a median of 42.5 ms (minimum 30.6 ms), and 42.4 ms (minimum 31.5 ms) in a second run, against the budget of 100 ms; with the keeping switched off in the same build on the same machine, under a heavier load, 186.1 ms and 154.8 ms. The budget is confirmed in [section 225](../spec/2-kernel/225-performance.md); the stored run is `bench/results/a2m-bench-linux-x86_64-warm-tab-kept.json`.

## Revisit if

A state a tab alone holds is found that these steps do not clear (then the spare tab of the alternatives); or reads of one surface run side by side often enough that four kept tabs are too few.
