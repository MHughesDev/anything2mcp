//! Reading one source as its definition says: check, parse, fingerprint,
//! locate, filter, map, type. Either every row comes back or every problem
//! does; nothing of a rejected input is accepted.

use std::collections::BTreeMap;
use std::sync::Arc;

use a2m_core::{Digest, Failure, Value, Values};
use a2m_expr::Bindings;

use crate::csv::CsvReader;
use crate::definition::{LoadedSource, OPERATION_READER, Origin};
use crate::error::{
    KEY_NOT_UNIQUE, Problem, Rejected, SCHEMA_INVALID, SOURCE_READER_UNKNOWN,
    SOURCE_TEMPLATE_UNKNOWN, definition, failure, too_large,
};
use crate::fingerprint::{Fingerprint, Structure};
use crate::json::JsonReader;
use crate::locate::{CellTable, locate};
use crate::model::{
    CellRef, ContentKind, Limits, Parsed, ParsedPart, ReaderName, SourceInput, SourceReader,
};
use crate::typing::{CellContext, normalized, row_value, typed};
use crate::xlsx::XlsxReader;

/// One row of a source: its row fields, typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRow {
    /// Its position among the rows accepted, from zero.
    pub index: u32,
    /// Where the row lies in the input: its first located cell. For rows
    /// handed in by a caller, part 0 and the row's position from one.
    pub origin: CellRef,
    /// The row fields, by name. A field with no value is `Null`.
    pub fields: Values,
}

/// The rows of a source, every one validated. The only form in which the
/// engine receives a source's rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRecords {
    /// The template's name, as the source file states it.
    pub template: String,
    /// The template version the input matched. Nothing for rows handed in
    /// by a caller, which have no file.
    pub version: Option<String>,
    /// The digest of the input's fingerprint. Nothing for rows handed in.
    pub fingerprint: Option<Digest>,
    /// The rows, in the order of the input.
    pub rows: Vec<SourceRow>,
    /// How many rows the definition's row filter said are not records.
    pub filtered_out: u32,
}

/// What an input is, for a discoverer that drafts a source definition: only
/// what the bytes state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description {
    /// The format the content was read as.
    pub kind: ContentKind,
    /// What the input is made of.
    pub structure: Structure,
    /// The fingerprint a definition of this layout would declare.
    pub fingerprint: Fingerprint,
    /// Each part's name, how many rows it has, and how many cells hold
    /// something.
    pub parts: Vec<PartSize>,
}

/// The size of one part of an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartSize {
    /// The part's name.
    pub name: String,
    /// How many rows it has.
    pub rows: u32,
    /// How many of its cells hold something.
    pub cells: u32,
}

/// The registered readers, and reading through them.
#[derive(Clone)]
pub struct Sources {
    readers: BTreeMap<ReaderName, Arc<dyn SourceReader>>,
}

impl std::fmt::Debug for Sources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sources")
            .field("readers", &self.readers.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn unknown_reader(name: &str) -> Failure {
    failure(
        SOURCE_READER_UNKNOWN,
        format!("No reader is registered under the name `{name}`."),
        "Use a reader the engine has, or enable the plug-in that brings it.",
    )
}

impl Sources {
    /// A registry of the readers given.
    ///
    /// # Errors
    ///
    /// `source_definition_invalid` when two readers have one name, or one is
    /// named `operation`, which is not a reader of bytes.
    pub fn new(readers: Vec<Arc<dyn SourceReader>>) -> Result<Self, Failure> {
        let mut registry = BTreeMap::new();
        for reader in readers {
            let name = reader.name().clone();
            if name.as_str() == OPERATION_READER {
                return Err(definition("no reader of bytes may be named `operation`"));
            }
            if registry.insert(name, reader).is_some() {
                return Err(definition("two readers are registered under one name"));
            }
        }
        Ok(Self { readers: registry })
    }

    /// The built-in readers: `csv`, `json`, and `xlsx`.
    #[must_use]
    pub fn builtin() -> Self {
        let readers: [Arc<dyn SourceReader>; 3] = [
            Arc::new(CsvReader::new()),
            Arc::new(JsonReader::new()),
            Arc::new(XlsxReader::new()),
        ];
        Self {
            readers: readers
                .into_iter()
                .map(|reader| (reader.name().clone(), reader))
                .collect(),
        }
    }

    /// The names of the registered readers, in order.
    pub fn readers(&self) -> impl Iterator<Item = &ReaderName> {
        self.readers.keys()
    }

    fn reader(&self, name: &str) -> Result<&dyn SourceReader, Failure> {
        self.readers
            .iter()
            .find(|(known, _)| known.as_str() == name)
            .map(|(_, reader)| reader.as_ref())
            .ok_or_else(|| unknown_reader(name))
    }

    /// Checks an input against its limits and its claimed format, and parses
    /// it: the parsed form, with the position of every value. The file pack
    /// reads through this.
    ///
    /// # Errors
    ///
    /// `source_reader_unknown`; `source_too_large`; and the reader's own:
    /// `file_type_mismatch`, `source_malformed`, `source_protected`,
    /// `source_encoding_unsupported`.
    pub fn parse(&self, reader: &str, input: &SourceInput<'_>) -> Result<Parsed, Failure> {
        let reader = self.reader(reader)?;
        if input.bytes.len() as u64 > input.limits.bytes {
            return Err(too_large(format!(
                "the input has more than {} bytes",
                input.limits.bytes
            )));
        }
        reader.check(input)?;
        reader.parse(input)
    }

    /// What an input is made of, and its fingerprint, with nothing inferred
    /// beyond what the bytes state.
    ///
    /// # Errors
    ///
    /// As for [`Sources::parse`].
    pub fn describe(&self, reader: &str, input: &SourceInput<'_>) -> Result<Description, Failure> {
        let parsed = self.parse(reader, input)?;
        let fingerprint = parsed.structure.fingerprint();
        let parts = parsed
            .parts
            .iter()
            .map(|part| PartSize {
                name: part.name.clone(),
                rows: part.rows,
                cells: u32::try_from(
                    part.cells
                        .iter()
                        .filter(|placed| placed.cell.raw != crate::RawCell::Empty)
                        .count(),
                )
                .unwrap_or(u32::MAX),
            })
            .collect();
        Ok(Description {
            kind: parsed.kind,
            structure: parsed.structure,
            fingerprint,
            parts,
        })
    }

    /// Reads one input as the source definition says. The bytes are the
    /// input the caller has already read once and stored.
    ///
    /// # Errors
    ///
    /// [`Rejected`], with every problem: `source_template_unknown` with each
    /// difference when the input matches no template version; otherwise the
    /// problems of locating the data and of each row. A failure of the reader
    /// itself comes back with no problems.
    pub fn read(
        &self,
        def: &LoadedSource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<ValidatedRecords, Rejected> {
        let input = SourceInput {
            bytes,
            limits: *limits,
            options: def.spec().options,
        };
        let parsed = self.parse(def.reader(), &input)?;
        Self::validate(def, &parsed)
    }

    /// Validates an input already parsed, by a built-in reader or by a
    /// plug-in's: the same fingerprint check, locating, filtering, mapping,
    /// and typing as [`Sources::read`].
    ///
    /// # Errors
    ///
    /// As for [`Sources::read`].
    pub fn validate(def: &LoadedSource, parsed: &Parsed) -> Result<ValidatedRecords, Rejected> {
        let spec = def.spec();
        // Fingerprint first: no value is read from an input that is not a
        // known template version.
        let found = parsed.structure.fingerprint();
        let Some(version) = spec
            .versions
            .iter()
            .find(|version| version.fingerprint.digest == found.digest)
        else {
            return Err(unknown_template(def, &found));
        };
        let table =
            locate(parsed, &spec.locate, &spec.columns, spec.hidden).map_err(Rejected::of)?;
        let part = parsed
            .part(table.part)
            .ok_or_else(|| Rejected::from(definition("the located part is not in the input")))?;
        let context = CellContext {
            date_system: parsed.date_system(),
            missing: &spec.missing,
        };
        let source = source_record(def, Some(&version.version));
        let mut rows = Rows::new(def);
        for &row in &table.rows {
            rows.take(
                &FileRow {
                    part,
                    table: &table,
                    row,
                },
                &context,
                &source,
            );
        }
        rows.finish(def, Some(version.version.clone()), Some(found.digest))
    }

    /// Validates rows handed in by a caller, for a source whose reader is
    /// `operation`: each row is a record of raw values by column name, and
    /// goes through the same filtering, mapping, typing, and key check.
    ///
    /// # Errors
    ///
    /// [`Rejected`], with every problem. A row with a name the definition
    /// does not list as a column is a problem.
    pub fn from_rows(
        &self,
        def: &LoadedSource,
        rows: Vec<Values>,
    ) -> Result<ValidatedRecords, Rejected> {
        let spec = def.spec();
        let context = CellContext {
            date_system: None,
            missing: &spec.missing,
        };
        let source = source_record(def, None);
        let mut taken = Rows::new(def);
        for (position, values) in rows.iter().enumerate() {
            let row = u32::try_from(position + 1).unwrap_or(u32::MAX);
            taken.take(
                &GivenRow {
                    values,
                    row,
                    columns: &spec.columns,
                },
                &context,
                &source,
            );
        }
        taken.finish(def, None, None)
    }
}

fn unknown_template(def: &LoadedSource, found: &Fingerprint) -> Rejected {
    // The differences are those from the nearest version: the one with the
    // fewest, and of those the first declared.
    let problems = def
        .spec()
        .versions
        .iter()
        .map(|version| version.fingerprint.differences(found))
        .min_by_key(Vec::len)
        .unwrap_or_default();
    Rejected {
        failure: failure(
            SOURCE_TEMPLATE_UNKNOWN,
            format!(
                "The input's layout matches no version of the template `{}`: {} difference(s) from the nearest version.",
                def.template(),
                problems.len()
            ),
            "Give a file made from a known version of the template, or add this layout to the source definition as a new version.",
        ),
        problems,
    }
}

/// The record `source` an expression reads: the template, the version the
/// input matched, and the reader.
fn source_record(def: &LoadedSource, version: Option<&str>) -> Value {
    let mut record = Values::new();
    record.insert("template", def.template());
    record.insert("reader", def.reader());
    record.insert("version", version.map_or(Value::Null, Value::from));
    Value::Record(record)
}

/// One row, from a file or handed in: what its columns hold.
trait RowSource {
    fn origin(&self) -> CellRef;
    /// Where the value under a header lies, for a problem's position.
    fn at(&self, header: &str) -> Option<CellRef>;
    /// The typed value under a header, by a field type.
    fn typed(
        &self,
        header: &str,
        typer: &crate::FieldTyper,
        context: &CellContext<'_>,
    ) -> Result<Value, Problem>;
    /// The record `row`, and the problems of building it. A column read by
    /// a bare-column field reports its own problem there, so `quiet` columns
    /// are left out of the problems here.
    fn record(&self, context: &CellContext<'_>, quiet: &[&str]) -> (Values, Vec<Problem>);
    /// A problem for each name in the row that the definition does not list
    /// as a column. A file's columns are checked once, when the data is
    /// located, so a file's row has none.
    fn unlisted(&self) -> Vec<Problem>;
}

struct FileRow<'a> {
    part: &'a ParsedPart,
    table: &'a CellTable,
    row: u32,
}

impl RowSource for FileRow<'_> {
    fn origin(&self) -> CellRef {
        let column = self.table.columns.first().map_or(1, |column| column.column);
        CellRef::new(self.table.part, self.row, column)
    }

    fn at(&self, header: &str) -> Option<CellRef> {
        self.table
            .column(header)
            .map(|column| CellRef::new(self.table.part, self.row, column))
    }

    fn typed(
        &self,
        header: &str,
        typer: &crate::FieldTyper,
        context: &CellContext<'_>,
    ) -> Result<Value, Problem> {
        let cell = self
            .table
            .column(header)
            .and_then(|column| self.part.cell(self.row, column));
        match cell {
            Some(placed) => typed(&placed.cell, typer, context),
            None => Ok(Value::Null),
        }
    }

    fn record(&self, context: &CellContext<'_>, quiet: &[&str]) -> (Values, Vec<Problem>) {
        let mut record = Values::new();
        let mut problems = Vec::new();
        for column in &self.table.columns {
            let value = match self.part.cell(self.row, column.column) {
                None => Value::Null,
                Some(placed) => match row_value(&placed.cell.raw, context) {
                    Ok(value) => value,
                    Err((code, message)) => {
                        if !quiet.contains(&column.header.as_str()) {
                            problems.push(
                                Problem::new(code, message)
                                    .at(placed.cell.at)
                                    .field(&column.header),
                            );
                        }
                        Value::Null
                    }
                },
            };
            record.insert(column.header.clone(), value);
        }
        (record, problems)
    }

    fn unlisted(&self) -> Vec<Problem> {
        Vec::new()
    }
}

struct GivenRow<'a> {
    values: &'a Values,
    row: u32,
    columns: &'a [String],
}

impl RowSource for GivenRow<'_> {
    fn origin(&self) -> CellRef {
        CellRef::new(0, self.row, 1)
    }

    fn at(&self, header: &str) -> Option<CellRef> {
        let column = self.columns.iter().position(|column| column == header)?;
        Some(CellRef::new(0, self.row, u32::try_from(column + 1).ok()?))
    }

    fn typed(
        &self,
        header: &str,
        typer: &crate::FieldTyper,
        context: &CellContext<'_>,
    ) -> Result<Value, Problem> {
        let value = match self.values.get(header) {
            Some(Value::Text(text)) if context.missing.contains(text) => &Value::Null,
            Some(value) => value,
            None => &Value::Null,
        };
        normalized(value, typer).map_err(|(code, message)| Problem::new(code, message))
    }

    fn record(&self, context: &CellContext<'_>, _quiet: &[&str]) -> (Values, Vec<Problem>) {
        let mut record = Values::new();
        for column in self.columns {
            let value = match self.values.get(column) {
                Some(Value::Text(text)) if context.missing.contains(text) => Value::Null,
                Some(value) => value.clone(),
                None => Value::Null,
            };
            record.insert(column.clone(), value);
        }
        (record, Vec::new())
    }

    fn unlisted(&self) -> Vec<Problem> {
        self.values
            .names()
            .filter(|name| !self.columns.iter().any(|column| column == name))
            .map(|name| {
                Problem::new(
                    SCHEMA_INVALID,
                    "The row has a name the source definition does not list as a column.",
                )
                .at(self.origin())
                .field(name)
            })
            .collect()
    }
}

/// The rows accepted so far and the problems found so far.
struct Rows<'a> {
    def: &'a LoadedSource,
    /// The headers that bare-column fields read.
    bare: Vec<&'a str>,
    rows: Vec<SourceRow>,
    problems: Vec<Problem>,
    filtered_out: u32,
    /// The key of each accepted row, with the row it came from.
    keys: BTreeMap<Vec<u8>, u32>,
}

impl<'a> Rows<'a> {
    fn new(def: &'a LoadedSource) -> Self {
        let bare = def
            .fields
            .iter()
            .filter_map(|field| match &field.from {
                Origin::Column(column) => Some(column.as_str()),
                Origin::Expression(_) => None,
            })
            .collect();
        Self {
            def,
            bare,
            rows: Vec::new(),
            problems: Vec::new(),
            filtered_out: 0,
            keys: BTreeMap::new(),
        }
    }

    fn take(&mut self, row: &dyn RowSource, context: &CellContext<'_>, source: &Value) {
        let origin = row.origin();
        let mut problems = row.unlisted();
        // The record `row` is built only when an expression reads it. When a
        // cell of it cannot be read, no expression is evaluated for the row.
        let mut record = Values::new();
        let mut record_whole = true;
        if self.def.reads_rows() {
            let (built, mut found) = row.record(context, &self.bare);
            record = built;
            record_whole = found.is_empty();
            problems.append(&mut found);
        }
        let record = Value::Record(record);
        let bindings = Bindings::new()
            .with(a2m_expr::names::ROW, &record)
            .with(a2m_expr::names::SOURCE, source);
        if let Some(filter) = &self.def.filter {
            if !record_whole {
                self.problems.append(&mut problems);
                return;
            }
            match filter.evaluate(&bindings) {
                Ok(Value::Bool(true)) => {}
                Ok(Value::Bool(false)) => {
                    self.filtered_out += 1;
                    self.problems.append(&mut problems);
                    return;
                }
                Ok(_) => {
                    problems.push(
                        Problem::new(SCHEMA_INVALID, "The row filter did not give true or false.")
                            .at(origin),
                    );
                    self.problems.append(&mut problems);
                    return;
                }
                Err(failure) => {
                    problems
                        .push(Problem::new(failure.code().clone(), failure.message()).at(origin));
                    self.problems.append(&mut problems);
                    return;
                }
            }
        }
        let mut fields = Values::new();
        for field in &self.def.fields {
            let (value, at) = match &field.from {
                Origin::Column(column) => (
                    row.typed(column, &field.typer, context),
                    row.at(column).unwrap_or(origin),
                ),
                // The record has a problem already reported; the expression
                // has nothing sound to read.
                Origin::Expression(_) if !record_whole => continue,
                Origin::Expression(program) => (
                    program
                        .evaluate(&bindings)
                        .map_err(|failure| (failure.code().clone(), failure.message().to_owned()))
                        .and_then(|value| normalized(&value, &field.typer))
                        .map_err(|(code, message)| Problem::new(code, message)),
                    origin,
                ),
            };
            match value {
                Ok(Value::Null) if field.required => {
                    problems.push(
                        Problem::new(SCHEMA_INVALID, "The row has no value for a required field.")
                            .at(at)
                            .field(&field.name),
                    );
                }
                Ok(value) => {
                    fields.insert(field.name.clone(), value);
                }
                Err(problem) => {
                    let problem = Problem {
                        at: problem.at.or(Some(at)),
                        ..problem
                    };
                    problems.push(problem.field(&field.name));
                }
            }
        }
        if !problems.is_empty() {
            self.problems.append(&mut problems);
            return;
        }
        let key = &self.def.spec().key;
        if !key.is_empty() {
            let mut bytes = Vec::new();
            let mut whole = true;
            for name in key {
                match fields.get(name) {
                    Some(Value::Null) | None => whole = false,
                    Some(value) => value.write_canonical(&mut bytes),
                }
                bytes.push(0x1F);
            }
            if !whole {
                self.problems.push(
                    Problem::new(
                        SCHEMA_INVALID,
                        "The row has no value for a field of its key.",
                    )
                    .at(origin),
                );
                return;
            }
            if let Some(first) = self.keys.get(&bytes) {
                self.problems.push(
                    Problem::new(
                        KEY_NOT_UNIQUE,
                        format!("The row has the same key as row {first}."),
                    )
                    .at(origin),
                );
                return;
            }
            self.keys.insert(bytes, origin.row);
        }
        self.rows.push(SourceRow {
            index: u32::try_from(self.rows.len()).unwrap_or(u32::MAX),
            origin,
            fields,
        });
    }

    fn finish(
        self,
        def: &LoadedSource,
        version: Option<String>,
        fingerprint: Option<Digest>,
    ) -> Result<ValidatedRecords, Rejected> {
        if self.problems.is_empty() {
            Ok(ValidatedRecords {
                template: def.template().to_owned(),
                version,
                fingerprint,
                rows: self.rows,
                filtered_out: self.filtered_out,
            })
        } else {
            Err(Rejected::of(self.problems))
        }
    }
}
