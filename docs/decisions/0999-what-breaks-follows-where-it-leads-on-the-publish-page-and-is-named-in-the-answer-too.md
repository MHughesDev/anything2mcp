# 0999. What breaks follows where it leads on the publish page, and is named in the answer too

**Status:** Accepted. Design decision, made by the worker who built package YL1, under the lead's brief. The owner had not read this record when it was written. It builds points 2 and 3 of [decision 0439](0439-a-tools-name-carries-no-version-and-a-breaking-change-is-named-at-publish.md) and settles what those points left to the builder. It changes a public contract by addition: the review summary gains a field, the answer of a person's act that waits gains two optional fields, and `version.diff` gains its handler and its output.

**Date:** 2026-11-25

**Changes:** [`a2m-files`](../architecture/crates/a2m-files.md), whose new `changes` module classifies and whose `ReviewSummary` gains `changes`; [`a2m-actions`](../architecture/crates/a2m-actions.md), which compares a publish with the version in force and builds `version.diff`; [`a2m-local`](../architecture/crates/a2m-local.md), whose publish page shows the breaks right after where each surface leads. It keeps the order of [decision 0418](0418-the-publish-page-lists-the-surfaces-first-and-marks-each-new-address-where-it-lists-it.md) whole and adds one section to it.

## Context

Decision 0439 gives the closed list of what breaks a caller and says that the review summary lists each breaking tool first, before the sign-ins, and that the publish is not refused. It does not say what a change is as a type, how a tool that changes in several ways is shown, which tools `version.diff` names, where on the page the list goes now that decision 0418 put the surfaces first, or whether the agent that asked for the publish is told.

## Decision

1. **A change is typed, one variant for each entry of the closed list and one for each compatible change.** The type `Change` of `a2m-files` has eleven breaking variants (`tool_gone`, `input_gone`, `input_required`, `input_type_changed`, `input_words_narrowed`, `output_gone`, `output_may_be_absent`, `output_type_changed`, `effect_rose`, `hazard_changed`, `key_changed`) and the compatible ones (`tool_added`, `input_added`, `input_loosened`, `input_words_widened`, `output_added`, `output_always_present`, `output_words_changed`, `effect_lowered`, `hazard_removed`, `description_changed`, and `definition_changed` for anything else a client reads, such as the title). Each carries the names, words, types, classes, and hazards it is about, so a page and a client build their own sentences. A tool's class is `breaking` when any of its changes is, `compatible` when it has others, and `unchanged` when it has none.
2. **What the list's words mean, read from the compiled target.** An input "must be given" when it is required and has no default; a new required input with a default is compatible. "Words" are a choice's codes; a free value newly limited to words narrows. A type is the value type a caller sends or reads (`text`, `int`, `choice`, `list of text`); a change of field type with the same value type is not a type change. A hazard that becomes `none` is compatible; any other change of hazard breaks. The key is the list of the identity's inputs by name, in order. A pipeline's result declares no fields, so a pipeline is compared by its inputs, effect, and hazard.
3. **`version.diff` names every tool of either version,** the breaking first, then the compatible, then the unchanged, each group by name; within a tool, the breaking changes first. It takes two published versions; a version not published is `input`/`not_found`.
4. **The review compares with the version in force.** The registry, which knows what is published, fills `ReviewSummary::changes` when a publish is asked for; the compiler leaves it empty. A first version has none. Every text in it passes the redactor with the rest of the summary.
5. **On the page, "What breaks" comes right after "Where it leads",** before the sign-ins, the reads a person asserted, and the tools: each breaking tool, and under it each change in the page's own words. Where a draft can reach stays first: it is the page's safety question (spec section 502, rule 2), and safety comes before correctness, which is what a break is about. A compatible change is shown inside its tool's item, among the rest. A version that breaks nothing has no such section.
6. **The agent is told too.** The answer of a waiting publish carries `compared_with`, the version in force, and `breaking`, the breaking tools as `version.diff` gives them (empty when nothing breaks), and one warning naming them. The publish still waits for the person, who decides.

## What was rejected

- **The list before "Where it leads", first of all.** A break is certain harm to every agent that calls the tool as it was, and read first it would not be passed over. But a new address is the page's safety question, and the product puts safety before correctness: the surfaces and their marks stay first, as decision 0418 put them, and the breaks come next, still before the sign-ins as decision 0439 asks.
- **The compatible changes in a section of their own.** They change nothing a caller depends on; a list of them beside the breaks would hide the breaks.
- **A sentence for each change made in `a2m-files`.** The summary holds no sentence built from a target's words (decision 0252); the page writes its own.
- **Telling the agent nothing.** The agent would ask the person to publish and learn of the break from its next failed call, which decision 0439 set out to end.

## Consequences

- `crates/anything2mcp/tests/breaking_publish.rs` holds it through the product: version 1.0.0 is published; version 2.0.0 makes one break of each kind and one compatible change; the page names the five breaking tools right after where it leads and before the tools, with why, the answer names the same, the person presses and it is published, and `diff` names the same five and the compatible one. Version 3.0.0 changes a description alone and shows no break.
- `a2m-files`' tests make each variant, both ways of each kind the record names two ways, and the order.

## What would reopen it

A page where a long list of surfaces hides the breaks below it; or a client that reads `breaking` and refuses to wait for the person, which would ask for the answer to say less.
