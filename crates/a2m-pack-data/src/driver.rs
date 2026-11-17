//! The `sql` driver: one step at a time against a relational store.
//!
//! A read runs only on a connection the store itself holds read-only. A
//! write runs inside one transaction: the count of rows it changed is
//! checked before `commit`, and any other count rolls back. The driver
//! never retries, and it says for every failure whether anything was sent.
//!
//! Every call into the store blocks, so each runs on a blocking thread; the
//! connection moves there and back. A step that is dropped while it waits
//! leaves its connection behind, which closes it, and the store rolls back.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use a2m_auth::SessionLease;
use a2m_core::{
    FailsAs, Failure, FailureClass, FailureCode, RecordKey, Redact, Step, Value, Values,
};
use a2m_provider::{
    BoxFuture, Cleanup, Deadline, Driver, DriverHealth, Observed, Posture, PrepareContext,
    Prepared, RootName, Roots, StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use a2m_telemetry::{Acting, Detail};

use crate::model::{
    Access, Column, Returns, Section, Statement, StepDetail, Stored, SurfaceFields,
};
use crate::names::{
    BEGIN, CATALOG, COMMIT, DATABASE, EXECUTE, EXPECT_ROWS, FETCH, PACK, STATEMENT, codes,
};
use crate::store::{
    Bound, Catalog, Cell, CommitFailed, Conn, Interrupter, Opener, Ran, Refusal, RunLimits,
};

/// The root an embedded store's file is in when its surface names none.
const DEFAULT_ROOT: &str = "project";
/// The most tables and views one catalog read returns.
pub(crate) const CATALOG_TABLES: usize = 2000;

/// Finds the opener of a store by the name a surface states.
pub(crate) type Openers = Arc<dyn Fn(&str) -> Option<Arc<dyn Opener>> + Send + Sync>;

/// The driver of the mode `sql`.
pub(crate) struct SqlDriver {
    redact: Arc<dyn Redact>,
    result_bytes: u64,
    openers: Openers,
    /// The pack's own roots, as its manifest declares them.
    roots: Roots,
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
        }
    }
}

/// What `prepare` acquired, and where the binding's transaction stands.
pub(crate) struct Session {
    file: std::path::PathBuf,
    opener: Arc<dyn Opener>,
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
    /// The statements whose shape was compared with the plan.
    described: BTreeSet<String>,
    /// The tables whose triggers and cascading keys were looked at.
    looked_at: BTreeSet<String>,
    /// A connection was lost or left in doubt. Nothing more is sent.
    lost: bool,
    /// What each step did, for the step trace. The contract has no place to
    /// hand this on yet; the driver keeps it so that its shape is fixed.
    pub(crate) acting: Vec<Acting>,
}

fn failure(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(class, code, message, next)
}

fn unexpected(message: &str) -> Failure {
    failure(
        FailureClass::UnknownVariant,
        codes::UNEXPECTED_STATUS,
        message,
        "Report this to the pack's author; the run can be started again.",
    )
}

fn drift(message: &str) -> Failure {
    failure(
        FailureClass::Drift,
        codes::SCHEMA_CHANGED,
        message,
        "Run discovery again, compare the store's catalog with the statements file, and publish a corrected version.",
    )
}

fn lost() -> Failure {
    failure(
        FailureClass::Transient,
        codes::CONNECTION_LOST,
        "The connection to the store ended before the step was sent.",
        "Start the run again; nothing was written.",
    )
}

/// What a store's refusal is as a failure. `access` is that of the
/// transaction the statement ran in.
fn refused(refusal: Refusal, access: Access) -> Failure {
    match refusal {
        Refusal::Locked => failure(
            FailureClass::Busy,
            codes::STORE_LOCKED,
            "Another connection held the store's lock for the whole time limit.",
            "Try again when the other program has finished its work.",
        ),
        Refusal::TimedOut => failure(
            codes::TRANSIENT,
            codes::TIMEOUT,
            "The statement ran past its time limit and was stopped.",
            "Try again, or raise the statement's `time_limit`.",
        ),
        Refusal::Constraint(kind) => failure(
            FailureClass::Rejected,
            codes::CONSTRAINT_VIOLATED,
            format!("A constraint of the store refused the write (kind: {kind})."),
            "Correct the values so that they satisfy the store's constraints; nothing was written.",
        ),
        Refusal::ReadOnly if access == Access::ReadOnly => failure(
            FailureClass::Policy,
            codes::READ_STATEMENT_WROTE,
            "A statement declared as a read tried to change data, and the store refused it.",
            "Correct the statement or declare it `effect: write`; do not use the binding as a read.",
        ),
        Refusal::ReadOnly | Refusal::Denied => failure(
            FailureClass::Policy,
            codes::DENIED_BY_STORE,
            "The store does not let this identity write.",
            "Ask the store's owner for the right to write, or use a route that has it.",
        ),
        Refusal::Shape => drift(
            "A table, a column, or a parameter of the statement differs from what the target declares.",
        ),
        Refusal::Unavailable => failure(
            FailureClass::Transient,
            codes::UPSTREAM_UNAVAILABLE,
            "The store cannot be opened.",
            "Check that the store's file exists and can be read, then try again.",
        ),
        Refusal::TooLarge => failure(
            FailureClass::UnknownVariant,
            codes::RESULT_TOO_LARGE,
            "The statement's result holds more text than the limit allows.",
            "Lower the statement's `max_rows`, or raise the pack's `result_bytes` setting.",
        ),
        Refusal::NotAllowed => failure(
            FailureClass::Policy,
            codes::STATEMENT_NOT_ALLOWED,
            "The connection's own guard refused the statement: it is not one plain read or one keyed write.",
            "Correct the statement in the statements file: a read is one `SELECT`, and a write is written by the pack from its record.",
        ),
        Refusal::NotGuarded => unexpected(
            "The store did not take the setting that holds the connection to its access.",
        ),
        Refusal::Other(code) => failure(
            FailureClass::UnknownVariant,
            codes::UNEXPECTED_STATUS,
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
                .map(|statement| (reference.entry.clone(), statement.clone()))
        });
        found.ok_or_else(|| {
            StepFailure::not_sent(unexpected(
                "The step refers to a statement the plan's section does not hold.",
            ))
        })
    }

    /// Rolls back whatever is open, on the connection that holds it. A
    /// connection that does not come back is closed by being dropped.
    async fn roll_back(&mut self) {
        self.ran = None;
        self.counted = false;
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

    fn note(&mut self, step: &Step, means: Vec<Detail>, measured: Vec<Detail>) {
        let mut all = vec![Detail::new("step", step.kind.as_str())];
        all.extend(means);
        self.acting.push(Acting {
            means: all,
            measured,
        });
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
                FailureClass::Policy,
                codes::READ_ONLY_POSTURE,
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
                None => {
                    let opener = Arc::clone(&self.opener);
                    let file = self.file.clone();
                    let opened =
                        tokio::task::spawn_blocking(move || opener.open(&file, Access::ReadWrite))
                            .await;
                    match opened {
                        Ok(Ok(conn)) => Some(conn),
                        Ok(Err(refusal)) => {
                            return Err(StepFailure::not_sent(refused(refusal, access)));
                        }
                        Err(_) => None,
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
        self.give_back(access, conn);
        match began {
            Ok(()) => {
                self.open = Some(access);
                self.note(
                    step,
                    vec![Detail::new(
                        "access",
                        match access {
                            Access::ReadOnly => "read_only",
                            Access::ReadWrite => "read_write",
                        },
                    )],
                    Vec::new(),
                );
                Ok(Observed::default())
            }
            // No transaction was opened, so nothing was sent that could
            // change data.
            Err(refusal) => Err(StepFailure::not_sent(refused(refusal, access))),
        }
    }

    #[allow(clippy::too_many_lines)] // one check after another, each a few lines; splitting hides the order
    async fn execute(
        &mut self,
        step: &Step,
        input: &StepInput<'_>,
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
        let mut params = Vec::with_capacity(statement.params.len());
        for param in &statement.params {
            let Some(value) = input.args.get(&param.name).and_then(bound) else {
                return Err(self
                    .not_sent(failure(
                        FailureClass::Input,
                        codes::VALUE_NOT_BINDABLE,
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
        let needs_shape = !self.described.contains(&name);
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
        let table_to_check = table.clone();
        let work = move |conn: &mut dyn Conn| -> Result<Ran, Result<Refusal, &'static str>> {
            if needs_shape {
                // The store describes the statement without running it; the
                // plan is held to that description.
                let shape = conn.describe(&text).map_err(Ok)?;
                if shape.params != expected_params {
                    return Err(Err(
                        "The statement takes another number of parameters than the target declares.",
                    ));
                }
                if shape.columns.iter().any(|column| !known.contains(column)) {
                    return Err(Err(
                        "The statement returns a column that the target's record does not declare.",
                    ));
                }
            }
            if let Some(table) = &table_to_check {
                let catalog: Catalog = conn.catalog(CATALOG_TABLES).map_err(Ok)?;
                let (triggers, cascades) = catalog.reach(table);
                if triggers + cascades > 0 {
                    return Err(Err(
                        "The table has a trigger or a cascading key, and the statement declares no hazard: the write would reach data beyond the rows it names.",
                    ));
                }
            }
            conn.run(&text, &params, &limits).map_err(Ok)
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
                self.note(
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
                Ok(Observed::default())
            }
            // Found before the statement ran.
            Err(Err(message)) => Err(self.not_sent(drift(message)).await),
            Err(Ok(refusal)) => Err(self.answered(refused(refusal, access)).await),
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
        if changed == expected {
            self.counted = true;
            self.note(
                step,
                vec![Detail::new("rows", expected.to_string())],
                Vec::new(),
            );
            return Ok(Observed::default());
        }
        // Any other count rolls back, so nothing is committed.
        let FailsAs { class, code } = step.fails_as.clone();
        let found = if changed < expected {
            failure(
                class,
                code,
                "The write changed no row: the record, or its version, is not what the preview saw.",
                "Read the record again, look at what changed, and decide whether to write again.",
            )
        } else {
            failure(
                FailureClass::Drift,
                codes::KEY_NOT_UNIQUE,
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
                FailureClass::UnknownVariant,
                codes::VALUE_KIND_UNSUPPORTED,
                format!(
                    "The field `{}` holds a float or bytes, and no rule reads either exactly.",
                    column.field
                ),
                "Read the column through a statement that returns it as exact text, or leave the field out.",
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

    async fn fetch(
        &mut self,
        step: &Step,
        redact: &dyn Redact,
    ) -> Result<StepOutcome, StepFailure> {
        let (_, statement) = self.statement(step)?;
        let Some(ran) = self.ran.take() else {
            return Err(self
                .not_sent(unexpected(
                    "Rows were to be fetched, and no statement had run.",
                ))
                .await);
        };
        let rows = ran.rows.len();
        let outcome = match statement.returns {
            Returns::OneOrNone if ran.more => Err(failure(
                FailureClass::Drift,
                codes::KEY_NOT_UNIQUE,
                "A read by key matched more than one row.",
                "Correct the record's key in the statements file.",
            )),
            Returns::OneOrNone => match ran.rows.into_iter().next() {
                None => Ok(StepOutcome::Absent(Observed::default())),
                Some(row) => Self::record(&statement, &ran.columns, row).map(|values| {
                    StepOutcome::Done(Observed {
                        values: redact.redact_values(&values),
                        ..Observed::default()
                    })
                }),
            },
            Returns::Many => ran
                .rows
                .into_iter()
                .map(|row| Self::record(&statement, &ran.columns, row).map(Value::Record))
                .collect::<Result<Vec<Value>, Failure>>()
                .map(|records| {
                    let values = Values::new()
                        .with("rows", Value::List(records))
                        .with("more", ran.more);
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
            Ok(outcome) => {
                self.note(
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
                self.note(step, Vec::new(), Vec::new());
                Ok(Observed::default())
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
                    FailureClass::PossibleCommit,
                    codes::NO_CONFIRMATION,
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
        self.note(
            step,
            Vec::new(),
            vec![Detail::new("tables", catalog.tables.len().to_string())],
        );
        Ok(Observed {
            values: redact.redact_values(&crate::discover::catalog_values(&catalog)),
            ..Observed::default()
        })
    }
}

impl Driver for SqlDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        _lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move {
            let surface = &cx.surface.surface;
            if surface.kind != DATABASE {
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
            // The store's file arrives as a checked path under the
            // surface's name. The driver resolves no path itself, and takes
            // only the file the target declares.
            let root = fields.root.as_deref().unwrap_or(DEFAULT_ROOT);
            let given = cx
                .paths
                .get(surface.name.as_str())
                .filter(|path| path.root().as_str() == root && path.relative() == surface.address)
                .cloned();
            // A run that was handed no path for the surface gets the same
            // file through the one check that makes a path: the surface's
            // root, among the roots the pack's manifest declares, and the
            // surface's address below it. Nothing in a plan yet tells the
            // engine that a surface's address is a file, so the engine
            // hands none over.
            let path = given.or_else(|| {
                let name = RootName::new(root).ok()?;
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
                    FailureClass::Input,
                    codes::PATH_NOT_ALLOWED,
                    format!(
                        "The run was given no checked path for the file of the surface `{}` in the root `{root}`.",
                        surface.name
                    ),
                    "Keep the store's file inside the named root, and name it in `target.yaml` by its path below that root.",
                )));
            };
            let file = path.as_path().to_path_buf();
            let section_index = cx.plan.packs.iter().position(|section| {
                section.pack == PACK
                    && section.kind == STATEMENT
                    && section.surface_kind == DATABASE
            });
            let section = match section_index.and_then(|index| cx.plan.packs.get(index)) {
                Some(section) => section
                    .payload
                    .decode::<Section>()
                    .map_err(StepFailure::not_sent)?,
                None => Section::default(),
            };
            let opening = {
                let opener = Arc::clone(&opener);
                let file = file.clone();
                tokio::task::spawn_blocking(move || opener.open(&file, Access::ReadOnly))
            };
            let opened = tokio::time::timeout_at(cx.deadline.at(), opening).await;
            let reader = match opened {
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
            };
            Ok(Prepared::new(Session {
                file,
                opener,
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
                described: BTreeSet::new(),
                looked_at: BTreeSet::new(),
                lost: false,
                acting: Vec::new(),
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
            if step.kind == BEGIN {
                session.begin(step, input).await.map(done)
            } else if step.kind == EXECUTE {
                session
                    .execute(step, input, self.result_bytes)
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
            }
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        // From memory: the driver holds nothing between runs that could be
        // unhealthy. A store that cannot be opened says so at `prepare`.
        DriverHealth::Ready
    }

    fn release<'a>(&'a self, prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            if let Ok(mut session) = prepared.into_state::<Session>() {
                // Rolls back anything open. The connections close when the
                // session is dropped, on a blocking thread: closing a file
                // is input and output too.
                session.roll_back().await;
                let _ = tokio::task::spawn_blocking(move || drop(session)).await;
            }
            Cleanup::complete()
        })
    }
}
