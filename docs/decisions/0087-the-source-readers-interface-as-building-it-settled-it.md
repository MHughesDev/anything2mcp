# 0087. The source readers' interface, as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the three open points of the `a2m-sources` design, and the open point "the type of a bare column in a mapping" in the [file formats](../reference/file-formats.md). It changes `SourceReader`, a public contract, against the sketch in the crate's design, which was written with the designs that [decision 0063](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md) records.

## Context

The sketch of `a2m-sources` was written before `a2m-core`'s plan and `a2m-expr` existed. Building the crate against them showed where the sketch did not fit. The design of the crate holds the result; this record holds the choices.

## Decision

**The contract**

- `SourceReader` has three calls: `name`, `check`, and `parse`. Structure, fingerprint, and the located cells are computed by this crate from what `parse` returns, the same way for every reader. *Instead of:* `structure`, `fingerprint`, and `cells` on the trait, which would let each reader, and each plug-in, compute a fingerprint its own way and read an input more than once.
- The calls are plain functions, not `async`. A reader computes over bytes it already holds. A plug-in reader is another process, so the plug-in host calls it and hands the `Parsed` it gets to `Sources::validate`, which runs the same fingerprint check, locating, mapping, and typing. *Instead of:* an `async` method, which a trait object cannot have without boxing every call, for the sake of readers that never wait.
- `SourceInput` holds the bytes, the limits, and the dialect. It holds no blob reference and no claimed format: the caller keeps its own reference, and the reader named by the definition is the claim. There is no path in any public type.
- A reader has limits, given by the caller: bytes, unpacked bytes, rows, cells, and the bytes of one cell. *Instead of:* no limits here, as the sketch said. The sketch meant the record limit of a run, which stays with the policy engine; a parser still needs a bound on its own work.

**The definition**

- The compiled source definition is `SourceSpec`, defined here and carried as the payload of the plan's `SourcePlan`, which `a2m-core` defines. `a2m-sources` does not depend on `a2m-files`. *Instead of:* a `CompiledSource` type in the compiler, which this crate would have had to depend on for one struct.
- **Every row field has a field type**, named in the definition, whether its value is a bare column or an expression. A bare column is typed straight from its cell. An expression's result is typed the same way. So every row field has a type before any file is read, which is what the compiler needs to check a pipeline's `with`. *Instead of:* inferring a column's type from the cells, which is a guess, or giving a bare column the type text, which would turn `0040` in a number column into text.
- A bare column is data in the definition, not an expression in the plan's `mapping`. The plan's `mapping` holds only the row fields that are expressions. An expression over `row` has no known type until it is wrapped in a function, and an expression of unknown type cannot go into a plan.
- The definition lists every column it reads. A column in the data that is not listed is a problem.
- A csv file's delimiter is declared, a comma when absent. Its encoding is UTF-8, with or without a byte-order mark. *Instead of:* detecting either, which is a guess that can change with one row.
- A definition may name the row fields that form a key. Two rows with one key are a problem.
- A hidden row or column inside the data is a problem unless the definition says `include`. Nobody looking at the sheet saw what it holds.

**Fingerprints**

- A template version carries its fingerprint's parts as well as the digest, so that an input matching no version is told how it differs from the nearest one. The digest must be the digest of the parts, or the definition does not load.
- A fingerprint's parts are placed by position, so a renamed sheet is one difference and its headers are still compared one by one.
- The date system and the presence of a macro part are in the fingerprint. A workbook in the other date system is another template: the same serial would be another day.
- Names a spreadsheet program keeps for itself, such as a print area or a filter's range, are left out. They change when a person uses the file, and would make every used file an unknown template.

**Typing**

- A number in a text field is accepted only when it is a plain whole number, which has one spelling. Anything else is a problem. *Instead of:* writing the stored digits out as text, which would make `1.50` and `1.5` different texts for one value.
- A plain number in a date field is a serial, read by the workbook's date system. In a format with no date system it is a problem.
- A cell formatted as a date in a number field is a problem, and so are true or false in any field that is not a boolean, an error value, and a list.
- The codes of a value that does not fit are `a2m-expr`'s, `value_unparsable` and `value_out_of_range`, so one failure has one code in every crate.
- A workbook is read in one walk, not loaded twice. The file keeps a formula and its last result in the same cell element, so both are read together; [spec section 210](../spec/2-kernel/210-sources-and-value-normalization.md) now says so. The rule it stood for is unchanged: a formula with no kept result is a problem, `formula_without_value`.

## Consequences

- `a2m-files` must write a `SourceSpec` into the payload of each `SourcePlan`, with the shape `a2m-sources` reads, and leave bare columns out of `mapping`. The two crates share the shape only as data. If that proves fragile, the type moves to `a2m-core`.
- `a2m-provider`'s plug-in host calls a plug-in reader itself and passes the result to `Sources::validate`.
- The engine calls `LoadedSource::load` once per plan and `Sources::read` per run, on a blocking thread for a large input.
- A date serial with a time of day cannot yet be read into a date-and-time or time-of-day field; it is a named problem until it is built.

## Alternatives not taken

- **The fingerprint as a digest only.** Smaller, and it cannot say what changed.
- **Rows with problems dropped and the rest accepted.** The design forbids it: a source is all of its rows or none.
