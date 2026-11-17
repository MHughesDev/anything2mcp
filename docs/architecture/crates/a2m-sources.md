# `a2m-sources`

Turns an input into validated records. It defines `SourceReader`, carries the built-in readers for xlsx, csv, and json, checks an input against the template versions a plan knows by their fingerprints, applies the mapping, and gives the engine typed rows or every problem at once. It takes bytes: it never opens a path and never reaches a target.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 4, Services |
| May depend on | Layers 1 to 3 |
| Depends on | `a2m-core`, for values, the plan's `SourcePlan` and field types, digests, and failures; `a2m-expr`, for mappings, row filters, the field types' normalizers, and the date systems. Outside: `miniz_oxide`, which inflates the parts of a workbook's package ([decision 0086](../../decisions/0086-the-source-readers-are-our-own-over-one-outside-crate-that-inflates.md)). It does not depend on `a2m-files`: the compiler writes the source definition into the plan as data, and this crate reads that data back. It does not use `a2m-telemetry` yet |
| Built in | Phase 3, group D and slice 3.9 |
| Carries out | Spec sections [210](../../spec/2-kernel/210-sources-and-value-normalization.md), [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (a file is read once, its content checked), [208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md) (step 1 of the preview), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) (the structure of a real file, for the drafter); the `sources/*.yaml` part of the [file formats](../../reference/file-formats.md#sourcesyaml) |

## Owns

- `SourceReader`, the extension point for a format.
- The built-in readers for xlsx, csv, and json, and the content check that an input is the format it claims.
- Template fingerprints: how each is computed, and the list of differences when an input matches no known version.
- `SourceSpec`, the compiled source definition: the payload of the plan's `SourcePlan`.
- Applying a mapping: locating the data, filtering rows, turning cells into row fields of declared types.
- `ValidatedRecords`: the only form in which the engine receives a source's rows.
- Describing an input's structure, for a discoverer to turn into a finding.
- The parsed form of an input, with each value's position in the bytes, for the file pack to read and patch files without parsing them a second way.

It must never: open a file by path; read an input twice; convert a number through a float; treat text such as "NA" as missing unless the definition says so; guess which column is which; calculate a formula; run a macro; follow a link to another file; name an operation.

## Interface

As built. The names are the ones `src/lib.rs` exports.

```rust
/// One input: bytes the caller has already read once and stored. There is no path.
pub struct SourceInput<'a> { pub bytes: &'a [u8], pub limits: Limits, pub options: ReaderOptions }
pub struct Limits { pub bytes: u64, pub unpacked_bytes: u64, pub rows: u32, pub cells: u32, pub cell_bytes: u32 }
pub struct ReaderOptions { pub delimiter: Option<char> }   // the dialect a source definition declares

pub trait SourceReader: Send + Sync {
    fn name(&self) -> &ReaderName;                           // `xlsx`, `csv`, `json`, or a plug-in's
    /// Refuses content that is not the format it reads, whatever the input was called.
    fn check(&self, input: &SourceInput<'_>) -> Result<(), Failure>;
    /// The whole input as parsed: its structure, and every cell with where its value lies.
    fn parse(&self, input: &SourceInput<'_>) -> Result<Parsed, Failure>;
}
pub struct CsvReader; pub struct JsonReader; pub struct XlsxReader;   // each with `new()`

/// An input as parsed.
pub struct Parsed { pub kind: ContentKind, pub structure: Structure, pub parts: Vec<ParsedPart> }
pub struct ParsedPart { pub name: String, pub path: String, pub bytes: PartBytes, pub cells: Vec<PlacedCell>, pub rows: u32, pub widths: Vec<u32> }
pub enum PartBytes { Input, Unpacked(Vec<u8>) }             // spans point into the input, or into a part of a package as unpacked
pub struct PlacedCell { pub cell: Cell, pub value_span: Option<ByteSpan>, pub shared: Option<u32> }
pub struct ByteSpan { pub start: u64, pub end: u64 }
pub struct Cell { pub raw: RawCell, pub at: CellRef, pub format: Option<String>, pub hidden: bool, pub merged: Option<CellRange> }
pub struct CellRef { pub part: u32, pub row: u32, pub column: u32 }   // rows and columns from one
pub enum RawCell { Empty, Text(String), Number(String), Bool(bool), DateSerial(String),
                   Formula { cached: Option<Box<RawCell>> }, Error(String), Nested }

/// What an input is made of, without its values, and its fingerprint.
pub struct Structure { pub date_system: Option<DateSystem>, pub sheets: Vec<SheetStructure>, pub names: Vec<DefinedName>, pub macros: bool, pub signed: bool }
pub struct SheetStructure { pub name: String, pub hidden: bool, pub headers: Vec<String>, pub merged: Vec<CellRange>, pub tables: Vec<TableStructure>, pub lists: Vec<DropdownList> }
impl Structure { pub fn fingerprint(&self) -> Fingerprint; }
pub struct Fingerprint { pub digest: Digest, pub parts: Vec<FingerprintPart> }
impl Fingerprint { pub fn differences(&self, found: &Fingerprint) -> Vec<Problem>; pub fn is_sound(&self) -> bool; }

/// The compiled source definition: the payload of the plan's `SourcePlan`.
pub struct SourceSpec { pub versions: Vec<TemplateVersion>, pub locate: Locate, pub columns: Vec<String>, pub fields: Vec<RowField>,
                        pub key: Vec<String>, pub missing: Vec<String>, pub hidden: HiddenRule, pub options: ReaderOptions }
pub struct TemplateVersion { pub version: String, pub fingerprint: Fingerprint }
pub struct RowField { pub name: String, pub field_type: FieldTypeIndex, pub required: bool, pub column: Option<String> }
pub enum Locate { Document, Named(String), Sheet(String) }
pub enum HiddenRule { Reject, Include }

/// A definition loaded from a plan once, and a field type loaded once.
impl LoadedSource { pub fn load(plan: &Plan, source: &SourcePlan) -> Result<LoadedSource, Failure>; }
impl FieldTyper { pub fn load(plan: &Plan, index: FieldTypeIndex) -> Result<FieldTyper, Failure>;
                  pub fn from_parts(field_type: &FieldTypePlan, normalizer: &PlanExpr) -> Result<FieldTyper, Failure>; }

pub struct Sources { /* the registered readers */ }
impl Sources {
    pub fn new(readers: Vec<Arc<dyn SourceReader>>) -> Result<Sources, Failure>;
    pub fn builtin() -> Sources;
    pub fn parse(&self, reader: &str, input: &SourceInput<'_>) -> Result<Parsed, Failure>;
    pub fn describe(&self, reader: &str, input: &SourceInput<'_>) -> Result<Description, Failure>;
    pub fn read(&self, def: &LoadedSource, bytes: &[u8], limits: &Limits) -> Result<ValidatedRecords, Rejected>;
    /// The same validation, for an input a plug-in reader has parsed.
    pub fn validate(def: &LoadedSource, parsed: &Parsed) -> Result<ValidatedRecords, Rejected>;
    /// For a source whose reader is `operation`: rows the engine got from another operation's result.
    pub fn from_rows(&self, def: &LoadedSource, rows: Vec<Values>) -> Result<ValidatedRecords, Rejected>;
}
/// Turns one cell into a typed value by a field type: the same code `read` uses.
pub fn typed(cell: &Cell, typer: &FieldTyper, context: &CellContext<'_>) -> Result<Value, Problem>;
pub struct CellContext<'a> { pub date_system: Option<DateSystem>, pub missing: &'a [String] }
/// Finds the data by name or header text.
pub fn locate(parsed: &Parsed, place: &Locate, wanted: &[String], hidden: HiddenRule) -> Result<CellTable, Vec<Problem>>;

pub struct ValidatedRecords { pub template: String, pub version: Option<String>, pub fingerprint: Option<Digest>, pub rows: Vec<SourceRow>, pub filtered_out: u32 }
pub struct SourceRow { pub index: u32, pub origin: CellRef, pub fields: Values }
/// Every problem in the input, at once. Nothing is partly accepted.
pub struct Rejected { pub failure: Failure, pub problems: Vec<Problem> }
pub struct Problem { pub at: Option<CellRef>, pub field: Option<String>, pub code: FailureCode, pub message: String }
pub struct Description { pub kind: ContentKind, pub structure: Structure, pub fingerprint: Fingerprint, pub parts: Vec<PartSize> }
```

Also public, for the file pack: `json::parse_document` and its tree `JsonNode`, with the span of every value at any depth; `field_text`, which reads one field of delimited text from its bytes; `number::exact`, which reads a stored number's digits into a decimal.

Contracts that the signatures do not show:

- **The caller reads the file; this crate reads bytes.** Resolving a path inside the allowed roots, refusing links that leave them, and storing the content as a blob happen before `read` is called, through the file access in `a2m-provider`, so that what was previewed is what is entered. No type here holds a path, and a test holds the source to that.
- **A reader is a function, not a task.** `check` and `parse` are plain calls: a reader computes over bytes it already holds and waits for nothing. The engine runs a large input on a blocking thread. A plug-in reader, which is another process, is called by the plug-in host in `a2m-provider`; the host hands the `Parsed` it gets to `Sources::validate`, so a plug-in cannot bypass the fingerprint, the mapping, or the typing ([decision 0087](../../decisions/0087-the-source-readers-interface-as-building-it-settled-it.md)).
- **Limits come from the caller.** The bytes of the input, the bytes its package unpacks to, the rows of a part, the cells of the input, and the bytes of one cell each have a limit in `SourceInput`. Passing one stops the read with `source_too_large`. These bound the work of parsing. The limit on how many records a run may change is the policy engine's, not this crate's.
- **Fingerprint first.** `read` computes the input's fingerprint and compares it with every template version in the definition. If none matches, it stops with `source_template_unknown`, and `problems` lists each difference from the nearest version: a sheet renamed, a header renamed, a column added, a named range moved, a dropdown list changed, another date system. It reads no value from an input it does not recognize.
- **The fingerprint is canonical.** It covers, in declared order: the date system; whether a macro part is present; each sheet's name and whether it is hidden; each header by position; each merged range; each table's range and headers; each dropdown list; each defined name and its target. The digest is SHA-256 over a fixed encoding in which every text has its length before it. Values, display formats, and column widths do not enter it. Names a spreadsheet program keeps for itself, such as a print area or a filter's range, do not enter it either, because they change as a file is used.
- **Header text, by a fixed rule.** For delimited text the headers are the first record. For a JSON list they are the keys of its records in the order they first appear. For a sheet they are the text cells of its first row that holds anything; a table's headers are the cells of its first row.
- **No float, ever.** An xlsx number is taken from the text stored in the file, a json number from its token text, and each is read straight into a `Decimal`; an exponent only moves the point. A date serial is converted with the workbook's own date system. A test feeds values a float cannot represent and checks them exactly, and another holds the source free of float types.
- **Both passes of a workbook in one walk.** A workbook keeps a formula and its last result in the same cell element, so the formula's presence and the kept result are read together. A formula in the data with no kept result is a problem, `formula_without_value`, not an empty cell. Nothing is ever calculated.
- **A cell is a date serial by its number format.** A number whose format is one of the built-in date and time formats, or a declared format with a day, month, year, hour, or second letter outside quotes, brackets, and escapes, is `DateSerial`. A plain number in a field whose type is a date is read as a serial too, because the field type declares it.
- **Found by name, never by coordinate.** `Locate` is the whole document, for delimited text and a JSON list at the root; a table, a defined name, or the key of a JSON list; or a sheet, whose header row is the first row holding every header the definition names. The definition lists every column it reads. A header that appears twice or not at all, a column with no header, and a column the definition does not name are each a problem.
- **What lies inside the data.** In a sheet, rows that hold nothing after the last row of data are not rows. A row that holds nothing inside the data, a merged range inside the data, and, unless the definition says `include`, a hidden row or column, are each a problem. In delimited text a row with another number of fields than the header is a problem.
- **Typing uses the field types.** Every row field has a field type in the plan. `typed` hands the field type's normalizer the raw value the cell states: text as text, a number as an exact decimal, a date serial as the date its date system gives, true or false as it is. Places and rounding, units and steps, formats, and codes are the normalizer's, from `a2m-expr`, so a unit lives in one place. What a field type cannot take is a problem: a number with a fraction in a text field, true or false in a number field, a date-formatted cell in a number field, an error value, a list.
- **Mappings are expressions.** A row field with no column is the expression of the same name in the plan's mapping, over `row` and `source`. `row` holds each listed column: text, an exact decimal for a number or a date serial, true or false, or null. `source` holds `template`, `version`, and `reader`. The result goes through the field's type like any other value. The `rows` filter is an expression too; a row it turns away is counted in `filtered_out`. An expression that fails on a row is a problem for that row.
- **A key, when the definition has one.** Two rows whose key fields have the same values are a problem, `key_not_unique`, at the second row.
- **All problems, then nothing.** `read` returns `ValidatedRecords` only if no row has a problem. Otherwise it returns every problem, each with its cell, its field, and a sentence that never holds a value from the input. Problems of locating the data come back alone, before any row is typed.
- **One parser, one meaning.** The file pack of section 308 depends on this crate for parsing, fingerprints, and `typed`, so a value never means one thing to a source and another to the file pack. A span covers exactly the literal of a value: a field of delimited text with its quotes, a JSON token, the content of a workbook cell's value element. For a cell that holds text through the shared strings, the span covers the entry's number and `shared` says which entry. Re-packing and checking a patched file are the file pack's work.

## What is refused, by format

| Format | Read | Refused, with the code |
| --- | --- | --- |
| Delimited text | UTF-8 with or without a byte-order mark; CRLF and LF; a declared one-character delimiter; quoted fields with doubled quotes, delimiters, and line breaks inside; a last line with no ending | Any other encoding, `source_encoding_unsupported`. A quote never closed, text after a closing quote, a quote inside an unquoted field, a carriage return alone, `source_malformed` |
| JSON | A list of records at the root, or a record whose members include lists of records; numbers as their token text | A key written twice in one record, anything JSON does not allow, nesting deeper than 64, `source_malformed`. A root that is not a list or a record, `file_type_mismatch` |
| Workbook | Stored and deflated parts; shared, inline, and formula strings; numbers, true or false, error values; both date systems; styles' number formats; merged ranges; hidden rows, columns, and sheets; tables; defined names; dropdown lists; the presence of a macro part and of a signature | A compound file, which is what an encrypted workbook is kept in, and an encrypted part, `source_protected`. A part whose checksum or size is wrong, a document type declaration, an entity a part would have to declare, cells out of order, the large-file form of a package, `source_malformed`. A package with no workbook, `file_type_mismatch` |

## A read, step by step

What happens between a pipeline call with a `source` input and the first record row in the ledger.

| # | Step | Done by |
| --- | --- | --- |
| 1 | Resolve the path or blob reference; check the allowed roots and that it is a file | `a2m-actions`, through the file access in `a2m-provider`, before this crate |
| 2 | Read the bytes once and store them as a blob, sealed; the run's input hash covers these bytes | `a2m-actions`, through `a2m-ledger`; the engine receives the bytes through `Control` |
| 3 | Check the size limit and that the content is the claimed format | `Sources::parse`, with `SourceReader::check` |
| 4 | Parse the input once | `SourceReader::parse` |
| 5 | Take the structure's fingerprint; find the template version that matches, or reject with the differences | `Sources::validate` |
| 6 | Locate the data by name or header text | `locate` |
| 7 | Apply the `rows` filter, then each mapping, then each field's type; check the key | `Sources::validate`, with `a2m-expr` |
| 8 | Return every row, or every problem | `Sources::validate` |
| 9 | Count the rows against the record limit | `a2m-policy` |
| 10 | Plan one record row per commit point the rows imply | `a2m-engine`, through `a2m-ledger` |

Steps 3 to 8 are pure: the same bytes and the same plan give the same result, which is what lets a second run of the same sheet change nothing.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-engine` | `LoadedSource::load` when a plan is loaded; `Sources::read` and `from_rows` at step 1 of the preview |
| `a2m-provider` | `SourceReader`, to register plug-in readers alongside the built-in ones; `Sources::validate` for what a plug-in parsed |
| The file pack, once it exists (section 308) | `Sources::parse`, `Parsed` and its positions, `Structure::fingerprint`, `locate`, `typed`, `FieldTyper`, `json::parse_document`, `field_text` |
| `a2m-actions` | `Sources::new` with the registry's readers; `describe` for the discoverer of a real file's structure that feeds the drafter |
| `a2m-files` | Nothing in code. It writes a `SourceSpec` into the payload of a `SourcePlan`, in the shape this crate reads |

## State

None. Inputs arrive as bytes already stored; results go back to the caller, which stores them.

## Failures

Every failure is of class `input`: the input was refused and nothing ran.

| Code | When |
| --- | --- |
| `source_template_unknown` | No template version's fingerprint matches; the problems list the differences |
| `file_type_mismatch` | The content is not the format it claims |
| `source_malformed` (new) | The content is its format and breaks the format's rules |
| `source_encoding_unsupported` (new) | Text that is not UTF-8 |
| `source_protected` (new) | An encrypted workbook |
| `source_too_large` (new) | The bytes, the unpacked bytes, the rows, the cells, or one cell exceed the caller's limits |
| `schema_invalid` | A header is missing, duplicated, or not named by the definition; a row has the wrong number of fields; a required value is missing; a cell holds a kind of value its field type cannot take |
| `value_unparsable`, `value_out_of_range` | A value does not fit its field type; both come from the field type's normalizer in `a2m-expr` |
| `formula_without_value` (new) | A formula cell in the data has no kept result |
| `key_not_unique` (new) | Two rows have the same key |
| `source_reader_unknown` (new) | The definition names a reader that is not registered |
| `source_definition_invalid` (new) | The compiled definition does not hold together: an index the plan lacks, a row field named twice, a fingerprint whose digest is not that of its parts |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Bytes in, records out, and no path anywhere | `bytes_go_in_and_validated_records_come_out`, against the file stand's own reader; `the_crate_parses_no_float_and_opens_no_file` |
| A changed template is rejected with its differences | `a_changed_header_changes_the_fingerprint_and_stops_with_the_differences`; the date system in `a_date_serial_is_read_by_the_workbooks_own_date_system` |
| The same bytes give the same fingerprint and the same rows on three operating systems | `the_same_bytes_give_the_same_result_and_a_fixed_fingerprint`, which holds two digests as literals |
| No value passes through a float | `a_decimal_keeps_its_exact_digits_through_every_reader`, and the source check |
| Text such as "NA" or "null" is text unless the definition lists it in `missing` | `text_such_as_na_is_text_unless_the_definition_lists_it_as_missing`, for each reader |
| A renamed file of another format is refused | `a_file_of_another_format_is_refused_whatever_reader_it_is_given_to`, for each pair |
| Every problem is reported in one pass, with a position and a reason, and no value | `every_problem_is_reported_in_one_pass_with_its_position_and_reason` |
| Every span in `Parsed` covers exactly the literal of its value | One test per reader that reads each span back as its cell's value and rewrites it with its own bytes |
| A formula with no kept result is a problem | `a_formula_with_no_kept_result_is_a_problem_not_an_empty_cell` |
| Limits stop an input that is too large | `limits_stop_an_input_that_is_too_large`, for each limit and reader |
| A package cannot make the reader read something else or grow without bound | The package and XML tests: a wrong checksum, an encrypted part, a document type, a declared entity |

## Build order

1. Done: `SourceReader`, `Structure`, fingerprints, and the csv reader; `ValidatedRecords` and `Rejected`.
2. Done: the xlsx reader, with formulas and kept results, both date systems, merged and hidden cells, tables, defined names, and dropdown lists.
3. Done: typing by field types, through `a2m-expr`'s normalizers.
4. Done: the json reader and `from_rows`.
5. Done: `Sources::parse` with positions, and `typed` as a public call.
6. Done: `describe`.

Not built:

- A date serial with a time of day, read into a field whose type is a date and time or a time of day. Such a cell is a problem today. A date and time written as text is read by its field type's format.
- A workbook's protection flags that do not encrypt it, such as a sheet locked against editing. They do not stop a read and are not recorded.
- A package in the large-file form, for files over four gigabytes. It is refused by name.
- Spans for the telemetry of a read. The crate does not depend on `a2m-telemetry` yet.
- The plug-in protocol's side of a reader. `Sources::validate` is the door it will come through.

## Open points

- Settled: the type of a bare column in a mapping. Every row field carries a field type in the definition, whether it reads a column or an expression ([decision 0087](../../decisions/0087-the-source-readers-interface-as-building-it-settled-it.md)).
- Settled: no outside xlsx reader is used; the reader reads the workbook's parts directly ([decision 0086](../../decisions/0086-the-source-readers-are-our-own-over-one-outside-crate-that-inflates.md)).
- Settled: a csv file's delimiter is declared in the source definition, a comma when absent, and its encoding is UTF-8. Nothing is detected.
- The compiler in `a2m-files` and this crate share the shape of `SourceSpec` only as data. If the two drift, a plan fails to load with `payload_unreadable`. Moving the type to `a2m-core` would make the compiler hold it to the shape at build time.
- Whether a cached formula result is stale cannot be told without calculating the workbook, as the [research](../../research/05-data-sources-and-normalization.md) says. The reader takes the kept result.
