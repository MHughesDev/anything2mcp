# Research: connecting Copilot Studio and other hosted agents

**Status:** gathered from web searches on 2026-11-15. Microsoft Learn pages are primary; several findings come from community posts and are marked. Nothing here was tested hands-on. Feeds [spec section 24](../spec/24-networking.md).

## Findings

### How Copilot Studio connects

- Copilot Studio supports the Streamable transport type. Servers are added through an MCP onboarding wizard or a custom connector. Authentication options are none, API key (header or query), or OAuth 2.0. [Microsoft Learn](https://learn.microsoft.com/en-us/microsoft-copilot-studio/mcp-add-existing-server-to-agent)
- The OAuth options are dynamic discovery, dynamic, and manual. Microsoft's page calls Dynamic Client Registration with discovery the simplest method where supported. [Microsoft Learn](https://learn.microsoft.com/en-us/microsoft-copilot-studio/mcp-add-existing-server-to-agent)
- Community sources report: SSE is no longer supported for MCP after August 2025; a hand-built connector's OpenAPI document needs `x-ms-agentic-protocol: mcp-streamable-1.0`; Copilot Studio supports MCP tools and resources but not prompts. [Hjarni](https://hjarni.com/mcp-fix/copilot-studio-mcp-connection-failed), [eSEOspace](https://eseospace.com/blog/copilot-studio-mcp-setup/)
- MCP servers ride on Power Platform connector infrastructure, so they inherit data-loss-prevention policies and virtual network integration. Secondary source. [BabyBots](https://www.babybots.ai/guides/connect-copilot-studio-agents-mcp-2026)
- An older community post found Copilot Studio sent the API key only on the first call, not every call. Dated; re-test. [The Developer's Cantina](https://www.developerscantina.com/p/mcp-copilot-studio-api-key/)

### Reaching a server on a private network

Copilot Studio calls from Microsoft's cloud, so a localhost, VPN-only, or private-address URL cannot work directly. [Hjarni](https://hjarni.com/mcp-fix/copilot-studio-mcp-connection-failed)

| Bridge | Evidence | Caveat |
| --- | --- | --- |
| Azure API Management or an application gateway in front | Microsoft publishes a guide for exposing an existing MCP server through API Management with OAuth 2.0 and Streamable HTTP. [Tech Community](https://techcommunity.microsoft.com/blog/azure-ai-foundry-blog/deployment-guide-copilot-studio-agent-with-mcp-server-exposed-by-api-management-/4462432) | Allowing Power Platform address ranges admits every tenant's environments, so treat the endpoint as public and rely on OAuth. [Practical 365](https://practical365.com/how-to-plan-mcp-deployment-for-copilot-studio-agents/) |
| Power Platform virtual network support | A community reference setup injects connector traffic into a subnet and reaches a private MCP server through private DNS. [hshen-ai](https://github.com/hshen-ai/copilot-studio-zscaler-private-mcp) | Personal, educational project; confirm against Microsoft's documentation |
| On-premises data gateway with a custom connector | One team got traffic relayed but found the `Mcp-Session-Id` header did not survive the round trip. [NetApp issue](https://github.com/NetApp/ontap-mcp/issues/119) | A stateless server avoids depending on that header |

Microsoft 365 Copilot declarative agents also need an endpoint reachable from the cloud; Microsoft's answer is to publish through a reverse proxy or gateway. [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/5936544/how-to-make-private-mcp-server-accessible-by-a-dec)

### Entra ID and agents

- Microsoft documents protecting an MCP server with Entra ID: require an OAuth access token on every request, use delegated scopes for clients acting for a signed-in user, and app roles to limit a client's access. Agents can authenticate with Entra Agent ID. [Microsoft Learn](https://learn.microsoft.com/en-us/entra/agent-id/secure-mcp-server-with-entra-id)
- In Azure AI Foundry, a hosted agent calling a custom MCP server presents the agent's identity unless OAuth identity passthrough is configured on the MCP tool. [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/5872669/user-identity-passthrough-for-hosted-agents-callin)

## Design consequences

1. Serve stateless Streamable HTTP. Offer OAuth and API keys.
2. With Entra ID as the identity provider, plan on a pre-registered app registration and Copilot Studio's manual OAuth option.
3. Document all three network bridges; recommend API Management or the virtual-network route.
4. Provide fallbacks that need no optional MCP feature: an approval link in the tool result, and the `w2m_run_get` tool for long jobs.

## To test hands-on in Phase 6

- Which MCP revision Copilot Studio speaks, and whether it accepts a 2026-07-28-only server.
- Whether it supports elicitation (form or URL), the Tasks extension, and resources.
- Its limits on tool count, payload size, and call duration.
- Whether each call carries the end user's identity (per-user connection) or a shared connection, for each authentication option.
- Current behavior of API-key delivery.
