# 225. Performance

Speed is a design metric with budgets, not a hope. The rule: Anything2MCP should add almost nothing on top of the time the target system itself takes, and should always take the fastest path that has been proven correct. This section sets the budgets and how they are measured. Built today: the benchmark harness runs in CI; no budget is measured yet, and the 10% gate is not wired.

**Budgets.** These are targets. None is measured yet; package 1.7 measures them, and a budget is then confirmed or revised by decision record.

| Path | Budget for Anything2MCP's own overhead |
| --- | --- |
| Process start to ready | Under 100 ms; no browser starts until a `ui` call needs one |
| Listing tools | Under 5 ms, served from a cache keyed by registry version |
| Policy check, scheduling, and ledger write per call | Under 3 ms |
| `http` and `api` operations | Under 5 ms beyond the system's response time |
| `page_http` operations | Under 20 ms on a warm page |
| `ui` steps | Under 50 ms beyond the system's response time, on a warm tab |
| Getting a tab on a warm, signed-in browser | Under 100 ms; a cold browser start under 1 s |
| Idle memory of the server | Under 50 MB |

**First measurement: a binding gained through shadow.** The scenario `promotion_speedup` in `bench/` reads one line item of the test application through a `ui` binding, adds an `http` binding in shadow, promotes it on the evidence of six distinct inputs, and reads again. In a release build on Linux, the factor was 12.4, 11.8, 12.9, and 11.9 in four runs of the test alone (for example 140.9 ms through the interface against 11.3 ms through the request binding), with every result identical to the interface's and the application hearing the JSON request, not the page. Run beside other heavy work the factor fell to 6.8, so the test is run alone. The test (`bench/tests/promotion.rs`) holds the factor at ten or more in a release build, and CI runs it with `--release`. The promotion does not yet put the faster binding first; a person does, in the file (roadmap package 4.6). No budget changes.

**How the design gets there.**

- **Rust, one process, async I/O.** No interpreter start-up, no garbage-collection pauses, and, in the web pack, no driver subprocess between the engine and the browser.
- **Compiled plans.** At publish time, target files are validated and compiled into a binary plan: resolved references, pre-compiled CEL expressions, pre-built request templates, and pre-parsed schemas. Nothing is parsed at call time.
- **Fastest verified mode first.** When several bindings are verified, the runtime prefers the cheapest. Where an operation states no order, the compiler sets it from each mode's rank in its provider's manifest; for the first packs that gives `api`, `http`, `page_http`, then `ui`. A spreadsheet-entry pipeline that takes minutes through the UI can take seconds over HTTP once that binding is promoted.
- **Event-driven waits.** Every wait is on a named condition. In the web pack, the browser engine waits with in-page observers and protocol events, never on a polling timer. One Rust engine reports a median of about 1 ms to notice an element this way, against roughly 100 ms for a polling cadence ([ferrous-browser, vendor benchmark](https://github.com/theoxfaber/ferrous-browser)).
- **Warm pools.** Signed-in sessions, their browsers, and HTTP/2 connections stay warm and are reused across calls and callers.
- **Read-back without a second trip.** Where the save response carries the stored values, the read-back uses it instead of issuing another request.
- **Parallelism by default.** Reads fan out up to the session's limits; pipeline records run in parallel where the binding and the system allow it.
- **Batched browser commands.** In the web pack, locating, checking, and acting are sent as one in-page call where possible, cutting round trips.
- **Group-committed ledger.** Intent records from concurrent runs are written to disk together, so durability does not cost one disk sync per record.
- **Lean results.** Results are concise by default, with large data passed as resource links, which also saves the calling agent's tokens and time.

**Guardrails.** Speed never overrides the first two priorities. A faster binding is used only after shadow verification. A binding in shadow holds a live read up by at most its time limit, 2 seconds unless set, once per operation in a job; past it the shadow is cancelled and counted as not compared ([decision 0207](../../decisions/0207-a-shadow-binding-reads-beside-the-verified-one-and-a-person-promotes-it-on-a-run-of-exact-matches.md)). Rate budgets per surface still apply, so Anything2MCP is never faster than the system's owner allows.

**Measured, always.** A benchmark suite runs in CI against a local test stand for every mode. A change that slows a budgeted path by more than 10% is to fail the build. The comparison exists (`a2m-bench compare`); CI runs the harness and stores its results, and does not run the comparison yet, because no budgeted path is measured and no baseline is stored. Each run's trace records time spent in Anything2MCP against time spent waiting on the system, so slow tools are visible in production.
