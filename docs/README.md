# Web2MCP documentation

Everything known about Web2MCP before the first line of code: the design, the reasons, the evidence, and the plan.

## Contents

| Folder | What it holds | Files |
| --- | --- | --- |
| [spec/](spec/README.md) | The system design, one section per file | 52 sections |
| [reference/](reference/technical-reference.md) | Interfaces, data model, protocols, failure codes; the [action catalog](reference/action-catalog.md), [file formats](reference/file-formats.md), [steps, snapshots, and diffs](reference/steps-snapshots-and-diffs.md), [ledger schema](reference/ledger-schema.md), [glossary](reference/glossary.md), [settings examples](reference/settings-example.md), and a [walkthrough](reference/walkthrough.md) | 8 |
| [decisions/](decisions/README.md) | Decision records: context, choice, consequences, alternatives | 44 |
| [research/](research/README.md) | Research notes, code reviews, and briefs still to run | 15 |
| [references/](references/sources.md) | Every source URL cited, by document; generated | 1 |
| [plan/](plan/readiness.md) | The [readiness report](plan/readiness.md), the [phase map](plan/phase-map.md), task lists for Phases [0](plan/phase-0.md), [1](plan/phase-1.md), [2](plan/phase-2.md), and [3](plan/phase-3.md), the [test application plan](plan/test-application.md), and the [publishing setup](plan/publishing-setup.md) | 8 |
| assets/ | Diagrams used by the spec | 3 |

## Reading paths

**Thirty minutes, to understand the system.** Spec sections [1](spec/01-summary.md), [4](spec/04-system-overview.md), [5](spec/05-core-concepts.md), [6](spec/06-modes-and-drivers.md), [23](spec/23-deployment-configurations.md), then [33](spec/33-requirements-audit.md) for what is and is not covered.

**To build.** [AGENTS.md](../AGENTS.md), the [readiness report](plan/readiness.md), [decisions](decisions/README.md), the [phase map](plan/phase-map.md), [Phase 0](plan/phase-0.md), then the sections for the phase in hand.

**To judge the design.** [Requirements audit](spec/33-requirements-audit.md), [security model](spec/19-security-model.md), [prior art](spec/35-prior-art.md), [open questions](spec/37-decisions-and-open-questions.md), and the status table in [research](research/README.md).

## How much to trust each part

| Part | Basis |
| --- | --- |
| Interface flavors, tool design, product landscape, browser runtime, data normalization | Research passes that opened primary sources |
| MCP protocol, Copilot Studio, Entra identity, Rust crates, packaging, agent interfaces, ledger and leases, sessions, the policy engine, traces, key stores | Search-result summaries; direction is sound, details need re-checking |
| Approvals, security model, access control, testing gates, vault, gap-review components, the local engine | Design judgment; no dedicated research yet |
| Speed budgets and the ten-minute setup target | Targets, not measurements |
| Example field names, URLs, and commands | Placeholders |

The notes on [what remains unresearched](research/11-open-research.md) and the [open questions](spec/37-decisions-and-open-questions.md) are part of the design, not an afterthought. Close them before the phase that depends on them.

## Keeping these documents true

- The spec in this folder is the master copy. Sections 1 to 38 were exported from a working design document on 2026-11-15; later sections were written here.
- A change in behavior means a change here, in the same commit as the code.
- A change of mind means a new decision record, never an edit to an old one.
- New research gets its own file in `research/`, and the brief it answers is removed from the open list.
