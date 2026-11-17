//! The ground-truth harness: gives a test a fresh database file and asks it, on
//! the stand's own connection, what is actually stored.
//!
//! A test never proves a write using Anything2MCP's own read path alone. It asks here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions, Permissions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, params_from_iter};

use crate::faults::{self, Fault};
use crate::rows::{Column, Row, Trigger, Value, WriteAction, WriteRecord, Writer};
use crate::schema::{self, Table};

/// How long the control interface waits for another connection's lock before it
/// gives up. A lock a test left open is reported, not waited out.
const CONTROL_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The separator between the parts of a key in the write log.
const KEY_SEPARATOR: char = '\u{1f}';

/// Why the stand could not do what a test asked.
#[derive(Debug)]
pub enum StandError {
    /// The folder or the file could not be made, changed, or removed.
    Io(std::io::Error),
    /// The engine refused a statement of the control interface.
    Sql(rusqlite::Error),
    /// The temporary folder's path is not Unicode, so it has no address form.
    Path(PathBuf),
    /// A key was given with the wrong number of parts for its table.
    WrongKey {
        /// The table asked about.
        table: Table,
        /// How many parts its key has.
        expected: usize,
        /// How many were given.
        given: usize,
    },
    /// The write log is switched off, so no count can be given.
    WriteLogOff,
    /// The fault was set and has no effect for the user running the test: a
    /// privileged user may write a file marked read-only. The fault was undone.
    FaultNotEffective(Fault),
}

impl fmt::Display for StandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "the stand's file could not be handled: {e}"),
            Self::Sql(e) => write!(f, "the store refused the control interface: {e}"),
            Self::Path(path) => write!(
                f,
                "the temporary folder {} is not Unicode",
                path.to_string_lossy()
            ),
            Self::WrongKey {
                table,
                expected,
                given,
            } => write!(
                f,
                "a key of {} has {expected} parts and {given} were given",
                table.name()
            ),
            Self::WriteLogOff => write!(f, "the write log is switched off"),
            Self::FaultNotEffective(fault) => write!(
                f,
                "the fault {fault:?} has no effect for the user running this test"
            ),
        }
    }
}

impl std::error::Error for StandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Sql(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StandError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}

impl From<std::io::Error> for StandError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// What the control interface holds.
struct Inner {
    /// The stand's own connection. The code under test never uses it.
    conn: Connection,
    /// The faults that are switched on.
    faults: BTreeSet<Fault>,
    /// Whether the triggers that keep the write log are installed.
    write_log: bool,
    /// The file's permissions before it was marked read-only.
    writable: Option<Permissions>,
}

/// The temporary folder, removed when dropped.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        // The folder is in the temporary directory. If a connection of the code
        // under test still holds the file open, some systems refuse; the folder is
        // then left for the system to clear.
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One database file in its own temporary folder, removed when dropped.
///
/// Two stands in one test share nothing. Drop the connections of the code under
/// test before the stand, so that the folder can be removed everywhere.
pub struct TestDb {
    // Order matters: the connection closes before the folder is removed.
    inner: Mutex<Inner>,
    path: PathBuf,
    address: String,
    _folder: Folder,
}

impl fmt::Debug for TestDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestDb").field("path", &self.path).finish()
    }
}

/// Counts the folders this process has made, so that each gets its own name.
static NEXT_FOLDER: AtomicU64 = AtomicU64::new(0);

impl TestDb {
    /// Makes a new database with the schema and the seed data, in a new folder
    /// under the system's temporary directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the folder or the database could not be made.
    pub fn start() -> Result<Self, StandError> {
        let folder = Folder(new_folder()?);
        let path = folder.0.join("example.db");
        let address = match path.to_str() {
            Some(text) => file_address(text, cfg!(windows)),
            None => return Err(StandError::Path(path)),
        };
        let conn = Connection::open(&path)?;
        conn.busy_timeout(CONTROL_BUSY_TIMEOUT)?;
        // Foreign keys are enforced by each connection, not by the file.
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        let faults = BTreeSet::new();
        atomically(&conn, |conn| {
            schema::rebuild(conn)?;
            faults::sync_triggers(conn, &faults, true)?;
            Ok(())
        })?;
        Ok(Self {
            inner: Mutex::new(Inner {
                conn,
                faults,
                write_log: true,
                writable: None,
            }),
            path,
            address,
            _folder: folder,
        })
    }

    /// The database file. The code under test opens its own connection to it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The database as an address the engine opens for reading and writing:
    /// `file:///...?mode=rw`. It never creates a missing file.
    #[must_use]
    pub fn uri(&self) -> String {
        format!("{}?mode=rw", self.address)
    }

    /// The database as an address the engine opens read-only: `file:///...?mode=ro`.
    /// The store refuses every write on a connection opened with it.
    #[must_use]
    pub fn read_only_uri(&self) -> String {
        format!("{}?mode=ro", self.address)
    }

    /// A new ordinary connection for reading and writing, as any user of the store
    /// would open. It is not the control interface: its writes are logged.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine could not open the file.
    pub fn connect(&self) -> Result<Connection, StandError> {
        Ok(Connection::open_with_flags(
            self.uri(),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
        )?)
    }

    /// A new ordinary connection the store holds read-only.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine could not open the file.
    pub fn connect_read_only(&self) -> Result<Connection, StandError> {
        Ok(Connection::open_with_flags(
            self.read_only_uri(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?)
    }

    /// Restores the schema and the seed data in the same file, empties the write
    /// log, switches every fault off, and switches the write log on.
    ///
    /// # Errors
    ///
    /// Returns an error if another connection holds a lock, or the file could not
    /// be changed. The stand is then in doubt: start a new one.
    pub fn reset(&self) -> Result<(), StandError> {
        let mut inner = self.lock();
        if inner.held_lock().is_some() {
            inner.conn.execute_batch("ROLLBACK")?;
            inner.faults.remove(&Fault::WritersLocked);
            inner.faults.remove(&Fault::Locked);
        }
        inner.restore_permissions(&self.path)?;
        inner.faults.remove(&Fault::ReadOnlyFile);
        let seed_state = BTreeSet::new();
        atomically(&inner.conn, |conn| {
            schema::rebuild(conn)?;
            faults::sync_triggers(conn, &seed_state, true)?;
            Ok(())
        })?;
        inner.faults = seed_state;
        inner.write_log = true;
        Ok(())
    }

    /// Exactly what is stored for one row, if anything. The key has one part per
    /// key column, parent first; for the table with no key, the engine's row number.
    ///
    /// # Errors
    ///
    /// Returns an error if the key has the wrong number of parts or the store could
    /// not be asked.
    pub fn stored_row(&self, table: Table, key: &[&str]) -> Result<Option<Row>, StandError> {
        let columns = table.locator_columns();
        if key.len() != columns.len() {
            return Err(StandError::WrongKey {
                table,
                expected: columns.len(),
                given: key.len(),
            });
        }
        let predicate: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(index, column)| format!("{column} = ?{}", index + 1))
            .collect();
        let sql = format!(
            "SELECT * FROM {} WHERE {}",
            table.name(),
            predicate.join(" AND ")
        );
        let inner = self.lock();
        let mut rows = read_rows(&inner.conn, &sql, key)?;
        Ok(rows.pop())
    }

    /// Every row of a table exactly as stored, in key order.
    ///
    /// # Errors
    ///
    /// Returns an error if the store could not be asked.
    pub fn stored_rows(&self, table: Table) -> Result<Vec<Row>, StandError> {
        let sql = format!(
            "SELECT * FROM {} ORDER BY {}",
            table.name(),
            table.locator_columns().join(", ")
        );
        let inner = self.lock();
        read_rows(&inner.conn, &sql, &[])
    }

    /// The columns of a table as the store's catalog has them now, in order.
    ///
    /// # Errors
    ///
    /// Returns an error if the store could not be asked.
    pub fn columns(&self, table: Table) -> Result<Vec<Column>, StandError> {
        let inner = self.lock();
        let mut statement = inner.conn.prepare(
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info(?1) ORDER BY cid",
        )?;
        let columns = statement.query_map([table.name()], |row| {
            Ok(Column {
                name: row.get(0)?,
                declared_type: row.get(1)?,
                not_null: row.get(2)?,
                key_position: row.get(3)?,
            })
        })?;
        Ok(columns.collect::<rusqlite::Result<_>>()?)
    }

    /// Every trigger in the store's catalog now, the stand's own included, by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the store could not be asked.
    pub fn triggers(&self) -> Result<Vec<Trigger>, StandError> {
        let inner = self.lock();
        let mut statement = inner.conn.prepare(
            "SELECT name, tbl_name FROM sqlite_master WHERE type = 'trigger' ORDER BY name",
        )?;
        let triggers = statement.query_map([], |row| {
            Ok(Trigger {
                name: row.get(0)?,
                table: row.get(1)?,
            })
        })?;
        Ok(triggers.collect::<rusqlite::Result<_>>()?)
    }

    /// Every change to a row since the last reset, oldest first, whoever made it.
    ///
    /// # Errors
    ///
    /// Returns an error if the write log is off or the store could not be asked.
    pub fn writes(&self) -> Result<Vec<WriteRecord>, StandError> {
        let inner = self.lock();
        if !inner.write_log {
            return Err(StandError::WriteLogOff);
        }
        let mut statement = inner.conn.prepare(
            "SELECT seq, table_name, row_key, action, actor FROM stand_write_log ORDER BY seq",
        )?;
        let mut rows = statement.query([])?;
        let mut records = Vec::new();
        while let Some(row) = rows.next()? {
            let table_name: String = row.get(1)?;
            let row_key: String = row.get(2)?;
            let action: String = row.get(3)?;
            let actor: String = row.get(4)?;
            // The log holds only what the stand's triggers wrote; a name that is
            // not one of theirs means the log was tampered with, and is left out.
            let Some(table) = Table::from_name(&table_name) else {
                continue;
            };
            let action = match action.as_str() {
                "insert" => WriteAction::Insert,
                "update" => WriteAction::Update,
                "delete" => WriteAction::Delete,
                _ => continue,
            };
            records.push(WriteRecord {
                seq: row.get(0)?,
                table,
                key: row_key.split(KEY_SEPARATOR).map(str::to_owned).collect(),
                action,
                writer: if actor == "stand" {
                    Writer::Stand
                } else {
                    Writer::Other
                },
            });
        }
        Ok(records)
    }

    /// How many times one row has been written since the last reset by any
    /// connection other than the control interface. A change the control interface
    /// made as someone else is in [`TestDb::writes`] and is not counted here.
    ///
    /// # Errors
    ///
    /// Returns an error if the write log is off or the store could not be asked.
    pub fn write_count(&self, table: Table, key: &[&str]) -> Result<usize, StandError> {
        let writes = self.writes()?;
        Ok(writes
            .iter()
            .filter(|w| w.table == table && w.writer == Writer::Other && w.key == key)
            .count())
    }

    /// A number that differs from an earlier reading if any other connection has
    /// committed a change to the file in between. Two equal readings mean nothing
    /// was committed, in any table, by anyone but the control interface.
    ///
    /// # Errors
    ///
    /// Returns an error if the store could not be asked.
    pub fn data_version(&self) -> Result<i64, StandError> {
        let inner = self.lock();
        Ok(inner
            .conn
            .query_row("PRAGMA data_version", [], |row| row.get(0))?)
    }

    /// Runs one statement as someone else: another user of the store, changing data
    /// between a preview and a write. Returns the number of rows it changed. The
    /// change is logged with [`Writer::Stand`] and moves versions as any write does.
    ///
    /// # Errors
    ///
    /// Returns an error if the store refused the statement; nothing is changed then.
    pub fn someone_else(&self, sql: &str, params: &[Value]) -> Result<usize, StandError> {
        let inner = self.lock();
        atomically(&inner.conn, |conn| {
            conn.execute_batch("UPDATE stand_state SET actor = 'stand'")?;
            let changed = conn.execute(sql, params_from_iter(params.iter()))?;
            conn.execute_batch("UPDATE stand_state SET actor = 'other'")?;
            Ok(changed)
        })
    }

    /// Switches a fault on. It stays on until cleared or reset. Switching on a
    /// fault that is on changes nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if the store or the file could not be changed, or the fault
    /// has no effect for the user running the test. The fault is then off.
    pub fn set_fault(&self, fault: Fault) -> Result<(), StandError> {
        let mut inner = self.lock();
        if inner.faults.contains(&fault) {
            return Ok(());
        }
        let mut next = inner.faults.clone();
        next.insert(fault);
        match fault {
            Fault::WritersLocked | Fault::Locked => {
                // One lock at a time: the engine has one.
                inner.release_lock()?;
                next = inner.faults.clone();
                next.insert(fault);
                inner.conn.execute_batch(if fault == Fault::Locked {
                    "BEGIN EXCLUSIVE"
                } else {
                    "BEGIN IMMEDIATE"
                })?;
            }
            Fault::ReadOnlyFile => {
                let before = fs::metadata(&self.path)?.permissions();
                let mut read_only = before.clone();
                read_only.set_readonly(true);
                fs::set_permissions(&self.path, read_only)?;
                // A privileged user writes a read-only file all the same. Say so,
                // rather than let a test pass without the fault.
                if OpenOptions::new().write(true).open(&self.path).is_ok() {
                    fs::set_permissions(&self.path, before)?;
                    return Err(StandError::FaultNotEffective(fault));
                }
                inner.writable = Some(before);
            }
            Fault::ColumnRenamed => {
                inner.reshape(&next, |conn| faults::rename_length(conn, true))?
            }
            Fault::ColumnRetyped => {
                inner.reshape(&next, |conn| faults::retype_price(conn, true))?
            }
            Fault::LengthRescaled
            | Fault::PriceRoundedDown
            | Fault::DescriptionPadded
            | Fault::TimeShifted
            | Fault::LineWritesRefused => inner.reshape(&next, |_| Ok(()))?,
        }
        inner.faults = next;
        Ok(())
    }

    /// Switches one fault off. A changed column gets its first name or type back.
    /// Switching off a fault that is off changes nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if the store or the file could not be changed. The fault is
    /// then still on.
    pub fn clear_fault(&self, fault: Fault) -> Result<(), StandError> {
        let mut inner = self.lock();
        if !inner.faults.contains(&fault) {
            return Ok(());
        }
        let mut next = inner.faults.clone();
        next.remove(&fault);
        match fault {
            Fault::WritersLocked | Fault::Locked => return inner.release_lock(),
            Fault::ReadOnlyFile => inner.restore_permissions(&self.path)?,
            Fault::ColumnRenamed => {
                inner.reshape(&next, |conn| faults::rename_length(conn, false))?;
            }
            Fault::ColumnRetyped => {
                inner.reshape(&next, |conn| faults::retype_price(conn, false))?;
            }
            Fault::LengthRescaled
            | Fault::PriceRoundedDown
            | Fault::DescriptionPadded
            | Fault::TimeShifted
            | Fault::LineWritesRefused => inner.reshape(&next, |_| Ok(()))?,
        }
        inner.faults = next;
        Ok(())
    }

    /// Switches every fault off. Stored data stays as it is.
    ///
    /// # Errors
    ///
    /// Returns the first error; the faults before it are off.
    pub fn clear_faults(&self) -> Result<(), StandError> {
        for fault in self.faults() {
            self.clear_fault(fault)?;
        }
        Ok(())
    }

    /// The faults that are switched on now.
    #[must_use]
    pub fn faults(&self) -> Vec<Fault> {
        self.lock().faults.iter().copied().collect()
    }

    /// Switches the write log on or off. Off removes the stand's logging triggers
    /// from the catalog, for a test of what a catalog read finds; no count can be
    /// asked for until it is on again.
    ///
    /// # Errors
    ///
    /// Returns an error if the store could not be changed.
    pub fn set_write_log(&self, on: bool) -> Result<(), StandError> {
        let mut inner = self.lock();
        if inner.write_log == on {
            return Ok(());
        }
        let faults = inner.faults.clone();
        atomically(&inner.conn, |conn| {
            Ok(faults::sync_triggers(conn, &faults, on)?)
        })?;
        inner.write_log = on;
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A test that panicked while holding the stand leaves it usable.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // Some systems do not remove a file marked read-only.
        let mut inner = self.lock();
        let _ = inner.restore_permissions(&self.path);
    }
}

impl Inner {
    /// The lock fault that is held, if one is.
    fn held_lock(&self) -> Option<Fault> {
        [Fault::WritersLocked, Fault::Locked]
            .into_iter()
            .find(|fault| self.faults.contains(fault))
    }

    /// Ends the transaction that holds a lock, keeping what the control interface
    /// did while it held it.
    fn release_lock(&mut self) -> Result<(), StandError> {
        if let Some(fault) = self.held_lock() {
            self.conn.execute_batch("COMMIT")?;
            self.faults.remove(&fault);
        }
        Ok(())
    }

    /// Gives the file back the permissions it had before it was marked read-only.
    fn restore_permissions(&mut self, path: &Path) -> Result<(), StandError> {
        if let Some(before) = self.writable.take() {
            if let Err(e) = fs::set_permissions(path, before.clone()) {
                self.writable = Some(before);
                return Err(StandError::Io(e));
            }
        }
        Ok(())
    }

    /// Changes the schema and brings the stand's triggers in line with the faults
    /// that will be on, as one step.
    fn reshape(
        &self,
        next: &BTreeSet<Fault>,
        change: impl FnOnce(&Connection) -> rusqlite::Result<()>,
    ) -> Result<(), StandError> {
        atomically(&self.conn, |conn| {
            // A column a trigger names can be neither dropped nor safely renamed.
            faults::drop_stand_triggers(conn)?;
            change(conn)?;
            faults::sync_triggers(conn, next, self.write_log)?;
            Ok(())
        })
    }
}

/// Runs several statements as one step: all of them, or none.
///
/// A savepoint, not a transaction, so that it also works while the control
/// interface holds a lock, which is an open transaction on the same connection.
fn atomically<T>(
    conn: &Connection,
    step: impl FnOnce(&Connection) -> Result<T, StandError>,
) -> Result<T, StandError> {
    conn.execute_batch("SAVEPOINT stand_step")?;
    let outcome = step(conn).and_then(|value| {
        conn.execute_batch("RELEASE stand_step")?;
        Ok(value)
    });
    if outcome.is_err() {
        // Undo, then close the savepoint. If the first statement fails there is
        // nothing more to try; the error returned is the one that mattered.
        let _ = conn.execute_batch("ROLLBACK TO stand_step; RELEASE stand_step");
    }
    outcome
}

/// Reads rows exactly as stored.
fn read_rows(conn: &Connection, sql: &str, key: &[&str]) -> Result<Vec<Row>, StandError> {
    let mut statement = conn.prepare(sql)?;
    let names: Vec<String> = statement.column_names().map(str::to_owned).collect();
    let mut rows = statement.query(params_from_iter(key.iter()))?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let mut values = BTreeMap::new();
        for (index, name) in names.iter().enumerate() {
            values.insert(name.clone(), Value::from_ref(row.get_ref(index)?));
        }
        found.push(Row::new(values));
    }
    Ok(found)
}

/// Makes a new, empty folder under the system's temporary directory.
fn new_folder() -> Result<PathBuf, StandError> {
    let base = std::env::temp_dir();
    let process = std::process::id();
    loop {
        let number = NEXT_FOLDER.fetch_add(1, Ordering::Relaxed);
        let folder = base.join(format!("example-db-{process}-{number}"));
        match fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            // Left behind by an earlier process with the same number: take the next.
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(StandError::Io(e)),
        }
    }
}

/// A file path as the address form the engine reads: `file:///` and the path, with
/// every character that means something in an address escaped.
fn file_address(path: &str, windows: bool) -> String {
    let path = if windows {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    };
    let mut address = String::from("file:///");
    for byte in path.trim_start_matches('/').bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                address.push(char::from(byte));
            }
            other => address.push_str(&format!("%{other:02X}")),
        }
    }
    address
}

#[cfg(test)]
mod tests {
    use super::file_address;

    #[test]
    fn an_address_escapes_what_a_path_may_hold() {
        assert_eq!(
            file_address("/tmp/my folder/a#b?c%d/example.db", false),
            "file:///tmp/my%20folder/a%23b%3Fc%25d/example.db"
        );
        assert_eq!(
            file_address(r"C:\Users\Example User\Temp\example.db", true),
            "file:///C:/Users/Example%20User/Temp/example.db"
        );
    }
}
