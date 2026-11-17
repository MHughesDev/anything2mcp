//! Process groups on Linux and macOS.
//!
//! Every child is the leader of a new process group, so the whole tree it
//! starts can be ended as one. The group is signalled only while its leader
//! has not been reaped: until then the leader's identifier, which is the
//! group's, cannot be given to another process, so the signal can reach
//! nothing the supervisor did not start. The leader's exit is therefore
//! observed without reaping it, the group is ended, and only then is the
//! leader reaped.
//!
//! Two things differ between the systems and are handled here. macOS
//! answers "not permitted", where Linux answers success, when a group is
//! signalled in which every process has exited or is in the middle of
//! exiting. With the leader exited that answer means the group is gone. But
//! a leader in the middle of its exit, whose files are already closed, has
//! not yet exited for the call that waits for it; a browser that was asked
//! to close is in that moment when the end of its pipe is seen. So on that
//! answer, with the leader not yet seen to exit, the supervisor waits for
//! the leader's exit, for a bounded time, and signals again; the verdict is
//! in [`super::ending`]. And macOS makes a pipe and marks it
//! close-on-exec in two steps, so a child started by another thread in
//! between would inherit the pipe; every start therefore holds one lock,
//! as on Windows.
//!
//! A program is opened once, without following a link at its name, and
//! digested through that handle. On Linux it is then started from the same
//! handle, by the name the system gives an open file, so the file that runs
//! is the file that was digested, whatever is done to its path meanwhile.
//! That needs the process file system, and cannot be done for a file that
//! starts with `#!`, which the system hands to an interpreter by name; and
//! macOS has no way to start an open file at all. In those three cases the
//! program is started by its path, after a last look, under the start lock,
//! that the path still leads to the very file that was digested, unchanged:
//! the same device and file number, the same size, and the same time of its
//! last change. A replacement in the moment between that look and the start
//! is not seen.

use std::fs::File;
use std::io;
use std::os::fd::AsFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Mutex;

use command_fds::{CommandFdExt, FdMapping};
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::unix::pipe;
use tokio::process::{Child, Command};

use super::ending::{self, Answer, Leader, Verdict};
use super::{ForeignPipe, StartError};

/// The two ends of a foreign link that stay with the engine.
pub(crate) type Pipes = (
    Box<dyn AsyncWrite + Send + Unpin>,
    Box<dyn AsyncRead + Send + Unpin>,
);

/// One child and the process group it leads.
pub(crate) struct Group {
    pgid: Pid,
    /// The leader, until reaping begins. While it is here it has not been
    /// waited for, so the group's identifier is still the supervisor's.
    child: Mutex<Option<Child>>,
}

/// Held by every start of a process, from before its pipes are made until
/// the child exists. Without it, on a system that cannot make a pipe
/// already marked close-on-exec, a child started by another thread could
/// inherit this child's pipe and hold it open after this child has ended.
static START: Mutex<()> = Mutex::new(());

/// How long a run waits for the end of its program's output once the group
/// is gone: not at all. What the group wrote is in the pipe by then, and is
/// taken with [`ReadNow`]; the end of the pipe may be held back by a process
/// that is not the program's.
pub(crate) const END_OF_OUTPUT_WAIT: std::time::Duration = std::time::Duration::ZERO;

/// Reading what is in a pipe now, without waiting for more.
pub(crate) trait ReadNow {
    /// The bytes read, or `None` when nothing is there now, the pipe has
    /// ended, or it cannot be read.
    fn read_now(&self, buffer: &mut [u8]) -> Option<usize>;
}

impl<T: AsFd> ReadNow for T {
    fn read_now(&self, buffer: &mut [u8]) -> Option<usize> {
        loop {
            // The descriptor does not block: the runtime set it so when it
            // took the pipe. Nothing there is "try again", which ends this.
            match rustix::io::read(self, &mut *buffer) {
                Ok(0) => return None,
                Ok(read) => return Some(read),
                Err(error) if error == rustix::io::Errno::INTR => {}
                Err(_) => return None,
            }
        }
    }
}

/// Whether the status is that of a program ended by a signal, the
/// supervisor's or anyone's, and not by its own exit.
pub(crate) fn ended_from_outside(status: ExitStatus) -> bool {
    status.code().is_none()
}

fn other(message: &'static str) -> io::Error {
    io::Error::other(message)
}

/// How long a group that answered "nothing could be signalled" is given for
/// its leader to finish exiting before it is judged again. On macOS a
/// leader in the middle of its exit, with its files already closed, answers
/// so; that lasts moments. Only a leader the engine's user may not signal
/// waits this long, and its group is then reported as not ended.
const LEADER_EXIT_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether the leader has exited. It is not reaped. Call it only while the
/// leader is unreaped.
fn leader_exited(pgid: Pid) -> bool {
    !matches!(
        waitid(
            WaitId::Pid(pgid),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG,
        ),
        Ok(None)
    )
}

/// Resolves when the leader has exited, without reaping it, or when it can
/// no longer be waited for.
async fn leader_exit_of(pid: Pid) {
    let waited = tokio::task::spawn_blocking(move || {
        loop {
            match waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
            ) {
                Err(error) if error == rustix::io::Errno::INTR => {}
                _ => break,
            }
        }
    });
    // A failed join means the wait itself ended; either way it is over.
    let _ = waited.await;
}

/// What the system's answer to a signal to a group means. This is the one
/// place where its error numbers are read. "Not permitted" is the answer
/// both systems give when the group exists and nothing in it could be
/// signalled; on macOS that includes a group whose members have all exited
/// or are exiting, which Linux answers with success. [`ending::verdict`]
/// tells those apart by the leader.
///
/// [`ending::verdict`]: super::ending::verdict
fn answer(result: rustix::io::Result<()>) -> Answer {
    match result {
        Ok(()) => Answer::Signalled,
        Err(error) if error == rustix::io::Errno::SRCH => Answer::NoGroup,
        Err(error) if error == rustix::io::Errno::PERM => Answer::NothingSignalled,
        Err(_) => Answer::Failed,
    }
}

/// Signals every process of the group to end, and judges the answer. Call
/// it only while the leader is unreaped, so that the group is ours.
/// `waited` says whether the leader has already been waited for.
fn signal_and_judge(pgid: Pid, waited: bool) -> Verdict {
    let answer = answer(kill_process_group(pgid, Signal::KILL));
    let leader = if answer != Answer::NothingSignalled {
        // The verdict does not depend on the leader.
        Leader::NotSeenToExit
    } else if leader_exited(pgid) {
        Leader::Exited
    } else if waited {
        Leader::StayedThroughGrace
    } else {
        Leader::NotSeenToExit
    };
    ending::verdict(answer, leader)
}

/// What tells a file from every other, and one state of its content from
/// the state before: where it is kept, how long it is, and when its content
/// last changed. The time its record last changed is left out on purpose:
/// it moves when a second name is given to the file or its mode is set,
/// which change nothing that runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    device: u64,
    file: u64,
    size: u64,
    modified: (i64, i64),
}

impl Stamp {
    fn of(meta: &std::fs::Metadata) -> Self {
        Self {
            device: meta.dev(),
            file: meta.ino(),
            size: meta.size(),
            modified: (meta.mtime(), meta.mtime_nsec()),
        }
    }
}

/// A program opened once, to be digested and then started.
pub(crate) struct Program {
    /// The path it was asked for by.
    named: PathBuf,
    file: File,
    stamp: Stamp,
    /// Whether it starts with `#!`. Known once it has been digested.
    script: bool,
}

/// What to start, and what must stay open until the child exists.
struct Start {
    path: PathBuf,
    _held: Option<std::os::fd::OwnedFd>,
}

impl Program {
    /// Opens the file at `real`, the resolved form of `named`. A link at
    /// the name is not followed, and only a regular file is taken.
    pub(crate) fn open(named: &Path, real: &Path) -> io::Result<Self> {
        use rustix::fs::{Mode, OFlags};
        // Not blocking, so that a pipe put at the name cannot hold the
        // start; reading a regular file is not changed by it.
        let handle = rustix::fs::open(
            real,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )?;
        let file = File::from(handle);
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(other("a program is a regular file"));
        }
        Ok(Self {
            named: named.to_path_buf(),
            file,
            stamp: Stamp::of(&meta),
            script: false,
        })
    }

    /// The digest of the open file. An error when the file changed while
    /// it was read.
    pub(crate) fn digest(&mut self) -> io::Result<a2m_core::Digest> {
        let (digest, script) = super::digest_read(&mut self.file)?;
        if Stamp::of(&self.file.metadata()?) != self.stamp {
            return Err(other("the program changed while it was read"));
        }
        self.script = script;
        Ok(digest)
    }

    /// The open file under the name the system gives it, where a child can
    /// be started from that name. The copy of the handle is numbered above
    /// `clear_of`, the numbers the child's own pipes will take.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn by_handle(&self, clear_of: i32) -> Option<Start> {
        use std::os::fd::AsRawFd;
        if self.script {
            return None;
        }
        let held = rustix::io::fcntl_dupfd_cloexec(&self.file, clear_of.max(2) + 1).ok()?;
        let path = PathBuf::from(format!("/proc/self/fd/{}", held.as_raw_fd()));
        // Without the process file system there is no such name.
        path.exists().then_some(Start {
            path,
            _held: Some(held),
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn by_handle(&self, _clear_of: i32) -> Option<Start> {
        None
    }

    /// What to start. Call it under the start lock, immediately before the
    /// start.
    fn start(&self, clear_of: i32) -> Result<Start, StartError> {
        if let Some(start) = self.by_handle(clear_of) {
            return Ok(start);
        }
        // By its path: only while the path leads to the file that was
        // digested, and that file has not been changed since.
        let unchanged = std::fs::metadata(&self.named)
            .is_ok_and(|now| Stamp::of(&now) == self.stamp)
            && self
                .file
                .metadata()
                .is_ok_and(|now| Stamp::of(&now) == self.stamp);
        if unchanged {
            Ok(Start {
                path: self.named.clone(),
                _held: None,
            })
        } else {
            Err(StartError::Changed)
        }
    }
}

/// Starts `program` as the leader of a new process group, with the command
/// `command` builds for the path it is given. With `foreign`, the child
/// receives the two pipe ends on the descriptors it names.
pub(crate) fn spawn(
    program: &Program,
    command: impl FnOnce(&Path) -> Command,
    foreign: Option<&ForeignPipe>,
) -> Result<(Group, Option<Pipes>), StartError> {
    let _one_start = START
        .lock()
        .map_err(|_| other("the start lock is broken"))?;
    let clear_of = foreign.map_or(2, |pipe| pipe.child_reads_fd.max(pipe.child_writes_fd));
    let start = program.start(clear_of)?;
    let mut command = command(&start.path);
    // The program sees the name it was asked for by, however it is started.
    command.arg0(&program.named);
    command.process_group(0);
    let pipes = match foreign {
        None => None,
        Some(pipe_spec) => {
            let (to_child, child_reads) = pipe::pipe()?;
            let (child_writes, from_child) = pipe::pipe()?;
            command.stdin(Stdio::null()).stdout(Stdio::null());
            command
                .fd_mappings(vec![
                    FdMapping {
                        parent_fd: child_reads.into_blocking_fd()?,
                        child_fd: pipe_spec.child_reads_fd,
                    },
                    FdMapping {
                        parent_fd: child_writes.into_blocking_fd()?,
                        child_fd: pipe_spec.child_writes_fd,
                    },
                ])
                .map_err(|_| other("the two ends of the pipe name one descriptor"))?;
            let pipes: Pipes = (Box::new(to_child), Box::new(from_child));
            Some(pipes)
        }
    };
    let child = command.spawn()?;
    // The command holds this process's copies of the child's pipe ends.
    // Until it is dropped a dead child would not show as the end of the pipe.
    drop(command);
    // The child exists: the copy of the program's handle it was started
    // from is no longer needed.
    drop(start);
    let pgid = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
        .ok_or_else(|| other("the child has no process identifier"))?;
    Ok((
        Group {
            pgid,
            child: Mutex::new(Some(child)),
        },
        pipes,
    ))
}

impl Group {
    /// The leader's process identifier.
    pub(crate) fn id(&self) -> u32 {
        u32::try_from(self.pgid.as_raw_nonzero().get()).unwrap_or(0)
    }

    /// Ends every process of the group, at once and without waiting. Does
    /// nothing once reaping began, because the group's identifier is then
    /// no longer provably ours. Returns false when the group could not be
    /// ended, or when that is not known yet; [`Group::end`] waits to know.
    pub(crate) fn kill(&self) -> bool {
        self.judge(false)
            .is_none_or(|verdict| verdict == Verdict::Ended)
    }

    /// Ends every process of the group, without reaping the leader, and
    /// says whether the group is ended. Where that is not known at once,
    /// it waits for the leader's exit, for at most the grace, and signals
    /// again. Once reaping began elsewhere, whoever reaps gives the verdict,
    /// and this says the group is ended.
    pub(crate) async fn end(&self) -> bool {
        match self.judge(false) {
            None => return true,
            Some(Verdict::WaitForLeader) => {}
            Some(verdict) => return verdict == Verdict::Ended,
        }
        let _ = tokio::time::timeout(LEADER_EXIT_GRACE, leader_exit_of(self.pgid)).await;
        self.judge(true)
            .is_none_or(|verdict| verdict == Verdict::Ended)
    }

    /// Signals the group and judges the answer, while the leader has not
    /// been taken for reaping. `None` once it has.
    fn judge(&self, waited: bool) -> Option<Verdict> {
        let Ok(child) = self.child.lock() else {
            return Some(Verdict::NotEnded);
        };
        child.as_ref()?;
        Some(signal_and_judge(self.pgid, waited))
    }

    /// Whether the leader has exited. It is not reaped.
    pub(crate) fn exited(&self) -> bool {
        let Ok(child) = self.child.lock() else {
            return true;
        };
        if child.is_none() {
            return true;
        }
        leader_exited(self.pgid)
    }

    /// Resolves when the leader has exited, without reaping it.
    pub(crate) async fn leader_exit(&self) {
        leader_exit_of(self.pgid).await;
    }

    /// Takes the child's standard streams, where it was given pipes.
    pub(crate) fn take_streams(
        &self,
    ) -> (
        Option<tokio::process::ChildStdin>,
        Option<tokio::process::ChildStdout>,
        Option<tokio::process::ChildStderr>,
    ) {
        match self.child.lock() {
            Ok(mut child) => match child.as_mut() {
                Some(child) => (child.stdin.take(), child.stdout.take(), child.stderr.take()),
                None => (None, None, None),
            },
            Err(_) => (None, None, None),
        }
    }

    /// Ends the group and reaps the leader. The status is `None` when the
    /// leader was reaped before, or cannot be waited for. The flag is false
    /// when the group could not be ended.
    pub(crate) async fn reap(&self) -> (Option<ExitStatus>, bool) {
        let taken = match self.child.lock() {
            Ok(mut child) => child.take(),
            Err(_) => return (None, false),
        };
        let Some(mut child) = taken else {
            return (None, true);
        };
        // The leader is held here and unreaped until it is waited for
        // below, so the group is ours throughout. The leader is reaped
        // whatever the verdict, and is waited for in any case.
        let mut verdict = signal_and_judge(self.pgid, false);
        if verdict == Verdict::WaitForLeader {
            let _ = tokio::time::timeout(LEADER_EXIT_GRACE, leader_exit_of(self.pgid)).await;
            verdict = signal_and_judge(self.pgid, true);
        }
        (child.wait().await.ok(), verdict == Verdict::Ended)
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        // A group that was never reaped is ended here. The runtime reaps
        // the dropped leader in the background.
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use rustix::io::Errno;

    use super::{Answer, answer, ended_from_outside};

    #[test]
    fn each_answer_of_the_system_to_a_signal_is_read_as_one_meaning() {
        assert_eq!(answer(Ok(())), Answer::Signalled);
        assert_eq!(answer(Err(Errno::SRCH)), Answer::NoGroup);
        assert_eq!(answer(Err(Errno::PERM)), Answer::NothingSignalled);
        assert_eq!(answer(Err(Errno::INVAL)), Answer::Failed);
    }

    #[test]
    fn a_signal_reads_as_ended_from_outside_and_an_exit_does_not() {
        // A wait status: a signal number in the low bits, an exit status
        // in the next byte.
        assert!(ended_from_outside(ExitStatus::from_raw(9)));
        assert!(!ended_from_outside(ExitStatus::from_raw(0)));
        assert!(!ended_from_outside(ExitStatus::from_raw(1 << 8)));
    }
}
