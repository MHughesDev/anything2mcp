# Web2MCP

Turn any existing app into safe, deterministic MCP tools. Rust core.

Web2MCP latches onto software you can already reach (a web app, a backend API, a desktop program, or an app with its own MCP server) and exposes it to AI agents as tools. It works through whatever access already exists, under a user's own sign-in or its own account, and runs on one laptop or as a company server.

## Status

**Design stage. No code yet.** This repository currently holds the system design, the decisions behind it, and the research it rests on. Implementation starts at Phase 0 of the build plan.

## Where to start

| You want to | Read |
| --- | --- |
| Understand what this is | [docs/spec/01-summary.md](docs/spec/01-summary.md) |
| See the whole design | [docs/spec/README.md](docs/spec/README.md) |
| See what was decided and why | [docs/decisions/README.md](docs/decisions/README.md) |
| Check what is researched and what is a guess | [docs/research/README.md](docs/research/README.md) |
| See whether the design is ready to build | [docs/plan/readiness.md](docs/plan/readiness.md) |
| Start building | [AGENTS.md](AGENTS.md), then [docs/plan/phase-0.md](docs/plan/phase-0.md) |

## What it will do

- **Every mode in one engine:** interface elements, HTTP from inside the page, direct HTTP, API descriptions, site-published tools, an app's own MCP server, and desktop accessibility. The runtime picks the fastest mode that has been proven correct.
- **Deterministic runs:** AI drafts and repairs tool definitions at build time; nothing improvises during a run.
- **Verified writes:** previewed, approved by a person, read back from the app, and resumable without duplicates.
- **Real identity:** each user acts with their own account, or Web2MCP acts with its own. Existing sign-ins are reused.
- **Governed:** administrators choose which tools reach which users and agents.
- **Many ways in:** MCP for agents; a REST API, SDKs, and a CLI for everything else.

## Use it responsibly

Web2MCP is designed to operate openly: it identifies itself as automation, respects rate limits, and never bypasses sign-in protections or bot defenses. Wrapping an application you do not own may be restricted by that application's terms or by your organization's policy. Get agreement from the application's owner and your IT team before using it on real work.

## License

Apache-2.0 is the chosen license; the license file is added in Phase 0.
