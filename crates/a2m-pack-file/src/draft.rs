//! Drafting: from a `file_layout` finding to a `documents/` file, and to the
//! reads over it, by rule.
//!
//! A cell of delimited text holds text, whatever a person later says it
//! means. A field there whose type nobody stated is drafted as the built-in
//! `untrusted_text`: what the file holds, marked as the system's words, and
//! no guess. A workbook stores the kind of each cell: a column of text is
//! `untrusted_text`, a column of numbers `decimal`, a column of true or
//! false `boolean`, and a column that holds a formula is marked `formula:
//! true` and read as the result the workbook kept, which section 308 says
//! a read of a formula's place returns; the pack never calculates one. A
//! column of cells of more than one kind is left out of the document and
//! the list, and its type is asked in the read by key (decision 0188). A
//! key is not needed to list. So the document drafted for such a file holds
//! no open question and compiles, and a `list` read over each of its record
//! kinds is drafted beside it. The document names the surface its file was
//! observed on.
//!
//! What the file does not state stays a question, where it blocks only what
//! needs it: the key and the types of the key's fields are asked in a
//! drafted read by key, which does not compile until they are answered.
//! The document's rules refuse the rest until someone states it: a record
//! kind of rows with no key is listed and never read by key or written, and
//! a field of the type `untrusted_text` is never written.
//!
//! A value in a JSON document has a type of its own that the file does not
//! name, so a JSON document keeps every type and its key as questions, and
//! no read is drafted over it.
//!
//! An open question is the pack's own words and holds no text from the file:
//! a header or a file name written as an instruction would reach the
//! developer's agent as part of a question. Nothing is drafted for a write;
//! no example and no fixture is drafted.

use std::collections::BTreeSet;

use a2m_core::{Failure, FileKindName, Finding, FindingKind, OperationName};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};

use crate::discover::{CellKind, ColumnFound, FILE_LAYOUT, LayoutFound, invalid};
use crate::model::{DOCUMENT_KIND, Format, MODE, TRUNCATED};

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// A name made from a file's own words: lowercase letters, digits, and
/// underscores, starting with a letter.
fn name_from(text: &str, taken: &mut BTreeSet<String>) -> String {
    let mut name = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            name.push(character.to_ascii_lowercase());
        } else if !name.ends_with('_') && !name.is_empty() {
            name.push('_');
        }
    }
    let mut name = name.trim_end_matches('_').to_owned();
    if !name.starts_with(|first: char| first.is_ascii_lowercase()) {
        name = format!("f_{name}");
    }
    name.truncate(28);
    let mut unique = name.clone();
    let mut number = 2;
    while !taken.insert(unique.clone()) {
        unique = format!("{name}_{number}");
        number += 1;
    }
    unique
}

/// How a drafted field's values are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Typed {
    /// As the built-in type named, and, when `formula`, from the result a
    /// formula kept: section 308 reads a formula's place as its kept
    /// result, and the pack never calculates one.
    As(&'static str, bool),
    /// The file names no type: a JSON value. A question.
    Unknown,
    /// A workbook's column of cells of more than one kind: no one type
    /// reads them all, so the field is left out of the document and the
    /// list, and its type is asked in the read by key.
    LeftOut,
}

impl Typed {
    /// A delimited cell is text. A workbook's cell is of the kind the
    /// workbook stores: text, which is marked untrusted, a number, read as
    /// an exact decimal, or true or false.
    fn of_cells(column: Option<&ColumnFound>) -> Self {
        let Some(column) = column else {
            return Self::As(UNTRUSTED_TEXT, false);
        };
        match column.cells {
            CellKind::Text => Self::As(UNTRUSTED_TEXT, column.formula),
            CellKind::Number => Self::As("decimal", column.formula),
            CellKind::Boolean => Self::As("boolean", column.formula),
            CellKind::Mixed => Self::LeftOut,
        }
    }
}

/// One field as both rules draft it.
struct FieldDraft {
    /// Its name in the document.
    name: String,
    /// Its place within the record: the key that states it, and its value.
    at: (&'static str, DraftValue),
    /// How its values are read.
    typed: Typed,
}

/// One record kind as both rules draft it.
struct KindDraft {
    /// Its name in the document.
    name: String,
    /// Its place in the file: the key that states it, and its value.
    place: (&'static str, DraftValue),
    /// Each field.
    fields: Vec<FieldDraft>,
}

impl KindDraft {
    /// The fields a read returns: every one whose type the file states.
    fn read(&self) -> impl Iterator<Item = (&FieldDraft, &'static str)> {
        self.fields.iter().filter_map(|field| match field.typed {
            Typed::As(ty, _) => Some((field, ty)),
            Typed::Unknown | Typed::LeftOut => None,
        })
    }
}

/// What both rules make of one layout.
struct LayoutDraft {
    /// The document's name: the file's name without its extension.
    document: String,
    format: Format,
    fingerprint: Vec<(&'static str, DraftValue)>,
    kinds: Vec<KindDraft>,
}

impl LayoutDraft {
    fn of(found: &LayoutFound) -> Self {
        let texts = |items: &[String]| {
            DraftValue::List(items.iter().map(|item| DraftValue::text(item)).collect())
        };
        let kind = |name: String,
                    place: (&'static str, DraftValue),
                    headers: &[String],
                    columns: &[ColumnFound]| {
            let json = found.format == Format::Json;
            let mut taken = BTreeSet::new();
            let fields = headers
                .iter()
                .enumerate()
                .filter(|(_, header)| !header.is_empty())
                .map(|(position, header)| {
                    let at = if json {
                        ("path", texts(std::slice::from_ref(header)))
                    } else {
                        ("header", DraftValue::text(header))
                    };
                    FieldDraft {
                        name: name_from(header, &mut taken),
                        at,
                        typed: if json {
                            Typed::Unknown
                        } else {
                            Typed::of_cells(columns.get(position))
                        },
                    }
                })
                .collect();
            KindDraft {
                name,
                place,
                fields,
            }
        };
        let mut taken = BTreeSet::new();
        let mut kinds = Vec::new();
        let mut fingerprint = Vec::new();
        match found.format {
            // Not discovered, so never drafted yet (decision 0246).
            Format::Ini | Format::PropertyList => {}
            Format::Delimited => {
                fingerprint.push(("headers", texts(&found.headers)));
                kinds.push(kind(
                    "rows".to_owned(),
                    ("rows", DraftValue::text("all")),
                    &found.headers,
                    &[],
                ));
            }
            Format::Json => {
                fingerprint.push(("keys", texts(&found.headers)));
                for table in &found.tables {
                    let place = ("path", texts(std::slice::from_ref(&table.name)));
                    kinds.push(kind(
                        name_from(&table.name, &mut taken),
                        place,
                        &table.headers,
                        &[],
                    ));
                }
            }
            Format::Workbook => {
                fingerprint.push(("sheets", texts(&found.sheets)));
                let tables: Vec<String> = found.tables.iter().map(|t| t.name.clone()).collect();
                fingerprint.push(("tables", texts(&tables)));
                fingerprint.push(("names", texts(&found.names)));
                for table in &found.tables {
                    let place = (
                        "table",
                        DraftValue::map(vec![
                            (
                                "sheet",
                                DraftValue::text(table.sheet.as_deref().unwrap_or_default()),
                            ),
                            ("name", DraftValue::text(&table.name)),
                        ]),
                    );
                    kinds.push(kind(
                        name_from(&table.name, &mut taken),
                        place,
                        &table.headers,
                        &table.columns,
                    ));
                }
            }
        }
        let stem = found
            .file
            .rsplit_once('.')
            .map_or(found.file.as_str(), |(stem, _)| stem);
        Self {
            document: name_from(stem, &mut BTreeSet::new()),
            format: found.format,
            fingerprint,
            kinds,
        }
    }

    /// Whether a cell's type is the text it holds: delimited text and a
    /// workbook's tables. A JSON value has a type the file does not name.
    fn as_text(&self) -> bool {
        self.format != Format::Json
    }

    /// The record kind as a description names it: the document, and the
    /// record kind where a file holds several. Both names are the pack's
    /// own spelling of the file's words.
    fn named(&self, kind: &KindDraft) -> String {
        if self.format == Format::Delimited {
            format!("the document `{}`", self.document)
        } else {
            format!(
                "the record kind `{}` of the document `{}`",
                kind.name, self.document
            )
        }
    }

    /// How a read returns the record kind's values, in the pack's words.
    fn values_read(&self, kind: &KindDraft) -> String {
        if self.format == Format::Delimited {
            return "each value as the text the file holds, marked untrusted.".to_owned();
        }
        let mut said = "each value as the workbook stores it: text as the text it holds, marked untrusted, and a number as an exact decimal.".to_owned();
        if kind
            .fields
            .iter()
            .any(|field| matches!(field.typed, Typed::As(_, true)))
        {
            said.push_str(" A field that holds a formula gives the result the workbook kept from its last calculation, which the pack never calculates.");
        }
        said
    }

    /// The object a read over a record kind names: the document, and the
    /// record kind where a file holds several.
    fn object(&self, kind: &KindDraft) -> String {
        if self.format == Format::Delimited {
            self.document.clone()
        } else {
            format!("{}_{}", self.document, kind.name)
        }
    }
}

/// What a list's description says of the fields it left out.
fn left_out(kind: &KindDraft) -> &'static str {
    if kind
        .fields
        .iter()
        .any(|field| field.typed == Typed::LeftOut)
    {
        " A column whose cells are of more than one kind is left out until its type is stated."
    } else {
        ""
    }
}

fn layout_of(finding: &Finding) -> Result<LayoutFound, Failure> {
    serde_json::from_str(finding.found())
        .map_err(|_| invalid("a `file_layout` finding does not hold a layout"))
}

fn layout_kind() -> Result<FindingKind, Failure> {
    FindingKind::new(FILE_LAYOUT).map_err(|_| invalid("its kind is not a name"))
}

/// From a `file_layout` finding to a `documents/` file.
pub struct DocumentFromLayout {
    finding_kind: FindingKind,
}

impl DocumentFromLayout {
    /// The rule.
    ///
    /// # Errors
    ///
    /// Never for the name this crate fixes; a failure of the name check.
    pub fn new() -> Result<Self, Failure> {
        Ok(Self {
            finding_kind: layout_kind()?,
        })
    }
}

impl DraftingRule for DocumentFromLayout {
    fn finding_kind(&self) -> FindingKind {
        self.finding_kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        DOCUMENT_KIND
    }

    /// Drafts the document with its fingerprint and a record kind for each
    /// table the file states. A cell of delimited text or of a workbook is
    /// `untrusted_text`, with no key; a JSON document's types and key are
    /// open questions. Nothing is drafted for a write.
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found = layout_of(finding)?;
        let layout = LayoutDraft::of(&found);
        let mut records = Vec::new();
        for (number, kind) in layout.kinds.iter().enumerate() {
            let number = number + 1;
            let fields = kind
                .fields
                .iter()
                .zip(1_usize..)
                .filter_map(|(drafted, field)| {
                    let mut entry = vec![drafted.at.clone()];
                    match drafted.typed {
                        Typed::As(ty, formula) => {
                            entry.push(("type", DraftValue::text(ty)));
                            if formula {
                                entry.push(("formula", DraftValue::Bool(true)));
                            }
                        }
                        Typed::Unknown => entry.push((
                            "type",
                            cx.open_question(&format!(
                                "Which field type do the values of field {field} of record kind {number} of this draft have?"
                            )),
                        )),
                        Typed::LeftOut => return None,
                    }
                    Some((drafted.name.clone(), DraftValue::map(entry)))
                })
                .collect();
            let mut body = vec![kind.place.clone()];
            if !layout.as_text() {
                body.push((
                    "key",
                    cx.open_question(&format!(
                        "Which fields identify one record of record kind {number} of this draft?"
                    )),
                ));
            }
            body.push(("fields", DraftValue::Map(fields)));
            records.push((kind.name.clone(), DraftValue::map(body)));
        }
        // The surface the file was observed on, so that in a target of
        // several file surfaces the document is not taken for the first's.
        let mut body = Vec::new();
        if let Some(surface) = &found.surface {
            body.push(("surface", DraftValue::text(surface)));
        }
        body.extend([
            ("file", DraftValue::text(&found.file)),
            ("format", DraftValue::text(found.format.as_str())),
        ]);
        if let Some(delimiter) = &found.delimiter {
            body.push((
                "dialect",
                DraftValue::map(vec![("delimiter", DraftValue::text(delimiter))]),
            ));
        }
        body.push(("fingerprint", DraftValue::map(layout.fingerprint)));
        body.push(("records", DraftValue::Map(records)));
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("documents/{}.yaml", layout.document))?,
            body: DraftValue::map(body),
        }])
    }
}

/// From a `file_layout` finding to the reads over the drafted document: a
/// `list` of each record kind, which compiles, and a read by key, which
/// holds the questions of the key.
pub struct ReadsFromLayout {
    finding_kind: FindingKind,
}

impl ReadsFromLayout {
    /// The rule.
    ///
    /// # Errors
    ///
    /// Never for the name this crate fixes; a failure of the name check.
    pub fn new() -> Result<Self, Failure> {
        Ok(Self {
            finding_kind: layout_kind()?,
        })
    }
}

/// An output of the type a drafted cell is read as.
fn output_of(ty: &'static str, list: bool) -> DraftValue {
    let mut output = vec![("type", DraftValue::text(ty))];
    if list {
        output.push(("list", DraftValue::Bool(true)));
    }
    DraftValue::map(output)
}

impl DraftingRule for ReadsFromLayout {
    fn finding_kind(&self) -> FindingKind {
        self.finding_kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let found = layout_of(finding)?;
        // A tool is named after its target; with none there is no name.
        let Some(target) = cx.target() else {
            return Ok(Vec::new());
        };
        let layout = LayoutDraft::of(&found);
        if !layout.as_text() {
            return Ok(Vec::new());
        }
        let mut docs = Vec::new();
        for kind in layout
            .kinds
            .iter()
            .filter(|kind| kind.read().next().is_some())
        {
            let object = layout.object(kind);
            let list_name = format!("{}_{object}_list", target.as_str());
            let get_name = format!("{}_{object}_get", target.as_str());
            if OperationName::new(&list_name).is_err() || OperationName::new(&get_name).is_err() {
                continue;
            }
            let binding = |verb: &str| {
                vec![
                    ("mode", DraftValue::text(MODE)),
                    ("verb", DraftValue::text(verb)),
                    ("document", DraftValue::text(&layout.document)),
                    ("record", DraftValue::text(&kind.name)),
                ]
            };
            // Each field as a list of its values in the file's order, and
            // whether the file held more rows than the list read.
            let mut outputs: Vec<(String, DraftValue)> = kind
                .read()
                .map(|(field, ty)| (field.name.clone(), output_of(ty, true)))
                .collect();
            if !kind.fields.iter().any(|field| field.name == TRUNCATED) {
                outputs.push((
                    TRUNCATED.to_owned(),
                    DraftValue::map(vec![("type", DraftValue::text("boolean"))]),
                ));
            }
            docs.push(DraftDoc {
                path: RelPath::new(&format!("operations/{object}_list.yaml"))?,
                body: DraftValue::map(vec![
                    ("operation", DraftValue::text(&list_name)),
                    ("effect", DraftValue::text("read")),
                    ("inputs", DraftValue::Map(Vec::new())),
                    ("outputs", DraftValue::Map(outputs)),
                    ("bindings", DraftValue::List(vec![DraftValue::map(binding("list"))])),
                    (
                        "description",
                        DraftValue::text(&format!(
                            "Lists the rows of {} in the order the file holds them, as many as the list limit allows, {}{} Drafted by rule from the file's layout; its name is provisional.",
                            layout.named(kind),
                            layout.values_read(kind),
                            left_out(kind),
                        )),
                    ),
                ]),
            });
            // The read by key: what the file does not state is asked here,
            // and nothing else waits on it.
            let mut get_binding = binding("read");
            get_binding.push((
                "key",
                cx.open_question(
                    "Which input gives each key field of the record kind this read names?",
                ),
            ));
            docs.push(DraftDoc {
                path: RelPath::new(&format!("operations/{object}_get.yaml"))?,
                body: DraftValue::map(vec![
                    ("operation", DraftValue::text(&get_name)),
                    ("effect", DraftValue::text("read")),
                    (
                        "key",
                        cx.open_question(
                            "Which fields identify one record of the record kind this read names? State the same fields under `key` of the record kind in its document.",
                        ),
                    ),
                    (
                        "inputs",
                        cx.open_question(
                            "Which field type does each key field have? A key is of a type someone states, such as a number, a date, a decimal, or an identifier, and never `untrusted_text`; state it in the document too.",
                        ),
                    ),
                    (
                        "outputs",
                        DraftValue::Map(
                            kind.fields
                                .iter()
                                .zip(1_usize..)
                                .map(|(field, number)| match field.typed {
                                    Typed::As(ty, _) => (field.name.clone(), output_of(ty, false)),
                                    // A column no one type reads is asked
                                    // here, where nothing else waits on it.
                                    Typed::Unknown | Typed::LeftOut => (
                                        field.name.clone(),
                                        cx.open_question(&format!(
                                            "Field {number} of the record kind this read names holds cells of more than one kind, and was left out of the document and the list. Which field type do its values have? State it in the document too."
                                        )),
                                    ),
                                })
                                .collect(),
                        ),
                    ),
                    ("bindings", DraftValue::List(vec![DraftValue::map(get_binding)])),
                    (
                        "description",
                        DraftValue::text(&format!(
                            "Reads one row of {} by its key, {} Drafted by rule from the file's layout; its name is provisional, and its key is still to be stated.",
                            layout.named(kind),
                            layout.values_read(kind),
                        )),
                    ),
                ]),
            });
        }
        Ok(docs)
    }
}
