//! The channel on macOS and Linux: a socket in a directory only the user
//! can enter, the peer credentials the kernel reports, and the owner of
//! the socket's file.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

use super::{ConnectError, Kept, PeerId, Place};

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
///
/// The folder is checked and closed as one object: it is opened without
/// following a link at its end, and the opened handle's own type and owner
/// are checked and its mode set through that handle. A link put in the
/// folder's place is refused, never followed, and a folder swapped in after
/// the open is not the one that is closed (decision 0273).
pub(crate) fn prepare_dir(dir: &Path) -> io::Result<()> {
    use rustix::fs::{Mode, OFlags};

    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::DirBuilder::new().mode(DIR_MODE).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let not_ours = || {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the runtime directory is not a directory of this user's",
        )
    };
    // `NOFOLLOW` refuses a link at the path's end; `DIRECTORY` refuses
    // anything but a folder, so a pipe or a device is never opened.
    let handle = match rustix::fs::open(
        dir,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(handle) => handle,
        Err(error) if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR => {
            return Err(not_ours());
        }
        Err(error) => return Err(error.into()),
    };
    let found = rustix::fs::fstat(&handle)?;
    let is_dir =
        rustix::fs::FileType::from_raw_mode(found.st_mode) == rustix::fs::FileType::Directory;
    if !is_dir || found.st_uid != rustix::process::getuid().as_raw() {
        return Err(not_ours());
    }
    // The mode bits of everyone but the owner ([`OTHERS`]); the folder's
    // mode becomes the user's alone ([`DIR_MODE`]).
    if Mode::from_raw_mode(found.st_mode).intersects(Mode::RWXG | Mode::RWXO) {
        rustix::fs::fchmod(&handle, Mode::RWXU)?;
    }
    Ok(())
}

/// The longest path, in bytes, that this system's address of a socket
/// holds: the size of the path in its `sockaddr_un`, less the one byte that
/// ends it (103 on macOS and the BSDs, 107 on Linux). It is found by the
/// standard library's own check of that address, the check that binding
/// and connecting make, and never written down here.
pub(crate) fn longest_socket_path() -> usize {
    static LONGEST: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *LONGEST.get_or_init(|| {
        let fits = |length: usize| {
            let path = format!("/{}", "x".repeat(length.saturating_sub(1)));
            std::os::unix::net::SocketAddr::from_pathname(path).is_ok()
        };
        // A path of one byte fits every system's address, and one of a
        // page fits none; between them, the longest that fits.
        let (mut fits_up_to, mut too_long) = (1_usize, 4096_usize);
        while too_long - fits_up_to > 1 {
            let middle = fits_up_to + (too_long - fits_up_to) / 2;
            if fits(middle) {
                fits_up_to = middle;
            } else {
                too_long = middle;
            }
        }
        fits_up_to
    })
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
/// expected user's: the socket's file and folder before it connects, and
/// the kernel's word on who serves the connected socket after. Nothing is
/// sent. A socket queues a connection the
/// engine has not taken yet, so nothing here waits for `_deadline`.
pub(crate) async fn connect(
    place: &Place,
    expected: &str,
    _deadline: tokio::time::Instant,
) -> Result<ClientStream, ConnectError> {
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
    let stream = reached(UnixStream::connect(&place.socket).await)?;
    let server = match stream.peer_cred() {
        Ok(cred) => Some(cred.uid().to_string()),
        // The engine closed the connection before the question: on macOS
        // the kernel then has no peer to name. That is an engine that went
        // away with nothing sent, as `reached` says of a reset, and is
        // trusted no more than before.
        Err(error) if gone(&error) => return Err(ConnectError::Lost),
        Err(_) => None,
    };
    served_by(server.as_deref(), expected)?;
    Ok(stream)
}

/// Whether reading a connected socket's peer failed because the other end
/// already closed it: not connected (`ENOTCONN`), reset, aborted, or, as
/// macOS answers for a socket whose peer is gone, `EINVAL`.
fn gone(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotConnected
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
    ) || error.raw_os_error() == Some(22)
}

/// The user of the process that serves a connected socket, as the kernel
/// reports it for the client's own end: the peer credentials of the
/// listening socket, which are its creator's. `None` when the kernel's
/// answer could not be read.
#[cfg(test)]
pub(crate) fn server_user(stream: &UnixStream) -> Option<String> {
    stream.peer_cred().ok().map(|cred| cred.uid().to_string())
}

/// The client's check of the engine's end of the connection, beside the
/// check of the socket's file and folder ([`check_owner`]): the kernel's
/// word on who serves must be `expected`. A server whose user could not be
/// read is nobody's, and is refused as another user's would be. Nothing
/// has been sent when this refuses.
pub(crate) fn served_by(server: Option<&str>, expected: &str) -> Result<(), ConnectError> {
    if server == Some(expected) {
        Ok(())
    } else {
        Err(ConnectError::NotOurs)
    }
}

/// What one attempt to connect came to. Nothing listening is an engine
/// that does not run. A connection the socket queued and that the engine
/// then closed untaken, as one does when it stops serving, comes back
/// reset: the kernel makes the connection at once and reports the reset
/// when the client next looks, which `UnixStream::connect` does before it
/// returns. That is an engine that went away with nothing sent, never a
/// refusal of this machine.
fn reached<T>(connected: io::Result<T>) -> Result<T, ConnectError> {
    connected.map_err(|error| match error.kind() {
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound => ConnectError::NotRunning,
        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted => ConnectError::Lost,
        _ => ConnectError::Io(error),
    })
}

/// Who may change a file or a folder: only this user or the administrator,
/// or others too. A link is judged with what it leads to, and its own owner
/// must be one of the two as well. A group may change it only when the
/// group is this user's own primary group.
pub(crate) fn kept(path: &Path) -> io::Result<Kept> {
    let me = rustix::process::getuid().as_raw();
    let mine = rustix::process::getgid().as_raw();
    let link = fs::symlink_metadata(path)?;
    let found = fs::metadata(path)?;
    if [&link, &found]
        .iter()
        .any(|meta| meta.uid() != me && meta.uid() != 0)
    {
        return Ok(Kept::Open("it belongs to another account"));
    }
    let mode = found.mode();
    if mode & 0o002 != 0 {
        return Ok(Kept::Open("every account may change it"));
    }
    if mode & 0o020 != 0 && found.gid() != mine {
        return Ok(Kept::Open("a group of other accounts may change it"));
    }
    Ok(Kept::Private)
}

/// Starts `command` detached from this process's terminal and process
/// group: in a session of its own, so that no signal of the terminal this
/// program was started at reaches it, and it has no controlling terminal.
/// Its standard streams are what `command` sets; every other handle this
/// program opens is closed on exec.
#[allow(
    unsafe_code,
    reason = "one system call between fork and exec; why it is sound is said where it is made"
)]
pub(crate) fn spawn_detached(
    command: &mut std::process::Command,
) -> io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    // SAFETY: the closure runs in the child between fork and exec, where
    // only calls that are safe in a signal handler may be made. It makes
    // one: `setsid`, which is such a call; rustix makes it as a bare system
    // call or through the C library's own `setsid`, allocating nothing and
    // taking no lock. The child is not a process group leader, because it
    // was just forked, so the call can succeed; when it fails the error is
    // the operating system's, and the start fails with it. The test
    // `a_command_with_no_engine_starts_one_detached_and_is_answered` of
    // the product crate reads the started engine's session.
    unsafe {
        command.pre_exec(|| {
            rustix::process::setsid()
                .map(|_| ())
                .map_err(io::Error::from)
        });
    }
    command.spawn()
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::SocketAddr;
    use std::path::PathBuf;

    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    use super::super::{ConnectError, Place};
    use super::{longest_socket_path, prepare_dir, reached};

    /// A connection the socket queued, and that its listener then closed
    /// untaken, is an engine that went away with nothing sent: the reset
    /// the kernel reports for it is never told as a refusal of this
    /// machine. Nothing listening is an engine that does not run, and any
    /// other error is the system's.
    #[test]
    fn a_connection_its_listener_closed_untaken_is_lost_and_not_a_refusal() {
        use std::io::{Error, ErrorKind};
        let root = scratch("untaken");
        let socket = root.join("s");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let client = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        drop(listener);
        // What the kernel says of the queued connection once its listener
        // is gone. Linux reports the reset here; a system that reports it
        // only when the client reads is held to the same reading below.
        let said = client
            .take_error()
            .unwrap()
            .unwrap_or_else(|| Error::from(ErrorKind::ConnectionReset));
        if cfg!(target_os = "linux") {
            assert_eq!(said.kind(), ErrorKind::ConnectionReset);
        }
        assert!(matches!(reached::<()>(Err(said)), Err(ConnectError::Lost)));
        assert!(matches!(
            reached::<()>(Err(ErrorKind::ConnectionAborted.into())),
            Err(ConnectError::Lost)
        ));
        for nothing in [ErrorKind::ConnectionRefused, ErrorKind::NotFound] {
            assert!(matches!(
                reached::<()>(Err(nothing.into())),
                Err(ConnectError::NotRunning)
            ));
        }
        assert!(matches!(
            reached::<()>(Err(ErrorKind::PermissionDenied.into())),
            Err(ConnectError::Io(_))
        ));
        assert!(matches!(reached(Ok(())), Ok(())));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A fresh folder of this test's own under the temporary folder.
    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("a2m-local-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    /// The folder is checked and closed through one handle, opened without
    /// following a link at its end: a folder others may enter is closed, and
    /// a link or a file in the folder's place is refused, and what a link
    /// points at is left as it was (decision 0273).
    #[test]
    fn a_folder_is_closed_through_its_handle_and_a_link_in_its_place_is_refused() {
        let root = scratch("prepare");

        let made = root.join("made");
        prepare_dir(&made).unwrap();
        assert_eq!(std::fs::metadata(&made).unwrap().mode() & 0o777, 0o700);

        let open = root.join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        prepare_dir(&open).unwrap();
        assert_eq!(std::fs::metadata(&open).unwrap().mode() & 0o777, 0o700);

        let target = root.join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let refused = prepare_dir(&link).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::metadata(&target).unwrap().mode() & 0o777, 0o755);

        let file = root.join("file");
        std::fs::write(&file, b"").unwrap();
        let refused = prepare_dir(&file).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_socket_path_is_as_long_as_the_systems_address_holds_and_no_longer() {
        let longest = longest_socket_path();
        // What the system's own `sockaddr_un` holds, on every Unix this
        // runs on: 104 bytes on macOS and the BSDs, 108 on Linux, each
        // less the byte that ends the path.
        if cfg!(target_os = "linux") {
            assert_eq!(longest, 107);
        } else if cfg!(target_vendor = "apple") {
            assert_eq!(longest, 103);
        }
        let of = |length: usize| format!("/{}", "x".repeat(length - 1));
        assert!(SocketAddr::from_pathname(of(longest)).is_ok());
        assert!(SocketAddr::from_pathname(of(longest + 1)).is_err());

        // A place whose socket fits is not too long; one byte more is.
        let fits = longest - "/engine.sock".len();
        let place = Place::new(PathBuf::from(of(fits)), "", "");
        assert_eq!(place.socket.as_os_str().len(), longest);
        assert_eq!(place.too_long(), None);
        let place = Place::new(PathBuf::from(of(fits + 1)), "", "");
        assert_eq!(
            place.too_long(),
            Some((place.socket.as_path(), longest + 1, longest))
        );
    }
}
