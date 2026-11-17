# 26. Agent toolkit: how tools get built

Any AI agent builds Anything2MCP tools through a set of authoring actions. Each action is available as a command and as an MCP tool, generated from one registry; commands and files are the primary interface and MCP is the second door ([section 40](40-authoring-interface.md)). The table uses the MCP names; the [action catalog](../reference/action-catalog.md) lists every action. Anything2MCP supplies evidence and guardrails; the agent supplies the reasoning. No model provider is built in.

| Authoring tool | What it gives the agent |
| --- | --- |
| `a2m_target_probe` | Platform, framework, support tier, and surfaces found at a URL or in a running desktop app |
| `a2m_auth_detect` | The sign-in method, with evidence, and a recommended identity mode |
| `a2m_screen_snapshot` | A compact accessibility snapshot of the current page or window, with element references |
| `a2m_capture_start`, `a2m_capture_stop` | A recording of a person or the agent using the app: actions, requests, and responses, with secrets already redacted |
| `a2m_endpoints_infer` | Endpoint templates, schemas, token and ID dependencies, and a map from typed values to request fields |
| `a2m_spec_import` | Operations generated from an OpenAPI, GraphQL, or OData description, or from an upstream MCP server's tool list |
| `a2m_draft_write` | Writes target files after schema validation and linting |
| `a2m_draft_try` | Runs a draft binding in a sandbox: reads freely, writes only against declared test records |
| `a2m_test_run` | Runs the publish gates and returns evidence |
| `a2m_shadow_report` | How often a shadow binding matched the verified one, and where it differed |
| `a2m_publish_request` | Submits a version for human review |

**The build loop.** Probe, detect auth, sign in, capture, infer, draft, try, test, request publish. A person's only required steps are signing in and approving the publish.

**Endpoint mining.** This is how an app with no documented API gains fast HTTP bindings:

1. **Capture** traffic inside the browser while the app is used normally. Capturing in the browser avoids installing a system proxy or certificate.
2. **Infer** endpoints by grouping requests into method and path templates, comparing recordings to find what varies, and matching typed values to body fields.
3. **Draft** an HTTP binding beside the UI binding for the same operation.
4. **Shadow.** While the UI binding does the real work, the HTTP binding predicts the request the page will send. Matches and differences are recorded.
5. **Promote** after a set number of exact matches and a person's approval. Reads can be promoted quickly; writes need more evidence.

Mining can stop short. If the front end computes values before saving or signs its requests, the HTTP binding stays in shadow or is marked unsupported, and the UI binding remains the way in.

**Discovery stays read-only by enforcement, not trust.**

- During exploration the browser blocks every request that is not a GET, except read endpoints a person has marked safe; each blocked request is logged.
- Controls whose role or name means committing, such as save, submit, delete, approve, send, or pay, are never activated.
- Exploration stays inside the target's declared surfaces and within a page and time budget.

**What the agent never sees.** Cookies, tokens, and passwords are redacted from every snapshot, capture, and trace before the agent receives it. The agent works with the shape of the traffic, not the secrets in it.
