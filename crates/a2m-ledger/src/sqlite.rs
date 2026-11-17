//! Everything that is specific to SQLite: opening a connection, its settings,
//! the authorizer that holds each crate to its own tables, running a
//! statement, and turning the database's errors into failures.
//!
//! No other module names the database crate, and nothing of it is public, so
//! a second database can be put behind the same [`Tx`](crate::Tx) later.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use a2m_core::Failure;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, ErrorCode, params_from_iter};

use crate::failure;
use crate::migrate::table_owner;
use crate::tx::{Durability, Owner, Row, SqlValue};

/// Where the database is.
#[derive(Debug, Clone)]
pub(crate) enum Place {
    /// A file in the data directory.
    File(PathBuf),
    /// Memory, for tests. It is gone when the store is dropped.
    Memory,
}

/// Whose statement is being prepared. The authorizer reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The store's own bookkeeping: settings, transactions, the migration
    /// table.
    Internal,
    /// A statement of the crate that owns some tables.
    Owner(Owner),
    /// A migration of that crate.
    Migration(Owner),
}

impl Scope {
    fn encode(self) -> u32 {
        match self {
            Self::Internal => 0,
            Self::Owner(owner) => 1 + owner.index(),
            Self::Migration(owner) => 101 + owner.index(),
        }
    }

    fn decode(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Internal),
            1..=100 => Owner::from_index(code - 1).map(Self::Owner),
            _ => Owner::from_index(code - 101).map(Self::Migration),
        }
    }
}

/// The two policy tables the ledger may read when writes begin: the one
/// declared exception to "a crate's SQL names only its own tables".
const READ_FOR_RULE_THREE: [&str; 2] = ["approvals", "decisions"];

/// The table whose changed rows are remembered for the length of one
/// transaction, so that rule 3 can tell an approval marked used in this
/// transaction from one marked earlier.
const WATCHED: &str = "approvals";

/// One connection, with the scope its next statement is prepared under.
pub(crate) struct Db {
    conn: Connection,
    scope: Arc<AtomicU32>,
    watched: Arc<Mutex<Vec<i64>>>,
    synced: AtomicBool,
    in_memory: bool,
}

impl Db {
    /// Opens a connection and gives it the settings the store needs: the
    /// write-ahead log, so readers never wait for the writer; a full sync on
    /// commit; foreign keys; and a wait, not an error, when another
    /// connection holds the file for a moment.
    pub(crate) fn open(place: &Place, read_only: bool) -> Result<Self, Failure> {
        let mut conn = match place {
            Place::File(path) => Connection::open(path),
            Place::Memory => Connection::open_in_memory(),
        }
        .map_err(map)?;
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(map)?;
        // The journal mode answers with a row, so it is read, not executed.
        let _mode: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .map_err(map)?;
        conn.execute_batch("PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON;")
            .map_err(map)?;
        if read_only {
            conn.execute_batch("PRAGMA query_only = ON;").map_err(map)?;
        }

        let scope = Arc::new(AtomicU32::new(Scope::Internal.encode()));
        let seen = Arc::clone(&scope);
        conn.authorizer(Some(move |cx: AuthContext<'_>| {
            authorize(seen.load(Ordering::SeqCst), &cx)
        }))
        .map_err(map)?;

        let watched = Arc::new(Mutex::new(Vec::new()));
        let rows = Arc::clone(&watched);
        conn.update_hook(Some(move |_action, _db: &str, table: &str, rowid: i64| {
            if table == WATCHED {
                rows.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(rowid);
            }
        }))
        .map_err(map)?;

        Ok(Self {
            conn,
            scope,
            watched,
            synced: AtomicBool::new(true),
            in_memory: matches!(place, Place::Memory),
        })
    }

    fn scoped(&self, scope: Scope) {
        self.scope.store(scope.encode(), Ordering::SeqCst);
    }

    /// The database's own check of its file.
    pub(crate) fn check_integrity(&self) -> Result<(), Failure> {
        self.scoped(Scope::Internal);
        let answer: String = self
            .conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(map)?;
        if answer == "ok" {
            Ok(())
        } else {
            Err(failure::corrupt("the database's integrity check"))
        }
    }

    /// Begins a transaction. A writing one takes the write lock at once, and
    /// sets how hard its commit is pushed to disk.
    pub(crate) fn begin(&self, write: Option<Durability>) -> Result<(), Failure> {
        self.scoped(Scope::Internal);
        self.watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        match write {
            Some(durability) => {
                let synced = durability == Durability::Synced;
                if self.synced.swap(synced, Ordering::SeqCst) != synced {
                    let setting = if synced {
                        "PRAGMA synchronous = FULL"
                    } else {
                        "PRAGMA synchronous = NORMAL"
                    };
                    self.conn.execute_batch(setting).map_err(map)?;
                }
                self.conn.execute_batch("BEGIN IMMEDIATE").map_err(map)
            }
            None => {
                // One connection serves an in-memory store, so a read on it
                // is held to reading by a setting, not by its connection.
                if self.in_memory {
                    self.conn
                        .execute_batch("PRAGMA query_only = ON")
                        .map_err(map)?;
                }
                self.conn.execute_batch("BEGIN").map_err(map)
            }
        }
    }

    /// Ends the transaction: commits it, or takes back everything it did.
    pub(crate) fn end(&self, commit: bool, write: bool) -> Result<(), Failure> {
        self.scoped(Scope::Internal);
        let ended = if commit {
            self.conn.execute_batch("COMMIT").map_err(map)
        } else {
            self.conn.execute_batch("ROLLBACK").map_err(map)
        };
        if ended.is_err() && commit {
            // A commit that failed leaves the transaction open.
            let _ = self.conn.execute_batch("ROLLBACK");
        }
        if !write && self.in_memory {
            self.conn
                .execute_batch("PRAGMA query_only = OFF")
                .map_err(map)?;
        }
        ended
    }

    /// Whether the row of the watched table with this row number was changed
    /// in the transaction that is open.
    pub(crate) fn changed_in_this_transaction(&self, rowid: i64) -> bool {
        self.watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&rowid)
    }

    /// Runs one statement that returns no rows. Gives the number of rows it
    /// changed.
    pub(crate) fn execute(
        &self,
        scope: Scope,
        sql: &str,
        params: &[SqlValue],
    ) -> Result<u64, Failure> {
        self.scoped(scope);
        let mut statement = self.prepare(scope, sql)?;
        let changed = statement
            .execute(params_from_iter(params.iter().map(to_sqlite)))
            .map_err(map)?;
        Ok(changed as u64)
    }

    /// Runs one statement and gives its rows.
    pub(crate) fn query(
        &self,
        scope: Scope,
        sql: &str,
        params: &[SqlValue],
    ) -> Result<Vec<Row>, Failure> {
        self.scoped(scope);
        let mut statement = self.prepare(scope, sql)?;
        let columns = statement.column_count();
        let mut rows = statement
            .query(params_from_iter(params.iter().map(to_sqlite)))
            .map_err(map)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(map)? {
            let mut values = Vec::with_capacity(columns);
            for column in 0..columns {
                values.push(match row.get_ref(column).map_err(map)? {
                    ValueRef::Null => SqlValue::Null,
                    ValueRef::Integer(number) => SqlValue::Int(number),
                    ValueRef::Text(text) => SqlValue::Text(
                        std::str::from_utf8(text)
                            .map_err(|_| failure::corrupt("a text column that is not UTF-8"))?
                            .to_owned(),
                    ),
                    ValueRef::Blob(bytes) => SqlValue::Bytes(bytes.to_vec()),
                    // The store keeps no floating-point number.
                    ValueRef::Real(_) => {
                        return Err(failure::corrupt("a floating-point number in a column"));
                    }
                });
            }
            out.push(Row::new(values));
        }
        Ok(out)
    }

    /// Runs the statements of one migration.
    pub(crate) fn run_migration(&self, owner: Owner, sql: &str) -> Result<(), Failure> {
        self.scoped(Scope::Migration(owner));
        self.conn.execute_batch(sql).map_err(map)
    }

    /// The names of every table in the database, in order.
    pub(crate) fn table_names(&self) -> Result<Vec<String>, Failure> {
        let rows = self.query(
            Scope::Internal,
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            &[],
        )?;
        rows.iter()
            .map(|row| row.text(0).map(str::to_owned))
            .collect()
    }

    /// The ledger's own statements are prepared once and kept. A statement of
    /// another crate is prepared each time, because the authorizer looks at a
    /// statement only when it is prepared: a kept one would let a second
    /// crate reuse the first crate's permission for the same text.
    fn prepare(&self, scope: Scope, sql: &str) -> Result<Prepared<'_>, Failure> {
        if scope == Scope::Owner(Owner::Ledger) {
            self.conn
                .prepare_cached(sql)
                .map(Prepared::Kept)
                .map_err(map)
        } else {
            self.conn.prepare(sql).map(Prepared::Once).map_err(map)
        }
    }
}

enum Prepared<'c> {
    Kept(rusqlite::CachedStatement<'c>),
    Once(rusqlite::Statement<'c>),
}

impl<'c> std::ops::Deref for Prepared<'c> {
    type Target = rusqlite::Statement<'c>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Kept(statement) => statement,
            Self::Once(statement) => statement,
        }
    }
}

impl std::ops::DerefMut for Prepared<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Kept(statement) => statement,
            Self::Once(statement) => statement,
        }
    }
}

fn to_sqlite(value: &SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Int(number) => Value::Integer(*number),
        SqlValue::Text(text) => Value::Text(text.clone()),
        SqlValue::Bytes(bytes) => Value::Blob(bytes.clone()),
    }
}

/// Decides whether a statement may touch what it names. It is asked while a
/// statement is prepared, for every table the statement reads or changes,
/// including what its triggers do.
fn authorize(code: u32, cx: &AuthContext<'_>) -> Authorization {
    let allowed = match Scope::decode(code) {
        None => false,
        Some(Scope::Internal) => true,
        Some(Scope::Migration(owner)) => match cx.action {
            AuthAction::Attach { .. } | AuthAction::Detach { .. } => false,
            AuthAction::Read { table_name, .. }
            | AuthAction::Insert { table_name }
            | AuthAction::Update { table_name, .. }
            | AuthAction::Delete { table_name }
            | AuthAction::DropTable { table_name }
            | AuthAction::AlterTable { table_name, .. } => {
                // The database's own tables have no owner here.
                table_owner(table_name).is_none_or(|of| of == owner)
            }
            _ => true,
        },
        Some(Scope::Owner(owner)) => match cx.action {
            AuthAction::Select | AuthAction::Recursive | AuthAction::Function { .. } => true,
            AuthAction::Read { table_name, .. } => {
                table_owner(table_name) == Some(owner)
                    || (owner == Owner::Ledger && READ_FOR_RULE_THREE.contains(&table_name))
            }
            AuthAction::Insert { table_name }
            | AuthAction::Update { table_name, .. }
            | AuthAction::Delete { table_name } => table_owner(table_name) == Some(owner),
            _ => false,
        },
    };
    if allowed {
        Authorization::Allow
    } else {
        Authorization::Deny
    }
}

/// What a trigger of the migrations starts its message with.
const RULE: &str = "a2m: ";

/// Turns the database's error into a failure. The text the database gives
/// names tables, columns, and constraints; values are bound, never part of
/// a statement, so none can appear in it.
fn map(error: rusqlite::Error) -> Failure {
    let (code, message) = match error {
        rusqlite::Error::SqliteFailure(error, message) => (error.code, message.unwrap_or_default()),
        rusqlite::Error::SqlInputError { error, msg, .. } => (error.code, msg),
        _ => return failure::unavailable("the database refused a statement"),
    };
    match code {
        ErrorCode::DiskFull => failure::full(),
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => {
            failure::unavailable("the database is in use by another writer")
        }
        ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => {
            failure::corrupt("the database file")
        }
        ErrorCode::AuthorizationForStatementDenied => failure::not_owned(
            "it names a table its crate does not own, or does what only the store may",
        ),
        ErrorCode::ReadOnly => failure::unexpected("a read cannot change the store"),
        ErrorCode::ConstraintViolation => match message.strip_prefix(RULE) {
            Some(rule) => failure::unexpected(rule),
            None => failure::unexpected(message),
        },
        ErrorCode::Unknown => failure::unexpected(message),
        ErrorCode::CannotOpen | ErrorCode::PermissionDenied => {
            failure::unavailable("the database file cannot be opened")
        }
        _ => failure::unavailable("the database answered with an error"),
    }
}
