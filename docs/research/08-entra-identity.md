# Research: acting as a user, or as a service, under Microsoft Entra ID

**Status:** partial. Gathered from web searches on 2026-11-15; a dedicated research round did not run. Items under "From general knowledge" are unverified. Feeds [spec section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md) and [section 602](../spec/6-running-it/602-identity-on-a-shared-server.md).

## Findings

### Agents and the On-Behalf-Of flow

- An agent can act on behalf of a signed-in user, use its own agent identity, or use its own agent user account. The On-Behalf-Of flow is delegated access: the agent reaches downstream resources with the user's identity and delegated permissions. That token exchange is evaluated by Conditional Access. A policy that targets an agent identity does not cover requests the agent makes on behalf of a user. [Microsoft Learn](https://learn.microsoft.com/en-us/entra/identity/conditional-access/agent-id)
- The agent On-Behalf-Of flow uses standard OAuth 2.0. The client credential can be a secret, a certificate, or a federated identity credential; Microsoft recommends a managed identity where possible. [Microsoft Learn](https://learn.microsoft.com/en-us/entra/agent-id/agent-on-behalf-of-oauth-flow)
- An MCP server protected by Entra ID uses delegated scopes for clients acting for a user and app roles to limit client access. [Microsoft Learn](https://learn.microsoft.com/en-us/entra/agent-id/secure-mcp-server-with-entra-id)

### The Windows sign-in broker

- Web Account Manager (WAM) ships with Windows 10 and later and Windows Server 2019 and later. It supports only Microsoft Entra ID, not third-party identity providers. An application using it must run in an active, interactive Windows user session and be able to show UI. The redirect URI pattern is `ms-appx-web://microsoft.aad.brokerplugin/{client_id}`. Refresh tokens obtained through it are device-bound. [Microsoft Learn](https://learn.microsoft.com/en-us/entra/msal/dotnet/acquiring-tokens/desktop-mobile/wam)

### Rust

- Microsoft publishes no MSAL for Rust. A community `msal` crate implements a public client application, silent token acquisition from a refresh token, the device authorization grant, and, with a `broker` feature, device enrollment and Primary Refresh Token requests. Treat it as something to evaluate, not a dependency to assume. [docs.rs msal](https://docs.rs/msal)

## Design consequences

1. **`per_user_token`** uses On-Behalf-Of. It works only when the target's API is registered in the tenant and grants Anything2MCP's app registration delegated permission. It cannot be used against an API that does not trust Anything2MCP's app.
2. **Broker-based sign-in belongs on user-device runners**, because the broker needs an interactive session. A headless server cannot use it.
3. **`service_account`** can be a dedicated user, a service principal, or an Entra agent identity, depending on what the target accepts.
4. **Anything2MCP never imitates another application's client id** to obtain tokens.

## From general knowledge, to verify

- Refresh tokens issued to single-page applications are limited to 24 hours.
- Token Protection and device-compliance rules in Conditional Access can reject sessions used from an unmanaged machine, which would break `per_user_session` on a server.
- Continuous access evaluation can revoke a session mid-run.
- Sign-in frequency and persistent-browser-session policies decide how often MFA is re-prompted.
- Dedicated user accounts for UI automation need a license in the target app and usually an MFA exemption or a compliant method.

## Open research

A dedicated round should answer: which Conditional Access settings block each identity mode; what Microsoft recommends for automation accounts; how Entra Agent ID fits an MCP server that sometimes acts for users and sometimes for itself; and exactly what to ask IT to approve. See [11-open-research.md](11-open-research.md).
