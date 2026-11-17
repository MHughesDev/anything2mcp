//! The `documents/` file kind: its schema, and how a document compiles into
//! the pack's section of the plan.

use a2m_core::{FileKindName, LintCode, PackPayload, SurfaceKind};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind, JsonSchema,
    KindContext, Located, Stage,
};

use crate::model::{
    DOCUMENT_KIND, DocumentSpec, FieldSpec, Format, Layout, PlaceSpec, RecordSpec, SECTION_FORMAT,
    SURFACE_KIND, Section,
};

/// A document that states something the pack cannot carry out.
pub const DOCUMENT_INVALID: LintCode = LintCode::fixed("file.document_invalid");
/// A format the pack names and does not read yet.
pub const FORMAT_NOT_BUILT: LintCode = LintCode::fixed("file.format_not_built");

/// The name a document is known by: the name of its file without the
/// extension. One file holds one document.
#[must_use]
pub fn document_name(doc: &Located<Doc>) -> &str {
    doc.at.file.stem()
}

fn field_schema() -> JsonSchema {
    JsonSchema::object()
        .optional("header", JsonSchema::text())
        .optional("path", JsonSchema::non_empty_list(JsonSchema::text()))
        .required("type", JsonSchema::field_type())
        .optional("writable", JsonSchema::bool())
        .optional("formula", JsonSchema::bool())
}

fn record_schema() -> JsonSchema {
    JsonSchema::object()
        .optional(
            "table",
            JsonSchema::object()
                .required("sheet", JsonSchema::text())
                .required("name", JsonSchema::text()),
        )
        .optional("sheet", JsonSchema::text())
        .optional(
            "cell",
            JsonSchema::object().required("name", JsonSchema::text()),
        )
        .optional("path", JsonSchema::list(JsonSchema::text()))
        .optional("rows", JsonSchema::words(&["all"]))
        .optional("key", JsonSchema::non_empty_list(JsonSchema::name()))
        .required("fields", JsonSchema::map_of(field_schema()))
}

/// The schema of one document under `documents/`.
#[must_use]
pub fn document_schema() -> JsonSchema {
    let names = || JsonSchema::list(JsonSchema::text());
    JsonSchema::object()
        .optional("surface", JsonSchema::surface())
        .required("file", JsonSchema::text())
        .required("format", JsonSchema::words(Format::NAMES))
        .optional(
            "dialect",
            JsonSchema::object()
                .optional("delimiter", JsonSchema::text())
                .optional("header", JsonSchema::bool())
                .optional("encoding", JsonSchema::words(&["utf-8"]))
                .optional("line_ending", JsonSchema::words(&["lf", "crlf"])),
        )
        .optional(
            "fingerprint",
            JsonSchema::object()
                .optional("sheets", names())
                .optional("tables", names())
                .optional("names", names())
                .optional("headers", names())
                .optional("keys", names()),
        )
        .optional("may_be_open", JsonSchema::bool())
        .optional(
            "lock_signal",
            JsonSchema::object().required("sibling", JsonSchema::text()),
        )
        .required("records", JsonSchema::map_of(record_schema()))
}

/// The `document` file kind.
pub struct DocumentKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl DocumentKind {
    /// The file kind, for `file` surfaces.
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema: document_schema(),
            kinds: vec![SURFACE_KIND],
        }
    }
}

impl Default for DocumentKind {
    fn default() -> Self {
        Self::new()
    }
}

fn invalid(at: &Located<Doc>, message: impl Into<String>, fix: &str) -> Diagnostic {
    Diagnostic::error(DOCUMENT_INVALID, Stage::Lower, &at.at, message, fix)
}

/// Whether the text is one file name: no folder, no step out, no drive, no
/// placeholder.
fn is_one_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '{', '}', '*', '?'])
        && !name.chars().any(char::is_control)
}

fn texts(doc: Option<&Located<Doc>>) -> Option<Vec<String>> {
    doc.map(|list| {
        list.items()
            .iter()
            .filter_map(|item| item.as_text().map(str::to_owned))
            .collect()
    })
}

fn place_of(
    record: &Located<Doc>,
    format: Format,
    problems: &mut Diagnostics,
) -> Option<PlaceSpec> {
    let stated: Vec<&str> = ["table", "sheet", "cell", "path", "rows"]
        .into_iter()
        .filter(|key| record.get(key).is_some())
        .collect();
    let [one] = stated.as_slice() else {
        problems.push(invalid(
            record,
            "A record kind states exactly one place: `table`, `sheet`, `cell`, `path`, or `rows`.",
            "Keep the one place the records live in.",
        ));
        return None;
    };
    let fits = match *one {
        "rows" => format == Format::Delimited,
        "path" => format == Format::Json,
        _ => format == Format::Workbook,
    };
    if !fits {
        problems.push(invalid(
            record,
            format!(
                "A `{one}` is not a place in a file of the format `{}`.",
                format.as_str()
            ),
            "Use `rows` for delimited text, `path` for JSON, and `table`, `sheet`, or `cell` for a workbook.",
        ));
        return None;
    }
    Some(match *one {
        "rows" => PlaceSpec::Rows,
        "path" => PlaceSpec::Path {
            keys: texts(record.get("path")).unwrap_or_default(),
        },
        "table" => {
            let table = record.get("table")?;
            PlaceSpec::Table {
                sheet: table.text("sheet")?.to_owned(),
                name: table.text("name")?.to_owned(),
            }
        }
        "sheet" => PlaceSpec::Sheet {
            sheet: record.text("sheet")?.to_owned(),
        },
        _ => PlaceSpec::Cell {
            name: record.get("cell")?.text("name")?.to_owned(),
        },
    })
}

fn record_of(
    record: &Located<Doc>,
    format: Format,
    cx: &KindContext<'_>,
    problems: &mut Diagnostics,
) -> Option<RecordSpec> {
    let place = place_of(record, format, problems)?;
    let key = texts(record.get("key")).unwrap_or_default();
    let columns = matches!(
        place,
        PlaceSpec::Rows | PlaceSpec::Table { .. } | PlaceSpec::Sheet { .. }
    );
    let mut fields = Vec::new();
    let before = problems.len();
    for entry in record
        .get("fields")
        .map(Located::entries)
        .unwrap_or_default()
    {
        let field = &entry.value;
        let header = field.text("header").map(str::to_owned);
        let path = texts(field.get("path")).unwrap_or_default();
        let formula = field.get("formula").and_then(Located::as_bool) == Some(true);
        let writable = field.get("writable").and_then(Located::as_bool) == Some(true);
        if columns && (header.is_none() || !path.is_empty()) {
            problems.push(invalid(
                field,
                "A field of a table, a sheet, or rows is found by the text of its `header`.",
                "Write `header:` with the column's header text, and no `path`.",
            ));
        }
        if matches!(place, PlaceSpec::Path { .. }) && (path.is_empty() || header.is_some()) {
            problems.push(invalid(
                field,
                "A field of a record in a JSON document is found by its `path` inside the record.",
                "Write `path:` with the list of keys that lead to the value, and no `header`.",
            ));
        }
        if matches!(place, PlaceSpec::Cell { .. }) && (header.is_some() || !path.is_empty()) {
            problems.push(invalid(
                field,
                "The field of a named cell is the cell itself.",
                "Remove `header` and `path`.",
            ));
        }
        if formula && (writable || format != Format::Workbook) {
            problems.push(invalid(
                field,
                "A place that holds a formula is in a workbook and is never written.",
                "Remove `writable`, or remove `formula`.",
            ));
        }
        let Some(field_type) = field
            .text("type")
            .and_then(|name| cx.target().field_type(name))
        else {
            continue;
        };
        fields.push(FieldSpec {
            name: entry.key.value.clone(),
            header,
            path,
            field_type: u32::try_from(field_type.index.get()).unwrap_or(u32::MAX),
            writable,
            formula,
        });
    }
    if fields.is_empty() {
        problems.push(invalid(
            record,
            "A record kind declares at least one field.",
            "Add a field under `fields`.",
        ));
    }
    if matches!(place, PlaceSpec::Cell { .. }) && (fields.len() != 1 || !key.is_empty()) {
        problems.push(invalid(
            record,
            "A named cell is one place: it has one field and no key.",
            "Declare one field, and remove `key`.",
        ));
    }
    if columns && key.is_empty() {
        problems.push(invalid(
            record,
            "The rows of a table, a sheet, or delimited text are told apart by a key.",
            "Add `key:` with the fields that identify one row.",
        ));
    }
    for name in &key {
        let known = fields.iter().find(|field| &field.name == name);
        if known.is_none_or(|field| field.writable || field.formula) {
            problems.push(invalid(
                record,
                format!("The key field `{name}` is a declared field that is neither writable nor a formula."),
                "Name declared fields under `key`, and do not mark them `writable`.",
            ));
        }
    }
    (problems.len() == before).then_some(RecordSpec { place, key, fields })
}

fn document_of(
    doc: &Located<Doc>,
    cx: &KindContext<'_>,
    problems: &mut Diagnostics,
) -> Option<DocumentSpec> {
    let before = problems.len();
    let surface = cx.surface_of(doc);
    if surface.is_none() {
        problems.push(invalid(
            doc,
            "The document is on no surface of the kind `file`.",
            "Declare a surface of the kind `file` in `target.yaml`, and name it under `surface`.",
        ));
    }
    let file_doc = doc.get("file").unwrap_or(doc);
    let file = file_doc.as_text().unwrap_or_default();
    if !is_one_name(file) {
        problems.push(invalid(
            file_doc,
            "A document's file is one name inside its surface's folder.",
            "Write the file's name with no folder, no `..`, and no placeholder.",
        ));
    }
    let format_doc = doc.get("format").unwrap_or(doc);
    let format_name = format_doc.as_text().unwrap_or_default();
    let Some(format) = Format::from_name(format_name) else {
        problems.push(Diagnostic::error(
            FORMAT_NOT_BUILT,
            Stage::Lower,
            &format_doc.at,
            format!("The file pack does not read the format `{format_name}` yet."),
            "Use `delimited`, `json`, or `workbook`, or leave the file out until the format is built.",
        ));
        return None;
    };
    let dialect = doc.get("dialect");
    let delimiter = dialect
        .and_then(|dialect| dialect.text("delimiter"))
        .map(str::to_owned);
    let one_ascii = |text: &String| {
        text.len() == 1 && text.is_ascii() && !matches!(text.as_str(), "\"" | "\r" | "\n")
    };
    if dialect.is_some() && format != Format::Delimited {
        problems.push(invalid(
            doc,
            "Only delimited text has a dialect.",
            "Remove `dialect`.",
        ));
    }
    if delimiter.as_ref().is_some_and(|text| !one_ascii(text)) {
        problems.push(invalid(
            doc,
            "A delimiter is one ASCII character that is not a quote or a line break.",
            "Write the one character that stands between fields, such as `\";\"`.",
        ));
    }
    if dialect
        .and_then(|dialect| dialect.get("header"))
        .and_then(Located::as_bool)
        == Some(false)
    {
        problems.push(invalid(
            doc,
            "Delimited text without a header row has no named place.",
            "Give the file a header row, or leave it out.",
        ));
    }
    let lock_sibling = doc
        .get("lock_signal")
        .and_then(|signal| signal.text("sibling"))
        .map(str::to_owned);
    if lock_sibling
        .as_ref()
        .is_some_and(|name| !is_one_name(name) || name == file)
    {
        problems.push(invalid(
            doc,
            "A lock signal is another file's name in the same folder.",
            "Write the sibling's name with no folder and no placeholder.",
        ));
    }
    let print = doc.get("fingerprint");
    let list = |key: &str| texts(print.and_then(|print| print.get(key)));
    let mut records = std::collections::BTreeMap::new();
    for entry in doc.get("records").map(Located::entries).unwrap_or_default() {
        if let Some(record) = record_of(&entry.value, format, cx, problems) {
            records.insert(entry.key.value.clone(), record);
        }
    }
    (problems.len() == before).then(|| DocumentSpec {
        surface: surface
            .map(|surface| surface.name.as_str().to_owned())
            .unwrap_or_default(),
        file: file.to_owned(),
        format,
        delimiter,
        line_ending: dialect
            .and_then(|dialect| dialect.text("line_ending"))
            .map(str::to_owned),
        layout: Layout {
            sheets: list("sheets"),
            tables: list("tables"),
            names: list("names"),
            headers: list("headers"),
            keys: list("keys"),
        },
        may_be_open: doc.get("may_be_open").and_then(Located::as_bool) == Some(true),
        lock_sibling,
        records,
    })
}

impl FileKind for DocumentKind {
    fn name(&self) -> FileKindName {
        DOCUMENT_KIND
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        "documents"
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        vec![Located::new(
            DefinedName {
                name: document_name(doc).to_owned(),
            },
            doc.at.clone(),
        )]
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut problems = Diagnostics::new();
        let mut section = Section::new();
        for doc in docs {
            if let Some(document) = document_of(doc, cx, &mut problems) {
                section.insert(document_name(doc).to_owned(), document);
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        PackPayload::encode(&section)
            .map(|payload| CompiledSection {
                format: SECTION_FORMAT,
                payload,
            })
            .map_err(|failure| {
                let at = docs.first().map_or_else(
                    || a2m_files::Place::start_of(&a2m_files::RelPath::target_file()),
                    |doc| doc.at.clone(),
                );
                Diagnostics::from(Diagnostic::error(
                    DOCUMENT_INVALID,
                    Stage::Lower,
                    &at,
                    failure.message().to_owned(),
                    "Keep the documents to names, texts, and whole numbers.",
                ))
            })
    }
}
