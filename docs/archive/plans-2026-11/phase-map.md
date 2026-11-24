# Phase map

> Archived on 2026-11-17. This plan was written for the first design and is kept as the record of it. The order of work is now the [roadmap](../../plan/roadmap.md).

What to read, what to research first, and what proves each phase is done. Generated from the [build plan](../spec-2026-11/36-build-plan.md), the [open research table](../../research/11-open-research.md), and the [action catalog](../../reference/action-catalog.md); regenerate it when any of them changes.

For every phase, also read [AGENTS.md](../../../AGENTS.md), the [glossary](../../reference/glossary.md), and the [technical reference](../../reference/technical-reference.md).

## Phase 0: Foundations and spikes

**Scope.** Workspace, CI on Windows, macOS, and Linux, file schemas, the test application, the benchmark harness, and reserving the package names. Three spikes: the browser engine base, an `rmcp` server over both transports, and the desktop accessibility library.

**Done when.** each spike has a written decision; benchmarks run in CI.

**Read.** [29](../spec-2026-11/29-repository-crates-and-packaging.md), [41](../spec-2026-11/41-packaging-and-release.md), [36](../spec-2026-11/36-build-plan.md), [7](../../spec/2-kernel/205-target-files.md), [46](../../spec/2-kernel/206-the-compiler-and-the-compiled-target.md), [17](../../spec/3-packs/302-the-web-pack-browser-engine.md), [22](../spec-2026-11/22-front-doors.md)

**Decisions that bind this phase.** [0001](../../decisions/0001-name-web2mcp.md), [0002](../../decisions/0002-rust-first-other-languages-where-they-fit.md), [0004](../../decisions/0004-rust-browser-engine-on-the-devtools-protocol.md), [0011](../../decisions/0011-stateless-streamable-http-and-an-oauth-resource-server.md), [0014](../../decisions/0014-declarative-files-yaml-targets-toml-settings-cel-expressions.md), [0018](../../decisions/0018-license-apache-2-0.md), [0022](../../decisions/0022-published-package-names.md), [0025](../../decisions/0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md)

**Research to run first.** brief 1 (Browser engine base)

**Mostly touches.** a2m-core, a2m-files, the workspace skeleton, the test application, bench

## Phase 1: Solo read path

**Scope.** Core types, the file loader and compiler of section 46, the step trace of section 47, the engine loop, the `ui` driver's read verbs, the local engine with its channel and on-demand start, the action registry with its command and MCP faces for the first few actions, MCP over stdio, traces.

**Done when.** a read operation on the test app gives five identical runs; two clients share one engine and one session; a locally built wheel installs and runs with `pip`; budgets are measured and confirmed or revised.

**Read.** [3](../../spec/1-product/103-design-principles.md), [4](../spec-2026-11/04-system-overview.md), [5](../spec-2026-11/05-core-concepts.md), [6](../spec-2026-11/06-modes-and-drivers.md), [7](../../spec/2-kernel/205-target-files.md), [8](../../spec/2-kernel/207-the-engine-loop.md), [9](../../spec/2-kernel/225-performance.md), [17](../../spec/3-packs/302-the-web-pack-browser-engine.md), [21](../../spec/2-kernel/221-the-tool-surface-agents-see.md), [39](../spec-2026-11/39-local-engine-and-clients.md), [40](../spec-2026-11/40-authoring-interface.md), [46](../../spec/2-kernel/206-the-compiler-and-the-compiled-target.md), [47](../spec-2026-11/47-traces-and-evidence.md)

**Decisions that bind this phase.** [0003](../../decisions/0003-no-ai-at-run-time.md), [0005](../../decisions/0005-seven-modes-behind-one-driver-interface.md), [0006](../../decisions/0006-the-runtime-picks-the-fastest-verified-binding.md), [0023](../../decisions/0023-commands-and-files-first-for-building-mcp-tools-second-one-registry-for-both.md), [0024](../../decisions/0024-one-local-engine-per-user-reached-over-a-socket-or-named-pipe.md), [0031](../../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md), [0036](../../decisions/0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md), [0037](../../decisions/0037-evidence-events-a-step-trace-and-artifacts-redacted-at-capture.md)

**Research to run first.** brief 2 (MCP features and dual-revision serving); brief 3 (Locators and hard widgets on the DevTools protocol); brief 11 (File-format prior art); brief 26 (Local channel on three operating systems); brief 28 (Wheel and npm packaging, verified hands-on)

**Mostly touches.** a2m-core, a2m-files, a2m-expr, a2m-engine, a2m-pack-web-browser, a2m-pack-web-ui, a2m-actions, a2m-local, a2m-mcp, a2m-telemetry, anything2mcp

**Actions built.** `tool.search`, `tool.describe`, `tool.call`, `run.get`, `run.trace`, `screen.snapshot`, `draft.lint`, `plan.show`, `engine.status`, `engine.start`, `engine.stop`, `engine.logs`, `config.show`, `browser.install`, `trace.show`, `trace.export`

## Phase 2: Sessions

**Scope.** Auth discovery, managed and existing sign-ins, the vault and key layout of section 48, the session keeper as designed in section 44.

**Done when.** an expired session stops every call with `needs_login`; ten runs that meet an expired session cause exactly one renewal; a read on one real app chosen by the owner, whose target files stay outside this repository, gives five identical runs; no secret appears in any trace, capture, or log.

**Read.** [15](../spec-2026-11/15-identity-sign-in-and-sessions.md), [44](../spec-2026-11/44-session-keeper.md), [48](../../spec/2-kernel/216-the-vault-and-its-keys.md), [19](../../spec/5-security/501-security-model.md), [13](../spec-2026-11/13-interface-flavors.md)

**Decisions that bind this phase.** [0009](../../decisions/0009-four-identity-modes-on-a-shared-server.md), [0017](../../decisions/0017-operate-openly-no-evasion-of-bot-defenses-or-mfa.md), [0031](../../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md), [0032](../../decisions/0032-one-live-session-per-identity-shared-by-tabs-with-a-single-renewer.md), [0035](../../decisions/0035-a-user-s-session-is-used-only-by-that-user-and-agents-acting-for-them.md), [0038](../../decisions/0038-vault-keys-in-four-layers-with-the-root-in-the-os-store-or-a-key-service.md)

**Research to run first.** brief 4 (Session persistence and Entra session rules); brief 5 (Vault design per operating system); brief 6 (Web sign-in methods and auto-detection); brief 30 (Sign-in in engine-owned profiles, and device-bound sessions)

**Mostly touches.** a2m-auth, a2m-vault

**Actions built.** `session.status`, `session.login`, `session.revoke`, `auth.detect`, `secret.set`, `doctor.run`

## Phase 3: Writes

**Scope.** The policy engine of section 45 with the built-in local policy set, identity strategies and dependent records from section 50, file-input rules and protected settings from section 51, the ledger as designed in section 42 with its rules as tests, reconcile, field types and normalization, sources, preview, the local approval and sign-in surfaces of section 49, read-back.

**Done when.** a spreadsheet-entry pipeline passes the round trip, idempotency, resume, and fault-injection gates on a test record.

**Read.** [10](../spec-2026-11/10-runs-ledger-and-resume.md), [42](../spec-2026-11/42-run-ledger-design.md), [50](../../spec/2-kernel/209-record-identity-and-dependent-records.md), [12](../../spec/2-kernel/210-sources-and-value-normalization.md), [18](../spec-2026-11/18-safety-policy-and-approvals.md), [45](../spec-2026-11/45-policy-engine.md), [49](../../spec/5-security/503-approval-and-sign-in-surfaces.md), [51](../../spec/5-security/502-defending-against-a-manipulated-agent.md), [52](../../spec/2-kernel/224-when-the-engine-itself-fails.md), [27](../../spec/4-building-tools/403-testing-and-publish-gates.md), [11](../../spec/2-kernel/211-concurrency.md)

**Decisions that bind this phase.** [0012](../../decisions/0012-approvals-happen-out-of-reach-of-the-agent-and-bind-to-the-diff.md), [0013](../../decisions/0013-writes-go-through-the-interface-until-a-faster-binding-is-proven.md), [0015](../../decisions/0015-sqlite-locally-postgres-on-servers-and-a-built-in-ledger.md), [0019](../../decisions/0019-priority-order-safe-correct-fast-easy.md), [0020](../../decisions/0020-every-write-is-verified-and-resume-reconciles-first.md), [0026](../../decisions/0026-local-approvals-come-from-the-engine-with-an-operating-system-user-presence-check.md), [0027](../../decisions/0027-ledger-one-row-per-commit-point-with-append-only-events-beside-current-state.md), [0033](../../decisions/0033-policy-decisions-use-cedar-with-grants-compiled-to-it.md), [0034](../../decisions/0034-policy-is-a-versioned-tested-artifact-and-every-decision-is-recorded.md), [0039](../../decisions/0039-local-approval-a-diff-page-for-the-moment-then-an-operating-system-prompt.md), [0041](../../decisions/0041-identity-strategies-make-creates-and-dependent-records-resumable.md), [0042](../../decisions/0042-design-for-an-agent-that-has-been-steered.md), [0043](../../decisions/0043-the-engine-fails-closed-with-three-health-states.md)

**Research to run first.** brief 7 (Durable execution and idempotency); brief 8 (Human approval patterns); brief 12 (Testing against third-party apps); brief 21 (Lessons from RPA products); brief 27 (Operating-system user-presence checks); brief 31 (Cedar spike)

**Mostly touches.** a2m-ledger, a2m-policy, a2m-sources, a2m-sched

**Actions built.** `run.list`, `run.resume`, `run.cancel`, `test.run`, `fixture.record`, `killswitch.set`, `config.set`

## Phase 4: HTTP modes and endpoint mining

**Scope.** `page_http`, `http`, and `api` drivers; capture, inference, shadow, promotion; description import.

**Done when.** an operation gains an HTTP binding through shadow verification, returns identical results, and runs at least ten times faster than its UI binding.

**Read.** [6](../spec-2026-11/06-modes-and-drivers.md), [26](../spec-2026-11/26-agent-toolkit-how-tools-get-built.md), [8](../../spec/2-kernel/207-the-engine-loop.md), [14](../spec-2026-11/14-edge-cases-and-large-applications.md), [51](../../spec/5-security/502-defending-against-a-manipulated-agent.md)

**Decisions that bind this phase.** [0006](../../decisions/0006-the-runtime-picks-the-fastest-verified-binding.md), [0007](../../decisions/0007-new-bindings-run-in-shadow-before-promotion.md), [0013](../../decisions/0013-writes-go-through-the-interface-until-a-faster-binding-is-proven.md), [0042](../../decisions/0042-design-for-an-agent-that-has-been-steered.md)

**Research to run first.** brief 9 (Network channel practice and endpoint inference)

**Mostly touches.** a2m-pack-http, a2m-pack-web-probe

**Actions built.** `capture.start`, `capture.stop`, `endpoints.infer`, `spec.import`, `shadow.report`, `binding.promote`

## Phase 5: Authoring and ease of use

**Scope.** Every authoring action in the registry with all faces generated, the project folder and build state, the seven commands, `doctor`, templates, the guidance file, the Python and TypeScript SDKs, and the release pipeline for pip, npm, crates, and standalone installs.

**Done when.** an agent with no prior knowledge takes a new app from URL to published read tools; first tool in under ten minutes, measured, using only the guidance file, `status`, and `docs`; a tagged pre-release installs from pip and npm on all three operating systems.

**Read.** [25](../../spec/4-building-tools/404-setup-and-ease-of-use.md), [26](../spec-2026-11/26-agent-toolkit-how-tools-get-built.md), [40](../spec-2026-11/40-authoring-interface.md), [41](../spec-2026-11/41-packaging-and-release.md), [51](../../spec/5-security/502-defending-against-a-manipulated-agent.md), [34](../spec-2026-11/34-first-target-archetypes.md)

**Decisions that bind this phase.** [0022](../../decisions/0022-published-package-names.md), [0023](../../decisions/0023-commands-and-files-first-for-building-mcp-tools-second-one-registry-for-both.md), [0025](../../decisions/0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md), [0040](../../decisions/0040-nothing-leaves-the-machine-unless-switched-on.md), [0042](../../decisions/0042-design-for-an-agent-that-has-been-steered.md)

**Research to run first.** brief 10 (AI discovery turned into deterministic steps); brief 29 (Command-line design for agents)

**Mostly touches.** a2m-actions, bindings/python, bindings/typescript, the release pipeline

**Actions built.** `target.add`, `target.probe`, `target.status`, `target.list`, `draft.read`, `draft.write`, `draft.try`, `version.diff`, `publish.request`, `version.publish`, `version.rollback`, `client.connect`, `action.search`, `action.describe`, `action.run`, `docs.get`

## Phase 6: Server

**Scope.** Streamable HTTP, OAuth resource server, toolsets, grants and roles compiled to policies, versioned policy sets with policy tests, decision explanations, REST API and SDKs, admin console, Postgres, approval pages.

**Done when.** two users with different grants share one Team server; a Copilot Studio agent connects through one of the three bridges.

**Read.** [20](../../spec/2-kernel/214-access-control-toolsets-and-tenancy.md), [21](../../spec/2-kernel/221-the-tool-surface-agents-see.md), [22](../spec-2026-11/22-front-doors.md), [23](../../spec/6-running-it/601-deployment-configurations.md), [24](../../spec/6-running-it/603-networking.md), [45](../spec-2026-11/45-policy-engine.md), [52](../../spec/2-kernel/224-when-the-engine-itself-fails.md), [31](../spec-2026-11/31-management-and-operations.md)

**Decisions that bind this phase.** [0010](../../decisions/0010-toolsets-and-few-tools-listed-up-front.md), [0011](../../decisions/0011-stateless-streamable-http-and-an-oauth-resource-server.md), [0033](../../decisions/0033-policy-decisions-use-cedar-with-grants-compiled-to-it.md), [0034](../../decisions/0034-policy-is-a-versioned-tested-artifact-and-every-decision-is-recorded.md), [0035](../../decisions/0035-a-user-s-session-is-used-only-by-that-user-and-agents-acting-for-them.md), [0044](../../decisions/0044-a-published-version-becomes-current-in-stages.md)

**Research to run first.** brief 13 (Local and remote MCP transport security); brief 14 (Prompt injection and MCP-specific threats); brief 15 (Enterprise governance of agent tools); brief 16 (Hosted-agent compatibility, hands-on)

**Mostly touches.** a2m-api, a2m-mcp, a2m-policy, web/console

**Actions built.** `version.activate`, `version.bundle`, `version.import`, `toolset.create`, `toolset.update`, `toolset.list`, `grant.add`, `grant.remove`, `grant.list`, `policy.publish`, `policy.test`, `policy.explain`, `session.list`, `session.end`, `audit.query`

## Phase 7: Runners and identity modes

**Scope.** Control-plane and runner split over the protocol in section 43, enrollment, leases and lost-lease handling, service accounts, On-Behalf-Of tokens, the hosted sign-in page, user-device runners.

**Done when.** one pipeline runs correctly in all four identity modes; killing a runner mid-write leaves no duplicate and no unverified record after automatic reconcile.

**Read.** [43](../../spec/2-kernel/223-the-runner-protocol.md), [16](../../spec/6-running-it/602-identity-on-a-shared-server.md), [44](../spec-2026-11/44-session-keeper.md), [48](../../spec/2-kernel/216-the-vault-and-its-keys.md), [32](../spec-2026-11/32-components-added-by-the-gap-review.md)

**Decisions that bind this phase.** [0008](../../decisions/0008-control-plane-plus-runners.md), [0009](../../decisions/0009-four-identity-modes-on-a-shared-server.md), [0028](../../decisions/0028-runner-link-mutually-authenticated-https-with-long-polls-and-the-same-interface-in-memory.md), [0029](../../decisions/0029-lost-leases-stop-early-quarantine-reconcile-verify-again.md), [0030](../../decisions/0030-where-session-secrets-live.md), [0035](../../decisions/0035-a-user-s-session-is-used-only-by-that-user-and-agents-acting-for-them.md)

**Research to run first.** brief 17 (Remote sign-in to a server-hosted browser); brief 18 (Deployment and scaling patterns)

**Mostly touches.** a2m-runner, a2m-sched, a2m-auth

**Actions built.** `runner.enroll`, `runner.list`, `runner.drain`, `runner.revoke`, `delegation.grant`, `delegation.revoke`, `delegation.list`

## Phase 8: Breadth

**Scope.** Desktop driver, MCP pass-through, site tools, platform profiles and widget drivers for the common platforms, the edge-case handlers.

**Done when.** every hard case in the test app passes on three operating systems; one desktop target is published.

**Read.** [6](../spec-2026-11/06-modes-and-drivers.md), [13](../spec-2026-11/13-interface-flavors.md), [14](../spec-2026-11/14-edge-cases-and-large-applications.md), [32](../spec-2026-11/32-components-added-by-the-gap-review.md)

**Decisions that bind this phase.** [0005](../../decisions/0005-seven-modes-behind-one-driver-interface.md), [0016](../../decisions/0016-plug-ins-are-separate-processes.md)

**Research to run first.** brief 19 (Desktop accessibility automation); brief 24 (WebMCP standard status)

**Mostly touches.** a2m-pack-desktop, a2m-pack-mcp, a2m-pack-web-ui, plugins, profiles

**Actions built.** `plugin.install`, `plugin.list`

## Phase 9: Operations and hardening

**Scope.** Canaries, repair loop, audit chain, alerts, upgrades, backup, every failure in section 52 as a fault-injection test, the manipulation test suite of section 51, the Cluster profile, an outside security review.

**Done when.** a deliberately changed screen is caught by a canary and repaired with no failed write; review findings are closed.

**Read.** [28](../spec-2026-11/28-observability-audit-and-repair.md), [31](../spec-2026-11/31-management-and-operations.md), [47](../spec-2026-11/47-traces-and-evidence.md), [52](../../spec/2-kernel/224-when-the-engine-itself-fails.md), [51](../../spec/5-security/502-defending-against-a-manipulated-agent.md), [32](../spec-2026-11/32-components-added-by-the-gap-review.md), [19](../../spec/5-security/501-security-model.md)

**Decisions that bind this phase.** [0003](../../decisions/0003-no-ai-at-run-time.md), [0029](../../decisions/0029-lost-leases-stop-early-quarantine-reconcile-verify-again.md), [0037](../../decisions/0037-evidence-events-a-step-trace-and-artifacts-redacted-at-capture.md), [0043](../../decisions/0043-the-engine-fails-closed-with-three-health-states.md)

**Research to run first.** brief 20 (Selector repair and drift); brief 22 (Reproducibility and observability)

**Mostly touches.** every crate; operations tooling

**Actions built.** `run.undo`, `version.migrate`, `vault.rotate`, `self.update`

## Research not tied to one phase

- Brief 23 (Vendor APIs and terms of use): before the first real target
- Brief 25 (Gap-review components): the phase each lands in
