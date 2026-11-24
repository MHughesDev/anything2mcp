# 225. Performance

Speed is a design metric with budgets, not a hope. The rule: Anything2MCP should add almost nothing on top of the time the target system itself takes, and should always take the fastest path that has been proven correct. This section sets the budgets and how they are measured. Built today: the benchmark harness runs in CI and measures six of the budgets below (package 1.7, [decision 0363](../../decisions/0363-the-budgets-as-first-measured-two-are-revised-and-four-cannot-yet-be-told-from-the-systems-own-time.md)); the comparison holds the floor of each number and runs in CI when a baseline is stored, and none is yet ([decision 0369](../../decisions/0369-the-comparison-holds-the-floor-of-each-number-and-the-program-has-a-size-budget.md)).

**Budgets.** These are targets, measured once by package 1.7; a budget is confirmed or revised by decision record, and decision 0363 did so for each row. The measured numbers are in the next section.

| Path | Budget for Anything2MCP's own overhead |
| --- | --- |
| Process start to ready | Under 100 ms with the vault's key from the machine's key store, and under 600 ms on the passphrase route, which derives its key on purpose; no browser starts until a `ui` call needs one |
| Listing tools | Under 5 ms, served from a cache keyed by registry version |
| Policy check, scheduling, and ledger write per call | Under 3 ms |
| `http` and `api` operations | Under 15 ms beyond the system's response time, through the command front door and the durable ledger; 5 ms is the aim for the engine's own loop |
| `page_http` operations | Under 20 ms on a warm page |
| `ui` steps | Under 50 ms beyond the system's response time, on a warm tab |
| Getting a tab on a warm, signed-in browser | Under 100 ms; a cold browser start under 1 s |
| Idle memory of the server | Under 50 MB |
| Size of the program | Under 25 MB, the stripped release program of every first-party pack; the wheel that carries it is smaller |

**The budgets, first measured (package 1.7).** Release build, Linux x86-64, one machine shared with other builds, against the test application, with the ledger on disk. The machine was busy: its load average was about 7 in the lighter run and 20 to 37 in the heavier, so every median is an upper bound and the minimum is the better floor. The program is `anything2mcp serve` as a child process; the other scenarios open the engine in the harness's own process and call a tool through the product's commands (`bench/src/measure.rs`). The stored run is `bench/results/a2m-bench-linux-x86_64-package-1-7.json`; the size is in `bench/results/a2m-bench-linux-x86_64-package-f4.json` (package F4, 2026-11-24).

| Budget row | Scenario | Lighter run | Heavier run | Standing |
| --- | --- | --- | --- | --- |
| Process start to ready | `start_to_ready`, vault key by passphrase | median 359 ms | median 338 ms, min 301 ms | Over 100 ms by the key derivation, which is deliberate; row revised to 600 ms for this route |
| Process start to ready | `start_to_ready_vault_closed` (not budgeted) | about 92 ms, by hand | median 108 ms, min 94 ms | The engine's own start is near the 100 ms budget; the key-store route is not measured, the container has no key store |
| Listing tools | `list_tools` (`tool search`, the command front door) | median 1.5 ms | median 3.5 ms | Within 5 ms; the MCP `tools/list` is not timed separately |
| `http` and `api` operations | `http_operation`, a call of a published tool with a verified `http` binding, against the same request made directly | overhead median 10.0 ms (min total 8.4 ms) | 12.5 ms | Over 5 ms; row revised to 15 ms; with the ledger in memory the overhead was 6.4 ms, so about 4 ms of it is the durable store |
| `ui` steps | `ui_step`, a read on a warm tab against a request for the page itself | overhead median 206 ms | 233 ms (min total 172 ms) | Over 50 ms, and not revised: the number includes the browser drawing the page, which no engine removes, so it is an upper bound whose product share is not known |
| Getting a tab on a warm, signed-in browser | `ui_step` bounds it; `cold_browser_start`, the first read of a new engine, includes the warm read | cold median 0.94 s | cold median 1.40 s (min 0.91 s) | Cold start within 1 s on the lighter run and over it on the heavier; unchanged; the warm tab alone is not told apart from the read |
| Idle memory of the server | `idle_memory`, resident memory of `serve` after one status | 37.3 MB | 40.3 MB | Within 50 MB |
| Size of the program | `program_size`, the stripped release program, every first-party pack | 24,042,992 bytes (24.0 MB), the gzip of it 11,113,931 | | Within 25 MB, by 0.96 MB; it was 23,688,568 at package 1.7 and 18,546,544 after the release profile, so the margin shrinks as packs deepen |
| Policy check, scheduling, and ledger write per call | no scenario | | | Not measured: only the whole call is timed, which `http_operation` bounds at 10 ms; the split needs the trace's own division of time (`run trace` is not built) |
| `page_http` operations | no scenario | | | Not measured: no benchmark project binds `page_http` |
| `api` operations | no scenario | | | Not measured: the `http` row stands for both |

**The wheel and its install (package 1.7, the same machine).** A wheel built locally with maturin 1.16.0 is 11.4 MB, with the stripped program in it 23.7 MB, under the 25 MB target of H-28 for both; installing it into an empty environment took 0.6 s, against a 10 s target; `anything2mcp --version` runs in 6 ms from the installed program and 16 ms through the script `a2m` (hypothesis H-39: the script adds about 10 ms). `bindings/python/tools/try_wheel.py` repeats this for any built wheel. Only Linux x86-64 was measured.

**A binding gained through shadow.** The scenario `promotion_speedup` in `bench/` reads one line item of the test application through a `ui` binding, adds an `http` binding in shadow, promotes it on the evidence of six distinct inputs, and reads again. In a release build on Linux, the factor was 12.4, 11.8, 12.9, and 11.9 in four runs of the test alone (for example 140.9 ms through the interface against 11.3 ms through the request binding), with every result identical to the interface's and the application hearing the JSON request, not the page. Run beside other heavy work the factor fell to 6.8, so the test is run alone. The test (`bench/tests/promotion.rs`) holds the factor at ten or more in a release build, and CI runs it with `--release`. It times the two paths in pairs, alternating which goes first, and asserts both the ratio of the medians and the median of the per-pair ratios at ten or more ([decision 0383](../../decisions/0383-the-promotion-exit-test-measures-the-two-paths-in-pairs.md)). The promotion does not yet put the faster binding first; a person does, in the file (roadmap package 4.6). No budget changes.

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

**Measured, always.** A benchmark suite runs in CI against a local test stand for every mode. A change that slows a budgeted path by more than 10% is to fail the build. The comparison (`a2m-bench compare`) holds the floor of each number and not its median: the fastest iteration of a time, the fastest operation less the fastest baseline of an overhead, the one reading of a memory or a size ([decision 0369](../../decisions/0369-the-comparison-holds-the-floor-of-each-number-and-the-program-has-a-size-budget.md)). On one shared machine the median of `http_operation` moved from 10 ms to 31 ms between runs of the same build, and the minimum moved far less, because load only adds time. A budget's limit is still the median's. CI compares a run with `bench/baseline/<os>-<arch>.json` when a person has stored one from a quiet runner; none is stored yet, so no build is gated, and the step says so. Each run's trace records time spent in Anything2MCP against time spent waiting on the system, so slow tools are visible in production.
