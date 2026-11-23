# 0297. A property a description does not require is drafted as possibly null

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** the drafting of an endpoint's `response_schema` (an answer's shape only, never a request body or a parameter) from a published description in `a2m-pack-http` (`openapi.rs`), a new case of the test application, [section 303](../spec/3-packs/303-the-http-pack.md), the design of `a2m-pack-http`. The check of an answer against a shape does not change.

## Context

Field test 1, finding 7 ([results](../reviews/field-tests.md)): two reads of three stopped as `drift`, `response_schema_mismatch`, because the system answered `null` for an object that its description gives as a property and does not list under `required`. The description does not mark the property `nullable`. Descriptions seldom do: the older form of description has no word for it, and a great many systems write "this is absent" as `null`.

The shape an endpoint file holds already tells the two apart. A property not listed under `required` may be missing from an answer; only a property marked `nullable` may be `null`. An output read through a property that may be missing or null is already marked as possibly absent. So the check is able to say what is wanted. The fault is in what the drafter writes down.

## Decision

1. **When an endpoint's shape is drafted from a description, a property the description does not list under `required` is written with `nullable: true`.** This holds for a property of any kind, at any depth.
2. **A property the description lists under `required` is drafted as it is today:** `nullable` only where the description says so.
3. **The check of an answer does not change.** A `null` where the endpoint file does not say `nullable` stops the read. A value of another kind than the one declared stops the read, marked `nullable` or not.
4. **An endpoint file a person wrote is not touched.** The rule is a drafting rule.
5. **An output that reads through such a property is returned as absent when the property is null or missing.** The tool's output schema does not yet say beforehand that it may be: the shape knows which paths are uncertain, and nothing after it carries that into the plan's outputs. That is a gap this decision leaves open, as roadmap package FT7. The first draft of this record said the declaration existed; building it showed that it does not.

## Why this keeps rule 7

Rule 7 stops a run on a response shape that has no rule. A description that gives a property and does not require it has said that the property may be absent; `null` is how many systems write that. After this decision the endpoint file says so in a word a person can read and change, so the answer has a rule. What still has no rule still stops: an undeclared kind, and a `null` under a property the description requires.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Make the check lenient: accept `null` for any property not required | The endpoint file would then say one thing and the check do another; knowledge of the shape would be in two places (rule 8) |
| A setting on the target that turns leniency on | The same split, and one more thing a person must find |
| Keep stopping, and offer a command that marks one property | A description of several hundred operations has thousands of such properties; a person would mark them all, without looking |
| Draft every property as `nullable` | A required property answered as `null` is a real difference from the description, and worth a stop |

## What this risks

A system that means different things by a missing property and a `null` one. A read then returns the output as absent in both cases. No write is affected: a write's read-back compares the value read with the value meant, and an absent value does not equal one.

## Proof

A case of the test application answers an optional object as `null`, as missing, and as a text. A read drafted from the application's description returns in the first two cases and stops in the third. In field test 1 the two reads return the values the software's own database holds.
