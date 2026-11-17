//! Drafting: from a `table` finding to a `statements/` file and to the
//! read operations over it, by rule.
//!
//! The statements file holds one record for the table and, when the catalog
//! states a key, one read by that key and one list of every row in the
//! order of the key, bounded by the pack's row limit. The operations are a
//! read by key and that list, drafted only where the record compiles: a
//! key and a type for every column. A draft carries no unit, no business
//! name, and no key the store did not state: where the catalog is silent
//! the draft holds an open question, which the compiler refuses until a
//! person answers it. No write is ever drafted, and no value of a row is
//! kept: discovery reads no row, so a read by key has no example.

use a2m_core::{
    Failure, FailureClass, FailureCode, FileKindName, Finding, FindingKind, OperationName,
};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};

use crate::discover::{ColumnFound, TableFound};
use crate::names::{FOLDER, STATEMENT};
use crate::store::opener;

/// The time limit a drafted read is given.
const DRAFT_TIME_LIMIT: &str = "2s";
/// The time limit a drafted list is given.
const DRAFT_LIST_TIME_LIMIT: &str = "5s";
/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// From `table` findings to `statements/` files.
pub(crate) struct TableToStatements {
    pub(crate) kind: FindingKind,
    /// The most rows a drafted list reads: the pack's `max_rows`.
    pub(crate) max_rows: u32,
}

/// From `table` findings to the read operations over a drafted record.
pub(crate) struct TableToReads {
    pub(crate) kind: FindingKind,
}

/// What both rules make of a table: the record's name, each column's field
/// name and type, and the columns of the key in its order.
struct Shaped {
    record: String,
    names: Vec<String>,
    types: Vec<Option<&'static str>>,
    key: Vec<usize>,
}

impl Shaped {
    fn of(table: &TableFound) -> Self {
        let record = plain_name(&table.table, "t_");
        // One field per column, named from the column's own words.
        let mut names: Vec<String> = Vec::new();
        for (position, column) in table.columns.iter().enumerate() {
            let mut name = plain_name(&column.name, "c_");
            if names.contains(&name) {
                name = format!("{name}_{position}");
            }
            names.push(name);
        }
        let mut key: Vec<(u32, usize)> = table
            .columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.key_position > 0)
            .map(|(position, column)| (column.key_position, position))
            .collect();
        key.sort_unstable();
        Self {
            record,
            types: table.columns.iter().map(field_type).collect(),
            names,
            key: if table.view {
                Vec::new()
            } else {
                key.into_iter().map(|(_, position)| position).collect()
            },
        }
    }

    /// The list of every row, in the order of the key: the dialect's own
    /// quoting of each name the catalog gave. `None` where a name cannot be
    /// quoted.
    fn list_text(&self, table: &TableFound) -> Option<String> {
        let opener = opener(&table.store)?;
        let columns: Option<Vec<String>> = table
            .columns
            .iter()
            .map(|column| opener.quote(&column.name))
            .collect();
        let order: Option<Vec<String>> = self
            .key
            .iter()
            .map(|position| opener.quote(&table.columns.get(*position)?.name))
            .collect();
        Some(format!(
            "SELECT {} FROM {} ORDER BY {}",
            columns?.join(", "),
            opener.quote(&table.table)?,
            order?.join(", ")
        ))
    }
}

/// A name of the store's as a name a target file may hold: lowercase
/// letters, digits, and underscores, starting with a letter.
fn plain_name(text: &str, fallback: &str) -> String {
    let mut name = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
        } else if !name.ends_with('_') {
            name.push('_');
        }
    }
    let name = name.trim_matches('_');
    if name.starts_with(|c: char| c.is_ascii_lowercase()) {
        name.to_owned()
    } else {
        format!("{fallback}{name}")
    }
}

/// The field type a column's declared type states, when it states one. A
/// type is never guessed from a column's name, and no unit is ever drafted.
fn field_type(column: &ColumnFound) -> Option<&'static str> {
    let declared = column.declared_type.to_ascii_uppercase();
    if declared.contains("INT") {
        Some("integer")
    } else if declared.contains("CHAR") || declared.contains("CLOB") || declared.contains("TEXT") {
        Some("text")
    } else if declared == "DATE" {
        Some("date")
    } else if declared == "TIMESTAMP" || declared == "DATETIME" {
        Some("datetime")
    } else {
        None
    }
}

fn unreadable() -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::FINDING_INVALID,
        "A `table` finding does not hold a table as the data pack writes one.",
        "Run discovery again with the data pack that is enabled now.",
    )
}

impl DraftingRule for TableToStatements {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        STATEMENT
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let table: TableFound = serde_json::from_str(finding.found()).map_err(|_| unreadable())?;
        // A finding of a store this build does not speak to is not drafted.
        opener(&table.store).ok_or_else(unreadable)?;
        let shaped = Shaped::of(&table);
        let (record, names) = (&shaped.record, &shaped.names);
        // The type the store states, or a question where it states none.
        let type_of = |position: usize| -> DraftValue {
            shaped.types.get(position).copied().flatten().map_or_else(
                || {
                    cx.open_question(&format!(
                        "Which field type reads the field `{}` of `{record}`? The store declares no type the pack knows.",
                        names.get(position).map_or("", String::as_str)
                    ))
                },
                DraftValue::text,
            )
        };
        let fields: Vec<(String, DraftValue)> = table
            .columns
            .iter()
            .zip(names)
            .enumerate()
            .map(|(position, (column, name))| {
                (
                    name.clone(),
                    DraftValue::map(vec![
                        ("column", DraftValue::text(&column.name)),
                        ("type", type_of(position)),
                    ]),
                )
            })
            .collect();
        let keyed = !shaped.key.is_empty();
        let key_value = if keyed {
            DraftValue::List(
                shaped
                    .key
                    .iter()
                    .filter_map(|position| names.get(*position))
                    .map(|name| DraftValue::text(name))
                    .collect(),
            )
        } else {
            cx.open_question(&format!(
                "Which fields identify one row of `{record}`? The store declares no key for it."
            ))
        };
        let mut statements = Vec::new();
        if keyed {
            // The read of one record by its key is written by the pack when
            // the file is compiled, from the record above: the draft holds
            // no statement text, and the read can read a write back.
            statements.push((
                format!("{record}_by_key"),
                DraftValue::map(vec![
                    ("effect", DraftValue::text("read")),
                    ("form", DraftValue::text("table")),
                    ("record", DraftValue::text(record)),
                    ("time_limit", DraftValue::text(DRAFT_TIME_LIMIT)),
                ]),
            ));
            // Every row, in the order of the key, as many as the pack's
            // limit lets one read return: the catalog's own names, quoted
            // by the store's dialect, and nothing from an input.
            if let Some(text) = shaped.list_text(&table) {
                statements.push((
                    format!("{record}_list"),
                    DraftValue::map(vec![
                        ("effect", DraftValue::text("read")),
                        ("record", DraftValue::text(record)),
                        ("text", DraftValue::text(&text)),
                        ("params", DraftValue::List(Vec::new())),
                        ("returns", DraftValue::text("many")),
                        ("max_rows", DraftValue::Int(i64::from(self.max_rows))),
                        ("time_limit", DraftValue::text(DRAFT_LIST_TIME_LIMIT)),
                    ]),
                ));
            }
        }
        let record_body = DraftValue::map(vec![
            ("table", DraftValue::text(&table.table)),
            ("key", key_value),
            ("fields", DraftValue::Map(fields)),
        ]);
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("{FOLDER}/{record}.yaml"))?,
            body: DraftValue::map(vec![
                ("surface", DraftValue::text(&table.surface)),
                (
                    "records",
                    DraftValue::Map(vec![(record.clone(), record_body)]),
                ),
                ("statements", DraftValue::Map(statements)),
            ]),
        }])
    }
}

/// The output type of a field: text the store holds is untrusted.
fn output_type(field_type: &str) -> &str {
    if field_type == "text" {
        UNTRUSTED_TEXT
    } else {
        field_type
    }
}

impl DraftingRule for TableToReads {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        let table: TableFound = serde_json::from_str(finding.found()).map_err(|_| unreadable())?;
        opener(&table.store).ok_or_else(unreadable)?;
        // A tool is named after its target; with none there is no name.
        let Some(target) = cx.target() else {
            return Ok(Vec::new());
        };
        let shaped = Shaped::of(&table);
        // Only over a record that compiles: a key, and a type for every
        // column. Otherwise the statements file holds the question, once.
        let Some(types) = shaped.types.iter().copied().collect::<Option<Vec<_>>>() else {
            return Ok(Vec::new());
        };
        if shaped.key.is_empty() {
            return Ok(Vec::new());
        }
        let record = &shaped.record;
        let field = |position: usize, list: bool| {
            let mut output = vec![("type", DraftValue::text(output_type(types[position])))];
            if list {
                output.push(("list", DraftValue::Bool(true)));
            }
            (shaped.names[position].clone(), DraftValue::map(output))
        };
        let get_name = format!("{}_{record}_get", target.as_str());
        let list_name = format!("{}_{record}_list", target.as_str());
        if OperationName::new(&get_name).is_err() || OperationName::new(&list_name).is_err() {
            return Ok(Vec::new());
        }
        let key: Vec<DraftValue> = shaped
            .key
            .iter()
            .map(|position| DraftValue::text(&shaped.names[*position]))
            .collect();
        let inputs: Vec<(String, DraftValue)> = shaped
            .key
            .iter()
            .map(|position| {
                (
                    shaped.names[*position].clone(),
                    DraftValue::map(vec![("type", DraftValue::text(types[*position]))]),
                )
            })
            .collect();
        let get = DraftValue::map(vec![
            ("operation", DraftValue::text(&get_name)),
            ("effect", DraftValue::text("read")),
            ("key", DraftValue::List(key)),
            ("inputs", DraftValue::Map(inputs)),
            (
                "outputs",
                DraftValue::Map(
                    (0..table.columns.len())
                        .filter(|position| !shaped.key.contains(position))
                        .map(|position| field(position, false))
                        .collect(),
                ),
            ),
            (
                "bindings",
                DraftValue::List(vec![DraftValue::map(vec![
                    ("mode", DraftValue::text(crate::names::PACK_MODE)),
                    ("statement", DraftValue::text(&format!("{record}_by_key"))),
                ])]),
            ),
            (
                "description",
                DraftValue::text(&format!(
                    "Reads one row of the table `{record}` by its key, inside a read-only transaction. Drafted by rule from the store's catalog; its name is provisional."
                )),
            ),
        ]);
        let mut docs = vec![DraftDoc {
            path: RelPath::new(&format!("operations/{record}_get.yaml"))?,
            body: get,
        }];
        if shaped.list_text(&table).is_some() {
            // One list per column, and whether the store holds more rows
            // than the list read, which the driver states as a fact.
            let mut list_outputs: Vec<(String, DraftValue)> = (0..table.columns.len())
                .map(|position| field(position, true))
                .collect();
            if !shaped.names.iter().any(|name| name == "more") {
                list_outputs.push((
                    "more".to_owned(),
                    DraftValue::map(vec![("type", DraftValue::text("boolean"))]),
                ));
            }
            let list = DraftValue::map(vec![
                ("operation", DraftValue::text(&list_name)),
                ("effect", DraftValue::text("read")),
                ("inputs", DraftValue::Map(Vec::new())),
                ("outputs", DraftValue::Map(list_outputs)),
                (
                    "bindings",
                    DraftValue::List(vec![DraftValue::map(vec![
                        ("mode", DraftValue::text(crate::names::PACK_MODE)),
                        ("verb", DraftValue::text("list")),
                        ("statement", DraftValue::text(&format!("{record}_list"))),
                    ])]),
                ),
                (
                    "description",
                    DraftValue::text(&format!(
                        "Lists the rows of the table `{record}` in the order of its key, as many as the pack's row limit allows, inside a read-only transaction. Drafted by rule from the store's catalog; its name is provisional."
                    )),
                ),
            ]);
            docs.push(DraftDoc {
                path: RelPath::new(&format!("operations/{record}_list.yaml"))?,
                body: list,
            });
        }
        Ok(docs)
    }
}
