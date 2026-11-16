# Research: code-level reviews of three related projects

**Status:** each repository was cloned and inspected on 2026-11-15. These are quick reviews of structure, dependencies, and documentation, not audits.

## browser-to-mcp

Repository: https://github.com/browser-to-mcp/browser-to-mcp

**What it is.** Python. Turns reviewed browser workflows, written in YAML or drafted from a recording, into typed, read-only MCP tools, replayed through Playwright's Chromium.

**Facts observed.**

- Apache-2.0. 190 commits between 2026-07-23 and 2026-11-02. About 10,000 lines of Python in `src/` and about 24,000 including tests.
- Modules include a workflow loader and validator, a compiler, a browser policy (in both Python and JavaScript), replay, a recorder, authoring, draft evaluation, a job store, a tool library, structured outputs, tracing, paging, and a WebMCP discovery command.
- Each workflow file declares its risk, a permissions block (allowed domains; downloads, uploads, form submission, messaging, and spending all off), typed inputs, and steps. Each step carries its own risk, idempotency, retry, and confirmation settings.
- Its README lists what it does not provide: hosted browsers or remote MCP transport; login, persistent sessions, or credential storage; writes, destructive actions, payments, uploads, or messaging; handling of browser challenges; broad coverage of arbitrary sites.
- Draft workflows are evaluated against independent expected answers before approval, and evidence (plans, reports, traces, screenshots) is stored per version.

**What Web2MCP borrows.** Declared permissions per tool; per-step risk and retry declarations; evaluating drafts against expected answers; keeping evidence with each version.

**Where it stops.** Read-only by design, no sign-in, local only.

## Pomerado

Repository: https://github.com/Pomerado/pomerado

**What it is.** TypeScript. Builds deterministic MCP integrations for websites from a natural-language description, driving Chromium through Playwright.

**Facts observed.**

- MIT. Version 0.4.0. Requires Node 24.21 or later within Node 24. About 227,000 lines of TypeScript including tests. Last commit dated 2026-11-15.
- Its README states: integrations are built from a description, with no HAR file or recording needed; integrations are deterministic; it works on sites behind a login and two-factor authentication. Prerequisites list macOS or Linux and an OpenAI API key, because it uses OpenAI models to build and review integrations.
- It runs as a stdio MCP server whose build tools are `mint`, `get_job`, `provide_input`, and `cancel_job`.
- Its source defines an "HTTP version" of an operation: one whose only network capability is the site's HTTP. Declared capabilities include `chromium-network`, `session-cookies`, `page-environment`, and others. Transport names in the source are `kernel-curl`, `page-fetch`, and `saved-http`.
- Its documentation states the local host does not mint HTTP versions, record network traffic, or replay captures, and that the local host keeps no login between calls.
- A separate reviewer ("Guardian") reviews each build. Read integrations must pass at least 70% of their live test cases before review. A write designs no tests: its single "act" session is the run that proves the tool. After a failed step labelled a write, the builder must read the site back before any further write.
- Element targeting in the source is mostly by role, label, placeholder, test id, and text; one raw mouse click was found.

**What Web2MCP borrows.** An operation form whose only capability is HTTP, with several transports; a reviewer separate from the builder; read-back before any further write after a failure; `possible_commit` reporting.

**Where Web2MCP goes further.** Provider-neutral building; sessions kept between calls; writes proven on test records with preview and approval, never by performing them once for real; Windows support; a server shape with identity modes and governance.

## Xzeroone/web2mcp (the pip package of the same name)

Repository: https://github.com/Xzeroone/web2mcp

**What it is.** Python. Analyzes a site over plain HTTP and generates a standalone MCP server that calls its endpoints.

**Facts observed.**

- Version 3.0.0 in `pyproject.toml`. Dependencies: `mcp`, `httpx`, `beautifulsoup4`, `lxml`, `jinja2`, `pyyaml`, `cryptography`, `pydantic`. No Playwright, Selenium, or Chromium anywhere in the code.
- About 1,900 lines of Python. Commits dated 2026-04-04 and 2026-04-05.
- Tools include discover, request (with stored auth injected), an auth store, an OAuth helper, API-spec parsing, schema inference, sitemap parsing, form extraction, and server generation.

**Relevance.** No browser, so it cannot sign in through single sign-on, drive an interface, or verify a save. It overlaps only with Web2MCP's `api` mode. Its README says `pip install web2mcp`, but a check on 2026-11-15 found nothing published under that name ([note 12](12-packaging-and-agent-interfaces.md)). An earlier version of this note said it occupied the `web2mcp` package name on pip, which matters for distribution; see [ADR 0001](../decisions/0001-name-web2mcp.md).

## A second project named Web2MCP (TypeScript)

Not inspected; its repository was not found. Listings describe it as observing the JSON API calls a web app makes while a person browses, generating tools from them, and injecting those learned requests into the page so an in-browser agent can call them. One listing confuses it with the W3C WebMCP proposal. [stork.ai](https://www.stork.ai/en/web2mcp), [The Daily Workflow](https://thedailyworkflow.com/ru/mcp/server/web2mcp)

**Relevance.** Its capture-and-infer idea matches Web2MCP's endpoint mining. Its run time stays inside the browser.
