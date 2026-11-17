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
//! signalled in which nothing is left alive but its exited leader; that
//! answer means the group is gone. And macOS makes a pipe and marks it
//! close-on-exec in two steps, so a child started by another thread in
//! between would inherit the pipe; every start therefore holds one lock,
//! as on Windows.

use std::io;
use std::os::fd::AsFd;
use std::process::{ExitStatus, Stdio};
use std::sync::Mutex;

use command_fds::{CommandFdExt, FdMapping};
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::unix::pipe;
use tokio::process::{Child, Command};

use super::ForeignPipe;

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

fn other(message: &'static str) -> io::Error {
    io::Error::other(message)
}

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

/// Ends every process of the group, and says whether the group is gone.
/// Call it only while the leader is unreaped, so that the group is ours.
fn end_group(pgid: Pid) -> bool {
    match kill_process_group(pgid, Signal::KILL) {
        // Signalled, or no such group: nothing is left to end.
        Ok(()) => true,
        Err(error) if error == rustix::io::Errno::SRCH => true,
        // macOS answers "not permitted" when the group holds nothing it can
        // signal, and it does not count an exited process. With the leader
        // exited that is the ordinary end of a program that left nothing
        // running. Linux answers success there, and never this for a group
        // whose leader is our own child. With the leader alive it would
        // mean the group cannot be ended, and is reported as that.
        Err(error) if error == rustix::io::Errno::PERM => leader_exited(pgid),
        Err(_) => false,
    }
}

/// Starts `command` as the leader of a new process group. With `foreign`,
/// the child receives the two pipe ends on the descriptors it names.
pub(crate) fn spawn(
    mut command: Command,
    foreign: Option<&ForeignPipe>,
) -> io::Result<(Group, Option<Pipes>)> {
    let _one_start = START
        .lock()
        .map_err(|_| other("the start lock is broken"))?;
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

    /// Ends every process of the group. Does nothing once reaping began,
    /// because the group's identifier is then no longer provably ours.
    /// Returns false when the group could not be signalled.
    pub(crate) fn kill(&self) -> bool {
        let Ok(child) = self.child.lock() else {
            return false;
        };
        if child.is_none() {
            return true;
        }
        end_group(self.pgid)
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
        let pid = self.pgid;
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
    /// when the group could not be signalled.
    pub(crate) async fn reap(&self) -> (Option<ExitStatus>, bool) {
        let (taken, ended) = {
            let Ok(mut child) = self.child.lock() else {
                return (None, false);
            };
            // The leader is unreaped while it is here, so the group is ours.
            let ended = child.is_none() || end_group(self.pgid);
            (child.take(), ended)
        };
        match taken {
            Some(mut child) => (child.wait().await.ok(), ended),
            None => (None, ended),
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        // A group that was never reaped is ended here. The runtime reaps
        // the dropped leader in the background.
        self.kill();
    }
}
