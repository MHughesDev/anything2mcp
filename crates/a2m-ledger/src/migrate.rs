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

/// The names after each `CREATE TABLE`, `CREATE INDEX`, `CREATE UNIQUE
/// INDEX`, `CREATE TRIGGER`, and `CREATE VIEW` of a migration: every object
/// of the database it makes.
fn created_objects(sql: &str) -> Vec<String> {
    let mut objects = Vec::new();
    for line in sql.lines() {
        let line = line.trim_start();
        let rest = [
            "CREATE TABLE ",
            "CREATE INDEX ",
            "CREATE UNIQUE INDEX ",
            "CREATE TRIGGER ",
            "CREATE VIEW ",
        ]
        .iter()
        .find_map(|start| line.strip_prefix(start));
        let Some(rest) = rest else { continue };
        let rest = rest.strip_prefix("IF NOT EXISTS ").unwrap_or(rest);
        let name: String = rest
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect();
        if !name.is_empty() {
            objects.push(name);
        }
    }
    objects
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
    migrate_through(db, now, migrations()?)
}

/// [`migrate`], over the sequence `known`: the engine's own, or one a test
/// makes.
fn migrate_through(db: &Db, now: Timestamp, known: &[Migration]) -> Result<(), Failure> {
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
    // Section 224: the engine refuses a half-migrated store. Migrations are
    // applied in order, each in one transaction with the row that records
    // it, so a store this engine migrated holds the first of them and no
    // other, and nothing that a migration it has not recorded makes.
    let in_order = known
        .iter()
        .take(applied.len())
        .all(|migration| applied.contains_key(&i64::from(migration.number)));
    if !in_order {
        return Err(failure::version_unsupported(
            "it holds a later migration without an earlier one",
        ));
    }
    if applied.len() < known.len() {
        let mut made: Vec<String> = known
            .iter()
            .take(applied.len())
            .flat_map(|migration| created_objects(migration.sql))
            .collect();
        made.push(OWN_TABLE.to_owned());
        for row in db.query(
            Scope::Internal,
            "SELECT name FROM sqlite_master
             WHERE type IN ('table', 'index', 'trigger', 'view') AND name NOT LIKE 'sqlite_%'",
            &[],
        )? {
            let name = row.text(0)?;
            if !made.iter().any(|made| made == name) {
                return Err(failure::version_unsupported(
                    "it holds what a migration makes without the record of that migration, so it was half migrated or changed outside this engine",
                ));
            }
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

#[cfg(test)]
mod tests {
    use a2m_core::{Digest, Timestamp};

    use super::{Migration, created_tables, migrate_through};
    use crate::sqlite::{Db, Place, Scope};
    use crate::tx::Owner;

    /// A migration made for a test, with the number and the text given.
    fn made(number: u32, name: &'static str, sql: &'static str) -> Migration {
        Migration {
            number,
            name,
            owner: Owner::Ledger,
            tables: created_tables(sql),
            checksum: Digest::of(sql.as_bytes()),
            sql,
        }
    }

    /// Every object of the database, and every row of the applied
    /// migrations, as text.
    fn held(db: &Db) -> (Vec<String>, Vec<String>) {
        let objects = db
            .query(
                Scope::Internal,
                "SELECT type || ' ' || name FROM sqlite_master
                 WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| row.text(0).unwrap().to_owned())
            .collect();
        let rows = db
            .query(
                Scope::Internal,
                "SELECT number || ' ' || name || ' ' || checksum FROM schema_migrations
                 ORDER BY number",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| row.text(0).unwrap().to_owned())
            .collect();
        (objects, rows)
    }

    /// Section 224: a migration interrupted in its middle leaves none half.
    /// Its first two statements make a table and an index, and its third
    /// fails, as a statement does on a full disk or a lost connection;
    /// the store holds what it held before, and no row of it. The next run
    /// with a good migration of that number applies it once.
    #[test]
    fn a_migration_that_fails_in_its_middle_leaves_neither_its_objects_nor_its_row() {
        let now = Timestamp::from_unix_millis(1_800_000_000_000);
        let db = Db::open(&Place::Memory, false).unwrap();
        let first = made(
            1,
            "0001-first.sql",
            "-- owner: a2m-ledger\nCREATE TABLE test_first (x INTEGER NOT NULL) STRICT;\n",
        );
        migrate_through(&db, now, std::slice::from_ref(&first)).unwrap();
        db.execute(
            Scope::Internal,
            "INSERT INTO test_first (x) VALUES (7)",
            &[],
        )
        .unwrap();
        let before = held(&db);
        assert_eq!(before.1.len(), 1, "{before:?}");

        let broken = made(
            2,
            "0002-second.sql",
            "-- owner: a2m-ledger\n\
             CREATE TABLE test_second (y INTEGER NOT NULL) STRICT;\n\
             CREATE INDEX test_second_y ON test_second (y);\n\
             INSERT INTO test_missing (z) VALUES (1);\n",
        );
        let failed = migrate_through(&db, now, &[first.clone(), broken]).unwrap_err();
        // It failed in the migration, and was not refused before it.
        assert!(failed.message().contains("test_missing"), "{failed:?}");
        assert_eq!(held(&db), before);
        // The row the first migration's table held is there still.
        let kept = db
            .query(Scope::Internal, "SELECT x FROM test_first", &[])
            .unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].int(0).unwrap(), 7);

        // The next open, with a good migration of that number, applies it
        // once; another open applies nothing.
        let good = made(
            2,
            "0002-second.sql",
            "-- owner: a2m-ledger\n\
             CREATE TABLE test_second (y INTEGER NOT NULL) STRICT;\n\
             CREATE INDEX test_second_y ON test_second (y);\n",
        );
        let known = [first, good];
        migrate_through(&db, now, &known).unwrap();
        let after = held(&db);
        assert_eq!(after.1.len(), 2, "{after:?}");
        assert!(after.1[1].starts_with("2 0002-second.sql "), "{after:?}");
        assert!(
            after.0.contains(&"table test_second".to_owned()),
            "{after:?}"
        );
        assert!(
            after.0.contains(&"index test_second_y".to_owned()),
            "{after:?}"
        );
        migrate_through(&db, now, &known).unwrap();
        assert_eq!(held(&db), after);
    }
}
