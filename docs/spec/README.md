# Anything2MCP system design

The specification, one section per file. Read in order the first time; after that, open only the sections a task touches.

Sections 1 to 38 were exported from the working design document on 2026-11-15. Sections 39 to 52 were added afterwards and refine sections 7, 10, 11, 15, 17, 18, 20, 22, 23, 25, 26, 28, and 29; where they differ, the later section wins. Sections 53 to 67 were added on 2026-11-17 and widen the design from applications to any existing system, as Anything2MCP; they refine sections 5, 6, 7, 26, 29, and 41, and win where they differ. Section 67 draws the line between what the runtime automates and what it leaves to the developer. Sections 68 to 72 are the designs of five further packs, each written before its pack is built.

**Side.** Each section is marked with the side of the kernel boundary it describes ([section 55](55-kernel-and-packs.md)). *Kernel*: the same whatever is being integrated. A named pack: knowledge of one kind of system. *Product-wide*: scope, method, plans, and catalogs that belong to neither. A section marked as kernel with web pack parts was written when applications were the only kind in view; its rules are the kernel's and its examples are the web pack's. This folder is the master copy.

| # | Section | File | Side |
| --- | --- | --- | --- |
| 1 | Summary | [01-summary.md](01-summary.md) | Product-wide |
| 2 | Requirements and guarantees | [02-requirements-and-guarantees.md](02-requirements-and-guarantees.md) | Product-wide |
| 3 | Design principles | [03-design-principles.md](03-design-principles.md) | Product-wide |
| 4 | System overview | [04-system-overview.md](04-system-overview.md) | Product-wide |
| 5 | Core concepts | [05-core-concepts.md](05-core-concepts.md) | Kernel |
| 6 | Modes and drivers | [06-modes-and-drivers.md](06-modes-and-drivers.md) | Product-wide |
| 7 | Files and formats | [07-files-and-formats.md](07-files-and-formats.md) | Kernel |
| 8 | Execution engine | [08-execution-engine.md](08-execution-engine.md) | Kernel |
| 9 | Performance | [09-performance.md](09-performance.md) | Kernel |
| 10 | Runs, ledger, and resume | [10-runs-ledger-and-resume.md](10-runs-ledger-and-resume.md) | Kernel |
| 11 | Concurrency: many agents at once | [11-concurrency-many-agents-at-once.md](11-concurrency-many-agents-at-once.md) | Kernel |
| 12 | Data sources and value normalization | [12-data-sources-and-value-normalization.md](12-data-sources-and-value-normalization.md) | Kernel |
| 13 | Interface flavors | [13-interface-flavors.md](13-interface-flavors.md) | Web pack |
| 14 | Edge cases and large applications | [14-edge-cases-and-large-applications.md](14-edge-cases-and-large-applications.md) | Web pack |
| 15 | Identity, sign-in, and sessions | [15-identity-sign-in-and-sessions.md](15-identity-sign-in-and-sessions.md) | Kernel, with web pack parts |
| 16 | Identity on a shared server | [16-identity-on-a-shared-server.md](16-identity-on-a-shared-server.md) | Product-wide |
| 17 | Browser engine and runtime | [17-browser-engine-and-runtime.md](17-browser-engine-and-runtime.md) | Web pack |
| 18 | Safety, policy, and approvals | [18-safety-policy-and-approvals.md](18-safety-policy-and-approvals.md) | Kernel |
| 19 | Security model | [19-security-model.md](19-security-model.md) | Kernel |
| 20 | Access control, toolsets, and tenancy | [20-access-control-toolsets-and-tenancy.md](20-access-control-toolsets-and-tenancy.md) | Kernel |
| 21 | Agent-facing tool surface | [21-agent-facing-tool-surface.md](21-agent-facing-tool-surface.md) | Kernel |
| 22 | Front doors | [22-front-doors.md](22-front-doors.md) | Kernel |
| 23 | Deployment configurations | [23-deployment-configurations.md](23-deployment-configurations.md) | Product-wide |
| 24 | Networking | [24-networking.md](24-networking.md) | Product-wide |
| 25 | Setup and ease of use | [25-setup-and-ease-of-use.md](25-setup-and-ease-of-use.md) | Product-wide |
| 26 | Agent toolkit: how tools get built | [26-agent-toolkit-how-tools-get-built.md](26-agent-toolkit-how-tools-get-built.md) | Kernel, with web pack parts |
| 27 | Testing and publish gates | [27-testing-and-publish-gates.md](27-testing-and-publish-gates.md) | Kernel |
| 28 | Observability, audit, and repair | [28-observability-audit-and-repair.md](28-observability-audit-and-repair.md) | Kernel |
| 29 | Repository, crates, and packaging | [29-repository-crates-and-packaging.md](29-repository-crates-and-packaging.md) | Product-wide |
| 30 | Settings | [30-settings.md](30-settings.md) | Kernel |
| 31 | Management and operations | [31-management-and-operations.md](31-management-and-operations.md) | Product-wide |
| 32 | Components added by the gap review | [32-components-added-by-the-gap-review.md](32-components-added-by-the-gap-review.md) | Product-wide |
| 33 | Requirements audit | [33-requirements-audit.md](33-requirements-audit.md) | Product-wide |
| 34 | First target archetypes | [34-first-target-archetypes.md](34-first-target-archetypes.md) | Web pack |
| 35 | Prior art | [35-prior-art.md](35-prior-art.md) | Product-wide |
| 36 | Build plan | [36-build-plan.md](36-build-plan.md) | Product-wide |
| 37 | Decisions and open questions | [37-decisions-and-open-questions.md](37-decisions-and-open-questions.md) | Product-wide |
| 38 | Sources | [38-sources.md](38-sources.md) | Product-wide |
| 39 | The local engine and its clients | [39-local-engine-and-clients.md](39-local-engine-and-clients.md) | Kernel |
| 40 | The authoring interface | [40-authoring-interface.md](40-authoring-interface.md) | Kernel |
| 41 | Packaging and release | [41-packaging-and-release.md](41-packaging-and-release.md) | Product-wide |
| 42 | Run ledger design | [42-run-ledger-design.md](42-run-ledger-design.md) | Kernel |
| 43 | The runner protocol | [43-runner-protocol.md](43-runner-protocol.md) | Kernel |
| 44 | The session keeper | [44-session-keeper.md](44-session-keeper.md) | Kernel, with web pack parts |
| 45 | The policy engine | [45-policy-engine.md](45-policy-engine.md) | Kernel |
| 46 | The target compiler and the compiled plan | [46-target-compiler-and-plan.md](46-target-compiler-and-plan.md) | Kernel |
| 47 | Traces and evidence | [47-traces-and-evidence.md](47-traces-and-evidence.md) | Kernel |
| 48 | The vault and its keys | [48-vault-and-keys.md](48-vault-and-keys.md) | Kernel |
| 49 | Approval and sign-in surfaces | [49-approval-and-sign-in-surfaces.md](49-approval-and-sign-in-surfaces.md) | Kernel, with web pack parts |
| 50 | Record identity and dependent records | [50-record-identity-and-dependent-records.md](50-record-identity-and-dependent-records.md) | Kernel |
| 51 | Defending against a manipulated agent | [51-defending-against-a-manipulated-agent.md](51-defending-against-a-manipulated-agent.md) | Kernel |
| 52 | When the engine itself fails | [52-when-the-engine-itself-fails.md](52-when-the-engine-itself-fails.md) | Kernel |
| 53 | Anything2MCP: the universal runtime | [53-the-universal-runtime.md](53-the-universal-runtime.md) | Product-wide |
| 54 | Interaction archetypes | [54-interaction-archetypes.md](54-interaction-archetypes.md) | Kernel |
| 55 | Kernel and packs | [55-kernel-and-packs.md](55-kernel-and-packs.md) | Kernel |
| 56 | The provider contract | [56-the-provider-contract.md](56-the-provider-contract.md) | Kernel |
| 57 | Supervising subsystems | [57-supervising-subsystems.md](57-supervising-subsystems.md) | Kernel |
| 58 | Discovery and findings | [58-discovery-and-findings.md](58-discovery-and-findings.md) | Kernel |
| 59 | The evidence ladder | [59-the-evidence-ladder.md](59-the-evidence-ladder.md) | Kernel |
| 60 | Overlaying tools on an existing system | [60-overlaying-tools-on-an-existing-system.md](60-overlaying-tools-on-an-existing-system.md) | Product-wide |
| 61 | Effects, hazards, and consent | [61-effects-hazards-and-consent.md](61-effects-hazards-and-consent.md) | Kernel |
| 62 | Catalog of surface kinds | [62-catalog-of-surface-kinds.md](62-catalog-of-surface-kinds.md) | Product-wide |
| 63 | Devices and firmware | [63-devices-and-firmware.md](63-devices-and-firmware.md) | Device pack |
| 64 | Rules for tools and capability discovery | [64-rules-for-tools-and-capability-discovery.md](64-rules-for-tools-and-capability-discovery.md) | Kernel |
| 65 | Python-first packaging | [65-python-first-packaging.md](65-python-first-packaging.md) | Product-wide |
| 66 | Readiness of the universal design | [66-readiness-of-the-universal-design.md](66-readiness-of-the-universal-design.md) | Product-wide |
| 67 | The scout, and who does what | [67-the-scout-and-who-does-what.md](67-the-scout-and-who-does-what.md) | Kernel |
| 68 | The command pack | [68-the-command-pack.md](68-the-command-pack.md) | Command pack |
| 69 | The data pack | [69-the-data-pack.md](69-the-data-pack.md) | Data pack |
| 70 | The file pack | [70-the-file-pack.md](70-the-file-pack.md) | File pack |
| 71 | The stream pack | [71-the-stream-pack.md](71-the-stream-pack.md) | Stream pack |
| 72 | The device pack | [72-the-device-pack.md](72-the-device-pack.md) | Device pack |

Related: [technical reference](../reference/technical-reference.md) · [action catalog](../reference/action-catalog.md) · [file formats](../reference/file-formats.md) · [steps, snapshots, and diffs](../reference/steps-snapshots-and-diffs.md) · [ledger schema](../reference/ledger-schema.md) · [walkthrough](../reference/walkthrough.md) · [glossary](../reference/glossary.md) · [settings examples](../reference/settings-example.md) · [decision records](../decisions/README.md) · [research notes](../research/README.md)
