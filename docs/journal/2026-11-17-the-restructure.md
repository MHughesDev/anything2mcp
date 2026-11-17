# 2026-11-17: the restructure, configuration, and the bar

## State at the end

The repository now reads as one design for Anything2MCP. The specification is 59 sections in seven parts, each stating the design as it stands; the rule that later sections win is gone, because no two sections cover the same ground. The first design's consolidated sections and its plans are in `docs/archive/`, with the key from old numbers to new. There is one plan, the roadmap. Configuration is designed in full, and "the best of its kind" is written as twelve things that can be measured. Findings, the first piece of discovery, are built in `a2m-core`.

## Done

- **The specification rebuilt** ([decision 0065](../decisions/0065-the-specification-is-rebuilt-in-parts-and-the-first-design-is-archived.md)). Thirty-seven sections moved and renumbered by script. Fourteen consolidated from two or three sections each, and eight written fresh, by seven agents in parallel from one brief: the summary, the overview, the concepts, the bar, configuration, and sections for the HTTP, MCP, and desktop packs. One further agent brought twenty-five sections that were to move up to date in place first, applying the corrections the last entry listed.
- **Links and numbers repaired by script** in every living document. Decision records and journal entries keep the numbers of their time; their links were re-aimed at the moved section, or at the archived original where a section was consolidated.
- **The audit knows the new shape:** a section sits in the part its number names; nothing else sits in the specification's folder; a living document that cites an old section number fails. Its self-test has 30 cases.
- **One plan.** The build plan, the phase map, the phase lists, the slice order, the long-term plan, the first readiness report, and the settings examples are archived. What was still true of them is in the roadmap, the working method, and sections 226 and 704.
- **Configuration** ([section 226](../spec/2-kernel/226-configuration.md), [decision 0066](../decisions/0066-configuration-in-layers-that-may-only-tighten-and-a-bar-that-is-measured.md)): eight layers, of which the upper four may only tighten; twelve scopes; presets; a group for every pack; seventeen guardrails with no key at all.
- **The bar** ([section 107](../spec/1-product/107-the-bar.md)): twelve dimensions, each with a test and the package that measures it, and a scorecard.
- **Roadmap package B2:** `Digest`, `Finding` with its builder and its level rules, `Subject`, `Source`, `LimitationCode`. 77 tests pass in the workspace. One new dependency, `sha2`, verified and recorded.

## Learned

- **Most of the old specification stood on its own.** Thirty-seven of 72 sections could move unchanged in substance. The contradictions lived in fourteen places where a later section had refined an earlier one.
- **A consolidation is safest when its tests come with it.** The test in `a2m-core` that reads the documents passed against the consolidated sections on the first run, because each consolidating agent was told which headings and tables that test reads.
- **Moving a file is cheap; a number in prose is not.** Links were mechanical. "Section 42" in a sentence needed a map, and ranges such as "sections 39 to 52" had to be rewritten by hand. Three-digit numbers that begin with the part's digit cannot be confused with the old ones, which is what lets the audit refuse an old number.
- **More configuration needs a direction.** The rule that the layers nearest a project may only tighten is what lets the product be highly configurable without a checked-out project file being able to lower an installation's guard.

## Changed in the design

- Decisions 0065 and 0066.
- The provider manifest gains a settings schema and a list of the kinds it reads from other packs.
- The roadmap gains package 1.4a, the settings loader, and the rule that every milestone ends by updating the scorecard.
- The technical reference lost the widget drivers and the platform profile schema to the web pack's section, and the undefined admin routes.
- The glossary gained guardrail, preset, and part, and seven definitions no longer say "application" where any system is meant.

## Noticed, not acted on

- **Eight sections are still shaped by the first product,** though labelled: the driver table in 207, the gates in 403, principles 1 and 7 in 103, the budget table in 225, the defaults in 211, and the one-machine sign-in in 503. Each is true of the web pack and silent about the others.
- **Fifteen small designs exist only as rows in section 604,** each with the section it belongs in: target environments, mid-run input, undo, the read cache, and others. Moving each to its section is a design review round of its own.
- **The action catalog still lacks** actions for starting a project, exporting a toolset's description, managing webhooks, and listing open subsystems, and its build toolset is over its own cap. Section 220 states both as open points.
- **The diagram `docs/assets/system-overview.png`** shows the first product and is now used only by an archived section.
- **A consolidation can lose something.** Each agent reported what it dropped and why, and nothing reported looked like a loss. Nobody has yet read all 59 sections end to end against the archive. A design review round with that as its lens is the check.
- The command pack's section and the configuration section are each longer than asked.

## Decided without the owner having read it

Records 0065 and 0066, and the choices in sections 107 and 226.

## For the owner

Nothing new. Still: the PyPI publisher and the release button; a first publish on crates.io and npm; private vulnerability reporting.

## Next

The packages the roadmap marks ready, on separate branches: B2a (the plan's types and the checked names, in `a2m-core`), then B3 (the manifest and the registry) and B4 (the kernel's file schemas and the loader); 0.4 and 0.5, the two spikes; and D2, the command-line test stand. Before any of them, one design review round reading the new specification end to end against the archive.
