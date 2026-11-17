//! The pack's own small interface over one kind of relational store.
//!
//! Everything that is specific to a store is behind [`Opener`] and
//! [`Conn`]: how a connection is opened read-only and proved so, how a
//! transaction begins, how a statement is described and run, what an error
//! of the store means, and how its catalog is read. The driver, the
//! discoverer, and the compiler know only this interface, so that a server
//! store can be added beside the embedded one.
//!
//! Every method here blocks. The driver calls them on a blocking thread and
//! never on an async one.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::model::{Access, Isolation};

/// A value bound to one parameter of a statement. A decimal, a date, and a
/// moment are bound as their exact text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Bound {
    /// No value.
    Null,
    /// A whole number.
    Whole(i64),
    /// Text.
    Text(String),
}

/// One stored value, as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cell {
    /// No value.
    Null,
    /// A whole number.
    Whole(i64),
    /// Text.
    Text(String),
    /// A float. Its value is not kept: no rule reads one.
    Float,
    /// Bytes. Their content is not kept: no rule reads them.
    Bytes,
}

/// What the store reports about a statement it prepared and did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shape {
    /// How many parameters it takes.
    pub params: usize,
    /// The names of its result columns, in order.
    pub columns: Vec<String>,
    /// Whether the store says the statement changes nothing it stores.
    pub reads_only: bool,
}

/// What running a statement gave.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Ran {
    /// The names of the result columns, in order.
    pub columns: Vec<String>,
    /// The rows read, at most the limit asked for.
    pub rows: Vec<Vec<Cell>>,
    /// Whether the store had at least one more row.
    pub more: bool,
    /// How many rows of the named table the statement changed. Nothing a
    /// trigger or a cascading key did is counted.
    pub changed: u64,
    /// The bytes of text in the rows read.
    pub bytes: u64,
}

/// The limits one run of a statement is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunLimits {
    /// The most rows kept. One more is asked for, so that "more exist" is a
    /// fact.
    pub rows: u32,
    /// The most bytes of text kept.
    pub bytes: u64,
    /// How long to wait for a lock.
    pub wait: Duration,
}

/// What a store's error means, in the pack's own words. The store's own
/// text is never kept: it may quote a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Another connection held a lock for the whole wait.
    Locked,
    /// The statement was interrupted at its time limit.
    TimedOut,
    /// A constraint refused the write; which kind, as a word of the pack's.
    Constraint(&'static str),
    /// The connection may not write, and the statement tried to.
    ReadOnly,
    /// The store denied the identity a right.
    Denied,
    /// The statement does not prepare, or takes other parameters than the
    /// plan says: a table, a column, or the statement's shape changed.
    Shape,
    /// The store cannot be opened or reached.
    Unavailable,
    /// The result holds more text than the limit allows.
    TooLarge,
    /// The connection's own guard refused the statement: it is not one
    /// plain read or one keyed write.
    NotAllowed,
    /// The store did not take the setting that makes the connection safe to
    /// use: read-only for a reader, enforced keys for a writer.
    NotGuarded,
    /// An error with no rule; the store's primary result code.
    Other(i32),
}

/// How a `commit` failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitFailed {
    /// The store said it did not commit. Nothing landed.
    NotCommitted(Refusal),
    /// Nothing says whether the transaction committed.
    Unknown,
}

/// One column, as the catalog describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnInfo {
    /// The column's name.
    pub name: String,
    /// The type it was declared with, as written.
    pub declared_type: String,
    /// Whether it refuses a null.
    pub not_null: bool,
    /// Its place in the primary key, from 1; 0 when it is not part of it.
    pub key_position: u32,
}

/// One foreign key of a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForeignKey {
    /// The columns of this table.
    pub columns: Vec<String>,
    /// The table they refer to.
    pub references: String,
    /// What a change of the referred key does here, as the store names it.
    pub on_update: String,
    /// What a deletion of the referred row does here, as the store names it.
    pub on_delete: String,
}

impl ForeignKey {
    /// Whether a write to the referred table changes rows of this one.
    pub(crate) fn cascades(&self) -> bool {
        let acts = |rule: &str| {
            let rule = rule.to_ascii_uppercase();
            rule == "CASCADE" || rule == "SET NULL" || rule == "SET DEFAULT"
        };
        acts(&self.on_update) || acts(&self.on_delete)
    }
}

/// One table or view, as the catalog describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableInfo {
    /// Its name.
    pub name: String,
    /// Whether it is a view.
    pub view: bool,
    /// Its columns, in declared order.
    pub columns: Vec<ColumnInfo>,
    /// Its unique constraints beside the primary key: the columns of each.
    pub unique: Vec<Vec<String>>,
    /// Its foreign keys.
    pub foreign_keys: Vec<ForeignKey>,
    /// The names of the triggers on it.
    pub triggers: Vec<String>,
}

/// The store's catalog, as far as it was read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Catalog {
    /// The tables and views, in order of name.
    pub tables: Vec<TableInfo>,
    /// Whether more exist than were read.
    pub truncated: bool,
}

/// Why a catalog cannot say what a write to a table reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unseen {
    /// More tables exist than were read, so a trigger or a cascading key
    /// may be among those not read.
    Truncated,
    /// The table is not in the catalog.
    NoTable,
}

/// Whether two names of the store are the same name. A relational store
/// compares the names of its tables without regard to the case of ASCII
/// letters, however they were written and whether or not they were quoted.
pub(crate) fn same_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

impl Catalog {
    /// How a write to `table` reaches beyond the rows it names: the number
    /// of triggers on the table, and of keys in any table that cascade from
    /// it. A catalog that was cut off, or that does not hold the table,
    /// cannot say, and says so: it is never read as "nothing".
    pub(crate) fn reach(&self, table: &str) -> Result<(usize, usize), Unseen> {
        if self.truncated {
            return Err(Unseen::Truncated);
        }
        let triggers = self
            .tables
            .iter()
            .find(|info| same_name(&info.name, table))
            .map(|info| info.triggers.len())
            .ok_or(Unseen::NoTable)?;
        let cascades = self
            .tables
            .iter()
            .flat_map(|info| &info.foreign_keys)
            .filter(|key| same_name(&key.references, table) && key.cascades())
            .count();
        Ok((triggers, cascades))
    }
}

/// Stops the statement a connection is running, from another thread.
pub(crate) type Interrupter = Arc<dyn Fn() + Send + Sync>;

/// One open connection to a store.
pub(crate) trait Conn: Send {
    /// The access the store itself holds the connection to.
    fn access(&self) -> Access;

    /// Begins a transaction of the connection's access at the isolation
    /// asked for, or a stronger one. A writer takes its lock here, so that a
    /// lock wait ends before anything is written.
    fn begin(&mut self, isolation: Isolation, wait: Duration) -> Result<(), Refusal>;

    /// Whether a transaction is open, as the store reports it.
    fn in_transaction(&self) -> bool;

    /// Prepares a statement of a plan without running it. The connection
    /// refuses one that is not a plain read or a keyed write, whatever its
    /// words are.
    fn describe(&mut self, text: &str) -> Result<Shape, Refusal>;

    /// Runs a statement of a plan with bound parameters. A connection the
    /// store holds read-only runs only a statement that the store says
    /// changes nothing and that returns columns.
    fn run(&mut self, text: &str, params: &[Bound], limits: &RunLimits) -> Result<Ran, Refusal>;

    /// Commits the open transaction.
    fn commit(&mut self, wait: Duration) -> Result<(), CommitFailed>;

    /// Rolls back whatever is open. Never fails: a connection that cannot
    /// roll back is closed by its owner, and the store rolls back for it.
    fn rollback(&mut self);

    /// Reads the catalog: at most `tables` tables and views. When more
    /// exist, the catalog says that it was cut off.
    fn catalog(&mut self, tables: usize) -> Result<Catalog, Refusal>;

    /// What stops the statement this connection is running.
    fn interrupter(&self) -> Interrupter;
}

/// One kind of store: its name, its quoting rule, and how it is opened.
pub(crate) trait Opener: Send + Sync {
    /// The store's name, as a surface states it under `store`.
    fn store(&self) -> &'static str;

    /// An identifier from a target file, quoted by the dialect's rule. `None`
    /// when the dialect cannot quote it.
    fn quote(&self, identifier: &str) -> Option<String>;

    /// The dialect's positional placeholder.
    fn placeholder(&self) -> &'static str;

    /// Whether an insert can return the key the store assigned.
    fn returns_assigned_key(&self) -> bool;

    /// Opens a connection to the store's file, held to `access` and to that
    /// one file by the store, with each setting read back before it is
    /// returned.
    fn open(&self, file: &Path, access: Access) -> Result<Box<dyn Conn>, Refusal>;
}

/// The stores this build speaks to, by name. Server stores are not built.
pub(crate) fn opener(store: &str) -> Option<Arc<dyn Opener>> {
    (store == crate::sqlite::Sqlite.store())
        .then(|| Arc::new(crate::sqlite::Sqlite) as Arc<dyn Opener>)
}

/// The names of the stores this build speaks to.
pub(crate) const STORES: &[&str] = &["sqlite"];
