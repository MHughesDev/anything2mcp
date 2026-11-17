//! The embedded store: one SQLite file, through `rusqlite`. Nothing outside
//! this module names that crate.
//!
//! A connection is held to its one file and to its access by the engine
//! itself, whatever a statement says. It is opened with no right to create
//! a file. Then, each read back where the engine can be asked: writes are
//! refused on a reader; foreign keys are enforced on a writer; extensions
//! cannot be loaded; a name in double quotes is a name and never a text; no
//! other file can be attached; and an authorizer, the engine's own check of
//! every statement it compiles, refuses whatever is not a plain read or,
//! on a writer, a change to rows.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::config::DbConfig;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode, MAIN_DB, OpenFlags, ffi};

use crate::model::{Access, Isolation};
use crate::store::{
    Bound, Catalog, Cell, ColumnInfo, CommitFailed, Conn, ForeignKey, Interrupter, Opener, Ran,
    Refusal, RunLimits, Shape, StoreAt, TableInfo,
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

    fn placeholder(&self, _position: usize) -> String {
        "?".to_owned()
    }

    fn dialect(&self) -> crate::shape::Dialect {
        crate::shape::Dialect::Embedded
    }

    fn returns_assigned_key(&self) -> bool {
        true
    }

    fn open(&self, at: &StoreAt, access: Access) -> Result<Box<dyn Conn>, Refusal> {
        #[cfg(feature = "postgres")]
        let StoreAt::File(file) = at else {
            return Err(Refusal::NotGuarded);
        };
        #[cfg(not(feature = "postgres"))]
        let StoreAt::File(file) = at;
        // The bundled engine is built to read a name that begins `file:` as
        // an address that may carry options, such as the right to create,
        // whether or not the URI flag is given. So only an absolute path is
        // opened: one cannot begin that way. No create flag: a missing file
        // is never made.
        if !file.is_absolute() {
            return Err(Refusal::NotGuarded);
        }
        let flags = OpenFlags::SQLITE_OPEN_NO_MUTEX
            | match access {
                Access::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
                Access::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
            };
        let mut conn = Connection::open_with_flags(file, flags).map_err(|error| at_open(&error))?;
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
        if !guarded || !confined(&conn).map_err(|error| at_open(&error))? {
            return Err(Refusal::NotGuarded);
        }
        // Last, so that the settings above are the last ones any statement
        // changes: from here the engine asks the guard about every statement
        // it compiles on this connection.
        let own = Arc::new(AtomicBool::new(false));
        conn.authorizer(Some(guard(access, Arc::clone(&own))))
            .map_err(|error| at_open(&error))?;
        Ok(Box::new(SqliteConn { conn, access, own }))
    }
}

/// Holds a connection to its one file and its own schema, and reads each
/// setting back. `false` when the engine did not take one.
fn confined(conn: &Connection) -> rusqlite::Result<bool> {
    // The bundled engine is built with extension loading compiled in, and
    // on for its programming interface. Off, for that and for statements.
    conn.load_extension_disable()?;
    let settings = [
        // No direct change to the schema's own tables, and no setting that
        // lets a statement corrupt the file.
        (DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true),
        // A name in double quotes that is no column is an error. Left on,
        // the engine reads it as a text, and a mistyped key column would
        // compare a text with the bound value instead of failing.
        (DbConfig::SQLITE_DBCONFIG_DQS_DML, false),
        (DbConfig::SQLITE_DBCONFIG_DQS_DDL, false),
        // Were a file attached all the same, it could not be made, and it
        // could not be written.
        (DbConfig::SQLITE_DBCONFIG_ENABLE_ATTACH_CREATE, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_ATTACH_WRITE, false),
    ];
    for (setting, wanted) in settings {
        conn.set_db_config(setting, wanted)?;
        if conn.db_config(setting)? != wanted {
            return Ok(false);
        }
    }
    // No file beside the store's own: the limit of attached files is none.
    conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
    Ok(conn.limit(Limit::SQLITE_LIMIT_ATTACHED)? == 0)
}

/// The settings the pack's own catalog queries read. Each only reads.
const CATALOG_SETTINGS: &[&str] = &["table_info", "index_list", "index_info", "foreign_key_list"];

/// Whether a table is one of the engine's own, which no statement of a
/// plan changes.
fn engines_own(table: &str) -> bool {
    table
        .as_bytes()
        .get(..7)
        .is_some_and(|start| start.eq_ignore_ascii_case(b"sqlite_"))
}

/// The engine's check of every statement it compiles on a connection.
///
/// `own` is set while the pack runs a statement of its own: the three that
/// begin and end a transaction, and the queries that read the catalog. A
/// statement of a plan may do none of that. It may read; on a writer it may
/// also change rows, its own and those a trigger of the store changes. It
/// may never attach or detach a file, read or change a setting, change the
/// schema, the temporary one included, or begin or end a transaction.
fn guard(
    access: Access,
    own: Arc<AtomicBool>,
) -> impl for<'c> FnMut(AuthContext<'c>) -> Authorization + Send + 'static {
    move |asked| {
        let own = own.load(Ordering::SeqCst);
        let allowed = match asked.action {
            AuthAction::Select | AuthAction::Recursive | AuthAction::Read { .. } => true,
            AuthAction::Function { function_name } => {
                !function_name.eq_ignore_ascii_case("load_extension")
            }
            AuthAction::Insert { table_name }
            | AuthAction::Delete { table_name }
            | AuthAction::Update { table_name, .. } => {
                !own && access == Access::ReadWrite && !engines_own(table_name)
            }
            AuthAction::Transaction { .. } => own,
            AuthAction::Pragma { pragma_name, .. } => {
                own && CATALOG_SETTINGS
                    .iter()
                    .any(|known| pragma_name.eq_ignore_ascii_case(known))
            }
            // Attach, detach, every change of schema, a savepoint, and any
            // action this build does not know.
            _ => false,
        };
        if allowed {
            Authorization::Allow
        } else {
            Authorization::Deny
        }
    }
}

struct SqliteConn {
    conn: Connection,
    access: Access,
    /// Whether the statement being compiled or run is the pack's own.
    own: Arc<AtomicBool>,
}

/// Sets the mark of the pack's own statements, and clears it when dropped.
struct Own<'a>(&'a AtomicBool);

impl<'a> Own<'a> {
    fn new(mark: &'a AtomicBool) -> Self {
        mark.store(true, Ordering::SeqCst);
        Self(mark)
    }
}

impl Drop for Own<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn code_of(error: &rusqlite::Error) -> Option<ffi::Error> {
    match error {
        rusqlite::Error::SqliteFailure(error, _) => Some(*error),
        rusqlite::Error::SqlInputError { error, .. } => Some(*error),
        _ => None,
    }
}

/// What the store says of the constraint, for the kinds whose message names
/// only schema: the part after `constraint failed:`, which is the columns
/// of a unique key or a not-null column, or the name of a check. A message
/// of any other kind is not read.
fn constraint_named(error: &rusqlite::Error) -> Option<String> {
    let rusqlite::Error::SqliteFailure(_, Some(message)) = error else {
        return None;
    };
    let (kind, names) = message.split_once(" constraint failed: ")?;
    Some(format!(
        "{} constraint failed: {names}",
        kind.to_lowercase()
    ))
}

/// What an error means while a statement runs.
fn refusal(error: &rusqlite::Error) -> Refusal {
    if matches!(error, rusqlite::Error::MultipleStatement) {
        return Refusal::NotAllowed;
    }
    let source = error;
    let Some(error) = code_of(error) else {
        return Refusal::Other(0);
    };
    match error.code {
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Refusal::Locked,
        ErrorCode::OperationInterrupted => Refusal::TimedOut,
        ErrorCode::ConstraintViolation => {
            let (kind, names) = match error.extended_code {
                ffi::SQLITE_CONSTRAINT_UNIQUE | ffi::SQLITE_CONSTRAINT_PRIMARYKEY => {
                    ("unique", true)
                }
                ffi::SQLITE_CONSTRAINT_CHECK => ("check", true),
                ffi::SQLITE_CONSTRAINT_NOTNULL => ("not-null", true),
                ffi::SQLITE_CONSTRAINT_FOREIGNKEY => ("foreign-key", false),
                ffi::SQLITE_CONSTRAINT_TRIGGER => ("trigger", false),
                _ => ("other", false),
            };
            Refusal::Constraint {
                kind,
                named: names.then(|| constraint_named(source)).flatten(),
            }
        }
        ErrorCode::ReadOnly => Refusal::ReadOnly,
        ErrorCode::PermissionDenied => Refusal::Denied,
        // Only the connection's own guard answers this.
        ErrorCode::AuthorizationForStatementDenied => Refusal::NotAllowed,
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
            // The engine compares the names of tables without regard to
            // case, and keeps a trigger's table as the trigger wrote it.
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1 COLLATE NOCASE ORDER BY name",
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
            rules: Vec::new(),
            generated: Vec::new(),
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
        let _own = Own::new(&self.own);
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
            reads_only: Some(statement.readonly()),
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
        // A reader runs only what the engine says changes nothing, and only
        // what returns columns. Asked before the first step: a statement
        // that would be refused half way may already have made a file.
        if self.access == Access::ReadOnly {
            if !statement.readonly() {
                return Err(Refusal::ReadOnly);
            }
            if statement.column_count() == 0 {
                return Err(Refusal::NotAllowed);
            }
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
        let _own = Own::new(&self.own);
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
            let _own = Own::new(&self.own);
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }

    fn catalog(&mut self, tables: usize) -> Result<Catalog, Refusal> {
        let _own = Own::new(&self.own);
        // One name more than is kept, so that "more exist" is a fact and
        // the read is bounded however many tables the store holds. The
        // number is the pack's own and never a value from outside.
        let mut names: Vec<(String, String)> = self.rows(
            &format!(
                "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name LIMIT {}",
                tables.saturating_add(1)
            ),
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
