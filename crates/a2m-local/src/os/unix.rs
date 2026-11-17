//! The channel on macOS and Linux: a socket in a directory only the user
//! can enter, the peer credentials the kernel reports, and the owner of
//! the socket's file.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

use super::{ConnectError, PeerId, Place};

/// The engine's end of one connection.
pub(crate) type ServerStream = UnixStream;
/// The client's end of one connection.
pub(crate) type ClientStream = UnixStream;

/// The directory's mode: the user alone.
const DIR_MODE: u32 = 0o700;
/// The socket's mode: the user alone.
const SOCKET_MODE: u32 = 0o600;
/// The mode bits of everyone but the owner.
const OTHERS: u32 = 0o077;

/// The user this process runs as, as the text of its user id. It cannot
/// fail here; the signature is the one Windows needs, where it can.
pub(crate) fn current_user() -> io::Result<String> {
    Ok(rustix::process::getuid().as_raw().to_string())
}

/// Makes the runtime directory with mode 0700, or checks that the one
/// there is a directory of this user's, and closes it to everyone else.
pub(crate) fn prepare_dir(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::DirBuilder::new().mode(DIR_MODE).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let found = fs::symlink_metadata(dir)?;
    if !found.is_dir() || found.uid().to_string() != current_user()? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the runtime directory is not a directory of this user's",
        ));
    }
    if found.mode() & OTHERS != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE))?;
    }
    Ok(())
}

/// The engine's listener. The socket's file is removed when it is dropped.
pub(crate) struct Listener {
    inner: UnixListener,
    socket: PathBuf,
}

impl Listener {
    /// Binds the socket. The caller holds the engine lock, so a socket
    /// file found here is stale and is removed first.
    pub(crate) fn bind(place: &Place) -> io::Result<Self> {
        match fs::remove_file(&place.socket) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let inner = UnixListener::bind(&place.socket)?;
        // The directory already keeps everyone else out; the socket's own
        // mode says the same.
        fs::set_permissions(&place.socket, fs::Permissions::from_mode(SOCKET_MODE))?;
        Ok(Self {
            inner,
            socket: place.socket.clone(),
        })
    }

    /// The next connection, with the peer the kernel reports for it.
    /// Nothing is read from the connection.
    pub(crate) async fn accept(&mut self) -> io::Result<(ServerStream, PeerId)> {
        let (stream, _) = self.inner.accept().await?;
        let peer = stream.peer_cred().map_or(
            PeerId {
                user: None,
                process: None,
            },
            |cred| PeerId {
                user: Some(cred.uid().to_string()),
                process: cred.pid().and_then(|id| u32::try_from(id).ok()),
            },
        );
        Ok((stream, peer))
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

/// The client's check of the engine: the directory and the socket's file
/// belong to `expected`, and nobody else can change the directory.
fn check_owner(place: &Place, expected: &str) -> Result<(), ConnectError> {
    let read = |path: &Path| match fs::symlink_metadata(path) {
        Ok(found) => Ok(found),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(ConnectError::NotRunning),
        Err(error) => Err(ConnectError::Io(error)),
    };
    let dir = read(&place.dir)?;
    let socket = read(&place.socket)?;
    let ours = dir.is_dir()
        && dir.uid().to_string() == expected
        && dir.mode() & OTHERS == 0
        && socket.file_type().is_socket()
        && socket.uid().to_string() == expected;
    if ours {
        Ok(())
    } else {
        Err(ConnectError::NotOurs)
    }
}

/// Connects to the engine, after checking that what listens is the
/// expected user's. Nothing is sent.
pub(crate) async fn connect(place: &Place, expected: &str) -> Result<ClientStream, ConnectError> {
    let checked = {
        let place = place.clone();
        let expected = expected.to_owned();
        tokio::task::spawn_blocking(move || check_owner(&place, &expected)).await
    };
    match checked {
        Ok(result) => result?,
        Err(_) => {
            return Err(ConnectError::Io(io::Error::other(
                "the owner check stopped",
            )));
        }
    }
    match UnixStream::connect(&place.socket).await {
        Ok(stream) => Ok(stream),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            Err(ConnectError::NotRunning)
        }
        Err(error) => Err(ConnectError::Io(error)),
    }
}
