# 0399. A document's cell may be empty, unless it is a key or declared `required`

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Carries on [decision 0315](0315-every-declared-output-is-in-every-result-on-every-path.md) for the file pack, and supersedes the sentence of its point 6 that left a document's cells as values that may not be absent.

**Date:** 2026-11-24

**Changes:** `FieldSpec` (new `required`), `RecordSpec::may_be_empty`, the `read` steps' payloads (`StepSpec::One` and `StepSpec::List` carry `may_be_empty`), and `absent_outputs` in `a2m-pack-file`; the field schema of a document (`required: true`); section 308; the design of [`a2m-pack-file`](../architecture/crates/a2m-pack-file.md). **Leaves alone:** the engine's rule that a `null` stops the run in an output that may not be absent, the folder's outputs, and a growing file's.

## Context

Decision 0315 made the engine stop with `output_unexpected` when an output not declared `may_be_absent` has no value. It left a document's cells in the file pack as values that may not be absent. An empty cell of delimited text or a workbook reads as no value, and an empty cell is ordinary in a spreadsheet a person keeps: a note column that is filled for some rows. So one empty cell made every read of that row fail, and the product's own read of the stock file through the file pack stopped. A JSON or settings file may also lack a key, or hold `null` there.

## Alternatives

| | |
| --- | --- |
| **(a) Every field of a document may be absent, except a key field and one the document declares `required: true`** | Chosen |
| (b) Keep the cells certain, and have each operation declare `may_be_absent: true` | Rejected. The fact is the file's, not the operation's: every operation over the column would have to say it, and one that forgets fails on the first empty cell of a real file. Decision 0315 point 6 itself says a pack that cannot tell which values are always there flags every output but those it knows are |
| (c) Read an empty cell as empty text | Rejected. It is not the value of a number, a date, or a decimal, and it would make "no value" and "the empty text" one thing, which a read-back cannot then tell apart |
| (d) Every field may be absent, the key included | Rejected. A key cell is how a row is found, and a row read by its key holds it; a row with no key in a list is a row the declaration does not describe, and unknown means stop |

## Decision

1. **A field of a document may be absent unless it is part of its record's key or is declared `required: true`.** This holds for every format: a cell of delimited text, a workbook, a sheet, or a table, and a value of a JSON or settings file at its path.
2. **`required: true` is the file's own statement** that the place always holds a value. A field so declared reads as a value that may not be absent, and an empty one stops the run with `output_unexpected`, as decision 0315 says. A key field is required without saying so.
3. **The names travel in the payload of the `read` steps** (`may_be_empty` on `one` and `list`), written by the lowering from the record's declaration, and `absent_outputs` reports them from there, as the HTTP pack's `extract` steps carry theirs. A record whose every field is required carries no list, and its payload is unchanged.

## Tests

`a_file_is_read_through_the_file_pack` in `crates/anything2mcp/tests/other_packs.rs`: a row of the stand's stock file whose note cell is empty reads, and the note is `null`. The two tests in `crates/anything2mcp/tests/start_on_demand.rs` that read the stock file through an engine started on demand.
