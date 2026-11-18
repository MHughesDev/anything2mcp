# 604. Management and operations

What it takes to keep an installation running from day to day: what an administrator can see and do, upgrades, backup, retention, alerts, the parts that daily use needs, and the cost of ownership. Every function here is an action in the registry, so whatever the admin console shows is also a command, a REST route, and an SDK method, and can be scripted ([section 220](../2-kernel/220-actions-and-the-authoring-interface.md)). The console has no route of its own into the kernel. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

## What an administrator can do

| Area | What an admin can do |
| --- | --- |
| Targets and versions | See every target, its surfaces, published versions, and binding health; roll a target back to a previous version in one step (`version.rollback`) |
| Toolsets and grants | Create toolsets, assign users and groups, set identity modes and approval policy ([section 214](../2-kernel/214-access-control-toolsets-and-tenancy.md)) |
| Sessions | See which identities are signed in to which targets, their expiry, and their last use; revoke any session. Revoking never lets anyone use the session as its owner ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)) |
| Runs | Watch live runs; inspect any run's ledger, diff, approval, and trace; cancel or resume |
| Approvals | See pending approvals; set standing approvals and their expiry ([section 212](../2-kernel/212-policy-and-approvals.md)) |
| Runners | See connected runners, their load, operating system, and assigned targets; drain a runner for maintenance |
| Providers | See each pack and plug-in, its state, and what it is missing (`provider.list`); the outside components each needs (`component.list`); check everything with `doctor.run` |
| Health | The engine's health state (`engine.status`); canary results, drift alerts, edge blocks, queue depth, error rates, performance against budget |
| Kill switches | Pause one operation, one target, one caller, or the whole system (`killswitch.set`); writes in flight stop at the next checkpoint |
| Audit | Search and export the audit log (`audit.query`); verify its hash chain |
| Secrets | Rotate service-account credentials, API keys, and encryption keys without downtime (`secret.set`, `vault.rotate`; [section 216](../2-kernel/216-the-vault-and-its-keys.md)) |

On one machine the same actions exist, and the person who runs the engine is its administrator. Approving is never among them: recording an approval is not an action and has no command, tool, or route ([section 503](../5-security/503-approval-and-sign-in-surfaces.md)).

## Upgrades

- **The store migrates itself.** The binary migrates its own database on start, and can run against the previous schema during a rolling upgrade.
- **Target files keep loading.** They carry a format version; older formats keep loading, and `version.migrate` (the shortcut `migrate`) rewrites them.
- **One part at a time.** Control-plane replicas and runners can be upgraded one at a time. Servers pin a version and upgrade deliberately; how an update arrives is in [section 605](605-packaging-and-release.md).

## Backup and recovery

The database and the evidence store are backed up with standard tools. The vault is backed up separately, with its keys. After a restore, every run that was in flight is treated as `possible_commit` and reconciled before it continues ([section 208](../2-kernel/208-runs-the-ledger-and-resume.md)).

## Retention

Traces default to 30 days and audit entries to one year; both are settings. Captures used for building tools are deleted once a version is published, unless kept as fixtures, and a fixture is kept only once it is scrubbed (below). Findings and their evidence follow their retention class ([section 217](../2-kernel/217-traces-evidence-and-repair.md)).

## Alerts

Drift, edge blocks, expired service-account sessions, a failed check of the audit chain, a lapsed review of a target's authorization record, and budget regressions raise alerts. Alerts are sent through webhooks, email, or a chat service the organization configures. Each destination is configured by a person; none is on by default ([section 605](605-packaging-and-release.md), "Nothing phones home").

## Starting work without a caller

Agents and programs are not the only way work starts.

- **Triggers.** Schedules, inbound webhooks, and watchers of a folder where files are dropped start pipelines. A trigger runs as a named principal under the same policy as any caller, using standing approvals; a trigger cannot approve.
- Until triggers are built, any outside scheduler can start a run through the REST interface ([section 222](../2-kernel/222-front-doors-and-the-local-engine.md)).

## Usage metering

Calls, records written, and runner time are counted per caller, toolset, and target. The counts can be exported for cost allocation and are tied to quotas. The ledger already holds the data; metering is a view of it.

## Parts that daily use needs, and where each is designed

Each of these closes a gap found by asking how such a system is used day to day. The design is given here in a line; the section named is where it belongs. These rest on design judgment; none has dedicated research behind it yet.

| Part | The gap | Design | Belongs in | Phase |
| --- | --- | --- | --- | --- |
| Target environments | A system has test and production instances | A target declares environments, each with its own surface addresses, sessions, and approval policy. Publish gates run against a test environment; a toolset pins one environment | [Section 205](../2-kernel/205-target-files.md) | 3 |
| Mid-run input | A pipeline could only succeed or stop | A pipeline can declare decision points, such as two customers matching a name. The run rests at `needs_input`, a person chooses through a form or the approval page, and the choice is recorded. Decision points are declared, never improvised | [Section 208](../2-kernel/208-runs-the-ledger-and-resume.md) | 3 |
| Undo | A wrong but approved write had no way back | The ledger keeps prior values for every write. `run.undo` builds the reverse diff, which goes through preview and approval like any write. Not offered for `irreversible` operations | [Section 208](../2-kernel/208-runs-the-ledger-and-resume.md) | 3 |
| Read cache | Every read reached the system | Opt-in per operation: a time limit, keyed by identity and inputs, cleared by the runtime's own writes to the same record. Never used for previews or read-back | [Section 207](../2-kernel/207-the-engine-loop.md) | 4 |
| Data classification | Any field could flow to any agent | Field types carry a sensitivity label. Toolset policy says which labels may reach which callers; others are masked or left out. Restricted fields never appear in what is given to a building agent. An optional hook lets a data-loss-prevention service inspect results | [Section 214](../2-kernel/214-access-control-toolsets-and-tenancy.md) | 6 |
| Identity isolation on runners | Several users' sessions could share one live process | Each identity gets its own process and data directory; in the web pack, its own browser process and profile. High-assurance setups can use a separate operating-system user or container per identity | [Section 602](602-identity-on-a-shared-server.md) | 7 |
| Systems that allow one session | Some systems allow one active session per account, so a sign-in on a server signs the person out elsewhere | The probe tests for it. Remedies: a service account, accepting the trade, or scheduling work outside working hours | [Section 215](../2-kernel/215-sessions-and-the-session-keeper.md) | 7 |
| Tool contract versioning | Changing a tool's inputs or outputs could silently break agents | A tool's schemas are a contract. Additive changes are minor; breaking changes need a new major version. Toolsets pin versions, old versions warn before removal, and a new version can be rolled out to named callers first | [Section 221](../2-kernel/221-the-tool-surface-agents-see.md) | 5 |
| Target authorization record | Nothing recorded that wrapping a system was permitted | On server profiles a target cannot be published without a record of the system's owner, who approved wrapping it, the agreed identity modes and rate limits, the terms reviewed, and a review date. A lapsed review raises an alert | [Section 213](../2-kernel/213-effects-hazards-and-consent.md) | 6 |
| Agent guidance | Agents got tools but no context about the work | Each toolset serves a usage guide and a glossary of the system's business terms as MCP resources and server instructions, generated from the target's files | [Section 221](../2-kernel/221-the-tool-surface-agents-see.md) | 5 |
| Client compatibility | Client support for MCP features was assumed | CI runs protocol conformance for each served MCP revision, and smoke tests against at least two unrelated clients. A table kept as data shows which features each client supports and which fallback applies; no client is preferred | [Section 403](../4-building-tools/403-testing-and-publish-gates.md) | 6 |
| Fixture hygiene | Recorded fixtures hold real business data | A scrubber replaces recorded values with synthetic ones of the same shape. The linter blocks unscrubbed fixtures from being committed | [Section 403](../4-building-tools/403-testing-and-publish-gates.md) | 2 |
| Interfaces in several languages | Locators assumed one interface language | A target declares its interface languages, with a locator set for each | [Section 301](../3-packs/301-the-web-pack.md) | 8 |
| Sharing targets between teams | Each team would rebuild the same targets and profiles | A target is a folder a developer keeps in their own repository. A version is moved between installations with `version.bundle` and `version.import`, signed, through Git or a private registry the team runs. The project hosts no catalog and no place to share them ([decision 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md)) | [Section 205](../2-kernel/205-target-files.md) | 9 |
| Operating-system permissions | The desktop pack needs the operating system's accessibility permission | `doctor.run` detects a missing permission and walks the person through granting it | [Section 305](../3-packs/305-the-desktop-pack.md) | 8 |
| Updates and offline install | No update or air-gapped story | Signed updates, off by default on servers, and an offline bundle | [Section 605](605-packaging-and-release.md) | 9 |
| Triggers, usage metering | Covered above | | This section | 6, 9 |

## Cost of ownership

An install on one machine needs no maintenance beyond updates. A company server needs an owner for three things: reviewing publish requests, answering drift alerts, and keeping service accounts healthy.

## Open points

- The admin console's screens. Every function already exists as an action, so the screens can wait for Phase 6.
- The exact alert destinations offered, and whether a chat service is reached through a webhook alone.
- Whether a trigger's standing approval may cover a write with a hazard other than `none`. The policy rules of [section 213](../2-kernel/213-effects-hazards-and-consent.md) say a person approves such a write every run, so the conservative reading is no.
