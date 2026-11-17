//! The embedded store: one SQLite file, through `rusqlite`. Nothing outside
//! this module names that crate.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode, MAIN_DB, OpenFlags, ffi};

use crate::model::{Access, Isolation};
use crate::store::{
    Bound, Catalog, Cell, ColumnInfo, CommitFailed, Conn, ForeignKey, Interrupter, Opener, Ran,
    Refusal, RunLimits, Shape, TableInfo,
};

/// The embedded store.
pub(crate) struct Sqlite;

impl Opener for Sqlite {
    fn store(&self) -> &'static str {
        "sqlite"
    }

    fn quote(&self, identifier: &str) -> Option<String> {
        // The dialect quotes with double quotes and doubles one inside. A
        // name with a control character is refused instead of quoted.
        if identifier.is_empty() || identifier.chars().any(char::is_control) {
            return None;
        }
        Some(format!("\"{}\"", identifier.replace('"', "\"\"")))
    }

    fn placeholder(&self) -> &'static str {
        "?"
    }

    fn returns_assigned_key(&self) -> bool {
        true
    }

    fn open(&self, file: &Path, access: Access) -> Result<Box<dyn Conn>, Refusal> {
        // No URI flag: nothing in a path can carry an option. No create
        // flag: a missing file is never made.
        let flags = OpenFlags::SQLITE_OPEN_NO_MUTEX
            | match access {
                Access::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
                Access::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
            };
        let conn = Connection::open_with_flags(file, flags).map_err(|error| at_open(&error))?;
        let guarded = match access {
            // Set, read back, go on only if it took: the engine's read-only
            // mode, and writes refused on the connection.
            Access::ReadOnly => {
                conn.pragma_update(None, "query_only", true)
                    .map_err(|error| at_open(&error))?;
                let refused: bool = conn
                    .pragma_query_value(None, "query_only", |row| row.get(0))
                    .map_err(|error| at_open(&error))?;
                refused && conn.is_readonly(MAIN_DB).map_err(|error| at_open(&error))?
            }
            // Foreign keys are enforced by each connection, not by the
            // file. A writer that left them off would skip the store's own
            // rules.
            Access::ReadWrite => {
                conn.pragma_update(None, "foreign_keys", true)
                    .map_err(|error| at_open(&error))?;
                conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))
                    .map_err(|error| at_open(&error))?
            }
        };
        if !guarded {
            return Err(Refusal::NotGuarded);
        }
        Ok(Box::new(SqliteConn { conn, access }))
    }
}

struct SqliteConn {
    conn: Connection,
    access: Access,
}

fn code_of(error: &rusqlite::Error) -> Option<ffi::Error> {
    match error {
        rusqlite::Error::SqliteFailure(error, _) => Some(*error),
        rusqlite::Error::SqlInputError { error, .. } => Some(*error),
        _ => None,
    }
}

/// What an error means while a statement runs.
fn refusal(error: &rusqlite::Error) -> Refusal {
    let Some(error) = code_of(error) else {
        return Refusal::Other(0);
    };
    match error.code {
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Refusal::Locked,
        ErrorCode::OperationInterrupted => Refusal::TimedOut,
        ErrorCode::ConstraintViolation => Refusal::Constraint(match error.extended_code {
            ffi::SQLITE_CONSTRAINT_UNIQUE | ffi::SQLITE_CONSTRAINT_PRIMARYKEY => "unique",
            ffi::SQLITE_CONSTRAINT_CHECK => "check",
            ffi::SQLITE_CONSTRAINT_NOTNULL => "not-null",
            ffi::SQLITE_CONSTRAINT_FOREIGNKEY => "foreign-key",
            ffi::SQLITE_CONSTRAINT_TRIGGER => "trigger",
            _ => "other",
        }),
        ErrorCode::ReadOnly => Refusal::ReadOnly,
        ErrorCode::PermissionDenied | ErrorCode::AuthorizationForStatementDenied => Refusal::Denied,
        ErrorCode::CannotOpen => Refusal::Unavailable,
        ErrorCode::SchemaChanged => Refusal::Shape,
        ErrorCode::TooBig => Refusal::TooLarge,
        _ => Refusal::Other(error.extended_code & 0xff),
    }
}

/// What an error means while a statement is prepared: the generic error is
/// then "no such table", "no such column", or text the store cannot read,
/// which is a changed shape.
fn at_prepare(error: &rusqlite::Error) -> Refusal {
    match refusal(error) {
        Refusal::Other(ffi::SQLITE_ERROR | 0) => Refusal::Shape,
        other => other,
    }
}

/// What an error means while a connection is opened.
fn at_open(error: &rusqlite::Error) -> Refusal {
    match refusal(error) {
        Refusal::Other(_) => Refusal::Unavailable,
        other => other,
    }
}

fn cell(value: ValueRef<'_>, bytes: &mut u64) -> Cell {
    match value {
        ValueRef::Null => Cell::Null,
        ValueRef::Integer(number) => {
            *bytes += 8;
            Cell::Whole(number)
        }
        ValueRef::Text(text) => {
            *bytes += text.len() as u64;
            match std::str::from_utf8(text) {
                Ok(text) => Cell::Text(text.to_owned()),
                // Text that is not UTF-8 is bytes: no rule reads it.
                Err(_) => Cell::Bytes,
            }
        }
        ValueRef::Real(_) => Cell::Float,
        ValueRef::Blob(_) => Cell::Bytes,
    }
}

impl SqliteConn {
    fn wait(&self, wait: Duration) -> Result<(), Refusal> {
        self.conn
            .busy_timeout(wait)
            .map_err(|error| refusal(&error))
    }

    /// The rows of a catalog query with one text parameter or none.
    fn rows<T>(
        &self,
        sql: &str,
        param: Option<&str>,
        read: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    ) -> Result<Vec<T>, Refusal> {
        let mut statement = self
            .conn
            .prepare_cached(sql)
            .map_err(|error| at_prepare(&error))?;
        let params: &[&dyn rusqlite::ToSql] = match &param {
            Some(param) => &[param],
            None => &[],
        };
        let rows = statement
            .query_map(params, |row| read(row))
            .map_err(|error| refusal(&error))?;
        rows.collect::<rusqlite::Result<Vec<T>>>()
            .map_err(|error| refusal(&error))
    }

    fn table(&self, name: String, view: bool) -> Result<TableInfo, Refusal> {
        let columns = self.rows(
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info(?1) ORDER BY cid",
            Some(&name),
            |row| {
                Ok(ColumnInfo {
                    name: row.get(0)?,
                    declared_type: row.get(1)?,
                    not_null: row.get(2)?,
                    key_position: row.get(3)?,
                })
            },
        )?;
        let indexes: Vec<String> = self.rows(
            "SELECT name FROM pragma_index_list(?1) WHERE \"unique\" = 1 AND origin = 'u' ORDER BY name",
            Some(&name),
            |row| row.get(0),
        )?;
        let mut unique = Vec::new();
        for index in indexes {
            let named: Vec<Option<String>> = self.rows(
                "SELECT name FROM pragma_index_info(?1) ORDER BY seqno",
                Some(&index),
                |row| row.get(0),
            )?;
            // An index over an expression has a column with no name; it is
            // not a key a row can be read by.
            if let Some(columns) = named.into_iter().collect::<Option<Vec<String>>>() {
                unique.push(columns);
            }
        }
        unique.sort();
        let parts: Vec<(i64, String, String, String, String)> = self.rows(
            "SELECT id, \"table\", \"from\", on_update, on_delete FROM pragma_foreign_key_list(?1) ORDER BY id, seq",
            Some(&name),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )?;
        let mut foreign_keys: Vec<(i64, ForeignKey)> = Vec::new();
        for (id, references, from, on_update, on_delete) in parts {
            match foreign_keys.last_mut() {
                Some((last, key)) if *last == id => key.columns.push(from),
                _ => foreign_keys.push((
                    id,
                    ForeignKey {
                        columns: vec![from],
                        references,
                        on_update,
                        on_delete,
                    },
                )),
            }
        }
        let triggers = self.rows(
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1 ORDER BY name",
            Some(&name),
            |row| row.get(0),
        )?;
        Ok(TableInfo {
            name,
            view,
            columns,
            unique,
            foreign_keys: foreign_keys.into_iter().map(|(_, key)| key).collect(),
            triggers,
        })
    }
}

impl Conn for SqliteConn {
    fn access(&self) -> Access {
        self.access
    }

    fn begin(&mut self, _isolation: Isolation, wait: Duration) -> Result<(), Refusal> {
        // Every transaction of this store is serializable, which is at
        // least as strong as any level a target may ask for.
        self.wait(wait)?;
        let sql = match self.access {
            Access::ReadOnly => "BEGIN",
            Access::ReadWrite => "BEGIN IMMEDIATE",
        };
        self.conn
            .execute_batch(sql)
            .map_err(|error| refusal(&error))
    }

    fn in_transaction(&self) -> bool {
        !self.conn.is_autocommit()
    }

    fn describe(&mut self, text: &str) -> Result<Shape, Refusal> {
        let statement = self
            .conn
            .prepare_cached(text)
            .map_err(|error| at_prepare(&error))?;
        Ok(Shape {
            params: statement.parameter_count(),
            columns: statement.column_names().map(str::to_owned).collect(),
        })
    }

    fn run(&mut self, text: &str, params: &[Bound], limits: &RunLimits) -> Result<Ran, Refusal> {
        self.wait(limits.wait)?;
        let mut statement = self
            .conn
            .prepare_cached(text)
            .map_err(|error| at_prepare(&error))?;
        if statement.parameter_count() != params.len() {
            return Err(Refusal::Shape);
        }
        for (position, param) in params.iter().enumerate() {
            let bound = match param {
                Bound::Null => statement.raw_bind_parameter(position + 1, rusqlite::types::Null),
                Bound::Whole(number) => statement.raw_bind_parameter(position + 1, number),
                Bound::Text(text) => statement.raw_bind_parameter(position + 1, text.as_str()),
            };
            bound.map_err(|error| refusal(&error))?;
        }
        // The store's own answer, not a reading of the text: only a
        // statement that can write has a count of rows changed.
        let writes = !statement.readonly();
        let mut ran = Ran {
            columns: statement.column_names().map(str::to_owned).collect(),
            ..Ran::default()
        };
        let width = ran.columns.len();
        {
            let mut rows = statement.raw_query();
            while let Some(row) = rows.next().map_err(|error| refusal(&error))? {
                if ran.rows.len() >= limits.rows as usize {
                    ran.more = true;
                    break;
                }
                let mut cells = Vec::with_capacity(width);
                for position in 0..width {
                    let value = row.get_ref(position).map_err(|error| refusal(&error))?;
                    cells.push(cell(value, &mut ran.bytes));
                }
                if ran.bytes > limits.bytes {
                    return Err(Refusal::TooLarge);
                }
                ran.rows.push(cells);
            }
        }
        drop(statement);
        if writes {
            ran.changed = self.conn.changes();
        }
        Ok(ran)
    }

    fn commit(&mut self, wait: Duration) -> Result<(), CommitFailed> {
        if self.wait(wait).is_err() {
            return Err(CommitFailed::NotCommitted(Refusal::Other(0)));
        }
        match self.conn.execute_batch("COMMIT") {
            Ok(()) => Ok(()),
            Err(error) => match refusal(&error) {
                // The store answers "busy" to a commit it did not make, and
                // leaves the transaction open.
                Refusal::Locked if self.in_transaction() => {
                    Err(CommitFailed::NotCommitted(Refusal::Locked))
                }
                _ => Err(CommitFailed::Unknown),
            },
        }
    }

    fn rollback(&mut self) {
        if self.in_transaction() {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }

    fn catalog(&mut self, tables: usize) -> Result<Catalog, Refusal> {
        let mut names: Vec<(String, String)> = self.rows(
            "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name",
            None,
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let truncated = names.len() > tables;
        names.truncate(tables);
        let mut catalog = Catalog {
            tables: Vec::with_capacity(names.len()),
            truncated,
        };
        for (name, kind) in names {
            catalog.tables.push(self.table(name, kind == "view")?);
        }
        Ok(catalog)
    }

    fn interrupter(&self) -> Interrupter {
        let handle = self.conn.get_interrupt_handle();
        Arc::new(move || handle.interrupt())
    }
}
