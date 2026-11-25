# Anything2MCP specification

What Anything2MCP does and why, one section per file, in seven parts. Each section states the design as it stands, and no section outranks another: when two disagree, one of them is out of date and is corrected. Read part 1 the first time. After that, open only the sections a task touches.

The specification describes the design, not the code. Much of it is built: the first paragraph of each section says whether what it describes is built today, and [section 704](7-project/704-readiness.md) gives the state of every section in one table, with where the detail is kept.

How the code is arranged is in [the architecture](../architecture/README.md). Interfaces, once settled, are in the [technical reference](../reference/technical-reference.md). The order of work is the [roadmap](../plan/roadmap.md). Why each choice was made is in the [decision records](../decisions/README.md).

A section's number has three digits, and the first is its part. Sections written before 2026-11-17 had other numbers; the [archive](../archive/README.md) has the key from the old numbers to these, and keeps the sections that were consolidated.

## Part 1: Product

What Anything2MCP is, what it promises, and what it reaches.

| # | Section |
| --- | --- |
| 101 | [Summary](1-product/101-summary.md) |
| 102 | [Requirements and guarantees](1-product/102-requirements-and-guarantees.md) |
| 103 | [Design principles](1-product/103-design-principles.md) |
| 104 | [System overview](1-product/104-system-overview.md) |
| 105 | [Core concepts](1-product/105-core-concepts.md) |
| 106 | [What it latches onto](1-product/106-what-it-latches-onto.md) |
| 107 | [The bar: what the best means](1-product/107-the-bar.md) |

## Part 2: Kernel

What is the same whatever kind of system is integrated.

| # | Section |
| --- | --- |
| 201 | [Access kinds and modes](2-kernel/201-access-kinds-and-modes.md) |
| 202 | [Kernel and packs](2-kernel/202-kernel-and-packs.md) |
| 203 | [The provider contract](2-kernel/203-the-provider-contract.md) |
| 204 | [Supervising subsystems](2-kernel/204-supervising-subsystems.md) |
| 205 | [Target files](2-kernel/205-target-files.md) |
| 206 | [The compiler and the compiled target](2-kernel/206-the-compiler-and-the-compiled-target.md) |
| 207 | [The engine loop](2-kernel/207-the-engine-loop.md) |
| 208 | [Runs, the ledger, and resume](2-kernel/208-runs-the-ledger-and-resume.md) |
| 209 | [Record identity and dependent records](2-kernel/209-record-identity-and-dependent-records.md) |
| 210 | [Sources and value normalization](2-kernel/210-sources-and-value-normalization.md) |
| 211 | [Concurrency: many callers at once](2-kernel/211-concurrency.md) |
| 212 | [Policy and approvals](2-kernel/212-policy-and-approvals.md) |
| 213 | [Effects, hazards, and consent](2-kernel/213-effects-hazards-and-consent.md) |
| 214 | [Access control, toolsets, and tenancy](2-kernel/214-access-control-toolsets-and-tenancy.md) |
| 215 | [Sessions and the session keeper](2-kernel/215-sessions-and-the-session-keeper.md) |
| 216 | [The vault and its keys](2-kernel/216-the-vault-and-its-keys.md) |
| 217 | [Traces, evidence, and repair](2-kernel/217-traces-evidence-and-repair.md) |
| 218 | [Discovery and findings](2-kernel/218-discovery-and-findings.md) |
| 219 | [The evidence ladder](2-kernel/219-the-evidence-ladder.md) |
| 220 | [Actions and the authoring interface](2-kernel/220-actions-and-the-authoring-interface.md) |
| 221 | [The tool surface agents see](2-kernel/221-the-tool-surface-agents-see.md) |
| 222 | [Front doors and the local engine](2-kernel/222-front-doors-and-the-local-engine.md) |
| 223 | [The runner protocol](2-kernel/223-the-runner-protocol.md) |
| 224 | [When the engine itself fails](2-kernel/224-when-the-engine-itself-fails.md) |
| 225 | [Performance](2-kernel/225-performance.md) |
| 226 | [Configuration](2-kernel/226-configuration.md) |

## Part 3: Packs

Knowledge of one kind of system each.

| # | Section |
| --- | --- |
| 301 | [The web pack](3-packs/301-the-web-pack.md) |
| 302 | [The web pack: the browser engine](3-packs/302-the-web-pack-browser-engine.md) |
| 303 | [The HTTP pack](3-packs/303-the-http-pack.md) |
| 304 | [The MCP pack](3-packs/304-the-mcp-pack.md) |
| 305 | [The desktop pack](3-packs/305-the-desktop-pack.md) |
| 306 | [The command pack](3-packs/306-the-command-pack.md) |
| 307 | [The data pack](3-packs/307-the-data-pack.md) |
| 308 | [The file pack](3-packs/308-the-file-pack.md) |
| 309 | [The stream pack](3-packs/309-the-stream-pack.md) |
| 310 | [The device pack](3-packs/310-the-device-pack.md) |

## Part 4: Building tools

How an existing system comes to have verified tools laid over it.

| # | Section |
| --- | --- |
| 401 | [Overlaying tools on an existing system](4-building-tools/401-overlaying-tools-on-an-existing-system.md) |
| 402 | [The scout, and who does what](4-building-tools/402-the-scout-and-who-does-what.md) |
| 403 | [Testing and publish gates](4-building-tools/403-testing-and-publish-gates.md) |
| 404 | [Setup and ease of use](4-building-tools/404-setup-and-ease-of-use.md) |

## Part 5: Security

What is defended, against whom, and how.

| # | Section |
| --- | --- |
| 501 | [Security model](5-security/501-security-model.md) |
| 502 | [Defending against a manipulated agent](5-security/502-defending-against-a-manipulated-agent.md) |
| 503 | [Approval and sign-in surfaces](5-security/503-approval-and-sign-in-surfaces.md) |

## Part 6: Running it

Deployment, networks, operation, and release.

| # | Section |
| --- | --- |
| 601 | [Deployment configurations](6-running-it/601-deployment-configurations.md) |
| 602 | [Identity on a shared server](6-running-it/602-identity-on-a-shared-server.md) |
| 603 | [Networking](6-running-it/603-networking.md) |
| 604 | [Management and operations](6-running-it/604-management-and-operations.md) |
| 605 | [Packaging and release](6-running-it/605-packaging-and-release.md) |

## Part 7: Project

Where the design stands and what it rests on.

| # | Section |
| --- | --- |
| 701 | [Prior art](7-project/701-prior-art.md) |
| 702 | [Requirements audit](7-project/702-requirements-audit.md) |
| 703 | [Decisions and open questions](7-project/703-open-questions.md) |
| 704 | [Readiness](7-project/704-readiness.md) |

## Beside the specification

| Document | Holds |
| --- | --- |
| [Technical reference](../reference/technical-reference.md) | The crate layering, the core types, the driver interface in brief, the data model, failure codes, the command line, the local channel, REST |
| [Action catalog](../reference/action-catalog.md) | Every action a person, an agent, or a program can ask for |
| [File formats](../reference/file-formats.md) | The target files |
| [Steps, snapshots, and diffs](../reference/steps-snapshots-and-diffs.md) | What a step is, what a snapshot holds, how a diff is hashed |
| [Ledger schema](../reference/ledger-schema.md) | The store's tables |
| [Glossary](../reference/glossary.md) | One meaning per word |
| [Walkthrough](../reference/walkthrough.md) | What the finished product should feel like |
