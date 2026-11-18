//! Who may change a program's file, and each folder on the way to it, on
//! Windows: the owner and the discretionary access list of each, read from
//! the system.
//!
//! Only the engine's user and the system's own identities may hold a right
//! that changes what the declared path names. The system's own are a closed
//! list: the local system account, the built-in Administrators group, and
//! the trusted installer service. Any other identity that holds such a right
//! is another user, and the program is refused.
//!
//! Which rights change what a path names differs by place:
//!
//! - **The file**: any right to write it, append to it, write its extended
//!   attributes or attributes, delete it, change its access list, or take
//!   it over.
//! - **The folder that holds it**: any right to add a file or a folder in it
//!   (the system looks there first for the libraries a program loads, and
//!   for a folder that redirects them), to delete what it holds, or to
//!   delete, change the access list of, or take over the folder itself.
//! - **A folder above**: any right to delete what it holds, or to delete,
//!   change the access list of, or take over the folder itself. Adding an
//!   entry is not one: it cannot change an entry that is already there, and
//!   every name on the way is. The root of a drive lets every signed-in user
//!   add a folder, so a stricter rule would refuse every program.
//!
//! An entry that only denies is not read (it can only take rights away, so
//! leaving it out refuses more, never less). An entry that applies only to
//! what is made inside is not read. An entry that allows, of a kind this
//! code does not read, is a refusal; so is a list that cannot be read, and
//! an object with no list at all, which allows everyone everything.
//!
//! A link, a junction, or any other reparse point on the way is refused,
//! not followed: that is the stricter of the two choices the Unix walk has,
//! and it leaves no second way of resolving a path to get wrong.
//!
//! Every call into the operating system is in this file. Each `unsafe`
//! block says why it is sound, and the tests at the end run each on
//! Windows.

use std::ffi::c_void;
use std::io;
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr;

use windows_sys::Win32::Foundation::{GENERIC_ALL, GENERIC_WRITE, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSidToSidW, GetSecurityInfo, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    ACE_HEADER, ACL, CopySid, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetLengthSid,
    GetTokenInformation, INHERIT_ONLY_ACE, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_APPEND_DATA, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA, READ_CONTROL,
    WRITE_DAC, WRITE_OWNER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::{Refused, Trust};

/// The rights over the file itself that let another change it.
const FILE_CHANGES: u32 = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_WRITE_ATTRIBUTES
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_WRITE
    | GENERIC_ALL;

/// The rights over a folder above that let another change what a name in it
/// names: delete or rename an entry, or the folder itself, or give
/// themselves any right.
const ABOVE_CHANGES: u32 = FILE_DELETE_CHILD | DELETE | WRITE_DAC | WRITE_OWNER | GENERIC_ALL;

/// The rights over the folder that holds the file: those of a folder
/// above, and adding anything, since a program loads what lies beside it.
const HOLDER_CHANGES: u32 = ABOVE_CHANGES | FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY | GENERIC_WRITE;

/// The type of an entry that allows, `ACCESS_ALLOWED_ACE_TYPE`.
const ALLOWED: u8 = 0;
/// The types of an entry that only denies: plain, object, callback, and
/// callback object.
const DENIED: &[u8] = &[1, 6, 10, 12];
/// Where an allowing entry's mask is, after its header.
const MASK_AT: usize = size_of::<ACE_HEADER>();
/// Where an allowing entry's identifier begins, after its mask.
const SID_AT: usize = MASK_AT + size_of::<u32>();

/// The system's own identities, as security identifiers: the local system
/// account, the built-in Administrators group, and the trusted installer
/// service.
const SYSTEMS_OWN: &[&str] = &[
    "S-1-5-18",
    "S-1-5-32-544",
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
];

/// Identities that stand for an object's owner: `OWNER RIGHTS` and
/// `CREATOR OWNER`. The owner is checked first, so these are ours when it
/// is.
const OWNERS: &[&str] = &["S-1-3-4", "S-1-3-0"];

/// A security identifier kept in a buffer of our own.
struct Sid {
    /// Whole 64-bit words, so that the identifier is aligned.
    buffer: Vec<u64>,
}

impl Sid {
    fn as_psid(&self) -> PSID {
        self.buffer.as_ptr().cast::<c_void>().cast_mut()
    }

    /// A copy of the identifier `sid` points to, which must be valid for
    /// the length of the call.
    #[allow(unsafe_code)] // two calls into the operating system, explained below
    fn copy_of(sid: PSID) -> io::Result<Self> {
        // SAFETY: the caller hands a pointer to a valid identifier; the call
        // only reads it.
        let length = unsafe { GetLengthSid(sid) };
        if length == 0 {
            return Err(io::Error::other("an identifier has no length"));
        }
        let mut buffer = vec![0_u64; (length as usize).div_ceil(size_of::<u64>())];
        // SAFETY: `buffer` is writable for at least `length` bytes and
        // outlives the call; `sid` is valid, as above.
        let ok = unsafe { CopySid(length, buffer.as_mut_ptr().cast::<c_void>(), sid) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { buffer })
    }

    /// The identifier written as `text`, such as `S-1-5-18`.
    #[allow(unsafe_code)] // two calls into the operating system, explained below
    fn from_text(text: &str) -> io::Result<Self> {
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let mut sid: PSID = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated and outlives the call, which
        // writes one pointer, ours, to an identifier it allocates.
        let ok = unsafe { ConvertStringSidToSidW(wide.as_ptr(), &raw mut sid) };
        if ok == 0 || sid.is_null() {
            return Err(io::Error::last_os_error());
        }
        let copied = Self::copy_of(sid);
        // SAFETY: `sid` was allocated by the conversion above, which
        // documents this call to free it; it is freed once, after its last
        // use.
        unsafe {
            LocalFree(sid);
        }
        copied
    }

    /// Whether `other`, a valid identifier for the length of the call, is
    /// this one, by the system's own comparison.
    #[allow(unsafe_code)] // one call into the operating system, explained below
    fn is(&self, other: PSID) -> bool {
        // SAFETY: both pointers are valid identifiers for the call: ours
        // lives in `self.buffer`, and the caller vouches for `other`.
        unsafe { EqualSid(self.as_psid(), other) != 0 }
    }
}

/// The user this process runs as.
#[allow(unsafe_code)] // calls into the operating system; each is explained where it is made
fn current_user() -> io::Result<Sid> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns a constant that stands for this
    // process and needs no closing. The call writes one handle into
    // `token`, which is ours.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) };
    if ok == 0 || token.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `token` is an open handle nothing else owns; it is wrapped
    // once, so it is closed once.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut needed = 0_u32;
    // SAFETY: a null buffer of length zero is allowed: the call then writes
    // only `needed`, which is ours, and fails saying the buffer is small.
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
    // until the copy below is made.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    Sid::copy_of(sid)
}

/// The identities whose rights are the system's own, or the user's.
struct Ours {
    user: Sid,
    system: Vec<Sid>,
    owners: Vec<Sid>,
}

impl Ours {
    fn read() -> io::Result<Self> {
        Ok(Self {
            user: current_user()?,
            system: SYSTEMS_OWN
                .iter()
                .map(|text| Sid::from_text(text))
                .collect::<io::Result<_>>()?,
            owners: OWNERS
                .iter()
                .map(|text| Sid::from_text(text))
                .collect::<io::Result<_>>()?,
        })
    }

    /// Whether `sid` may own what the engine starts.
    fn may_own(&self, sid: PSID) -> bool {
        self.user.is(sid) || self.system.iter().any(|ours| ours.is(sid))
    }

    /// Whether a right `sid` holds is ours to hold, once the owner is ours.
    fn may_hold(&self, sid: PSID) -> bool {
        self.may_own(sid) || self.owners.iter().any(|owner| owner.is(sid))
    }
}

/// Why a file or folder is not closed to other users.
enum Open {
    /// Another identity owns it or holds a right named.
    Other,
    /// Its owner or its list could not be read, or the list holds an entry
    /// of a kind not read here.
    Unreadable,
}

/// Opens a file or folder to read its owner and access list, never through
/// a link at its name: a link there is opened as itself.
fn open(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// Whether what `file` holds open is owned by us, and no identity but ours
/// holds a right in `changes` over it.
#[allow(unsafe_code)] // calls into the operating system and reads what they give; each is explained
fn closed(file: &std::fs::File, ours: &Ours, changes: u32) -> Result<(), Open> {
    let mut owner: PSID = ptr::null_mut();
    let mut list: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle is open for `READ_CONTROL` and owned by `file`,
    // which outlives the call. The call writes three pointers, all ours;
    // the owner and the list point into the descriptor it allocates. The
    // group and the system list are not asked for, which nulls say.
    let error = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &raw mut owner,
            ptr::null_mut(),
            &raw mut list,
            ptr::null_mut(),
            &raw mut descriptor,
        )
    };
    if error != 0 || descriptor.is_null() {
        return Err(Open::Unreadable);
    }
    // SAFETY: `owner` and `list` point into `descriptor`, which stays
    // allocated until it is freed below, after their last use. The list's
    // header says how many entries it holds, and `GetAce` gives a pointer to
    // each, inside the list. Every entry starts with an `ACE_HEADER`; one
    // that allows has its mask and then its identifier after the header, at
    // the offsets named, inside the entry. Both are read without assuming
    // their alignment.
    let verdict = unsafe {
        if owner.is_null() {
            Err(Open::Unreadable)
        } else if !ours.may_own(owner) {
            Err(Open::Other)
        } else if list.is_null() {
            // No list at all: everyone may do everything.
            Err(Open::Other)
        } else {
            let mut verdict = Ok(());
            for index in 0..u32::from((*list).AceCount) {
                let mut ace: *mut c_void = ptr::null_mut();
                if GetAce(list, index, &raw mut ace) == 0 || ace.is_null() {
                    verdict = Err(Open::Unreadable);
                    break;
                }
                let header = ptr::read_unaligned(ace.cast::<ACE_HEADER>());
                if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0
                    || DENIED.contains(&header.AceType)
                {
                    continue;
                }
                if header.AceType != ALLOWED || usize::from(header.AceSize) <= SID_AT {
                    verdict = Err(Open::Unreadable);
                    break;
                }
                let bytes = ace.cast::<u8>();
                let mask = ptr::read_unaligned(bytes.add(MASK_AT).cast::<u32>());
                let trustee: PSID = bytes.add(SID_AT).cast::<c_void>();
                if mask & changes != 0 && !ours.may_hold(trustee) {
                    verdict = Err(Open::Other);
                    break;
                }
            }
            verdict
        }
    };
    // SAFETY: `descriptor` was allocated by `GetSecurityInfo`, which
    // documents this call to free it; it is freed once, after the last use
    // of `owner` and `list`, which point into it.
    unsafe {
        LocalFree(descriptor);
    }
    verdict
}

/// What sits at `path`, opened as itself: a folder or a file, never a link.
fn entry(path: &Path) -> Result<(std::fs::File, bool), Refused> {
    let file = open(path).map_err(|_| Refused::Missing)?;
    let metadata = file.metadata().map_err(|_| Refused::Missing)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Refused::Link(path.to_path_buf()));
    }
    Ok((file, metadata.is_dir()))
}

/// The refusal for what `closed` found at `path`.
fn refusal(why: Open, path: &Path) -> Refused {
    match why {
        Open::Other => Refused::Writable(path.to_path_buf()),
        Open::Unreadable => Refused::Unreadable(path.to_path_buf()),
    }
}

/// Walks an absolute path on a local drive name by name, as the system
/// resolves it (a `..` takes the folder before it away, as the system does
/// to the text before it opens anything), and gives the path it names.
///
/// The root of the drive and every folder entered must be closed to other
/// users by the rights named for its place, and the file by the rights
/// named for a file; a link on the way is refused. So when this passes,
/// nobody else can change what the declared path names between this check
/// and a start.
pub(super) fn walk(declared: &Path, _trust: &Trust) -> Result<PathBuf, Refused> {
    let mut parts = declared.components();
    let (Some(Component::Prefix(prefix)), Some(Component::RootDir)) = (parts.next(), parts.next())
    else {
        return Err(Refused::NotAbsolute);
    };
    let verbatim = match prefix.kind() {
        Prefix::Disk(_) => false,
        Prefix::VerbatimDisk(_) => true,
        _ => return Err(Refused::Elsewhere),
    };
    let root: PathBuf = [Component::Prefix(prefix), Component::RootDir]
        .iter()
        .collect();
    let mut names: Vec<&std::ffi::OsStr> = Vec::new();
    for part in parts {
        match part {
            Component::Normal(name) => names.push(name),
            Component::CurDir => {}
            // In a long-form path the system takes `..` as a name; nothing
            // declared needs one.
            Component::ParentDir if verbatim => return Err(Refused::Missing),
            Component::ParentDir => {
                names.pop();
            }
            Component::Prefix(_) | Component::RootDir => return Err(Refused::NotAbsolute),
        }
    }
    let Some(last) = names.len().checked_sub(1) else {
        // The root of a drive is not a program.
        return Err(Refused::Missing);
    };
    let ours = Ours::read().map_err(|_| Refused::Unreadable(declared.to_path_buf()))?;
    let (top, _) = entry(&root)?;
    let changes = if last == 0 {
        HOLDER_CHANGES
    } else {
        ABOVE_CHANGES
    };
    closed(&top, &ours, changes).map_err(|why| refusal(why, &root))?;
    let mut at = root;
    for (index, name) in names.iter().enumerate() {
        let next = at.join(name);
        let (file, folder) = entry(&next)?;
        let changes = match (folder, index) {
            // A folder is not a program, and a file is the end of a path.
            (true, index) if index == last => return Err(Refused::Missing),
            (false, index) if index != last => return Err(Refused::Missing),
            (false, _) => FILE_CHANGES,
            (true, index) if index + 1 == last => HOLDER_CHANGES,
            (true, _) => ABOVE_CHANGES,
        };
        closed(&file, &ours, changes).map_err(|why| refusal(why, &next))?;
        at = next;
    }
    Ok(at)
}

/// The file itself was checked by [`walk`], by the rights over a file.
pub(super) fn owned_by_us(
    _real: &Path,
    _metadata: &std::fs::Metadata,
    _trust: &Trust,
) -> Result<(), Refused> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::windows::io::AsRawHandle as _;
    use std::path::{Path, PathBuf};
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
        SetSecurityInfo,
    };
    use windows_sys::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PSECURITY_DESCRIPTOR,
    };
    use windows_sys::Win32::Storage::FileSystem::{READ_CONTROL, WRITE_DAC};

    use super::super::{Refused, Role, Trust, check};

    /// A folder of the test's own under the temporary folder, removed when
    /// dropped, with a program in it.
    struct Place(PathBuf);

    impl Place {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("a2m-command-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("real")).unwrap();
            std::fs::write(root.join("real").join("example-tool.exe"), b"MZ").unwrap();
            Self(root)
        }

        fn program(&self) -> PathBuf {
            self.0.join("real").join("example-tool.exe")
        }
    }

    impl Drop for Place {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Adds to the access list of `path` the entries written in `added`, in
    /// the system's own text form, keeping what it inherits. With
    /// `protected`, nothing is inherited and the list is only `added`.
    #[allow(unsafe_code)] // calls into the operating system; each is explained where it is made
    fn grant(path: &Path, added: &str) {
        use std::os::windows::fs::OpenOptionsExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
        let file = std::fs::OpenOptions::new()
            .access_mode(READ_CONTROL | WRITE_DAC)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .unwrap();
        let rule: Vec<u16> = format!("D:{added}").encode_utf16().chain(Some(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `rule` is NUL-terminated and outlives the call, which
        // writes one pointer, ours.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                rule.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                ptr::null_mut(),
            )
        };
        assert_ne!(ok, 0);
        let (mut present, mut defaulted) = (0, 0);
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
        assert!(ok != 0 && present != 0 && !list.is_null());
        // SAFETY: the handle is open for `WRITE_DAC` and owned by `file`;
        // `list` points into `descriptor`, still allocated. The system
        // copies the list, and merges what the object inherits, before the
        // call returns: the list is not protected.
        let error = unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                list,
                ptr::null(),
            )
        };
        // SAFETY: `descriptor` came from the conversion above, which
        // documents this call to free it; freed once, after `list`'s use.
        unsafe {
            LocalFree(descriptor);
        }
        assert_eq!(error, 0);
    }

    fn program_check(path: &Path) -> Result<(), Refused> {
        check(path, &Trust::default(), Role::Program)
    }

    #[test]
    fn a_program_only_the_user_and_the_system_can_change_is_accepted() {
        let place = Place::new("closed");
        assert_eq!(program_check(&place.program()), Ok(()));
        // A folder above that lets everyone add an entry, and no more, does
        // not change what the path names.
        grant(&place.0, "(A;;0x4;;;WD)(A;;0x2;;;BU)");
        assert_eq!(program_check(&place.program()), Ok(()));
    }

    #[test]
    fn a_program_another_identity_can_write_is_refused() {
        let place = Place::new("file");
        let program = place.program();
        // Everyone may write the file.
        grant(&program, "(A;;FW;;;WD)");
        assert_eq!(
            program_check(&program),
            Err(Refused::Writable(program.clone()))
        );
    }

    #[test]
    fn a_folder_another_identity_can_change_is_refused() {
        // The ordinary users may add a file beside the program: a library
        // the program loads would be theirs.
        let place = Place::new("holder");
        let holder = place.0.join("real");
        grant(&holder, "(A;;0x2;;;BU)");
        assert_eq!(
            program_check(&place.program()),
            Err(Refused::Writable(holder))
        );
        // Everyone may delete what a folder above holds.
        let place = Place::new("above");
        grant(&place.0, "(A;;0x40;;;WD)");
        assert_eq!(
            program_check(&place.program()),
            Err(Refused::Writable(place.0.clone()))
        );
    }

    #[test]
    fn a_link_on_the_way_and_a_share_are_refused() {
        let place = Place::new("link");
        let link = place.0.join("through");
        // A junction needs no privilege, where a symbolic link may.
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(place.0.join("real"))
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        assert_eq!(
            program_check(&link.join("example-tool.exe")),
            Err(Refused::Link(link.clone()))
        );
        assert_eq!(
            program_check(Path::new(r"\\example\share\example-tool.exe")),
            Err(Refused::Elsewhere)
        );
    }
}
