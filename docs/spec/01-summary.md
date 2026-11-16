# 1. Summary

**North Star: latch onto any existing application and expose it to the user's AI as reliable MCP tools.**

**Web2MCP** is an all-in-one, open-source system, built from an empty repository, that wraps software an organization or person can already reach and exposes it as MCP tools. The target can be a web app in Edge, a backend API, a desktop program, or an app that already has its own MCP server. Web2MCP reads and writes through whatever access already exists, under a user's own sign-in or its own dedicated account.

The core runtime and engine are written in **Rust**, shipped as one binary. Other languages are used where they fit better: TypeScript for the web pages, Python and TypeScript for SDKs, shell scripts for installers, and any language for plug-ins.

The four qualities every design decision is weighed against, in this order when they conflict: **safe, correct, fast, easy**. Speed is a first-class metric with budgets and regression tests, not an afterthought.

What makes it different from what exists today:

1. **Every mode in one engine.** One operation can run through the UI element tree, HTTP from inside the page, direct HTTP, an API description, tools a site publishes, an app's own MCP server, or a desktop app's accessibility tree. The runtime picks the fastest verified mode per call.
2. **Runs anywhere, in any shape.** One binary scales from a single laptop to a company server on a dedicated VM to a cluster, on Windows, macOS, and Linux.
3. **Many ways in.** AI agents connect over MCP, including hosted agents such as Copilot Studio. Other software uses a REST API, Python and TypeScript SDKs, or embeds the Rust library.
4. **Built by the agent you already have.** Any coding agent builds tools with Web2MCP's commands and target files, or through matching MCP tools. No model provider is required.
5. **Identity done properly.** Each user acts with their own account and permissions, or Web2MCP acts with its own account. Existing Entra ID, MFA, and SSO sign-ins are discovered, reused, and kept alive, so nobody approves a sign-in per call. A person's session can be used only by that person and agents acting for them.
6. **Deterministic and safe at run time.** No AI improvises during a run. Every write is previewed, approved, verified by reading it back, and resumable.
7. **Governed.** Administrators decide which tools are exposed to which users and agents, with roles, approvals, limits, and a tamper-evident audit log.
