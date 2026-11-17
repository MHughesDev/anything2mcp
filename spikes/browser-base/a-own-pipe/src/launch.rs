//! Throwaway spike code. Starting the browser with its protocol on a pipe.
//!
//! On POSIX systems the browser reads requests on file descriptor 3 and writes
//! replies on file descriptor 4. On Windows it takes two inherited pipe handles,
//! named by value in `--remote-debugging-io-pipes=<read>,<write>`.
//!
//! Proven by running: the two POSIX ways below, on Linux. Only compiled, for
//! `x86_64-pc-windows-msvc`: the Windows way.

use std::path::Path;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, Command};

pub struct Launched {
    pub child: Child,
    pub to_browser: Box<dyn AsyncWrite + Send + Unpin>,
    pub from_browser: Box<dyn AsyncRead + Send + Unpin>,
}

/// How the pipe ends reach the child.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handoff {
    /// A helper crate maps the descriptors between fork and exec. No shell.
    /// The `unsafe` is inside the helper crate; none is ours.
    #[cfg(unix)]
    MappedFds,
    /// No `unsafe` anywhere and the standard library only: a shell moves the
    /// child's standard input and output to descriptors 3 and 4, then becomes
    /// the browser with `exec`. Arguments stay a list.
    #[cfg(unix)]
    ShellExec,
    /// Two named pipes; the child's ends are marked inheritable for the start.
    #[cfg(windows)]
    InheritedHandles,
}

pub fn launch(program: &Path, args: &[String], handoff: Handoff) -> std::io::Result<Launched> {
    match handoff {
        #[cfg(unix)]
        Handoff::MappedFds => unix::mapped_fds(program, args),
        #[cfg(unix)]
        Handoff::ShellExec => unix::shell_exec(program, args),
        #[cfg(windows)]
        Handoff::InheritedHandles => windows::inherited_handles(program, args),
    }
}

fn common(command: &mut Command) {
    command
        .arg("--remote-debugging-pipe")
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    // Its own process group, so that the whole tree can be ended as one. Ending
    // the group is the supervisor's job in the product; the spike only sets it.
    #[cfg(unix)]
    command.process_group(0);
}

#[cfg(unix)]
mod unix {
    use std::process::Stdio;

    use command_fds::{CommandFdExt, FdMapping};
    use tokio::net::unix::pipe;

    use super::*;

    pub fn mapped_fds(program: &Path, args: &[String]) -> std::io::Result<Launched> {
        let (to_browser, browser_reads) = pipe::pipe()?;
        let (browser_writes, from_browser) = pipe::pipe()?;
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        common(&mut command);
        command
            .fd_mappings(vec![
                FdMapping {
                    parent_fd: browser_reads.into_blocking_fd()?,
                    child_fd: 3,
                },
                FdMapping {
                    parent_fd: browser_writes.into_blocking_fd()?,
                    child_fd: 4,
                },
            ])
            .map_err(|_| std::io::Error::other("two mappings name one descriptor"))?;
        let child = command.spawn()?;
        // The command holds this process's copies of the child's ends. Until it
        // is dropped they stay open here, and a dead browser would not show as
        // the end of the pipe.
        drop(command);
        Ok(Launched {
            child,
            to_browser: Box::new(to_browser),
            from_browser: Box::new(from_browser),
        })
    }

    pub fn shell_exec(program: &Path, args: &[String]) -> std::io::Result<Launched> {
        let mut command = Command::new("/bin/sh");
        // "$0" is the browser program and "$@" its arguments: nothing is parsed as shell text.
        command
            .arg("-c")
            .arg(r#"exec "$0" "$@" 3<&0 4>&1 0</dev/null 1>/dev/null"#)
            .arg(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        common(&mut command);
        let mut child = command.spawn()?;
        let to_browser = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("no pipe"))?;
        let from_browser = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no pipe"))?;
        Ok(Launched {
            child,
            to_browser: Box::new(to_browser),
            from_browser: Box::new(from_browser),
        })
    }
}

#[cfg(windows)]
mod windows {
    use std::fs::{File, OpenOptions};
    use std::os::windows::io::AsRawHandle;
    use std::sync::Mutex;

    use tokio::net::windows::named_pipe::ServerOptions;
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};

    use super::*;

    /// One browser start at a time. While the flag is set, any other process
    /// this program starts would inherit the handles too, so in the product
    /// every process start, of any kind, must hold this lock.
    static START: Mutex<()> = Mutex::new(());

    fn set_inherit(file: &File, on: bool) -> std::io::Result<()> {
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
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn inherited_handles(program: &Path, args: &[String]) -> std::io::Result<Launched> {
        // The name is not a secret: each pipe allows one instance and one
        // client, and this process is that client before the browser starts.
        static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let unique = format!("{}-{count}", std::process::id());
        let down = format!(r"\\.\pipe\a2m-spike-{unique}-to-browser");
        let up = format!(r"\\.\pipe\a2m-spike-{unique}-from-browser");
        // This process keeps the server ends, which Tokio reads and writes
        // without blocking. The browser gets the client ends, opened by the
        // standard library for ordinary blocking use, which is what it expects.
        let to_browser = ServerOptions::new()
            .first_pipe_instance(true)
            .max_instances(1)
            .access_inbound(false)
            .access_outbound(true)
            .create(&down)?;
        let from_browser = ServerOptions::new()
            .first_pipe_instance(true)
            .max_instances(1)
            .access_inbound(true)
            .access_outbound(false)
            .create(&up)?;
        let browser_reads = OpenOptions::new().read(true).open(&down)?;
        let browser_writes = OpenOptions::new().write(true).open(&up)?;

        let mut command = Command::new(program);
        command.args(args).arg(format!(
            "--remote-debugging-io-pipes={},{}",
            browser_reads.as_raw_handle() as usize,
            browser_writes.as_raw_handle() as usize
        ));
        common(&mut command);
        let child = {
            let _one_start = START.lock().map_err(|_| std::io::Error::other("lock"))?;
            set_inherit(&browser_reads, true)?;
            set_inherit(&browser_writes, true)?;
            let child = command.spawn();
            set_inherit(&browser_reads, false)?;
            set_inherit(&browser_writes, false)?;
            child?
        };
        // Close this process's copies of the browser's ends, so that a dead
        // browser shows as the end of the pipe.
        drop((browser_reads, browser_writes));
        Ok(Launched {
            child,
            to_browser: Box::new(to_browser),
            from_browser: Box::new(from_browser),
        })
    }
}
