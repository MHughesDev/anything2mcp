# Research: Rust building blocks

**Status:** partial. The MCP SDK, browser engine options, and desktop accessibility library were looked up on 2026-11-15. Everything under "Candidates from general knowledge" is unverified and must be confirmed in Phase 0 before it is added to `Cargo.toml`. Feeds [spec section 29](../spec/29-repository-crates-and-packaging.md) and [section 17](../spec/17-browser-engine-and-runtime.md).

## Verified by search

### MCP

- `rmcp` is the official Rust SDK for the Model Context Protocol; its 3.x README targets the 2026-07-28 specification. Features include stdio and Streamable HTTP server transports, OAuth support, and elicitation. [docs.rs rmcp](https://docs.rs/crate/rmcp)

### Browser engine

There is no official Playwright client for Rust. The options found:

| Option | What it is | Notes |
| --- | --- | --- |
| chromiumoxide | Async Rust client for the Chrome DevTools Protocol, with a Puppeteer-like API | Requires an installed Chrome or Chromium. One comparison says it has no built-in wait-for-selector. [Scrapfly](https://scrapfly.io/blog/posts/best-rust-web-scraping-libraries), [ferrous-browser README](https://github.com/theoxfaber/ferrous-browser) |
| Rustwright (Skyvern) | Playwright's API on an in-process Rust DevTools-protocol engine, with Python and Node bindings | MIT. Alpha. Chromium only; Firefox and WebKit not planned. Includes a Rust MCP server exposing browser tools. Vendor reports about 70% less memory and 2.55 times faster boot than the equivalent Playwright process. [Skyvern](https://www.skyvern.com/blog/rustwright-playwright-rewritten-in-rust-that-occupies-70-less-memory-and-is-2-55x-faster/), [summary](https://codestuff.io/repo-of-the-day/916) |
| ferrous-browser | Async Rust DevTools-protocol automation | Its own benchmarks report a median of about 1 ms to notice an element through an in-page observer, against about 100 ms for Playwright's polling. Vendor benchmark. [ferrous-browser](https://github.com/theoxfaber/ferrous-browser) |
| headless_chrome | Simpler Rust client | Described as less maintained. [DEV](https://dev.to/vhub_systems_ed5641f65d59/puppeteer-in-rust-chromiumoxide-and-headlesschrome-vs-the-python-alternative-4ji0) |
| playwright-rust wrappers | Rust bindings that bundle and drive Playwright's Node driver | Full Playwright features, but a Node process sits in the middle. [Project Awesome](https://project-awesome.org/r/padamson-playwright-rust) |

**Decision so far.** Web2MCP owns a small, verb-level engine on the DevTools protocol. Phase 0 runs a spike to choose between building on chromiumoxide, building on Rustwright's core, or writing a protocol client. See [ADR 0004](../decisions/0004-rust-browser-engine-on-the-devtools-protocol.md).

**Spike criteria.** Event-driven waits; accessibility-tree access; network event capture and in-page fetch; connect to a browser the engine launched with its own data directory; stability under 30 minutes of continuous use; license; maintenance activity; Windows, macOS, and Linux support.

### Desktop accessibility

- `xa11y` is a Rust library, with Python and JavaScript bindings, that offers one Playwright-style API over the three desktop accessibility systems: AXUIElement on macOS, AT-SPI2 on Linux, and UI Automation on Windows. MIT. [docs.rs xa11y](https://docs.rs/crate/xa11y/0.3.0), [author's post](https://crowecawcaw.github.io/general/2026/05/30/accessibility-for-computer-use.html)
- Per-platform crates also exist: `uiautomation` for Windows UI Automation, `atspi` for AT-SPI2 on Linux, and `accessibility-sys` for macOS. [lib.rs accessibility](https://lib.rs/accessibility)
- AccessKit is for applications that expose an accessibility tree, not for driving other applications. It is not a fit for the desktop driver. [AccessKit](https://github.com/AccessKit/accesskit)

## Candidates from general knowledge (unverified)

Confirm each crate's current name, maintenance, license, and platform support before use.

| Need | Candidate |
| --- | --- |
| Async runtime | tokio |
| HTTP server | axum, hyper |
| HTTP client | reqwest or hyper, with rustls |
| Serialization | serde, serde_json; a maintained YAML crate; toml |
| JSON Schema | schemars for generation; a JSON Schema validator crate |
| CLI | clap |
| SQLite and Postgres | sqlx, or rusqlite plus tokio-postgres |
| Expressions | a CEL interpreter crate |
| Spreadsheets | calamine |
| Decimals | rust_decimal or bigdecimal |
| OS key store | keyring |
| OAuth 2.1 and OpenID Connect | oauth2, openidconnect |
| JWT validation | jsonwebtoken |
| Encryption | a RustCrypto AEAD crate, or ring |
| Telemetry | opentelemetry, tracing |
| Python bindings | PyO3 with maturin |
| Node bindings | napi-rs |

## Open

- Whether one maintained crate covers the Windows sign-in broker; likely not, so broker support may need direct Windows API calls on user-device runners.
- Chromium version pinning and download from Rust without Playwright's installer.
- A YAML crate with good error locations, since agents and people will read the lint output.
