//! The `sql` driver: one step at a time against a relational store.
//!
//! A read runs only on a connection the store itself holds read-only. A
//! write runs inside one transaction: the count of rows it changed is
//! checked before `commit`, and any other count rolls back. The driver
//! never retries, and it says for every failure whether anything was sent.
//!
//! The driver trusts no plan to be the compiler's. Before a statement is
//! first used it is held to the form of one plain read or one keyed write,
//! as the compiler held it; a writing transaction takes one statement and
//! one row; and the read of a record by its key must return that key.
//!
//! Every call into the store blocks, so each runs on a blocking thread; the
//! connection moves there and back. A step that is dropped while it waits
//! leaves its connection behind, which closes it, and the store rolls back.

#[cfg(feature = "postgres")]
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;
#[cfg(feature = "postgres")]
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use a2m_auth::SessionLease;
use a2m_core::{CodeEntry, Failure, RecordKey, Redact, Step, Value, Values};
use a2m_provider::{
    BoxFuture, Capture, CheckedPath, Cleanup, Deadline, Driver, DriverHealth, FileIdentity,
    Observed, Posture, PrepareContext, Prepared, RootName, Roots, StepFailure, StepInput,
    StepOutcome, SurfaceRef,
};
use a2m_telemetry::{Acting, Detail};

use crate::model::{
    Access, Column, Does, FOUND, Returns, Section, Statement, StepDetail, Stored, SurfaceFields,
};
#[cfg(feature = "postgres")]
use crate::names::DATABASE;
use crate::names::{BEGIN, CATALOG, COMMIT, EXECUTE, EXPECT_ROWS, FETCH, PACK, STATEMENT, codes};
use crate::shape::{self, Wrong};
use crate::store::{
    Bound, Cell, CommitFailed, Conn, Interrupter, Opener, Ran, Refusal, RunLimits, StoreAt, Unseen,
};

/// The root an embedded store's file is in when its surface names none.
const DEFAULT_ROOT: &str = "project";
/// The most tables and views one catalog read returns.
pub(crate) const CATALOG_TABLES: usize = 2000;

/// Finds the opener of a store by the name a surface states.
pub(crate) type Openers = Arc<dyn Fn(&str) -> Option<Arc<dyn Opener>> + Send + Sync>;

#[cfg(feature = "postgres")]
/// Which connections may serve the next run: those of one surface, opened
/// with one session's credentials at one epoch, to one address, port,
/// database, and schema, as one role, by one link mode. A connection is
/// never handed to a run of another session, nor of the same session after
/// its credentials were renewed, nor to a run whose surface now leads
/// elsewhere.
pub(crate) type PoolKey = (String, String, u64, crate::store::Leads);

/// The most idle connections kept for one key and one access.
#[cfg(feature = "postgres")]
const POOL_MOST: usize = 2;

/// Idle connections of server stores, kept between runs. Only a connection
/// with no open transaction, from a session in which nothing failed, is put
/// here; every other is closed at release.
#[cfg(feature = "postgres")]
#[derive(Default)]
pub(crate) struct Pool {
    idle: Mutex<BTreeMap<PoolKey, Vec<Idle>>>,
}

/// One idle connection, and the access the store holds it to.
#[cfg(feature = "postgres")]
type Idle = (Access, Box<dyn Conn>);

#[cfg(feature = "postgres")]
impl Pool {
    /// An idle connection of the key and the access, when one is usable and
    /// says it leads where the key does.
    fn take(&self, key: &PoolKey, access: Access) -> Option<Box<dyn Conn>> {
        let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
        // A connection of the same surface and session at an older epoch, or
        // to another place, is never used again: its credentials were
        // replaced, or the surface was published again to lead elsewhere.
        let stale: Vec<PoolKey> = idle
            .keys()
            .filter(|held| {
                held.0 == key.0 && held.1 == key.1 && (held.2 != key.2 || held.3 != key.3)
            })
            .cloned()
            .collect();
        for held in stale {
            if let Some(conns) = idle.remove(&held) {
                for (_, conn) in conns {
                    close(conn);
                }
            }
        }
        let conns = idle.get_mut(key)?;
        while let Some(position) = conns.iter().position(|(held, _)| *held == access) {
            let (_, conn) = conns.remove(position);
            // The connection's own record of where it leads, held to the
            // plan's surface again here: a connection that says anything
            // else is closed and never used.
            if conn.usable() && conn.leads_to() == Some(&key.3) {
                return Some(conn);
            }
            close(conn);
        }
        None
    }

    /// Keeps a connection for the next run, or closes it when enough are kept.
    fn give(&self, key: PoolKey, access: Access, conn: Box<dyn Conn>) {
        let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
        let conns = idle.entry(key).or_default();
        if conn.usable()
            && !conn.in_transaction()
            && conns.iter().filter(|(held, _)| *held == access).count() < POOL_MOST
        {
            conns.push((access, conn));
        } else {
            close(conn);
        }
    }
}

/// The driver of the mode `sql`.
pub(crate) struct SqlDriver {
    redact: Arc<dyn Redact>,
    result_bytes: u64,
    openers: Openers,
    /// The pack's own roots, as its manifest declares them.
    roots: Roots,
    /// Idle connections of server stores.
    #[cfg(feature = "postgres")]
    pool: Arc<Pool>,
}

impl SqlDriver {
    pub(crate) fn new(
        redact: Arc<dyn Redact>,
        result_bytes: u64,
        openers: Openers,
        roots: Roots,
    ) -> Self {
        Self {
            redact,
            result_bytes,
            openers,
            roots,
            #[cfg(feature = "postgres")]
            pool: Arc::new(Pool::default()),
        }
    }
}

/// What `prepare` acquired, and where the binding's transaction stands.
pub(crate) struct Session {
    /// The name of the surface the session was prepared for.
    surface: String,
    at: StoreAt,
    /// The embedded store's file by its checked path, with its identity when
    /// the run was prepared: every connection opened by its name is held to
    /// it.
    file: Option<(CheckedPath, FileIdentity)>,
    opener: Arc<dyn Opener>,
    /// Where the session's connections go at release, for a store whose
    /// connections are kept: the pool and the key.
    #[cfg(feature = "postgres")]
    pooled: Option<(Arc<Pool>, PoolKey)>,
    /// Whether a step of the session failed. Its connections are then
    /// closed at release, never kept.
    failed: bool,
    may_write: bool,
    section: Section,
    section_index: Option<usize>,
    reader: Option<Box<dyn Conn>>,
    writer: Option<Box<dyn Conn>>,
    /// The access of the open transaction, when one is open.
    open: Option<Access>,
    /// What the last `execute` gave, until `fetch` takes it.
    ran: Option<Ran>,
    /// Whether `expect_rows` passed for what the open transaction changed.
    counted: bool,
    /// Whether a statement that writes ran in the open transaction. A
    /// transaction takes one, so that the one count checked is the count of
    /// everything it commits.
    wrote: bool,
    /// The key the last read by key asked for: each key column with the
    /// value bound to it, until `fetch` holds the record to it.
    asked: Option<Vec<(String, Bound)>>,
    /// The statements whose shape was compared with the plan.
    described: BTreeSet<String>,
    /// The tables whose triggers and cascading keys were looked at.
    looked_at: BTreeSet<String>,
    /// A connection was lost or left in doubt. Nothing more is sent.
    lost: bool,
}

/// A failure of the code of `row`, in the class its table gives it.
fn failure(row: &CodeEntry, message: impl Into<String>, next: impl Into<String>) -> Failure {
    row.failure(message, next)
}

fn unexpected(message: &str) -> Failure {
    failure(
        &codes::row::UNEXPECTED_STATUS,
        message,
        "Report this to the pack's author; the run can be started again.",
    )
}

fn drift(message: &str) -> Failure {
    failure(
        &codes::row::SCHEMA_CHANGED,
        message,
        "Run discovery again, compare the store's catalog with the statements file, and publish a corrected version.",
    )
}

fn lost() -> Failure {
    failure(
        &a2m_provider::codes::row::CONNECTION_LOST,
        "The connection to the store ended before the step was sent.",
        "Start the run again; this step sent nothing.",
    )
}

/// A statement of the plan that is not one plain read or one keyed write.
fn not_allowed(message: &str) -> Failure {
    failure(
        &codes::row::STATEMENT_NOT_ALLOWED,
        message,
        "Correct the statement in the statements file: a read is one `SELECT`, and a write is written by the pack from its record.",
    )
}

/// A statement declared a read that would change data.
fn read_wrote() -> Failure {
    failure(
        &codes::row::READ_STATEMENT_WROTE,
        "A statement declared as a read tried to change data, and it was refused.",
        "Correct the statement or declare it `effect: write`; do not use the binding as a read.",
    )
}

/// The identity may do more than the store's own rights would bar.
#[cfg(feature = "postgres")]
fn too_powerful() -> Failure {
    failure(
        &codes::server::row::IDENTITY_IS_ADMINISTRATOR,
        "The identity the session signs in as is a superuser, bypasses row security, may replicate or make roles, or owns tables of the schema, so the store's own rights would not bar what a statement does. Nothing was sent.",
        "Sign in as a role that owns nothing and holds only the rights to read and write the rows the target names.",
    )
}

/// The connection, or the file, does not lead where the plan names.
#[cfg(feature = "postgres")]
fn leads_elsewhere() -> Failure {
    failure(
        &codes::server::row::CONNECTION_LEADS_ELSEWHERE,
        "The connection does not lead where the plan's surface names: the server reports another database, role, or `search_path`, or another address or port than when the connection was opened. It was closed, and nothing was sent.",
        "Check the surface's address, port, database, and schema, and what else may change a connection's settings; then start the run again.",
    )
}

/// The embedded store's file is no longer the file the run was prepared
/// with: another file was put at its name.
fn file_replaced() -> Failure {
    failure(
        &codes::row::STORE_FILE_REPLACED,
        "The store's file was replaced by another file at its name since the run was prepared, so nothing was sent to it.",
        "Check what replaced the file, then start the run again.",
    )
}

fn not_by_key(message: &str) -> Failure {
    failure(
        &codes::row::READ_NOT_BY_KEY,
        message,
        "Compare the record's key and columns in the statements file with the store's catalog, and publish a corrected version.",
    )
}

/// Why a statement did not run, or what the store said when it did.
enum Stopped {
    /// Found before the statement was sent.
    Unsent(Failure),
    /// The store's own answer.
    Store(Refusal),
}

/// Whether the store returned the value that was bound, exactly.
fn same_value(bound: &Bound, cell: &Cell) -> bool {
    match (bound, cell) {
        (Bound::Whole(asked), Cell::Whole(found)) => asked == found,
        (Bound::Text(asked), Cell::Text(found)) => asked == found,
        _ => false,
    }
}

/// What a store's refusal is as a failure. `access` is that of the
/// transaction the statement ran in.
fn refused(refusal: Refusal, access: Access) -> Failure {
    match refusal {
        Refusal::Locked => failure(
            &codes::row::STORE_LOCKED,
            "Another connection held the store's lock for the whole time limit.",
            "Try again when the other program has finished its work.",
        ),
        Refusal::TimedOut => failure(
            &a2m_provider::codes::row::TIMEOUT,
            "The statement ran past its time limit and was stopped.",
            "Try again, or raise the statement's `time_limit`.",
        ),
        // The sentence is the product's. What the store says of the
        // constraint, its name, goes apart from it, and `act` redacts it.
        Refusal::Constraint { kind, named } => {
            let refused = failure(
                &codes::row::CONSTRAINT_VIOLATED,
                format!("A constraint of the store refused the write (kind: {kind})."),
                "Correct the values so that they satisfy the store's constraints; nothing was written.",
            );
            match named {
                Some(said) => refused.with_said(said),
                None => refused,
            }
        }
        Refusal::ReadOnly if access == Access::ReadOnly => read_wrote(),
        Refusal::ReadOnly | Refusal::Denied => failure(
            &codes::row::DENIED_BY_STORE,
            "The store does not let this identity write.",
            "Ask the store's owner for the right to write, or use a route that has it.",
        ),
        Refusal::Shape => drift(
            "A table, a column, or a parameter of the statement differs from what the target declares.",
        ),
        Refusal::Unavailable => failure(
            &codes::row::UPSTREAM_UNAVAILABLE,
            "The store cannot be opened.",
            "Check that the store's file exists and can be read, then try again.",
        ),
        Refusal::TooLarge => failure(
            &codes::row::RESULT_TOO_LARGE,
            "The statement's result holds more text than the limit allows.",
            "Lower the statement's `max_rows`, or raise the pack's `result_bytes` setting.",
        ),
        Refusal::NotAllowed => failure(
            &codes::row::STATEMENT_NOT_ALLOWED,
            "The connection's own guard refused the statement: it is not one plain read or one keyed write.",
            "Correct the statement in the statements file: a read is one `SELECT`, and a write is written by the pack from its record.",
        ),
        Refusal::NotGuarded => unexpected(
            "The store did not take a setting that holds the connection to its access and to its one file, or its one database, schema, and identity.",
        ),
        #[cfg(feature = "postgres")]
        Refusal::Elsewhere => leads_elsewhere(),
        #[cfg(feature = "postgres")]
        Refusal::TooPowerful => too_powerful(),
        Refusal::Other(code) => failure(
            &codes::row::UNEXPECTED_STATUS,
            format!("The store answered with an error no rule covers (result code {code})."),
            "Look at the store's own log, then report the code to the pack's author.",
        ),
    }
}

/// A value as a bound parameter. A list, a record, and a moment have no
/// stored form the target declared, and are refused.
fn bound(value: &Value) -> Option<Bound> {
    match value {
        Value::Null => Some(Bound::Null),
        Value::Bool(flag) => Some(Bound::Whole(i64::from(*flag))),
        Value::Int(number) => Some(Bound::Whole(*number)),
        Value::Text(text) => Some(Bound::Text(text.clone())),
        Value::Decimal(number) => Some(Bound::Text(number.to_string())),
        Value::Date(day) => Some(Bound::Text(day.to_string())),
        Value::Duration(millis) => Some(Bound::Whole(millis.get())),
        Value::Instant(_) | Value::List(_) | Value::Record(_) => None,
    }
}

/// Interrupts the running statement when dropped before it is disarmed:
/// when the step's future is dropped while the store still works.
struct InterruptOnDrop(Option<Interrupter>);

impl Drop for InterruptOnDrop {
    fn drop(&mut self) {
        if let Some(interrupt) = self.0.take() {
            interrupt();
        }
    }
}

/// Runs `work` with the connection on a blocking thread. At the deadline the
/// statement is interrupted, when `interruptible`. `None` when the thread
/// could not give the connection back.
async fn on_store<T: Send + 'static>(
    conn: Box<dyn Conn>,
    deadline: Deadline,
    interruptible: bool,
    work: impl FnOnce(&mut dyn Conn) -> T + Send + 'static,
) -> Option<(Box<dyn Conn>, T)> {
    let interrupt = conn.interrupter();
    let mut guard = InterruptOnDrop(interruptible.then(|| Arc::clone(&interrupt)));
    let mut task = tokio::task::spawn_blocking(move || {
        let mut conn = conn;
        let out = work(conn.as_mut());
        (conn, out)
    });
    let joined = if interruptible {
        tokio::select! {
            joined = &mut task => joined,
            () = tokio::time::sleep_until(deadline.at()) => {
                interrupt();
                task.await
            }
        }
    } else {
        task.await
    };
    guard.0 = None;
    joined.ok()
}

/// Closes a connection that will not be used again, on a blocking thread:
/// closing a file is input and output too. The store rolls back whatever
/// the connection left open.
fn close(conn: Box<dyn Conn>) {
    drop(tokio::task::spawn_blocking(move || drop(conn)));
}

/// The moment a step must be done by: its own time limit or the caller's
/// deadline, whichever is first.
/// How a step was done, as the capture the trace takes: `means` is what the
/// trace's signature covers, `measured` what it does not.
fn acting(step: &Step, means: Vec<Detail>, measured: Vec<Detail>) -> Capture {
    let mut all = vec![Detail::new("step", step.kind.as_str())];
    all.extend(means);
    Capture::acting(&Acting {
        means: all,
        measured,
    })
}

fn until(step: &Step, input: &StepInput<'_>) -> Deadline {
    let own = Deadline::after(Duration::from_millis(u64::from(step.timeout_ms)));
    own.min(input.deadline)
}

impl Session {
    fn statement(&self, step: &Step) -> Result<(String, Statement), StepFailure> {
        let found = step.refers_to.as_ref().and_then(|reference| {
            (Some(reference.section.get()) == self.section_index)
                .then(|| self.section.statements.get(&reference.entry))
                .flatten()
                // A statement runs only against the store it was declared
                // for: the one this session opened.
                .filter(|statement| statement.surface == self.surface)
                .map(|statement| (reference.entry.clone(), statement.clone()))
        });
        found.ok_or_else(|| {
            StepFailure::not_sent(unexpected(
                "The step refers to a statement the plan's section does not hold for this surface.",
            ))
        })
    }

    /// Rolls back whatever is open, on the connection that holds it. A
    /// connection that does not come back is closed by being dropped.
    async fn roll_back(&mut self) {
        self.ran = None;
        self.counted = false;
        self.wrote = false;
        self.asked = None;
        let Some(access) = self.open.take() else {
            return;
        };
        let slot = match access {
            Access::ReadOnly => &mut self.reader,
            Access::ReadWrite => &mut self.writer,
        };
        if let Some(conn) = slot.take() {
            let back = on_store(
                conn,
                Deadline::after(Duration::from_secs(5)),
                false,
                |conn| {
                    conn.rollback();
                    conn.in_transaction()
                },
            )
            .await;
            match back {
                Some((conn, false)) => *slot = Some(conn),
                // Still open: in doubt, so closed and never used again.
                Some((conn, true)) => {
                    close(conn);
                    self.lost = true;
                }
                None => self.lost = true,
            }
        }
    }

    /// Fails the step after rolling back: the store answered, and nothing
    /// of the transaction stays.
    async fn answered(&mut self, failure: Failure) -> StepFailure {
        self.roll_back().await;
        StepFailure::answered(failure)
    }

    /// Fails the step before anything of it was sent, after rolling back.
    async fn not_sent(&mut self, failure: Failure) -> StepFailure {
        self.roll_back().await;
        StepFailure::not_sent(failure)
    }

    /// Takes the connection of the open transaction.
    fn take_open(&mut self) -> Result<(Access, Box<dyn Conn>), StepFailure> {
        let Some(access) = self.open else {
            return Err(StepFailure::not_sent(unexpected(
                "The step needs an open transaction, and none is open.",
            )));
        };
        let slot = match access {
            Access::ReadOnly => &mut self.reader,
            Access::ReadWrite => &mut self.writer,
        };
        match slot.take() {
            Some(conn) => Ok((access, conn)),
            None => {
                self.lost = true;
                self.open = None;
                Err(StepFailure::not_sent(lost()))
            }
        }
    }

    fn give_back(&mut self, access: Access, conn: Box<dyn Conn>) {
        match access {
            Access::ReadOnly => self.reader = Some(conn),
            Access::ReadWrite => self.writer = Some(conn),
        }
    }

    /// Whether the embedded store's file is still the file the run was
    /// prepared with. Always true of a server store.
    fn same_file(&self) -> bool {
        self.file
            .as_ref()
            .is_none_or(|(path, identity)| path.identity().ok().as_ref() == Some(identity))
    }

    async fn begin(&mut self, step: &Step, input: &StepInput<'_>) -> Result<Observed, StepFailure> {
        let detail: StepDetail = step.payload.decode().map_err(StepFailure::not_sent)?;
        let (Some(access), Some(isolation)) = (detail.access, detail.isolation) else {
            return Err(StepFailure::not_sent(unexpected(
                "A `begin` step states no access mode.",
            )));
        };
        if self.open.is_some() {
            return Err(self
                .not_sent(unexpected(
                    "A transaction was still open when another was to begin.",
                ))
                .await);
        }
        if access == Access::ReadWrite && !self.may_write {
            return Err(StepFailure::not_sent(failure(
                &a2m_provider::codes::row::READ_ONLY_POSTURE,
                "A transaction that may write was asked for where nothing may be written.",
                "Run the operation live, or against a declared test record.",
            )));
        }
        let deadline = until(step, input);
        let conn = match access {
            Access::ReadOnly => self.reader.take(),
            Access::ReadWrite => match self.writer.take() {
                Some(conn) => Some(conn),
                // The writing connection is opened when the first write
                // begins: a run that only reads never holds one.
                #[cfg(feature = "postgres")]
                None if let Some(conn) = self
                    .pooled
                    .as_ref()
                    .and_then(|(pool, key)| pool.take(key, Access::ReadWrite)) =>
                {
                    Some(conn)
                }
                None => {
                    if !self.same_file() {
                        self.lost = true;
                        return Err(StepFailure::not_sent(file_replaced()));
                    }
                    let opener = Arc::clone(&self.opener);
                    let at = self.at.clone();
                    let opening =
                        tokio::task::spawn_blocking(move || opener.open(&at, Access::ReadWrite));
                    // Held to the step's time, as the reader's opening is
                    // held to `prepare`'s. A connection that opens later is
                    // closed where it was opened.
                    match tokio::time::timeout_at(deadline.at(), opening).await {
                        Ok(Ok(Ok(conn))) => Some(conn),
                        Ok(Ok(Err(refusal))) => {
                            return Err(StepFailure::not_sent(refused(refusal, access)));
                        }
                        Ok(Err(_)) => None,
                        Err(_) => {
                            return Err(StepFailure::not_sent(refused(Refusal::TimedOut, access)));
                        }
                    }
                }
            },
        };
        let Some(conn) = conn else {
            self.lost = true;
            return Err(StepFailure::not_sent(lost()));
        };
        let wait = deadline.remaining();
        let Some((conn, began)) = on_store(conn, deadline, true, move |conn| {
            if conn.access() != access {
                return Err(Refusal::NotGuarded);
            }
            conn.begin(isolation, wait)
        })
        .await
        else {
            self.lost = true;
            return Err(StepFailure::not_sent(lost()));
        };
        // A connection the server says leads elsewhere is never used again.
        #[cfg(feature = "postgres")]
        if matches!(began, Err(Refusal::Elsewhere)) {
            close(conn);
            self.lost = true;
            return Err(StepFailure::not_sent(leads_elsewhere()));
        }
        // A writer opened by the file's name holds the file the run was
        // prepared with, or nothing is sent to it.
        if access == Access::ReadWrite && !self.same_file() {
            close(conn);
            self.lost = true;
            return Err(StepFailure::not_sent(file_replaced()));
        }
        self.give_back(access, conn);
        match began {
            Ok(()) => {
                self.open = Some(access);
                Ok(Observed {
                    captures: vec![acting(
                        step,
                        vec![Detail::new(
                            "access",
                            match access {
                                Access::ReadOnly => "read_only",
                                Access::ReadWrite => "read_write",
                            },
                        )],
                        Vec::new(),
                    )],
                    ..Observed::default()
                })
            }
            // No transaction was opened, so nothing was sent that could
            // change data.
            Err(refusal) => Err(StepFailure::not_sent(refused(refusal, access))),
        }
    }

    /// What the driver itself refuses of a statement, before the store is
    /// asked anything: the refusals the compiler made, made again where the
    /// harm would be done.
    fn held(statement: &Statement, dialect: shape::Dialect) -> Result<(), Failure> {
        let reads = statement.does == Does::Read;
        if reads != (statement.access == Access::ReadOnly) {
            return Err(not_allowed(
                "The statement's access is not the one its effect states.",
            ));
        }
        shape::check_in(&statement.text, statement.does, dialect).map_err(|wrong| match wrong {
            Wrong::Writes => read_wrote(),
            Wrong::Form(message) => not_allowed(message),
        })
    }

    /// The key a read by key asks for: each key column of the record, with
    /// the value bound to the parameter of that field.
    fn key_asked(statement: &Statement, params: &[Bound]) -> Option<Vec<(String, Bound)>> {
        let mut key: Vec<&Column> = statement
            .columns
            .iter()
            .filter(|column| column.key_position > 0)
            .collect();
        key.sort_by_key(|column| column.key_position);
        if key.is_empty() || key.len() != statement.params.len() {
            return None;
        }
        key.into_iter()
            .map(|column| {
                let position = statement
                    .params
                    .iter()
                    .position(|param| param.key && param.name == column.field)?;
                Some((column.column.clone(), params.get(position)?.clone()))
            })
            .collect()
    }

    #[allow(clippy::too_many_lines)] // one check after another, each a few lines; splitting hides the order
    async fn execute(
        &mut self,
        step: &Step,
        input: &StepInput<'_>,
        recorded: &Values,
        result_bytes: u64,
    ) -> Result<Observed, StepFailure> {
        let (name, statement) = self.statement(step)?;
        if self.open != Some(statement.access) {
            return Err(self
                .not_sent(unexpected(
                    "The statement's access is not that of the open transaction.",
                ))
                .await);
        }
        let needs_shape = !self.described.contains(&name);
        if needs_shape && let Err(found) = Self::held(&statement, self.opener.dialect()) {
            return Err(self.not_sent(found).await);
        }
        if statement.access == Access::ReadWrite && self.wrote {
            // The one count that `expect_rows` checks must be the count of
            // everything the transaction commits.
            return Err(self
                .not_sent(unexpected(
                    "A second statement was to write in a transaction that takes one.",
                ))
                .await);
        }
        let mut params = Vec::with_capacity(statement.params.len());
        for param in &statement.params {
            // A key the store assigned is not an input: the read-back of such
            // a write takes it from what the write recorded before its
            // commit point, which is the same on a resume as in the pass.
            let given = input.args.get(&param.name).or_else(|| {
                recorded
                    .get(&param.name)
                    .filter(|_| param.key && statement.access == Access::ReadOnly)
            });
            let Some(value) = given.and_then(bound) else {
                return Err(self
                    .not_sent(failure(
                        &codes::row::VALUE_NOT_BINDABLE,
                        format!(
                            "The parameter `{}` of the statement `{name}` was given no value, or one that is a list, a record, or a moment.",
                            param.name
                        ),
                        "Pass a text, a whole number, a decimal, a date, or a truth value for the parameter.",
                    ))
                    .await);
            };
            params.push(value);
        }
        let asked = if statement.by_key {
            let Some(asked) = Self::key_asked(&statement, &params) else {
                return Err(self
                    .not_sent(not_by_key(
                        "A read by key does not take exactly the fields of its record's key.",
                    ))
                    .await);
            };
            Some(asked)
        } else {
            None
        };
        let table = statement
            .table
            .clone()
            .filter(|table| !statement.hazard_declared && !self.looked_at.contains(table));
        let limits = RunLimits {
            rows: match statement.returns {
                Returns::Many => statement.max_rows,
                Returns::OneOrNone | Returns::AssignedKey => 1,
                Returns::Nothing => 0,
            },
            bytes: result_bytes,
            wait: until(step, input).remaining(),
        };
        let (access, conn) = self.take_open()?;
        let text = statement.text.clone();
        let expected_params = statement.params.len();
        let known: Vec<String> = statement
            .columns
            .iter()
            .map(|column| column.column.clone())
            .collect();
        let key_columns: Vec<String> = asked
            .iter()
            .flatten()
            .map(|(column, _)| column.clone())
            .collect();
        let table_to_check = table.clone();
        let work = move |conn: &mut dyn Conn| -> Result<Ran, Stopped> {
            if needs_shape {
                // The store describes the statement without running it; the
                // plan is held to that description.
                let shape = conn.describe(&text).map_err(Stopped::Store)?;
                if shape.params != expected_params {
                    return Err(Stopped::Unsent(drift(
                        "The statement takes another number of parameters than the target declares.",
                    )));
                }
                if shape.columns.iter().any(|column| !known.contains(column)) {
                    return Err(Stopped::Unsent(drift(
                        "The statement returns a column that the target's record does not declare.",
                    )));
                }
                if access == Access::ReadOnly {
                    // The store's own answer, before anything runs. A store
                    // that cannot say refuses a write on the read-only
                    // transaction instead.
                    if shape.reads_only == Some(false) {
                        return Err(Stopped::Unsent(read_wrote()));
                    }
                    // A statement that returns nothing is not a read, and
                    // what else it is, is not known.
                    if shape.columns.is_empty() {
                        return Err(Stopped::Unsent(not_allowed(
                            "A statement declared as a read returns no column, so it is not a read.",
                        )));
                    }
                }
                if key_columns
                    .iter()
                    .any(|column| !shape.columns.contains(column))
                {
                    return Err(Stopped::Unsent(not_by_key(
                        "A read by key does not return the columns of its record's key, so the record it finds cannot be held to the key asked for.",
                    )));
                }
            }
            if let Some(table) = &table_to_check {
                let reach = conn
                    .reach(table, CATALOG_TABLES)
                    .map_err(|refusal| match refusal {
                        // Found before the statement was sent.
                        #[cfg(feature = "postgres")]
                        Refusal::TooPowerful => Stopped::Unsent(too_powerful()),
                        other => Stopped::Store(other),
                    })?;
                match reach {
                    Ok((0, 0)) => {}
                    Ok(_) => {
                        return Err(Stopped::Unsent(drift(
                            "The table has a trigger or a cascading key, and the statement declares no hazard: the write would reach data beyond the rows it names.",
                        )));
                    }
                    // Unknown means stop: a catalog that was cut off does
                    // not show that the write reaches nothing else.
                    Err(Unseen::Truncated) => {
                        return Err(Stopped::Unsent(failure(
                            &codes::row::CATALOG_TOO_LARGE,
                            format!(
                                "The store holds more than {CATALOG_TABLES} tables and views, so its catalog cannot show that the write reaches no data beyond the rows it names."
                            ),
                            "Declare `hazard: data_elsewhere` on the statement and on its operation, so that a person agrees to each write; or keep the tables in a smaller store.",
                        )));
                    }
                    Err(Unseen::NoTable) => {
                        return Err(Stopped::Unsent(drift(
                            "The table the statement writes is not in the store's catalog, so what the write reaches cannot be shown.",
                        )));
                    }
                }
            }
            conn.run(&text, &params, &limits).map_err(Stopped::Store)
        };
        let Some((conn, ran)) = on_store(conn, until(step, input), true, work).await else {
            self.lost = true;
            self.open = None;
            return Err(StepFailure::not_sent(lost()));
        };
        self.give_back(access, conn);
        match ran {
            Ok(ran) => {
                self.described.insert(name.clone());
                if let Some(table) = table {
                    self.looked_at.insert(table);
                }
                let how = acting(
                    step,
                    vec![Detail::new("statement", name)],
                    vec![
                        Detail::new("rows", ran.rows.len().to_string()),
                        Detail::new("bytes", ran.bytes.to_string()),
                        Detail::new("changed", ran.changed.to_string()),
                    ],
                );
                self.ran = Some(ran);
                self.counted = false;
                self.wrote = access == Access::ReadWrite;
                self.asked = asked;
                Ok(Observed {
                    captures: vec![how],
                    ..Observed::default()
                })
            }
            // Found before the statement ran.
            Err(Stopped::Unsent(found)) => Err(self.not_sent(found).await),
            Err(Stopped::Store(refusal)) => Err(self.answered(refused(refusal, access)).await),
        }
    }

    async fn expect_rows(&mut self, step: &Step) -> Result<Observed, StepFailure> {
        let detail: StepDetail = step.payload.decode().map_err(StepFailure::not_sent)?;
        let (Some(expected), Some(changed)) =
            (detail.rows, self.ran.as_ref().map(|ran| ran.changed))
        else {
            return Err(self
                .not_sent(unexpected(
                    "A count of rows was to be checked, and no statement had run.",
                ))
                .await);
        };
        // A keyed write changes one row. The plan states the count, and the
        // driver takes no other: a plan that asked for more would commit
        // more rows than the one record the ledger holds an intent for.
        if expected != 1 || self.open != Some(Access::ReadWrite) || !self.wrote {
            return Err(self
                .not_sent(unexpected(
                    "A count of rows other than one was to be checked, or outside a transaction that wrote.",
                ))
                .await);
        }
        if changed == expected {
            self.counted = true;
            return Ok(Observed {
                captures: vec![acting(
                    step,
                    vec![Detail::new("rows", expected.to_string())],
                    Vec::new(),
                )],
                ..Observed::default()
            });
        }
        // Any other count rolls back, so nothing is committed.
        let found = if changed < expected {
            step.fails_as.failure(
                "The write changed no row: the record, or its version, is not what the preview saw.",
                "Read the record again, look at what changed, and decide whether to write again.",
            )
        } else {
            failure(
                &codes::row::KEY_NOT_UNIQUE,
                "The write matched more than one row, so its key does not identify one record.",
                "Correct the record's key in the statements file; nothing was written.",
            )
        };
        Err(self.answered(found).await)
    }

    fn value(column: &Column, cell: Cell) -> Result<Value, Failure> {
        match (cell, column.stored) {
            (Cell::Null, _) => Ok(Value::Null),
            (Cell::Text(text), Stored::Text | Stored::Either) => Ok(Value::Text(text)),
            (Cell::Whole(number), Stored::Whole | Stored::Either) => Ok(Value::Int(number)),
            (Cell::Float | Cell::Bytes, _) => Err(failure(
                &codes::row::VALUE_KIND_UNSUPPORTED,
                format!(
                    "The field `{}` holds a float or bytes, and no rule reads either exactly.",
                    column.field
                ),
                "Read the column through a statement that returns it as exact text, or leave the field out.",
            )),
            #[cfg(feature = "postgres")]
            (Cell::Unsupported, _) => Err(failure(
                &codes::row::VALUE_KIND_UNSUPPORTED,
                format!(
                    "The field `{}` holds a value of a type the data pack has no rule for, such as a moment with no zone.",
                    column.field
                ),
                "Store the value as a type the pack reads exactly (text, a whole number, `numeric`, a date, or a moment with its zone), or leave the field out.",
            )),
            (Cell::Text(_) | Cell::Whole(_), _) => Err(drift(&format!(
                "The field `{}` is stored another way than its field type declares.",
                column.field
            ))),
        }
    }

    fn record(
        statement: &Statement,
        columns: &[String],
        row: Vec<Cell>,
    ) -> Result<Values, Failure> {
        let mut values = Values::new();
        for (name, cell) in columns.iter().zip(row) {
            let Some(column) = statement
                .columns
                .iter()
                .find(|column| &column.column == name)
            else {
                return Err(drift(
                    "The statement returned a column that the target's record does not declare.",
                ));
            };
            values.insert(column.field.clone(), Self::value(column, cell)?);
        }
        Ok(values)
    }

    /// Each field a many-row statement returned, as one list of its values
    /// in row order, a missing value as `null`: what an operation's list
    /// outputs are read from, since an output is one value or a list of
    /// values and never a record.
    fn columns_as_lists(
        statement: &Statement,
        columns: &[String],
        records: &[Value],
    ) -> Vec<(String, Vec<Value>)> {
        let fields: Vec<&str> = columns
            .iter()
            .filter_map(|name| {
                statement
                    .columns
                    .iter()
                    .find(|column| &column.column == name)
                    .map(|column| column.field.as_str())
            })
            .collect();
        fields
            .into_iter()
            .map(|field| {
                let list = records
                    .iter()
                    .map(|record| {
                        record
                            .as_record()
                            .and_then(|values| values.get(field))
                            .cloned()
                            .unwrap_or(Value::Null)
                    })
                    .collect();
                (field.to_owned(), list)
            })
            .collect()
    }

    /// Holds the record a read by key returned to the key it was asked for.
    /// A statement that finds some other record, by whatever predicate, is
    /// stopped here: its answer is never taken for the record that was
    /// written.
    fn is_asked(
        statement: &Statement,
        asked: Option<&[(String, Bound)]>,
        columns: &[String],
        row: &[Cell],
    ) -> Result<(), Failure> {
        if !statement.by_key {
            return Ok(());
        }
        let same = asked.is_some_and(|asked| {
            asked.iter().all(|(column, bound)| {
                columns
                    .iter()
                    .position(|name| name == column)
                    .and_then(|position| row.get(position))
                    .is_some_and(|cell| same_value(bound, cell))
            })
        });
        if same {
            Ok(())
        } else {
            Err(not_by_key(
                "A read by key returned a record whose key is not the one asked for.",
            ))
        }
    }

    async fn fetch(
        &mut self,
        step: &Step,
        redact: &dyn Redact,
    ) -> Result<StepOutcome, StepFailure> {
        let (_, statement) = self.statement(step)?;
        let detail: StepDetail = step.payload.decode().map_err(StepFailure::not_sent)?;
        let Some(ran) = self.ran.take() else {
            return Err(self
                .not_sent(unexpected(
                    "Rows were to be fetched, and no statement had run.",
                ))
                .await);
        };
        let asked = self.asked.take();
        let rows = ran.rows.len();
        let outcome = match statement.returns {
            Returns::OneOrNone if ran.more => Err(failure(
                &codes::row::KEY_NOT_UNIQUE,
                "A read by key matched more than one row.",
                "Correct the record's key in the statements file.",
            )),
            Returns::OneOrNone => match ran.rows.into_iter().next() {
                // A read that answers a missing row says so (decision 0348).
                None if detail.answers_missing => Ok(StepOutcome::Done(Observed {
                    values: Values::new().with(FOUND, false),
                    ..Observed::default()
                })),
                None => Ok(StepOutcome::Absent(Observed::default())),
                Some(row) => {
                    // A value stored another way than declared is named as
                    // that first; then the record is held to its key.
                    let held = Self::is_asked(&statement, asked.as_deref(), &ran.columns, &row);
                    Self::record(&statement, &ran.columns, row)
                        .and_then(|values| held.map(|()| values))
                        .map(|mut values| {
                            if detail.answers_missing {
                                values.insert(FOUND, Value::Bool(true));
                            }
                            StepOutcome::Done(Observed {
                                values: redact.redact_values(&values),
                                ..Observed::default()
                            })
                        })
                }
            },
            Returns::Many => ran
                .rows
                .into_iter()
                .map(|row| Self::record(&statement, &ran.columns, row).map(Value::Record))
                .collect::<Result<Vec<Value>, Failure>>()
                .map(|records| {
                    let lists = Self::columns_as_lists(&statement, &ran.columns, &records);
                    let mut values = Values::new()
                        .with("rows", Value::List(records))
                        .with("more", ran.more);
                    // A field named `rows` or `more` is the field's list:
                    // what a statement's record declares comes first.
                    for (field, list) in lists {
                        values.insert(field, Value::List(list));
                    }
                    StepOutcome::Done(Observed {
                        values: redact.redact_values(&values),
                        ..Observed::default()
                    })
                }),
            Returns::AssignedKey => match ran.rows.into_iter().next() {
                Some(row) if !ran.more => {
                    Self::record(&statement, &ran.columns, row).and_then(|values| {
                        let mut parts: Vec<&Column> = statement.columns.iter().collect();
                        parts.sort_by_key(|column| column.key_position);
                        let key = parts
                            .iter()
                            .filter_map(|column| values.get(&column.field).cloned())
                            .collect::<Vec<Value>>();
                        let key = RecordKey::new(key).map_err(|_| {
                            drift("The store assigned a key that is not a plain value.")
                        })?;
                        // The key is known before the commit point, so that a
                        // lost answer to `commit` can be reconciled by it.
                        let values = redact.redact_values(&values);
                        Ok(StepOutcome::Done(Observed {
                            values: values.clone(),
                            assigned_key: Some(key),
                            for_reconcile: values,
                            captures: Vec::new(),
                        }))
                    })
                }
                _ => Err(drift(
                    "The insert did not return the one key the store assigned.",
                )),
            },
            Returns::Nothing => Err(unexpected(
                "A statement that returns no rows has nothing to fetch.",
            )),
        };
        // A read is over once its rows are taken: the read-only transaction
        // ends here. A writing transaction stays open for `commit`.
        if self.open == Some(Access::ReadOnly) {
            self.roll_back().await;
        }
        match outcome {
            Ok(mut outcome) => {
                let how = acting(
                    step,
                    vec![Detail::new(
                        "found",
                        match &outcome {
                            StepOutcome::Absent(_) => "absent",
                            StepOutcome::Done(_) => "rows",
                        },
                    )],
                    vec![Detail::new("rows", rows.to_string())],
                );
                match &mut outcome {
                    StepOutcome::Absent(observed) | StepOutcome::Done(observed) => {
                        observed.captures.push(how);
                    }
                }
                Ok(outcome)
            }
            Err(found) => Err(self.answered(found).await),
        }
    }

    async fn commit(
        &mut self,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<Observed, StepFailure> {
        if !step.commits {
            // The driver's own guard, in every posture: a writing
            // transaction changes nothing until it is committed, and only
            // the step marked as the commit point commits. In a sandbox
            // with test records this is the one step the guard is lifted
            // for; whatever else ran in the transaction is rolled back.
            return Err(self
                .not_sent(failure(
                    &a2m_provider::codes::row::READ_ONLY_POSTURE,
                    "A commit was asked for by a step that is not marked as the commit point, so nothing was written.",
                    "Send the commit as the binding's commit point, after its intent is stored.",
                ))
                .await);
        }
        if self.open != Some(Access::ReadWrite) || !self.counted {
            // Nothing is committed whose count of rows was not checked.
            return Err(self
                .not_sent(unexpected(
                    "A commit was asked for with no writing transaction whose count of rows was checked.",
                ))
                .await);
        }
        let (access, conn) = self.take_open()?;
        let deadline = until(step, input);
        let wait = deadline.remaining();
        // From here the commit is sent. It is never interrupted: a commit
        // cut short is exactly the doubt this step must not create.
        let done = on_store(conn, deadline, false, move |conn| conn.commit(wait)).await;
        match done {
            Some((conn, Ok(()))) => {
                self.give_back(access, conn);
                self.open = None;
                self.ran = None;
                self.counted = false;
                self.wrote = false;
                Ok(Observed {
                    captures: vec![acting(step, Vec::new(), Vec::new())],
                    ..Observed::default()
                })
            }
            // A link that had ended before `COMMIT` was sent: nothing was
            // sent, and the store answered nothing.
            Some((conn, Err(CommitFailed::NotCommitted(Refusal::Unavailable)))) => {
                self.give_back(access, conn);
                Err(self.not_sent(refused(Refusal::Unavailable, access)).await)
            }
            Some((conn, Err(CommitFailed::NotCommitted(refusal)))) => {
                self.give_back(access, conn);
                Err(self.answered(refused(refusal, access)).await)
            }
            // The connection is in doubt: it is closed, never used again.
            in_doubt @ (Some((_, Err(CommitFailed::Unknown))) | None) => {
                if let Some((conn, _)) = in_doubt {
                    close(conn);
                }
                self.lost = true;
                self.open = None;
                self.ran = None;
                Err(StepFailure::unknown(failure(
                    &a2m_provider::codes::row::NO_CONFIRMATION,
                    "The commit was sent and the store's answer did not arrive, so the write may have landed.",
                    "Reconcile the record: read it back by its key before anything is written again.",
                )))
            }
        }
    }

    async fn catalog(
        &mut self,
        step: &Step,
        input: &StepInput<'_>,
        redact: &dyn Redact,
    ) -> Result<Observed, StepFailure> {
        if self.open.is_some() {
            return Err(self
                .not_sent(unexpected(
                    "The catalog is read outside any other transaction.",
                ))
                .await);
        }
        let Some(conn) = self.reader.take() else {
            self.lost = true;
            return Err(StepFailure::not_sent(lost()));
        };
        let deadline = until(step, input);
        let wait = deadline.remaining();
        let work = move |conn: &mut dyn Conn| {
            // Inside a read-only transaction, so that the catalog is one
            // consistent picture; rolled back, because nothing was changed.
            conn.begin(crate::model::Isolation::Serializable, wait)?;
            let catalog = conn.catalog(CATALOG_TABLES);
            conn.rollback();
            catalog
        };
        let Some((conn, catalog)) = on_store(conn, deadline, true, work).await else {
            self.lost = true;
            return Err(StepFailure::not_sent(lost()));
        };
        self.give_back(Access::ReadOnly, conn);
        let catalog =
            catalog.map_err(|refusal| StepFailure::answered(refused(refusal, Access::ReadOnly)))?;
        Ok(Observed {
            values: redact.redact_values(&crate::discover::catalog_values(&catalog)),
            captures: vec![acting(
                step,
                Vec::new(),
                vec![Detail::new("tables", catalog.tables.len().to_string())],
            )],
            ..Observed::default()
        })
    }
}

impl SqlDriver {
    /// Where a store in a file is: its checked path.
    fn file_of(
        &self,
        cx: &PrepareContext<'_>,
        surface: &a2m_core::SurfacePlan,
        fields: &SurfaceFields,
    ) -> Result<CheckedPath, StepFailure> {
        // The store's file arrives as a checked path under the surface's
        // name, when the run was handed one. The driver takes only the file
        // the target declares.
        let root = fields.root.as_deref().unwrap_or(DEFAULT_ROOT);
        let given = cx
            .paths
            .get(surface.name.as_str())
            .filter(|path| path.root().as_str() == root && path.relative() == surface.address)
            .cloned();
        if let Some(path) = given {
            return Ok(path);
        }
        // A file named in the root `project` is a file of the project of the
        // request that started the run: it is resolved against that folder,
        // never against the folder the engine was started in. A run with no
        // project folder stops by name, before anything is opened.
        if root == DEFAULT_ROOT {
            let path = cx
                .paths
                .project_folder()
                .and_then(|folder| folder.join(&surface.address))
                .map_err(StepFailure::not_sent)?;
            return Ok(path);
        }
        // Any other root is one the pack's manifest declares: the surface's
        // root, among them, and the surface's address below it, through the
        // one check that makes a path.
        let path = RootName::new(root).ok().and_then(|name| {
            self.roots
                .check(&name, &surface.address, a2m_provider::Access::Write)
                .or_else(|_| {
                    self.roots
                        .check(&name, &surface.address, a2m_provider::Access::Read)
                })
                .ok()
        });
        let Some(path) = path else {
            return Err(StepFailure::not_sent(failure(
                &a2m_provider::codes::row::PATH_NOT_ALLOWED,
                format!(
                    "The run was given no checked path for the file of the surface `{}` in the root `{root}`.",
                    surface.name
                ),
                "Keep the store's file inside the named root, and name it in `target.yaml` by its path below that root.",
            )));
        };
        Ok(path)
    }
}

impl Driver for SqlDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move {
            let surface = &cx.surface.surface;
            if !crate::names::is_database(&surface.kind) {
                return Err(StepFailure::not_sent(unexpected(
                    "The `sql` driver was given a surface that is not a `database`.",
                )));
            }
            let fields: SurfaceFields = surface.payload.decode().map_err(StepFailure::not_sent)?;
            let Some(opener) = (self.openers)(&fields.store) else {
                return Err(StepFailure::not_sent(unexpected(
                    "The surface names a store this build does not speak to.",
                )));
            };
            #[cfg(feature = "postgres")]
            let (at, file, pooled): (StoreAt, _, Option<(Arc<Pool>, PoolKey)>) = if surface.kind
                == DATABASE
            {
                let path = self.file_of(cx, surface, &fields)?;
                (
                    StoreAt::File(path.as_path().to_path_buf()),
                    Some(path),
                    None,
                )
            } else {
                // A server store is one of the run's destinations, as
                // every other surface a run reaches is.
                if !cx.destinations.contains(&surface.address) {
                    return Err(StepFailure::not_sent(failure(
                        &codes::server::row::SURFACE_NOT_DECLARED,
                        format!(
                            "The surface `{}` is not among the destinations the operation may reach.",
                            surface.name
                        ),
                        "Name the surface in the target, and run an operation of that target.",
                    )));
                }
                let (at, key) = crate::postgres::server_of(surface, &fields, lease)?;
                let pooled = opener.pooled().then(|| (Arc::clone(&self.pool), key));
                (at, None, pooled)
            };
            // An embedded file has no sign-in.
            #[cfg(not(feature = "postgres"))]
            let (at, file) = {
                let _ = lease;
                let path = self.file_of(cx, surface, &fields)?;
                (StoreAt::File(path.as_path().to_path_buf()), Some(path))
            };
            // The file's identity now, before it is opened, and again once it
            // is: a connection opened by the name holds the file named.
            let file = match file {
                Some(path) => {
                    let identity = path.identity().map_err(StepFailure::not_sent)?;
                    Some((path, identity))
                }
                None => None,
            };
            let section_index = cx.plan.packs.iter().position(|section| {
                section.pack == PACK
                    && section.kind == STATEMENT
                    && section.surface_kind == surface.kind
            });
            let section = match section_index.and_then(|index| cx.plan.packs.get(index)) {
                Some(section) => section
                    .payload
                    .decode::<Section>()
                    .map_err(StepFailure::not_sent)?,
                None => Section::default(),
            };
            #[cfg(feature = "postgres")]
            let kept = pooled
                .as_ref()
                .and_then(|(pool, key)| pool.take(key, Access::ReadOnly));
            #[cfg(not(feature = "postgres"))]
            let kept: Option<Box<dyn Conn>> = None;
            let reader = match kept {
                Some(conn) => conn,
                None => {
                    let opening = {
                        let opener = Arc::clone(&opener);
                        let at = at.clone();
                        tokio::task::spawn_blocking(move || opener.open(&at, Access::ReadOnly))
                    };
                    let opened = tokio::time::timeout_at(cx.deadline.at(), opening).await;
                    match opened {
                        Ok(Ok(Ok(conn))) => conn,
                        Ok(Ok(Err(refusal))) => {
                            return Err(StepFailure::not_sent(refused(refusal, Access::ReadOnly)));
                        }
                        Ok(Err(_)) => return Err(StepFailure::not_sent(lost())),
                        Err(_) => {
                            return Err(StepFailure::not_sent(refused(
                                Refusal::TimedOut,
                                Access::ReadOnly,
                            )));
                        }
                    }
                }
            };
            let session_file = file;
            if session_file
                .as_ref()
                .is_some_and(|(path, identity)| path.identity().ok().as_ref() != Some(identity))
            {
                close(reader);
                return Err(StepFailure::not_sent(file_replaced()));
            }
            Ok(Prepared::new(Session {
                surface: surface.name.as_str().to_owned(),
                at,
                file: session_file,
                opener,
                #[cfg(feature = "postgres")]
                pooled,
                failed: false,
                may_write: matches!(
                    cx.posture,
                    Posture::Live | Posture::Sandbox { test_records: true }
                ),
                section,
                section_index,
                reader: Some(reader),
                writer: None,
                open: None,
                ran: None,
                counted: false,
                wrote: false,
                asked: None,
                described: BTreeSet::new(),
                looked_at: BTreeSet::new(),
                lost: false,
            }))
        })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let recorded = prepared.recorded().clone();
            let Some(session) = prepared.state::<Session>() else {
                return Err(StepFailure::not_sent(unexpected(
                    "The step was given a state the `sql` driver did not prepare.",
                )));
            };
            if session.lost {
                // After a lost connection nothing more is sent, and never a
                // second write.
                return Err(StepFailure::not_sent(lost()));
            }
            let done = |observed: Observed| StepOutcome::Done(observed);
            let outcome = if step.kind == BEGIN {
                session.begin(step, input).await.map(done)
            } else if step.kind == EXECUTE {
                session
                    .execute(step, input, &recorded, self.result_bytes)
                    .await
                    .map(done)
            } else if step.kind == EXPECT_ROWS {
                session.expect_rows(step).await.map(done)
            } else if step.kind == FETCH {
                session.fetch(step, self.redact.as_ref()).await
            } else if step.kind == COMMIT {
                session.commit(step, input).await.map(done)
            } else if step.kind == CATALOG {
                session
                    .catalog(step, input, self.redact.as_ref())
                    .await
                    .map(done)
            } else {
                Err(StepFailure::not_sent(unexpected(
                    "The step is of a kind the `sql` driver does not carry out.",
                )))
            };
            if outcome.is_err() {
                session.failed = true;
            }
            // What the store said is redacted here, before it leaves the
            // pack; the sentence is the pack's own.
            outcome.map_err(|mut stopped| {
                if stopped.failure.said().is_some() {
                    stopped.failure = self.redact.redact_failure(&stopped.failure);
                }
                stopped
            })
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        // From memory: the driver holds nothing between runs that could be
        // unhealthy. A store that cannot be opened says so at `prepare`.
        DriverHealth::Ready
    }

    fn release<'a>(&'a self, prepared: Prepared, poisoned: bool) -> BoxFuture<'a, Cleanup> {
        #[cfg(not(feature = "postgres"))]
        let _ = poisoned;
        Box::pin(async move {
            if let Ok(mut session) = prepared.into_state::<Session>() {
                // Rolls back anything open. The connections close when the
                // session is dropped, on a blocking thread: closing a file
                // is input and output too.
                session.roll_back().await;
                // A server store's connections are kept for the next run of
                // the same surface and session, unless anything failed: a
                // connection that saw a failure is never used again.
                #[cfg(feature = "postgres")]
                if let Some((pool, key)) = session.pooled.take()
                    && !poisoned
                    && !session.failed
                    && !session.lost
                    && session.open.is_none()
                {
                    if let Some(conn) = session.reader.take() {
                        pool.give(key.clone(), Access::ReadOnly, conn);
                    }
                    if let Some(conn) = session.writer.take() {
                        pool.give(key, Access::ReadWrite, conn);
                    }
                }
                let _ = tokio::task::spawn_blocking(move || drop(session)).await;
            }
            Cleanup::complete()
        })
    }
}
