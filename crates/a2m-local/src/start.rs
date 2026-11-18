//! Start on demand: what a thin client does when no engine answers.
//!
//! The client takes the start lock, looks again, and starts the engine
//! detached from its own terminal and process group: on macOS and Linux in
//! a session of its own, on Windows with no console and in a process group
//! of its own. The engine's standard output and error go to its log, its
//! standard input is empty or carries the vault's passphrase once, and it
//! inherits no other handle of the client, because every handle this
//! program opens is closed on exec or not inheritable. The client then
//! waits for the channel to answer, up to a deadline, and connects.
//!
//! Two clients that start at once end with one engine: the start lock
//! lets one start it while the other waits and then connects; and if two
//! engines are started all the same, the engine lock lets one bind and the
//! other ends. Nothing here decides whether an engine may be started on
//! demand, which program it is, or with which environment: the caller
//! hands a [`Launch`].

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use a2m_actions::Paths;
use a2m_core::{Failure, Secret};

use crate::codes;
use crate::connector::{Connector, Link, shown};
use crate::lock::EngineLock;
use crate::os;

/// The name of the start lock's file in the runtime directory.
pub const START_LOCK_NAME: &str = "start.lock";

/// The name of the file in the log directory that an engine started on
/// demand writes its standard output and error to. It is not the
/// engine's own redacted log, `engine.log`, which telemetry writes and
/// rotates: what a process writes to its standard error is not redacted,
/// and a file renamed under an open handle would take the output with it
/// (decision 0181).
pub const ENGINE_LOG_NAME: &str = "engine-output.log";

/// How the line begins that a client appends to the engine's log when it
/// has started an engine on demand; the process's number follows.
pub const STARTED_LINE: &str = "anything2mcp: a client started the engine on demand as process";

/// How the line begins that an engine started on demand writes to its
/// output when it stops before it serves: its process's number and the
/// failure, as JSON, follow on the same line. The client that started it
/// reads the line back and answers with that failure, so that an engine
/// that cannot open its vault ends, and says why, instead of lingering
/// (decision 0220). [`stopped_line`] makes it.
pub const STOPPED_LINE: &str = "anything2mcp: the engine stopped before it served:";

/// The most of the end of the engine's output a client reads to find the
/// line [`stopped_line`] made.
const STOPPED_TAIL: u64 = 64 * 1024;

/// The longest sentence or next step a client takes from a stopped line.
const STOPPED_LONGEST: usize = 4096;

/// The pause between two looks at the channel while an engine starts.
/// The client waits on the channel answering; the operating system offers
/// no handle to wait on for a socket or a pipe that does not exist yet.
const PAUSE: Duration = Duration::from_millis(25);

/// How an engine is to be started: the program, its arguments, the folder
/// it runs in, the whole environment it starts with, and what its standard
/// input carries.
pub struct Launch {
    /// The program: this program, as the operating system names it.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// The folder it runs in.
    pub cwd: PathBuf,
    /// Its whole environment. Nothing of the client's own is passed on
    /// beside it.
    pub environment: BTreeMap<String, String>,
    /// The file its standard output and error are appended to.
    pub log: PathBuf,
    /// What its standard input carries, once, before it is closed: the
    /// vault's passphrase, when the engine takes its key from one. `None`
    /// is empty input.
    pub input: Option<Secret>,
}

impl std::fmt::Debug for Launch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launch")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("log", &self.log)
            .field("input", &self.input.is_some())
            .finish_non_exhaustive()
    }
}

impl Launch {
    /// The log of the engine of `paths`.
    #[must_use]
    pub fn log_of(paths: &Paths) -> PathBuf {
        paths.logs.join(ENGINE_LOG_NAME)
    }
}

/// What [`Connector::connect_or_start`] found or did.
#[derive(Debug)]
pub enum Started {
    /// An engine answered without this client starting one: it ran
    /// already, or another client started it meanwhile.
    Found(Link),
    /// This client started the engine, and it answers.
    Launched(Link),
    /// The operating system refused to start a process. The caller may
    /// run the engine inside itself instead; the failure says why.
    Refused(Failure),
}

/// The start lock, held by a client only while it starts the engine.
#[derive(Debug)]
struct StartLock {
    _file: File,
}

impl StartLock {
    /// Takes the start lock without waiting. `None` when another client
    /// holds it.
    fn try_take(paths: &Paths) -> Result<Option<Self>, Failure> {
        os::prepare_dir(&paths.runtime)
            .map_err(|error| codes::unavailable("The runtime directory", &error))?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(paths.runtime.join(START_LOCK_NAME))
            .map_err(|error| codes::unavailable("The start lock's file", &error))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => {
                Err(codes::unavailable("The start lock's file", &error))
            }
        }
    }
}

fn unreachable(failure: &Failure) -> bool {
    *failure.code() == codes::ENGINE_UNREACHABLE
}

/// Whether a failure to connect means that no engine answers yet: nothing
/// listens, or an engine that was starting closed the connection before
/// it answered its hello. Either way nothing was sent, and the client goes
/// on looking until its patience ends (decision 0181).
fn not_answering(failure: &Failure) -> bool {
    unreachable(failure) || *failure.code() == codes::CONNECTION_LOST
}

/// The engine did not answer within the deadline.
fn too_long(log: &Path) -> Failure {
    codes::busy(
        codes::ENGINE_UNREACHABLE,
        "An engine was started on demand, or is starting or closing for this data directory, and did not answer on the local channel in time.",
        format!(
            "Run the command again. If it stays so, read the engine's log, `{}`, or start the engine with `anything2mcp serve` and read what it says.",
            shown(log)
        ),
    )
}

/// The line an engine started on demand writes to its standard error,
/// which is its output file, when it stops before it serves: the class,
/// code, sentence, and next step of `failure`, and never what a system
/// said, which a failure keeps apart. The engine's own words hold no
/// secret, by the rule every failure keeps.
#[must_use]
pub fn stopped_line(failure: &Failure) -> String {
    let bare = Failure::new(
        failure.class(),
        failure.code().clone(),
        failure.message(),
        failure.next_step(),
    );
    // A failure is plain fields, which always serialize.
    let json = serde_json::to_string(&bare).unwrap_or_default();
    format!("{STOPPED_LINE} {} {json}", std::process::id())
}

/// The failure that the engine of process `process` wrote, with
/// [`stopped_line`], into the end of `log`. `None` when it wrote none, or
/// one that does not read as a failure an engine stops with.
fn stopped_in(log: &Path, process: u32) -> Option<Failure> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = File::open(log).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(STOPPED_TAIL)))
        .ok()?;
    let mut tail = Vec::new();
    file.take(STOPPED_TAIL).read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);
    let ours = format!("{STOPPED_LINE} {process} ");
    let json = tail
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(ours.as_str()))?;
    let found: Failure = serde_json::from_str(json).ok()?;
    // An engine that has not served has sent nothing anywhere: a line that
    // says otherwise, or is too long to be the engine's own, is not taken.
    let plausible = found.class() != a2m_core::FailureClass::PossibleCommit
        && found.message().len() <= STOPPED_LONGEST
        && found.next_step().len() <= STOPPED_LONGEST;
    plausible.then_some(found)
}

/// The engine this client started, process `process`, ended before it
/// answered: with the failure it wrote as it stopped, when it wrote one.
fn ended_early(log: &Path, process: Option<u32>) -> Failure {
    if let Some(stopped) = process.and_then(|process| stopped_in(log, process)) {
        return Failure::new(
            stopped.class(),
            stopped.code().clone(),
            format!(
                "The engine started on demand stopped before it answered: {}",
                stopped.message()
            ),
            format!(
                "{} The engine's log is `{}`.",
                stopped.next_step(),
                shown(log)
            ),
        );
    }
    codes::busy(
        codes::ENGINE_UNREACHABLE,
        "The engine started on demand ended before it answered on the local channel.",
        format!(
            "Read the engine's log, `{}`, for why; or start the engine with `anything2mcp serve` and read what it says.",
            shown(log)
        ),
    )
}

/// Opens the engine's output file for appending, closed to everyone but
/// the user on macOS and Linux. On those systems a link at its name, a
/// file that is not this user's, and a file swapped in while it was
/// opened are refused, and a file others may read is narrowed to the user
/// (decision 0181). On Windows a link at its name and a file another owns
/// are refused, and the file is given an access list that names only this
/// user (decision 0192).
fn open_log(log: &Path) -> std::io::Result<File> {
    if let Some(folder) = log.parent() {
        std::fs::create_dir_all(folder)?;
    }
    // On Windows: never through a link, only a file of this user's, and
    // with an access list of its own that names only this user.
    #[cfg(windows)]
    {
        os::open_output(log)
    }
    #[cfg(not(windows))]
    {
        open_output_elsewhere(log)
    }
}

#[cfg(not(windows))]
fn open_output_elsewhere(log: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        if std::fs::symlink_metadata(log).is_ok_and(|found| !found.is_file()) {
            return Err(not_a_plain_file());
        }
    }
    let file = options.open(log)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let opened = file.metadata()?;
        let named = std::fs::symlink_metadata(log)?;
        let ours = rustix::process::getuid().as_raw();
        if !named.is_file()
            || (opened.dev(), opened.ino()) != (named.dev(), named.ino())
            || opened.uid() != ours
        {
            return Err(not_a_plain_file());
        }
        if opened.mode() & 0o077 != 0 {
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(file)
}

#[cfg(unix)]
fn not_a_plain_file() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "the engine's output file is a link, another's, or not a plain file",
    )
}

/// Starts the engine detached, with its standard streams set. A failure is
/// the operating system's refusal to start a process.
fn launch(start: &Launch) -> std::io::Result<Child> {
    let mut log = open_log(&start.log)?;
    let mut command = Command::new(&start.program);
    command
        .args(&start.args)
        .current_dir(&start.cwd)
        .env_clear()
        .envs(&start.environment)
        .stdin(if start.input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?);
    let mut child = os::spawn_detached(&mut command)?;
    // One line, appended at once, so that a person who reads the log knows
    // which process wrote what follows.
    let _ = writeln!(log, "{STARTED_LINE} {}.", child.id());
    if let Some(input) = &start.input
        && let Some(mut stdin) = child.stdin.take()
    {
        // Small enough for the pipe's buffer, so the write does not wait
        // for the engine to read. The pipe is closed when `stdin` is
        // dropped, which ends the engine's input. An engine that cannot
        // read it opens without its key, and says so.
        let _ = stdin.write_all(input.expose().as_bytes());
    }
    Ok(child)
}

/// Waits for a child the client started, off the async threads, so that a
/// client that lives on, such as the bridge, leaves no ended process
/// behind it.
fn reap(mut child: Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

async fn engine_lock_held(paths: &Paths) -> Result<bool, Failure> {
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || EngineLock::held(&paths))
        .await
        .map_err(|_| {
            codes::unavailable(
                "The engine's lock file",
                &std::io::Error::other("the look stopped"),
            )
        })?
}

/// Starts the engine on a blocking thread, which owns the `Launch`:
/// making the output file, starting the process, and writing its input
/// are calls that wait on the operating system, and none of them runs on
/// an async thread.
async fn launch_off_thread(start: &Arc<Launch>) -> std::io::Result<Child> {
    let start = Arc::clone(start);
    tokio::task::spawn_blocking(move || launch(&start))
        .await
        .unwrap_or_else(|_| Err(std::io::Error::other("the start stopped")))
}

async fn take_start_lock(paths: &Paths) -> Result<Option<StartLock>, Failure> {
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || StartLock::try_take(&paths))
        .await
        .map_err(|_| {
            codes::unavailable(
                "The start lock's file",
                &std::io::Error::other("the look stopped"),
            )
        })?
}

impl Connector {
    /// Connects to the engine of `paths`, and when none answers starts one
    /// as `start` says and connects to it once it answers.
    ///
    /// While another client holds the start lock, this one waits and looks
    /// at the channel. With the start lock taken it looks again; an engine
    /// lock held with nothing answering is an engine starting or closing,
    /// or a command that runs the engine inside itself, and is waited for.
    /// Then it starts the engine and waits for the channel to answer. All
    /// of it ends at `patience`.
    ///
    /// # Errors
    ///
    /// What [`Connector::connect`] answers for an engine that cannot be
    /// used; `busy`/`engine_unreachable` when nothing answered in time, or
    /// the engine started ended before it answered, with the log to read;
    /// `busy`/`channel_unavailable` when a lock file cannot be opened.
    pub async fn connect_or_start(
        &self,
        paths: &Paths,
        start: Launch,
        patience: Duration,
    ) -> Result<Started, Failure> {
        // The launch is owned by the blocking task that starts the engine;
        // the log's path is kept here for what the client is told.
        let start = Arc::new(start);
        let give_up = tokio::time::Instant::now() + patience;
        let late = || tokio::time::Instant::now() >= give_up;
        // A look at the channel ends at `give_up` too: an engine that has
        // bound its channel and stalls while it opens is waited for no
        // longer than one that never binds.
        let look = || tokio::time::timeout_at(give_up, self.connect());
        let _start_lock = loop {
            match look().await {
                Err(_) => return Err(too_long(&start.log)),
                Ok(Ok(link)) => return Ok(Started::Found(link)),
                Ok(Err(failure)) if not_answering(&failure) => {}
                Ok(Err(failure)) => return Err(failure),
            }
            if let Some(lock) = take_start_lock(paths).await? {
                break lock;
            }
            if late() {
                return Err(too_long(&start.log));
            }
            tokio::time::sleep(PAUSE).await;
        };
        let mut child: Option<Child> = None;
        // Whether an engine this client started has ended already, and its
        // process's number.
        let mut ours_ended = false;
        let mut ended_process = None;
        loop {
            match look().await {
                Err(_) => {
                    if let Some(child) = child {
                        reap(child);
                    }
                    return Err(too_long(&start.log));
                }
                Ok(Ok(link)) => {
                    return Ok(match child {
                        Some(child) => {
                            reap(child);
                            Started::Launched(link)
                        }
                        None => Started::Found(link),
                    });
                }
                Ok(Err(failure)) if not_answering(&failure) => {}
                Ok(Err(failure)) => {
                    if let Some(child) = child {
                        reap(child);
                    }
                    return Err(failure);
                }
            }
            if let Some(started) = child.as_mut()
                && started.try_wait().ok().flatten().is_some()
            {
                // It ended: it failed, or lost the engine lock to an engine
                // that is starting or closing, which is then waited for.
                ended_process = Some(started.id());
                child = None;
                ours_ended = true;
            }
            // The engine lock is asked about only when no engine of this
            // client's is starting, so that the question never takes the
            // lock from under it.
            if child.is_none() && !engine_lock_held(paths).await? {
                if ours_ended {
                    return Err(ended_early(&start.log, ended_process));
                }
                match launch_off_thread(&start).await {
                    Ok(started) => child = Some(started),
                    Err(error) => {
                        return Ok(Started::Refused(codes::unavailable(
                            "A detached engine process",
                            &error,
                        )));
                    }
                }
            }
            if late() {
                if let Some(child) = child {
                    reap(child);
                }
                return Err(too_long(&start.log));
            }
            tokio::time::sleep(PAUSE).await;
        }
    }
}
