# Anything2MCP system design

The specification, one section per file. Read in order the first time; after that, open only the sections a task touches.

Sections 1 to 38 were exported from the working design document on 2026-11-15. Sections 39 to 52 were added afterwards and refine sections 7, 10, 11, 15, 17, 18, 20, 22, 23, 25, 26, 28, and 29; where they differ, the later section wins. Sections 53 to 67 were added on 2026-11-17 and widen the design from applications to any existing system, as Anything2MCP; they refine sections 5, 6, 7, 26, 29, and 41, and win where they differ. Section 67 draws the line between what the runtime automates and what it leaves to the developer. This folder is the master copy.

| # | Section | File |
| --- | --- | --- |
| 1 | Summary | [01-summary.md](01-summary.md) |
| 2 | Requirements and guarantees | [02-requirements-and-guarantees.md](02-requirements-and-guarantees.md) |
| 3 | Design principles | [03-design-principles.md](03-design-principles.md) |
| 4 | System overview | [04-system-overview.md](04-system-overview.md) |
| 5 | Core concepts | [05-core-concepts.md](05-core-concepts.md) |
| 6 | Modes and drivers | [06-modes-and-drivers.md](06-modes-and-drivers.md) |
| 7 | Files and formats | [07-files-and-formats.md](07-files-and-formats.md) |
| 8 | Execution engine | [08-execution-engine.md](08-execution-engine.md) |
| 9 | Performance | [09-performance.md](09-performance.md) |
| 10 | Runs, ledger, and resume | [10-runs-ledger-and-resume.md](10-runs-ledger-and-resume.md) |
| 11 | Concurrency: many agents at once | [11-concurrency-many-agents-at-once.md](11-concurrency-many-agents-at-once.md) |
| 12 | Data sources and value normalization | [12-data-sources-and-value-normalization.md](12-data-sources-and-value-normalization.md) |
| 13 | Interface flavors | [13-interface-flavors.md](13-interface-flavors.md) |
| 14 | Edge cases and large applications | [14-edge-cases-and-large-applications.md](14-edge-cases-and-large-applications.md) |
| 15 | Identity, sign-in, and sessions | [15-identity-sign-in-and-sessions.md](15-identity-sign-in-and-sessions.md) |
| 16 | Identity on a shared server | [16-identity-on-a-shared-server.md](16-identity-on-a-shared-server.md) |
| 17 | Browser engine and runtime | [17-browser-engine-and-runtime.md](17-browser-engine-and-runtime.md) |
| 18 | Safety, policy, and approvals | [18-safety-policy-and-approvals.md](18-safety-policy-and-approvals.md) |
| 19 | Security model | [19-security-model.md](19-security-model.md) |
| 20 | Access control, toolsets, and tenancy | [20-access-control-toolsets-and-tenancy.md](20-access-control-toolsets-and-tenancy.md) |
| 21 | Agent-facing tool surface | [21-agent-facing-tool-surface.md](21-agent-facing-tool-surface.md) |
| 22 | Front doors | [22-front-doors.md](22-front-doors.md) |
| 23 | Deployment configurations | [23-deployment-configurations.md](23-deployment-configurations.md) |
| 24 | Networking | [24-networking.md](24-networking.md) |
| 25 | Setup and ease of use | [25-setup-and-ease-of-use.md](25-setup-and-ease-of-use.md) |
| 26 | Agent toolkit: how tools get built | [26-agent-toolkit-how-tools-get-built.md](26-agent-toolkit-how-tools-get-built.md) |
| 27 | Testing and publish gates | [27-testing-and-publish-gates.md](27-testing-and-publish-gates.md) |
| 28 | Observability, audit, and repair | [28-observability-audit-and-repair.md](28-observability-audit-and-repair.md) |
| 29 | Repository, crates, and packaging | [29-repository-crates-and-packaging.md](29-repository-crates-and-packaging.md) |
| 30 | Settings | [30-settings.md](30-settings.md) |
| 31 | Management and operations | [31-management-and-operations.md](31-management-and-operations.md) |
| 32 | Components added by the gap review | [32-components-added-by-the-gap-review.md](32-components-added-by-the-gap-review.md) |
| 33 | Requirements audit | [33-requirements-audit.md](33-requirements-audit.md) |
| 34 | First target archetypes | [34-first-target-archetypes.md](34-first-target-archetypes.md) |
| 35 | Prior art | [35-prior-art.md](35-prior-art.md) |
| 36 | Build plan | [36-build-plan.md](36-build-plan.md) |
| 37 | Decisions and open questions | [37-decisions-and-open-questions.md](37-decisions-and-open-questions.md) |
| 38 | Sources | [38-sources.md](38-sources.md) |
| 39 | The local engine and its clients | [39-local-engine-and-clients.md](39-local-engine-and-clients.md) |
| 40 | The authoring interface | [40-authoring-interface.md](40-authoring-interface.md) |
| 41 | Packaging and release | [41-packaging-and-release.md](41-packaging-and-release.md) |
| 42 | Run ledger design | [42-run-ledger-design.md](42-run-ledger-design.md) |
| 43 | The runner protocol | [43-runner-protocol.md](43-runner-protocol.md) |
| 44 | The session keeper | [44-session-keeper.md](44-session-keeper.md) |
| 45 | The policy engine | [45-policy-engine.md](45-policy-engine.md) |
| 46 | The target compiler and the compiled plan | [46-target-compiler-and-plan.md](46-target-compiler-and-plan.md) |
| 47 | Traces and evidence | [47-traces-and-evidence.md](47-traces-and-evidence.md) |
| 48 | The vault and its keys | [48-vault-and-keys.md](48-vault-and-keys.md) |
| 49 | Approval and sign-in surfaces | [49-approval-and-sign-in-surfaces.md](49-approval-and-sign-in-surfaces.md) |
| 50 | Record identity and dependent records | [50-record-identity-and-dependent-records.md](50-record-identity-and-dependent-records.md) |
| 51 | Defending against a manipulated agent | [51-defending-against-a-manipulated-agent.md](51-defending-against-a-manipulated-agent.md) |
| 52 | When the engine itself fails | [52-when-the-engine-itself-fails.md](52-when-the-engine-itself-fails.md) |
| 53 | Anything2MCP: the universal runtime | [53-the-universal-runtime.md](53-the-universal-runtime.md) |
| 54 | Interaction archetypes | [54-interaction-archetypes.md](54-interaction-archetypes.md) |
| 55 | Kernel and packs | [55-kernel-and-packs.md](55-kernel-and-packs.md) |
| 56 | The provider contract | [56-the-provider-contract.md](56-the-provider-contract.md) |
| 57 | Supervising subsystems | [57-supervising-subsystems.md](57-supervising-subsystems.md) |
| 58 | Discovery and findings | [58-discovery-and-findings.md](58-discovery-and-findings.md) |
| 59 | The evidence ladder | [59-the-evidence-ladder.md](59-the-evidence-ladder.md) |
| 60 | Overlaying tools on an existing system | [60-overlaying-tools-on-an-existing-system.md](60-overlaying-tools-on-an-existing-system.md) |
| 61 | Effects, hazards, and consent | [61-effects-hazards-and-consent.md](61-effects-hazards-and-consent.md) |
| 62 | Catalog of surface kinds | [62-catalog-of-surface-kinds.md](62-catalog-of-surface-kinds.md) |
| 63 | Devices and firmware | [63-devices-and-firmware.md](63-devices-and-firmware.md) |
| 64 | Rules for tools and capability discovery | [64-rules-for-tools-and-capability-discovery.md](64-rules-for-tools-and-capability-discovery.md) |
| 65 | Python-first packaging | [65-python-first-packaging.md](65-python-first-packaging.md) |
| 66 | Readiness of the universal design | [66-readiness-of-the-universal-design.md](66-readiness-of-the-universal-design.md) |
| 67 | The scout, and who does what | [67-the-scout-and-who-does-what.md](67-the-scout-and-who-does-what.md) |

Related: [technical reference](../reference/technical-reference.md) · [action catalog](../reference/action-catalog.md) · [file formats](../reference/file-formats.md) · [steps, snapshots, and diffs](../reference/steps-snapshots-and-diffs.md) · [ledger schema](../reference/ledger-schema.md) · [walkthrough](../reference/walkthrough.md) · [glossary](../reference/glossary.md) · [settings examples](../reference/settings-example.md) · [decision records](../decisions/README.md) · [research notes](../research/README.md)
