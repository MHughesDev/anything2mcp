# Research notes

What was looked up while designing Web2MCP, what it found, and how far each note can be trusted. Every claim in the spec that came from outside reading traces back to a note here.

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
| [10-code-reviews.md](10-code-reviews.md) | browser-to-mcp, Pomerado, the pip `web2mcp` | Completed; quick reviews |
| [11-open-research.md](11-open-research.md) | 31 research briefs still to run | To do |
| [12-packaging-and-agent-interfaces.md](12-packaging-and-agent-interfaces.md) | Package names, pip and npm packaging, commands versus MCP tools | Partial; search results and registry checks |
| [13-ledger-leases-and-runner-links.md](13-ledger-leases-and-runner-links.md) | Exactly-once limits, database job queues, leases and fencing, workers that dial out | Partial; search results only |
| [14-sessions-and-policy-engines.md](14-sessions-and-policy-engines.md) | Browser profile limits, device-bound sessions, Entra session rules, Cedar | Partial; search results only |
| [15-traces-and-key-stores.md](15-traces-and-key-stores.md) | OpenTelemetry conventions for MCP; operating-system key stores from Rust | Partial; search results only |
| [16-rea-methods.md](16-rea-methods.md) | How the REA project controls outside engines, records evidence, designs tools, and observes applications and firmware; what applies here | Completed; read from source at one commit |

## How to read the status

- **Completed:** a research pass opened the primary sources listed. Items it could not verify are listed at the end of the note.
- **Partial:** built from search-result summaries. Useful for direction; re-check against the primary source before relying on a detail.
- **Vendor-reported:** numbers such as memory and speed gains come from the project that benefits from them. Measure before believing.

## Sections of the spec with no research behind them

These rest on design judgment and should be treated as proposals until their research brief is run: safety and approvals, the security model, access control and toolsets, testing and publish gates, the per-OS vault, the components added by the gap review, and the local engine and its approval rules.

Every URL cited anywhere in these docs is listed in [references/sources.md](../references/sources.md).
