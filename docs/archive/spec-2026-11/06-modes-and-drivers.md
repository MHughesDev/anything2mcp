# 6. Modes and drivers

> Archived on 2026-11-17. This is section 6 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 201, Archetypes and modes](../../spec/2-kernel/201-access-kinds-and-modes.md).

The engine reaches applications through seven drivers. Every driver implements the same interface (prepare, act, read back, check health), so operations, pipelines, the ledger, and the policy gate work the same in every mode.

| Mode | How it reaches the app | Needs | Best for | Main risk |
| --- | --- | --- | --- | --- |
| `ui` | The browser engine drives Chrome, Edge, or Chromium over the DevTools protocol, acting on elements by role, label, text, or test id; no pixel clicks | A browser session | Writes that must pass the app's own validation; apps with no usable traffic | Layout and label changes |
| `page_http` | Requests sent from inside the signed-in page with the page's own session, cookies, and headers | A browser session | Fast reads and proven writes without driving the UI | Front-end logic is skipped |
| `http` | Requests sent directly from code, with no browser at run time | A session cookie or token from the session keeper | Bulk and high-volume work; servers without a browser | Undocumented endpoints change; bot defenses |
| `api` | Calls generated from a published description: OpenAPI, GraphQL schema, OData metadata, or gRPC reflection | A token or key | Apps with official APIs | Coverage gaps |
| `site_tools` | Calls tools the site registers in the browser (WebMCP) | A browser session | Sites that offer them | Early standard, few sites |
| `mcp` | Connects to an MCP server the application already provides and re-exposes chosen tools with this system's policy, approval, and audit around them | The app's MCP endpoint and auth | Apps that ship their own MCP server | Its tools may be too broad or too thin |
| `desktop` | Drives a desktop app through the OS accessibility tree: UI Automation on Windows, the Accessibility API on macOS, AT-SPI on Linux; Electron apps through the browser engine | The app running on the device | Desktop clients | Apps with poor accessibility data |

**How the runtime chooses a mode.** By default the runtime prefers the cheapest mode: API, then direct HTTP, then in-page HTTP, then the UI. An operation can list its bindings in a different order. Each binding has a status: `draft`, `shadow`, `verified`, or `retired`. At call time the runtime takes the first `verified` binding whose driver is available, whose session is valid, and whose surface has not drifted. The choice and the reason are recorded in the trace.

**Fallback rules.**

- Reads fall back to the next verified binding automatically.
- Writes fall back only before anything was sent, or after reconciliation proves nothing was committed. A write never switches mode mid-record.
- A caller or setting can pin a mode, such as `ui` only, for a run.

**Promotion.** A new binding starts in `shadow`. While the verified binding does the real work, the shadow binding predicts what it would have sent and returned, without acting. After a set number of exact matches, the binding can be promoted to `verified` by a person. This is how an operation that began as UI steps gains a direct HTTP binding safely.

**Extension drivers.** The driver interface is public, so others can add modes such as a command-line driver for CLI tools or a database driver for read-only reporting, without changing the core.
