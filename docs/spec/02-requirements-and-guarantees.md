# 2. Requirements and guarantees

What the system must do:

| Area | Requirement |
| --- | --- |
| Reach | Wrap web apps, backend APIs, apps with their own MCP servers, and desktop apps the user's device can reach |
| Modes | UI tree, in-page HTTP, direct HTTP, API description, site tools, MCP pass-through, desktop accessibility; chosen per call by the runtime |
| Reads and writes | Both first-class, as single operations and as end-to-end pipelines |
| Ways in | MCP server for agents; REST API, Python SDK, and TypeScript SDK for other software; CLI; embeddable library |
| Building | Any AI agent builds tools through the system's own commands and target files, or through matching MCP authoring tools; no single model provider required |
| Sign-in | Discover and reuse existing sign-ins (Entra ID, MFA, SSO, cookies, tokens) or use custom credentials; keep sessions alive between calls |
| Concurrency | Many agents and applications at once against one running system |
| Scale | Large applications with many modules, servers, and origins |
| Edge cases | Many frameworks, platforms, and app behaviors, with a named stop for anything unknown |
| Platforms | Windows, macOS, Linux; laptop, shared server, or container |
| Install | `pip install`, `npm install`, a single binary, or cloned into an existing repo for an agent to adapt |

Non-goals:

- Defeating MFA, CAPTCHAs, bot defenses, licensing, or access controls. The system acts with exactly the user's rights, openly.
- Live AI improvisation during a run.
- Pixel-only targets such as Citrix or remote desktops, until a later vision mode is justified.

The four guarantees, each with how it is met and proven:

| Guarantee | Meaning | How it is met | How it is proven |
| --- | --- | --- | --- |
| Repeatable | Running the same call twice leaves the app in the same state | Record keys, reconcile-before-write, resume from the ledger | Every write pipeline runs twice in testing; the second run changes nothing |
| Reproducible | Another machine, or the same one months later, behaves identically | Pinned runtime and drivers; all app knowledge in versioned files; fixed locale, time zone, and viewport | A fresh install passes the full suite against recorded fixtures |
| Deterministic | No model and no chance decides anything during a run | Fixed steps, named waits, exact normalized comparison, mode chosen by declared rules | Repeated runs on the same input produce the same trace |
| Safe | A run cannot do more than was declared and approved | Effect classes, permission manifest, validation first, approval bound to a diff, read-back, limits, vault | Fault-injection tests for blocked targets, bad input, expired sessions, stale approvals, and edge blocks all stop cleanly |

One limit applies to all four: the wrapped application is outside the system's control. When it changes, the guarantee is a clean stop with a named cause.
