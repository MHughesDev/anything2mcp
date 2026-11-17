//! The control folder: where the program records every start and reads its
//! faults, and where the harness reads the one and writes the other.
//!
//! The program finds the folder by the `EXAMPLE_CLI_CONTROL` variable, or else
//! as a folder named `example-cli-control` beside its own executable. The
//! second way works when the program is started with a bare environment. With
//! neither, the program records nothing and has no faults.
//!
//! | File | Holds |
//! | --- | --- |
//! | `lock` | Locked by whoever is reading or changing the folder |
//! | `faults.json` | The faults that are switched on |
//! | `starts/000001.json` | What start 1 received |
//! | `starts/000001.exit` | The exit status of start 1, if it ended by itself |
//! | `starts/000001.hung` | Present while start 1 waits to be killed |
//! | `starts/000001.child` | Locked by the child of start 1 while it lives |
//! | `stop-children` | Present to tell every child to end |

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::faults::{FaultFile, FaultKind};

/// The name of the control folder beside the program.
pub const CONTROL_FOLDER: &str = "example-cli-control";

const LOCK_FILE: &str = "lock";
const FAULTS_FILE: &str = "faults.json";
const STARTS_FOLDER: &str = "starts";
const STOP_CHILDREN_FILE: &str = "stop-children";

/// How often a child looks for the file that tells it to end.
pub(crate) const CHILD_POLL: Duration = Duration::from_millis(10);
/// How long a reset waits for children to end before it gives up.
const CHILDREN_END_WITHIN: Duration = Duration::from_secs(10);

/// One start of the program, as the program itself recorded it before it did
/// anything else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invocation {
    /// Starts are numbered from one, in the order they began.
    pub start: u64,
    /// Which build of the program ran: `first` or `rebuilt`.
    pub build: String,
    /// The command the program took the arguments to mean, such as `item set`.
    pub command: String,
    /// Every argument after the program's own name, in order, as received.
    pub args: Vec<String>,
    /// The whole environment the program was started with.
    pub env: BTreeMap<String, String>,
    /// The folder the program was started in.
    pub cwd: String,
    /// The home folder the arguments or the environment named, if any.
    pub home: Option<String>,
    /// The exit status, if the program ended by itself. `None` while it runs,
    /// and for a start that was killed or ended abnormally.
    #[serde(default, skip_serializing)]
    pub exit: Option<u8>,
    /// Whether the start is waiting to be killed, or was killed while waiting.
    #[serde(default, skip_serializing)]
    pub hung: bool,
}

/// One control folder. Both the program and the harness go through this type,
/// so the layout is written down once.
#[derive(Clone, Debug)]
pub(crate) struct ControlDir {
    path: PathBuf,
}

impl ControlDir {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Makes the folder ready for use. Does nothing if it already is.
    pub(crate) fn create(&self) -> io::Result<()> {
        fs::create_dir_all(self.path.join(STARTS_FOLDER))
    }

    /// Waits for the folder's lock. It is released when the file is dropped.
    fn lock(&self) -> io::Result<File> {
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.path.join(LOCK_FILE))?;
        file.lock()?;
        Ok(file)
    }

    fn start_file(&self, start: u64, extension: &str) -> PathBuf {
        self.path
            .join(STARTS_FOLDER)
            .join(format!("{start:06}.{extension}"))
    }

    /// The numbers of the starts that left a file with this extension, in order.
    fn starts_with(&self, extension: &str) -> io::Result<Vec<u64>> {
        let mut starts = Vec::new();
        for entry in fs::read_dir(self.path.join(STARTS_FOLDER))? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some(extension) {
                continue;
            }
            let number = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse().ok());
            if let Some(number) = number {
                starts.push(number);
            }
        }
        starts.sort_unstable();
        Ok(starts)
    }

    fn read_faults(&self) -> io::Result<FaultFile> {
        match fs::read(self.path.join(FAULTS_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(FaultFile::default()),
            Err(e) => Err(e),
        }
    }

    fn write_faults(&self, faults: &FaultFile) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(faults).map_err(io::Error::other)?;
        fs::write(self.path.join(FAULTS_FILE), bytes)
    }

    // What the program does.

    /// Records a start and takes the faults that apply to it. `record` is
    /// given the start's number and returns what to write down.
    pub(crate) fn begin(
        &self,
        command: &str,
        record: impl FnOnce(u64) -> Invocation,
    ) -> io::Result<(u64, Vec<FaultKind>)> {
        self.create()?;
        let _lock = self.lock()?;
        let start = self.starts_with("json")?.last().copied().unwrap_or(0) + 1;
        let bytes = serde_json::to_vec_pretty(&record(start)).map_err(io::Error::other)?;
        fs::write(self.start_file(start, "json"), bytes)?;
        let mut faults = self.read_faults()?;
        let taken = faults.take_for(command);
        if !taken.is_empty() {
            self.write_faults(&faults)?;
        }
        Ok((start, taken))
    }

    /// Records that a start ended by itself, with this status.
    pub(crate) fn end(&self, start: u64, code: u8) -> io::Result<()> {
        fs::write(self.start_file(start, "exit"), code.to_string())
    }

    /// Records that a start is now waiting to be killed.
    pub(crate) fn mark_hung(&self, start: u64) -> io::Result<()> {
        fs::write(self.start_file(start, "hung"), b"")
    }

    /// What a child process does for as long as it lives: holds a lock on its
    /// file, and says so once it has it.
    pub(crate) fn hold_child_file(&self, start: u64) -> io::Result<File> {
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.start_file(start, "child"))?;
        file.lock()?;
        fs::write(self.start_file(start, "ready"), b"")?;
        Ok(file)
    }

    /// Whether the child of a start has said it is alive.
    pub(crate) fn child_is_ready(&self, start: u64) -> bool {
        self.start_file(start, "ready").exists()
    }

    /// Whether children have been told to end.
    pub(crate) fn children_must_stop(&self) -> bool {
        self.path.join(STOP_CHILDREN_FILE).exists()
    }

    // What the harness does.

    /// Every start so far, oldest first.
    pub(crate) fn invocations(&self) -> io::Result<Vec<Invocation>> {
        let _lock = self.lock()?;
        let mut invocations = Vec::new();
        for start in self.starts_with("json")? {
            let bytes = fs::read(self.start_file(start, "json"))?;
            let mut invocation: Invocation =
                serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            invocation.exit = fs::read_to_string(self.start_file(start, "exit"))
                .ok()
                .and_then(|text| text.trim().parse().ok());
            invocation.hung = self.start_file(start, "hung").exists();
            invocations.push(invocation);
        }
        Ok(invocations)
    }

    /// The starts that are waiting to be killed, or were killed while waiting.
    pub(crate) fn hung_starts(&self) -> io::Result<Vec<u64>> {
        self.starts_with("hung")
    }

    /// The starts whose child process is alive now.
    pub(crate) fn living_children(&self) -> io::Result<Vec<u64>> {
        let mut living = Vec::new();
        for start in self.starts_with("child")? {
            let file = File::options()
                .write(true)
                .open(self.start_file(start, "child"))?;
            match file.try_lock() {
                Ok(()) => {}
                Err(fs::TryLockError::WouldBlock) => living.push(start),
                Err(fs::TryLockError::Error(e)) => return Err(e),
            }
        }
        Ok(living)
    }

    /// Tells every child to end. They notice within a moment.
    pub(crate) fn stop_children(&self) -> io::Result<()> {
        fs::write(self.path.join(STOP_CHILDREN_FILE), b"")
    }

    pub(crate) fn faults(&self) -> io::Result<FaultFile> {
        let _lock = self.lock()?;
        self.read_faults()
    }

    pub(crate) fn change_faults(&self, change: impl FnOnce(&mut FaultFile)) -> io::Result<()> {
        let _lock = self.lock()?;
        let mut faults = self.read_faults()?;
        change(&mut faults);
        self.write_faults(&faults)
    }

    /// Forgets every start and every fault. Children are told to end first,
    /// and the call waits until they have.
    pub(crate) fn clear(&self) -> io::Result<()> {
        self.create()?;
        self.stop_children()?;
        let deadline = Instant::now() + CHILDREN_END_WITHIN;
        while !self.living_children()?.is_empty() {
            if Instant::now() > deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "a child of the program did not end when told to",
                ));
            }
            thread::sleep(CHILD_POLL);
        }
        let _lock = self.lock()?;
        for entry in fs::read_dir(self.path.join(STARTS_FOLDER))? {
            fs::remove_file(entry?.path())?;
        }
        self.write_faults(&FaultFile::default())?;
        fs::remove_file(self.path.join(STOP_CHILDREN_FILE))
    }
}
