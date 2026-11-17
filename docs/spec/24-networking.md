# 24. Networking

The network design has three rules: one inbound endpoint, outbound-only runners, and egress limited to declared targets.

**Inbound.** A single HTTPS listener, normally behind the organization's reverse proxy or API gateway:

| Path | Purpose |
| --- | --- |
| `/mcp/{toolset}` | MCP over Streamable HTTP, one path per toolset |
| `/api/v1` | REST API |
| `/.well-known/oauth-protected-resource` | OAuth metadata that tells clients which identity provider to use |
| `/approve`, `/signin` | Approval pages and the secure sign-in page |
| `/admin` | Admin console; can be bound to a separate internal-only listener |
| `/runners` | Runner connections, mutually authenticated |
| `/healthz`, `/metrics` | Health checks and metrics; internal only |

**Runner links.** Runners dial the control plane over mutually authenticated TLS, using ordinary HTTPS requests and long polls ([section 43](43-runner-protocol.md)). A user's laptop therefore needs no inbound firewall rule, and runners can sit in a different network segment from the control plane.

**Egress.** Each runner may reach only the surfaces of the targets assigned to it, plus the control plane and identity provider. Corporate proxies and private certificate authorities are supported through settings.

**Connecting Copilot Studio.** Copilot Studio accepts only the Streamable transport, and offers no authentication, an API key, or OAuth 2.0 ([Microsoft Learn](https://learn.microsoft.com/en-us/microsoft-copilot-studio/mcp-add-existing-server-to-agent)). It calls from Microsoft's cloud, so a server that exists only on a private network needs one of three bridges:

| Option | How | Notes |
| --- | --- | --- |
| Published endpoint | Expose `/mcp/{toolset}` through Azure API Management or an application gateway, with OAuth and address restrictions | Microsoft documents exposing an existing MCP server through API Management ([guide](https://techcommunity.microsoft.com/blog/azure-ai-foundry-blog/deployment-guide-copilot-studio-agent-with-mcp-server-exposed-by-api-management-/4462432)). Treat the endpoint as public and rely on OAuth |
| Power Platform virtual network | Connector traffic is injected into a subnet of the organization's Azure virtual network, which reaches the server privately | Reported working in a community reference setup ([example](https://github.com/hshen-ai/copilot-studio-zscaler-private-mcp)); confirm against Microsoft's current documentation |
| On-premises data gateway | A custom connector relays traffic through the gateway | One team found the session header did not survive the relay ([issue](https://github.com/NetApp/ontap-mcp/issues/119)); Anything2MCP's stateless mode avoids depending on it |

For Copilot Studio with Entra ID as the identity provider, plan on a pre-registered app and the manual OAuth option, since Copilot Studio's simplest path relies on Dynamic Client Registration. Which MCP revision Copilot Studio speaks, and whether it supports elicitation and the Tasks extension, must be tested; the approval-link and `a2m_run_get` fallbacks exist for exactly that case.

**Other clients.** Claude, VS Code, Cursor, and similar clients on the corporate network connect straight to the internal endpoint with OAuth. `anything2mcp connect <client>` writes or prints the right configuration for each.
