//! Process groups on Linux and macOS.
//!
//! Every child is the leader of a new process group, so the whole tree it
//! starts can be ended as one. The group is signalled only while its leader
//! has not been reaped: until then the leader's identifier, which is the
//! group's, cannot be given to another process, so the signal can reach
//! nothing the supervisor did not start. The leader's exit is therefore
//! observed without reaping it, the group is ended, and only then is the
//! leader reaped.

use std::io;
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

fn other(message: &'static str) -> io::Error {
    io::Error::other(message)
}

/// Starts `command` as the leader of a new process group. With `foreign`,
/// the child receives the two pipe ends on the descriptors it names.
pub(crate) fn spawn(
    mut command: Command,
    foreign: Option<&ForeignPipe>,
) -> io::Result<(Group, Option<Pipes>)> {
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
        // ESRCH means nothing is left to end, which is what was asked.
        match kill_process_group(self.pgid, Signal::KILL) {
            Ok(()) => true,
            Err(error) => error == rustix::io::Errno::SRCH,
        }
    }

    /// Whether the leader has exited. It is not reaped.
    pub(crate) fn exited(&self) -> bool {
        let Ok(child) = self.child.lock() else {
            return true;
        };
        if child.is_none() {
            return true;
        }
        !matches!(
            waitid(
                WaitId::Pid(self.pgid),
                WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG,
            ),
            Ok(None)
        )
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
            let ended = if child.is_some() {
                // The leader is unreaped here, so the group is still ours.
                match kill_process_group(self.pgid, Signal::KILL) {
                    Ok(()) => true,
                    Err(error) => error == rustix::io::Errno::SRCH,
                }
            } else {
                true
            };
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
