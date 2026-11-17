# 30. Settings

> Archived on 2026-11-17. This is section 30 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 226, Configuration](../../spec/2-kernel/226-configuration.md).

Every behavior has a safe default, and every default can be changed at the right level. Settings are layered, with later layers overriding earlier ones:

1. Built-in defaults for the chosen profile.
2. The system file, `anything2mcp.toml`.
3. Environment variables, prefixed `A2M_`.
4. Command-line flags.
5. Workspace and toolset settings, set by admins.
6. Target, operation, and pipeline files.
7. Per-call options, limited to what policy lets a caller change, such as pinning a mode.

Admins can lock any setting so lower layers cannot override it. `anything2mcp config show --effective` prints the final value of every setting and which layer set it.

| Group | What it controls | Examples |
| --- | --- | --- |
| `profile` | The deployment shape | `solo`, `workstation`, `team`, `company`, `cluster`; roles to run: control plane, runner, or both |
| `server` | Front doors | Listeners, TLS, enabled front doors, base URL, proxy headers, protocol versions served |
| `auth.callers` | Who may connect | Identity provider, accepted audiences, client registration modes, API keys on or off |
| `identity` | Who targets see | Default identity mode per target, allowed modes, service accounts, session lifetime and renewal |
| `modes` | How work is carried out | Enabled packs and drivers, which provider carries out a mode where two could, shadow match threshold for promotion, whether direct HTTP writes are allowed |
| `browser` | The web pack's browser engine | Installed or pinned browser, headless or visible, viewport, locale, time zone, recycle limits |
| `scheduler` | Concurrency | Parallel reads and writes per session, global context cap, queue length, priorities, quotas |
| `policy` | Safety | Approval rules per effect class, pre-approval rules, record limits, two-person rule, kill switches |
| `network` | Reach | Egress allowlist, outbound proxy, private certificate authorities, rate budgets per surface |
| `storage` | Data at rest | Database, trace store, vault back end, retention periods, encryption keys |
| `tools` | The agent-facing surface | Tools listed up front, toolsets, naming prefix, result size cap, concise or detailed results |
| `telemetry` | Visibility | OpenTelemetry endpoint, log level, redaction rules, trace sampling |
| `authoring` | Building | Build profile on or off, capture limits, discovery budgets, when the scout runs and what it may try ([section 67](../../spec/4-building-tools/402-the-scout-and-who-does-what.md)), who may publish |
| `plugins` | Extensions | Allowed plug-ins, signature requirement, capabilities granted |

A complete Solo configuration is empty, because the defaults are the Solo profile. A minimal company server:

```toml
[profile]
name = "company"
roles = ["control-plane"]

[server]
base_url = "https://mcp.example.com"

[auth.callers]
issuer = "https://login.microsoftonline.com/<tenant-id>/v2.0"
audience = "api://anything2mcp"

[storage]
database = "postgres://anything2mcp@db.internal/anything2mcp"
vault = "azure-key-vault"

[policy.write]
approval = "caller-confirms"

[policy.irreversible]
approval = "named-approver"
```
