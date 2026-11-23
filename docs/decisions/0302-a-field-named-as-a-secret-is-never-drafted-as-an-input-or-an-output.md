# 0302. A field named as a secret is never drafted as an input or an output

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** the drafting rules of `a2m-pack-http` (`draft.rs`, `openapi.rs`), one function of `a2m-files` that says whether a name is a secret's, a case of the test application, [section 303](../spec/3-packs/303-the-http-pack.md), the design of `a2m-pack-http`. The compiler's refusal of a secret in a file does not change.

## Context

Field test 1, finding 10 ([results](../reviews/field-tests.md)): a system's published description gives an answer a property named as a client's secret. The drafter mapped it as an output of the drafted read, like any plain value. The compiler refused the file with `kernel.credential_in_file`, as it must, and that one draft stopped every tool of the target.

Rule 4 says a secret never appears in a tool's input or result. The compiler already holds that line for any file: a key that names a secret may hold only a vault reference. So no tool that returns such a field can ever be compiled, whoever wrote it. What was wrong is that the drafter wrote a file the compiler was certain to refuse.

Three facts about what happens to the value itself, checked in the code: the HTTP driver keeps no part of an answer's body in a trace, only its size; a recorded fixture replaces every value of an answer by a made-up one, whether or not it is an output; and a message about an answer's shape never quotes a value. So a secret a system returns in a field that is no output goes nowhere.

## Decision

1. **The drafter never drafts as an output a property whose name the compiler takes for a secret's.** It asks the same function the compiler uses, so the two cannot disagree. The read is drafted without that output.
2. **The drafter says so.** The drafted operation's description ends with one sentence that names the properties left out because their names are a secret's, and the scout's report counts them.
3. **A parameter named as a secret is never drafted as an input.** Where the parameter is optional it is left out. Where it is required the read is not drafted by rule, and is reported the way a read with a required parameter outside the path is reported today.
4. **The compiler's refusal stays as it is.** It is the guarantee. A person who writes such an output by hand is still refused.
5. **The same rule is owed by every pack that drafts** a column, a tool's result, a field of a file, or a control by its name. The HTTP pack is first because a field test showed it; the others are roadmap package FT10.

## Why not hide the value and keep the output

A field marked secret that the engine returns masked would need the driver to register a value from an answer with the redactor at run time, which no pack can do today, and would put a secret's value in the path of every result on the strength of its name alone. Leaving the field out puts nothing in that path.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Draft the output and let the compiler refuse | The first field test: one such field stops the whole target |
| Hold the draft with an open question | There is nothing to ask. No answer makes returning a secret right |
| Loosen the compiler for a path into an answer under a secret's name | That is the line rule 4 rests on |
| Return the field masked | See above |

## What this risks

A field whose name is a secret's and whose value is not one, such as a count of secrets, is left out of a drafted tool. A person who needs it writes the output by hand under another name, with the extract path as its value, and the review shows it.

## Proof

A case of the test application answers with a property named as a secret, and takes an optional and a required parameter named as one. The drafted read holds no such output and no such input, compiles, and returns its other values; the read with the required parameter is not drafted and is reported. In field test 1 the system's whole description compiles and its reads are tried.
