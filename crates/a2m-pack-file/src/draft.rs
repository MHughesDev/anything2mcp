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
//! A value in a JSON document has a type of its own, which the file does
//! state: text is `untrusted_text`, a number `decimal`, true or false
//! `boolean`. A key whose values are of more than one kind, are lists or
//! records, or are nowhere set keeps its type as a question; so does the key
//! of a list of records, which the file does not state. No read is drafted
//! over a JSON document.
//!
//! A settings file names one-value places by a path of keys (decision
//! 0246). Its document has a record kind for the keys before the first
//! section of an INI file, for each section, and for each dictionary of a
//! property list that holds a value, and a field for each value of a shape
//! the pack reads as one value. An INI value is text, whatever it spells,
//! and is drafted `untrusted_text`; a property list states each value's
//! element, so a `string` is `untrusted_text`, an `integer` or a `real` a
//! `decimal`, and `true` or `false` a `boolean`. A value whose key names a
//! secret or that has the form of a credential is left out, so that no
//! drafted read ever gives it to a caller (rule 4); so are a value whose
//! form programs read in more than one way, a key the file holds twice, a
//! list, a date, and data. Beside the document, one `read` of each record
//! kind, which compiles: a settings record has no key to ask (decision
//! 0255).
//!
//! An open question is the pack's own words and holds no text from the file:
//! a header or a file name written as an instruction would reach the
//! developer's agent as part of a question. Nothing is drafted for a write;
//! no example and no fixture is drafted.

use std::collections::BTreeSet;

use a2m_core::{Failure, FileKindName, Finding, FindingKind, OperationName};
use a2m_files::{
    DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT, names_a_secret,
};

use crate::discover::{CellKind, ColumnFound, FILE_LAYOUT, LayoutFound, invalid};
use crate::evidence::ValueShape;
use crate::model::{DOCUMENT_KIND, Format, MODE, TRUNCATED};

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// A name made from a file's own words: lowercase letters, digits, and
/// underscores, starting with a letter.
pub(crate) fn name_from(text: &str, taken: &mut BTreeSet<String>) -> String {
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

impl Typed {
    /// A JSON value has a type of its own: text is `untrusted_text`, a
    /// number an exact decimal, and true or false a boolean. What no type
    /// reads (values of more than one kind, a list or a record inside, a
    /// key that holds nothing anywhere, and a finding made before the kinds
    /// were carried) stays a question (finding 26 of copy test 4).
    fn of_json(column: Option<&ColumnFound>) -> Self {
        match (column, Self::of_cells(column)) {
            (None, _) | (_, Self::LeftOut) => Self::Unknown,
            (_, typed) => typed,
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
    /// The names of the columns left out because their names are a secret's,
    /// as the fields they would have been (decision 0302).
    secrets: Vec<String>,
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
            // A header named as a secret, as the file writes it or as the
            // field it becomes, is never a field: no read gives its values.
            let mut secrets = Vec::new();
            let fields = headers
                .iter()
                .enumerate()
                .filter(|(_, header)| !header.is_empty())
                .filter(|(_, header)| {
                    let field = name_from(header, &mut BTreeSet::new());
                    let secret = names_a_secret(header) || names_a_secret(&field);
                    if secret {
                        secrets.push(field);
                    }
                    !secret
                })
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
                            Typed::of_json(columns.get(position))
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
                secrets,
            }
        };
        let mut taken = BTreeSet::new();
        let mut kinds = Vec::new();
        let mut fingerprint = Vec::new();
        match found.format {
            Format::Ini | Format::PropertyList => {
                if found.format == Format::Ini {
                    fingerprint.push(("sections", texts(&found.sections)));
                } else {
                    fingerprint.push(("keys", texts(&found.headers)));
                }
                // One record kind for each path above a value read, in the
                // order the file first holds it.
                let mut records: Vec<(Vec<String>, KindDraft, BTreeSet<String>)> = Vec::new();
                for setting in &found.settings {
                    let Some(ty) = settings_type(found.format, setting.shape) else {
                        continue;
                    };
                    let Some((last, above)) = setting.path.split_last() else {
                        continue;
                    };
                    let at = match records.iter().position(|(path, ..)| path == above) {
                        Some(at) => at,
                        None => {
                            let name = if above.is_empty() {
                                name_from("general", &mut taken)
                            } else {
                                name_from(&above.join(" "), &mut taken)
                            };
                            records.push((
                                above.to_vec(),
                                KindDraft {
                                    name,
                                    place: ("path", texts(above)),
                                    fields: Vec::new(),
                                    secrets: Vec::new(),
                                },
                                BTreeSet::new(),
                            ));
                            records.len() - 1
                        }
                    };
                    if let Some((_, kind, names)) = records.get_mut(at) {
                        kind.fields.push(FieldDraft {
                            name: name_from(last, names),
                            at: ("path", texts(std::slice::from_ref(last))),
                            typed: Typed::As(ty, false),
                        });
                    }
                }
                kinds.extend(records.into_iter().map(|(_, kind, _)| kind));
            }
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
                        &table.columns,
                    ));
                }
            }
            Format::Workbook => {
                fingerprint.push(("sheets", texts(&found.sheets)));
                // A sheet's rows under its header row are no table the
                // workbook defines, and are not in its fingerprint.
                let tables: Vec<String> = found
                    .tables
                    .iter()
                    .filter(|table| !table.on_sheet)
                    .map(|table| table.name.clone())
                    .collect();
                fingerprint.push(("tables", texts(&tables)));
                fingerprint.push(("names", texts(&found.names)));
                for table in &found.tables {
                    let sheet = table.sheet.as_deref().unwrap_or_default();
                    let place = if table.on_sheet {
                        ("sheet", DraftValue::text(sheet))
                    } else {
                        (
                            "table",
                            DraftValue::map(vec![
                                ("sheet", DraftValue::text(sheet)),
                                ("name", DraftValue::text(&table.name)),
                            ]),
                        )
                    };
                    kinds.push(kind(
                        name_from(&table.name, &mut taken),
                        place,
                        &table.headers,
                        &table.columns,
                    ));
                }
            }
        }
        // A settings file is named by its whole name: a settings file and a
        // data file of one stem stand side by side often.
        let stem = if found.format.is_settings() {
            found.file.as_str()
        } else {
            found
                .file
                .rsplit_once('.')
                .map_or(found.file.as_str(), |(stem, _)| stem)
        };
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

    /// Whether a record kind is found by a key the file does not state: a
    /// JSON document's. A settings record is one place, with no key.
    fn asks_key(&self) -> bool {
        self.format == Format::Json
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
        if self.format == Format::Ini {
            return "each value as the text the file holds, marked untrusted.".to_owned();
        }
        if self.format == Format::PropertyList {
            return "each value as the file's element states it: a string as the text it holds, marked untrusted, a number as an exact decimal, and true or false as a boolean.".to_owned();
        }
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

/// The type a settings value of a shape is drafted as, where the shape is
/// one value the pack reads. Nothing for a value withheld, unread, a list,
/// a record, a date, or data.
fn settings_type(format: Format, shape: ValueShape) -> Option<&'static str> {
    match (format, shape) {
        (Format::Ini, ValueShape::Text | ValueShape::Number | ValueShape::Boolean)
        | (_, ValueShape::Text | ValueShape::Empty) => Some(UNTRUSTED_TEXT),
        (Format::PropertyList, ValueShape::Number) => Some("decimal"),
        (Format::PropertyList, ValueShape::Boolean) => Some("boolean"),
        _ => None,
    }
}

/// What a settings read's description says of the values it left out.
fn settings_left_out(found: &LayoutFound) -> &'static str {
    if found
        .settings
        .iter()
        .any(|setting| settings_type(found.format, setting.shape).is_none())
    {
        " A value whose key names a secret or that has the form of a credential is never drafted, nor is a value whose form programs read in more than one way, a key the file holds twice, a list, a date, or data."
    } else {
        ""
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
            if layout.asks_key() {
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
        if let Some(separator) = &found.separator {
            body.push((
                "dialect",
                DraftValue::map(vec![("separator", DraftValue::text(separator))]),
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
        if found.format.is_settings() {
            return settings_reads(&found, &layout, target.as_str());
        }
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
                            "Lists the rows of {} in the order the file holds them, as many as the list limit allows, {}{} Drafted by rule from the file's layout; its name is provisional.{}",
                            layout.named(kind),
                            layout.values_read(kind),
                            left_out(kind),
                            a2m_files::left_out_sentence(&kind.secrets),
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
                            "Reads one row of {} by its key, {} Drafted by rule from the file's layout; its name is provisional, and its key is still to be stated.{}",
                            layout.named(kind),
                            layout.values_read(kind),
                            a2m_files::left_out_sentence(&kind.secrets),
                        )),
                    ),
                ]),
            });
        }
        Ok(docs)
    }
}

/// One `read` of each record kind of a settings document: every field the
/// document holds, as the type it is drafted as. A settings record is one
/// place, so the read takes no input and holds no question.
fn settings_reads(
    found: &LayoutFound,
    layout: &LayoutDraft,
    target: &str,
) -> Result<Vec<DraftDoc>, Failure> {
    let mut docs = Vec::new();
    for kind in layout
        .kinds
        .iter()
        .filter(|kind| kind.read().next().is_some())
    {
        let object = layout.object(kind);
        let name = format!("{target}_{object}_get");
        if OperationName::new(&name).is_err() {
            continue;
        }
        let outputs = kind
            .read()
            .map(|(field, ty)| (field.name.clone(), output_of(ty, false)))
            .collect();
        docs.push(DraftDoc {
            path: RelPath::new(&format!("operations/{object}_get.yaml"))?,
            body: DraftValue::map(vec![
                ("operation", DraftValue::text(&name)),
                ("effect", DraftValue::text("read")),
                ("inputs", DraftValue::Map(Vec::new())),
                ("outputs", DraftValue::Map(outputs)),
                (
                    "bindings",
                    DraftValue::List(vec![DraftValue::map(vec![
                        ("mode", DraftValue::text(MODE)),
                        ("verb", DraftValue::text("read")),
                        ("document", DraftValue::text(&layout.document)),
                        ("record", DraftValue::text(&kind.name)),
                    ])]),
                ),
                (
                    "description",
                    DraftValue::text(&format!(
                        "Reads the values of {}, {}{} A key the file no longer holds is read as nothing. Drafted by rule from the file's keys; its name is provisional.",
                        layout.named(kind),
                        layout.values_read(kind),
                        settings_left_out(found),
                    )),
                ),
            ]),
        });
    }
    Ok(docs)
}
