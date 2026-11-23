# Open research

Research that was planned but not completed. Each item is a ready brief: hand it to a research agent, write the result up as a new file in this folder, then update the spec section it feeds and remove the item from this list.

Run an item before the build phase that depends on it. The phase numbers refer to the [roadmap](../plan/roadmap.md).

| # | Topic | Needed before | Feeds spec section |
| --- | --- | --- | --- |
| 1 | Browser engine base. Answered by note 17 | Phase 0 | 17 |
| 2 | MCP features and dual-revision serving. Answered by note 18 | Phase 1 | 21, 22 |
| 3 | Locators and hard widgets on the DevTools protocol | Phase 1 | 13, technical reference |
| 4 | Session persistence and Entra session rules | Phase 2 | 15, 16 |
| 5 | Vault design per operating system | Phase 2 | 15, 19 |
| 6 | Web sign-in methods and auto-detection | Phase 2 | 15 |
| 7 | Durable execution and idempotency | Phase 3 | 10 |
| 8 | Human approval patterns | Phase 3 | 18 |
| 9 | Network channel practice and endpoint inference | Phase 4 | 6, 26 |
| 10 | AI discovery turned into deterministic steps | Phase 5 | 26 |
| 11 | File-format prior art | Phase 1 | 7 |
| 12 | Testing against third-party apps | Phase 3 | 27 |
| 13 | Local and remote MCP transport security | Phase 6 | 19, 22 |
| 14 | Prompt injection and MCP-specific threats | Phase 6 | 18, 19 |
| 15 | Enterprise governance of agent tools | Phase 6 | 20 |
| 16 | Hosted-agent compatibility, hands-on | Phase 6 | 24 |
| 17 | Remote sign-in to a server-hosted browser | Phase 7 | 16 |
| 18 | Deployment and scaling patterns | Phase 7 | 23 |
| 19 | Desktop accessibility automation. Answered by note 19 | Phase 8 | 6 |
| 20 | Selector repair and drift | Phase 9 | 28 |
| 21 | Lessons from RPA products | Phase 3 | 8, 10, 31 |
| 22 | Reproducibility and observability | Phase 9 | 28, 29 |
| 23 | Vendor APIs and terms of use | Before the first real target | 34 |
| 24 | WebMCP standard status. Answered by note 25 | Phase 8 | 6 |
| 25 | Gap-review components | The phase each lands in | 32 |
| 26 | Local channel on three operating systems | Phase 1 | 39 |
| 27 | Operating-system user-presence checks. Answered by note 20 | Phase 3 | 18, 39 |
| 28 | Wheel and npm packaging, verified hands-on. Answered by note 21 | Phase 1 | 41 |
| 29 | Command-line design for agents | Phase 5 | 40 |
| 30 | Sign-in in engine-owned profiles, and device-bound sessions | Phase 2 | 44 |
| 31 | Cedar spike. Answered by decision 0082 | Phase 3 | 45 |
| 32 | First standard wire protocol of the stream pack. Answered by note 22 | Milestone 8, packages J2 and J3 | 309 |
| 33 | Client crates for a server database beside the embedded one. Answered by note 23 | Milestone 8 package G3; milestone 9 package 6.1 | 307, 208 |
| 34 | What turning TLS on needs in this workspace. Answered by note 24 | Before the first real target; milestone 9 | 226, 303 |
| 35 | What a program on each operating system can reach from outside another, and what each route asks of the person | Milestone 10, package 8.3's Windows and macOS back ends; before a scripting route is designed | 106, 305, 306 |
| 36 | How stably a network can be recognized, and how forgeable each recognition signal is, per operating system. Answered from documents by note 32; a run on each system remains (H-44) | Before the network-awareness design is built (slice N3) | 218, network-awareness plan |
| 37 | The conduct and data-protection limits on keeping an inventory of a network one is on but does not own | Before network findings are persisted (decision 0226) | 217, 218 |

## Briefs

**1. Browser engine base.** (Answered by [note 17](17-browser-engine-base.md) and [decision 0070](../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md). The brief stays so that references to it resolve.) Compare chromiumoxide, Skyvern's Rustwright core, ferrous-browser, and writing a DevTools-protocol client from scratch. Judge on: event-driven waits, accessibility-tree access, network capture and in-page fetch, launching installed Chrome or Edge with an engine-owned data directory, stability over long sessions, license, maintenance, and Windows, macOS, and Linux support. Produce a recommendation and a spike plan.

**2. MCP features and dual-revision serving.** (Answered by [note 18](18-mcp-server-base.md) and [decision 0071](../decisions/0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md). The brief stays so that references to it resolve.) In the 2026-07-28 specification, confirm tool annotations, output schemas, resources and resource links, the Tasks extension lifecycle, Multi Round-Trip Requests, and list caching. Find out whether the `rmcp` crate can serve 2026-07-28 and 2025-11-25 clients from one server, and how.

**3. Locators and hard widgets.** Best practice for locating and acting on elements through the DevTools protocol and the accessibility tree without Playwright: actionability checks, shadow DOM, iframes, virtualized grids, custom dropdowns, date pickers, rich-text editors, file uploads, and single-page navigation.

**4. Session persistence and Entra session rules.** (Partly answered by [note 14](14-sessions-and-policy-engines.md).) How cookies, localStorage, IndexedDB, and sessionStorage persist in Chromium profiles; where MSAL-based apps keep tokens; what controls Entra session length (sign-in frequency, persistent browser session, refresh-token lifetime, continuous access evaluation); what Token Protection and device-compliance rules do to a session used from a server; what Microsoft recommends for automation identities; what to ask IT to approve.

**5. Vault design per operating system.** (Partly answered by [note 15](15-traces-and-key-stores.md); the design is in spec section 216.) Windows DPAPI and Credential Manager, macOS Keychain, Linux Secret Service; Rust crates for each; envelope encryption of profile directories; how Chromium encrypts cookies on each OS, including app-bound encryption on Windows, and whether a copied profile still works; options on servers and in containers; keeping secrets out of logs and traces.

**6. Web sign-in methods and auto-detection.** For OIDC and SAML redirects, pop-up sign-in, form login, HTTP Basic and Digest, NTLM and Kerberos, client certificates, and passkeys: what Chromium supports under automation, the pitfalls, and how to detect each method from a URL and detect expiry mid-run.

**7. Durable execution and idempotency.** (Partly answered by [note 13](13-ledger-leases-and-runner-links.md); check the design in spec section 208 against primary sources.) The minimal sound design for the run ledger, checkpointing, and resume when the side effect is in a system with no transactions. Draw on Temporal, Restate, DBOS, the saga pattern, and idempotency-key practice. Address the crash between sending a write and recording it. Recommend a ledger schema for SQLite and Postgres.

**8. Human approval patterns.** MCP elicitation and client support; approval pages; Teams, Slack, and email approvals; binding an approval to a content hash with expiry and single use; tiering and batching to avoid fatigue; what to record for audit; how LangGraph, the OpenAI Agents SDK, Temporal, GitHub environments, and Terraform handle it.

**9. Network channel practice and endpoint inference.** Capturing traffic through the DevTools protocol; tools that derive API descriptions from captures (mitmproxy2swagger, HAR-to-OpenAPI, Integuru); anti-forgery tokens, GraphQL, WebSockets, and server-rendered pages; schema validation of undocumented responses; the stability and terms risks of calling internal endpoints.

**10. AI discovery turned into deterministic steps.** How Stagehand, workflow-use, Skyvern, AgentQL, and Pomerado convert a model's decision into a stored step and re-validate it later; their reported limits; how to keep exploration strictly read-only on a live system.

**11. File-format prior art.** Lessons from Chrome DevTools Recorder JSON, browser-to-mcp's YAML, Robot Framework, Maestro, the OpenAPI Arazzo workflow specification, and GitHub Actions: versioning, expressions, loops, reuse, linting, and the trap of logic creeping into YAML.

**12. Testing against third-party apps.** Safe testing of write workflows without a test environment; recorded fixtures as a mock target; contract tests for locators and response shapes; flakiness thresholds; fault injection; canary frequency and alerting.

**13. Local and remote MCP transport security.** Risks of localhost HTTP servers (DNS rebinding, any local page connecting) and the recommended mitigations; what the MCP authorization specification requires end to end; sharing one server among many clients.

**14. Prompt injection and MCP-specific threats.** Indirect injection through application text returned in tool results; tool poisoning, shadowing, and confused-deputy problems; guidance from the MCP security pages and OWASP; which mitigations fit a deterministic server.

**15. Enterprise governance of agent tools.** (The decision engine is covered by [note 14](14-sessions-and-policy-engines.md).) Per-user and per-group tool visibility with Entra groups and app roles; admin consent; allowlists; audit suitable for compliance; data-loss-prevention hooks; how Microsoft, Anthropic, and OpenAI recommend governing MCP tools.

**16. Hosted-agent compatibility, hands-on.** Connect a test server to Copilot Studio and record: MCP revision spoken, elicitation, Tasks, resources, limits on tools, payload, and duration, and whether calls carry the end user's identity. Repeat for other hosted agent platforms in use.

**17. Remote sign-in to a server-hosted browser.** How Browserbase Live View, Steel, Browserless, and noVNC-style viewers stream a server-side browser to a remote user securely; whether passkeys and platform authenticators can work remotely; push MFA; session storage per user; the risks of a server holding many users' sessions.

**18. Deployment and scaling patterns.** (Job queues and worker links are partly answered by [note 13](13-ledger-leases-and-runner-links.md).) Control plane and worker splits; Postgres-based job queues; browser workers in containers; Windows desktop workers; Kubernetes versus Compose versus a plain service; upgrades without downtime.

**19. Desktop accessibility automation.** (Answered by [note 19](19-desktop-accessibility-base.md) and [decision 0120](../decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md). The brief stays so that references to it resolve.) The state of UI Automation, the macOS Accessibility API, and AT-SPI from Rust; `xa11y` maturity; Java Access Bridge and SAP GUI scripting; whether desktop automation can run in a service session or needs an interactive one; how stable element identifiers are.

**20. Selector repair and drift.** Healenium and similar tools; LLM-based locator repair; fallback locators and element fingerprints; the failure mode where healing picks the wrong element; how mature systems keep repair safe.

**21. Lessons from RPA products.** UiPath's object repository, selectors, queues, and exception-handling framework; Power Automate Desktop; the documented causes of bot failure and maintenance cost; how RPA vendors now combine agents with deterministic bots.

**22. Reproducibility and observability.** (Trace conventions are partly answered by [note 15](15-traces-and-key-stores.md).) Pinning a Chromium build without Playwright's installer; environment factors that cause drift; trace and screenshot storage and redaction; OpenTelemetry conventions for MCP tool calls; tamper-evident audit logs.

**23. Vendor APIs and terms of use.** For the first real target, chosen privately by the owner: what its published API covers and omits; rate limits; whether it offers its own MCP server; what its terms say about automated access. In general: what typical SaaS terms say about automating a licensed user's own account. Not legal advice. Keep the application's name out of the repository.

**24. WebMCP standard status.** (Answered by [note 25](25-webmcp-standard-status.md). The brief stays so that references to it resolve.) The current state of the W3C community-group proposal and Chrome's implementation; whether tools can be discovered and called by an external agent through the DevTools protocol; adoption.

**25. Gap-review components.** For each component in [spec section 604](../spec/6-running-it/604-management-and-operations.md), find prior art and pitfalls before designing it in detail: environments, triggers, mid-run input, undo, read caching, data classification, identity isolation, single-session apps, tool versioning, authorization records, usage metering, updates, and target packs.

**26. Local channel on three operating systems.** Rust crates for Unix domain sockets and Windows named pipes behind one interface; serving HTTP over them with the chosen web framework; reading the connecting user's identity on each system; directory and access-list permissions; starting a detached background process and guarding it with a lock file; how comparable tools (language servers, container and database daemons) handle version skew and idle shutdown.

**27. Operating-system user-presence checks.** (Answered by [note 20](20-operating-system-user-presence-checks.md). The brief stays so that references to it resolve.) What Windows Hello, macOS Touch ID and password prompts, and Linux polkit offer to a background process running as the user; whether each can be called from Rust; behavior on machines with no biometric hardware, over remote desktop, and on headless servers; what a program running as the same user can and cannot bypass.

**28. Wheel and npm packaging, verified hands-on.** (Answered by [note 21](21-wheel-and-npm-packaging.md) and [decision 0135](../decisions/0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md). The brief stays so that references to it resolve.) Build a maturin `bin` wheel that also carries a pure-Python package and confirm the command and the import both work; the platform tags to publish; a launcher plus platform packages on npm, tested with optional dependencies switched off and with install scripts disabled; what `dist` covers; current registry rules for short-lived publishing credentials and build provenance.

**29. Command-line design for agents.** How current coding agents discover and use commands; output formats that cost the least context; evidence on `--json` conventions, exit codes, and paging; how guidance files and skills are loaded by the main agents today; measured comparisons of commands and MCP tools for the same task.

**30. Sign-in in engine-owned profiles, and device-bound sessions.** On Windows, macOS, and Linux, with Chrome and Edge: does a new data directory sign in silently through the device's account, and what must be configured for it to; how Device Bound Session Credentials behave in an automated browser, on virtual machines, and in containers; how to tell from outside that a session is device-bound; what exporting and restoring cookies does to such a session.

**31. Cedar spike.** (Answered by [decision 0082](../decisions/0082-the-decision-core-is-the-policy-crates-own-evaluator.md), which ran the spike and chose the policy crate's own evaluator. The brief stays so that references to it resolve.) Add the `cedar-policy` crate to a test binary and measure the size it adds and the time per decision for a realistic policy set; confirm its license; model Anything2MCP's principals, tools, and questions in a Cedar schema; test strict validation and the fail-closed wrapper; assess partial evaluation for listing what a caller may use.

**32. First standard wire protocol of the stream pack.** (Answered by [note 22](22-stream-wire-protocol.md). The brief stays so that references to it resolve.) Added after [decision 0104](../decisions/0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md) left the choice open. Compare NATS with JetStream, MQTT 5, AMQP 1.0, and Kafka against the nine requirements of that record. Check each client crate on the registry and against the closed licence list, and weigh a client of our own against a crate.

**33. Client crates for a server database beside the embedded one.** (Answered by [note 23](23-server-database-clients.md). The brief stays so that references to it resolve.) Added for the ledger's Postgres back end and the data pack's server store. Compare the client crates for Postgres, MySQL, and SQL Server on maintenance, licence tree, C code, encrypted links, and what they leave to the caller: deadlines, the fate of a failed call, read-only transactions, exact numbers, pools.

**34. What turning TLS on needs in this workspace.** (Answered by [note 24](24-turning-tls-on.md). The brief stays so that references to it resolve.) Added after the overnight build's journal named "TLS is off". What the HTTP client, the server listener, the database and stream links, and the test stands need; the cost of each stack; trust roots on three systems and in minimal containers.

**35. What a program on each operating system can reach from outside another.** (Added by the [coverage review](../reviews/coverage-review-2026-11-18.md) of 2026-11-18.) On Windows, macOS, and Linux, from primary documentation and a run on each where possible: (a) the owner, group, and access list of the folders where ordinary installations put programs (on macOS the folder of installed applications, believed writable by the administrators' group; on Windows the root of the system drive and the per-user program folders; on Linux the local program folders of the common distributions), and so which programs the command pack's refusal of a folder a group can write would refuse; (b) whether a script passed by file to the system's own script hosts (PowerShell with `-File`, `osascript` with a script file) receives its arguments as literal strings with nothing evaluated, element for element, as hypothesis H-32 asks of programs; (c) the scripting interfaces: Apple events and scripting dictionaries, COM automation and type libraries, D-Bus introspection; what each describes about whether a call only reads; which Rust crates reach each, with licences on the closed list; what permission the person grants for each (Automation on macOS, nothing on Windows at the same integrity level, the session bus on Linux); (d) whether the common browser-runtime application frameworks accept a data directory and a debugging pipe given at start, so that the web pack can start one with a data directory the engine owns; (e) how each system's own elevation prompt can be asked to start one declared program by path, and what a program running as the user can do against it.

**36. How stably a network can be recognized, and how forgeable each signal is.** (Answered from documents by [note 32](32-how-stably-a-network-is-recognized.md) and [decision 0318](../decisions/0318-a-network-is-recognized-by-its-gateway-and-subnet-and-a-forged-one-can-recall-a-reach.md). What remains: each signal read as an ordinary user, and its stability observed across a real reconnection, a renewed lease, and a roam, on each system; hypothesis H-44. The brief stays so that references to it resolve.) (Added by the first network-awareness design wave of 2026-11-18, for [decision 0223](../decisions/0223-a-network-is-a-place-a-recognized-network-and-a-reach.md) and hypothesis H-44, which research notes 28 and 29 do not cover.) On Windows, macOS, and Linux, from primary documentation and a run on each where possible: which facts the machine already holds at rung 0 can fingerprint a network (default gateway hardware address, DHCP server identifier, DNS search domain, subnet, and on wireless the SSID and BSSID); how stable each is across a reconnection, a renewed lease, and roaming between access points of one network; how forgeable each is by someone on the link, and at what cost; and which stable subset makes a fingerprint that recalls a saved reach reliably without misfiring. The design rests on recognition only recalling a person's prior act, never authorizing a send; this brief settles whether that recall is stable enough to be worth building and confirms the spoofability is only of a reach the person already trusted.

**37. The conduct and data-protection limits on keeping an inventory of a network one is on but does not own.** (Added by the first network-awareness design wave of 2026-11-18, for [decision 0226](../decisions/0226-whether-and-how-long-findings-about-a-network-are-kept.md), which research note 29's conduct norms may touch but not settle.) What norms and rules bear on keeping a list of the devices and services seen on a network a person is on but does not own (a café, a client's office, a shared building): whether such a list is personal data about third parties under the common data-protection regimes, what keeping it for the life of a run versus persisting it would ask of the person, and whether a network the person has named as theirs is treated differently. Feeds the retention rule for network findings and the recommendation that findings about an unnamed network are not kept past the run.
