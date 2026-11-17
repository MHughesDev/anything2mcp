# 31. Management and operations

Everything an operator needs is in the admin console and mirrored in the CLI and REST API, so it can be scripted.

| Area | What an admin can do |
| --- | --- |
| Targets and versions | See every target, its surfaces, published versions, and binding health; roll a target back to a previous version in one step |
| Toolsets and grants | Create toolsets, assign users and groups, set identity modes and approval policy |
| Sessions | See which identities are signed in to which targets, their expiry, and their last use; revoke any session |
| Runs | Watch live runs; inspect any run's ledger, diff, approval, and trace; cancel or resume |
| Approvals | See pending approvals; set standing approvals and their expiry |
| Runners | See connected runners, their load, operating system, and assigned targets; drain a runner for maintenance |
| Health | Canary results, drift alerts, edge blocks, queue depth, error rates, performance against budget |
| Kill switches | Pause one operation, one target, one caller, or the whole system; in-flight writes stop at the next checkpoint |
| Audit | Search and export the audit log; verify its hash chain |
| Secrets | Rotate service-account credentials, API keys, and encryption keys without downtime |

**Upgrades.** The binary migrates its own database on start and can run against the previous schema during a rolling upgrade. Target files carry a format version; older formats keep loading, and `anything2mcp migrate` rewrites them. Control-plane replicas and runners can be upgraded one at a time.

**Backup and recovery.** The database and trace store are backed up with standard tools; the vault is backed up separately with its keys. After a restore, every run that was in flight is treated as `possible_commit` and reconciled before it continues.

**Retention.** Traces default to 30 days and audit entries to 1 year, both configurable. Captures used for authoring are deleted once a version is published, unless kept as fixtures.

**Alerts.** Drift, edge blocks, expired service-account sessions, a failed audit-chain check, and budget regressions raise alerts through webhooks, email, Teams, or Slack.

**Cost of ownership.** A Solo install needs no maintenance beyond updates. A company server needs an owner for three things: reviewing publish requests, answering drift alerts, and keeping service accounts healthy.
