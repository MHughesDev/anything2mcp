# 0396. A tool stopped by a drift is listed and marked, never left out

**Status:** Accepted. Design decision, made by the design architect. The owner had not read this record when it was merged. Nothing in it is built.

**Date:** 2026-11-24

**Changes:** how package 9.1a is built; what `tool.search`, `tool.describe`, and the MCP door's tool list say of a tool; sections [217](../spec/2-kernel/217-traces-evidence-and-repair.md) and [222](../spec/2-kernel/222-front-doors-and-the-local-engine.md) when it is built.

## Context

[Decision 0380](0380-a-drift-marks-the-knowledge-it-was-seen-in-and-stops-every-binding-that-relies-on-it.md) stops every binding that relies on drifted knowledge, and says what it left: a stopped tool is refused when called and looks like any other until then. [Decision 0390](0390-the-open-questions-of-2026-11-24-are-settled-by-the-lead-and-the-first-release-is-what-progress-counts-toward.md), point 8, asks that it be shown as unavailable where tools are listed. The MCP tool list has no field for that, so there are two designs: leave the tool out, or list it with a mark.

## Decision

1. **A tool is unavailable when the engine would refuse it now for drift:** every binding of its operation is covered by a mark that applies to the published version (`drift_covers`, decision 0380). A read with a second binding that no mark covers is available. A pipeline is unavailable when a step's operation is.
2. **It stays in every list, marked.** A tool that vanishes tells an agent nothing, and an agent that planned around it cannot learn why. `tool.search` and `tool.describe` give each tool `available`, and for an unavailable one `unavailable`: the reason `drift`, the name of the knowledge marked, as the mark holds it, and when the mark was made.
3. **At the MCP door** the tool is listed with its schema unchanged. Its description begins with one fixed sentence of the product's own: "Unavailable: the system changed and this tool was stopped until its target is repaired." Its `_meta` carries `anything2mcp/unavailable` with the same three facts. No word of the system's is in either.
4. **A call answers as today:** `drift`, `binding_drifted`, before anything is sent.
5. **When the mark is lifted,** by a canary that passes or by a published version whose knowledge differs, the next listing shows the tool available. The MCP service keeps no session ([section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md)), so no notice of a changed list is sent; a client learns by listing again.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Leave the tool out of the list | The agent sees a tool gone with no reason, and a person asking it what happened gets a guess |
| Mark it only in `_meta` | Few clients show a model the `_meta` of a tool; the description is what a model reads |
| Send a notice that the list changed | It needs a session at the door, which the door does not keep |

## Proof

Package 9.1a's row: through the product, against the test application with a screen changed on purpose, a listing over MCP and over the command line shows the stopped write marked, with the screen's name, and a read through another screen unmarked; the call is refused as `binding_drifted`; after a repaired version is published both listings show it available.

## Revisit if

The MCP protocol gains a field for a tool that cannot be called now.
