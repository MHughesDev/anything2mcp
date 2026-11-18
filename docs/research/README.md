# Research notes

What was looked up while designing Anything2MCP, what it found, and how far each note can be trusted. Every claim in the spec that came from outside reading traces back to a note here.

| File | Topic | Status |
| --- | --- | --- |
| [01-interface-flavors.md](01-interface-flavors.md) | Web frameworks and platforms: pitfalls and techniques | Completed |
| [02-agent-tool-design.md](02-agent-tool-design.md) | Naming, describing, and shaping tools for agents | Completed |
| [03-product-landscape.md](03-product-landscape.md) | Existing products and how close each is | Completed; mostly from descriptions |
| [04-browser-runtime-and-bot-defenses.md](04-browser-runtime-and-bot-defenses.md) | Browser lifecycle, queues, bot defenses | Completed |
| [05-data-sources-and-normalization.md](05-data-sources-and-normalization.md) | Spreadsheets, mapping, exact comparison | Completed |
| [06-mcp-protocol-and-authorization.md](06-mcp-protocol-and-authorization.md) | The stateless MCP revision, OAuth, elicitation | Partial; search results only |
| [07-copilot-studio-and-hosted-agents.md](07-copilot-studio-and-hosted-agents.md) | Connecting Copilot Studio; private networks | Partial; untested |
| [08-entra-identity.md](08-entra-identity.md) | Acting as a user or a service under Entra ID | Partial |
| [09-rust-building-blocks.md](09-rust-building-blocks.md) | Crates and engine options | Partial; most crates unverified |
| [10-code-reviews.md](10-code-reviews.md) | browser-to-mcp, Pomerado, the pip `anything2mcp` | Completed; quick reviews |
| [11-open-research.md](11-open-research.md) | 37 research briefs still to run | To do; briefs 1, 2, 24, 27, 32, 33, and 34 are answered by notes 17, 18, 25, 20, 22, 23, and 24 |
| [12-packaging-and-agent-interfaces.md](12-packaging-and-agent-interfaces.md) | Package names, pip and npm packaging, commands versus MCP tools | Partial; search results and registry checks |
| [13-ledger-leases-and-runner-links.md](13-ledger-leases-and-runner-links.md) | Exactly-once limits, database job queues, leases and fencing, workers that dial out | Partial; search results only |
| [14-sessions-and-policy-engines.md](14-sessions-and-policy-engines.md) | Browser profile limits, device-bound sessions, Entra session rules, Cedar | Partial; search results only |
| [15-traces-and-key-stores.md](15-traces-and-key-stores.md) | OpenTelemetry conventions for MCP; operating-system key stores from Rust | Partial; search results only |
| [16-rea-methods.md](16-rea-methods.md) | How the REA project controls outside engines, records evidence, designs tools, and observes applications and firmware; what applies here | Completed; read from source at one commit |
| [17-browser-engine-base.md](17-browser-engine-base.md) | What the browser engine stands on: a DevTools-protocol client of our own, an existing crate, or WebDriver | Completed; a spike run on Linux only, and source read |
| [18-mcp-server-base.md](18-mcp-server-base.md) | What the MCP server stands on, and which protocol revisions it serves | Completed; a spike with two reference client libraries, and the specification's text read |
| [19-desktop-accessibility-base.md](19-desktop-accessibility-base.md) | What the desktop pack stands on, on each operating system | Completed; the Linux back end was run against two small windows, the Windows and macOS programs were type-checked and not run |
| [20-operating-system-user-presence-checks.md](20-operating-system-user-presence-checks.md) | Which prompt the operating system owns can prove a person is present, on each system; what a program running as the user can do against it; how to call each from Rust | Completed; documentation and source read, call paths type-checked for three systems, no prompt pressed |
| [21-wheel-and-npm-packaging.md](21-wheel-and-npm-packaging.md) | How the pip wheel carries the binary and a client, the npm launcher and platform packages, the platform tags, `dist`, and the registries' rules for short-lived publishing credentials | Completed; built and installed on Linux x86-64 only, registry documentation read in its sources, nothing published |
| [22-stream-wire-protocol.md](22-stream-wire-protocol.md) | Which standard protocol the stream pack speaks first, and whether its client is a crate or our own | Completed; documentation and the server's source read, client crates checked on the registry, nothing run against a broker |
| [23-server-database-clients.md](23-server-database-clients.md) | Client crates for Postgres, MySQL, and SQL Server beside the embedded store, for the ledger and the data pack | Completed; registry, source, and licence trees checked, nothing compiled or run |
| [24-turning-tls-on.md](24-turning-tls-on.md) | What it takes to switch TLS on across the workspace: the panic, the options, the cost, the server side, trust roots | Completed; the repository and the crates' source read, licence trees checked, nothing compiled or run |
| [25-webmcp-standard-status.md](25-webmcp-standard-status.md) | The state of the WebMCP draft and whether the engine can call a site's tools through the DevTools protocol | Completed; the draft's repository read, the protocol run against one Chrome build on Linux only |
| [26-desktop-pack-before-it-is-built.md](26-desktop-pack-before-it-is-built.md) | What the desktop pack needs before it is built: each operating system's accessibility interface, the crates, a test stand that runs in CI, the product's rules applied to a desktop application, and the first slice | Completed; sources and registries read, nothing compiled or run; builds on note 19 |
| [27-the-public-landscape-of-agent-tools.md](27-the-public-landscape-of-agent-tools.md) | What else exists in public that lets agents reach software, in eight families; what those projects can do that this product cannot, what makes the popular ones popular, and what this product has that they do not | Completed; repository and registry pages read, nothing installed or run; star counts as rendered, not verified; extends notes 03 and 16 |
| [28-how-a-network-can-be-seen-from-one-machine.md](28-how-a-network-can-be-seen-from-one-machine.md) | How a program running as an ordinary user can say what is on the networks its machine can reach: what the machine already knows, what announces itself, what can be asked, what each operating system asks of the program, how a network is told from another, and the crates | Partial; vendor documentation, source repositories and the crate registry read; no standard opened, nothing built or run; each claim tagged read, search, or memory |
| [29-network-tools-for-agents-and-the-conduct-of-looking.md](29-network-tools-for-agents-and-the-conduct-of-looking.md) | What network tools agents already have and how they bound where an agent may look, and the published norms for looking at a network without it being taken as hostile | Partial; project and standards pages read, several hosts did not load, legal statements from summaries only; nothing installed or run |
| [30-what-can-tell-what-a-compiled-program-does.md](30-what-can-tell-what-a-compiled-program-does.md) | What can tell what a compiled program does: the interfaces a program and its operating system publish, command metadata, the plain facts of a binary, its signer, managed-code metadata, decompilers, firmware unpackers, tracing, and traffic capture, each as a program the person installs, a part to adapt, or code of our own; the studied project as a dependency and as a source of code; the Rust libraries to link and to avoid | Completed; repository, release-feed and registry pages read, nothing installed or run; four commercial engines and several vendor pages only from search summaries; extends note 16 |

## How to read the status

- **Completed:** a research pass opened the primary sources listed. Items it could not verify are listed at the end of the note.
- **Partial:** built from search-result summaries. Useful for direction; re-check against the primary source before relying on a detail.
- **Vendor-reported:** numbers such as memory and speed gains come from the project that benefits from them. Measure before believing.

## Sections of the spec with no research behind them

These rest on design judgment and should be treated as proposals until their research brief is run: safety and approvals, the security model, access control and toolsets, testing and publish gates, the per-OS vault, the components added by the gap review, and the local engine and its approval rules.

Every URL cited anywhere in these docs is listed in [reference/sources.md](../reference/sources.md).
