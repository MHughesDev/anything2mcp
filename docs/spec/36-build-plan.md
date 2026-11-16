# 36. Build plan

Ten phases from an empty repository. Each ends in something usable and is closed by an exit test, not a date.

1. **Phase 0: Foundations and spikes.** Workspace, CI on Windows, macOS, and Linux, file schemas, the test application, the benchmark harness, and reserving the package names. Three spikes: the browser engine base, an `rmcp` server over both transports, and the desktop accessibility library.
   - Exit: each spike has a written decision; benchmarks run in CI.
2. **Phase 1: Solo read path.** Core types, the file loader and compiler of section 46, the step trace of section 47, the engine loop, the `ui` driver's read verbs, the local engine with its channel and on-demand start, the action registry with its command and MCP faces for the first few actions, MCP over stdio, traces.
   - Exit: a read operation on the test app gives five identical runs; two clients share one engine and one session; a locally built wheel installs and runs with `pip`; budgets are measured and confirmed or revised.
3. **Phase 2: Sessions.** Auth discovery, managed and existing sign-ins, the vault and key layout of section 48, the session keeper as designed in section 44.
   - Exit: an expired session stops every call with `needs_login`; ten runs that meet an expired session cause exactly one renewal; a read on one real app chosen by the owner, whose target files stay outside this repository, gives five identical runs; no secret appears in any trace, capture, or log.
4. **Phase 3: Writes.** The policy engine of section 45 with the built-in local policy set, identity strategies and dependent records from section 50, file-input rules and protected settings from section 51, the ledger as designed in section 42 with its rules as tests, reconcile, field types and normalization, sources, preview, the local approval and sign-in surfaces of section 49, read-back.
   - Exit: a spreadsheet-entry pipeline passes the round trip, idempotency, resume, and fault-injection gates on a test record.
5. **Phase 4: HTTP modes and endpoint mining.** `page_http`, `http`, and `api` drivers; capture, inference, shadow, promotion; description import.
   - Exit: an operation gains an HTTP binding through shadow verification, returns identical results, and runs at least ten times faster than its UI binding.
6. **Phase 5: Authoring and ease of use.** Every authoring action in the registry with all faces generated, the project folder and build state, the seven commands, `doctor`, templates, the guidance file, the Python and TypeScript SDKs, and the release pipeline for pip, npm, crates, and standalone installs.
   - Exit: an agent with no prior knowledge takes a new app from URL to published read tools; first tool in under ten minutes, measured, using only the guidance file, `status`, and `docs`; a tagged pre-release installs from pip and npm on all three operating systems.
7. **Phase 6: Server.** Streamable HTTP, OAuth resource server, toolsets, grants and roles compiled to policies, versioned policy sets with policy tests, decision explanations, REST API and SDKs, admin console, Postgres, approval pages.
   - Exit: two users with different grants share one Team server; a Copilot Studio agent connects through one of the three bridges.
8. **Phase 7: Runners and identity modes.** Control-plane and runner split over the protocol in section 43, enrollment, leases and lost-lease handling, service accounts, On-Behalf-Of tokens, the hosted sign-in page, user-device runners.
   - Exit: one pipeline runs correctly in all four identity modes; killing a runner mid-write leaves no duplicate and no unverified record after automatic reconcile.
9. **Phase 8: Breadth.** Desktop driver, MCP pass-through, site tools, platform profiles and widget drivers for the common platforms, the edge-case handlers.
   - Exit: every hard case in the test app passes on three operating systems; one desktop target is published.
10. **Phase 9: Operations and hardening.** Canaries, repair loop, audit chain, alerts, upgrades, backup, every failure in section 52 as a fault-injection test, the manipulation test suite of section 51, the Cluster profile, an outside security review.
    - Exit: a deliberately changed screen is caught by a canary and repaired with no failed write; review findings are closed.
