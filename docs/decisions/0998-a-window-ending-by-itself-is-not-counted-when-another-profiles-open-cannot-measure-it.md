# 0998. A window ending by itself is not counted when another profile's open cannot measure it

**Status:** Accepted. Design decision, made by the worker who found an intermittent failure of the capture test, under the lead's brief. The owner had not read this record when it was merged. It changes no public contract.

**Date:** 2026-11-25

**Changes:** [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), the memory check of `Browsers::open` (`browsers.rs`). It narrows the third point of [decision 0425](0425-a-browsers-memory-is-measured-from-its-own-list-of-its-processes-and-past-a-ceiling-no-new-browser-work-starts.md), that an open starts nothing while one browser cannot be measured, for browsers that are ending; the rest of 0425 stands.

## Context

Once, `capture_use::a_person_s_use_of_pages_drawn_on_the_server_drafts_a_list_and_a_record_held_that_a_person_releases_and_tries` stopped at `draft try` with `busy`/`memory_short`: "an open browser could not be measured: it did not say which processes are its own". The cause: the person's use is watched in a window of the profile `WATCHING_ACCOUNT`, and the run that `draft try` starts opens a browser of the run's own profile. When the capture stops, the watching tab and the window's last handle go, and with the handle the window's notice tab; a browser with a window and no window left ends by itself. It stays among the open browsers until its link closes. The run's open measures every open browser that still runs: when it asks the ending browser for its processes at the moment the browser is closing, the call fails, and by decision 0425 one browser that cannot be measured stops every new run.

A browser that is ending gives its memory back; its silence says nothing about memory running short.

## Decision

1. **A window whose every handle is gone is marked going.** When the last handle of a window's browser goes, its notice is closed and the browser is marked going before the notice tab closes: it has no window left and ends by itself.
2. **An ending browser that cannot be measured counts for nothing.** In the memory check of an open, a browser whose measurement fails, and that is marked going or whose link has closed by the time the measurement fails, is left out of the sum and out of the check, as a browser that has ended is. A going browser that is measured counts what it holds.
3. **Every other browser that cannot be measured still stops new browser work,** as decision 0425 says: a window whose handle is held and that does not answer refuses another run's open with `busy`/`memory_short`.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Have the watch close its window's browser with `Browsers::close` when it ends, as a sign-in does | The browser can still end by itself first (a person closes the window), and the close takes the same lock as every open: the race with the measurement stays. It is the probe's, and may be added too |
| Retry a failed measurement once | A fixed second try is a wait on no named condition, and a browser that is not ending would be retried for nothing |
| Leave out every browser with no tab held | A browser that does not answer and is not ending is the case decision 0425 stops on |

## Proof

`a_window_ending_by_itself_does_not_stop_another_runs_memory_check_and_a_browser_that_cannot_be_measured_still_does` (`crates/a2m-pack-web-browser/src/tests/memory.rs`): a window's browser is stopped from outside with a Unix signal, so that it neither answers nor ends. While its handle is held, another profile's open is refused `memory_short` with nothing measured; once its handle goes, the other profile's open starts its browser. Without this decision the second open fails `memory_short` every time. The test runs where a screen can be had, on Linux and macOS; on Windows no process is stopped from outside this way, and the rule is the same code.

## Revisit if

An ending browser stays among the open ones long enough to hold memory a new run needs; then the open ends it first.
