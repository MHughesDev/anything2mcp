# Anything2MCP documentation

Everything known about Anything2MCP: the design, the reasons, the evidence, and the plan. The design came first and the code is being built to it, so most of what is described here does not run yet; the [changelog](../CHANGELOG.md) says what does.

## Contents

| Folder | What it holds | Files |
| --- | --- | --- |
| [spec/](spec/README.md) | The specification: what the system does and why, in seven parts | 59 sections |
| [architecture/](architecture/README.md) | How the code is put together: who defines, implements, and joins every interface, and one design per crate | 31 |
| [reference/](reference/technical-reference.md) | Interfaces, data model, protocols, failure codes; the [action catalog](reference/action-catalog.md), [file formats](reference/file-formats.md), [steps, snapshots, and diffs](reference/steps-snapshots-and-diffs.md), [ledger schema](reference/ledger-schema.md), [glossary](reference/glossary.md), and a [walkthrough](reference/walkthrough.md) | 7 |
| [plan/](plan/roadmap.md) | The [roadmap](plan/roadmap.md) of every work package, the [working method](plan/working-method.md), the [design review lenses](plan/design-review-lenses.md), the [test application plan](plan/test-application.md), and the [publishing setup](plan/publishing-setup.md) | 5 |
| [decisions/](decisions/README.md) | Decision records: context, choice, consequences, alternatives | 185 |
| [hypotheses/](hypotheses/README.md) | The design's bets, each with an alternative and a test; results as they come in | 40 open |
| [journal/](journal/README.md) | One entry per working session; the newest says where things stand | 9 |
| [research/](research/README.md) | Research notes, code reviews, and briefs still to run | 25 |
| [references/](references/sources.md) | Every source URL cited, by document; generated | 1 |
| [archive/](archive/README.md) | The first design's consolidated sections and its plans, kept as history; the key from old section numbers to new | 45 |
| assets/ | Diagrams | 3 |

## Reading paths

**Thirty minutes, to understand the system.** Spec sections [101](spec/1-product/101-summary.md), [104](spec/1-product/104-system-overview.md), [105](spec/1-product/105-core-concepts.md), [106](spec/1-product/106-what-it-latches-onto.md), [202](spec/2-kernel/202-kernel-and-packs.md), and [402](spec/4-building-tools/402-the-scout-and-who-does-what.md).

**To build.** [AGENTS.md](../AGENTS.md), the newest [journal](journal/README.md) entry, the [working method](plan/working-method.md), the [roadmap](plan/roadmap.md), the [architecture](architecture/README.md) and the design of the crate in hand, then the spec sections that design names.

**To configure it.** [Configuration](spec/2-kernel/226-configuration.md): the layers, the scopes, the presets, every group of settings, and what no setting can do.

**To judge the design.** [The bar](spec/1-product/107-the-bar.md), the [requirements audit](spec/7-project/702-requirements-audit.md), the [security model](spec/5-security/501-security-model.md), [prior art](spec/7-project/701-prior-art.md), [readiness](spec/7-project/704-readiness.md), [open questions](spec/7-project/703-open-questions.md), and the status table in [research](research/README.md).

## How much to trust each part

| Part | Basis |
| --- | --- |
| Interface flavors, tool design, product landscape, browser runtime, data normalization | Research passes that opened primary sources |
| MCP protocol, Copilot Studio, Entra identity, Rust crates, packaging, agent interfaces, ledger and leases, sessions, the policy engine, traces, key stores | Search-result summaries; direction is sound, details need re-checking |
| Approvals, security model, access control, testing gates, vault, gap-review components, the local engine | Design judgment; no dedicated research yet |
| Speed budgets and the ten-minute setup target | Targets, not measurements |
| Example field names, URLs, and commands | Placeholders |

The notes on [what remains unresearched](research/11-open-research.md) and the [open questions](spec/7-project/703-open-questions.md) are part of the design, not an afterthought. Close them before the phase that depends on them.

## Keeping these documents true

- The spec in this folder is the master copy. It was rebuilt in seven parts on 2026-11-17; the [archive](archive/README.md) keeps what it was consolidated from.
- A change in behavior means a change here, in the same commit as the code.
- A change of mind means a new decision record, never an edit to an old one.
- New research gets its own file in `research/`, and the brief it answers is removed from the open list.
- A tested hypothesis gets a result file in `hypotheses/` and its status updated in the register.
- Every session ends with a journal entry.
