//! Drafting: from a `file_layout` finding to a `documents/` file, and to the
//! reads over it, by rule.
//!
//! A cell of delimited text or of a workbook's table holds text, whatever a
//! person later says it means. A field there whose type nobody stated is
//! drafted as the built-in `untrusted_text`: what the file holds, marked as
//! the system's words, and no guess. A key is not needed to list. So the
//! document drafted for such a file holds no open question and compiles,
//! and a `list` read over each of its record kinds is drafted beside it.
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

use crate::discover::{FILE_LAYOUT, LayoutFound, invalid};
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

/// One record kind as both rules draft it.
struct KindDraft {
    /// Its name in the document.
    name: String,
    /// Its place in the file: the key that states it, and its value.
    place: (&'static str, DraftValue),
    /// Each field: its name, and how it is found.
    fields: Vec<(String, (&'static str, DraftValue))>,
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
        let kind = |name: String, place: (&'static str, DraftValue), headers: &[String]| {
            let json = found.format == Format::Json;
            let mut taken = BTreeSet::new();
            let fields = headers
                .iter()
                .filter(|header| !header.is_empty())
                .map(|header| {
                    let at = if json {
                        ("path", texts(std::slice::from_ref(header)))
                    } else {
                        ("header", DraftValue::text(header))
                    };
                    (name_from(header, &mut taken), at)
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
            Format::Delimited => {
                fingerprint.push(("headers", texts(&found.headers)));
                kinds.push(kind(
                    "rows".to_owned(),
                    ("rows", DraftValue::text("all")),
                    &found.headers,
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
                .map(|((name, at), field)| {
                    let field_type = if layout.as_text() {
                        DraftValue::text(UNTRUSTED_TEXT)
                    } else {
                        cx.open_question(&format!(
                            "Which field type do the values of field {field} of record kind {number} of this draft have?"
                        ))
                    };
                    (
                        name.clone(),
                        DraftValue::map(vec![at.clone(), ("type", field_type)]),
                    )
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
        let mut body = vec![
            ("file", DraftValue::text(&found.file)),
            ("format", DraftValue::text(found.format.as_str())),
        ];
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

/// An output of the type a drafted cell holds.
fn text_output(list: bool) -> DraftValue {
    let mut output = vec![("type", DraftValue::text(UNTRUSTED_TEXT))];
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
        for kind in layout.kinds.iter().filter(|kind| !kind.fields.is_empty()) {
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
                .fields
                .iter()
                .map(|(name, _)| (name.clone(), text_output(true)))
                .collect();
            if !kind.fields.iter().any(|(name, _)| name == TRUNCATED) {
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
                        DraftValue::text(
                            "Lists the rows of one file in the order the file holds them, as many as the list limit allows, each value as the text the file holds, marked untrusted. Drafted by rule from the file's layout; its name is provisional.",
                        ),
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
                                .map(|(name, _)| (name.clone(), text_output(false)))
                                .collect(),
                        ),
                    ),
                    ("bindings", DraftValue::List(vec![DraftValue::map(get_binding)])),
                    (
                        "description",
                        DraftValue::text(
                            "Reads one row of one file by its key, each value as the text the file holds, marked untrusted. Drafted by rule from the file's layout; its name is provisional, and its key is still to be stated.",
                        ),
                    ),
                ]),
            });
        }
        Ok(docs)
    }
}
