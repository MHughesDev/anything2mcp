//! Faults a test can switch on, so that a failure is reproduced on demand, and the
//! triggers that keep the write log.
//!
//! Every trigger here is the stand's own and carries the stand's prefix. A trigger
//! that changes the row just written raises `stand_state.internal` while it does,
//! so that its change is not logged as a write and does not change the version a
//! second time. That holds whether or not the writing connection lets a trigger
//! fire itself.

use std::collections::BTreeSet;

use rusqlite::Connection;

use crate::schema::Table;

/// One fault. Each stays on until it is cleared or the stand is reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fault {
    /// The control interface holds the engine's write lock. Other connections read,
    /// and a write by any of them is answered busy. The engine locks the whole
    /// database, not a row.
    WritersLocked,
    /// The control interface holds the engine's exclusive lock. A read or a write by
    /// any other connection is answered busy.
    Locked,
    /// A length written to `order_lines` is stored at four decimal places: `150.5`
    /// is stored as `150.5000`. The same value at another scale.
    LengthRescaled,
    /// A price written to `order_lines` is stored without its cents: `12.49` is
    /// stored as `12.00`. A different value.
    PriceRoundedDown,
    /// A description written to `order_lines` is stored padded with spaces to 24
    /// characters.
    DescriptionPadded,
    /// A time written to `orders.submitted_at` is stored in the zone five hours
    /// behind: `2026-03-06T09:30:00+00:00` is stored as `2026-03-06T04:30:00-05:00`.
    /// The same instant in another zone.
    TimeShifted,
    /// A rule in the store refuses every insert, update, and delete on
    /// `order_lines`, with the message [`LINES_FROZEN_MESSAGE`].
    LineWritesRefused,
    /// The column `order_lines.length_in` is renamed to `length_inches`.
    ColumnRenamed,
    /// The column `order_lines.unit_price` changes from decimal text to a whole
    /// number of cents: `12.50` becomes `1250`, and the column moves to the end.
    ColumnRetyped,
    /// The database file is marked read-only for the operating system. Connections
    /// opened afterwards can read and cannot write; a connection that was already
    /// open keeps the access it had.
    ReadOnlyFile,
}

/// What the store says when [`Fault::LineWritesRefused`] refuses a write.
pub const LINES_FROZEN_MESSAGE: &str = "order lines are frozen";

/// The name of the length column before and after [`Fault::ColumnRenamed`].
pub(crate) const LENGTH_COLUMNS: (&str, &str) = ("length_in", "length_inches");

/// The length column's name under these faults.
pub(crate) fn length_column(faults: &BTreeSet<Fault>) -> &'static str {
    if faults.contains(&Fault::ColumnRenamed) {
        LENGTH_COLUMNS.1
    } else {
        LENGTH_COLUMNS.0
    }
}

/// Drops every trigger of the stand's, then creates the ones this state calls for.
pub(crate) fn sync_triggers(
    conn: &Connection,
    faults: &BTreeSet<Fault>,
    write_log: bool,
) -> rusqlite::Result<()> {
    drop_stand_triggers(conn)?;
    if write_log {
        for table in Table::ALL {
            conn.execute_batch(&log_triggers(table))?;
        }
    }
    for fault in faults {
        if let Some(sql) = fault_triggers(*fault, faults) {
            conn.execute_batch(&sql)?;
        }
    }
    Ok(())
}

/// Drops every trigger whose name starts with the stand's prefix.
pub(crate) fn drop_stand_triggers(conn: &Connection) -> rusqlite::Result<()> {
    let names: Vec<String> = {
        let mut statement = conn.prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'trigger' AND substr(name, 1, 6) = 'stand_' ORDER BY name",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for name in names {
        conn.execute_batch(&format!("DROP TRIGGER \"{name}\""))?;
    }
    Ok(())
}

/// The three triggers that log each insert, update, and delete on one table.
fn log_triggers(table: Table) -> String {
    let name = table.name();
    let key = |row: &str| {
        table
            .locator_columns()
            .iter()
            .map(|column| format!("CAST({row}.{column} AS TEXT)"))
            .collect::<Vec<_>>()
            .join(" || char(31) || ")
    };
    [("insert", "NEW"), ("update", "NEW"), ("delete", "OLD")]
        .iter()
        .map(|(action, row)| {
            format!(
                "CREATE TRIGGER stand_log_{name}_{action} AFTER {action} ON {name}
                 WHEN (SELECT internal FROM stand_state) = 0
                 BEGIN
                     INSERT INTO stand_write_log (table_name, row_key, action, actor)
                     VALUES ('{name}', {key}, '{action}', (SELECT actor FROM stand_state));
                 END;",
                key = key(row),
            )
        })
        .collect()
}

/// The triggers one fault needs, if it works by triggers.
fn fault_triggers(fault: Fault, faults: &BTreeSet<Fault>) -> Option<String> {
    let length = length_column(faults);
    match fault {
        Fault::LengthRescaled => Some(rewrite_triggers(
            "rescale",
            "order_lines",
            &format!(
                "{length} = CASE WHEN instr(NEW.{length}, '.') = 0 THEN NEW.{length} || '.0000' \
                 ELSE substr(NEW.{length} || '0000', 1, instr(NEW.{length}, '.') + 4) END"
            ),
            "order_id = NEW.order_id AND item_code = NEW.item_code",
        )),
        Fault::PriceRoundedDown => {
            let set = if faults.contains(&Fault::ColumnRetyped) {
                "unit_price = (NEW.unit_price / 100) * 100"
            } else {
                "unit_price = CASE WHEN instr(NEW.unit_price, '.') = 0 \
                 THEN NEW.unit_price || '.00' \
                 ELSE substr(NEW.unit_price, 1, instr(NEW.unit_price, '.') - 1) || '.00' END"
            };
            Some(rewrite_triggers(
                "round_down",
                "order_lines",
                set,
                "order_id = NEW.order_id AND item_code = NEW.item_code",
            ))
        }
        Fault::DescriptionPadded => Some(rewrite_triggers(
            "pad",
            "order_lines",
            "description = printf('%-24s', NEW.description)",
            "order_id = NEW.order_id AND item_code = NEW.item_code",
        )),
        Fault::TimeShifted => Some(rewrite_triggers(
            "shift_time",
            "orders",
            "submitted_at = COALESCE(\
             strftime('%Y-%m-%dT%H:%M:%S', NEW.submitted_at, '-5 hours') || '-05:00', \
             NEW.submitted_at)",
            "order_id = NEW.order_id",
        )),
        Fault::LineWritesRefused => Some(
            ["insert", "update", "delete"]
                .iter()
                .map(|action| {
                    format!(
                        "CREATE TRIGGER stand_fault_frozen_{action} BEFORE {action} ON order_lines
                         WHEN (SELECT internal FROM stand_state) = 0
                         BEGIN
                             SELECT RAISE(ABORT, '{LINES_FROZEN_MESSAGE}');
                         END;"
                    )
                })
                .collect(),
        ),
        Fault::WritersLocked
        | Fault::Locked
        | Fault::ColumnRenamed
        | Fault::ColumnRetyped
        | Fault::ReadOnlyFile => None,
    }
}

/// Two triggers, after insert and after update, that store a written row
/// differently from what was sent.
fn rewrite_triggers(name: &str, table: &str, set: &str, row: &str) -> String {
    ["insert", "update"]
        .iter()
        .map(|action| {
            format!(
                "CREATE TRIGGER stand_fault_{name}_{action} AFTER {action} ON {table}
                 WHEN (SELECT internal FROM stand_state) = 0
                 BEGIN
                     UPDATE stand_state SET internal = internal + 1;
                     UPDATE {table} SET {set} WHERE {row};
                     UPDATE stand_state SET internal = internal - 1;
                 END;"
            )
        })
        .collect()
}

/// Renames the length column, forwards or back.
pub(crate) fn rename_length(conn: &Connection, renamed: bool) -> rusqlite::Result<()> {
    let (from, to) = if renamed {
        LENGTH_COLUMNS
    } else {
        (LENGTH_COLUMNS.1, LENGTH_COLUMNS.0)
    };
    conn.execute_batch(&format!(
        "ALTER TABLE order_lines RENAME COLUMN {from} TO {to}"
    ))
}

/// Changes the price column to whole cents, or back to decimal text.
///
/// The caller has dropped the stand's triggers, because a column a trigger names
/// cannot be dropped. Row versions do not move: this is a change of schema, not a
/// write to a record.
pub(crate) fn retype_price(conn: &Connection, to_cents: bool) -> rusqlite::Result<()> {
    let (new_column, convert) = if to_cents {
        (
            "unit_price_new INTEGER NOT NULL DEFAULT 0",
            "CASE WHEN instr(unit_price, '.') = 0 THEN CAST(unit_price AS INTEGER) * 100 \
             ELSE CAST(substr(unit_price, 1, instr(unit_price, '.') - 1) AS INTEGER) * 100 \
             + CAST(substr(substr(unit_price, instr(unit_price, '.') + 1) || '00', 1, 2) \
             AS INTEGER) END",
        )
    } else {
        (
            "unit_price_new TEXT NOT NULL DEFAULT '0.00'",
            "printf('%d.%02d', unit_price / 100, unit_price % 100)",
        )
    };
    conn.execute_batch(&format!(
        "ALTER TABLE order_lines ADD COLUMN {new_column};
         UPDATE stand_state SET internal = internal + 1;
         UPDATE order_lines SET unit_price_new = {convert};
         UPDATE stand_state SET internal = internal - 1;
         ALTER TABLE order_lines DROP COLUMN unit_price;
         ALTER TABLE order_lines RENAME COLUMN unit_price_new TO unit_price;"
    ))
}
