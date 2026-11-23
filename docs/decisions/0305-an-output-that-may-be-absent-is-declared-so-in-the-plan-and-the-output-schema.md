# 0305. An output that may be absent is declared so, in the plan and in the output schema

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Completes point 5 of [decision 0297](0297-a-property-a-description-does-not-require-is-drafted-as-possibly-null.md).

**Date:** 2026-11-23

**Changes:** the output of a plan in `a2m-core`, `output_schema_of` in `a2m-files`, the lowering of `a2m-pack-http`, the file formats, roadmap package FT7. A change to the plan's form.

## Context

Since decision 0297 a read returns when a property its description does not require is absent. The endpoint's shape already knows which paths into an answer are certain (`at_path` returns it). The tool's output schema does not: a record lists its properties with no `required`, so a client cannot tell an output that is always there from one that may not be.

## Decision

1. **Each output of a plan says whether it may be absent.** The pack's lowering sets it; the kernel does not work it out. In the HTTP pack an output may be absent exactly when its extract path is not certain in the endpoint's shape.
2. **An output that is absent is returned as `null` under its own name.** The key is always in the result. A caller can then tell "the system has no value here" from "the tool has no such output".
3. **The output schema says both.** A record's `required` lists every output. The type of an output that may be absent admits `null` beside its own type; the type of one that may not does not.
4. **A `null` in an output that may not be absent stops the run** with the pack's existing shape failure. Unknown means stop.
5. **A person may declare it by hand**, with `may_be_absent: true` on an output of an operation, where a pack has no shape to read it from. A pack that knows the path is certain refuses the declaration as untrue only where it would hide a required value; elsewhere the declaration stands.
6. **The other packs default to "may not be absent"** and change one at a time as a field test shows the need.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Leave the key out of the result when the value is absent | Two results of one tool would have different keys; a client's check of the shape then says nothing |
| Mark every output as possibly absent | The schema would promise nothing, and a caller would have to guard every value |

## Proof

A drafted tool of the test application whose output reads through a property that may be null: its schema admits `null` for that output and for no other; a result with the value absent validates against the schema with a JSON Schema validator in the test; a `null` in a certain output stops.
