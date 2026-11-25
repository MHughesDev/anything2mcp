//! What differs between operating systems: the socket or the named pipe,
//! who is on its other end, and who owns it.
//!
//! These modules use the standard library, Tokio, and the operating
//! system's own calls, and nothing of the engine, so that each can be
//! compiled and linted for its own system alone.

use std::io;
use std::path::PathBuf;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{
    Listener, ServerStream, connect, current_user, kept, prepare_dir, spawn_detached,
};

/// The longest path of a socket this system takes, for the tests.
#[cfg(all(unix, test))]
pub(crate) use unix::longest_socket_path as unix_longest_socket_path;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{
    Listener, ServerStream, connect, current_user, open_output, prepare_dir, spawn_detached,
};

/// The test build's gate on the children this process starts. A child
/// shares every file this process has open from the moment it is forked
/// until it runs its program, which closes them; for that moment it holds
/// every file lock this process holds, the engine lock among them, after
/// this process has let go. An engine is one process and never takes its
/// own lock again, so this matters only where many tests share one
/// process: a test that lets a lock go and asks for it again holds the
/// gate alone, and every start of a child in the tests holds it shared,
/// so that no child is between its fork and its program while the lock is
/// asked for.
#[cfg(test)]
pub(crate) mod children {
    use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

    static GATE: RwLock<()> = RwLock::new(());

    /// Held while a child is started, until the start has returned: by
    /// then the child runs its own program, and the files it shared are
    /// closed in it.
    pub(crate) fn starting() -> RwLockReadGuard<'static, ()> {
        GATE.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Held while a test lets a lock go and asks for it again: once it is
    /// held, no child is between its fork and its program, and none starts.
    pub(crate) fn none_starting() -> RwLockWriteGuard<'static, ()> {
        GATE.write().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The file name of the socket inside the runtime directory.
#[cfg(unix)]
const SOCKET_NAME: &str = "engine.sock";

/// Where the channel is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    /// The runtime directory, which holds the socket and the lock files.
    pub(crate) dir: PathBuf,
    /// The socket's file.
    #[cfg(unix)]
    pub(crate) socket: PathBuf,
    /// The pipe's name.
    #[cfg(windows)]
    pub(crate) pipe: String,
}

impl Place {
    /// The channel in `dir`. On Windows the pipe's name carries the user's
    /// security identifier and `tag`, which tells one runtime directory
    /// from another.
    #[cfg(unix)]
    pub(crate) fn new(dir: PathBuf, _user: &str, _tag: &str) -> Self {
        Self {
            socket: dir.join(SOCKET_NAME),
            dir,
        }
    }

    /// The channel in `dir`. On Windows the pipe's name carries the user's
    /// security identifier and `tag`, which tells one runtime directory
    /// from another.
    #[cfg(windows)]
    pub(crate) fn new(dir: PathBuf, user: &str, tag: &str) -> Self {
        Self {
            dir,
            pipe: format!(r"\\.\pipe\anything2mcp.{user}.{tag}"),
        }
    }
}

impl Place {
    /// When the socket's path is longer than this system's address of a
    /// socket holds: the path, its length, and the longest the system
    /// takes, both in bytes. A socket of that path can be neither bound nor
    /// reached, so both ends ask this first and stop by name. Never on
    /// Windows, whose pipe has a name of a fixed length in the system's
    /// own name space.
    pub(crate) fn too_long(&self) -> Option<(&std::path::Path, usize, usize)> {
        #[cfg(unix)]
        {
            let length = self.socket.as_os_str().len();
            let longest = unix::longest_socket_path();
            (length > longest).then_some((self.socket.as_path(), length, longest))
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
}

/// Who is on the other end of an accepted connection, as the operating
/// system reports it. A user that could not be read is `None`, which equals
/// nobody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerId {
    pub(crate) user: Option<String>,
    pub(crate) process: Option<u32>,
}

/// Who may change a file or a folder, as far as this build can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    /// Only this process's user, or the administrator.
    Private,
    /// Others may change it too; the words say who.
    Open(&'static str),
    /// This build does not read who may change a file on this operating
    /// system: Windows, where the owner and the access list would have to
    /// be read and are not yet.
    NotChecked,
}

/// Who may change the file or folder at `path`. Its parts above it are not
/// looked at; a caller that walks a path asks for each folder.
///
/// # Errors
///
/// What the operating system answers when the path cannot be read.
pub fn kept_by(path: &std::path::Path) -> io::Result<Kept> {
    #[cfg(unix)]
    {
        kept(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path).map(|_| Kept::NotChecked)
    }
}

/// Why a client did not reach the engine.
#[derive(Debug)]
pub(crate) enum ConnectError {
    /// Nothing listens: no socket or pipe, or a stale socket.
    NotRunning,
    /// Something listens, and it is not the expected user's.
    NotOurs,
    /// Every instance of the pipe stayed taken until the caller's patience
    /// ended: an engine holds its channel and does not take connections
    /// yet, or holds as many as it can.
    #[cfg(windows)]
    Busy,
    /// The connection was made and the engine closed it before taking it:
    /// an engine that stopped serving. Nothing was sent.
    #[cfg(unix)]
    Lost,
    /// The operating system refused for another reason.
    Io(io::Error),
}

/// The longest one of the system's waits for a free instance of the pipe
/// may be. A wait cannot be called off once it has begun, so a client whose
/// patience ends while it waits holds a thread no longer than this.
#[cfg(any(windows, test))]
const LONGEST_WAIT: std::time::Duration = std::time::Duration::from_millis(200);

/// How long the next of the system's waits for a free instance of the pipe
/// may be, in whole milliseconds, for a client whose patience ends at
/// `deadline`: what remains of it, no more than [`LONGEST_WAIT`], and never
/// zero, which would ask the system for the pipe's own default instead.
/// `None` once the deadline has come: the client stops waiting.
#[cfg(any(windows, test))]
pub(crate) fn next_wait(now: tokio::time::Instant, deadline: tokio::time::Instant) -> Option<u32> {
    let left = deadline
        .checked_duration_since(now)
        .filter(|left| !left.is_zero())?;
    let wait = left.min(LONGEST_WAIT);
    // At most `LONGEST_WAIT`, so it fits; a part of a millisecond is waited
    // as a whole one.
    let millis = u32::try_from(wait.as_micros().div_ceil(1000)).unwrap_or(u32::MAX);
    Some(millis.max(1))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;

    use super::{LONGEST_WAIT, next_wait};

    #[test]
    fn a_wait_for_the_pipe_ends_with_the_callers_patience_and_is_never_zero() {
        let now = Instant::now();
        // Patience left beyond one wait: one whole wait, then the next.
        assert_eq!(
            next_wait(now, now + Duration::from_secs(30)),
            Some(u32::try_from(LONGEST_WAIT.as_millis()).unwrap_or(u32::MAX))
        );
        // Less left than one wait: what is left, to the millisecond above.
        assert_eq!(next_wait(now, now + Duration::from_millis(37)), Some(37));
        assert_eq!(
            next_wait(now, now + Duration::from_micros(37_200)),
            Some(38)
        );
        // A part of a millisecond is one, never zero: zero would ask the
        // system for the pipe's default wait, which is not the caller's.
        assert_eq!(next_wait(now, now + Duration::from_micros(1)), Some(1));
        // At the deadline and after it, no wait: the client stops.
        assert_eq!(next_wait(now, now), None);
        assert_eq!(next_wait(now + Duration::from_millis(5), now), None);
    }
}
