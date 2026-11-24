# 0399. The command pack drafts an operation only once a person has declared the command's effect

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md) by the lead. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the drafting rules of the command pack (a second rule, from `command` findings to `operations/` files, and the first rule now leaves a command a person declared alone); section 306 and 402 sentences; the E4 and 4.4 rows of the roadmap.

## Context

Package E4's proof is "a draft made by rule compiles, names its findings, and carries no unit or key the system did not state". Every pack that observes a system's data or its own interface drafts reads that meet it. The command pack drafts a `commands/` file for each command it finds, and that file fails to compile only on open questions: the effect, the exit statuses, the output form, and the read-back. It drafts no operation. [Spec section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md) gives the reason: help text does not say which commands read and which write, so no operation is drafted. Section 306 adds that a name such as `show` proves nothing.

## Decision

1. **No rule infers an effect.** A rule that drafted a read over a command from its help or its name would state an effect the program never stated; rule 7 and rule 3 forbid it.
2. **A person's declaration is the source.** The command pack drafts one `read` operation for each command whose entry in a `commands/` file a person wrote with `effect: read` and a declared output, named `{target}_{command}`, bound `mode: cli` to that entry, with the entry's params as inputs and its declared output fields as outputs (a JSON field as its declared base type, text as `untrusted_text`; a table column and a pattern group as `untrusted_text`). The verb is `list` for rows (a table, JSON lines, a pattern matched on every line) and `read` for one record. Nothing is drafted, and the report says why, for a command nobody declared, one declared a write or irreversible, one that takes the record's stamp, one whose output follows a path into a document, and one whose output or param type the rule does not read. A command a person declared is found by its entry name or by an argument list that begins with the listed words; the commands rule leaves it alone, so the person's file is never drafted over or twice. An operation already bound to the command is the person's. Proven in `a_command_a_person_declared_a_read_is_drafted_as_an_operation_that_reads_the_stand`: the folder compiles, the drafted reads return what the stand holds, the stand shows no write.
3. **Alternative rejected.** Inferring `read` from a verb in a command's name, or from a PowerShell `Get-` verb. A name states a convention, not a guarantee; a program may write on any verb.

## Inferred

That the owner's intent for E4 is the proof as written, not a change to section 402. If the owner means the command pack to draft by name, this record is wrong and should be answered.
