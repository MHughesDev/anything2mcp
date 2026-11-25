# Anything2MCP documentation

Everything known about Anything2MCP: the design, the reasons, the evidence, and the plan. The design came first and the code is built to it, in small proven steps. Much of it is now built and tested against the test stands; the [roadmap](plan/roadmap.md) and each crate design say what is built, [readiness](spec/7-project/704-readiness.md) gives the state of each section in one table, and [SCOPE.md](../SCOPE.md) says what the product does not do today.

The scope, both sides, is one page at the root: [SCOPE.md](../SCOPE.md) says what the product does and answers for, and what it does not, with each limit marked as a rule, a design choice, or not built yet.

## Contents

| Folder | What it holds | Files |
| --- | --- | --- |
| [spec/](spec/README.md) | The specification: what the system does and why, in seven chapters | 59 sections |
| [architecture/](architecture/README.md) | How the code is put together: who defines, implements, and joins every interface, and one design per crate | 31 designs and 3 pages |
| [reference/](reference/technical-reference.md) | Interfaces, data model, protocols, failure codes; the [action catalog](reference/action-catalog.md), [file formats](reference/file-formats.md), [steps, snapshots, and diffs](reference/steps-snapshots-and-diffs.md), [ledger schema](reference/ledger-schema.md), [glossary](reference/glossary.md), a [walkthrough](reference/walkthrough.md), and the generated list of every [source URL](reference/sources.md) cited, by document | 8 |
| [plan/](plan/README.md) | The [roadmap](plan/roadmap.md) of every work package, the [working method](plan/working-method.md), the [design review lenses](plan/design-review-lenses.md), the [test application plan](plan/test-application.md), the [publishing setup](plan/publishing-setup.md), the [guide to trying the preview on Windows](plan/trying-the-preview-on-windows.md), the [adoption plan](plan/adoption.md) for being found, tried, and trusted, the [network awareness proposal](plan/network-awareness.md), and the [open questions for the owner](plan/open-questions-for-the-owner.md); the folder's [index](plan/README.md) sorts them by kind | 11 |
| [reviews/](reviews/README.md) | The results of design review rounds: what each looked at, what it found, and where each finding went; the [coverage map](reviews/coverage-map.md) of every way a computer's software can be reached is kept here too; the folder's [index](reviews/README.md) lists them | 12 |
| [decisions/](decisions/README.md) | Decision records: context, choice, consequences, alternatives | 464 |
| [hypotheses/](hypotheses/README.md) | The design's bets, each with an alternative and a test; results as they come in | 42 open |
| [journal/](journal/README.md) | One entry per working session; the newest says where things stand | 16 entries |
| [research/](research/README.md) | Research notes, code reviews, and briefs still to run | 39 |
| [archive/](archive/README.md) | The first design's consolidated sections and its plans, kept as history; the key from old section numbers to new | 44 documents and 1 image |
| assets/ | Diagrams | 2 |

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

Where a part is built, its tests against a test stand are firmer evidence than any row of this table.

The notes on [what remains unresearched](research/11-open-research.md) and the [open questions](spec/7-project/703-open-questions.md) are part of the design, not an afterthought. Close them before the phase that depends on them.

## Keeping these documents true

- The spec in this folder is the master copy. It was rebuilt in seven chapters on 2026-11-17; the [archive](archive/README.md) keeps what it was consolidated from.
- A change in behavior means a change here, in the same commit as the code.
- A change of mind means a new decision record, never an edit to an old one.
- New research gets its own file in `research/`, and the brief it answers is removed from the open list.
- A tested hypothesis gets a result file in `hypotheses/` and its status updated in the register.
- Every session ends with a journal entry.
- Examples, tests, and fixtures name no real application; they use the imaginary `example` target (rule 9 of AGENTS.md, [decision 0021](decisions/0021-no-real-application-is-named-in-the-repository.md)).
