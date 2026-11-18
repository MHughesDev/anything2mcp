# 701. Prior art

This section surveys what other products and projects do, and what this design takes from them or refuses. It describes no code of ours.

Most existing products use AI to build automations and replay code to run them, but fall back to a live agent when replay fails. Anything2MCP keeps the first half and refuses the second, and adds what none of them combine: every mode in one engine, real identity handling, governance, and a server shape.

| Project | What it does | Borrow | Where Anything2MCP goes further |
| --- | --- | --- | --- |
| [Pomerado](https://github.com/Pomerado/pomerado) | Builds deterministic MCP integrations for websites, driving Chromium; its core defines an HTTP-only operation form | The HTTP operation idea; reviewing every build; reading the site back before another write | Its local version needs OpenAI models to build, signs in again on each call, and documents macOS and Linux only |
| [browser-to-mcp](https://github.com/browser-to-mcp/browser-to-mcp) | Turns reviewed YAML workflows into read-only MCP tools | Declared permissions; evidence per version | Writes, sign-in, sessions |
| Integuru | Turns recorded traffic into request code | Endpoint mining from traffic | Verification, identity, MCP, a UI fallback |
| [Skyvern](https://www.skyvern.com/docs/developers/features/code-caching), [Stagehand](https://docs.stagehand.dev/v3/best-practices/caching) | Replay cached code or actions; an agent takes over on failure | Caching AI decisions as steps | No agent at run time |
| [Rustwright](https://codestuff.io/repo-of-the-day/916) | Playwright's API on a Rust DevTools-protocol engine | A possible base for the browser engine | A verb-level engine with read-back |
| [xa11y](https://docs.rs/crate/xa11y/0.3.0) | One Rust API over the three desktop accessibility systems | A possible base for the desktop driver | Operations, policy, and audit on top |
| [Playwright MCP](https://github.com/microsoft/playwright-mcp) and extension bridges | An agent drives the browser live, one action per tool | Accessibility snapshots for authoring | Deterministic tools, not live control |
| WebMCP proposal | Sites register their own tools in the browser | Used as the `site_tools` mode | Works on apps whose owners have added nothing |
| CloudCruise, Kadoa, Apify | Hosted workflow, extraction, and scraping platforms | Vaults, repair loops with human review, asynchronous runs | Self-hosted, write-capable, per-user identity |

Other projects use the name Anything2MCP, including a Python project that generates MCP servers from HTTP endpoints without a browser and documents `pip install anything2mcp`, although nothing was published under that name when checked. The name is kept by decision; the published package names are fixed in [section 605](../6-running-it/605-packaging-and-release.md).
