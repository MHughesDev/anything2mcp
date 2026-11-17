//! The `statements/` file kind: its schema, the names it defines, and its
//! compilation into the pack's section of the plan.
//!
//! A statement's text is a constant of the plan. A hand-written text is
//! copied as it stands, once its form is held to one plain read; a
//! `form: table` statement is written here, from identifiers of the file
//! quoted by the dialect's rule. No value from a tool input is ever placed
//! in either.

use std::collections::BTreeMap;
use std::sync::Arc;

use a2m_core::{FileKindName, LintCode, PackPayload, SurfaceKind};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind, JsonSchema,
    KindContext, Located, Place, Stage,
};

use crate::model::{
    Access, Column, Does, Isolation, Param, Returns, SECTION_FORMAT, Section, Statement, Stored,
};
use crate::names::{DATABASE, FOLDER, STATEMENT, lint};
use crate::shape;
use crate::store::{Opener, STORES, opener};

/// The longest a statement may be given, in milliseconds: ten minutes.
const LONGEST_MS: u64 = 600_000;
/// The name of the parameter that carries the version a guard compares.
const GUARD_PARAM: &str = "version";

/// The file kind.
pub(crate) struct StatementKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
    /// The most rows a statement's `max_rows` may ask for, from the pack's
    /// settings.
    max_rows: u32,
}

impl StatementKind {
    pub(crate) fn new(max_rows: u32) -> Self {
        let field = JsonSchema::object()
            .required("column", JsonSchema::text())
            .required("type", JsonSchema::field_type());
        let record = JsonSchema::object()
            .required("table", JsonSchema::text())
            .required("key", JsonSchema::list(JsonSchema::name()))
            .optional("version", JsonSchema::text())
            .required("fields", JsonSchema::map_of(field));
        let param = JsonSchema::object()
            .required("name", JsonSchema::name())
            .required("type", JsonSchema::field_type());
        let statement = JsonSchema::object()
            .required("effect", JsonSchema::words(&["read", "write"]))
            .required("record", JsonSchema::name())
            .optional("text", JsonSchema::text())
            .optional("params", JsonSchema::list(param))
            .optional("returns", JsonSchema::words(&["one_or_none", "many"]))
            .optional("max_rows", JsonSchema::int_between(1, i64::from(u32::MAX)))
            .optional("isolation", JsonSchema::words(Isolation::NAMES))
            .required("time_limit", JsonSchema::text())
            .optional("form", JsonSchema::words(&["table", "statement"]))
            .optional("verb", JsonSchema::words(&["insert", "update", "delete"]))
            .optional("set", JsonSchema::non_empty_list(JsonSchema::name()))
            .optional(
                "guard",
                JsonSchema::words(&["version", "before_values", "none"]),
            )
            .optional("hazard", JsonSchema::words(&["none", "data_elsewhere"]));
        Self {
            schema: JsonSchema::object()
                .optional("surface", JsonSchema::surface())
                .required("records", JsonSchema::map_of(record))
                .required("statements", JsonSchema::map_of(statement)),
            kinds: vec![DATABASE],
            max_rows,
        }
    }
}

fn problem(code: &LintCode, at: &Place, message: String, fix: &str) -> Diagnostic {
    Diagnostic::error(code.clone(), Stage::Lower, at, message, fix)
}

/// A time limit as a target file writes it: whole seconds or milliseconds,
/// such as `2s` or `500ms`.
pub(crate) fn time_limit_ms(text: &str) -> Option<u32> {
    let (digits, scale) = match text.strip_suffix("ms") {
        Some(digits) => (digits, 1),
        None => (text.strip_suffix('s')?, 1000),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let millis = digits.parse::<u64>().ok()?.checked_mul(scale)?;
    (1..=LONGEST_MS)
        .contains(&millis)
        .then(|| u32::try_from(millis).ok())
        .flatten()
}

/// One field of a record, resolved.
struct Field {
    name: String,
    column: String,
    field_type: String,
    stored: Stored,
    key_position: u32,
}

/// One record kind of a document, resolved.
struct Record {
    table: String,
    key: Vec<String>,
    version: Option<String>,
    fields: Vec<Field>,
}

impl Record {
    fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// Every column a read of the record may return: its fields, and its
    /// version when no field holds it.
    fn columns(&self) -> Vec<Column> {
        let mut columns: Vec<Column> = self
            .fields
            .iter()
            .map(|field| Column {
                column: field.column.clone(),
                field: field.name.clone(),
                stored: field.stored,
                key_position: field.key_position,
            })
            .collect();
        if let Some(version) = &self.version
            && !self.fields.iter().any(|field| &field.column == version)
        {
            columns.push(Column {
                column: version.clone(),
                field: version.clone(),
                stored: Stored::Either,
                key_position: 0,
            });
        }
        columns
    }
}

fn records(
    doc: &Located<Doc>,
    cx: &KindContext<'_>,
    problems: &mut Diagnostics,
) -> BTreeMap<String, Record> {
    let mut records = BTreeMap::new();
    for entry in doc.get("records").map(Located::entries).unwrap_or_default() {
        let body = &entry.value;
        let mut fields = Vec::new();
        for field in body.get("fields").map(Located::entries).unwrap_or_default() {
            let type_name = field.value.text("type").unwrap_or_default();
            // A field type the kernel does not know was reported where it is
            // written; the field is left out here.
            let Some(field_type) = cx.target().field_type(type_name) else {
                continue;
            };
            fields.push(Field {
                name: field.key.value.clone(),
                column: field.value.text("column").unwrap_or_default().to_owned(),
                field_type: type_name.to_owned(),
                stored: Stored::of(&field_type.plan.ty),
                key_position: 0,
            });
        }
        let mut key = Vec::new();
        for part in body.get("key").map(Located::items).unwrap_or_default() {
            let name = part.as_text().unwrap_or_default();
            let position = u32::try_from(key.len() + 1).unwrap_or(u32::MAX);
            match fields.iter_mut().find(|field| field.name == name) {
                Some(field) if field.key_position == 0 => {
                    field.key_position = position;
                    key.push(name.to_owned());
                }
                _ => problems.push(problem(
                    &lint::UNKNOWN_FIELD,
                    &part.at,
                    format!(
                        "The key of the record `{}` names `{name}`, which is not one of its fields, or is named twice.",
                        entry.key.value
                    ),
                    "Name each field of the key once, parent first, by the name it has under `fields`.",
                )),
            }
        }
        records.insert(
            entry.key.value.clone(),
            Record {
                table: body.text("table").unwrap_or_default().to_owned(),
                key,
                version: body.text("version").map(str::to_owned),
                fields,
            },
        );
    }
    records
}

/// A `form: table` statement, as the pack wrote it.
struct Written {
    text: String,
    params: Vec<Param>,
    does: Does,
    guarded: bool,
    returns: Returns,
    columns: Vec<Column>,
}

/// What a `form: table` statement is written from.
struct TableForm<'a> {
    record: &'a Record,
    record_name: &'a str,
    body: &'a Located<Doc>,
    store: &'a dyn Opener,
}

impl TableForm<'_> {
    fn invalid(&self, at: &Place, message: String, fix: &str) -> Diagnostic {
        problem(&lint::STATEMENT_INVALID, at, message, fix)
    }

    fn quoted(&self, identifier: &str, at: &Place) -> Result<String, Diagnostic> {
        self.store.quote(identifier).ok_or_else(|| {
            self.invalid(
                at,
                "A table or a column of this statement has a name the store's dialect cannot quote."
                    .to_owned(),
                "Name the table and the columns as the store's catalog does.",
            )
        })
    }

    /// The fields under `set`, each a field of the record.
    fn set(&self) -> Result<Vec<&Field>, Diagnostic> {
        let Some(set) = self.body.get("set") else {
            return Err(self.invalid(
                &self.body.at,
                "An insert or an update names the fields it writes.".to_owned(),
                "Add `set:` with the fields to write.",
            ));
        };
        let mut fields: Vec<&Field> = Vec::new();
        for item in set.items() {
            let name = item.as_text().unwrap_or_default();
            match self.record.field(name) {
                Some(field) if !fields.iter().any(|known| known.name == name) => fields.push(field),
                _ => {
                    return Err(problem(
                        &lint::UNKNOWN_FIELD,
                        &item.at,
                        format!(
                            "`set` names `{name}`, which is not a field of the record `{}`, or is named twice.",
                            self.record_name
                        ),
                        "Name each field once, as it is named under the record's `fields`.",
                    ));
                }
            }
        }
        Ok(fields)
    }

    /// The full key predicate, with its parameters. There is no table-form
    /// statement that finds a row any other way.
    fn key_predicate(&self, params: &mut Vec<Param>) -> Result<Vec<String>, Diagnostic> {
        if self.record.key.is_empty() {
            return Err(self.invalid(
                &self.body.at,
                format!(
                    "The record `{}` declares no key, so a row of it cannot be read, updated, or deleted by key.",
                    self.record_name
                ),
                "Declare the record's key, or leave the statement out.",
            ));
        }
        let mut predicate = Vec::new();
        for name in &self.record.key {
            let Some(field) = self.record.field(name) else {
                continue;
            };
            predicate.push(format!(
                "{} = {}",
                self.quoted(&field.column, &self.body.at)?,
                self.store.placeholder()
            ));
            params.push(Param {
                name: field.name.clone(),
                field_type: Some(field.field_type.clone()),
                key: true,
            });
        }
        Ok(predicate)
    }

    /// The read of one record by its full key: every field's column and the
    /// version, from the record's table, where the key is the one bound.
    fn read_by_key(&self) -> Result<Written, Diagnostic> {
        let table = self.quoted(&self.record.table, &self.body.at)?;
        let columns = self.record.columns();
        let mut selected = Vec::new();
        for column in &columns {
            selected.push(self.quoted(&column.column, &self.body.at)?);
        }
        let mut params = Vec::new();
        let predicate = self.key_predicate(&mut params)?;
        Ok(Written {
            text: format!(
                "SELECT {} FROM {table} WHERE {}",
                selected.join(", "),
                predicate.join(" AND ")
            ),
            params,
            does: Does::Read,
            guarded: false,
            returns: Returns::OneOrNone,
            columns,
        })
    }

    /// The key predicate and the guard, with their parameters. There is no
    /// table-form update or delete without the full key.
    fn by_key(&self, params: &mut Vec<Param>) -> Result<(String, bool), Diagnostic> {
        let mut predicate = self.key_predicate(params)?;
        let guard = self.body.get("guard");
        let guarded = match guard.and_then(Located::as_text) {
            Some("none") => false,
            Some("version") => {
                let Some(version) = &self.record.version else {
                    return Err(self.invalid(
                        guard.map_or(&self.body.at, |guard| &guard.at),
                        format!(
                            "The guard is `version`, and the record `{}` declares no version column.",
                            self.record_name
                        ),
                        "Add `version:` to the record with the column the store changes on every write, or use `guard: none`.",
                    ));
                };
                // The guard's value is the step argument `version`. A field
                // of that name among the parameters would be bound to the
                // same argument, and the guard would compare the wrong value.
                if params.iter().any(|param| param.name == GUARD_PARAM) {
                    return Err(self.invalid(
                        guard.map_or(&self.body.at, |guard| &guard.at),
                        format!(
                            "The guard's parameter is named `{GUARD_PARAM}`, and the statement also takes a field of the record `{}` with that name.",
                            self.record_name
                        ),
                        "Give the field another name under `fields`; its `column` stays as it is.",
                    ));
                }
                predicate.push(format!(
                    "{} = {}",
                    self.quoted(version, &self.body.at)?,
                    self.store.placeholder()
                ));
                params.push(Param {
                    name: GUARD_PARAM.to_owned(),
                    field_type: None,
                    key: false,
                });
                true
            }
            Some(_) => {
                return Err(self.invalid(
                    guard.map_or(&self.body.at, |guard| &guard.at),
                    "The guard `before_values` is not built yet.".to_owned(),
                    "Use `guard: version` with a version column, or `guard: none`.",
                ));
            }
            None => {
                return Err(self.invalid(
                    &self.body.at,
                    "An update or a delete states its guard.".to_owned(),
                    "Add `guard: version`, or `guard: none` when the record has no version column.",
                ));
            }
        };
        Ok((predicate.join(" AND "), guarded))
    }

    /// The statement's text, its parameters, what it does, whether it is
    /// guarded, and what it returns.
    fn write(&self) -> Result<Written, Diagnostic> {
        let table = self.quoted(&self.record.table, &self.body.at)?;
        let placeholder = self.store.placeholder();
        let mut params = Vec::new();
        let as_param = |field: &Field| Param {
            name: field.name.clone(),
            field_type: Some(field.field_type.clone()),
            key: field.key_position > 0,
        };
        match self.body.text("verb") {
            Some("update") => {
                let set = self.set()?;
                if let Some(field) = set.iter().find(|field| field.key_position > 0) {
                    return Err(self.invalid(
                        &self.body.at,
                        format!(
                            "An update does not set `{}`, a field of the record's key.",
                            field.name
                        ),
                        "Leave the key's fields out of `set`.",
                    ));
                }
                let mut assignments = Vec::new();
                for field in set {
                    assignments.push(format!(
                        "{} = {placeholder}",
                        self.quoted(&field.column, &self.body.at)?
                    ));
                    params.push(as_param(field));
                }
                let (predicate, guarded) = self.by_key(&mut params)?;
                Ok(Written {
                    text: format!(
                        "UPDATE {table} SET {} WHERE {predicate}",
                        assignments.join(", ")
                    ),
                    params,
                    does: Does::Update,
                    guarded,
                    returns: Returns::Nothing,
                    columns: Vec::new(),
                })
            }
            Some("delete") => {
                let (predicate, guarded) = self.by_key(&mut params)?;
                Ok(Written {
                    text: format!("DELETE FROM {table} WHERE {predicate}"),
                    params,
                    does: Does::Delete,
                    guarded,
                    returns: Returns::Nothing,
                    columns: Vec::new(),
                })
            }
            Some("insert") => {
                let set = self.set()?;
                let mut columns = Vec::new();
                for field in &set {
                    columns.push(self.quoted(&field.column, &self.body.at)?);
                    params.push(as_param(field));
                }
                let missing: Vec<&Field> = self
                    .record
                    .key
                    .iter()
                    .filter_map(|name| self.record.field(name))
                    .filter(|field| !set.iter().any(|given| given.name == field.name))
                    .collect();
                let mut text = format!(
                    "INSERT INTO {table} ({}) VALUES ({})",
                    columns.join(", "),
                    vec![placeholder; columns.len()].join(", ")
                );
                if missing.is_empty() {
                    return Ok(Written {
                        text,
                        params,
                        does: Does::Insert,
                        guarded: false,
                        returns: Returns::Nothing,
                        columns: Vec::new(),
                    });
                }
                if missing.len() != self.record.key.len() || !self.store.returns_assigned_key() {
                    return Err(self.invalid(
                        &self.body.at,
                        "An insert sets every field of the record's key, or none of them when the store assigns the key.".to_owned(),
                        "Add the key's fields to `set`, or leave all of them out.",
                    ));
                }
                let mut returned = Vec::new();
                let mut assigned = Vec::new();
                for field in missing {
                    returned.push(self.quoted(&field.column, &self.body.at)?);
                    assigned.push(Column {
                        column: field.column.clone(),
                        field: field.name.clone(),
                        stored: field.stored,
                        key_position: field.key_position,
                    });
                }
                text.push_str(" RETURNING ");
                text.push_str(&returned.join(", "));
                Ok(Written {
                    text,
                    params,
                    does: Does::Insert,
                    guarded: false,
                    returns: Returns::AssignedKey,
                    columns: assigned,
                })
            }
            _ => Err(self.invalid(
                &self.body.at,
                "A `form: table` statement states what it does.".to_owned(),
                "Add `verb:` with `insert`, `update`, or `delete`.",
            )),
        }
    }
}

/// Refuses a statement whose words are not those of one plain read or one
/// keyed write. The driver makes the same refusal before it runs one.
fn held_to_its_form(text: &str, does: Does, at: &Place) -> Result<(), Diagnostic> {
    shape::check(text, does).map_err(|wrong| {
        problem(
            &lint::STATEMENT_INVALID,
            at,
            wrong.message().to_owned(),
            "Write a read as one `SELECT` with `?` for each parameter, and quote a table or a column whose name is one of the refused words.",
        )
    })
}

impl StatementKind {
    fn statement(
        &self,
        name: &str,
        body: &Located<Doc>,
        surface: &str,
        records: &BTreeMap<String, Record>,
        store: &dyn Opener,
    ) -> Result<Statement, Diagnostic> {
        let invalid = |at: &Place, message: String, fix: &str| {
            problem(&lint::STATEMENT_INVALID, at, message, fix)
        };
        let record_name = body.text("record").unwrap_or_default();
        let Some(record) = records.get(record_name) else {
            return Err(problem(
                &lint::UNKNOWN_RECORD,
                body.get("record").map_or(&body.at, |record| &record.at),
                format!(
                    "The statement `{name}` names the record `{record_name}`, which this file does not declare."
                ),
                "Name a record declared under `records:` in the same file.",
            ));
        };
        let limit = body.text("time_limit").unwrap_or_default();
        let Some(timeout_ms) = time_limit_ms(limit) else {
            return Err(invalid(
                body.get("time_limit").map_or(&body.at, |limit| &limit.at),
                format!(
                    "The time limit of the statement `{name}` is not whole seconds or milliseconds of at most ten minutes."
                ),
                "Write the limit as `2s` or `500ms`.",
            ));
        };
        let isolation = body
            .text("isolation")
            .and_then(Isolation::from_name)
            .unwrap_or(Isolation::ReadCommitted);
        let reads = body.text("effect") == Some("read");
        let extra = |keys: &[&'static str]| {
            keys.iter()
                .find_map(|key| body.get(key).map(|found| (*key, found)))
        };
        if reads && body.text("form") == Some("table") {
            // The read of one record by its key, which the pack writes: the
            // only read a write may be read back with.
            if let Some((key, found)) = extra(&[
                "text", "params", "returns", "max_rows", "verb", "set", "guard", "hazard",
            ]) {
                return Err(invalid(
                    &found.at,
                    format!(
                        "The statement `{name}` is a read the pack writes from its record, and `{key}` does not belong to one."
                    ),
                    "Remove the key: a `form: table` read returns the record's fields, by its key.",
                ));
            }
            let form = TableForm {
                record,
                record_name,
                body,
                store,
            };
            let written = form.read_by_key()?;
            held_to_its_form(&written.text, written.does, &body.at)?;
            return Ok(Statement {
                surface: surface.to_owned(),
                record: record_name.to_owned(),
                by_key: true,
                access: Access::ReadOnly,
                text: written.text,
                does: written.does,
                guarded: false,
                params: written.params,
                timeout_ms,
                columns: written.columns,
                returns: written.returns,
                max_rows: 1,
                table: None,
                hazard_declared: false,
                isolation,
            });
        }
        if reads {
            if let Some((key, found)) = extra(&["form", "verb", "set", "guard", "hazard"]) {
                return Err(invalid(
                    &found.at,
                    format!("The statement `{name}` is a read, and `{key}` belongs to a write."),
                    "Remove the key, or declare `effect: write`.",
                ));
            }
            let (Some(text), Some(params), Some(returns)) =
                (body.text("text"), body.get("params"), body.text("returns"))
            else {
                return Err(invalid(
                    &body.at,
                    format!(
                        "The read statement `{name}` states its `text`, its `params`, and what it `returns`."
                    ),
                    "Add the three keys; `params: []` for a statement with no parameter.",
                ));
            };
            // One plain read, by its words. The store proves that it only
            // reads; this refuses what a read-only connection does not stop.
            held_to_its_form(text, Does::Read, body.get("text").map_or(&body.at, |text| &text.at))?;
            let mut declared: Vec<Param> = Vec::new();
            for param in params.items() {
                let param_name = param.text("name").unwrap_or_default();
                if declared.iter().any(|known| known.name == param_name) {
                    return Err(invalid(
                        &param.at,
                        format!("The statement `{name}` names the parameter `{param_name}` twice."),
                        "Give each placeholder its own parameter name.",
                    ));
                }
                declared.push(Param {
                    name: param_name.to_owned(),
                    field_type: param.text("type").map(str::to_owned),
                    key: record.key.iter().any(|key| key == param_name),
                });
            }
            let (returns, max_rows) = if returns == "many" {
                let stated = body.get("max_rows");
                let rows = stated
                    .and_then(Located::as_int)
                    .and_then(|rows| u32::try_from(rows).ok())
                    .filter(|rows| *rows <= self.max_rows);
                let Some(rows) = rows else {
                    return Err(problem(
                        &lint::LIMIT_INVALID,
                        stated.map_or(&body.at, |stated| &stated.at),
                        format!(
                            "The statement `{name}` returns many rows and states no `max_rows` of at most {}.",
                            self.max_rows
                        ),
                        "Add `max_rows:` with the most rows one call may return.",
                    ));
                };
                (Returns::Many, rows)
            } else {
                (Returns::OneOrNone, 1)
            };
            return Ok(Statement {
                surface: surface.to_owned(),
                record: record_name.to_owned(),
                by_key: false,
                access: Access::ReadOnly,
                text: text.to_owned(),
                does: Does::Read,
                guarded: false,
                params: declared,
                timeout_ms,
                columns: record.columns(),
                returns,
                max_rows,
                table: None,
                hazard_declared: false,
                isolation,
            });
        }
        if let Some((key, found)) = extra(&["text", "params", "returns", "max_rows"]) {
            return Err(invalid(
                &found.at,
                format!(
                    "The statement `{name}` is a write the pack writes from its record, and `{key}` belongs to a hand-written statement."
                ),
                "Remove the key.",
            ));
        }
        if body.text("form") != Some("table") {
            return Err(invalid(
                body.get("form").map_or(&body.at, |form| &form.at),
                format!(
                    "The write statement `{name}` is not `form: table`; hand-written writes are not built yet."
                ),
                "Write `form: table` with `verb:` and `set:`.",
            ));
        }
        let form = TableForm {
            record,
            record_name,
            body,
            store,
        };
        let Written {
            text,
            params,
            does,
            guarded,
            returns,
            columns,
        } = form.write()?;
        held_to_its_form(&text, does, &body.at)?;
        Ok(Statement {
            surface: surface.to_owned(),
            record: record_name.to_owned(),
            by_key: false,
            access: Access::ReadWrite,
            text,
            does,
            guarded,
            params,
            timeout_ms,
            columns,
            returns,
            max_rows: 1,
            table: Some(record.table.clone()),
            hazard_declared: body.text("hazard") == Some("data_elsewhere"),
            isolation,
        })
    }
}

impl FileKind for StatementKind {
    fn name(&self) -> FileKindName {
        STATEMENT
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        FOLDER
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        doc.get("statements")
            .map(Located::entries)
            .unwrap_or_default()
            .iter()
            .map(|entry| {
                Located::new(
                    DefinedName {
                        name: entry.key.value.clone(),
                    },
                    entry.key.at.clone(),
                )
            })
            .collect()
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        // A statement holds no expression: its text is never a template.
        Vec::new()
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut section = Section::default();
        let mut problems = Diagnostics::new();
        for doc in docs {
            let surface = cx
                .surface_of(doc)
                .filter(|surface| *surface.kind == DATABASE);
            let Some(surface) = surface else {
                problems.push(problem(
                    &lint::SURFACE_INVALID,
                    doc.get("surface").map_or(&doc.at, |surface| &surface.at),
                    "A statements file lives on a surface of the kind `database`, and the target declares none that this file names.".to_owned(),
                    "Add `surface:` with the name of a `database` surface of `target.yaml`.",
                ));
                continue;
            };
            let store_name = surface.doc.text("store").unwrap_or_default();
            let Some(store): Option<Arc<dyn Opener>> = opener(store_name) else {
                problems.push(problem(
                    &lint::STORE_NOT_BUILT,
                    surface
                        .doc
                        .get("store")
                        .map_or(&surface.doc.at, |store| &store.at),
                    format!(
                        "The surface `{}` names a store this build does not speak to. Built: {}.",
                        surface.name,
                        STORES.join(", ")
                    ),
                    "Name a store that is built.",
                ));
                continue;
            };
            let records = records(doc, cx, &mut problems);
            for entry in doc
                .get("statements")
                .map(Located::entries)
                .unwrap_or_default()
            {
                let name = entry.key.value.as_str();
                if a2m_core::StepKind::new(name).is_err() {
                    problems.push(problem(
                        &lint::STATEMENT_INVALID,
                        &entry.key.at,
                        "A statement's name is lowercase letters, digits, and underscores, starting with a letter.".to_owned(),
                        "Rename the statement, such as `line_item_by_key`.",
                    ));
                    continue;
                }
                match self.statement(
                    name,
                    &entry.value,
                    surface.name.as_str(),
                    &records,
                    store.as_ref(),
                ) {
                    Ok(statement) => {
                        section.statements.insert(name.to_owned(), statement);
                    }
                    Err(found) => problems.push(found),
                }
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
                Diagnostics::from(problem(
                    &lint::STATEMENT_INVALID,
                    &Place::start_of(&a2m_files::RelPath::target_file()),
                    failure.message().to_owned(),
                    "Report this to the pack's author: a statements section holds only text and whole numbers.",
                ))
            })
    }
}
