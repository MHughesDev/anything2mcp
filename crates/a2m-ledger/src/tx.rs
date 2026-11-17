//! The transaction: the only way to read or write a table.
//!
//! A [`Tx`] lives inside one closure given to [`Store::write`] or
//! [`Store::read`](crate::Store::read), on the store's own thread, so it is
//! never held across an `await` and the single writer is never left waiting
//! for a task that went elsewhere.
//!
//! [`Store::write`]: crate::Store::write

use a2m_core::{Failure, Timestamp};

use crate::failure;
use crate::sqlite::{Db, Scope};
use crate::store::Shared;

/// How hard a commit is pushed to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// Synced to disk before the commit returns. For intents, approvals,
    /// keys, and the audit chain.
    Synced,
    /// Committed, and synced with the next checkpoint. For the run rows of
    /// reads. A crash of the machine may lose the last such commits; a crash
    /// of the process loses nothing.
    Relaxed,
}

/// A crate that owns the meaning of some tables. The store owns the
/// connection, the transactions, and the migrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Owner {
    /// `a2m-ledger`: runs, events, records, locks, blobs, findings,
    /// versions, the audit chain, the vault's keys.
    Ledger,
    /// `a2m-policy`: decisions, approvals, grants, and the rest of what only
    /// the policy engine decides.
    Policy,
    /// `a2m-auth`: sessions, their events, and saved session state.
    Auth,
    /// `a2m-sched`: jobs, leases, runners, budgets.
    Sched,
    /// `a2m-engine`: the health of each binding, written through `Control`.
    Engine,
    /// `a2m-actions`: toolsets and the scout's stage results.
    Actions,
}

impl Owner {
    /// Every owner, in layer order.
    pub const ALL: [Self; 6] = [
        Self::Ledger,
        Self::Policy,
        Self::Auth,
        Self::Sched,
        Self::Engine,
        Self::Actions,
    ];

    /// The crate's name, as the first line of its migrations writes it.
    #[must_use]
    pub const fn crate_name(self) -> &'static str {
        match self {
            Self::Ledger => "a2m-ledger",
            Self::Policy => "a2m-policy",
            Self::Auth => "a2m-auth",
            Self::Sched => "a2m-sched",
            Self::Engine => "a2m-engine",
            Self::Actions => "a2m-actions",
        }
    }

    /// The owner a migration's first line names.
    #[must_use]
    pub fn from_crate_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|owner| owner.crate_name() == name)
    }

    pub(crate) const fn index(self) -> u32 {
        self as u32
    }

    pub(crate) fn from_index(index: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|owner| owner.index() == index)
    }
}

/// One value bound to a statement or read from a row. The store keeps no
/// floating-point number, so there is none here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlValue {
    /// No value.
    Null,
    /// A whole number.
    Int(i64),
    /// Text.
    Text(String),
    /// Bytes.
    Bytes(Vec<u8>),
}

impl From<i64> for SqlValue {
    fn from(number: i64) -> Self {
        Self::Int(number)
    }
}

impl From<u32> for SqlValue {
    fn from(number: u32) -> Self {
        Self::Int(i64::from(number))
    }
}

impl From<bool> for SqlValue {
    fn from(flag: bool) -> Self {
        Self::Int(i64::from(flag))
    }
}

impl From<&str> for SqlValue {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for SqlValue {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<Vec<u8>> for SqlValue {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes(bytes)
    }
}

impl From<Timestamp> for SqlValue {
    fn from(moment: Timestamp) -> Self {
        Self::Int(moment.as_unix_millis())
    }
}

impl<T: Into<SqlValue>> From<Option<T>> for SqlValue {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Into::into)
    }
}

/// One row of an answer. A column is read by its position, as the type the
/// caller expects; a column of another type means the store does not hold
/// what the code expects, which is `busy`/`store_corrupt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    values: Vec<SqlValue>,
}

impl Row {
    pub(crate) fn new(values: Vec<SqlValue>) -> Self {
        Self { values }
    }

    /// The value at `column`, whatever it is.
    #[must_use]
    pub fn value(&self, column: usize) -> Option<&SqlValue> {
        self.values.get(column)
    }

    /// The text at `column`.
    ///
    /// # Errors
    ///
    /// `busy`/`store_corrupt` when the column is absent or not text.
    pub fn text(&self, column: usize) -> Result<&str, Failure> {
        match self.values.get(column) {
            Some(SqlValue::Text(text)) => Ok(text),
            _ => Err(wrong_column()),
        }
    }

    /// The text at `column`, or `None` where the column holds no value.
    ///
    /// # Errors
    ///
    /// As [`Row::text`].
    pub fn opt_text(&self, column: usize) -> Result<Option<&str>, Failure> {
        match self.values.get(column) {
            Some(SqlValue::Null) => Ok(None),
            Some(SqlValue::Text(text)) => Ok(Some(text)),
            _ => Err(wrong_column()),
        }
    }

    /// The whole number at `column`.
    ///
    /// # Errors
    ///
    /// `busy`/`store_corrupt` when the column is absent or not a number.
    pub fn int(&self, column: usize) -> Result<i64, Failure> {
        match self.values.get(column) {
            Some(SqlValue::Int(number)) => Ok(*number),
            _ => Err(wrong_column()),
        }
    }

    /// The whole number at `column`, or `None` where it holds no value.
    ///
    /// # Errors
    ///
    /// As [`Row::int`].
    pub fn opt_int(&self, column: usize) -> Result<Option<i64>, Failure> {
        match self.values.get(column) {
            Some(SqlValue::Null) => Ok(None),
            Some(SqlValue::Int(number)) => Ok(Some(*number)),
            _ => Err(wrong_column()),
        }
    }

    /// The bytes at `column`.
    ///
    /// # Errors
    ///
    /// `busy`/`store_corrupt` when the column is absent or not bytes.
    pub fn bytes(&self, column: usize) -> Result<&[u8], Failure> {
        match self.values.get(column) {
            Some(SqlValue::Bytes(bytes)) => Ok(bytes),
            _ => Err(wrong_column()),
        }
    }

    /// The bytes at `column`, or `None` where it holds no value.
    ///
    /// # Errors
    ///
    /// As [`Row::bytes`].
    pub fn opt_bytes(&self, column: usize) -> Result<Option<&[u8]>, Failure> {
        match self.values.get(column) {
            Some(SqlValue::Null) => Ok(None),
            Some(SqlValue::Bytes(bytes)) => Ok(Some(bytes)),
            _ => Err(wrong_column()),
        }
    }
}

fn wrong_column() -> Failure {
    failure::corrupt("a column that is absent or of another type than the code expects")
}

/// One database transaction. Everything done through it commits together or
/// not at all: when the closure it was given to returns `Ok`, it commits;
/// when the closure returns a failure, everything is taken back.
pub struct Tx<'a> {
    pub(crate) db: &'a Db,
    pub(crate) shared: &'a Shared,
    now: Timestamp,
    writes: bool,
}

impl<'a> Tx<'a> {
    pub(crate) fn new(db: &'a Db, shared: &'a Shared, writes: bool) -> Self {
        Self {
            db,
            shared,
            now: shared.clock.now(),
            writes,
        }
    }

    /// The moment of this transaction: the store's clock, read once when the
    /// transaction began. Every row it writes carries this one moment.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        self.now
    }

    /// Whether this transaction may write.
    #[must_use]
    pub fn writes(&self) -> bool {
        self.writes
    }

    /// Ten random bytes from the store's random source: what a new
    /// identifier is made from, with [`Tx::now`].
    ///
    /// # Errors
    ///
    /// What the random source returns when it has no bytes to give.
    pub fn random(&mut self) -> Result<[u8; 10], Failure> {
        let mut bytes = [0u8; 10];
        self.shared.random.fill(&mut bytes)?;
        Ok(bytes)
    }

    /// The tables of `owner`. A crate passes its own name here and nothing
    /// else; the statements it then runs are refused when they name a table
    /// of another crate.
    ///
    /// The policy engine's tables are not reached this way: approvals and
    /// the decisions that clear a run to write are what rule 3 trusts, so
    /// they are reached only with the [`PolicyKey`] the store hands out
    /// once, through [`Tx::policy_tables`]. `tables(Owner::Policy)` gives
    /// tables whose every statement is refused (decision 0145).
    pub fn tables(&mut self, owner: Owner) -> Tables<'_, 'a> {
        Tables {
            tx: self,
            owner,
            keyed: false,
        }
    }

    /// The policy engine's tables, for the holder of the store's
    /// [`PolicyKey`].
    ///
    /// # Errors
    ///
    /// `unknown_variant`/`table_not_owned` when the key was handed out by
    /// another store.
    pub fn policy_tables(&mut self, key: &PolicyKey) -> Result<Tables<'_, 'a>, Failure> {
        if key.store != self.shared.id {
            return Err(failure::not_owned(
                "the policy engine's key was handed out by another store",
            ));
        }
        Ok(Tables {
            tx: self,
            owner: Owner::Policy,
            keyed: true,
        })
    }

    /// A key to the policy engine's tables of this transaction's store, for
    /// the stand-in of [`crate::testing`].
    #[cfg(feature = "testing")]
    pub(crate) fn shared_policy_key_for_tests(&self) -> PolicyKey {
        PolicyKey::new(self.shared.id)
    }

    /// The policy engine's tables, for a test that stands in for the policy
    /// engine. Only with the `testing` feature, which no normal build turns
    /// on; [`crate::testing::approve`] writes a whole approval for one.
    #[cfg(feature = "testing")]
    pub fn policy_tables_for_tests(&mut self) -> Tables<'_, 'a> {
        Tables {
            tx: self,
            owner: Owner::Policy,
            keyed: true,
        }
    }

    /// One of the ledger's own statements that returns no rows.
    pub(crate) fn exec(&mut self, sql: &'static str, params: &[SqlValue]) -> Result<u64, Failure> {
        self.db.execute(Scope::Owner(Owner::Ledger), sql, params)
    }

    /// One of the ledger's own statements, and its rows.
    pub(crate) fn rows(
        &mut self,
        sql: &'static str,
        params: &[SqlValue],
    ) -> Result<Vec<Row>, Failure> {
        self.db.query(Scope::Owner(Owner::Ledger), sql, params)
    }

    /// The first row of one of the ledger's own statements.
    pub(crate) fn row(
        &mut self,
        sql: &'static str,
        params: &[SqlValue],
    ) -> Result<Option<Row>, Failure> {
        Ok(self.rows(sql, params)?.into_iter().next())
    }
}

/// One crate's tables, inside a transaction. The statements are fixed text
/// with numbered parameters (`?1`, `?2`); values are always bound, never
/// written into the text.
pub struct Tables<'t, 'a> {
    tx: &'t mut Tx<'a>,
    owner: Owner,
    keyed: bool,
}

/// The right to write the policy engine's tables: approvals, decisions,
/// declines, policy versions, kill switches, and quota counters.
///
/// A store hands out one, once, through [`Store::policy_key`]; the policy
/// engine takes it when it is opened, so only the policy engine records an
/// approval or a decision that rule 3 trusts. It cannot be cloned, made
/// another way, or used on another store. What remains: code in this
/// process that is handed the key, or opens the database file itself, can
/// still write those tables; the kernel hands the key to the policy engine
/// alone, and a test holds that no other crate's source names it.
///
/// [`Store::policy_key`]: crate::Store::policy_key
///
/// ```compile_fail
/// fn forge(key: &a2m_ledger::PolicyKey) -> a2m_ledger::PolicyKey {
///     key.clone()
/// }
/// ```
pub struct PolicyKey {
    store: u64,
}

impl PolicyKey {
    pub(crate) const fn new(store: u64) -> Self {
        Self { store }
    }
}

impl std::fmt::Debug for PolicyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PolicyKey")
    }
}

impl Tables<'_, '_> {
    /// Runs a statement that returns no rows, and gives the number of rows
    /// it changed.
    ///
    /// # Errors
    ///
    /// `unknown_variant`/`table_not_owned` when the statement names a table
    /// that is not this owner's, or tries what only the store may do, such
    /// as ending the transaction. `unknown_variant`/`ledger_state_unexpected`
    /// when a constraint of the table refuses the row. `busy` failures when
    /// the database cannot do it.
    pub fn execute(&mut self, sql: &'static str, params: &[SqlValue]) -> Result<u64, Failure> {
        self.tx.db.execute(self.scope()?, sql, params)
    }

    /// Runs a statement and gives its rows.
    ///
    /// # Errors
    ///
    /// As [`Tables::execute`].
    pub fn query(&mut self, sql: &'static str, params: &[SqlValue]) -> Result<Vec<Row>, Failure> {
        self.tx.db.query(self.scope()?, sql, params)
    }

    /// Runs a statement and gives its first row, if it has one.
    ///
    /// # Errors
    ///
    /// As [`Tables::execute`].
    pub fn query_first(
        &mut self,
        sql: &'static str,
        params: &[SqlValue],
    ) -> Result<Option<Row>, Failure> {
        Ok(self.query(sql, params)?.into_iter().next())
    }

    /// The moment of the transaction.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        self.tx.now()
    }

    fn scope(&self) -> Result<Scope, Failure> {
        if self.owner == Owner::Policy && !self.keyed {
            return Err(failure::not_owned(
                "the policy engine's tables are reached only with its key",
            ));
        }
        if self.owner == Owner::Ledger {
            // The ledger's tables carry the nine rules; they are reached
            // through the ledger's own interface and no other way.
            return Err(failure::not_owned(
                "the ledger's tables are reached through the ledger's interface",
            ));
        }
        Ok(Scope::Owner(self.owner))
    }
}
