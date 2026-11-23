# 702. Requirements audit

Every stated requirement is covered by the design; five are covered only under conditions or limits outside Anything2MCP's control, and two rest on targets not yet measured. It checks the design against the requirements; how much is built is in [section 704](704-readiness.md).

| Requirement | Verdict | How | Condition or limit |
| --- | --- | --- | --- |
| Run on a dedicated machine or VM for a company | Covered | Team, Company, and Cluster profiles | None |
| Act per user, with each user's existing account | Covered, with conditions | Three per-user identity modes | Token mode needs the app's API registered in the tenant; hosted sessions fail where sign-in requires a managed device; device mode needs a runner on the user's computer |
| Act with its own account in the target software | Covered | `service_account` mode | The app must license the account; IT must approve it |
| Latch on through any interface, front end or back end | Covered, within limits | Seven modes | Not reachable: pixel-only apps such as Citrix; direct HTTP where the front end signs requests; apps that talk only over WebSockets, in the first release |
| Be exposed on the company network as a company MCP server | Covered | Streamable HTTP, OAuth through the identity provider, toolset endpoints | None |
| Connect from Copilot Studio and similar hosted agents | Covered, with conditions | Stateless Streamable HTTP, OAuth or API key, three network bridges | Must be reachable from Microsoft's cloud; its support for elicitation, the Tasks extension, and the newest MCP revision is untested |
| Expose created tasks and workflows to a chosen set of users | Covered | Toolsets, grants, roles, workspaces | None |
| Run entirely on one user's machine | Covered | Solo and Workstation profiles | None |
| Different configurations for different uses | Covered | Seven profiles, layered settings, per-target identity and mode choices | None |
| Many agents at once | Covered | Scheduler, shared sessions, record locks, quotas | Desktop apps run one job at a time |
| Usable by software as well as agents | Covered | REST API, SDKs, CLI, embeddable library, OpenAPI export | None |
| Tools built by any AI agent, any provider | Covered | Authoring tools over MCP and CLI | None |
| Sign in once; no approval per call | Covered, with conditions | Session keeper with silent renewal | The identity provider's policy still decides how often MFA is re-prompted |
| Any operating system | Covered, with limits | Native builds for Windows, macOS, Linux | Desktop driving and broker sign-in need an interactive user session |
| Large applications with many modules and services | Covered | Modules, multiple surfaces, on-demand loading, tool search | None |
| Use the user's own account, usable only by that user and their agents | Covered | Session ownership rule and built-in guardrail (sections 215, 212) | On a server, an agent with its own identity needs the person's explicit delegation |
| A pip package as the interface for a coding agent | Covered | pip ships the binary and an SDK; the agent builds through commands and files (sections 220, 605) | The pip name must be reserved before another project publishes it |
| Rust core, other languages where applicable | Covered | Crate layout; language table in section 605 | The browser engine is new work in Rust |
| Speed as a core metric | Covered by design | Budgets, compiled plans, fastest-verified-mode, CI gates | Budgets are targets until measured in phase 1 |
| Easy to use | Covered by design | Seven commands, safe defaults, `doctor`, agent-ready repository | The ten-minute target is unproven until tested with real users |
| Repeatable, reproducible, deterministic, safe | Covered | Ledger, pinned builds, no AI at run time, policy gate | The target app can still change; the guarantee is a clean stop |

**Gaps this audit found in the earlier draft, now closed.**

- The earlier design assumed one user. It now has four identity modes and user-device runners.
- Nothing scoped tools to people. Toolsets, grants, roles, and workspaces now do.
- The engine and server were one process. They are now a control plane and runners that can be split or combined.
- There was no network design. Inbound paths, runner links, egress, and the Copilot Studio bridges are now specified.
- Approvals assumed the person was at the same machine. They now work for remote users through approval pages, URL-mode elicitation, and chat.
- The runtime was planned in Python. It is now Rust, with speed budgets.

**The risks that remain, largest first.**

1. **Scope.** This is a large system. The Build Order orders it so each phase is useful alone.
2. **The Rust browser engine.** Rebuilding reliable waiting and acting is the hardest engineering in the project.
3. **Per-user access on a server** depends on each organization's Conditional Access rules.
4. **Permission.** Wrapping third-party software can conflict with vendor terms and IT policy. Anything2MCP is built to operate openly and with approval.
5. **Hosted-agent compatibility**, starting with Copilot Studio, needs hands-on testing.
6. **Desktop mode** depends on how well each desktop app exposes accessibility data.
