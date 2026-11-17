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
pub(crate) use unix::{ClientStream, Listener, ServerStream, connect, current_user, prepare_dir};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{
    ClientStream, Listener, ServerStream, connect, current_user, prepare_dir,
};

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

/// Who is on the other end of an accepted connection, as the operating
/// system reports it. A user that could not be read is `None`, which equals
/// nobody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerId {
    pub(crate) user: Option<String>,
    pub(crate) process: Option<u32>,
}

/// Why a client did not reach the engine.
#[derive(Debug)]
pub(crate) enum ConnectError {
    /// Nothing listens: no socket or pipe, or a stale socket.
    NotRunning,
    /// Something listens, and it is not the expected user's.
    NotOurs,
    /// The operating system refused for another reason.
    Io(io::Error),
}
