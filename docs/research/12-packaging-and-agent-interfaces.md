# Packaging, package names, and agent interfaces

> Written when this product was named Web2MCP. The names in this note are the ones checked or reviewed at the time, and are left as they were. The product is now Anything2MCP ([decision 0056](../decisions/0056-one-name-everywhere-anything2mcp.md)).

**Status:** Partial. Built from search-result summaries and three registry checks on 2026-11-15. No primary page was opened in full. Re-check each detail before relying on it.

Feeds spec sections [39](../spec/39-local-engine-and-clients.md), [40](../spec/40-authoring-interface.md), and [41](../spec/41-packaging-and-release.md).

## Package names

Checked on 2026-11-15 by asking each registry's own tool for the name: `pip index versions`, `npm view`, and `cargo search`.

| Name | PyPI | crates.io | npm |
| --- | --- | --- | --- |
| `web2mcp` | Nothing published | No crate of that name | Taken |
| `w2mcp` | Nothing published | No crate of that name | Free |
| `w2m` | Taken | No crate of that exact name | Taken |
| `web2mcp-rs`, `web2mcp-engine`, `web2mcp-core` | Nothing published | Not checked | Free |

- An existing project on GitHub with the same name tells readers to `pip install web2mcp`, and its `pyproject.toml` declares that name, but `pip` found no distribution under it. An earlier note in this folder said the name was occupied on pip; that was taken from the project's README and was wrong as far as these checks can tell.
- A registry page could not be opened directly, so a name that is registered with no files, reserved, or blocked as too similar to another would not show up here. Uploading a placeholder is the real test.
- The `w2m-*` crate names were not checked one by one.

## Shipping a Rust binary through pip

- maturin has a `bin` mode that packages a Rust binary into a wheel as a script, so the command is on the user's path after install ([maturin bindings](https://maturin.rs/bindings)).
- The same page advises against shipping a binary and a native library in one wheel because it doubles the size, and suggests one or the other.
- From general knowledge, not verified in this pass: well-known Rust-built Python tools are distributed this way, with a small pure-Python package alongside the binary. Confirm that a `bin` wheel can carry a Python package before building on it.

**Consequence for the design.** The pip package ships the binary and a pure-Python SDK that talks to it. In-process bindings are left out of the first release.

## Shipping a binary through npm

- The usual pattern is one package per operating system and processor, plus a top-level package that lists them all as optional dependencies. Each platform package declares which system it is for, and the package manager installs only the matching one ([Sentry](https://blog.sentry.io/publishing-binaries-on-npm/), [platform packages guide](https://thekevinscott.github.io/putitoutthere/guide/npm-platform-packages)).
- esbuild moved to this pattern from a post-install download script ([esbuild pull request](https://github.blesdmm.dns-dynamic.net/evanw/esbuild/pull/1621)).
- Known pitfall: installing with optional dependencies switched off skips every platform package, so the launcher must detect that and say what to do. Sentry's guide adds a post-install download as a fallback; Web2MCP chooses not to, because many organizations disable install scripts.

## Release tooling

- `dist`, formerly cargo-dist, plans a release, builds archives and installers, and publishes them from a version tag. Its installer back ends include Homebrew, a Windows installer, and npm ([dist on docs.rs](https://docs.rs/crate/cargo-dist/latest)).
- Version 0.32.0 was current in search results from 2026, so the tool is maintained.
- Not established: whether it can publish wheels, and whether its npm installer uses platform packages or a download step.

## Commands versus MCP tools for coding agents

What the published comparisons say, with their limits:

- An MCP server's tool definitions are loaded into the agent's context when it connects, whether or not they are used; a command costs nothing until it runs ([CircleCI](https://circleci.com/blog/mcp-vs-cli), [MindStudio](https://www.mindstudio.ai/blog/mcp-vs-cli-ai-agents-token-costs-when-to-use)).
- One vendor write-up argues the real distinction is loading eagerly versus on demand, not the protocol ([PubNub](https://www.pubnub.com/blog/mcp-vs-cli-context-window-cost-blocks-ai)). Tool search in newer clients narrows the gap.
- MCP keeps real advantages: clients with no shell can use it, access is scoped per server, and it carries authentication and audit ([mcpservers.org](https://blog.mcpservers.org/posts/cli-vs-mcp)).
- A practitioner benchmark found that many MCP servers are thin wrappers over commands and that poorly designed ones flood the context ([Mario Zechner](https://mariozechner.at/posts/2025-08-15-mcp-vs-cli/)).
- A counterpoint: one browser-automation comparison found the command route used fewer tokens but took longer ([Ranger](https://outpost.ranger.net/post/the-hidden-cost-of-fewer-tokens/)).
- Several posts repeat cost and success-rate figures from a single third-party benchmark. They are not used here.

**Consequence for the design.** Commands and files are the primary building interface; MCP authoring tools are the second door; both are generated from one registry and both keep large results out of the context until asked for.

## Not verified

- That a maturin `bin` wheel can include a pure-Python package, and the right platform tags.
- Current rules on PyPI, npm, and crates.io for publishing with short-lived credentials.
- Rust support for Unix sockets, Windows named pipes, and peer-identity checks in one crate.
- Operating-system user-presence checks callable from a background process.
