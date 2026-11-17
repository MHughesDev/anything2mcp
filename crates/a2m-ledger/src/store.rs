//! The store: its threads, its transactions, and what is attached to it.
//!
//! The database crate blocks, so no database call is ever made on an async
//! thread. One thread owns the writing connection and runs one transaction
//! at a time; a file store has reading threads beside it, each with its own
//! connection, so a read never waits for a write. A caller hands a closure to
//! [`Store::write`] or [`Store::read`] and awaits the answer (decision 0080).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::thread;

use a2m_core::{Failure, KeyedDigest, Redact};
use a2m_telemetry::{Clock, SystemClock, Telemetry};
use a2m_vault::{OsRandom, RandomSource, Vault};
use tokio::sync::oneshot;

use crate::failure;
use crate::keys::StoreKeyRecords;
use crate::migrate;
use crate::sqlite::{Db, Place};
use crate::tx::{Durability, Owner, Tx};

/// The name of the database file in the data directory.
pub const DATABASE_FILE_NAME: &str = "store.sqlite3";
/// The name of the folder of evidence blobs in the data directory.
pub const BLOB_FOLDER_NAME: &str = "blobs";
/// The name of the span around each commit.
pub const COMMIT_SPAN: &str = "a2m.store.commit";

/// How many reading threads a file store starts.
const READERS: usize = 2;

/// Where the store is, and where its time and randomness come from.
pub struct StoreConfig {
    data_dir: Option<PathBuf>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    telemetry: Option<Telemetry>,
}

impl StoreConfig {
    /// A store in `data_dir`, a folder the engine owns. The database file and
    /// the blob folder are made inside it.
    #[must_use]
    pub fn in_dir(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: Some(data_dir.into()),
            clock: Arc::new(SystemClock::new()),
            random: Arc::new(OsRandom),
            telemetry: None,
        }
    }

    /// A store in memory, for tests. It is gone when the store is dropped,
    /// and its blobs are kept in the database.
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            data_dir: None,
            clock: Arc::new(SystemClock::new()),
            random: Arc::new(OsRandom),
            telemetry: None,
        }
    }

    /// Takes the time from `clock`, so a test can fix it.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Takes the random bytes of new identifiers from `random`, so a test
    /// gets the same identifiers on every run.
    #[must_use]
    pub fn with_random(mut self, random: Arc<dyn RandomSource>) -> Self {
        self.random = random;
        self
    }

    /// Reports a span for each commit to `telemetry`.
    #[must_use]
    pub fn with_telemetry(mut self, telemetry: Telemetry) -> Self {
        self.telemetry = Some(telemetry);
        self
    }
}

/// What joins the store once the vault is open.
pub(crate) struct Attached {
    pub(crate) vault: Arc<Vault>,
    pub(crate) digest: Arc<dyn KeyedDigest>,
    pub(crate) redact: Arc<dyn Redact>,
}

/// What every transaction can reach.
pub(crate) struct Shared {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) random: Arc<dyn RandomSource>,
    pub(crate) blob_dir: Option<PathBuf>,
    attached: OnceLock<Attached>,
    telemetry: Option<Telemetry>,
}

impl Shared {
    /// The vault's parts, or a refusal while no vault is attached.
    pub(crate) fn attached(&self) -> Result<&Attached, Failure> {
        self.attached
            .get()
            .ok_or_else(|| failure::unavailable("the vault is not attached yet"))
    }
}

type Job = Box<dyn FnOnce(&Db) + Send + 'static>;

/// Some threads that serve one queue of jobs, each with its own connection.
struct Lane {
    queue: Mutex<Option<mpsc::Sender<Job>>>,
    threads: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl Lane {
    async fn start(place: &Place, read_only: bool, threads: usize) -> Result<Self, Failure> {
        let (queue, jobs) = mpsc::channel::<Job>();
        let jobs = Arc::new(Mutex::new(jobs));
        let mut handles = Vec::new();
        for number in 0..threads {
            let (opened, answer) = oneshot::channel();
            let jobs = Arc::clone(&jobs);
            let place = place.clone();
            let name = if read_only {
                format!("a2m-store-read-{number}")
            } else {
                "a2m-store-write".to_owned()
            };
            let handle = thread::Builder::new()
                .name(name)
                .spawn(move || {
                    let db = match Db::open(&place, read_only) {
                        Ok(db) => {
                            let _ = opened.send(Ok(()));
                            db
                        }
                        Err(failure) => {
                            let _ = opened.send(Err(failure));
                            return;
                        }
                    };
                    loop {
                        let job = {
                            let jobs = jobs.lock().unwrap_or_else(PoisonError::into_inner);
                            jobs.recv()
                        };
                        match job {
                            Ok(job) => job(&db),
                            // Every sender is gone: the store was dropped.
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|_| failure::unavailable("a store thread could not be started"))?;
            handles.push(handle);
            answer
                .await
                .map_err(|_| failure::unavailable("a store thread stopped while opening"))??;
        }
        Ok(Self {
            queue: Mutex::new(Some(queue)),
            threads: Mutex::new(handles),
        })
    }

    fn send(&self, job: Job) -> Result<(), Failure> {
        let queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue
            .as_ref()
            .ok_or_else(stopped)?
            .send(job)
            .map_err(|_| stopped())
    }

    /// Runs `work` on one of the lane's threads and gives its answer.
    async fn run<T, F>(&self, work: F) -> Result<T, Failure>
    where
        T: Send + 'static,
        F: FnOnce(&Db) -> Result<T, Failure> + Send + 'static,
    {
        let (reply, answer) = oneshot::channel();
        self.send(Box::new(move |db| {
            let _ = reply.send(work(db));
        }))?;
        answer.await.map_err(|_| stopped())?
    }
}

impl Drop for Lane {
    fn drop(&mut self) {
        // Closing the queue ends each thread after the job it is in.
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let me = thread::current().id();
        for handle in self
            .threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            if handle.thread().id() != me {
                let _ = handle.join();
            }
        }
    }
}

fn stopped() -> Failure {
    failure::unavailable("its threads have stopped")
}

pub(crate) struct Inner {
    writer: Lane,
    readers: Option<Lane>,
    pub(crate) shared: Arc<Shared>,
}

/// The store. Cheap to clone; every clone is the same store. It closes when
/// the last clone is dropped.
#[derive(Clone)]
pub struct Store {
    pub(crate) inner: Arc<Inner>,
}

/// What the store reports about itself, for the kernel's health state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreHealth {
    /// Whether a transaction can be committed now.
    pub can_commit: bool,
    /// Whether the vault is attached. Until it is, blobs, events, records,
    /// and the audit chain refuse.
    pub vault_attached: bool,
}

impl Store {
    /// Opens the store: makes the data directory if it is not there, checks
    /// the database's integrity, and applies the migrations it lacks.
    ///
    /// # Errors
    ///
    /// `busy`/`store_corrupt` when the integrity check fails;
    /// `busy`/`store_version_unsupported` when a newer engine wrote the
    /// store or an applied migration differs from the one this engine
    /// carries; `busy`/`store_unavailable` when the file cannot be opened.
    pub async fn open(config: StoreConfig) -> Result<Self, Failure> {
        let (place, blob_dir) = match &config.data_dir {
            Some(dir) => {
                let blobs = dir.join(BLOB_FOLDER_NAME);
                std::fs::create_dir_all(&blobs)
                    .map_err(|_| failure::unavailable("the data directory cannot be made"))?;
                (Place::File(dir.join(DATABASE_FILE_NAME)), Some(blobs))
            }
            None => (Place::Memory, None),
        };
        let shared = Arc::new(Shared {
            clock: config.clock,
            random: config.random,
            blob_dir,
            attached: OnceLock::new(),
            telemetry: config.telemetry,
        });

        let writer = Lane::start(&place, false, 1).await?;
        let now = shared.clock.now();
        writer
            .run(move |db| {
                db.check_integrity()?;
                migrate::migrate(db, now)
            })
            .await?;
        let readers = match place {
            Place::File(_) => Some(Lane::start(&place, true, READERS).await?),
            Place::Memory => None,
        };
        Ok(Self {
            inner: Arc::new(Inner {
                writer,
                readers,
                shared,
            }),
        })
    }

    /// What the vault is opened with: where its wrapped keys are kept. It
    /// reads and writes only the table of wrapped keys.
    #[must_use]
    pub fn key_records(&self) -> StoreKeyRecords {
        StoreKeyRecords::new(Arc::downgrade(&self.inner))
    }

    /// Joins the opened vault to the store: its sealing for blobs, its keyed
    /// digest for the names of blobs and record keys, and its redactor for
    /// every text the store keeps. Done once, at start.
    ///
    /// # Errors
    ///
    /// What the vault returns when its digest key cannot be had;
    /// `unknown_variant`/`ledger_state_unexpected` when a vault is attached
    /// already.
    pub async fn attach(&self, vault: Arc<Vault>) -> Result<(), Failure> {
        let digest: Arc<dyn KeyedDigest> = vault.keyed_digest().await?;
        let redact: Arc<dyn Redact> = vault.redactor();
        self.inner
            .shared
            .attached
            .set(Attached {
                vault,
                digest,
                redact,
            })
            .map_err(|_| failure::unexpected("a vault is attached already"))
    }

    /// Runs `work` in one writing transaction, on the store's writing
    /// thread. When `work` returns `Ok`, the transaction commits and the
    /// value is returned; when it returns a failure, or panics, everything
    /// it did is taken back.
    ///
    /// Everything `work` does through the [`Tx`] commits together, whichever
    /// crates' tables it touches. `work` must not wait on anything: it holds
    /// the one writer.
    ///
    /// # Errors
    ///
    /// The failure `work` returned; `busy`/`store_unavailable` or
    /// `busy`/`storage_full` when the commit fails.
    pub async fn write<T, F>(&self, durability: Durability, work: F) -> Result<T, Failure>
    where
        T: Send + 'static,
        F: FnOnce(&mut Tx<'_>) -> Result<T, Failure> + Send + 'static,
    {
        let shared = Arc::clone(&self.inner.shared);
        self.inner
            .writer
            .run(move |db| transact(db, &shared, Some(durability), work))
            .await
    }

    /// Runs `work` in one reading transaction, which sees one unchanging
    /// state of the store and can change nothing. On a file store it runs
    /// beside the writer and never waits for it.
    ///
    /// # Errors
    ///
    /// The failure `work` returned; `busy` failures of the database.
    pub async fn read<T, F>(&self, work: F) -> Result<T, Failure>
    where
        T: Send + 'static,
        F: FnOnce(&mut Tx<'_>) -> Result<T, Failure> + Send + 'static,
    {
        let shared = Arc::clone(&self.inner.shared);
        let lane = self.inner.readers.as_ref().unwrap_or(&self.inner.writer);
        lane.run(move |db| transact(db, &shared, None, work)).await
    }

    /// Whether the store can commit, and whether the vault is attached.
    pub async fn health(&self) -> StoreHealth {
        let can_commit = self
            .write(Durability::Relaxed, |tx| {
                tx.exec("UPDATE schema_migrations SET number = number WHERE 0", &[])
            })
            .await
            .is_ok();
        StoreHealth {
            can_commit,
            vault_attached: self.inner.shared.attached.get().is_some(),
        }
    }

    /// The name of every table the store holds, in order, each with the
    /// crate that owns it; `None` for a table no migration of this engine
    /// creates.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub async fn tables(&self) -> Result<Vec<(String, Option<Owner>)>, Failure> {
        self.inner
            .writer
            .run(|db| {
                Ok(db
                    .table_names()?
                    .into_iter()
                    .map(|name| {
                        let owner = migrate::table_owner(&name);
                        (name, owner)
                    })
                    .collect())
            })
            .await
    }

    /// The clock the store stamps its rows with.
    #[must_use]
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.inner.shared.clock)
    }

    /// The folder evidence blobs are kept in, or `None` for a store in
    /// memory.
    #[must_use]
    pub fn blob_dir(&self) -> Option<&Path> {
        self.inner.shared.blob_dir.as_deref()
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.inner.shared
    }
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("in_memory", &self.inner.shared.blob_dir.is_none())
            .finish()
    }
}

impl Inner {
    /// For the key records, which hold the store weakly.
    pub(crate) async fn write<T, F>(&self, durability: Durability, work: F) -> Result<T, Failure>
    where
        T: Send + 'static,
        F: FnOnce(&mut Tx<'_>) -> Result<T, Failure> + Send + 'static,
    {
        let shared = Arc::clone(&self.shared);
        self.writer
            .run(move |db| transact(db, &shared, Some(durability), work))
            .await
    }
}

pub(crate) fn upgrade(store: &Weak<Inner>) -> Result<Arc<Inner>, Failure> {
    store
        .upgrade()
        .ok_or_else(|| failure::unavailable("it has been closed"))
}

/// One transaction around `work`, on the thread that owns `db`.
fn transact<T, F>(
    db: &Db,
    shared: &Shared,
    write: Option<Durability>,
    work: F,
) -> Result<T, Failure>
where
    F: FnOnce(&mut Tx<'_>) -> Result<T, Failure>,
{
    db.begin(write)?;
    let mut tx = Tx::new(db, shared, write.is_some());
    // A panic in `work` must not take the store's thread with it, and must
    // not leave its transaction open.
    let outcome = catch_unwind(AssertUnwindSafe(|| work(&mut tx)));
    match outcome {
        Ok(Ok(value)) => {
            let span = match (&shared.telemetry, write) {
                (Some(telemetry), Some(_)) => Some(telemetry.span(COMMIT_SPAN, None, &[])),
                _ => None,
            };
            db.end(write.is_some(), write.is_some())?;
            drop(span);
            Ok(value)
        }
        Ok(Err(failure)) => {
            let _ = db.end(false, write.is_some());
            Err(failure)
        }
        Err(_) => {
            let _ = db.end(false, write.is_some());
            Err(failure::unavailable(
                "the code inside a transaction stopped unexpectedly, and the transaction was taken back",
            ))
        }
    }
}
