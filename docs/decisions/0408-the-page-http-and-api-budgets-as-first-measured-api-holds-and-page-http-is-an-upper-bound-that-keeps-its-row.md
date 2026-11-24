# 0408. The `page_http` and `api` budgets as first measured: `api` holds, and `page_http` is an upper bound that keeps its row

**Status:** Accepted. Design decision. The owner had not read this record when it was merged. Carries out package 1.9, and confirms two rows of the budget table in [spec section 225](../spec/2-kernel/225-performance.md) that [decision 0363](0363-the-budgets-as-first-measured-two-are-revised-and-four-cannot-yet-be-told-from-the-systems-own-time.md) could not measure.

**Date:** 2026-11-24

**Changes:** the harness in `bench/` (two scenarios, `api_operation` and `page_http_operation`, and the two projects of target files they read through, `bench/projects/api` and `bench/projects/pagehttp`); section 225's account of what was measured. It changes no limit in `bench/budgets.json`.

## Context

Decision 0363 left the `page_http` and `api` rows with no scenario, because no benchmark project bound either mode. Package 1.9 adds one for each, on the model of `http_operation`: an engine over the test application, a published tool with one verified binding of the mode, a read of it through the product's commands as an agent, and the same request made straight to the application as the baseline. Before a scenario times anything it checks what the application heard: the `api` read must have fetched the published description and then the item, and never opened the page; the `page_http` read must have opened the order's summary page and sent the totals request. The test of `bench/tests/measure.rs` holds the values read against what the application itself says it stores.

## Decision

1. **The `api` row stands at 15 ms.** The scenario `api_operation` read a line item through a verified `api` binding pinned to the test application's description; the overhead median was 8.96 ms and the floor of the read 6.5 ms, on a machine whose load average was 26 at the end of the run. The description's second read, once a pass, which decision 0389 left unmeasured, is inside that number. It is within the budget the `http` row already has.
2. **The `page_http` row stands at 20 ms and is not met by this scenario.** The overhead median was 245 ms (the floor of the read 118 ms, the baseline 1.4 ms), with the application's deliberate slowness of its totals taken out. The driver opens the order's summary page in an observing tab for each call and reads from inside it, so the number holds a page navigation and the browser's own work, which no engine removes, exactly as `ui_step` does. The row reads "on a warm page", and the scenario cannot tell the warm page apart from the page's opening, so the number is an upper bound whose product share is not known. The limit is not revised: no number says it is wrong, and revising it to the measured upper bound would write the browser's time into a budget about the product's. The row is held to the same split as the `ui` and warm-tab rows, which package 1.9b gives (time marks in the trace).
3. **The test application's slowness is taken out of both reads.** The totals endpoint sleeps 200 ms by design (case 4). A scenario sets that to nothing before it measures, because a test stand's time is never the product's. A first run that left it in read 242 ms of overhead, and the second 245 ms: the slowness was counted once on each side and cancelled, so the figure is the same, and the baseline is now honest.

## Alternatives weighed

- **Revise `page_http` to the measured 245 ms.** Not taken: the number is the browser's, and a budget that the browser's time can satisfy tells nothing of the product.
- **Hold the row's scenario back until a page can be kept open across calls.** Not taken: the driver's page handling is its design, and the scenario measures what a call costs today.
- **Bind `api` through an endpoint of the HTTP pack's own, as `http`.** It would be `http_operation` again. The point of the row is the description's check before the first call.

## Consequences

- The budget table now has a measured number, or a stated reason, for every row but two: the split of a call and the warm tab alone (package 1.9b).
- The Windows and macOS numbers of both new scenarios are read from the benchmark results CI stores; none is in this record.
- A quiet runner could show `page_http` under 20 ms only if the page were kept open; nothing here says it is.

## What would reopen it

The trace's division of time showing the product's share of a `page_http` call under or over 20 ms; a driver that keeps a page open between calls; or an `api` run on a quiet runner over 15 ms.
