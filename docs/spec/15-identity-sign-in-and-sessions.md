# 15. Identity, sign-in, and sessions

The system works out how each target signs in, reuses sign-ins the device already has where it can, and keeps one session alive for all callers, so a person approves MFA only as often as their organization's policy requires, not once per call.

**Three auth modes**, chosen per surface:

| Mode | What it uses | Typical case |
| --- | --- | --- |
| `use_existing` | What the device already has: the OS sign-in broker for token-based apps (Windows Web Account Manager on Entra-joined PCs, the macOS Enterprise SSO extension, the Linux identity broker where installed), or the engine's own browser profile signing in silently through the device's account where the browser supports it. Never the user's everyday browser profile ([section 44](44-session-keeper.md)) | A work laptop with Entra ID and Edge |
| `managed_session` | A person signs in once in a visible window; the session is stored encrypted and kept alive | Apps with their own login, or no device SSO |
| `custom` | Dedicated credentials: API keys, OAuth client credentials, client certificates, Basic, NTLM or Kerberos, or a dedicated account's password and one-time-code secret where IT allows it | Service accounts, shared servers, vendor APIs |

**Auth discovery.** `anything2mcp add <url>` and the `a2m_auth_detect` tool work out the method and recommend a mode, with reasons, for a person to confirm. They look for:

- Redirects to known identity hosts (Microsoft, Okta, Google, ADFS, Ping) and the provider's OpenID discovery document.
- `WWW-Authenticate` challenges for Basic, NTLM, Negotiate, or Bearer.
- A request for a client certificate.
- In captured traffic: whether the app's backend calls carry a cookie or an `Authorization: Bearer` token; the token's audience and scopes, decoded locally and never sent anywhere; and how the front end renews it.
- Whether the device has an OS broker that can issue a token for that audience to a registered client.

**The session keeper** runs in the background for every active session. [Section 44](44-session-keeper.md) is its full design and takes precedence over this outline:

- **Health check.** A cheap signed-in read at a set interval confirms the session still works.
- **Silent renewal.** Before expiry it renews the session the same way the app would: through the OS broker, a refresh token, or by loading the app in a background browser context so the app's own silent renewal runs.
- **One sign-in for everyone.** All callers allowed to use an identity share its session.
- **Asking a person only when required.** When the identity provider demands interaction, such as an MFA re-prompt, the keeper notifies the person through the MCP client, a desktop notification, or the approval page. Calls wait in the queue instead of failing.
- **Device-bound sessions.** Where Conditional Access binds sessions to a managed device, the keeper runs on that device and does not copy sessions elsewhere.

**Tokens for HTTP modes.** The keeper hands drivers short-lived tokens in memory only. The system never imitates another application's client to obtain tokens. Where a direct token is wanted, the path is an app registration that IT approves with delegated permission to the target's API, so tokens carry the user's own rights and appear openly in sign-in logs.

**The vault.** [Section 48](48-vault-and-keys.md) is the full design. Envelope encryption: sessions and secrets are encrypted with a data key held by the OS key store (Windows DPAPI or Credential Manager, macOS Keychain, Linux Secret Service). On servers the key store is replaced by an external manager such as HashiCorp Vault, a cloud KMS, or 1Password. Every secret value is registered with the redactor so it is masked anywhere it might surface.

The OS broker details in this section come from general knowledge and must be verified per platform in the build phase that implements them.
