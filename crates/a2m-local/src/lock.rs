//! The engine lock: one engine per user and per data directory.
//!
//! It is two operating-system file locks, taken together and held for the
//! engine's whole life: one in the runtime directory, which guards the
//! channel, and one in the data directory, which guards the store. Two
//! runtime directories can lead to one data directory, for example when a
//! second shell has no runtime directory of its own, and two engines on one
//! store must never run. The operating system releases both when the
//! process ends, however it ends; a lock file that merely exists means
//! nothing.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use a2m_actions::Paths;
use a2m_core::Failure;

use crate::codes;
use crate::os;

/// The name of the lock file in the runtime directory and in the data
/// directory.
pub const ENGINE_LOCK_NAME: &str = "engine.lock";

/// The engine lock, held. Dropping it releases it.
#[derive(Debug)]
pub struct EngineLock {
    _runtime: File,
    _data: File,
    place: LockPlace,
}

/// Where the engine lock of the data directory was taken, and which file
/// it is. Cheap to clone. An engine asks it, on each use of its data
/// directory that it can see, whether the lock it holds is still the file
/// at that place: when the data directory was removed, or replaced, the
/// store and the lock in it are no longer where the engine opened them,
/// and a second engine could take a new lock there and open a new store
/// beside this one (decision 0181).
#[derive(Debug, Clone)]
pub struct LockPlace {
    path: PathBuf,
    identity: Option<(u64, u64)>,
}

impl LockPlace {
    fn of(path: PathBuf, held: &File) -> Self {
        let identity = held.metadata().ok().and_then(|held| identity(&held));
        Self { path, identity }
    }

    /// Whether the file at the lock's place is the one this engine holds.
    /// On macOS and Linux the device and file numbers are compared, so a
    /// data directory removed and made again is seen as gone. On Windows a
    /// directory that holds an open file can seldom be removed; this build
    /// reads no file number there and asks only that the lock file still
    /// stands at its place. It reads the file's metadata and opens nothing.
    #[must_use]
    pub fn holds(&self) -> bool {
        match std::fs::metadata(&self.path) {
            Ok(now) if now.is_file() => match (self.identity, identity(&now)) {
                (Some(held), Some(now)) => held == now,
                // Where no file number is read, the file standing there
                // is all that is asked.
                (None, None) => true,
                _ => false,
            },
            _ => false,
        }
    }
}

#[cfg(unix)]
fn identity(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
const fn identity(_metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

fn running() -> Failure {
    codes::busy(
        codes::ENGINE_RUNNING,
        "An engine already runs for this user and data directory.",
        "Use the running engine, or stop it with `anything2mcp engine stop` first.",
    )
}

fn open(dir: &Path) -> Result<File, Failure> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(ENGINE_LOCK_NAME))
        .map_err(|error| codes::unavailable("The engine's lock file", &error))
}

fn lock(file: &File) -> Result<(), Failure> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(running()),
        Err(TryLockError::Error(error)) => {
            Err(codes::unavailable("The engine's lock file", &error))
        }
    }
}

impl EngineLock {
    /// Takes the engine lock, making the runtime and data directories when
    /// they are missing. It does not wait.
    ///
    /// # Errors
    ///
    /// `busy`/`engine_running` when another engine holds either lock;
    /// `busy`/`channel_unavailable` when a directory or a lock file cannot
    /// be opened.
    pub fn take(paths: &Paths) -> Result<Self, Failure> {
        os::prepare_dir(&paths.runtime)
            .map_err(|error| codes::unavailable("The runtime directory", &error))?;
        std::fs::create_dir_all(&paths.data)
            .map_err(|error| codes::unavailable("The data directory", &error))?;
        let runtime = open(&paths.runtime)?;
        lock(&runtime)?;
        let data = open(&paths.data)?;
        lock(&data)?;
        let place = LockPlace::of(paths.data.join(ENGINE_LOCK_NAME), &data);
        Ok(Self {
            _runtime: runtime,
            _data: data,
            place,
        })
    }

    /// Where the lock of the data directory was taken, to ask later
    /// whether it is still there.
    #[must_use]
    pub fn place(&self) -> LockPlace {
        self.place.clone()
    }

    /// Whether an engine holds the lock, without keeping it. This is a
    /// question for a report, never a guard: the answer may be wrong the
    /// moment it is given. Whatever opens the store, a `serve` or a command
    /// that runs the kernel inside its own process, takes the lock with
    /// [`EngineLock::take`] and holds it until the kernel has closed.
    ///
    /// # Errors
    ///
    /// `busy`/`channel_unavailable` when a directory or a lock file cannot
    /// be opened.
    pub fn held(paths: &Paths) -> Result<bool, Failure> {
        match Self::take(paths) {
            Ok(_released_here) => Ok(false),
            Err(failure) if *failure.code() == codes::ENGINE_RUNNING => Ok(true),
            Err(failure) => Err(failure),
        }
    }
}
