# Settings examples

Two illustrative `web2mcp.toml` files for the groups in [spec section 30](../spec/30-settings.md). Key names are a draft to be settled when the settings loader is written in Phase 1; the groups and defaults are the design. Every value shown is the default unless a comment says otherwise.

## A laptop

A new install needs no file at all. This is what the defaults amount to.

```toml
[profile]
name = "solo"                 # one engine per user, started on demand

[engine]
idle_stop_minutes = 30        # the engine exits after this long with nothing to do
in_process = false

[browser]
use = "installed"             # the installed Chrome or Edge, with the engine's own data directory
headless = true
locale = "en-US"
timezone = "UTC"
viewport = [1440, 900]

[modes]
enabled = ["ui", "page_http", "http", "api", "site_tools", "mcp"]
order = ["api", "http", "page_http", "ui"]
direct_http_writes = "after_promotion"
shadow_matches_to_promote = { read = 5, write = 20 }

[scheduler]
reads_per_session = 4
ui_writes_per_session = 1
http_writes_per_session = 4
queue_length_per_target = 100

[sessions]
keep_alive_hours = 8          # after last use
probe_minutes = 15
login_wait_minutes = 5

[policy]
approve_writes = "always"
approve_irreversible = "always"   # cannot be changed
approval_window_minutes = 15
preview_freshness_seconds = 60
max_records_per_run = 50

[files]
allowed_roots = ["<project>"]   # where sources and uploads may be read from; protected

[ledger]
intent_window = { ui = 1, http = 8 }

[evidence]
capture = "standard"
trace_retention_days = 30
audit_retention_days = 365

[telemetry]
opentelemetry = false
update_check = false
crash_reports = false
```

## A company server

Only the settings that differ from the defaults are shown.

```toml
[profile]
name = "company"
roles = ["control_plane"]     # runners are separate processes

[server]
base_url = "https://mcp.example.com"
front_doors = ["mcp_http", "rest", "console"]
mcp_revisions = ["2026-07-28", "2025-11-25"]

[auth.callers]
issuer = "https://login.example.com/tenant-id/v2.0"
audience = "api://web2mcp"
api_keys = false

[store]
database = "postgres://web2mcp@db.example.com/web2mcp"
blobs = "s3://web2mcp-evidence"

[vault]
root_key = "kms://example/web2mcp-root"

[identity]
default_mode = "per_user_session"
allowed_modes = ["per_user_token", "per_user_session", "per_user_device", "service_account"]

[runners]
lease_seconds = 30
heartbeat_seconds = 10
quarantine_seconds = 60
certificate_hours = 24

[sessions]
keep_alive_hours = 4

[policy]
approve_writes = "named_approver"
two_person_irreversible = true

[network]
egress = "declared_targets_only"
outbound_proxy = "http://proxy.example.com:8080"

[telemetry]
opentelemetry = true
endpoint = "https://otel.example.com"
```

## Rules for settings

- Layers override in the order given in spec section 30, and an administrator can lock any setting.
- A setting that would weaken a built-in guardrail is refused when the file loads, with the guardrail named.
- Protected settings (approval rules, record limits, allowed file roots, capture level on writes, and guardrail switches) can be changed on one machine only with the operating system's user-presence prompt.
- Secrets never appear in this file. Values such as database passwords are vault or environment references.
- `web2mcp config show --effective` prints every final value and which layer set it.
