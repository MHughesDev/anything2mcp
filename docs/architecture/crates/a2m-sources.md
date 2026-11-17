# `a2m-sources`

Turns an input file into validated records. It defines `SourceReader`, carries the built-in readers for xlsx, csv, and json, checks a file against the template versions a plan knows by their fingerprints, applies the mapping, and gives the engine typed rows or every problem at once. It never opens a path and never reaches a target.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 4, Services |
| May depend on | Layers 1 to 3 |
| Depends on | `a2m-core`; `a2m-expr` for mappings, row filters, and the host functions for units, numbers, and dates; `a2m-files` for the compiled source definition and the field types in a plan; `a2m-telemetry`. Outside: an xlsx reader, a csv reader, and a json reader that keeps a number's text, each to be verified |
| Built in | Phase 3, group D and slice 3.9. Group C's normalization rules arrive with slice 3.4 and are used here for typing cells |
| Carries out | Spec sections [12](../../spec/12-data-sources-and-value-normalization.md), [51](../../spec/51-defending-against-a-manipulated-agent.md) (a file is read once, its content checked), [42](../../spec/42-run-ledger-design.md) (step 1 of the preview), [67](../../spec/67-the-scout-and-who-does-what.md) (the structure of a real file, for the drafter); the `sources/*.yaml` part of the [file formats](../../reference/file-formats.md#sourcesyaml) |

## Owns

- `SourceReader`, the extension point for a file format.
- The built-in readers for xlsx, csv, and json, and the content check that a file is the format it claims.
- Template fingerprints: how each is computed, and the list of differences when a file matches no known version.
- Applying a mapping: locating the data, filtering rows, turning cells into row fields of declared types.
- `ValidatedRecords`: the only form in which the engine receives a source's rows.
- Describing a file's structure, for a discoverer to turn into a finding.
- The parsed form of a file, with each value's position in the bytes, for the file pack to read and patch files without parsing them a second way.

It must never: open a file by path; read a file twice; convert a number through a float; treat text such as "NA" as missing unless the mapping says so; guess which column is which; name an operation.

## Interface

```rust
/// The bytes of one input, already read once and stored as a blob by the caller.
pub struct SourceInput<'a> { pub bytes: &'a [u8], pub claimed: ContentKind, pub blob: BlobRef, pub size_limit: u64 }
pub enum ContentKind { Xlsx, Csv, Json }

pub trait SourceReader: Send + Sync {
    fn name(&self) -> ReaderName;                            // `xlsx`, `csv`, `json`, or a plug-in's
    /// Refuses content that is not the format it claims, whatever its name says.
    fn check(&self, input: &SourceInput<'_>) -> Result<(), Failure>;
    fn structure(&self, input: &SourceInput<'_>) -> Result<Structure, Failure>;
    fn fingerprint(&self, s: &Structure) -> Fingerprint;
    /// Cells in the located region, with what the format says about each. No typing yet.
    async fn cells(&self, input: &SourceInput<'_>, locate: &Locate) -> Result<CellTable, Failure>;
    /// The whole file as parsed, with positions. `cells` is a view of this.
    fn parse(&self, input: &SourceInput<'_>) -> Result<Parsed, Failure>;
}

/// A file as parsed: its structure, and every cell with where its value lies in the bytes.
pub struct Parsed { pub structure: Structure, pub parts: Vec<ParsedPart> }
pub struct ParsedPart { pub name: String, pub span: ByteSpan, pub cells: Vec<PlacedCell> }  // a sheet's XML part; a csv file; a json document
pub struct PlacedCell { pub cell: Cell, pub value_span: Option<ByteSpan> }                 // None where the format stores no literal, such as an empty cell
pub struct ByteSpan { pub start: u64, pub end: u64 }                                       // within the part's bytes, after any unpacking

/// What a file is made of, without its values: sheets, named ranges and their targets, header text,
/// merged ranges, dropdown lists, the date system; for csv the header; for json the record keys.
pub struct Structure { /* per format, in declared order */ }
pub struct Fingerprint { pub digest: Digest, pub parts: Vec<FingerprintPart> }

pub struct Cell { pub raw: RawCell, pub at: CellRef, pub format: Option<String>, pub hidden: bool, pub merged: Option<CellRange> }
pub enum RawCell { Empty, Text(String), Number(String), Bool(bool), DateSerial(String), Formula { cached: Option<Box<RawCell>> } }

/// Reads one source as the plan's source definition says.
pub struct Sources { /* the registered readers */ }
impl Sources {
    pub fn new(readers: Vec<Arc<dyn SourceReader>>) -> Sources;
    pub async fn read(&self, def: &CompiledSource, input: SourceInput<'_>, ex: &Evaluator) -> Result<ValidatedRecords, Rejected>;
    /// For a reader of kind `operation`: rows the engine got from another operation's result.
    pub fn from_rows(&self, def: &CompiledSource, rows: Vec<Values>, ex: &Evaluator) -> Result<ValidatedRecords, Rejected>;
    pub fn describe(&self, input: SourceInput<'_>) -> Result<(Structure, Fingerprint), Failure>;
    /// Turns one cell into a typed value by a field type: the same code `read` uses.
    pub fn typed(&self, cell: &Cell, ty: &FieldType, ex: &Evaluator) -> Result<Value, Problem>;
}

pub struct ValidatedRecords { pub template: TemplateVersion, pub fingerprint: Digest, pub rows: Vec<SourceRow> }
pub struct SourceRow { pub index: u32, pub origin: CellRef, pub fields: Values }
/// Every problem in the file, at once. Nothing is partly accepted.
pub struct Rejected { pub failure: Failure, pub problems: Vec<Problem> }
pub struct Problem { pub at: Option<CellRef>, pub field: Option<String>, pub code: FailureCode, pub message: String }
```

Contracts that the signatures do not show:

- **The caller reads the file; this crate reads bytes.** Resolving a path inside the allowed roots, refusing links that leave them, the size limit, and storing the content as a blob happen before `read` is called, through the file access in `a2m-provider`, so that what was previewed is what is entered.
- **Fingerprint first.** `read` computes the file's fingerprint and compares it with every template version in `def`. If none matches, it stops with `source_template_unknown`, and `problems` lists each differing part: a sheet missing, a header renamed, a named range moved, a dropdown list changed. It reads no values from a file it does not recognize.
- **The fingerprint is canonical.** Its parts are taken in declared order, names compared exactly, and the digest is computed over a canonical encoding, so the same file gives the same digest on every operating system. Display formatting, column widths, and values do not enter it.
- **No float, ever.** An xlsx number is taken from the text stored in the file, a json number from its token text, and each is parsed straight to `Decimal`. A date serial is converted with the workbook's own date system. A test feeds values a float cannot represent and checks them exactly.
- **Two passes for a workbook.** Formulas and cached values are both read. A formula cell in the located region with no cached value is a problem, not an empty cell.
- **Found by name, never by coordinate.** `Locate` is a named range or header text from the source definition. A header that appears twice, or not at all, is a problem.
- **Typing uses the field types.** Each row field has a type in the plan. The cell's raw value is turned into a `Value` by that type's rules: places and rounding, units and steps, the time-zone rule, choices by stored code. Those rules come from the field types and `a2m-expr`'s host functions, not from code here, so a unit lives in one place.
- **Mappings are expressions.** A row field that is not a bare column is a CEL expression over `row` and `source`, evaluated by `a2m-expr`. The `rows` filter is one too. An expression that fails on a row is a problem for that row.
- **All problems, then nothing.** `read` returns `ValidatedRecords` only if no row has a problem. Otherwise it returns every problem, each with its cell and field, and the run is `rejected` with nothing changed.
- **Record limits are not here.** The count of rows goes to `a2m-policy`'s `check_limits`. This crate does not know a limit.
- **One parser, one meaning.** The file pack of section 70 depends on this crate for parsing, fingerprints, and `typed`, so a value never means one thing to a source and another to the file pack. A `ByteSpan` points into the part as stored, after unpacking a workbook's archive, so the file pack can replace a value's bytes and leave the rest of the part as it was. Re-packing and checking the patched file are the file pack's work.
- **Plug-in readers** reach this trait through the plug-in host in `a2m-provider`, with the methods `read` and `fingerprint` of the plug-in protocol. A plug-in reader returns cells, and typing and mapping still happen here, so a plug-in cannot bypass validation.

## A read, step by step

What happens between a pipeline call with a `source` input and the first record row in the ledger.

| # | Step | Done by |
| --- | --- | --- |
| 1 | Resolve the path or blob reference; check the allowed roots, the size limit, and that it is a file | `a2m-actions`, through the file access in `a2m-provider`, before this crate |
| 2 | Read the bytes once and store them as a blob, sealed; the run's input hash covers these bytes | `a2m-actions`, through `a2m-ledger`; the engine receives the bytes through `Control` |
| 3 | Check the content is the claimed format | `SourceReader::check` |
| 4 | Take the structure and its fingerprint; find the template version that matches, or reject with the differences | `Sources::read` |
| 5 | Locate the data by named range or header text, and read its cells | `SourceReader::cells` |
| 6 | Apply the `rows` filter, then each mapping, then each field's type | `Sources::read`, with `a2m-expr` |
| 7 | Return every row, or every problem | `Sources::read` |
| 8 | Count the rows against the record limit | `a2m-policy` |
| 9 | Plan one record row per commit point the rows imply | `a2m-engine`, through `a2m-ledger` |

Steps 3 to 7 are pure: the same bytes and the same plan give the same result, which is what lets a second run of the same sheet change nothing.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-engine` | `Sources::read` and `from_rows` at step 1 of the preview |
| `a2m-provider` | `SourceReader`, to register plug-in readers alongside the built-in ones |
| The file pack, once it exists (section 70) | `SourceReader::parse`, `Parsed` and its positions, fingerprints, `Sources::typed` |
| `a2m-actions` | `Sources::new` with the registry's readers; `describe` for the discoverer of a real file's structure that feeds the drafter |

It uses `a2m-files` for `CompiledSource` and field types, `a2m-expr` for expressions and host functions, and `a2m-telemetry` for spans.

## State

None. Inputs arrive as bytes already stored; results go back to the caller, which stores them.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `source_template_unknown` | No template version's fingerprint matches; the problems list the differences |
| `input` | `file_type_mismatch` | The content is not the format it claims |
| `input` | `schema_invalid` | A required field is missing, a header is duplicated, or a value does not fit its type |
| `input` | `value_out_of_range` | A decimal, date, or measurement cannot be represented in its field type |
| `input` | `formula_without_value` (new) | A formula cell in the data has no cached value |
| `input` | `source_too_large` (new) | The bytes, rows, or cells exceed the declared limits before reading finishes |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A changed template is rejected with its differences | Phase 3, group D's test, one changed part at a time |
| The same file gives the same fingerprint and the same rows on three operating systems | A CI test over a fixed synthetic workbook |
| No value passes through a float | A test with decimals a float would round, and a source check for float parsing |
| Text such as "NA" or "null" is text unless the mapping lists it in `missing` | A unit test per reader |
| A renamed file of another format is refused | A test per pair of formats |
| Every problem is reported in one pass | A test file with problems in several rows and fields |
| Every span in `Parsed` covers exactly the literal of its value | A test that rewrites each span with its own text and gets identical bytes, per reader |

## Build order

1. `SourceReader`, `Structure`, fingerprints, and the csv reader, which is the simplest; `ValidatedRecords` and `Rejected`. Slice 3.9.
2. The xlsx reader, with both passes, the date system, merged and hidden cells, and named ranges.
3. Typing by field types, once slice 3.4 has the normalization rules.
4. The json reader and `from_rows`.
5. `parse` with positions, and `typed` as a public call, when the file pack is built.
6. `describe`, with the first discoverer that drafts a source definition. Stage E, or the file pack of section 70.

## Open points

- The type of a bare column in a mapping, which the file formats leave to slice 3.9: every row field needs a type before any file is read.
- Which outside xlsx reader exposes cached values and number text without converting them to floats. If none does, the reader reads the workbook's parts directly.
- Whether a csv file declares its delimiter and encoding in the source definition or has them detected by a fixed rule.
