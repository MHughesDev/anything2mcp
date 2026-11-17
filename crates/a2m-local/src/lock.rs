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
use std::path::Path;

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
        Ok(Self {
            _runtime: runtime,
            _data: data,
        })
    }

    /// Whether an engine holds the lock, without keeping it. A command
    /// that would run the engine inside itself asks this first.
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
