//! The supervisor: the only way anything starts a process.
//!
//! A program is named by its path, digested before it starts, given only
//! the environment its caller lists, placed in a group the supervisor can
//! end as one, bounded in time and output, and never left behind. A pack
//! reaches it only through a [`ScopedSupervisor`], which starts nothing the
//! pack's manifest, or the published plan, does not declare.
//!
//! Two kinds of link are built: [`OneShot`], a program run to completion
//! with its output captured, and a foreign link ([`ForeignPipe`]), a
//! program that speaks its own protocol over a pair of pipes. The framed
//! link of a plug-in, with its launch handshake, is not built yet.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_core::{Digest, Failure, FailureClass, SubsystemState};
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Command;

use crate::codes;
use crate::name::{ProviderName, SubsystemKind};
use crate::path::CheckedPath;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as os;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as os;

/// The variable that carries the owner mark into a child's environment.
pub const OWNER_MARK_VARIABLE: &str = "ANYTHING2MCP_OWNER_MARK";

/// A program to start: where it is, and the digest it must have when one
/// was declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramRef {
    /// Its absolute path. Never a name to search for.
    pub path: PathBuf,
    /// The digest a manifest or a published plan declared for it.
    pub digest: Option<Digest>,
}

impl From<&a2m_core::DeclaredProgram> for ProgramRef {
    /// A program a published plan declares for a surface, as the supervisor
    /// takes it. The engine makes the list for [`DriverHandle::supervisor`]
    /// with `surface.programs.iter().map(ProgramRef::from).collect()`.
    ///
    /// [`DriverHandle::supervisor`]: crate::DriverHandle::supervisor
    fn from(program: &a2m_core::DeclaredProgram) -> Self {
        Self {
            path: PathBuf::from(&program.path),
            digest: program.digest,
        }
    }
}

/// The whole environment of a child. Nothing else of the engine's
/// environment reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvAllowlist {
    /// Names passed through from the engine's own environment, when set
    /// there.
    pub pass: Vec<String>,
    /// Variables set to a value the caller gives. Never a secret: a secret
    /// reaches a program through a lease, on its standard input.
    pub set: Vec<(String, OsString)>,
}

/// What to start.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    /// The program.
    pub program: ProgramRef,
    /// Its arguments, as a list. Never a shell line.
    pub args: Vec<OsString>,
    /// Its environment.
    pub env: EnvAllowlist,
    /// Its working folder. Without one it starts in the system's temporary
    /// folder, never in the engine's own.
    pub cwd: Option<CheckedPath>,
    /// What kind of subsystem it is; running subsystems are counted by
    /// kind.
    pub kind: SubsystemKind,
    /// The place it takes, such as a profile or a device. One subsystem of
    /// a kind holds a place at a time.
    pub place: Option<String>,
}

/// A program run to completion: no handshake, its exit status and bounded
/// output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneShot {
    /// The most bytes kept of each output stream. What comes after is
    /// read, counted, and dropped.
    pub max_output_bytes: u64,
    /// How long it may run before its whole group is ended.
    pub time_limit: Duration,
    /// What is written to its standard input, which is then closed. `None`
    /// gives it no standard input.
    pub stdin: Option<Vec<u8>>,
}

/// How the child of a foreign link receives its pipe (decision 0070).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignPipe {
    /// On Linux and macOS: the descriptor the child reads requests from.
    pub child_reads_fd: i32,
    /// On Linux and macOS: the descriptor the child writes replies to.
    pub child_writes_fd: i32,
    /// On Windows: one argument added after the others, in which `{read}`
    /// and `{write}` are replaced by the values of the two inherited
    /// handles.
    pub windows_argument: String,
}

/// The kinds of link between the engine and a subsystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// Run to completion.
    OneShot(OneShot),
    /// The program's own protocol over a pair of pipes, framed by its pack.
    Foreign(ForeignPipe),
}

/// How a program ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exit {
    /// It exited with this status.
    Code(i32),
    /// It was ended from outside, by a signal or by the supervisor.
    Killed,
}

/// What a one-shot run gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// How it ended.
    pub exit: Exit,
    /// Its standard output, up to the bound.
    pub stdout: Vec<u8>,
    /// Its standard error, up to the bound.
    pub stderr: Vec<u8>,
    /// How many bytes it wrote to standard output in all.
    pub stdout_bytes: u64,
    /// How many bytes it wrote to standard error in all.
    pub stderr_bytes: u64,
    /// The digest of the program that ran.
    pub program_digest: Digest,
    /// Whether that digest is one that was declared. An undeclared digest
    /// is not refused; the mark travels with what the run produced.
    pub verified: bool,
    /// What could not be cleaned up, if anything.
    pub cleanup: Cleanup,
}

impl Finished {
    /// Whether either stream wrote more than was kept.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.stdout_bytes > self.stdout.len() as u64 || self.stderr_bytes > self.stderr.len() as u64
    }
}

/// Identifies one running subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubsystemId(u64);

impl fmt::Display for SubsystemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sub_{}", self.0)
    }
}

/// A running program on a foreign link. Its pack frames the bytes.
pub struct ForeignLink {
    /// The subsystem, for stopping it.
    pub id: SubsystemId,
    /// What the engine writes and the child reads.
    pub to_child: Box<dyn AsyncWrite + Send + Unpin>,
    /// What the child writes and the engine reads. Its end is the end of
    /// the child.
    pub from_child: Box<dyn AsyncRead + Send + Unpin>,
    /// The digest of the program that runs.
    pub program_digest: Digest,
    /// Whether that digest is one that was declared.
    pub verified: bool,
}

impl fmt::Debug for ForeignLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForeignLink")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// One subsystem, as `doctor.run` and the listing show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsystemListing {
    /// Its identifier.
    pub id: SubsystemId,
    /// Its kind.
    pub kind: SubsystemKind,
    /// The place it holds.
    pub place: Option<String>,
    /// The provider it was started for; `None` for the kernel itself.
    pub for_whom: Option<ProviderName>,
    /// `one_shot` or `foreign`.
    pub link: &'static str,
    /// Its process identifier.
    pub process: u32,
    /// Whether it runs.
    pub state: SubsystemState,
}

/// Something a stop could not end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    /// The kind of subsystem it belonged to.
    pub kind: Option<SubsystemKind>,
    /// What remains, in a sentence.
    pub what: String,
}

/// How a stop ended. An incomplete cleanup is a state, not a detail: it
/// names what remains and blocks new starts of that kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleanup {
    /// Whether everything was ended.
    pub complete: bool,
    /// What remains.
    pub remaining: Vec<Leftover>,
}

impl Cleanup {
    /// A cleanup that left nothing.
    #[must_use]
    pub fn complete() -> Self {
        Self {
            complete: true,
            remaining: Vec::new(),
        }
    }

    /// A cleanup that left something.
    #[must_use]
    pub fn incomplete(remaining: Vec<Leftover>) -> Self {
        Self {
            complete: remaining.is_empty(),
            remaining,
        }
    }
}

/// Whether a request to a subsystem ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ran {
    /// The program did not start.
    NotStarted,
    /// It started, and what it did before it was ended is not known.
    Unknown,
}

/// The failure of a start or a run, with whether anything ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnFailure {
    /// What went wrong.
    pub failure: Failure,
    /// Whether the program ran.
    pub ran: Ran,
}

impl fmt::Display for ConnFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.failure.fmt(f)
    }
}

impl std::error::Error for ConnFailure {}

/// How many subsystems may run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorLimits {
    /// The most of one kind, for a kind not listed below.
    pub per_kind: usize,
    /// The most of each kind listed.
    pub kinds: BTreeMap<SubsystemKind, usize>,
}

impl Default for SupervisorLimits {
    fn default() -> Self {
        Self {
            per_kind: 16,
            kinds: BTreeMap::new(),
        }
    }
}

struct Entry {
    kind: SubsystemKind,
    place: Option<String>,
    for_whom: Option<ProviderName>,
    link: &'static str,
    group: Arc<os::Group>,
}

#[derive(Default)]
struct State {
    live: BTreeMap<u64, Entry>,
    stopping: bool,
    leftovers: Vec<Leftover>,
}

struct Inner {
    limits: SupervisorLimits,
    state: Mutex<State>,
    next: AtomicU64,
    shutdown: tokio::sync::Mutex<()>,
}

/// The supervisor. Cheap to clone; every clone is the same supervisor.
#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
}

impl fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Supervisor").finish_non_exhaustive()
    }
}

fn not_started(failure: Failure) -> ConnFailure {
    ConnFailure {
        failure,
        ran: Ran::NotStarted,
    }
}

/// The digest of a file, read in pieces.
pub(crate) fn digest_of(path: &Path) -> std::io::Result<Digest> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Digest::from_bytes(hasher.finalize().into()))
}

/// A fresh owner mark: 128 random bits, as text.
fn owner_mark() -> Result<String, Failure> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| {
        codes::busy(
            codes::START_FAILED,
            "The operating system gave no random bytes for an owner mark.",
            "Try again; if it persists, check the machine's random source.",
        )
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// What is kept of one output stream, and how many bytes it wrote in all.
type Kept = Arc<Mutex<(Vec<u8>, u64)>>;

/// Keeps at most `max` bytes of what was read and counts all of it. False
/// when the store is broken.
fn keep(into: &Kept, max: u64, bytes: &[u8]) -> bool {
    let Ok(mut kept) = into.lock() else {
        return false;
    };
    kept.1 += bytes.len() as u64;
    let room = usize::try_from(max.saturating_sub(kept.0.len() as u64)).unwrap_or(usize::MAX);
    let take = bytes.len().min(room);
    kept.0.extend_from_slice(&bytes[..take]);
    true
}

/// Reads a stream to its end, keeping at most `max` bytes and counting all.
/// When `gone` says the program's group is gone, it takes what is in the
/// pipe now and stops: the end of the pipe may be held back by a process
/// that is not the program's, and is not waited for.
async fn capture<S>(mut stream: S, max: u64, into: Kept, gone: Arc<tokio::sync::Notify>)
where
    S: AsyncRead + Unpin + os::ReadNow,
{
    let mut buffer = vec![0_u8; 16 * 1024];
    loop {
        tokio::select! {
            biased;
            read = stream.read(&mut buffer) => match read {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    if !keep(&into, max, &buffer[..read]) {
                        return;
                    }
                }
            },
            () = gone.notified() => break,
        }
    }
    while let Some(read) = stream.read_now(&mut buffer) {
        if !keep(&into, max, &buffer[..read]) {
            return;
        }
    }
}

/// Ends the group when a run's future is dropped before it finished.
struct EndOnDrop {
    supervisor: Supervisor,
    id: u64,
    group: Arc<os::Group>,
    armed: bool,
}

impl Drop for EndOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.group.kill();
        }
        self.supervisor.forget(self.id);
    }
}

impl Supervisor {
    /// A supervisor with nothing running.
    #[must_use]
    pub fn new(limits: SupervisorLimits) -> Self {
        Self {
            inner: Arc::new(Inner {
                limits,
                state: Mutex::new(State::default()),
                next: AtomicU64::new(1),
                shutdown: tokio::sync::Mutex::new(()),
            }),
        }
    }

    fn forget(&self, id: u64) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.live.remove(&id);
        }
    }

    /// Checks the counts and the place, before anything is started.
    fn admit(&self, spec: &LaunchSpec) -> Result<(), Failure> {
        let state = self.inner.state.lock().map_err(|_| {
            codes::busy(
                codes::START_FAILED,
                "The supervisor's own state is broken.",
                "Restart the engine.",
            )
        })?;
        if state.stopping {
            return Err(codes::busy(
                codes::SHUTTING_DOWN,
                "The engine is stopping and starts nothing.",
                "Start the engine again.",
            ));
        }
        if let Some(left) = state
            .leftovers
            .iter()
            .find(|left| left.kind.as_ref() == Some(&spec.kind))
        {
            return Err(codes::busy(
                codes::CLEANUP_INCOMPLETE,
                format!(
                    "An earlier `{}` subsystem could not be cleaned up: {}",
                    spec.kind, left.what
                ),
                "Run `anything2mcp doctor` to see what remains, end it, and try again.",
            ));
        }
        let limit = self
            .inner
            .limits
            .kinds
            .get(&spec.kind)
            .copied()
            .unwrap_or(self.inner.limits.per_kind);
        let running = state
            .live
            .values()
            .filter(|entry| entry.kind == spec.kind)
            .count();
        if running >= limit {
            return Err(codes::busy(
                codes::SUBSYSTEM_LIMIT,
                format!(
                    "{running} `{}` subsystems run, which is the limit.",
                    spec.kind
                ),
                "Wait for one to finish, or raise the limit in the settings.",
            ));
        }
        let taken = spec.place.is_some()
            && state
                .live
                .values()
                .any(|entry| entry.kind == spec.kind && entry.place == spec.place);
        if taken {
            return Err(codes::busy(
                codes::SUBSYSTEM_LIMIT,
                format!(
                    "A `{}` subsystem already holds the place asked for.",
                    spec.kind
                ),
                "Wait for it to finish, or stop it.",
            ));
        }
        Ok(())
    }

    /// Identifies the program, builds its command, and starts it in a group
    /// of its own.
    async fn launch(
        &self,
        spec: &LaunchSpec,
        for_whom: Option<&ProviderName>,
        link: &LinkKind,
    ) -> Result<(u64, Arc<os::Group>, Option<os::Pipes>, Digest, bool), Failure> {
        self.admit(spec)?;
        if !spec.program.path.is_absolute() {
            return Err(codes::input(
                codes::PROGRAM_NOT_FOUND,
                "A program is started by its absolute path, never by a name to search for.",
                "Name the program's absolute path where it is declared.",
            ));
        }
        // Identify before starting.
        let path = spec.program.path.clone();
        let digest = tokio::task::spawn_blocking(move || digest_of(&path))
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| {
                codes::input(
                    codes::PROGRAM_NOT_FOUND,
                    format!(
                        "The program `{}` is not there, or cannot be read.",
                        codes::shown(&spec.program.path.display().to_string())
                    ),
                    "Install the program at the path that is declared, or correct the declaration.",
                )
            })?;
        let verified = match spec.program.digest {
            None => false,
            Some(declared) if declared == digest => true,
            Some(_) => {
                return Err(codes::policy(
                    codes::EFFECT_NOT_DECLARED,
                    format!(
                        "The program `{}` is not the one that was declared: its digest is different.",
                        codes::shown(&spec.program.path.display().to_string())
                    ),
                    "Review the program that is installed now, and declare its digest.",
                ));
            }
        };

        let mut command = Command::new(&spec.program.path);
        command.args(&spec.args);
        // Nothing is inherited from the engine's environment unless listed.
        command.env_clear();
        for name in &spec.env.pass {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        for (name, value) in &spec.env.set {
            command.env(name, value);
        }
        command.env(OWNER_MARK_VARIABLE, owner_mark()?);
        match &spec.cwd {
            Some(folder) => command.current_dir(folder.as_path()),
            None => command.current_dir(std::env::temp_dir()),
        };
        match link {
            LinkKind::OneShot(one_shot) => {
                command
                    .stdin(if one_shot.stdin.is_some() {
                        Stdio::piped()
                    } else {
                        Stdio::null()
                    })
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
            }
            LinkKind::Foreign(_) => {
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
            }
        }
        let foreign = match link {
            LinkKind::Foreign(pipe) => Some(pipe),
            LinkKind::OneShot(_) => None,
        };
        let (group, pipes) = os::spawn(command, foreign).map_err(|error| {
            codes::busy(
                codes::START_FAILED,
                format!(
                    "The operating system did not start `{}`: {}.",
                    codes::shown(&spec.program.path.display().to_string()),
                    error.kind()
                ),
                "Check that the program can be run on this machine.",
            )
        })?;
        let group = Arc::new(group);
        let id = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let entry = Entry {
            kind: spec.kind.clone(),
            place: spec.place.clone(),
            for_whom: for_whom.cloned(),
            link: match link {
                LinkKind::OneShot(_) => "one_shot",
                LinkKind::Foreign(_) => "foreign",
            },
            group: Arc::clone(&group),
        };
        let stopping = match self.inner.state.lock() {
            Ok(mut state) => {
                if state.stopping {
                    true
                } else {
                    state.live.insert(id, entry);
                    false
                }
            }
            Err(_) => true,
        };
        if stopping {
            group.reap().await;
            return Err(codes::busy(
                codes::SHUTTING_DOWN,
                "The engine is stopping and starts nothing.",
                "Start the engine again.",
            ));
        }
        Ok((id, group, pipes, digest, verified))
    }

    /// Runs a program to completion: its arguments as a list, only the
    /// environment listed, its output kept up to a bound and counted beyond
    /// it, and its whole group ended at exit or at the time limit.
    ///
    /// # Errors
    ///
    /// With [`Ran::NotStarted`]: `input`/`program_not_found`,
    /// `policy`/`effect_not_declared` for a digest other than the one
    /// declared, `busy`/`subsystem_limit`, `busy`/`cleanup_incomplete`,
    /// `busy`/`shutting_down`, `busy`/`start_failed`. With [`Ran::Unknown`]:
    /// `transient`/`timeout` when the time limit passed; the caller of a
    /// committing run treats that as a write whose fate is unknown.
    pub async fn run(&self, spec: LaunchSpec, link: OneShot) -> Result<Finished, ConnFailure> {
        self.run_for(spec, link, None).await
    }

    pub(crate) async fn run_for(
        &self,
        spec: LaunchSpec,
        link: OneShot,
        for_whom: Option<&ProviderName>,
    ) -> Result<Finished, ConnFailure> {
        let kind = spec.kind.clone();
        let link_kind = LinkKind::OneShot(link);
        let (id, group, _, program_digest, verified) = self
            .launch(&spec, for_whom, &link_kind)
            .await
            .map_err(not_started)?;
        let LinkKind::OneShot(link) = link_kind else {
            return Err(not_started(codes::busy(
                codes::START_FAILED,
                "A one-shot run lost its link.",
                "Report this as a defect of the engine.",
            )));
        };
        let mut guard = EndOnDrop {
            supervisor: self.clone(),
            id,
            group: Arc::clone(&group),
            armed: true,
        };

        let (stdin, stdout, stderr) = group.take_streams();
        let out = Arc::new(Mutex::new((Vec::new(), 0_u64)));
        let err = Arc::new(Mutex::new((Vec::new(), 0_u64)));
        let mut readers = Vec::new();
        if let Some(stream) = stdout {
            let gone = Arc::new(tokio::sync::Notify::new());
            let reader = tokio::spawn(capture(
                stream,
                link.max_output_bytes,
                Arc::clone(&out),
                Arc::clone(&gone),
            ));
            readers.push((reader, gone));
        }
        if let Some(stream) = stderr {
            let gone = Arc::new(tokio::sync::Notify::new());
            let reader = tokio::spawn(capture(
                stream,
                link.max_output_bytes,
                Arc::clone(&err),
                Arc::clone(&gone),
            ));
            readers.push((reader, gone));
        }
        if let (Some(mut stream), Some(input)) = (stdin, link.stdin) {
            tokio::spawn(async move {
                // A program that reads none of it closes the pipe; that is
                // its choice and not a failure.
                let _ = stream.write_all(&input).await;
                let _ = stream.shutdown().await;
            });
        }

        let timed_out = tokio::select! {
            () = group.leader_exit() => false,
            () = tokio::time::sleep(link.time_limit) => true,
        };
        // Ends whatever the program left running, and the program itself
        // when its time is up; then reaps it.
        let (status, ended) = group.reap().await;
        guard.armed = false;

        let mut remaining = Vec::new();
        if !ended {
            remaining.push(Leftover {
                kind: Some(kind.clone()),
                what: format!(
                    "the process group of {} could not be ended",
                    SubsystemId(id)
                ),
            });
        }
        // The group is gone, so everything it wrote is in the pipes. A
        // reader that has not seen the end of its pipe is told to take what
        // is there and stop. Who else may hold the pipe is not the
        // program's doing, and is no part of the verdict on its cleanup.
        for (mut reader, gone) in readers {
            if tokio::time::timeout(os::END_OF_OUTPUT_WAIT, &mut reader)
                .await
                .is_err()
            {
                gone.notify_one();
                let _ = reader.await;
            }
        }
        if !remaining.is_empty()
            && let Ok(mut state) = self.inner.state.lock()
        {
            state.leftovers.extend(remaining.iter().cloned());
        }
        drop(guard);

        if timed_out {
            return Err(ConnFailure {
                failure: Failure::new(
                    FailureClass::Transient,
                    codes::TIMEOUT,
                    format!(
                        "The program ran longer than its limit of {} ms and was ended.",
                        link.time_limit.as_millis()
                    ),
                    "If the run was to write, read the record back before anything is written again.",
                ),
                ran: Ran::Unknown,
            });
        }
        let take = |kept: &Arc<Mutex<(Vec<u8>, u64)>>| {
            kept.lock()
                .map(|mut kept| (std::mem::take(&mut kept.0), kept.1))
                .unwrap_or_default()
        };
        let (stdout, stdout_bytes) = take(&out);
        let (stderr, stderr_bytes) = take(&err);
        Ok(Finished {
            exit: status
                .and_then(|status| status.code())
                .map_or(Exit::Killed, Exit::Code),
            stdout,
            stderr,
            stdout_bytes,
            stderr_bytes,
            program_digest,
            verified,
            cleanup: Cleanup::incomplete(remaining),
        })
    }

    /// Starts a program that speaks its own protocol over a pair of pipes.
    /// The supervisor starts, counts, and stops it; its pack frames the
    /// bytes. The program runs until [`Supervisor::stop`] or
    /// [`Supervisor::shutdown`].
    ///
    /// # Errors
    ///
    /// As the not-started failures of [`Supervisor::run`].
    pub async fn start_foreign(
        &self,
        spec: LaunchSpec,
        pipe: ForeignPipe,
    ) -> Result<ForeignLink, Failure> {
        self.start_foreign_for(spec, pipe, None).await
    }

    pub(crate) async fn start_foreign_for(
        &self,
        spec: LaunchSpec,
        pipe: ForeignPipe,
        for_whom: Option<&ProviderName>,
    ) -> Result<ForeignLink, Failure> {
        let (id, group, pipes, program_digest, verified) = self
            .launch(&spec, for_whom, &LinkKind::Foreign(pipe))
            .await?;
        let Some((to_child, from_child)) = pipes else {
            self.forget(id);
            group.reap().await;
            return Err(codes::busy(
                codes::START_FAILED,
                "A foreign link was started without its pipe.",
                "Report this as a defect of the engine.",
            ));
        };
        Ok(ForeignLink {
            id: SubsystemId(id),
            to_child,
            from_child,
            program_digest,
            verified,
        })
    }

    /// Every subsystem that is running or has exited and not been stopped.
    #[must_use]
    pub fn list(&self) -> Vec<SubsystemListing> {
        let Ok(state) = self.inner.state.lock() else {
            return Vec::new();
        };
        state
            .live
            .iter()
            .map(|(id, entry)| SubsystemListing {
                id: SubsystemId(*id),
                kind: entry.kind.clone(),
                place: entry.place.clone(),
                for_whom: entry.for_whom.clone(),
                link: entry.link,
                process: entry.group.id(),
                state: if entry.group.exited() {
                    SubsystemState::Exited
                } else {
                    SubsystemState::Busy
                },
            })
            .collect()
    }

    /// What earlier stops could not end. While a kind is listed here, no
    /// subsystem of that kind starts.
    #[must_use]
    pub fn leftovers(&self) -> Vec<Leftover> {
        self.inner
            .state
            .lock()
            .map(|state| state.leftovers.clone())
            .unwrap_or_default()
    }

    /// Forgets the leftovers, after a person has ended what remained.
    pub fn clear_leftovers(&self) {
        if let Ok(mut state) = self.inner.state.lock() {
            state.leftovers.clear();
        }
    }

    async fn end(&self, id: u64, entry: Entry) -> Option<Leftover> {
        if entry.link == "one_shot" {
            // The run that owns it reaps it; ending the group is enough.
            return (!entry.group.kill()).then(|| Leftover {
                kind: Some(entry.kind.clone()),
                what: format!(
                    "the process group of {} could not be ended",
                    SubsystemId(id)
                ),
            });
        }
        let (_, ended) = entry.group.reap().await;
        (!ended).then(|| Leftover {
            kind: Some(entry.kind.clone()),
            what: format!(
                "the process group of {} could not be ended",
                SubsystemId(id)
            ),
        })
    }

    /// Stops one subsystem: ends its whole group and waits for it.
    pub async fn stop(&self, id: SubsystemId) -> Cleanup {
        self.stop_for(id, None).await
    }

    pub(crate) async fn stop_for(&self, id: SubsystemId, owner: Option<&ProviderName>) -> Cleanup {
        let entry = match self.inner.state.lock() {
            Ok(mut state) => {
                let owned = state
                    .live
                    .get(&id.0)
                    .is_some_and(|entry| owner.is_none() || entry.for_whom.as_ref() == owner);
                if owned {
                    state.live.remove(&id.0)
                } else {
                    None
                }
            }
            Err(_) => None,
        };
        let Some(entry) = entry else {
            return Cleanup::complete();
        };
        let left: Vec<Leftover> = self.end(id.0, entry).await.into_iter().collect();
        if let (false, Ok(mut state)) = (left.is_empty(), self.inner.state.lock()) {
            state.leftovers.extend(left.iter().cloned());
        }
        Cleanup::incomplete(left)
    }

    /// Stops everything and starts nothing afterwards. Every caller that
    /// asks joins the same cleanup and gets its result.
    pub async fn shutdown(&self) -> Cleanup {
        let _one = self.inner.shutdown.lock().await;
        let entries: Vec<(u64, Entry)> = match self.inner.state.lock() {
            Ok(mut state) => {
                state.stopping = true;
                std::mem::take(&mut state.live).into_iter().collect()
            }
            Err(_) => Vec::new(),
        };
        let mut left = Vec::new();
        for (id, entry) in entries {
            left.extend(self.end(id, entry).await);
        }
        match self.inner.state.lock() {
            Ok(mut state) => {
                state.leftovers.extend(left);
                Cleanup::incomplete(state.leftovers.clone())
            }
            Err(_) => Cleanup::incomplete(left),
        }
    }
}

/// What a pack gets: a supervisor that starts only the programs the pack's
/// manifest declares, and, where the manifest says `plan_programs`, those a
/// published plan names. Anything else is `policy`/`effect_not_declared`.
#[derive(Clone)]
pub struct ScopedSupervisor {
    supervisor: Supervisor,
    provider: ProviderName,
    declared: Arc<Vec<ProgramRef>>,
    plan_allowed: bool,
    plan: Arc<Vec<ProgramRef>>,
}

impl fmt::Debug for ScopedSupervisor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopedSupervisor")
            .field("provider", &self.provider)
            .field("declared", &self.declared.len())
            .field("plan", &self.plan.len())
            .finish()
    }
}

impl ScopedSupervisor {
    pub(crate) fn new(
        supervisor: Supervisor,
        provider: ProviderName,
        declared: Vec<ProgramRef>,
        plan_allowed: bool,
    ) -> Self {
        Self {
            supervisor,
            provider,
            declared: Arc::new(declared),
            plan_allowed,
            plan: Arc::new(Vec::new()),
        }
    }

    /// The same scope with the programs of a published plan added, when the
    /// manifest allows plan programs at all.
    pub(crate) fn with_plan_programs(&self, programs: Vec<ProgramRef>) -> Self {
        let mut scoped = self.clone();
        scoped.plan = Arc::new(if self.plan_allowed {
            programs
        } else {
            Vec::new()
        });
        scoped
    }

    /// Checks the program against what is declared and gives the spec to
    /// start, with the declared digest filled in.
    fn allowed(&self, mut spec: LaunchSpec) -> Result<LaunchSpec, Failure> {
        let asked = std::fs::canonicalize(&spec.program.path).ok();
        let found = self
            .declared
            .iter()
            .chain(self.plan.iter())
            .find(|declared| {
                asked.is_some() && std::fs::canonicalize(&declared.path).ok() == asked
            });
        let Some(found) = found else {
            return Err(codes::policy(
                codes::EFFECT_NOT_DECLARED,
                format!(
                    "The provider `{}` asked to start a program that neither its manifest nor the published plan declares.",
                    self.provider
                ),
                "Declare the program under `effects.processes` in the provider's manifest, or name it in the target's files and publish.",
            ));
        };
        if found.digest.is_some() {
            spec.program.digest = found.digest;
        }
        Ok(spec)
    }

    /// As [`Supervisor::run`], for a declared program.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared` for a program that is not declared,
    /// or whose digest is not the declared one; then as
    /// [`Supervisor::run`].
    pub async fn run(&self, spec: LaunchSpec, link: OneShot) -> Result<Finished, ConnFailure> {
        let spec = self.allowed(spec).map_err(not_started)?;
        self.supervisor
            .run_for(spec, link, Some(&self.provider))
            .await
    }

    /// As [`Supervisor::start_foreign`], for a declared program.
    ///
    /// # Errors
    ///
    /// `policy`/`effect_not_declared`; then as
    /// [`Supervisor::start_foreign`].
    pub async fn start_foreign(
        &self,
        spec: LaunchSpec,
        pipe: ForeignPipe,
    ) -> Result<ForeignLink, Failure> {
        let spec = self.allowed(spec)?;
        self.supervisor
            .start_foreign_for(spec, pipe, Some(&self.provider))
            .await
    }

    /// Stops a subsystem this provider started. Another provider's is left
    /// alone.
    pub async fn stop(&self, id: SubsystemId) -> Cleanup {
        self.supervisor.stop_for(id, Some(&self.provider)).await
    }
}
