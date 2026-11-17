//! Drafting: from a `table` finding to a `statements/` file, by rule.
//!
//! The draft holds one record for the table and, when the catalog states a
//! key, one read by that key. It carries no unit, no business name, and no
//! key the store did not state: where the catalog is silent the draft holds
//! an open question, which the compiler refuses until a person answers it.
//! No write is ever drafted.

use a2m_core::{Failure, FailureClass, FailureCode, FileKindName, Finding, FindingKind};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath};

use crate::discover::{ColumnFound, TableFound};
use crate::names::{FOLDER, STATEMENT};
use crate::store::opener;

/// The time limit a drafted read is given.
const DRAFT_TIME_LIMIT: &str = "2s";

/// From `table` findings to `statements/` files.
pub(crate) struct TableToStatements {
    pub(crate) kind: FindingKind,
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
        let record = plain_name(&table.table, "t_");
        // One field per column, named from the column's own words, with the
        // type the store states or a question where it states none.
        let mut names: Vec<String> = Vec::new();
        for (position, column) in table.columns.iter().enumerate() {
            let mut name = plain_name(&column.name, "c_");
            if names.contains(&name) {
                name = format!("{name}_{position}");
            }
            names.push(name);
        }
        let type_of = |position: usize| -> DraftValue {
            table
                .columns
                .get(position)
                .and_then(field_type)
                .map_or_else(
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
            .zip(&names)
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
        let mut key: Vec<(u32, usize)> = table
            .columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.key_position > 0)
            .map(|(position, column)| (column.key_position, position))
            .collect();
        key.sort_unstable();
        let keyed = !key.is_empty() && !table.view;
        let key_value = if keyed {
            DraftValue::List(
                key.iter()
                    .filter_map(|(_, position)| names.get(*position))
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
                    ("record", DraftValue::text(&record)),
                    ("time_limit", DraftValue::text(DRAFT_TIME_LIMIT)),
                ]),
            ));
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
                ("records", DraftValue::Map(vec![(record, record_body)])),
                ("statements", DraftValue::Map(statements)),
            ]),
        }])
    }
}
