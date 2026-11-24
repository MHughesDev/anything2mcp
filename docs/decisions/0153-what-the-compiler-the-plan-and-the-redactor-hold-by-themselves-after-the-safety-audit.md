# 0153. What the compiler, the plan, and the redactor hold by themselves, after the safety audit

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the 22 findings of a safety audit of `a2m-core`, `a2m-files`, `a2m-expr`, and `a2m-sources`. Adds to the registration traits, which are a public contract, by methods with defaults; changes no signature and not the plan's shape. Supersedes one line each of [decision 0073](0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md) (a currency symbol) and [decision 0124](0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md) (a program on a network share).

## Context

A reviewer read the four crates against their documents and the ten rules and ran nothing. Every finding was checked against the code, and all 22 were confirmed. Each has a test. For fourteen of them the test was run with the fix taken out and failed: the two on writes, the plan's check, the numeric secret, the Scope rule, the two on the drafter, the four in the expression library, and two of the three in the source readers. The other eight were confirmed by reading the code as it stood, and their tests were written with the fix. Every other crate is built on these four and was being changed at the same time, so a fix could keep every signature and the plan's bytes, and could tighten a compile rule only where no fixture in the repository would then fail.

## Decision

**A write, as the compiler holds it whatever a pack says (F1, F2).** The compiler's only evidence that a step saves was the `commits` flag a pack's lowering set, which the test pack set from a target file's own word.

- An operation whose effect is `write` or `irreversible` declares at least one commit point, and each binding has exactly that many committing steps. An operation whose effect is `read` declares none.
- A lowering states, once for each kind of step, whether a step of that kind saves: `StepCommits::Never`, `Always`, or `AsDeclared`. A lowered step marked otherwise is `lowering_invalid`.
- A lowering states the least effect a binding has by what it does, `Lowering::least_effect`. An operation that declares less is `effect_lower_than_derived`. This is how `irreversible` is derived.
- Both methods have defaults, `AsDeclared` and `read`, under which nothing changes for a pack that has not answered. *Instead of:* a required method, which would stop every pack from compiling tonight; and *instead of:* "exactly one committing step", which section 208 contradicts: an operation that saves on two tabs has two rows.
- *What it costs:* until a pack answers, the compiler has only that pack's own flag. Every pack is owed both answers.

**The plan's own check holds the same rules (C2).** `Plan::check`, which `encode` and `decode` both call, refuses a plan that breaks a rule of a write; the list is in the `a2m-core` design. *Instead of:* a second check in the engine, which every other reader of a plan would have to repeat. *Left out:* that a probe is a read, which the sample plan under the pinned hash breaks, and the pipeline rules that are lints.

**A secret that is a number (C1).** `Redact::redact_value` judges a whole number, a decimal, a date, a moment, and a length of time by the text each is written as, and replaces a match by the text `[redacted]`. *Costs:* a short numeric secret removes every number whose digits hold it; the value's kind becomes text; one search per number. *Not found:* a secret whose leading or trailing zeros the number lost. *Instead of:* leaving numbers alone, which rule 4 forbids; and *instead of:* redacting digits inside the number, which would hand a caller a different number. Two names that redact alike keep both values, the later with a number after it.

**Where the drafter writes (F4).** Only to a regular file inside the target folder: each folder on the way is looked at without following a link, a new file is made with create-new, and a link in the way puts the draft in `refused`. An unchanged draft of the drafter's own is still rewritten, as the design promised, by renaming a new file over it. *Instead of:* never touching an existing name, which would make a second scout of the same system report every draft it wrote as a conflict.

**What the drafter writes (F5).** No write in any file kind. `FileKind::writes` says whether a document of a pack's kind describes a write, with a default of no. A rule may draft `operation`, `source`, or a kind its own pack registered. *Instead of:* compiling the folder with the drafts and refusing by what the plan holds, which cannot be done for a draft with an open question, since it does not compile.

**Scope (F3).** Built as far as a file has a place for it: each destination is one exact address, each upload and download one name, each once, as `kernel.scope_invalid`. Not built, and said so in section 206: checking a destination against origins a person registered, which no file holds. *Instead of:* refusing `permissions` by name until then, which would stop a pack's fixture that declares a plain host name.

**Choices, dates, money, and case (E1 to E4).** A code list in which a text stands for two choices does not compile and does not load. A date format in which digits follow a part of one or two digits does not compile. A currency symbol is read only for a currency it is written for. Text compared without case is compared by the lower case of its upper case. Characters that are not seen are compared as written: a mismatch stops, and a match is never made of one.

**The source readers (S1 to S3).** An exponent of any size is out of range or zero. A workbook tag whose attributes break the format, a flag that is neither true nor false, a closing tag that closes nothing open, and a style the workbook lacks are `source_malformed`. A difference from a template quotes the template and never the input.

**The minor findings in the compiler (F6 to F12, C3).** An open question is refused at any depth inside a field that takes any value. The credential lint knows more names for a secret as errors, and `token`, `cookie`, `authorization`, `pin`, and `bearer` as a warning under `kernel.credential_suspected`, since such a key holds the name of a secret as often as the secret; no diagnostic quotes a value the lint knows. A link to a folder is refused and a file has a size limit. A lowering names a surface of a kind its own pack registered. A lone carriage return is a line ending. A declared program's path holds no `.` or `..`, and a program on a network share is declared with its digest.

**Not closed, and stated in the design (F9).** A draft compiled under a release version has the published plan's bytes. Refusing it would break the test helpers of nine crates, and a warning would break one that asserts its fixtures give no other warning. The limit is in the design's "Not built" list with a test that pins it.

## Consequences

- Every pack's lowering is owed `commits` and `least_effect`, and every file kind that can describe a write is owed `writes`. Until then those packs are held as before.
- A result's schema must allow text where a number may be redacted.
- A message about a changed template no longer shows what the input holds in the changed place.
- The engine pools every operation's declared destinations into one list for a run. That is the engine's to narrow.

## Revisit if

A pack needs a kind of step that both reads and saves in one step; a target needs destinations registered apart from its operations; a real system writes money with a symbol the table lacks; or numeric redaction removes numbers a caller needs.

## Spec sections

- [205. Target files](../spec/2-kernel/205-target-files.md)
- [206. The compiler and the plan](../spec/2-kernel/206-the-compiler-and-the-compiled-target.md)
- [210. Sources and value normalization](../spec/2-kernel/210-sources-and-value-normalization.md)
