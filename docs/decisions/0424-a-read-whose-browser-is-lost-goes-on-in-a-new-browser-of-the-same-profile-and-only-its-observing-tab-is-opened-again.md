# 0424. A read whose browser is lost goes on in a new browser of the same profile, and only its observing tab is opened again

**Status:** Accepted. Design decision, made by the worker on package 9.3e under the lead's brief. The owner had not read this record when it was written. It changes what the `ui` driver does at the start of a step; it adds `ObserveTab::browser_lost` to the browser engine; no public contract changes.

**Date:** 2026-11-24

**Changes:** `a2m-pack-web-ui` (`src/driver.rs`), `a2m-pack-web-browser` (`src/tab.rs`); the designs of both crates; [section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md)'s map.

## Context

Section 224 says that when the web pack's browser crashes, reads retry on a new browser, and the sign-in survives because the browser is started again from the same profile. Before this record the browser engine already started a new browser for a profile whose browser had exited, at the next `open`. Nothing used it inside a run. The `ui` driver opens its observing tab at prepare and keeps it. The engine tries a `transient` step that commits nothing again on the same prepared context, so after a lost browser each try met the same dead tab and failed `connection_lost` again, until the tries ran out.

The page a step acts on is the one the steps before it reached. A step tried again in a new tab sees a blank page, so the tab's page has to be reached again before the step can run.

## Decision

1. **The observing tab is opened again when its browser is lost.** At the start of a step that runs in the observing tab, the `ui` driver asks the tab whether its browser is lost (`ObserveTab::browser_lost`: the engine's link to the browser is closed). If it is, the driver places the session's state into the profile's browser, which the browser engine starts afresh from the same data directory and whose dead process it reaps first. It then opens a new observing tab there, with the same environment, origins, and asserted reads. The driver does not retry anything itself. The step fails `transient`/`connection_lost` as before, and the engine's own retry, bounded by `transient_retries`, finds the new tab. A step that starts after the loss was already seen is helped on its first try.
2. **The page is reached again by the steps that reached it.** The session keeps the last `goto` made in the observing tab, with its arguments, and each `wait` made there since. For a step that is not a `goto`, those steps are done again in the new tab before the step. In that tab the browser fails every request that could change the system, and the tab gives no input, so these steps are the whole of what made its page. A `wait` done again that now finds the record absent stops the step as `unknown_variant`/`unexpected_page`. A failure of a step done again is that step's failure, under the same rules as before.
3. **No sign-in, no new state.** The state placed is the one the run's lease handed over at prepare, under the same mark, so the session keeper stays the only renewer. A page that shows a declared sign of expiry stops as it would have the first time, and the engine tells the keeper.
4. **The acting tab is never opened again.** A step in the acting tab, after its browser was lost, fails as before. A write that may have been sent is `possible_commit`, and one that was not sent is not resent through a tab the driver opened in its place.

## Alternatives

| Alternative | Why not |
| --- | --- |
| The engine releases the lost context, prepares a new one, and runs the binding again from its first step | The engine would have to know which failure means a lost browser and which bindings may be started again. Each kind of driver would earn a new rule in the kernel for what one pack knows. The engine's per-step retry already exists and is bounded |
| Open the tab again only at a `goto`, and let other steps fail | A browser lost under a `wait` or a `read` would fail the run though the read was safe to make again. The proof of 9.3e kills the browser between a read's `wait` and its `read` |
| The driver retries the step itself after opening the tab again | It would be a second retry, uncounted, beside the engine's |
| Also open the acting tab again | A write's steps would then go on in a browser the run did not choose, and the line between a read that may be made again and a write that never is would lie inside the driver's choice of tab |

## Consequences

- A read through `ui` survives a lost browser in any of its steps. The test application counts one sign-in, and no process is left. Proven by `a_browser_lost_under_a_read_is_started_again_from_its_profile_and_the_read_completes_signed_in` (`crates/a2m-pack-web-ui/tests/sign_in.rs`): the browser is killed under the read's `goto`, which the application holds, and again between its `wait` and its `read`.
- A `wait` that dismissed a notice is done again with its dismissal. In the observing tab that cannot click, so such a `wait` fails there as it did the first time.
- Not built: the same for `page_http`'s reads, whose observing tab is opened at prepare in the same way. A lost browser under one still fails each try. The trace does not say that a step was helped by a new browser.
- What would reopen it: a page whose state, in a tab that can only read, depends on more than its `goto` and its `wait` steps.
