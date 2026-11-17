# 9. Performance

Speed is a design metric with budgets, not a hope. The rule: Anything2MCP should add almost nothing on top of the time the target application itself takes, and should always take the fastest path that has been proven correct.

**Budgets.** These are initial targets to be confirmed by benchmark in phase 1, then enforced in CI.

| Path | Budget for Anything2MCP's own overhead |
| --- | --- |
| Process start to ready | Under 100 ms; no browser starts until a `ui` call needs one |
| Listing tools | Under 5 ms, served from a cache keyed by registry version |
| Policy check, scheduling, and ledger write per call | Under 3 ms |
| `http` and `api` operations | Under 5 ms beyond the app's response time |
| `page_http` operations | Under 20 ms on a warm page |
| `ui` steps | Under 50 ms beyond the app's response time, on a warm tab |
| Getting a tab on a warm, signed-in browser | Under 100 ms; a cold browser start under 1 s |
| Idle memory of the server | Under 50 MB |

**How the design gets there.**

- **Rust, one process, async I/O.** No interpreter start-up, no garbage-collection pauses, and no driver subprocess between the engine and the browser.
- **Compiled plans.** At publish time, target files are validated and compiled into a binary plan: resolved references, pre-compiled CEL expressions, pre-built request templates, and pre-parsed schemas. Nothing is parsed at call time.
- **Fastest verified mode first.** When several bindings are verified, the runtime prefers the cheapest: `api`, `http`, `page_http`, then `ui`. A spreadsheet-entry pipeline that takes minutes through the UI can take seconds over HTTP once that binding is promoted.
- **Event-driven waits.** The browser engine waits with in-page observers and protocol events, never on a polling timer. One Rust engine reports a median of about 1 ms to notice an element this way, against roughly 100 ms for a polling cadence ([ferrous-browser, vendor benchmark](https://github.com/theoxfaber/ferrous-browser)).
- **Warm pools.** Signed-in sessions, their browsers, and HTTP/2 connections stay warm and are reused across calls and callers.
- **Read-back without a second trip.** Where the save response carries the stored values, the read-back uses it instead of issuing another request.
- **Parallelism by default.** Reads fan out up to the session's limits; pipeline records run in parallel where the binding and the app allow it.
- **Batched browser commands.** Locating, checking, and acting are sent as one in-page call where possible, cutting round trips.
- **Group-committed ledger.** Intent records from concurrent runs are written to disk together, so durability does not cost one disk sync per record.
- **Lean results.** Results are concise by default, with large data passed as resource links, which also saves the calling agent's tokens and time.

**Guardrails.** Speed never overrides the first two priorities. A faster binding is used only after shadow verification. Rate budgets per app still apply, so Anything2MCP is never faster than the app owner allows.

**Measured, always.** A benchmark suite runs in CI against a local test app for every mode. A change that slows a budgeted path by more than 10% fails the build. Each run's trace records time spent in Anything2MCP against time spent waiting on the app, so slow tools are visible in production.
