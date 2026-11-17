# Phase 0: foundations and spikes

> Archived on 2026-11-17. This plan was written for the first design and is kept as the record of it. The order of work is now the [roadmap](../../plan/roadmap.md).

Phase 0 turns an empty repository into a workspace that builds, tests, and measures on three operating systems, and answers the three technical questions the rest of the plan depends on. It ships no user-facing feature.

**Exit test:** each spike has a written decision record; the workspace builds and tests on Windows, macOS, and Linux in CI; benchmarks run in CI against the test application.

## Session prompt

Paste this into a coding agent opened at the repository root:

```text
Run the start-session skill. Read AGENTS.md, the newest entry in docs/journal/,
docs/plan/working-method.md, and docs/plan/anything2mcp-master-plan.md.

Take the next work package the journal names. State the plan, the hypotheses it
tests, and an alternative you considered, then build it, prove it, and merge it
when every check is green. Record every decision. Before you stop, run the
end-session skill.
```

## Task groups

The order of work is the slice order in [core first](core-first.md); the groups below list everything Phase 0 contains. Work the groups in order where the slice order does not say otherwise. Groups D, E, and F are spikes: time-boxed experiments in a `spikes/` folder that end in a decision record, not in production code.

### A. Repository scaffold

| # | Task | Done when |
| --- | --- | --- |
| A1 | Hold the package names: `.github/workflows/release.yml` and a placeholder `anything2mcp` release on PyPI through trusted publishing; then placeholders on crates.io and npm. See [publishing setup](../../plan/publishing-setup.md) | `pip install anything2mcp` installs the placeholder; each registry's row in publishing setup says held |
| A2 | Add `LICENSE` (MIT), `.gitignore`, `rust-toolchain.toml`, `rustfmt.toml`, lint settings | `cargo fmt --check` and `cargo clippy` run clean on an empty workspace |
| A3 | Create the Cargo workspace with the crates from spec section 29 as empty crates. A crate declares a dependency only when code needs it ([decision 0045](../../decisions/0045-the-workspace-starts-with-every-crate-and-the-audit-enforces-the-layering.md)) | `cargo build` and `cargo test` pass; the layering in the technical reference is enforced by `tools/docs_audit.py` |
| A4 | Add CI for Windows, macOS, and Linux: format, lint, build, test | A pull request shows three green jobs |
| A5 | Add dependency auditing and license checking to CI | A disallowed license fails the build |
| A6 | Run `tools/docs_audit.py` in CI | A documentation contradiction or broken link fails the build |
| A7 | Add `SECURITY.md` with how to report a vulnerability, `CONTRIBUTING.md`, a code of conduct, a changelog, and the release steps. A notices file for third-party licences is generated with the first real release | The files exist and the audit's link check covers them |

### B. Core types and file schemas

| # | Task | Done when |
| --- | --- | --- |
| B1 | In `a2m-core`, define the enums and identifiers: mode, effect class, failure class and codes, run and record states, binding status | Types compile, serialize, and are documented |
| B2 | Define a secret wrapper type that cannot be printed, logged, or serialized | A test proves formatting and serializing it never reveals the value |
| B3 | Write JSON Schemas for target, auth, screen, endpoint, field type, operation, pipeline, source, and test files, from the [file format reference](../../reference/file-formats.md) | The example files in spec section 7 validate; invalid examples fail with useful messages |
| B4 | In `a2m-files`, load and validate a target folder and report errors with file and line | A broken example produces an error a person can act on |

### C. Test application and benchmark harness

| # | Task | Done when |
| --- | --- | --- |
| C0 | Read the [test application plan](../../plan/test-application.md); Phase 0 delivers its cases 1 to 4, the control interface, and the first conformance groups | The plan's Phase 0 list is the scope for C1 to C3 |
| C1 | Build a small local test application with a JSON API and a web interface: a list, a form, a save with validation, optimistic locking, and a slow endpoint | It starts with one command and has its own tests |
| C2 | Add the hard cases as pages: a virtualized grid, a postback-style form, a controlled-input form, an anti-forgery token, an iframe, a shadow-DOM component | Each page has a stated expected behavior |
| C3 | Create `bench/` with a harness that measures overhead beyond the application's own response time | A sample benchmark runs in CI and stores its result |

### D. Spike: browser engine base

**Question.** Build the DevTools-protocol engine on chromiumoxide, on Rustwright's core, or on a new protocol client?

**Method.** Run research brief 1 in `docs/research/11-open-research.md`. Then, for each viable candidate, implement the same five things against the test application: launch and attach; locate an element by role and name through the accessibility tree; an event-driven wait for an element; fill a controlled input and read it back; capture a network response.

**Measure.** Lines of code needed, time to notice an element, memory of one context, behavior on a page crash, and whether it works on all three operating systems.

**Output.** A decision record choosing the base, with the measurements.

### E. Spike: MCP server

**Question.** Can `rmcp` serve one tool over stdio and over stateless Streamable HTTP, for both the 2026-07-28 and 2025-11-25 revisions, with structured output?

**Method.** Run research brief 2. Build a minimal server with one tool that returns structured content and an output schema. Connect at least two different MCP clients.

**Output.** A decision record stating which revisions will be served and how, and what client differences were seen.

### F. Spike: desktop accessibility

**Question.** Is `xa11y` mature enough to be the base of the desktop driver, or are per-platform crates needed?

**Method.** Run research brief 19. On each operating system, read the accessibility tree of a simple native application, find a text field by name, set its value, and read it back.

**Output.** A decision record choosing the approach. This spike can run last; the desktop driver is not built until Phase 8.

### G. Close out

| # | Task | Done when |
| --- | --- | --- |
| G1 | Confirm or replace each crate candidate in `docs/research/09-rust-building-blocks.md` | The table lists verified choices with versions and licenses |
| G2 | Update spec sections 17 and 29 with what the spikes decided | The spec matches the decision records |
| G3 | Revise the draft [Phase 1 task list](phase-1.md) to reflect the spikes | The revision is merged and the journal says what changed |

## What the owner has settled, and what only the owner can do

Settled on 2026-11-17 ([decision 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md)):

- Real target files live in the developer's own repository. This repository names no real application and publishes no catalog.
- No MCP client is preferred. Spike E uses whichever unrelated clients are at hand.
- Real systems are tried at the end, with the owner.

Only the owner can:

- set up each registry so that the release workflow may publish to it ([publishing setup](../../plan/publishing-setup.md));
- switch on private vulnerability reporting in the repository's settings, which [SECURITY.md](../../../SECURITY.md) relies on.

Not known: whether a Windows machine is available for local testing. Until one is, CI is the only Windows environment.
