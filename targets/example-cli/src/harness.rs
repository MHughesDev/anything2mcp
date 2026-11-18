//! The ground-truth harness: gives a test a program of its own and a fresh
//! home folder, and tells it, through the control interface, what the program
//! actually stored and what it was actually started with.
//!
//! A test never proves a write using the product's own read path alone, and
//! never by running the program's read commands. It asks [`Control`].

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::control::{CONTROL_FOLDER, ControlDir, Invocation};
use crate::faults::Fault;
use crate::store::{
    DATA_TEMP_FILE, HomeLock, PushRecord, SESSION_FILE, Store, StoredItem, WriteRecord,
};
use crate::{Build, HOME_ENV};

/// The variable that names a folder holding both built binaries, for a test
/// run that must not start a build. See [`Programs::built`].
pub const BIN_DIR_ENV: &str = "EXAMPLE_CLI_BIN_DIR";

/// How often the harness looks again while it waits for a named condition.
const POLL: Duration = Duration::from_millis(5);

/// Why the harness could not do what a test asked.
#[derive(Debug)]
pub enum StandError {
    /// A file or folder of the stand could not be used.
    Io {
        /// What the harness was doing.
        doing: &'static str,
        /// The error it met.
        source: io::Error,
    },
    /// The program could not be built or found.
    Build(String),
    /// A condition the test waited for did not come about in time.
    TimedOut(&'static str),
}

impl fmt::Display for StandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { doing, source } => write!(f, "the stand failed while {doing}: {source}"),
            Self::Build(why) => write!(f, "the stand's program is not available: {why}"),
            Self::TimedOut(what) => write!(f, "the stand waited too long for {what}"),
        }
    }
}

impl std::error::Error for StandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Build(_) | Self::TimedOut(_) => None,
        }
    }
}

fn io_error(doing: &'static str) -> impl FnOnce(io::Error) -> StandError {
    move |source| StandError::Io { doing, source }
}

/// Where the stand's two built binaries are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Programs {
    first: PathBuf,
    rebuilt: PathBuf,
}

impl Programs {
    /// Two binaries the caller already has. This crate's own tests pass
    /// `env!("CARGO_BIN_EXE_example-cli")` and
    /// `env!("CARGO_BIN_EXE_example-cli-rebuilt")`.
    #[must_use]
    pub fn at(first: impl Into<PathBuf>, rebuilt: impl Into<PathBuf>) -> Self {
        Self {
            first: first.into(),
            rebuilt: rebuilt.into(),
        }
    }

    /// The binaries, for a test in any crate: found, or built if they are not
    /// there or are out of date. The answer is kept for the rest of the process.
    ///
    /// If the variable `EXAMPLE_CLI_BIN_DIR` names a folder, the two binaries
    /// are taken from it and nothing is built. Otherwise this runs
    /// `cargo build --package example-cli --bins`, with the cargo that is
    /// running the tests, and reads the paths from what cargo reports. That
    /// is quick when nothing changed, and it names no file suffix, so it is
    /// the same on every system.
    ///
    /// # Errors
    ///
    /// Returns an error if the folder lacks a binary, or the build fails.
    pub fn built() -> Result<Self, StandError> {
        static BUILT: OnceLock<Result<Programs, String>> = OnceLock::new();
        BUILT
            .get_or_init(|| match std::env::var_os(BIN_DIR_ENV) {
                Some(dir) => Self::in_folder(Path::new(&dir)),
                None => Self::build_with_cargo(),
            })
            .clone()
            .map_err(StandError::Build)
    }

    /// The path of one build's binary.
    #[must_use]
    pub fn path(&self, build: Build) -> &Path {
        match build {
            Build::First => &self.first,
            Build::Rebuilt => &self.rebuilt,
        }
    }

    fn in_folder(dir: &Path) -> Result<Self, String> {
        let find = |build: Build| {
            let path = dir.join(file_name(build));
            if path.is_file() {
                Ok(path)
            } else {
                Err(format!("{} is not a file", path.display()))
            }
        };
        Ok(Self {
            first: find(Build::First)?,
            rebuilt: find(Build::Rebuilt)?,
        })
    }

    fn build_with_cargo() -> Result<Self, String> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .args(["build", "--package", "example-cli", "--bins"])
            .arg("--message-format=json")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("cargo could not be started: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "cargo build failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let mut found = [None, None];
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Ok(message) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if message["reason"] != "compiler-artifact" {
                continue;
            }
            let Some(executable) = message["executable"].as_str() else {
                continue;
            };
            for (slot, build) in found.iter_mut().zip([Build::First, Build::Rebuilt]) {
                if message["target"]["name"] == build.binary() {
                    *slot = Some(PathBuf::from(executable));
                }
            }
        }
        match found {
            [Some(first), Some(rebuilt)] => Ok(Self { first, rebuilt }),
            _ => Err("cargo did not report both binaries".to_owned()),
        }
    }
}

/// The file name of a build's binary on this system.
fn file_name(build: Build) -> String {
    format!("{}{}", build.binary(), std::env::consts::EXE_SUFFIX)
}

/// What a finished start of the program left behind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    /// The exit status, or `None` if the program was ended by a signal.
    pub code: Option<i32>,
    /// Everything it printed on standard output.
    pub stdout: String,
    /// Everything it printed on standard error.
    pub stderr: String,
}

impl From<Output> for Finished {
    fn from(output: Output) -> Self {
        Self {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

/// One stand: a program of its own, a fresh home folder with the seed data,
/// and a control folder. Removed from disk when dropped.
///
/// ```text
/// <root>/example-cli            the program (with the system's suffix)
/// <root>/example-cli-control/   the control folder, found by the program beside itself
/// <root>/home/                  the home folder
/// ```
#[derive(Debug)]
pub struct Stand {
    root: PathBuf,
    program: PathBuf,
    programs: Programs,
    control: Control,
}

impl Stand {
    /// Starts a stand with the binaries [`Programs::built`] finds or builds.
    /// This is the one call a test in another crate makes.
    ///
    /// # Errors
    ///
    /// Returns an error if the program is not available or the folders cannot be made.
    pub fn start() -> Result<Self, StandError> {
        Self::start_with(Programs::built()?)
    }

    /// Starts a stand with binaries the caller names, in a folder of its own
    /// under the system's temporary folder, on every system. The engine
    /// refuses a program that another user could replace, and the folder a
    /// build writes to need not be closed to other users: on a Windows
    /// machine whose checkout is on a drive other than the system's, every
    /// signed-in user may change it, as on a hosted CI runner. The user's
    /// temporary folder is closed to them on Windows and macOS, and on Linux
    /// it is a folder with the sticky mark, where only an entry's owner may
    /// replace it.
    ///
    /// # Errors
    ///
    /// Returns an error if the folders cannot be made.
    pub fn start_with(programs: Programs) -> Result<Self, StandError> {
        Self::start_in(programs, &std::env::temp_dir().join("example-cli-stands"))
    }

    /// Starts a stand in a new folder inside `parent`.
    ///
    /// # Errors
    ///
    /// Returns an error if the folders cannot be made.
    pub fn start_in(programs: Programs, parent: &Path) -> Result<Self, StandError> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        fs::create_dir_all(parent).map_err(io_error("making the stands folder"))?;
        let root = loop {
            let number = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = parent.join(format!("{}-{number}", std::process::id()));
            match fs::create_dir(&root) {
                Ok(()) => break root,
                // Left behind by an earlier process with the same number.
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(io_error("making the stand's folder")(e)),
            }
        };
        let stand = Self {
            program: root.join(file_name(Build::First)),
            control: Control::at(&root.join(CONTROL_FOLDER), &root.join("home")),
            programs,
            root,
        };
        place(stand.programs.path(Build::First), &stand.program)?;
        stand.control.reset()?;
        Ok(stand)
    }

    /// The path of this stand's program. It is the stand's own: no other stand
    /// starts the same file path.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The home folder the stand made. [`Stand::command`] passes it to the program.
    #[must_use]
    pub fn home(&self) -> &Path {
        self.control.home()
    }

    /// The stand's own folder. A test may make more folders in it.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The control interface.
    #[must_use]
    pub fn control(&self) -> &Control {
        &self.control
    }

    /// A start of the program, not yet begun, for a test that acts as a user:
    /// these arguments, an environment that holds the home folder and nothing
    /// else, and the stand's folder as the working folder. The caller sets the
    /// three streams.
    #[must_use]
    pub fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .env_clear()
            .env(HOME_ENV, self.home())
            .current_dir(&self.root);
        // A process on Windows cannot load its system libraries without it.
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        command
    }

    /// Runs the program to its end with standard input closed.
    ///
    /// # Errors
    ///
    /// Returns an error if the program cannot be started.
    pub fn run(&self, args: &[&str]) -> Result<Finished, StandError> {
        self.command(args)
            .stdin(Stdio::null())
            .output()
            .map(Finished::from)
            .map_err(io_error("running the program"))
    }

    /// Runs the program to its end, with `input` on its standard input.
    ///
    /// # Errors
    ///
    /// Returns an error if the program cannot be started.
    pub fn run_with_input(&self, args: &[&str], input: &str) -> Result<Finished, StandError> {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(io_error("starting the program"))?;
        if let Some(mut stdin) = child.stdin.take() {
            // A program that ends without reading has closed the pipe. That is its right.
            let _ = stdin.write_all(input.as_bytes());
        }
        child
            .wait_with_output()
            .map(Finished::from)
            .map_err(io_error("running the program"))
    }

    /// Puts another build at the program's path. Do it while no start is running.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be replaced.
    pub fn replace_program(&self, build: Build) -> Result<(), StandError> {
        fs::remove_file(&self.program).map_err(io_error("removing the program"))?;
        place(self.programs.path(build), &self.program)
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        // Best effort. A child that still runs is told to end, and is given
        // a moment to, because a running program's file cannot be removed on
        // every system.
        let _ = self.control.stop_children();
        let _ = self.control.wait_until_no_children(Duration::from_secs(5));
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Puts a binary at a path. On Linux and macOS a link where the disk allows
/// it: it is quick, and a file that was never open for writing can always be
/// started at once. On Windows a copy: a link there shares one access list
/// with the file the build wrote, which may be open to other users, and a
/// copy takes the access list of the stand's own folder.
fn place(binary: &Path, at: &Path) -> Result<(), StandError> {
    #[cfg(not(windows))]
    if fs::hard_link(binary, at).is_ok() {
        return Ok(());
    }
    fs::copy(binary, at)
        .map(|_| ())
        .map_err(io_error("placing the program"))
}

/// The control interface: resets the stand, switches faults on, and reports
/// ground truth by reading the program's files directly.
#[derive(Clone, Debug)]
pub struct Control {
    dir: ControlDir,
    home: PathBuf,
}

impl Control {
    /// The control interface over a control folder and a home folder. A stand
    /// makes its own; a test uses this when the program is given a home folder
    /// the stand did not make.
    #[must_use]
    pub fn at(control_folder: &Path, home: &Path) -> Self {
        Self {
            dir: ControlDir::new(control_folder.to_path_buf()),
            home: home.to_path_buf(),
        }
    }

    /// The same control folder, reporting on another home folder.
    #[must_use]
    pub fn for_home(&self, home: &Path) -> Self {
        Self {
            dir: self.dir.clone(),
            home: home.to_path_buf(),
        }
    }

    /// The home folder this control interface reports on.
    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The control folder. A program that is not beside it finds it through
    /// the `EXAMPLE_CLI_CONTROL` variable.
    #[must_use]
    pub fn folder(&self) -> &Path {
        self.dir.path()
    }

    /// Restores the seed items; forgets every write, start, and fault; signs
    /// the program out. Children of earlier starts are ended first.
    ///
    /// # Errors
    ///
    /// Returns an error if a file cannot be written or a child does not end.
    pub fn reset(&self) -> Result<(), StandError> {
        self.dir
            .clear()
            .map_err(io_error("clearing the control folder"))?;
        fs::create_dir_all(&self.home).map_err(io_error("making the home folder"))?;
        Store::seeded()
            .save(&self.home)
            .map_err(io_error("writing the seed data"))?;
        self.expire_session()
    }

    fn store(&self) -> Result<Store, StandError> {
        Store::load(&self.home).map_err(io_error("reading the data file"))
    }

    /// Exactly what the data file holds for an item, if anything.
    ///
    /// # Errors
    ///
    /// Returns an error if the data file cannot be read.
    pub fn stored_item(&self, sku: &str) -> Result<Option<StoredItem>, StandError> {
        Ok(self.store()?.items.remove(sku))
    }

    /// Every item the data file holds, by item code.
    ///
    /// # Errors
    ///
    /// Returns an error if the data file cannot be read.
    pub fn stored_items(&self) -> Result<Vec<StoredItem>, StandError> {
        Ok(self.store()?.items.into_values().collect())
    }

    /// Every committed write since the last reset, oldest first: which start
    /// of the program changed which item.
    ///
    /// # Errors
    ///
    /// Returns an error if the data file cannot be read.
    pub fn writes(&self) -> Result<Vec<WriteRecord>, StandError> {
        Ok(self.store()?.writes)
    }

    /// How many times one item has been written since the last reset.
    ///
    /// # Errors
    ///
    /// Returns an error if the data file cannot be read.
    pub fn write_count(&self, sku: &str) -> Result<usize, StandError> {
        Ok(self.writes()?.iter().filter(|w| w.sku == sku).count())
    }

    /// Every committed catalog push since the last reset, oldest first.
    ///
    /// # Errors
    ///
    /// Returns an error if the data file cannot be read.
    pub fn pushes(&self) -> Result<Vec<PushRecord>, StandError> {
        Ok(self.store()?.pushes)
    }

    /// Whether a write got as far as its temporary file and has not committed:
    /// it is under way, or it was killed part-way.
    #[must_use]
    pub fn uncommitted_write(&self) -> bool {
        self.home.join(DATA_TEMP_FILE).exists()
    }

    /// Every start the program received since the last reset, oldest first,
    /// with its arguments in order, its environment, and how it ended.
    ///
    /// # Errors
    ///
    /// Returns an error if the control folder cannot be read.
    pub fn invocations(&self) -> Result<Vec<Invocation>, StandError> {
        self.dir
            .invocations()
            .map_err(io_error("reading the invocation log"))
    }

    /// Switches a fault on.
    ///
    /// # Errors
    ///
    /// Returns an error if the faults file cannot be written.
    pub fn set_fault(&self, fault: &Fault) -> Result<(), StandError> {
        self.dir
            .change_faults(|file| file.faults.push(fault.clone()))
            .map_err(io_error("writing the faults"))
    }

    /// The faults that are switched on now. A fault that was used up is gone.
    ///
    /// # Errors
    ///
    /// Returns an error if the faults file cannot be read.
    pub fn faults(&self) -> Result<Vec<Fault>, StandError> {
        self.dir
            .faults()
            .map(|file| file.faults)
            .map_err(io_error("reading the faults"))
    }

    /// Switches every fault off.
    ///
    /// # Errors
    ///
    /// Returns an error if the faults file cannot be written.
    pub fn clear_faults(&self) -> Result<(), StandError> {
        self.dir
            .change_faults(|file| file.faults.clear())
            .map_err(io_error("writing the faults"))
    }

    /// Waits until `count` starts have reached a hang since the last reset,
    /// and returns their numbers. A start that hangs stays in this list after
    /// it is killed.
    ///
    /// # Errors
    ///
    /// Returns an error if that does not happen within `within`.
    pub fn wait_until_hung(&self, count: usize, within: Duration) -> Result<Vec<u64>, StandError> {
        let deadline = Instant::now() + within;
        loop {
            let hung = self
                .dir
                .hung_starts()
                .map_err(io_error("reading the control folder"))?;
            if hung.len() >= count {
                return Ok(hung);
            }
            if Instant::now() > deadline {
                return Err(StandError::TimedOut("the program to hang"));
            }
            thread::sleep(POLL);
        }
    }

    /// The starts whose child process is alive now. The operating system
    /// answers this, through a lock each child holds for as long as it lives.
    ///
    /// # Errors
    ///
    /// Returns an error if the control folder cannot be read.
    pub fn living_children(&self) -> Result<Vec<u64>, StandError> {
        self.dir
            .living_children()
            .map_err(io_error("reading the control folder"))
    }

    /// Tells every child process to end. Until the next reset, new children
    /// end at once.
    ///
    /// # Errors
    ///
    /// Returns an error if the control folder cannot be written.
    pub fn stop_children(&self) -> Result<(), StandError> {
        self.dir
            .stop_children()
            .map_err(io_error("writing the control folder"))
    }

    /// Waits until no child process is alive.
    ///
    /// # Errors
    ///
    /// Returns an error if one is still alive after `within`.
    pub fn wait_until_no_children(&self, within: Duration) -> Result<(), StandError> {
        let deadline = Instant::now() + within;
        while !self.living_children()?.is_empty() {
            if Instant::now() > deadline {
                return Err(StandError::TimedOut("the program's children to end"));
            }
            thread::sleep(POLL);
        }
        Ok(())
    }

    /// Takes the home folder's write lock, as a second start of the program
    /// that is writing would. While the returned lock lives, every write
    /// answers that the program is busy.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock is already held or cannot be taken.
    pub fn hold_write_lock(&self) -> Result<HomeLock, StandError> {
        fs::create_dir_all(&self.home).map_err(io_error("making the home folder"))?;
        HomeLock::try_take(&self.home)
            .map_err(io_error("taking the write lock"))?
            .ok_or(StandError::TimedOut("the write lock, which is held"))
    }

    /// Whether the program has a kept session in the home folder.
    #[must_use]
    pub fn signed_in(&self) -> bool {
        self.home.join(SESSION_FILE).exists()
    }

    /// Ends the program's kept session, as time or the other side would.
    ///
    /// # Errors
    ///
    /// Returns an error if the session file cannot be removed.
    pub fn expire_session(&self) -> Result<(), StandError> {
        match fs::remove_file(self.home.join(SESSION_FILE)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_error("ending the session")(e)),
            _ => Ok(()),
        }
    }
}
