# Research: products that turn applications into APIs or agent tools

**Status:** completed by a research agent on 2026-11-15, plus search results gathered in the design sessions. Only browser-to-mcp, Pomerado, and the pip `web2mcp` package were inspected at code level (see [10-code-reviews.md](10-code-reviews.md)); everything else is from product pages, docs, and listings. Feeds [spec section 35](../spec/35-prior-art.md).

## The four underlying mechanisms

| Mechanism | How it works | Examples |
| --- | --- | --- |
| Live UI control | An agent looks at the page and acts, one tool call per action | Playwright MCP, Chrome extension bridges |
| Recorded UI replay | Actions are recorded or authored once and replayed as fixed steps | browser-to-mcp, webpage-mcp, Pomerado |
| Traffic-derived API | Watch the requests the UI sends, then send them from code | Integuru, the pip `web2mcp` (crudely) |
| Site-provided tools | The site publishes its own tools in the browser | WebMCP proposal and polyfills |

Web2MCP combines the second and third, uses the fourth where offered, and uses the first only as evidence at build time.

## Findings from the landscape research

1. **Most vendors use AI at build time and replay code at run time, but fall back to an agent when replay fails.** Skyvern has `run_with="code"` with AI fallback on by default ([Skyvern](https://www.skyvern.com/docs/developers/features/code-caching)). Stagehand replays from cache with no model call, but clearing a stale cache is manual ([Stagehand](https://docs.stagehand.dev/v3/best-practices/caching)). HyperAgent stores XPath plus the instruction, falls back to the model, and can export a TypeScript script ([Hyperbrowser](https://hyperbrowser.ai/docs/hyperagent/action-cache)). Browserbase turns a run or a prompt into a Stagehand script ([Browserbase](https://docs.browserbase.com/platform/agents/generating-scripts)).
2. **Pomerado (MIT, launched October 2026) is the closest analog.** It builds one MCP server per integration as source code you own, and calls no model at run time. Each integration is marked read or write. Failed calls return `write_status` and `possible_commit`. [Pomerado](https://github.com/Pomerado/pomerado)
3. **CloudCruise** uses a JSON workflow format. A builder agent writes workflows and a maintenance agent patches them. It has a credential vault, 2FA inboxes, and a per-credential concurrency queue. It does not describe how writes are verified. [CloudCruise](https://cloudcruise.com/blog/cloudcruise-vs-skyvern-healthcare-automation)
4. **Integuru works at the HTTP level.** It takes a HAR file plus cookies, builds a request dependency graph, and outputs Python. It makes no read and write distinction. [Integuru](https://github.com/Integuru-AI/Integuru)
5. **Extraction platforms are read-first.** Kadoa ships versioned, reviewed pipelines with validation and a repair loop that escalates to humans ([Kadoa](https://www.kadoa.com/how-kadoa-works)). Browse AI stores several locators per element ([Browse AI](https://help.browse.ai/en/articles/12271136)). Apify's MCP server runs actors asynchronously and returns a dataset id ([Apify](https://docs.apify.com/platform/integrations/mcp)).
6. **Keeping credentials away from the model is now expected.** In the Browserbase and 1Password integration, models never see credentials and a human approves each use ([Browserbase](https://www.browserbase.com/changelog/1password-for-agents)). Steel injects credentials and generates one-time codes, but only in its cloud product ([Steel](https://llms.steel.dev/articles/secure-browser-auth-for-agents)).
7. **Firecrawl profiles allow one writer at a time;** a second session trying to save gets a 409 ([Firecrawl](https://docs.firecrawl.dev/features/interact)).
8. **Consolidation.** Sierra acquired Anon on 2026-11-05 ([Sierra](https://sierra.ai/blog/sierra-acquires-anon)).
9. **Browser Use's workflow-use** records a task into a JSON workflow; self-healing and MCP support were still on its roadmap ([workflow-use](https://github.com/browser-use/workflow-use)).

## Closeness ranking

Ranked by how much of the Web2MCP design each covers. Positions below the first two rest on descriptions, not code.

1. Pomerado
2. browser-to-mcp
3. CloudCruise
4. webpage-mcp (mcpland)
5. Web2MCP, the TypeScript project of the same name
6. holon-run/webmcp-bridge
7. grain-mcp-server
8. Skyvern
9. Stagehand and Browserbase Director
10. HyperAgent
11. Browser Use workflow-use
12. zen-mcp/browser
13. Integuru
14. Xzeroone/web2mcp (pip)
15. Kadoa
16. Browse AI
17. Apify MCP
18. mcp-browser-use
19. Microsoft Playwright MCP
20. executeautomation Playwright MCP and its fork
21. auto-browser
22. ByteDance Browser MCP
23. Arthur
24. Provar, local-browser-mcp, simple-playwright-mcp, Automata Labs
25. Extension bridges: BrowserMCP, Browsentic, agent-browser-bridge, Browser Bridge, T3rnel, byob
26. WebMCP family: the W3C proposal, MCP-B, teatak/mcp-server-browser, Cloudflare's preview, cf-webmcp, WebMCP Bridge

## The gap Web2MCP fills

None of these combines: self-hosted, no model at run time, every mode in one engine, verified writes with out-of-band approval, resumable runs, per-user and service identity under enterprise single sign-on, governance, and a company-server shape.

## Uncertain

- Self-healing claims from Skyvern, Twin, and Simular are not documented; CloudCruise's 99% figure is the vendor's own.
- Induced being offline and MultiOn pivoting come only from secondary directories.
- The TypeScript Web2MCP project's repository was not found; its description comes from listings, one of which confuses it with the W3C WebMCP proposal.
