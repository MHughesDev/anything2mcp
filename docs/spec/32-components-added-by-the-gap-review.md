# 32. Components added by the gap review

A review of the design against how such a system is used day to day found eighteen missing components. Each is now part of the design, with the build phase where it lands. These rest on design judgment; none has dedicated research behind it yet.

| Component | The gap | Design | Phase |
| --- | --- | --- | --- |
| Target environments | Apps have test and production instances; the spec had one | A target declares environments, each with its own base URLs, sessions, and approval policy. Publish gates run against a test environment; a toolset pins one environment | 3 |
| Triggers | Only agents and applications could start work | Schedules, inbound webhooks, and file-drop watchers start pipelines. A trigger runs as a named principal under the same policy, using standing approvals | 6 |
| Mid-run input | A pipeline could only succeed or stop | A pipeline can declare decision points, such as two customers matching a name. The run pauses at `needs_input`, a person chooses through a form or the approval page, and the choice is recorded. Decision points are declared, never improvised | 3 |
| Undo | A wrong but approved write had no way back | The ledger keeps prior values for every write. `w2m_run_undo` builds the reverse diff, which goes through preview and approval like any write. Not offered for `irreversible` operations | 3 |
| Read cache | Every read hit the app | Opt-in per operation: a time limit, keyed by identity and inputs, cleared by Web2MCP writes to the same record. Never used for previews or read-back | 4 |
| Data classification | Any field could flow to any agent | Field types carry a sensitivity label. Toolset policy says which labels may reach which callers; others are masked or left out. Restricted fields never appear in captures given to building agents. An optional hook lets a data-loss-prevention service inspect results | 6 |
| Identity isolation on runners | Several users' sessions could share one browser process | Each identity gets its own browser process and profile directory. High-assurance setups can use a separate OS user or container per identity | 7 |
| Single-session apps | Some apps allow one active session per account, so a server sign-in logs the person out | The probe tests for it. Remedies: use a service account, accept the trade, or schedule work outside working hours | 7 |
| Tool contract versioning | Changing a tool's inputs or outputs could silently break agents | A tool's schemas are a contract. Additive changes are minor; breaking changes need a new major version. Toolsets pin versions, old versions warn before removal, and a new version can be rolled out to named callers first | 5 |
| Target authorization record | Nothing recorded that wrapping an app was permitted | On server profiles a target cannot be published without a record of the app's owner, who approved wrapping it, the agreed identity modes and rate limits, the terms reviewed, and a review date. A lapsed review raises an alert | 6 |
| Agent guidance | Agents got tools but no domain context | Each toolset serves a usage guide and a glossary of the app's business terms as MCP resources and server instructions, generated from the target's files | 5 |
| Usage metering | No view of who used what | Calls, records written, and runner time per caller, toolset, and target, exportable for cost allocation and tied to quotas | 9 |
| Updates and offline install | No update or air-gapped story | Signed updates on stable and beta channels, off by default on servers. An offline bundle includes the pinned browser. No data leaves an installation unless configured | 9 |
| Client compatibility | Client support for MCP features was assumed | CI runs protocol conformance for both served MCP revisions and smoke tests against major clients. A published matrix shows which features each client supports and which fallback applies | 6 |
| Fixture hygiene | Recorded fixtures hold real business data | A scrubber replaces recorded values with synthetic ones of the same shape. The linter blocks unscrubbed fixtures from being committed | 2 |
| Localized apps | Locators assumed one interface language | A target declares its interface languages, with a locator set for each | 8 |
| Shared target packs | Each team would rebuild the same targets and profiles | Signed packs of targets and platform profiles, shared through Git or a private registry | 9 |
| Desktop permissions | Desktop mode needs OS accessibility permission | `web2mcp doctor` detects the missing permission and walks the person through granting it | 8 |
