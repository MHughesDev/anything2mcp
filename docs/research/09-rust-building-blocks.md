# Research: Rust building blocks

**Status:** partial. The MCP SDK, browser engine options, and desktop accessibility library were looked up on 2026-11-15. Everything under "Candidates from general knowledge" is unverified and must be confirmed in Phase 0 before it is added to `Cargo.toml`. Feeds [spec section 605](../spec/6-running-it/605-packaging-and-release.md) and [section 302](../spec/3-packs/302-the-web-pack-browser-engine.md).

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

**Decision so far.** Anything2MCP owns a small, verb-level engine on the DevTools protocol ([ADR 0004](../decisions/0004-rust-browser-engine-on-the-devtools-protocol.md)). The spike of package 0.4 chose a protocol client of our own, over the debugging pipe, in place of building on chromiumoxide or on Rustwright's core: see [note 17](17-browser-engine-base.md) and [decision 0070](../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md).

**Spike criteria.** Event-driven waits; accessibility-tree access; network event capture and in-page fetch; connect to a browser the engine launched with its own data directory; stability under 30 minutes of continuous use; license; maintenance activity; Windows, macOS, and Linux support.

### Desktop accessibility

- `xa11y` is a Rust library, with Python and JavaScript bindings, that offers one Playwright-style API over the three desktop accessibility systems: AXUIElement on macOS, AT-SPI2 on Linux, and UI Automation on Windows. MIT. [docs.rs xa11y](https://docs.rs/crate/xa11y/0.3.0), [author's post](https://crowecawcaw.github.io/general/2026/05/30/accessibility-for-computer-use.html)
- Per-platform crates also exist: `uiautomation` for Windows UI Automation, `atspi` for AT-SPI2 on Linux, and `accessibility-sys` for macOS. [lib.rs accessibility](https://lib.rs/accessibility)
- AccessKit is for applications that expose an accessibility tree, not for driving other applications. It is not a fit for the desktop driver. [AccessKit](https://github.com/AccessKit/accesskit)

## Verified by use

Added to `Cargo.toml` in slice 0.3, on 2026-11-16, for the test application. Each was looked up on crates.io on the day, and builds and passes tests in CI on Windows, macOS, and Linux.

| Crate | Version | Licence | Used for | Note |
| --- | --- | --- | --- | --- |
| `tokio` | 1.54.1 | MIT | Async runtime | Features `macros`, `net`, `rt-multi-thread`, `sync`, `time` |
| `axum` | 0.8.9 | MIT | HTTP server | Default features off; `http1`, `json`, `tokio` |
| `serde`, `serde_json` | 1.0.229, 1.0.154 | MIT or Apache-2.0 | Serialization | |
| `sha2` | 0.11.1 | MIT or Apache-2.0 | SHA-256, for digests in `a2m-core` | From the RustCrypto project; default features off. Checked on 2026-11-17: published 2026-10-12, builds on three operating systems in CI, and brings only crates under the same licences |
| `reqwest` | 0.13.5 | MIT or Apache-2.0 | HTTP client in the test harness | Default features off, so no TLS yet. The product's choice of TLS is still to make |
| `rusqlite` | 0.43.0 | MIT | The one SQLite of the workspace: the store, the data pack, and the test stands | Feature `bundled`, so SQLite is built in. [Decision 0067](../decisions/0067-the-kernel-is-built-layer-by-layer-in-parallel-and-the-first-driver-is-the-http-packs.md). Its tree brought the Zlib licence |
| `rust_decimal` | 1.43.0 | MIT | Exact decimals, wrapped by `a2m-core` and named nowhere else | Default features off. [Decision 0068](../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md) |
| `unicode-normalization` | 0.1.26 | MIT or Apache-2.0, with Unicode-3.0 data | Unicode form C for text in `a2m-expr` | Default features off. [Decision 0073](../decisions/0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md) |
| `tracing` | 0.1.44 | MIT | Logging macros and the subscriber interface; `a2m-telemetry` writes the subscriber | Default features off; `std`. [Decision 0077](../decisions/0077-logs-through-the-tracing-crate-with-our-own-subscriber-and-spans-through-a-handle.md) |
| `chacha20poly1305` | 0.11.0 | Apache-2.0 or MIT | The vault's one cipher | Default features off; `alloc`, `zeroize`. This row and the eight below: [decision 0074](../decisions/0074-the-vaults-cipher-key-stores-and-other-outside-crates.md) |
| `argon2` | 0.6.0 | MIT or Apache-2.0 | The vault's passphrase route | Default features off; `alloc`, `zeroize` |
| `getrandom` | 0.4.3 | MIT or Apache-2.0 | Keys and nonces | |
| `zeroize` | 1.9.1 | Apache-2.0 or MIT | Wiping keys and secrets from memory | |
| `aho-corasick` | 1.1.5 | MIT or Unlicense | The redactor's matcher | Used under MIT |
| `keyring-core` | 1.0.1 | MIT or Apache-2.0 | The interface over credential stores | |
| `apple-native-keyring-store` | 1.0.2 | MIT or Apache-2.0 | macOS only: the Keychain | Feature `keychain` |
| `windows-native-keyring-store` | 1.1.0 | MIT or Apache-2.0 | Windows only: Credential Manager | Default features off |
| `sha1` | 0.11.0 | MIT or Apache-2.0 | SHA-1 for one-time codes in `a2m-auth` | Default features off. The reason is recorded in decision 0084 |
| `saphyr-parser` | 0.3.4 | MIT or Apache-2.0 | The YAML parser for target files in `a2m-files` | The reason is recorded in decision 0078 |
| `miniz_oxide` | 0.9.1 | MIT, Zlib, or Apache-2.0 | Inflate for a workbook's package in `a2m-sources` | Default features off; `with-alloc`. The reason is recorded in decision 0086 |

The rows from `rusqlite` down were added on 2026-11-17, as the kernel's crates were built. The decision record in each row holds the check that was made. Whether each has yet built in CI on Windows and macOS is not recorded here.

The whole dependency tree is checked by `cargo-deny` in CI against `deny.toml`. The licences in it are MIT, Apache-2.0, BSD-3-Clause, Unicode-3.0, and Zlib. Unicode-3.0 comes through the Unicode data crates that `reqwest` brings in by way of `url`. Zlib was added on 2026-11-17 for a crate in the tree of `rusqlite`.

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
