# 0460. A tool's digest is of its definition as published, and the untrusted mark sits on the output's own property

**Status:** Accepted. Design decision, made by the worker who built package YL2, under the lead's brief. The owner had not read this record when it was written. It builds point 1 of [decision 0439](0439-a-tools-name-carries-no-version-and-a-breaking-change-is-named-at-publish.md) and points 3 and 5 of [decision 0325](0325-what-a-browser-can-reach-and-what-a-tools-definition-may-hold.md), and settles what they left to the builder. It changes a public contract by addition: a listed tool's `_meta` gains a key, `tool.describe` gains `digest`, and a tool's output schema gains a mark.

**Date:** 2026-11-25

**Changes:** [`a2m-files`](../architecture/crates/a2m-files.md), which computes the digest (`tool_digest`) and writes the mark (`UNTRUSTED_MARK`) into the output schema it compiles; [`a2m-actions`](../architecture/crates/a2m-actions.md), whose `tool.describe` and full `tool.search` give `digest`; [`a2m-mcp`](../architecture/crates/a2m-mcp.md), whose tool list carries it as `anything2mcp/digest`; [section 221](../spec/2-kernel/221-the-tool-surface-agents-see.md).

## Context

Decisions 0325 and 0439 say a listed tool carries the SHA-256 of its name, description, input schema, output schema, and annotations in canonical JSON, and that each output field holding the system's words carries `x-anything2mcp-untrusted: true`. They do not say how the five parts are put together, under which names the annotations go, whether the digest is of the tool as compiled or as a door shows it, where in a field's schema the mark goes, or whether `version.diff` should compare tools by the digest.

## Decision

1. **The digest is of one canonical JSON object,** `{"annotations": {...}, "description": ..., "input_schema": ..., "name": ..., "output_schema": ...}`, the annotations under the compiled target's names (`read_only`, `destructive`, `idempotent`, `open_world`), which are the names `tool.describe` shows, encoded as every compiled target is (keys sorted, no spaces, whole numbers only). A client that has `tool.describe`'s answer can compute it again.
2. **It is of the tool as published,** read from the compiled target, so `tool.describe`, a full `tool.search`, and the MCP list give one value. The sentence a door puts first in the description of a tool a drift stopped is the product's, of the moment, and not in it; the title, the target, and the version are not in it either. The same files published under another version keep the digest.
3. **The mark sits on the output's own property,** at its top, beside `anyOf` where the output may be absent: `{"anyOf": [...], "x-anything2mcp-untrusted": true}`. Only `true` is written; an output not marked carries no key. A pipeline's result declares no fields and carries none.
4. **`version.diff` names changes field by field, and its catch-all is the digest.** A digest says only that something changed; the closed list of decision 0439 needs to know what. So the comparison stays field by field, and "anything else a client reads" (`definition_changed`) is now a changed digest or a changed title. A tool `version.diff` calls unchanged therefore keeps its digest.

## What was rejected

- **The digest of what the MCP list shows,** with the annotations as `readOnlyHint` and the rest. It would differ between a drift-stopped listing and a normal one, and between the MCP door and `tool.describe`, so a client pinning it would be told of changes that are not changes.
- **The title in the digest.** Decisions 0325 and 0439 name the five parts, and the title is shown to people, not read by an agent to choose or call a tool. `version.diff` still names a changed title.
- **The mark on each arm of the `anyOf`, or on a list's items.** The mark is about the output field, which is what a client reads by name; one place is enough.
- **`version.diff` by the digest alone.** It would class every change as one unnamed change.

## Consequences

- `crates/anything2mcp/tests/tool_digest.rs` holds it through the product: an MCP client's `tools/list` carries the digest in the tool's `_meta`, a `tools/call` of `a2m_tool_describe` gives the same value and the output schema with the mark on the untrusted output and none on the other; publishing the same files again keeps the digest, a changed description changes it, and `diff` says so.
- `a2m-files`' tests hold the digest of the `example` target's order read as one value on the three systems, the same under another version number and another under a changed description.

## What would reopen it

A client that pins tools and needs the title in the digest; or one that reads the mark inside an `anyOf` arm and not beside it.
