# Archive

Documents that were true once and are kept as the record of how the design got here. Nothing in this folder is in force. Decision records and journal entries written before 2026-11-17 cite sections by the numbers and names they had then; this page is the key.

## Why there is an archive

The product was first designed as a tool for wrapping web applications. Its specification grew to 72 sections in three generations, with a rule that later sections won where they differed. On 2026-11-17 the specification was rebuilt in seven parts around what the product had become: a neutral kernel and packs that each know one kind of system ([decision 0065](../decisions/0065-the-specification-is-rebuilt-in-parts-and-the-first-design-is-archived.md)). Sections that stood on their own were moved and renumbered. Sections that had been refined by later ones were consolidated, and the originals are kept here.

## From the old section numbers to the new

| Old | Now | What happened | The original |
| --- | --- | --- | --- |
| 1 | [101, Summary](../spec/1-product/101-summary.md) | Consolidated | [Summary](spec-2026-11/01-summary.md) |
| 2 | [102, Requirements and guarantees](../spec/1-product/102-requirements-and-guarantees.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 3 | [103, Design principles](../spec/1-product/103-design-principles.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 4 | [104, System overview](../spec/1-product/104-system-overview.md) | Consolidated | [System overview](spec-2026-11/04-system-overview.md) |
| 5 | [105, Core concepts](../spec/1-product/105-core-concepts.md) | Consolidated | [Core concepts](spec-2026-11/05-core-concepts.md) |
| 6 | [201, Archetypes and modes](../spec/2-kernel/201-archetypes-and-modes.md) | Consolidated | [Modes and drivers](spec-2026-11/06-modes-and-drivers.md) |
| 7 | [205, Target files](../spec/2-kernel/205-target-files.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 8 | [207, The engine loop](../spec/2-kernel/207-the-engine-loop.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 9 | [225, Performance](../spec/2-kernel/225-performance.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 10 | [208, Runs, the ledger, and resume](../spec/2-kernel/208-runs-the-ledger-and-resume.md) | Consolidated | [Runs, ledger, and resume](spec-2026-11/10-runs-ledger-and-resume.md) |
| 11 | [211, Concurrency: many callers at once](../spec/2-kernel/211-concurrency.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 12 | [210, Sources and value normalization](../spec/2-kernel/210-sources-and-value-normalization.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 13 | [301, The web pack](../spec/3-packs/301-the-web-pack.md) | Consolidated | [Interface flavors](spec-2026-11/13-interface-flavors.md) |
| 14 | [301, The web pack](../spec/3-packs/301-the-web-pack.md) | Consolidated | [Edge cases and large applications](spec-2026-11/14-edge-cases-and-large-applications.md) |
| 15 | [215, Sessions and the session keeper](../spec/2-kernel/215-sessions-and-the-session-keeper.md) | Consolidated | [Identity, sign-in, and sessions](spec-2026-11/15-identity-sign-in-and-sessions.md) |
| 16 | [602, Identity on a shared server](../spec/6-running-it/602-identity-on-a-shared-server.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 17 | [302, The web pack: the browser engine](../spec/3-packs/302-the-web-pack-browser-engine.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 18 | [212, Policy and approvals](../spec/2-kernel/212-policy-and-approvals.md) | Consolidated | [Safety, policy, and approvals](spec-2026-11/18-safety-policy-and-approvals.md) |
| 19 | [501, Security model](../spec/5-security/501-security-model.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 20 | [214, Access control, toolsets, and tenancy](../spec/2-kernel/214-access-control-toolsets-and-tenancy.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 21 | [221, The tool surface agents see](../spec/2-kernel/221-the-tool-surface-agents-see.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 22 | [222, Front doors and the local engine](../spec/2-kernel/222-front-doors-and-the-local-engine.md) | Consolidated | [Front doors](spec-2026-11/22-front-doors.md) |
| 23 | [601, Deployment configurations](../spec/6-running-it/601-deployment-configurations.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 24 | [603, Networking](../spec/6-running-it/603-networking.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 25 | [404, Setup and ease of use](../spec/4-building-tools/404-setup-and-ease-of-use.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 26 | [401, Overlaying tools on an existing system](../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md) | Consolidated | [Agent toolkit: how tools get built](spec-2026-11/26-agent-toolkit-how-tools-get-built.md) |
| 27 | [403, Testing and publish gates](../spec/4-building-tools/403-testing-and-publish-gates.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 28 | [217, Traces, evidence, and repair](../spec/2-kernel/217-traces-evidence-and-repair.md) | Consolidated | [Observability, audit, and repair](spec-2026-11/28-observability-audit-and-repair.md) |
| 29 | [605, Packaging and release](../spec/6-running-it/605-packaging-and-release.md) | Consolidated | [Repository, crates, and packaging](spec-2026-11/29-repository-crates-and-packaging.md) |
| 30 | [226, Configuration](../spec/2-kernel/226-configuration.md) | Consolidated | [Settings](spec-2026-11/30-settings.md) |
| 31 | [604, Management and operations](../spec/6-running-it/604-management-and-operations.md) | Consolidated | [Management and operations](spec-2026-11/31-management-and-operations.md) |
| 32 | [604, Management and operations](../spec/6-running-it/604-management-and-operations.md) | Consolidated | [Components added by the gap review](spec-2026-11/32-components-added-by-the-gap-review.md) |
| 33 | [702, Requirements audit](../spec/7-project/702-requirements-audit.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 34 | [301, The web pack](../spec/3-packs/301-the-web-pack.md) | Consolidated | [First target archetypes](spec-2026-11/34-first-target-archetypes.md) |
| 35 | [701, Prior art](../spec/7-project/701-prior-art.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 36 | [roadmap.md](../plan/roadmap.md) | Retired; its content lives outside the specification | [Build plan](spec-2026-11/36-build-plan.md) |
| 37 | [703, Decisions and open questions](../spec/7-project/703-open-questions.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 38 | [sources.md](../references/sources.md) | Retired; its content lives outside the specification | [Sources](spec-2026-11/38-sources.md) |
| 39 | [222, Front doors and the local engine](../spec/2-kernel/222-front-doors-and-the-local-engine.md) | Consolidated | [The local engine and its clients](spec-2026-11/39-local-engine-and-clients.md) |
| 40 | [220, Actions and the authoring interface](../spec/2-kernel/220-actions-and-the-authoring-interface.md) | Consolidated | [The authoring interface](spec-2026-11/40-authoring-interface.md) |
| 41 | [605, Packaging and release](../spec/6-running-it/605-packaging-and-release.md) | Consolidated | [Packaging and release](spec-2026-11/41-packaging-and-release.md) |
| 42 | [208, Runs, the ledger, and resume](../spec/2-kernel/208-runs-the-ledger-and-resume.md) | Consolidated | [Run ledger design](spec-2026-11/42-run-ledger-design.md) |
| 43 | [223, The runner protocol](../spec/2-kernel/223-the-runner-protocol.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 44 | [215, Sessions and the session keeper](../spec/2-kernel/215-sessions-and-the-session-keeper.md) | Consolidated | [The session keeper](spec-2026-11/44-session-keeper.md) |
| 45 | [212, Policy and approvals](../spec/2-kernel/212-policy-and-approvals.md) | Consolidated | [The policy engine](spec-2026-11/45-policy-engine.md) |
| 46 | [206, The compiler and the plan](../spec/2-kernel/206-the-compiler-and-the-plan.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 47 | [217, Traces, evidence, and repair](../spec/2-kernel/217-traces-evidence-and-repair.md) | Consolidated | [Traces and evidence](spec-2026-11/47-traces-and-evidence.md) |
| 48 | [216, The vault and its keys](../spec/2-kernel/216-the-vault-and-its-keys.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 49 | [503, Approval and sign-in surfaces](../spec/5-security/503-approval-and-sign-in-surfaces.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 50 | [209, Record identity and dependent records](../spec/2-kernel/209-record-identity-and-dependent-records.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 51 | [502, Defending against a manipulated agent](../spec/5-security/502-defending-against-a-manipulated-agent.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 52 | [224, When the engine itself fails](../spec/2-kernel/224-when-the-engine-itself-fails.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 53 | [106, What it latches onto](../spec/1-product/106-what-it-latches-onto.md) | Consolidated | [Anything2MCP: the universal runtime](spec-2026-11/53-the-universal-runtime.md) |
| 54 | [201, Archetypes and modes](../spec/2-kernel/201-archetypes-and-modes.md) | Consolidated | [Interaction archetypes](spec-2026-11/54-interaction-archetypes.md) |
| 55 | [202, Kernel and packs](../spec/2-kernel/202-kernel-and-packs.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 56 | [203, The provider contract](../spec/2-kernel/203-the-provider-contract.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 57 | [204, Supervising subsystems](../spec/2-kernel/204-supervising-subsystems.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 58 | [218, Discovery and findings](../spec/2-kernel/218-discovery-and-findings.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 59 | [219, The evidence ladder](../spec/2-kernel/219-the-evidence-ladder.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 60 | [401, Overlaying tools on an existing system](../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md) | Consolidated | [Overlaying tools on an existing system](spec-2026-11/60-overlaying-tools-on-an-existing-system.md) |
| 61 | [213, Effects, hazards, and consent](../spec/2-kernel/213-effects-hazards-and-consent.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 62 | [106, What it latches onto](../spec/1-product/106-what-it-latches-onto.md) | Consolidated | [Catalog of surface kinds](spec-2026-11/62-catalog-of-surface-kinds.md) |
| 63 | [310, The device pack](../spec/3-packs/310-the-device-pack.md) | Consolidated | [Devices and firmware](spec-2026-11/63-devices-and-firmware.md) |
| 64 | [220, Actions and the authoring interface](../spec/2-kernel/220-actions-and-the-authoring-interface.md) | Consolidated | [Rules for tools and capability discovery](spec-2026-11/64-rules-for-tools-and-capability-discovery.md) |
| 65 | [605, Packaging and release](../spec/6-running-it/605-packaging-and-release.md) | Consolidated | [Python-first packaging](spec-2026-11/65-python-first-packaging.md) |
| 66 | [704, Readiness](../spec/7-project/704-readiness.md) | Consolidated | [Readiness of the universal design](spec-2026-11/66-readiness-of-the-universal-design.md) |
| 67 | [402, The scout, and who does what](../spec/4-building-tools/402-the-scout-and-who-does-what.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 68 | [306, The command pack](../spec/3-packs/306-the-command-pack.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 69 | [307, The data pack](../spec/3-packs/307-the-data-pack.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 70 | [308, The file pack](../spec/3-packs/308-the-file-pack.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 71 | [309, The stream pack](../spec/3-packs/309-the-stream-pack.md) | Moved and renumbered; the wording was kept | Not kept: the section itself moved |
| 72 | [310, The device pack](../spec/3-packs/310-the-device-pack.md) | Consolidated | [The device pack](spec-2026-11/72-the-device-pack.md) |

## The plans of the first design

Kept in [plans-2026-11/](plans-2026-11/). The order of work is now the [roadmap](../plan/roadmap.md).

| Plan | What it was |
| --- | --- |
| [The long-term plan](plans-2026-11/anything2mcp-master-plan.md) | The audit, the study of an outside project, the target architecture, and the stages of the change from the first product to this one |
| [Core first](plans-2026-11/core-first.md) | The order of the first slices, and why the core comes before everything else. Its slices keep their numbers in the roadmap |
| [The phase map](plans-2026-11/phase-map.md) | For each phase of the first build plan: what to read, which decisions bind it, what to research first |
| Phases [0](plans-2026-11/phase-0.md), [1](plans-2026-11/phase-1.md), [2](plans-2026-11/phase-2.md), [3](plans-2026-11/phase-3.md) | Task lists for the first four phases |
| [Settings examples](plans-2026-11/settings-example.md) | Two illustrative settings files in the first design's layers and keys. Their successor is spec section 226 |
| [Readiness](plans-2026-11/readiness.md) | What was ready to build on 2026-11-15, and the risks. Its successor is spec section 704 |
