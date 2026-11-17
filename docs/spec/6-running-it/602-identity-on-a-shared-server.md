# 602. Identity on a shared server

On a company server, every run must answer one question: who does the target application see? Anything2MCP supports four answers, set per target and per toolset, and they can be mixed.

| Identity mode | The app sees | How it works | Modes it serves | Conditions |
| --- | --- | --- | --- | --- |
| `per_user_token` | The calling user | The caller's access token is exchanged through the OAuth On-Behalf-Of flow for a token to the target's API | `api`, `http` | The target API is registered in the organization's Entra tenant and grants Anything2MCP delegated permission |
| `per_user_session` | The calling user | The user signs in once to a browser hosted by a server runner, shown to them on a secure sign-in page; the session is stored for that user alone | `ui`, `page_http`, cookie-based `http` | Fails where Conditional Access demands a managed device; device-bound methods such as Windows Hello are not expected to work in a remote browser |
| `per_user_device` | The calling user | The job is routed to a runner on the user's own computer, which uses the sign-in that device already has | All, including `desktop` | The computer is on and the runner is installed |
| `service_account` | Anything2MCP's own account | A dedicated user in the app, a service principal, or an Entra agent identity, with credentials in the vault | All | The app licenses the account; IT approves it |

**Choosing.** Use `per_user_token` wherever the app's API allows it: it is the cleanest and fastest. Use `per_user_device` when sessions are bound to managed devices. Use `per_user_session` for UI-only apps without device rules. Use `service_account` for shared, unattended, or high-volume work, and wherever attribution to one automation account is acceptable.

**Rules that hold in every mode.**

- **Policy picks the identity, never the caller.** A call cannot name the account it runs as. The identity comes from the caller's verified token and the toolset's settings.
- **No user borrows another's rights.** A per-user session is usable only by that user and by agents acting for that user; nobody else, including administrators, can run through it ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)).
- **Service accounts do not widen access.** Anything2MCP checks the caller's grants before using the service account, and records the real caller in the audit log. Where the app has a note or reference field, a pipeline can stamp who the entry was made for.
- **Mixed setups are normal.** A common pattern is reads through a service account and writes as the user.
- **Leavers lose access at once.** When the identity provider disables a user, their token stops validating, and their stored sessions are revoked on the next sync.

**Entra specifics.** Microsoft documents how to protect an MCP server with Entra ID using delegated scopes and app roles, and how agents authenticate with their own Entra Agent ID instead of an embedded secret ([Microsoft Learn](https://learn.microsoft.com/en-us/entra/agent-id/secure-mcp-server-with-entra-id)). Agents can act for a signed-in user through On-Behalf-Of or with their own identity, and the On-Behalf-Of exchange is itself evaluated by Conditional Access ([Microsoft Learn](https://learn.microsoft.com/en-us/entra/identity/conditional-access/agent-id)). Anything2MCP's `service_account` mode can therefore be an Entra agent identity where the target accepts app-only tokens.

**Brokers and Rust.** Microsoft publishes no official MSAL for Rust. Token flows use standard OAuth 2.1 and OpenID Connect crates. The Windows sign-in broker requires an interactive user session ([Microsoft Learn](https://learn.microsoft.com/en-us/entra/msal/dotnet/acquiring-tokens/desktop-mobile/wam)), so broker-based sign-in belongs on user-device runners, not on a headless server. A community [`msal` crate](https://docs.rs/msal) exists and should be evaluated, not assumed.
