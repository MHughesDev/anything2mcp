# 0422. A tab that answers not even a probe is closed, and three hangs recycle its browser between runs

**Status:** Accepted. Design decision, made by the worker on package 9.3f under the lead's brief. The owner had not read this record when it was written. It adds two settings and one field to the browser engine's interface, all additions: `BrowserSettings::hangs_before_recycle` and `hang_probe`, with the named defaults `HANGS_BEFORE_RECYCLE` and `HANG_PROBE`, and `OpenBrowser::hangs`.

**Date:** 2026-11-24

**Changes:** `a2m-pack-web-browser` (`src/tab.rs`, `src/browsers.rs`, `src/protocol.rs`); the test application's page `/traps/hangs`; [section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md)'s map; the design of `a2m-pack-web-browser`.

## Context

Section 224 says that when a tab hangs, the step times out with its declared class, the tab is closed, and the browser is recycled after repeated hangs. Only the first was true. No document said what makes a tab hung, how many hangs recycle a browser, or where that count is set.

A deadline that passes does not by itself say a page hung. A wait whose condition never holds ends at its deadline while the page answers; a page whose answer races the deadline is slow, not stuck; and the web pack's interface driver waits in short slices, treating each slice's timeout as ordinary. Closing a tab at every timeout would close tabs that work.

Measured on Chrome for Testing 153 on Linux, against the test application's `/traps/hangs`, whose own script never returns:

- the page answers nothing on its session, a `Runtime.evaluate` of a constant included;
- `Target.closeTarget` removes the tab from the browser's list, and the renderer process goes on running the loop, one core busy, after the tab is gone;
- `Page.crash` is carried out by the page itself, so a hung page does not take it;
- `Runtime.terminateExecution` is answered at once, ends the loop, and the renderer is idle after it.

## Decision

1. **A hang is a page that does not answer a probe.** When a call on a tab's session ends at its deadline, or is dropped before its reply (the engine drops a step's future at the step's time limit), and when a `goto` passes its deadline before its document is parsed, the tab is probed on a task of its own, so the caller's step ends at its own deadline: the page is asked for a constant, in the helper's world where there is one, and given `hang_probe` to answer. A page that answers is slow and its tab goes on. A page that gives no answer is hung. A call that ended for a named reason (a crash, a dialog, a lost browser, a refusal) is no hang and is not probed. A sign-in tab and a watching tab are a person's and are never probed or closed by the engine.
2. **A hung tab is closed.** Its script is ended with `Runtime.terminateExecution`, so its renderer stops; the browser is asked to close its page and the windows it opened; the engine waits, up to the call deadline, for the browser to say the tab's session is detached. Every call on the tab from then on is `transient`/`timeout` at once, with nothing sent. The handle stays with whoever opened the tab until it is dropped.
3. **The hang is counted against its browser.** `OpenBrowser::hangs` says how many. When the count reaches `hangs_before_recycle`, the next `Browsers::open` of that profile that finds no tab of the browser held closes the browser, as `Browsers::close` does, and starts it afresh. A run holds its tabs for its whole length, the hung one included until it is dropped, so a browser is recycled between two runs and never inside one. The count of a new browser starts at nought.
4. **The defaults.** `HANGS_BEFORE_RECYCLE` is 3: one hang may be one page's bug, and three in one browser say the browser is worth starting afresh at the cost of one cold start. `HANG_PROBE` is 5 seconds: long enough that a page busy for a few seconds is not called hung, short enough that a hung tab is closed well within the next run's start. Both are settings of `BrowserSettings`, the struct that holds every other bound of the browser engine; neither is a key of the pack's settings group yet, since nothing has measured a need for a person to change them. A count of 0 is taken as 1. Both are drafts in the sense of section 226: set when first measured on real pages.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Close a tab at every timeout of a call | A wait that races its deadline, and every slice of the driver's sliced waits, would close tabs that work; the existing test of a page busy for three seconds shows the tab must go on |
| Let the crate above close the tab, by dropping it | The engine drops the step's future at the time limit; the crate above has no word of whether the page is stuck, and every driver would repeat it |
| Recycle the browser at the hang itself | Inside a run: the browser keeps the session's cookies in memory (decision 0245), and other tabs of the run would lose them |
| Kill the hung renderer's process by its number | The crate starts and ends processes only through its supervisor, and no protocol answer says which renderer is the tab's; ending the script does the same through the protocol |
| `Page.crash` | Carried out by the page, which a hung page never does (measured) |
| The count of hangs a key of the pack's settings | No one has measured a reason to change it; a key can be declared when one is |

## Consequences

- A hung tab costs its step the step's own deadline, and the browser the probe's time and a close, on a task of its own.
- A renderer held by something other than its own script, such as a loop in layout, is not ended by `Runtime.terminateExecution`; its tab is still closed and counted, and the recycle ends its process between runs.
- Proven by `crates/a2m-pack-web-browser/src/tests/hanging.rs`, `a_hung_tab_ends_its_step_as_a_timeout_is_closed_and_repeated_hangs_recycle_the_browser_between_runs`; a page busy for three seconds keeps its tab, in `a_wait_ends_on_an_event_of_the_page_and_never_on_a_guess`.
