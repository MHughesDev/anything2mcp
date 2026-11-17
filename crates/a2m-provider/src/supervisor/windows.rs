//! Job objects on Windows.
//!
//! Every child is placed in a job object of its own, set to end every
//! process in it when the job's last handle closes. The supervisor holds
//! that handle, so the tree ends when the supervisor ends it, and also when
//! the engine itself dies. Membership of the job is the proof of ownership:
//! ending the job reaches only what was started in it.
//!
//! A child is created suspended, placed in its job, and only then resumed.
//! It runs no instruction outside the job, so a process it starts at once
//! is in the job too. The standard library does not hand out the handle of
//! the child's first thread, so the thread is found in a snapshot of the
//! system's threads by the child's process identifier and resumed through a
//! handle opened for that alone.
//!
//! A program is opened once, shared for reading only, and digested through
//! that handle. The handle stays open until the child exists: while it is
//! open the system lets nobody write the file, rename it, or delete it, so
//! the file that is started by its path is the file that was digested.
//! Windows cannot start a process from an open handle. A folder on the way
//! to the file is not held: under the start lock the path is resolved once
//! more and must still lead to the same file, and a link on the way that is
//! repointed in the moment between that look and the start is not seen.
//!
//! The standard library starts every child with handle inheritance on. A
//! foreign link marks two handles inheritable for the moment of one start,
//! so every start of a process takes the one lock below; a second child
//! started at that moment would otherwise inherit the first child's pipe.

use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io;
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll, Waker};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::process::{Child, Command};
use windows_sys::Win32::Foundation::{
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};

use super::{ForeignPipe, StartError};

/// The two ends of a foreign link that stay with the engine.
pub(crate) type Pipes = (
    Box<dyn AsyncWrite + Send + Unpin>,
    Box<dyn AsyncRead + Send + Unpin>,
);

/// Held by every start of a process on Windows.
static START: Mutex<()> = Mutex::new(());

/// One child and the job object that holds its tree.
pub(crate) struct Group {
    job: OwnedHandle,
    id: u32,
    child: tokio::sync::Mutex<Option<Child>>,
}

/// How long a run waits for the end of its program's output once the job
/// is ended. The pipe ends when the last process holding it is gone, which
/// ending the job brings about; the wait only bounds a process that broke
/// away from the job.
pub(crate) const END_OF_OUTPUT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Reading what is in a pipe now, without waiting for more. Not offered on
/// Windows, where a run reads to the end of the pipe.
pub(crate) trait ReadNow {
    /// Always `None` here.
    fn read_now(&self, buffer: &mut [u8]) -> Option<usize>;
}

impl<T> ReadNow for T {
    fn read_now(&self, _buffer: &mut [u8]) -> Option<usize> {
        None
    }
}

/// The exit status the supervisor gives every process it ends with the job.
/// Windows has no signals: a process that is ended from outside exits with
/// whatever status its ender names. This one is the supervisor's own mark,
/// so that a program the supervisor ended is told apart from a program that
/// exited by itself, whatever status that program chose.
const ENDED_BY_SUPERVISOR: u32 = 0x4132_4D01;

/// Whether the status is that of a program the supervisor ended. A program
/// ended by someone else shows the status that ender named, as any exit.
pub(crate) fn ended_from_outside(status: ExitStatus) -> bool {
    status
        .code()
        .is_some_and(|code| u32::from_ne_bytes(code.to_ne_bytes()) == ENDED_BY_SUPERVISOR)
}

fn other(message: &'static str) -> io::Error {
    io::Error::other(message)
}

/// Makes a job object that ends its processes when its last handle closes.
#[allow(unsafe_code)] // three calls into the operating system; each is explained where it is made
fn new_job() -> io::Result<OwnedHandle> {
    // SAFETY: both arguments may be null: default security, and no name.
    // The call touches no memory of ours.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a handle the call above just returned, it is not
    // null, and nothing else owns it. From here the `OwnedHandle` closes it.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let size = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
        .map_err(|_| other("the job limits do not fit a length"))?;
    // SAFETY: the job handle is open and owned by `job`. The pointer is to
    // `limits`, a live value of exactly the type the information class
    // names, and `size` is its size. The call only reads it.
    let ok = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            size,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

/// Sets or clears the mark that lets a child inherit `file`'s handle.
#[allow(unsafe_code)] // one call into the operating system, explained below
fn set_inherit(file: &File, on: bool) -> io::Result<()> {
    // SAFETY: the handle is open and owned by `file`, which outlives the
    // call. The call changes one flag on the handle and touches no memory.
    let ok = unsafe {
        SetHandleInformation(
            file.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            if on { HANDLE_FLAG_INHERIT } else { 0 },
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// The four ends of a foreign link. The engine keeps the two server ends,
/// which the runtime reads and writes without blocking. The child gets the
/// two client ends, opened for ordinary blocking use, which is what a child
/// expects of a handle it is given.
struct ForeignEnds {
    to_child: NamedPipeServer,
    from_child: NamedPipeServer,
    child_reads: File,
    child_writes: File,
}

/// Completes the engine's end of a pipe whose client end is already open.
///
/// The runtime starts no read and reports no room to write on a server end
/// until that end has been connected, also when its client is already
/// there. Without this the engine's ends carry nothing in either direction:
/// a request is never written and an answer never read, while the child
/// waits. With the client already open the connection completes at once,
/// so it is asked for once here and never waited for.
fn connect_now(server: &NamedPipeServer) -> io::Result<()> {
    let mut connecting = std::pin::pin!(server.connect());
    let mut context = Context::from_waker(Waker::noop());
    match connecting.as_mut().poll(&mut context) {
        Poll::Ready(connected) => connected,
        Poll::Pending => Err(other(
            "the engine's end of a pipe did not connect to its client end",
        )),
    }
}

/// Makes the two pipes of a foreign link, each with both of its ends open
/// and connected.
fn foreign_ends() -> io::Result<ForeignEnds> {
    // The name is not a secret: each pipe allows one instance and one
    // client, and this process is that client before the child starts.
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let unique = format!("{}-{count}", std::process::id());
    let down = format!(r"\\.\pipe\anything2mcp-{unique}-to-child");
    let up = format!(r"\\.\pipe\anything2mcp-{unique}-from-child");
    let to_child = ServerOptions::new()
        .first_pipe_instance(true)
        .max_instances(1)
        .access_inbound(false)
        .access_outbound(true)
        .create(&down)?;
    let from_child = ServerOptions::new()
        .first_pipe_instance(true)
        .max_instances(1)
        .access_inbound(true)
        .access_outbound(false)
        .create(&up)?;
    let child_reads = OpenOptions::new().read(true).open(&down)?;
    let child_writes = OpenOptions::new().write(true).open(&up)?;
    connect_now(&to_child)?;
    connect_now(&from_child)?;
    Ok(ForeignEnds {
        to_child,
        from_child,
        child_reads,
        child_writes,
    })
}

/// The share mode of a file others may open for reading, and for nothing
/// else: not for writing, and not for deleting or renaming.
const FILE_SHARE_READ: u32 = 0x0000_0001;

/// A program opened once, to be digested and then started.
pub(crate) struct Program {
    /// The path it was asked for by, which it is started by.
    named: PathBuf,
    /// That path with every link resolved, when it was opened.
    real: PathBuf,
    /// Held open, shared for reading only, until the child exists.
    file: File,
}

impl Program {
    /// Opens the file at `real`, the resolved form of `named`, so that
    /// nobody can write, rename, or delete it while the handle is open.
    /// Only a regular file is taken. A file someone has open for writing
    /// cannot be opened this way, and is not started.
    pub(crate) fn open(named: &Path, real: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(real)?;
        if !file.metadata()?.is_file() {
            return Err(other("a program is a regular file"));
        }
        Ok(Self {
            named: named.to_path_buf(),
            real: real.to_path_buf(),
            file,
        })
    }

    /// The digest of the open file.
    pub(crate) fn digest(&mut self) -> io::Result<a2m_core::Digest> {
        super::digest_read(&mut self.file).map(|(digest, _)| digest)
    }

    /// Whether the path the program is started by still leads to the file
    /// that is held open.
    fn still_there(&self) -> bool {
        std::fs::canonicalize(&self.named).is_ok_and(|now| now == self.real)
    }
}

/// Starts the command with its first thread suspended: the process exists
/// and has run nothing.
fn start_suspended(command: &mut Command) -> io::Result<Child> {
    command.creation_flags(CREATE_SUSPENDED);
    command.spawn()
}

/// Places the child in the job.
#[allow(unsafe_code)] // one call into the operating system, explained below
fn admit(job: &OwnedHandle, child: &Child) -> io::Result<()> {
    let process = child
        .raw_handle()
        .ok_or_else(|| other("the child has no process handle"))?;
    // SAFETY: both handles are open: the job is owned by `job`, and the
    // process handle by `child`, which is alive here. The process handle
    // is the one the system gave when it created the process, which carries
    // every right the call asks for. The call touches no memory of ours.
    let ok = unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Resumes the threads of a process that was created suspended. Such a
/// process has one thread, its first. The threads are found in a snapshot
/// of every thread in the system, by the identifier of the process that
/// owns them.
#[allow(unsafe_code)] // five calls into the operating system; each is explained where it is made
fn resume(process_id: u32) -> io::Result<()> {
    // SAFETY: the call takes two numbers and touches no memory of ours. For
    // a snapshot of threads the process identifier is not used.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a handle the call above just returned, it is not the
    // value that means failure, and nothing else owns it. From here the
    // `OwnedHandle` closes it.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>())
            .map_err(|_| other("a thread entry does not fit a length"))?,
        ..THREADENTRY32::default()
    };
    let mut resumed = 0_u32;
    // SAFETY: the snapshot handle is open and owned by `snapshot`. The
    // pointer is to `entry`, a live value of the type the call fills, whose
    // first field holds its size as the call requires.
    let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == process_id {
            // SAFETY: the call takes three numbers and touches no memory of
            // ours. It asks for the one right the next call needs, and for
            // a handle no child inherits.
            let raw = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `raw` is a handle the call above just returned, it is
            // not null, and nothing else owns it.
            let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
            // SAFETY: the thread handle is open, owned by `thread`, and was
            // opened with the right to resume. The call touches no memory.
            let before = unsafe { ResumeThread(thread.as_raw_handle()) };
            if before == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            resumed += 1;
        }
        // SAFETY: as for the first entry above.
        more = unsafe { Thread32Next(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    }
    if resumed == 0 {
        return Err(other("the child has no thread to resume"));
    }
    Ok(())
}

/// Starts `program` inside a new job object, with the command `command`
/// builds for the path it is given. The child is created suspended, placed
/// in the job, and then resumed, so nothing it starts is outside the job.
/// With `foreign`, the child inherits the two pipe ends, named by value in
/// one added argument.
pub(crate) fn spawn(
    program: &Program,
    command: impl FnOnce(&Path) -> Command,
    foreign: Option<&ForeignPipe>,
) -> Result<(Group, Option<Pipes>), StartError> {
    let job = new_job()?;
    let mut command = command(&program.named);
    let mut child_ends = None;
    let pipes = match foreign {
        None => None,
        Some(pipe_spec) => {
            let ForeignEnds {
                to_child,
                from_child,
                child_reads,
                child_writes,
            } = foreign_ends()?;
            command.arg(
                pipe_spec
                    .windows_argument
                    .replace(
                        "{read}",
                        &(child_reads.as_raw_handle() as usize).to_string(),
                    )
                    .replace(
                        "{write}",
                        &(child_writes.as_raw_handle() as usize).to_string(),
                    ),
            );
            child_ends = Some((child_reads, child_writes));
            let pipes: Pipes = (Box::new(to_child), Box::new(from_child));
            Some(pipes)
        }
    };
    let mut child = {
        let _one_start = START
            .lock()
            .map_err(|_| other("the start lock is broken"))?;
        // The file is held open and cannot change. The path to it is looked
        // at once more, as late as can be.
        if !program.still_there() {
            return Err(StartError::Changed);
        }
        if let Some((reads, writes)) = &child_ends {
            set_inherit(reads, true)?;
            set_inherit(writes, true)?;
        }
        let started = start_suspended(&mut command);
        if let Some((reads, writes)) = &child_ends {
            set_inherit(reads, false)?;
            set_inherit(writes, false)?;
        }
        started?
    };
    // Close this process's copies of the child's ends, so that a dead child
    // shows as the end of the pipe.
    drop(child_ends);
    // Into the job first, and only then allowed to run.
    let id = child.id().unwrap_or(0);
    if let Err(error) = admit(&job, &child).and_then(|()| resume(id)) {
        // It has run nothing. It is ended on its own, in case it is not in
        // the job; if it is, closing the job's last handle ends it as well.
        let _ = child.start_kill();
        return Err(StartError::Failed(error));
    }
    Ok((
        Group {
            job,
            id,
            child: tokio::sync::Mutex::new(Some(child)),
        },
        pipes,
    ))
}

impl Group {
    /// The child's process identifier.
    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    /// Ends every process in the job. Returns false when it could not.
    #[allow(unsafe_code)] // one call into the operating system, explained below
    pub(crate) fn kill(&self) -> bool {
        // SAFETY: the job handle is open and owned by `self.job`. The call
        // touches no memory of ours. It reaches only processes in the job.
        let ok = unsafe { TerminateJobObject(self.job.as_raw_handle(), ENDED_BY_SUPERVISOR) };
        ok != 0
    }

    /// Whether the child has exited.
    pub(crate) fn exited(&self) -> bool {
        match self.child.try_lock() {
            Ok(mut child) => match child.as_mut() {
                Some(child) => !matches!(child.try_wait(), Ok(None)),
                None => true,
            },
            // Someone is waiting for it, so it has not exited yet.
            Err(_) => false,
        }
    }

    /// Resolves when the child has exited.
    pub(crate) async fn leader_exit(&self) {
        let mut child = self.child.lock().await;
        if let Some(child) = child.as_mut() {
            let _ = child.wait().await;
        }
    }

    /// Takes the child's standard streams, where it was given pipes.
    pub(crate) fn take_streams(
        &self,
    ) -> (
        Option<tokio::process::ChildStdin>,
        Option<tokio::process::ChildStdout>,
        Option<tokio::process::ChildStderr>,
    ) {
        match self.child.try_lock() {
            Ok(mut child) => match child.as_mut() {
                Some(child) => (child.stdin.take(), child.stdout.take(), child.stderr.take()),
                None => (None, None, None),
            },
            Err(_) => (None, None, None),
        }
    }

    /// Ends the job and waits for the child. The flag is false when the job
    /// could not be ended.
    pub(crate) async fn reap(&self) -> (Option<ExitStatus>, bool) {
        let ended = self.kill();
        let mut child = self.child.lock().await;
        match child.take() {
            Some(mut child) => (child.wait().await.ok(), ended),
            None => (None, ended),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::windows::process::ExitStatusExt;
    use std::process::ExitStatus;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use std::os::windows::io::{AsRawHandle, OwnedHandle};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use tokio::process::Command;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
        QueryInformationJobObject,
    };

    use super::{
        ENDED_BY_SUPERVISOR, ForeignEnds, Program, admit, ended_from_outside, foreign_ends,
        new_job, resume, set_inherit, spawn, start_suspended,
    };

    /// Long enough for a loaded machine; the tests wait on a condition and
    /// not for this long.
    const WITHIN: Duration = Duration::from_secs(60);

    /// The system's own command interpreter, which every Windows has.
    fn shell() -> PathBuf {
        std::env::var_os("ComSpec").map_or_else(
            || PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            PathBuf::from,
        )
    }

    /// How many processes the job has ever held, and how many it holds now.
    #[allow(unsafe_code)] // one call into the operating system, explained below
    fn processes(job: &OwnedHandle) -> (u32, u32) {
        let mut counts = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let size =
            u32::try_from(std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>()).unwrap();
        // SAFETY: the job handle is open and owned by `job`. The pointer is
        // to `counts`, a live value of exactly the type the information
        // class names, and `size` is its size. The length written is not
        // asked for, which a null pointer says.
        let ok = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&raw mut counts).cast(),
                size,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(ok, 0, "the job could not be asked for its counts");
        (counts.TotalProcesses, counts.ActiveProcesses)
    }

    /// The order that keeps a tree whole: the child exists, has run
    /// nothing, is placed in the job, and only then runs. A program that
    /// does nothing but exit shows it: suspended, it never exits.
    #[tokio::test]
    async fn a_child_runs_nothing_until_it_has_been_placed_in_its_job() {
        let job = new_job().unwrap();
        let mut command = Command::new(shell());
        command.args(["/c", "exit 7"]);
        let mut child = start_suspended(&mut command).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(3), child.wait())
                .await
                .is_err(),
            "the child ran before it was placed in its job"
        );
        assert_eq!(processes(&job), (0, 0));
        admit(&job, &child).unwrap();
        assert_eq!(processes(&job).1, 1, "the suspended child is in the job");
        resume(child.id().unwrap()).unwrap();
        let status = tokio::time::timeout(WITHIN, child.wait())
            .await
            .expect("the child was not resumed")
            .unwrap();
        assert_eq!(status.code(), Some(7));
    }

    /// A child that starts another process at once: both are in the job,
    /// and ending the job leaves neither. The second process would run for
    /// ten minutes.
    #[tokio::test]
    async fn a_process_the_child_starts_at_once_is_in_the_job_and_ends_with_it() {
        let shell = shell();
        let real = std::fs::canonicalize(&shell).unwrap();
        let mut program = Program::open(&shell, &real).unwrap();
        program.digest().unwrap();
        let started = spawn(
            &program,
            |path| {
                let mut command = Command::new(path);
                command
                    .args(["/c", "ping -n 600 127.0.0.1"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                command
            },
            None,
        );
        drop(program);
        let Ok((group, _)) = started else {
            panic!("the interpreter did not start");
        };
        // Wait until the job holds the interpreter and what it started.
        let begun = Instant::now();
        while processes(&group.job).1 < 2 {
            assert!(
                begun.elapsed() < WITHIN,
                "the process the child started is not in the job: {:?}",
                processes(&group.job)
            );
            tokio::task::yield_now().await;
        }
        let (status, ended) = group.reap().await;
        assert!(ended, "the job could not be ended");
        assert!(status.is_some_and(ended_from_outside));
        let begun = Instant::now();
        while processes(&group.job).1 != 0 {
            assert!(
                begun.elapsed() < WITHIN,
                "ending the job left a process running: {:?}",
                processes(&group.job)
            );
            tokio::task::yield_now().await;
        }
    }

    /// A program is held open so that nobody can write, rename, or delete
    /// it between its digest and its start.
    #[test]
    fn a_program_cannot_be_changed_while_it_is_held_for_its_start() {
        let folder = std::env::temp_dir().join(format!("a2m-provider-held-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("program.exe");
        std::fs::write(&path, b"reviewed").unwrap();
        let real = std::fs::canonicalize(&path).unwrap();
        let mut program = Program::open(&path, &real).unwrap();
        assert_eq!(program.digest().unwrap(), a2m_core::Digest::of(b"reviewed"));
        assert!(program.still_there());
        assert!(std::fs::write(&path, b"replaced").is_err());
        assert!(std::fs::remove_file(&path).is_err());
        assert!(std::fs::rename(&path, folder.join("moved.exe")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"reviewed");
        drop(program);
        // Released, it can be changed again, and a program someone holds
        // open for writing is not taken.
        let writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        assert!(Program::open(&path, &real).is_err());
        drop(writer);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    /// The pipes of a foreign link, without a child: a thread stands for
    /// the child and uses its two ends with ordinary blocking calls, as a
    /// child does. A request written by the engine arrives, an answer
    /// written by the child is read, and the child closing its ends is the
    /// end of the pipe.
    #[tokio::test]
    async fn the_two_pipes_of_a_foreign_link_carry_bytes_both_ways() {
        let ForeignEnds {
            mut to_child,
            mut from_child,
            child_reads,
            child_writes,
        } = foreign_ends().unwrap();
        let child = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut reads, mut writes) = (child_reads, child_writes);
            let mut request = [0_u8; 4];
            reads.read_exact(&mut request).unwrap();
            writes.write_all(&request).unwrap();
            writes.write_all(b"!").unwrap();
        });
        let carried = async {
            to_child.write_all(b"ping").await.unwrap();
            let mut answer = Vec::new();
            // A pipe whose other end closed reads as its end, or as broken;
            // either way what was written before is all there.
            let _ = from_child.read_to_end(&mut answer).await;
            answer
        };
        let answer = tokio::time::timeout(std::time::Duration::from_secs(60), carried)
            .await
            .expect("the pipes of a foreign link carried nothing");
        assert_eq!(answer, b"ping!");
        child.join().unwrap();
    }

    #[test]
    fn only_the_supervisors_own_status_reads_as_ended_from_outside() {
        assert!(ended_from_outside(ExitStatus::from_raw(
            ENDED_BY_SUPERVISOR
        )));
        assert!(!ended_from_outside(ExitStatus::from_raw(0)));
        assert!(!ended_from_outside(ExitStatus::from_raw(1)));
    }

    #[test]
    fn the_inherit_mark_is_set_and_cleared_on_an_open_handle() {
        let folder = std::env::temp_dir();
        let path = folder.join(format!("a2m-provider-inherit-{}", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        set_inherit(&file, true).unwrap();
        set_inherit(&file, false).unwrap();
        drop(file);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_job_object_is_made_with_its_limit() {
        new_job().unwrap();
    }
}
