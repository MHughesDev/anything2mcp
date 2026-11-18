//! The one numbered sequence of migrations that covers the whole schema.
//!
//! A migration is a file in `migrations/`, named `NNNN-title.sql`. Its first
//! line names the crate that owns the tables it creates: `-- owner: a2m-x`.
//! The store runs it; the owner's code then reads and writes those tables
//! through [`Tx::tables`](crate::Tx::tables). A migration that was applied is
//! never edited after a release has carried it: its checksum is kept, and a
//! store whose applied migration differs from the one this engine carries is
//! refused.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use a2m_core::{Digest, Failure, Timestamp};

use crate::failure;
use crate::sqlite::{Db, Scope};
use crate::tx::{Durability, Owner, SqlValue};

/// The files, listed by the build script: number, file name, text.
const FILES: &[(u32, &str, &str)] = include!(concat!(env!("OUT_DIR"), "/migrations.rs"));

/// What the first line of a migration starts with.
const OWNER_LINE: &str = "-- owner: ";

/// The table the store keeps its own applied migrations in.
const OWN_TABLE: &str = "schema_migrations";

/// One migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    /// Its place in the sequence.
    pub number: u32,
    /// Its file's name.
    pub name: &'static str,
    /// The crate that owns the tables it creates.
    pub owner: Owner,
    /// The tables it creates, in the order it creates them, including a
    /// table of its crate it builds again (migration 0026 builds `watches`
    /// and `watch_changes` again).
    pub tables: Vec<String>,
    /// The digest of its text, with line endings made the same everywhere.
    pub checksum: Digest,
    sql: &'static str,
}

struct Known {
    migrations: Vec<Migration>,
    owners: BTreeMap<String, Owner>,
}

fn known() -> &'static Result<Known, String> {
    static KNOWN: OnceLock<Result<Known, String>> = OnceLock::new();
    KNOWN.get_or_init(|| {
        let mut migrations = Vec::new();
        let mut owners = BTreeMap::from([(OWN_TABLE.to_owned(), Owner::Ledger)]);
        for (number, name, sql) in FILES {
            let owner = sql
                .lines()
                .next()
                .and_then(|line| line.trim_end().strip_prefix(OWNER_LINE))
                .and_then(Owner::from_crate_name)
                .ok_or_else(|| {
                    format!("migration `{name}` does not name its owner on its first line")
                })?;
            let tables = created_tables(sql);
            for table in &tables {
                // A table built again by its own crate, to widen what a
                // column may hold, is created twice by the one owner; one
                // created by two crates is refused.
                if let Some(other) = owners.insert(table.clone(), owner)
                    && other != owner
                {
                    return Err(format!(
                        "table `{table}` is created twice, by {} and by {}",
                        other.crate_name(),
                        owner.crate_name()
                    ));
                }
            }
            let text: String = sql.chars().filter(|character| *character != '\r').collect();
            migrations.push(Migration {
                number: *number,
                name,
                owner,
                tables,
                checksum: Digest::of(text.as_bytes()),
                sql,
            });
        }
        Ok(Known { migrations, owners })
    })
}

/// The names after each `CREATE TABLE` of a migration.
fn created_tables(sql: &str) -> Vec<String> {
    let mut tables = Vec::new();
    for line in sql.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("CREATE TABLE ") {
            let name: String = rest
                .chars()
                .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                .collect();
            if !name.is_empty() {
                tables.push(name);
            }
        }
    }
    tables
}

/// Every migration this engine carries, in order.
///
/// # Errors
///
/// `busy`/`store_corrupt` when a migration file names no owner or the files
/// of two crates create one table. A test holds this to never happening in a build.
pub fn migrations() -> Result<&'static [Migration], Failure> {
    match known() {
        Ok(known) => Ok(&known.migrations),
        Err(_) => Err(failure::corrupt("the migrations this engine carries")),
    }
}

/// The crate that owns `table`, or `None` for a table no migration creates.
#[must_use]
pub fn table_owner(table: &str) -> Option<Owner> {
    known()
        .as_ref()
        .ok()
        .and_then(|known| known.owners.get(table).copied())
}

/// Brings a database to this engine's schema. A migration that was applied
/// is not applied again; a database that holds a migration this engine does
/// not carry, or carries differently, is refused and left as it was.
pub(crate) fn migrate(db: &Db, now: Timestamp) -> Result<(), Failure> {
    let known = migrations()?;
    db.execute(
        Scope::Internal,
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            number INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            owner TEXT NOT NULL,
            checksum TEXT NOT NULL,
            applied_at INTEGER NOT NULL
        ) STRICT",
        &[],
    )?;
    let mut applied = BTreeMap::new();
    for row in db.query(
        Scope::Internal,
        "SELECT number, checksum FROM schema_migrations ORDER BY number",
        &[],
    )? {
        applied.insert(row.int(0)?, row.text(1)?.to_owned());
    }
    for (number, checksum) in &applied {
        let ours = known
            .iter()
            .find(|migration| i64::from(migration.number) == *number)
            .ok_or_else(|| {
                failure::version_unsupported(
                    "it holds a migration this engine does not carry, so a newer engine wrote it",
                )
            })?;
        if ours.checksum.to_string() != *checksum {
            return Err(failure::version_unsupported(
                "a migration it applied is not the one this engine carries",
            ));
        }
    }
    for migration in known {
        if applied.contains_key(&i64::from(migration.number)) {
            continue;
        }
        db.begin(Some(Durability::Synced))?;
        let done = db
            .run_migration(migration.owner, migration.sql)
            .and_then(|()| {
                db.execute(
                    Scope::Internal,
                    "INSERT INTO schema_migrations (number, name, owner, checksum, applied_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                    &[
                        SqlValue::from(migration.number),
                        SqlValue::from(migration.name),
                        SqlValue::from(migration.owner.crate_name()),
                        SqlValue::from(migration.checksum.to_string()),
                        SqlValue::from(now),
                    ],
                )
            });
        match done {
            Ok(_) => db.end(true, true)?,
            Err(failure) => {
                let _ = db.end(false, true);
                return Err(failure);
            }
        }
    }
    Ok(())
}
