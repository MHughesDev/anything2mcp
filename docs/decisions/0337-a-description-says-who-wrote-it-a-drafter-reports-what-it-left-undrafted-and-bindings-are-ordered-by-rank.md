# 0337. A description says who wrote it, a drafter reports what it left undrafted, and bindings are ordered by rank

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Builds the compiler's side of roadmap packages V5, FT11, and 4.6; each package's proof still waits on `a2m-actions` or the provider registry, as the points say.

**Date:** 2026-11-23

**Changes:** `a2m-core` (the plan's tool), `a2m-files` (the compiler, the drafter, the compiler set), `a2m-pack-http` (its drafting rule), [file formats](../reference/file-formats.md), [section 206](../spec/2-kernel/206-the-compiler-and-the-plan.md), the designs of `a2m-files` and `a2m-pack-http`. Nothing is decided here about the engine's walk of the plan.

## Context

Three small things were left open by earlier records, each of which named the design and not the form.

[Decision 0307](0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md), point 6, says a description is at most 1,024 characters of plain text and that a client is told whether a rule or a builder wrote it. A drafted file kept the fact only as a comment, `# drafted_from:`, which no reader of the plan can see.

[Decision 0302](0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md), points 2 and 3, says the scout reports the read that is not drafted and counts the properties left out. A drafting rule returned no document and said nothing, so the scout could not tell a read left to a person from a finding the rule did not read.

[Section 201](../spec/2-kernel/201-archetypes-and-modes.md) says that where an operation states no order of its bindings, the compiler sets it from each mode's `rank` in its provider's manifest. Nothing said how an operation states one, and nothing set the order.

## Decision

1. **The plan's tool says who wrote its description.** `ToolPlan` gains `description_by`, a `DescriptionBy`: `rule` or `builder`. It is absent from a plan's bytes when it is `builder`, and a plan without it reads as `builder`, so no existing plan changes its bytes or its hash. An operation or a pipeline file may state `description_by`; the drafter writes `description_by: rule` after the description of each operation and pipeline it drafts, and the compiler copies the file's word into the plan. A person who rewrites a drafted description removes the line; the compiler does not judge whether a description was rewritten. The MCP door sends it as `anything2mcp/description_by` (a2m-mcp's part).
2. **A description is bounded text, and the compiler refuses what is not.** More than 1,024 characters, counted as characters of the trimmed text, or any control character (a line end and a tab are control characters), is `kernel.description_unbounded`, an error. `kernel.description_weak` keeps only what it always said first, a description of fewer than 20 characters, and stays a warning.
3. **A drafting rule can say why it drafted nothing, and how many it left out for a secret's name.** `DraftContext::not_drafted(reason)` and `DraftContext::left_out_as_secret(count)` are the rule's two calls. `DraftReport` gains `not_drafted`, a list of the findings a rule covered and drafted nothing from, each with its pack and the reason (empty where the rule gave none), and `left_out_as_secret`, the sum of the counts. A reason is the pack's own sentence and quotes nothing the system said. The HTTP rule gives a reason for every safe read it leaves out, and counts a property for each secret-named property left out of a drafted read, and for each one in a read whose every plain property was such. A GET the description does not mark safe, and any other method, are not reads and are not listed. The two calls are additive: a rule that makes neither is reported with an empty reason and a count of none.
4. **Bindings are ordered by rank, unless the operation says `binding_order: listed`.** The compiler sorts an operation's bindings by the rank of each binding's mode, lower first, and by place in the file among bindings of equal rank. `binding_order: listed` keeps the file's order. A mode's rank reaches the compiler through `CompilerSetBuilder::mode_rank(pack, mode, rank)`, which the provider registry calls from each manifest's modes; a mode given none has the manifest's own default, 100, so a set that is given no rank orders every operation as the file does and no plan changes. A binding's `position` in a test file counts places in the operation's file, whatever order the plan has, and a test keeps the plan's place of the binding it names.

## Why rank, and not a promotion that moves the binding

The row of package 4.6 offered two ways to make a promoted binding the first one tried: the compiler orders by rank, or `binding.promote` rewrites the file to put the binding first. Rank was chosen, for these reasons.

- It needs no edit of a person's file. A rewrite by an action puts a machine's change into a file a person reviews and signs, and makes the action know the shape of every pack's bindings.
- It is the design already: section 201 and section 225 say the cheapest verified mode is tried first and the order is data from the manifest. The rewrite would have been a second mechanism for the same end.
- It is right for every new binding and not only a promoted one: a drafted `http` binding added beside a `ui` one is tried first once it is verified, and a `draft` or `shadow` binding sorted first costs nothing, because the engine walks past a binding that is not verified.

What it risks: a rank is of a mode, not of a measured speed, so two bindings of one mode keep the file's order, and a person who wants a slower binding first states `binding_order: listed`. Section 225's "fastest verified mode first" is satisfied only as far as the manifests' ranks are true.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep who wrote a description in a comment | No reader of the plan can see a comment |
| A `description_by` that is always in the plan | Every existing plan would change its bytes and its hash for nothing |
| Judge in the compiler whether a drafted description was rewritten | It cannot be done by rule: any edit might be a fix or a rewrite |
| A rule returns a result that carries its reason | The rule's signature is a public contract; two calls on the context add to it and change no implementer |
| Count the properties by their names | Names are the system's words; a count shows the size of what was left out and quotes none of it |
| `binding.promote` puts the promoted binding first in the file | See above |
| Order by the ledger's measured time | The ledger holds no measured time of a mode, and the order must be reviewable in the files |

## What is not built, and who builds it

- **V5:** `a2m-actions` (`described()` in `handlers/tools.rs`) outputs `description_by`, and `a2m-mcp` (`entry()`) adds `anything2mcp/description_by` to the tool's `_meta`.
- **FT11:** the scout's report in `a2m-actions` shows `not_drafted` (each with the finding's description and its reason) and `left_out_as_secret` in its draft stage, and its summary names the count.
- **4.6:** the provider registry calls `CompilerSetBuilder::mode_rank` for each mode of each enabled manifest; `a2m-actions` takes a binding's position from the file where it reads or rewrites a file, and from the plan where it reads the plan (they differ after ordering); a live read after a promotion is then proven end to end.

## What would reopen it

A second use for a description's author, such as a policy that refuses a rule-written description a write tool, would need the field to be a closed type in more places. A measured speed in the ledger, kept by mode, would let the order be set by what happened and not what a manifest says.
