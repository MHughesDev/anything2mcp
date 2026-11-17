//! Job objects on Windows.
//!
//! Every child is placed in a job object of its own, set to end every
//! process in it when the job's last handle closes. The supervisor holds
//! that handle, so the tree ends when the supervisor ends it, and also when
//! the engine itself dies. Membership of the job is the proof of ownership:
//! ending the job reaches only what was started in it.
//!
//! The standard library starts every child with handle inheritance on. A
//! foreign link marks two handles inheritable for the moment of one start,
//! so every start of a process takes the one lock below; a second child
//! started at that moment would otherwise inherit the first child's pipe.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::ExitStatus;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::ServerOptions;
use tokio::process::{Child, Command};
use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

use super::ForeignPipe;

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

/// Starts `command` inside a new job object. With `foreign`, the child
/// inherits the two pipe ends, named by value in one added argument.
#[allow(unsafe_code)] // one call into the operating system, explained below
pub(crate) fn spawn(
    mut command: Command,
    foreign: Option<&ForeignPipe>,
) -> io::Result<(Group, Option<Pipes>)> {
    let job = new_job()?;
    let mut child_ends = None;
    let pipes = match foreign {
        None => None,
        Some(pipe_spec) => {
            // The name is not a secret: each pipe allows one instance and
            // one client, and this process is that client before the child
            // starts.
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
    let child = {
        let _one_start = START
            .lock()
            .map_err(|_| other("the start lock is broken"))?;
        if let Some((reads, writes)) = &child_ends {
            set_inherit(reads, true)?;
            set_inherit(writes, true)?;
        }
        let started = command.spawn();
        if let Some((reads, writes)) = &child_ends {
            set_inherit(reads, false)?;
            set_inherit(writes, false)?;
        }
        started?
    };
    // Close this process's copies of the child's ends, so that a dead child
    // shows as the end of the pipe.
    drop(child_ends);
    let process = child
        .raw_handle()
        .ok_or_else(|| other("the child has no process handle"))?;
    // SAFETY: both handles are open: the job is owned by `job`, and the
    // process handle by `child`, which is alive here. The call touches no
    // memory of ours.
    let ok = unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) };
    if ok == 0 {
        let error = io::Error::last_os_error();
        // The child is not in the job, so it is ended on its own.
        let mut child = child;
        let _ = child.start_kill();
        return Err(error);
    }
    let id = child.id().unwrap_or(0);
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

    use super::{ENDED_BY_SUPERVISOR, ended_from_outside, new_job, set_inherit};

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
