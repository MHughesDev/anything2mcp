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
//!
//! Beside the data directory's lock the engine writes the runtime directory
//! it serves, so that a command whose own runtime directory holds no engine
//! can say where the engine is when it is refused. That record names a
//! place in a message; nothing connects there by it (decision 0432).

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use a2m_actions::{OsFamily, Paths};
use a2m_core::Failure;

use crate::codes;
use crate::os;

/// The name of the lock file in the runtime directory and in the data
/// directory.
pub const ENGINE_LOCK_NAME: &str = "engine.lock";

/// The name of the file in the data directory, beside its lock, that names
/// the runtime directory of the engine that holds the lock (decision 0432).
pub const RUNTIME_RECORD_NAME: &str = "engine.runtime";

/// The longest record that is read: a path longer than this is no runtime
/// directory this engine wrote.
const RECORD_LONGEST: u64 = 4096;

/// The engine lock, held. Dropping it releases it: the data directory's
/// lock first, so that a lock of the data directory held alone is never an
/// engine of the same runtime directory that is closing.
#[derive(Debug)]
pub struct EngineLock {
    _data: File,
    _runtime: File,
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
    codes::failure(
        &codes::row::ENGINE_RUNNING,
        "An engine already runs for this user and data directory.",
        "Use the running engine, or stop it with `anything2mcp engine stop` first.",
    )
}

/// Refused at the data directory's lock while this command's runtime
/// directory holds no engine: the engine that holds it serves another
/// runtime directory, `there`, which this command's environment does not
/// lead to. The fix names the folder, and on Linux the variable that leads
/// there.
fn running_elsewhere(os: OsFamily, there: &Path, here: &Path) -> Failure {
    let there_shown = crate::connector::shown(there);
    let fix = match (os, there.parent()) {
        (OsFamily::Linux, Some(parent)) if there.ends_with(a2m_actions::APP_FOLDER) => format!(
            "Use the running engine: start the command with `XDG_RUNTIME_DIR` set to `{}`, as the engine was. Or stop that engine with `anything2mcp engine stop` from where it was started, and run the command again.",
            crate::connector::shown(parent)
        ),
        _ => format!(
            "Use the running engine from where it was started, or stop it there with `anything2mcp engine stop`, and run the command again. Its runtime directory is `{there_shown}`."
        ),
    };
    codes::failure(
        &codes::row::ENGINE_RUNNING,
        format!(
            "An engine already runs for this user and data directory, and serves its local channel in the runtime directory `{there_shown}`; this command's environment leads to `{}`, where no engine runs.",
            crate::connector::shown(here)
        ),
        fix,
    )
}

/// The runtime directory the record in the data directory names, when it
/// names one other than `here`. A record that is missing, too long, not
/// text, holds a control character, or is not an absolute path names
/// nothing: it is read for a message, and a message never carries what is
/// not a plain path.
fn recorded_elsewhere(paths: &Paths) -> Option<PathBuf> {
    use std::io::Read as _;
    let file = File::open(paths.data.join(RUNTIME_RECORD_NAME)).ok()?;
    let mut text = String::new();
    file.take(RECORD_LONGEST + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.is_empty() || text.len() as u64 > RECORD_LONGEST || text.chars().any(char::is_control) {
        return None;
    }
    let there = PathBuf::from(text);
    (there.is_absolute() && there != paths.runtime).then_some(there)
}

/// Writes the runtime directory this engine serves into the data
/// directory, whole: into a new file of its own, closed to everyone but the
/// user on macOS and Linux, which then takes the record's name. A runtime
/// directory whose path is not text is not written, and the record of an
/// earlier engine is removed.
fn record_runtime(paths: &Paths) -> std::io::Result<()> {
    use std::io::Write as _;
    let record = paths.data.join(RUNTIME_RECORD_NAME);
    let Some(text) = paths.runtime.to_str() else {
        return match std::fs::remove_file(&record) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    };
    let fresh = paths.data.join(format!("{RUNTIME_RECORD_NAME}.new"));
    // Whatever stands at the new file's name, a link among them, is
    // removed, not followed; the new file is made where nothing is.
    match std::fs::remove_file(&fresh) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&fresh)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&fresh, &record)
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

/// Why the engine lock was not taken.
enum Refused {
    /// An engine of another runtime directory holds the data directory's
    /// lock, and its record names that directory.
    Elsewhere(Failure),
    /// Anything else.
    Other(Failure),
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
    ///
    /// Refused at the data directory's lock alone, while the runtime
    /// directory's is free, by an engine whose record names another runtime
    /// directory, the refusal names that directory and the way there
    /// (decision 0432).
    pub fn take(paths: &Paths) -> Result<Self, Failure> {
        Self::take_or_say(paths, true).map_err(|refused| match refused {
            Refused::Elsewhere(failure) | Refused::Other(failure) => failure,
        })
    }

    /// Whether the engine lock is held by an engine that serves another
    /// runtime directory than `paths` names, which a command of this
    /// environment can never reach: the refusal that says where it is, or
    /// `None` when the lock is free, or held by an engine of this runtime
    /// directory, or by one that wrote no record. Like [`EngineLock::held`],
    /// a question for a report, never a guard.
    ///
    /// # Errors
    ///
    /// `busy`/`channel_unavailable` when a directory or a lock file cannot
    /// be opened.
    pub fn elsewhere(paths: &Paths) -> Result<Option<Failure>, Failure> {
        match Self::take_or_say(paths, false) {
            Ok(_released_here) => Ok(None),
            Err(Refused::Elsewhere(failure)) => Ok(Some(failure)),
            Err(Refused::Other(failure)) if *failure.code() == codes::ENGINE_RUNNING => Ok(None),
            Err(Refused::Other(failure)) => Err(failure),
        }
    }

    /// Takes the lock; `record` writes the runtime directory beside it, as
    /// an engine does and a question does not.
    fn take_or_say(paths: &Paths, record: bool) -> Result<Self, Refused> {
        os::prepare_dir(&paths.runtime)
            .map_err(|error| Refused::Other(codes::unavailable("The runtime directory", &error)))?;
        std::fs::create_dir_all(&paths.data)
            .map_err(|error| Refused::Other(codes::unavailable("The data directory", &error)))?;
        let runtime = open(&paths.runtime).map_err(Refused::Other)?;
        lock(&runtime).map_err(Refused::Other)?;
        let data = open(&paths.data).map_err(Refused::Other)?;
        if let Err(failure) = lock(&data) {
            // The runtime directory's lock is free and the data
            // directory's is held: the engine that holds it took the lock
            // of another runtime directory first.
            if *failure.code() == codes::ENGINE_RUNNING
                && let Some(there) = recorded_elsewhere(paths)
            {
                return Err(Refused::Elsewhere(running_elsewhere(
                    OsFamily::current(),
                    &there,
                    &paths.runtime,
                )));
            }
            return Err(Refused::Other(failure));
        }
        // The record is read for a message and guards nothing, so an engine
        // that cannot write it still runs; the record of an engine before
        // it is removed, so that no message names a place it is not.
        if record && record_runtime(paths).is_err() {
            let _ = std::fs::remove_file(paths.data.join(RUNTIME_RECORD_NAME));
        }
        let place = LockPlace::of(paths.data.join(ENGINE_LOCK_NAME), &data);
        Ok(Self {
            _data: data,
            _runtime: runtime,
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
        // A question writes no record.
        match Self::take_or_say(paths, false) {
            Ok(_released_here) => Ok(false),
            Err(Refused::Elsewhere(_)) => Ok(true),
            Err(Refused::Other(failure)) if *failure.code() == codes::ENGINE_RUNNING => Ok(true),
            Err(Refused::Other(failure)) => Err(failure),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test: a helper that cannot do its work stops the test"
    )]

    use std::path::{Path, PathBuf};

    use a2m_actions::{OsFamily, Paths};

    use super::{EngineLock, RUNTIME_RECORD_NAME, running_elsewhere};
    use crate::codes;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("a2m-local-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// Two layouts that share the data directory and differ in their
    /// runtime directories, as two processes of one user do when one has
    /// `XDG_RUNTIME_DIR` and the other does not.
    fn two_runtimes(root: &Path) -> (Paths, Paths) {
        let first = Paths::under(root);
        let mut second = first.clone();
        second.runtime = root.join("other-run");
        (first, second)
    }

    /// Decision 0432: the engine records the runtime directory it serves
    /// beside the data directory's lock, closed to everyone but its user;
    /// a second layout with another runtime directory is refused
    /// `engine_running` with that directory named, and is told so as a
    /// lock held elsewhere; a layout of the same runtime directory is
    /// refused as before, and is not.
    #[test]
    fn a_lock_held_from_another_runtime_directory_is_refused_with_that_directory_named() {
        let root = root("elsewhere");
        let (first, second) = two_runtimes(&root);
        let held = EngineLock::take(&first).unwrap();
        let record = first.data.join(RUNTIME_RECORD_NAME);
        assert_eq!(
            std::fs::read_to_string(&record).unwrap(),
            first.runtime.to_str().unwrap()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&record).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "{mode:o}");
        }

        let refused = EngineLock::take(&second).unwrap_err();
        assert_eq!(refused.code(), &codes::ENGINE_RUNNING);
        let shown = crate::connector::shown(&first.runtime);
        assert!(refused.message().contains(&shown), "{refused:?}");
        let told = EngineLock::elsewhere(&second).unwrap().unwrap();
        assert_eq!(told.code(), &codes::ENGINE_RUNNING);
        assert!(told.message().contains(&shown), "{told:?}");
        assert!(EngineLock::held(&second).unwrap());

        // The same runtime directory: refused as before, and not elsewhere.
        let again = EngineLock::take(&first).unwrap_err();
        assert_eq!(again.code(), &codes::ENGINE_RUNNING);
        assert!(!again.message().contains(&shown), "{again:?}");
        assert!(EngineLock::elsewhere(&first).unwrap().is_none());

        // A record that is not a plain absolute path names nothing.
        drop(held);
        let held = EngineLock::take(&first).unwrap();
        std::fs::write(&record, "relative/run").unwrap();
        assert!(EngineLock::elsewhere(&second).unwrap().is_none());
        std::fs::write(&record, format!("{}\n", first.runtime.display())).unwrap();
        assert!(EngineLock::elsewhere(&second).unwrap().is_none());
        drop(held);
        // Free: nothing is elsewhere, and the one that takes it now writes
        // its own runtime directory.
        assert!(EngineLock::elsewhere(&second).unwrap().is_none());
        let taken = EngineLock::take(&second).unwrap();
        assert_eq!(
            std::fs::read_to_string(&record).unwrap(),
            second.runtime.to_str().unwrap()
        );
        drop(taken);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The fix names the variable that leads to the engine's runtime
    /// directory on Linux, and the directory alone elsewhere.
    #[test]
    fn the_fix_names_the_way_to_the_engine_for_each_system() {
        let there = Path::new("/run/user/1000/anything2mcp");
        let here = Path::new("/home/pat/.local/share/anything2mcp/run");
        let linux = running_elsewhere(OsFamily::Linux, there, here);
        assert!(linux.message().contains("/run/user/1000/anything2mcp"));
        assert!(linux.message().contains(here.to_str().unwrap()));
        assert!(
            linux
                .next_step()
                .contains("`XDG_RUNTIME_DIR` set to `/run/user/1000`"),
            "{linux:?}"
        );
        // Elsewhere, no variable leads to another runtime directory of the
        // same data directory: the folder is named, and no variable.
        let mac = running_elsewhere(OsFamily::MacOs, there, here);
        assert!(mac.next_step().contains(there.to_str().unwrap()), "{mac:?}");
        assert!(!mac.next_step().contains("XDG_RUNTIME_DIR"), "{mac:?}");
        let other = running_elsewhere(OsFamily::Linux, Path::new("/home/pat/run"), here);
        assert!(other.next_step().contains("/home/pat/run"), "{other:?}");
        assert!(!other.next_step().contains("XDG_RUNTIME_DIR"), "{other:?}");
    }
}
