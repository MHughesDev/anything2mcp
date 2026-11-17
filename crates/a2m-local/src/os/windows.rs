//! The channel on Windows: a named pipe whose access list names only the
//! user, created with the first-instance flag, with the user of the process
//! on the other end read from that process's token.
//!
//! Every call into the operating system is in this file. Each `unsafe`
//! block says why it is sound, and the tests at the end of the file run
//! each of them on Windows.

use std::ffi::c_void;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::ptr;
use std::time::Duration;

use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE, LocalFree, SetHandleInformation,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetTokenInformation,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_WRITE, FILE_WRITE_DATA, READ_CONTROL, WRITE_DAC,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};
use windows_sys::Win32::System::Threading::{
    CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS, GetCurrentProcess,
    OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::{ConnectError, PeerId, Place};

/// The engine's end of one connection.
pub(crate) type ServerStream = NamedPipeServer;
/// The client's end of one connection.
pub(crate) type ClientStream = NamedPipeClient;

/// How often a client tries again while every instance of the pipe is
/// taken, and how long it lets the engine reach its next accept between
/// tries. The operating system offers no handle to wait on for this.
const BUSY_TRIES: u32 = 40;
const BUSY_PAUSE: Duration = Duration::from_millis(25);

/// The text form of the security identifier of the user a token is for.
#[allow(unsafe_code)] // four calls into the operating system; each is explained where it is made
fn token_user(token: &OwnedHandle) -> io::Result<String> {
    let mut needed = 0_u32;
    // SAFETY: the token handle is open and owned by `token`. A null buffer
    // of length zero is allowed: the call then writes only `needed`, which
    // is ours, and fails saying the buffer is too small.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &raw mut needed,
        );
    }
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // Whole 64-bit words, so that the buffer is aligned for `TOKEN_USER`,
    // which holds a pointer.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>())];
    let length = u32::try_from(buffer.len() * size_of::<u64>())
        .map_err(|_| io::Error::other("the token's user does not fit a buffer"))?;
    // SAFETY: `buffer` is writable for `length` bytes and outlives the
    // call, and `needed` is ours.
    let ok = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast::<c_void>(),
            length,
            &raw mut needed,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call succeeded, so `buffer` starts with a `TOKEN_USER`,
    // and a buffer of 64-bit words is aligned at least as strictly as that
    // type. The identifier it points to lies inside `buffer`, which lives
    // until this function returns.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    // `sid` points into `buffer`, which lives until this function returns.
    sid_text(sid)
}

/// The text form of the security identifier `sid` points to, which must
/// be a valid identifier for the length of the call.
#[allow(unsafe_code)] // two calls into the operating system; each is explained where it is made
fn sid_text(sid: PSID) -> io::Result<String> {
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: the caller hands a pointer to a valid identifier. The call
    // writes one pointer into `text`, which is ours.
    let ok = unsafe { ConvertSidToStringSidW(sid, &raw mut text) };
    if ok == 0 || text.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: on success `text` is a NUL-terminated string the system
    // allocated. It is read up to its NUL and no further, copied, and then
    // freed exactly once with the call the system documents for it; nothing
    // uses the pointer afterwards.
    let said = unsafe {
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let said = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast::<c_void>());
        said
    };
    Ok(said)
}

/// The user a process runs as: this process, or the one with `id`.
#[allow(unsafe_code)] // calls into the operating system; each is explained where it is made
fn user_of_process(id: Option<u32>) -> io::Result<String> {
    let opened = match id {
        None => None,
        Some(id) => {
            // SAFETY: the call takes three plain values and touches no
            // memory of ours. It returns null when it fails, or a new
            // handle that only this function knows.
            let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, id) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `raw` is an open handle that nothing else owns, and
            // it is wrapped exactly once, so it is closed exactly once.
            Some(unsafe { OwnedHandle::from_raw_handle(raw) })
        }
    };
    let process: RawHandle = opened.as_ref().map_or_else(
        // SAFETY: the call takes nothing and returns a constant that
        // stands for this process; it is not a handle to close.
        || unsafe { GetCurrentProcess() },
        AsRawHandle::as_raw_handle,
    );
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `process` is this process or the handle `opened` keeps open
    // past this call. The call writes one handle into `token`, which is
    // ours.
    let ok = unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) };
    if ok == 0 || token.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `token` is an open handle that nothing else owns, and it is
    // wrapped exactly once, so it is closed exactly once.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    token_user(&token)
}

/// The user this process runs as, as the text of its security identifier.
pub(crate) fn current_user() -> io::Result<String> {
    user_of_process(None)
}

/// The security identifier of the local Administrators group, which may
/// own what this user made while it ran elevated.
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// Not this user's to trust: another owns it, or it is a link.
fn not_ours(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("the {what} is not this user's, or is a link"),
    )
}

/// Opens a file or folder to read its owner and replace its access list,
/// never through a link at its name: a link there is opened as itself.
fn open_guarded(path: &Path, extra: u32) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC | extra)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// Whether what `file` holds open is a link, or another kind of reparse
/// point, rather than the file or folder itself.
fn is_reparse_point(file: &std::fs::File) -> io::Result<bool> {
    use std::os::windows::fs::MetadataExt as _;
    Ok(file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

/// The owner of what `file` holds open, as the text of its identifier.
#[allow(unsafe_code)] // two calls into the operating system; each is explained where it is made
fn owner_of(file: &std::fs::File) -> io::Result<String> {
    let mut owner: PSID = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle is open for `READ_CONTROL` and owned by `file`,
    // which outlives the call. The call writes two pointers, both ours;
    // the group and the lists are not asked for, which null pointers say.
    let error = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &raw mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut descriptor,
        )
    };
    if error != 0 || descriptor.is_null() {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    // `owner` points into `descriptor`, which is still allocated.
    let said = if owner.is_null() {
        Err(io::Error::other("the owner could not be read"))
    } else {
        sid_text(owner)
    };
    // SAFETY: `descriptor` was allocated by the call above, which documents
    // this call to free it. It is freed exactly once, after the last use of
    // `owner`, which points into it.
    unsafe {
        LocalFree(descriptor);
    }
    said
}

/// Replaces the access list of what `file` holds open with a protected one
/// that names only `user`, with every right; with `inherit`, files and
/// folders made inside take it. Nothing of the folders above is inherited.
#[allow(unsafe_code)] // four calls into the operating system; each is explained where it is made
fn keep_to(file: &std::fs::File, user: &str, inherit: bool) -> io::Result<()> {
    if !is_sid(user) {
        return Err(io::Error::other("the user is not a security identifier"));
    }
    let flags = if inherit { "OICI" } else { "" };
    let rule: Vec<u16> = format!("D:P(A;{flags};FA;;;{user})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `rule` is NUL-terminated and outlives the call. The call
    // writes one pointer into `descriptor`, which is ours.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            rule.as_ptr(),
            SDDL_REVISION_1,
            &raw mut descriptor,
            ptr::null_mut(),
        )
    };
    if ok == 0 || descriptor.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut present = 0;
    let mut defaulted = 0;
    let mut list: *mut ACL = ptr::null_mut();
    // SAFETY: `descriptor` is the one made above, still allocated; the
    // call writes three values, all ours. `list` then points into it.
    let ok = unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &raw mut present,
            &raw mut list,
            &raw mut defaulted,
        )
    };
    let set = if ok == 0 || present == 0 || list.is_null() {
        Err(io::Error::other("the access list could not be made"))
    } else {
        // SAFETY: the handle is open for `WRITE_DAC` and owned by `file`;
        // `list` points into `descriptor`, still allocated. The system
        // copies the list before the call returns.
        let error = unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                list,
                ptr::null(),
            )
        };
        if error == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(error as i32))
        }
    };
    // SAFETY: `descriptor` came from the conversion above, which documents
    // this call to free it. It is freed exactly once, after the last use of
    // `list`, which points into it.
    unsafe {
        LocalFree(descriptor);
    }
    set
}

/// Makes the runtime directory, or checks that the one there is a folder,
/// not a link, owned by this user or the administrators; and in either case
/// gives it an access list that names only this user, which the lock files
/// made inside take (decision 0192). The pipe lives in the system's own
/// name space, guarded by its own access list.
pub(crate) fn prepare_dir(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::create_dir(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let folder = open_guarded(dir, 0)?;
    if is_reparse_point(&folder)? || !folder.metadata()?.is_dir() {
        return Err(not_ours("runtime directory"));
    }
    let user = current_user()?;
    let owner = owner_of(&folder)?;
    if owner != user && owner != ADMINISTRATORS {
        return Err(not_ours("runtime directory"));
    }
    keep_to(&folder, &user, true)
}

/// Opens the started engine's output file to append to it: never through a
/// link at its name, only a plain file this user or the administrators
/// own, and with an access list that names only this user (decision 0192).
pub(crate) fn open_output(path: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    // Appending, as `OpenOptions::append` asks for it: every right to write
    // but the right to write over what is there.
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .access_mode((FILE_GENERIC_WRITE & !FILE_WRITE_DATA) | READ_CONTROL | WRITE_DAC)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if is_reparse_point(&file)? || !file.metadata()?.is_file() {
        return Err(not_ours("engine's output file"));
    }
    let user = current_user()?;
    let owner = owner_of(&file)?;
    if owner != user && owner != ADMINISTRATORS {
        return Err(not_ours("engine's output file"));
    }
    keep_to(&file, &user, false)?;
    Ok(file)
}

/// Whether `user` has the form of a security identifier: `S-` and then
/// digits and hyphens. Only such text is put into an access list.
fn is_sid(user: &str) -> bool {
    user.strip_prefix("S-").is_some_and(|rest| {
        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    })
}

/// One instance of the pipe, with an access list that names only `user`.
/// With `first`, the call fails when any process already made the pipe.
#[allow(unsafe_code)] // three calls into the operating system; each is explained where it is made
fn instance(name: &str, user: &str, first: bool) -> io::Result<NamedPipeServer> {
    if !is_sid(user) {
        return Err(io::Error::other("the user is not a security identifier"));
    }
    // A protected access list with one entry: everything, for this user.
    // Nothing is inherited, and nobody else is named.
    let rule: Vec<u16> = format!("D:P(A;;GA;;;{user})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `rule` is NUL-terminated and outlives the call. The call
    // writes one pointer into `descriptor`, which is ours; the size is not
    // asked for, which a null pointer says.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            rule.as_ptr(),
            SDDL_REVISION_1,
            &raw mut descriptor,
            ptr::null_mut(),
        )
    };
    if ok == 0 || descriptor.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true);
    // SAFETY: `attributes` is a valid `SECURITY_ATTRIBUTES` that lives
    // across the call, and its descriptor is the one made above, still
    // allocated. The system copies what it needs before the call returns.
    let made = unsafe {
        options.create_with_security_attributes_raw(name, (&raw mut attributes).cast::<c_void>())
    };
    // SAFETY: `descriptor` came from the conversion above, which documents
    // this call to free it. It is freed exactly once and not used again.
    unsafe {
        LocalFree(descriptor);
    }
    made
}

/// The engine's listener: always one instance of the pipe that waits for
/// the next client.
pub(crate) struct Listener {
    name: String,
    user: String,
    next: NamedPipeServer,
}

impl Listener {
    /// Makes the pipe. Fails when any process, of any user, made a pipe of
    /// this name first.
    pub(crate) fn bind(place: &Place) -> io::Result<Self> {
        let user = current_user()?;
        let next = instance(&place.pipe, &user, true)?;
        Ok(Self {
            name: place.pipe.clone(),
            user,
            next,
        })
    }

    /// The next connection, with the user of the client's process. Nothing
    /// is read from the connection. The instance that takes the following
    /// client exists before this one is handed on.
    pub(crate) async fn accept(&mut self) -> io::Result<(ServerStream, PeerId)> {
        self.next.connect().await?;
        let following = instance(&self.name, &self.user, false)?;
        let stream = std::mem::replace(&mut self.next, following);
        let peer = client_of(&stream);
        Ok((stream, peer))
    }
}

/// The process on the client's end of a pipe, and its user.
#[allow(unsafe_code)] // one call into the operating system, explained below
fn client_of(pipe: &NamedPipeServer) -> PeerId {
    let mut id = 0_u32;
    // SAFETY: the pipe's handle is open and owned by `pipe`, which outlives
    // the call. The call writes one number into `id`, which is ours.
    let ok = unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle(), &raw mut id) };
    if ok == 0 {
        return PeerId {
            user: None,
            process: None,
        };
    }
    PeerId {
        user: user_of_process(Some(id)).ok(),
        process: Some(id),
    }
}

/// The user of the process on the engine's end of a pipe.
#[allow(unsafe_code)] // one call into the operating system, explained below
fn server_user(pipe: &NamedPipeClient) -> io::Result<String> {
    let mut id = 0_u32;
    // SAFETY: the pipe's handle is open and owned by `pipe`, which outlives
    // the call. The call writes one number into `id`, which is ours.
    let ok = unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &raw mut id) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    user_of_process(Some(id))
}

/// Connects to the engine, then checks that the process which serves the
/// pipe runs as the expected user. Nothing is sent before that.
pub(crate) async fn connect(place: &Place, expected: &str) -> Result<ClientStream, ConnectError> {
    let mut tries = 0;
    let pipe = loop {
        match ClientOptions::new().open(&place.pipe) {
            Ok(pipe) => break pipe,
            Err(error) if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => {
                return Err(ConnectError::NotRunning);
            }
            Err(error)
                if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && tries < BUSY_TRIES =>
            {
                // Every instance is taken until the engine reaches its
                // next accept. This is a pause before trying again, with a
                // limit, because the system gives nothing to wait on.
                tries += 1;
                tokio::time::sleep(BUSY_PAUSE).await;
            }
            Err(error) => return Err(ConnectError::Io(error)),
        }
    };
    match server_user(&pipe) {
        Ok(user) if user == expected => Ok(pipe),
        Ok(_) | Err(_) => Err(ConnectError::NotOurs),
    }
}

/// Makes this process's own standard handles not inheritable. Windows
/// hands a new process every inheritable handle of its parent, not only
/// the three it is given, so without this an engine started by a command
/// would hold the command's own output open, and whoever reads it would
/// wait for the engine to end. The standard library still gives a child
/// that inherits a standard stream a copy made for it.
#[allow(unsafe_code)] // two calls into the operating system; each is explained where it is made
fn keep_standard_handles() {
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: `GetStdHandle` takes one of the three constants and
        // returns a handle of this process, null, or the invalid value; it
        // writes nothing of ours.
        let handle = unsafe { GetStdHandle(which) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            continue;
        }
        // SAFETY: the handle is this process's own, from the call above;
        // the call changes only its inherit flag. A handle that does not
        // take the flag, such as some console handles, makes it fail, which
        // changes nothing and is not this start's failure.
        unsafe {
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
        }
    }
}

/// Starts `command` detached from this process's console and process
/// group: with no console, in a process group of its own, and out of this
/// process's job where the job allows it, so that closing the terminal or
/// the job that started the client does not end the engine. Where the job
/// does not allow it, the engine is started inside it.
pub(crate) fn spawn_detached(
    command: &mut std::process::Command,
) -> io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    keep_standard_handles();
    let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    command.creation_flags(detached | CREATE_BREAKAWAY_FROM_JOB);
    match command.spawn() {
        Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED.cast_signed()) => {
            command.creation_flags(detached);
            command.spawn()
        }
        started => started,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::{ConnectError, Place};
    use super::{Listener, connect, current_user, is_sid};

    fn place(tag: &str) -> Place {
        let user = current_user().unwrap();
        Place::new(
            PathBuf::from("unused"),
            &user,
            &format!("{tag}-{}", std::process::id()),
        )
    }

    #[test]
    fn this_process_has_a_user_with_a_security_identifier() {
        let user = current_user().unwrap();
        assert!(is_sid(&user), "{user}");
        assert!(!is_sid("everyone"));
        assert!(!is_sid("S-1-5)(A;;GA;;;WD"));
    }

    #[tokio::test]
    async fn the_pipe_is_made_once_and_each_end_reads_the_others_user() {
        let place = place("ends");
        let user = current_user().unwrap();
        let mut listener = Listener::bind(&place).unwrap();
        // The first-instance flag: nobody can make the pipe a second time.
        assert!(Listener::bind(&place).is_err());

        let client = tokio::spawn({
            let place = place.clone();
            let user = user.clone();
            async move { connect(&place, &user).await }
        });
        let (server, peer) = listener.accept().await.unwrap();
        assert_eq!(peer.user.as_deref(), Some(user.as_str()));
        assert_eq!(peer.process, Some(std::process::id()));
        let client = client.await.unwrap().unwrap();

        client.writable().await.unwrap();
        assert_eq!(client.try_write(b"ping").unwrap(), 4);
        let mut got = [0_u8; 4];
        loop {
            server.readable().await.unwrap();
            match server.try_read(&mut got) {
                Ok(read) => {
                    assert_eq!(read, 4);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("{error}"),
            }
        }
        assert_eq!(&got, b"ping");

        // A client that expects another user refuses this engine.
        let other = connect(&place, "S-1-0-0").await;
        assert!(matches!(other, Err(ConnectError::NotOurs)), "{other:?}");
        // With no engine, nothing answers.
        drop(listener);
        drop(server);
        let gone = connect(&place, &user).await;
        assert!(matches!(gone, Err(ConnectError::NotRunning)), "{gone:?}");
    }
}

#[cfg(test)]
mod kept_to_the_user {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle as _;
    use std::path::{Path, PathBuf};
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
        SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};

    use super::{current_user, open_guarded, open_output, prepare_dir};

    fn fresh(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("a2m-local-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::remove_file(&path);
        path
    }

    /// The access list of what is at `path`, in the system's own text.
    #[allow(unsafe_code)] // two calls into the operating system and one free; each is explained
    fn access_list(path: &Path) -> String {
        let file = open_guarded(path, 0).unwrap();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: the handle is open for `READ_CONTROL`; the call writes
        // one pointer, ours, and is asked for nothing else.
        let error = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut descriptor,
            )
        };
        assert_eq!(error, 0);
        let mut text: *mut u16 = ptr::null_mut();
        // SAFETY: `descriptor` is the one the call above allocated; the
        // call writes one pointer, ours; the length is not asked for.
        let ok = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &raw mut text,
                ptr::null_mut(),
            )
        };
        assert_ne!(ok, 0);
        // SAFETY: `text` is a NUL-terminated string the system allocated,
        // read up to its NUL; it and `descriptor` are each freed once, with
        // the call the system documents, and not used again.
        unsafe {
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let said = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast::<c_void>());
            LocalFree(descriptor);
            said
        }
    }

    /// A protected list with one entry, which allows this user.
    fn only_the_user(list: &str) {
        let user = current_user().unwrap();
        assert!(list.starts_with("D:P"), "{list}");
        assert_eq!(list.matches('(').count(), 1, "{list}");
        assert!(list.contains(&format!(";;;{user})")), "{list}");
        assert!(list.contains("(A;"), "{list}");
    }

    #[test]
    fn the_runtime_directory_and_the_output_file_are_kept_to_this_user() {
        // Made, and made again over itself: both times one entry, the user's.
        let dir = fresh("runtime");
        prepare_dir(&dir).unwrap();
        only_the_user(&access_list(&dir));
        prepare_dir(&dir).unwrap();
        let list = access_list(&dir);
        only_the_user(&list);
        // What is made inside takes it.
        assert!(list.contains("OICI"), "{list}");

        // The output file has a list of its own, and is appended to.
        let output = dir.join("engine-output.log");
        {
            use std::io::Write as _;
            let mut file = open_output(&output).unwrap();
            file.write_all(b"one\n").unwrap();
        }
        {
            use std::io::Write as _;
            let mut file = open_output(&output).unwrap();
            file.write_all(b"two\n").unwrap();
        }
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "one\ntwo\n");
        only_the_user(&access_list(&output));

        // A file where the directory should be is not a runtime directory.
        let not_a_dir = fresh("not-a-dir");
        std::fs::write(&not_a_dir, b"").unwrap();
        assert!(prepare_dir(&not_a_dir).is_err());
        // A folder where the output file should be is not one.
        assert!(open_output(&dir).is_err());

        std::fs::remove_file(&not_a_dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
