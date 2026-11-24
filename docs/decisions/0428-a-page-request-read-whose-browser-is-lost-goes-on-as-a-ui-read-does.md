# 0428. A page request read whose browser is lost goes on as a `ui` read does

**Status:** Accepted. Design decision, made by the worker on the gap that [decision 0424](0424-a-read-whose-browser-is-lost-goes-on-in-a-new-browser-of-the-same-profile-and-only-its-observing-tab-is-opened-again.md) left, under the lead's brief. The owner had not read this record when it was written. It widens 0424's scope to the `page_http` mode and changes nothing else; no public contract changes.

**Date:** 2026-11-24

**Changes:** `a2m-pack-web-ui` (`src/page_http.rs`, `src/driver.rs`); its crate design; [section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md)'s rows.

## Context

Decision 0424 made a read through the `ui` mode go on when its browser is lost, and said that `page_http`'s reads were not helped yet. The `page_http` driver opens its observing tab at prepare in the same way, so after a lost browser each try of a read met the same dead tab and failed `transient`/`connection_lost` until the engine's tries ran out.

## Decision

The design of 0424 holds for the `page_http` mode, with two differences that follow from what that mode's steps are.

1. **The same reopening.** At the start of a step that runs in the observing tab, a `goto` that reaches its page there or a `request` whose entry reads, the driver asks the tab whether its browser is lost. If it is, it places the state the run's lease handed over into the profile's browser, which the browser engine starts afresh from the same data directory, and opens a new observing tab there with the same environment, origins, and asserted reads. The step's failure, if any, is left to the engine's retry, bounded by `transient_retries`. The code that does this is one, `Reopen::observing_tab`, shared by both drivers.
2. **The page is reached again by its last `goto` alone.** The mode has no `wait` step: the `goto` waits for the page's document to load, and the `request` is made from inside it. So for a `request`, the driver does again the last `goto` made in the observing tab, with its arguments, and nothing else. A `goto` in the acting tab does not clear it, because every read of this mode is made in the observing tab whichever tab the last `goto` chose; the driver keeps the tab the steps act in as it was.
3. **A write is never sent again.** A `request` whose entry writes is sent only from the acting tab, and is never helped. After its browser is lost it fails `transient`/`connection_lost`, not sent, on every try, and no browser is started in its place. A write that had left is `possible_commit`, as before.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the `page_http` read failing, and let the person run the tool again | The sign-in survives a lost browser; a read that would complete on the engine's own retry would instead fail the run |
| Do again every `goto` since prepare | Only the last one made the observing tab's page; doing earlier ones again loads pages the read does not stand on |
| Clear the remembered `goto` at a `goto` in the acting tab, as the `ui` driver does | In this mode the read-back's `request` would then be made from a blank page, though the observing tab's page was still the one its own `goto` reached |

## Consequences

- A `page_http` read survives a lost browser under its `goto` and between its `goto` and its `request`. Proven by `a_browser_lost_under_a_page_request_read_is_started_again_from_its_profile_and_the_read_completes_signed_in` (`crates/a2m-pack-web-ui/tests/sign_in.rs`): signed in through the keeper, the application counts one sign-in, the totals read equal what the stand stored, and no browser process is left.
- A browser lost after a write's `goto` and before its request: the request fails not sent on three tries, no browser is started, the stand saw no `PUT` and stored nothing, and the read-back goes on in a new browser. Proven by `a_browser_lost_before_a_write_is_sent_never_sends_it_and_the_read_back_goes_on` (`crates/a2m-pack-web-ui/tests/page_http_write.rs`).
- Not built, as in 0424: the trace does not say that a step was helped by a new browser.
- What would reopen it: a page whose data request, made from inside the page, depends on more than the page its `goto` loaded.
