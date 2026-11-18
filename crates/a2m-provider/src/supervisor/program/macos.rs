//! The access list beyond the mode of a file or folder, on macOS, read from
//! the system (finding D3 of the sixth safety audit, decision 0271).
//!
//! On macOS an entry of a file's extended access list can let another user
//! write it, delete it, add to a folder, or change its rights, with no
//! change to the mode bits the walk reads. Two things here would need more
//! than the system's list functions to read soundly: whom an entry names
//! (a 128-bit identifier that only the system's membership service maps to
//! a user or a group) and which rights it grants (bit values this code
//! could not check against a running system when it was written). So an
//! entry is read for its kind alone:
//!
//! - an entry that **denies** can only take rights away, and is passed
//!   over, which refuses more, never less (the system puts such entries on
//!   a user's own folders, to keep anyone from deleting them);
//! - an entry that **allows**, whatever it grants and to whomever, is a
//!   refusal, [`ProgramRefused::Unreadable`]: what it grants is not read
//!   here, and so it is not vouched for;
//! - an entry of any other kind, a list that cannot be read, and a list
//!   longer than the system allows, are refusals too.
//!
//! The system returns no list, and says the file is not there, both for a
//! file with no list and for one with an empty list; the walk has just
//! found the file, so this is read as a list with no entry. A file system
//! that keeps no lists says so, and the mode is then all there is.
//!
//! Every call into the operating system is in this file. Each `unsafe`
//! block says why it is sound, and the test at the end runs each on macOS.

use std::ffi::{CString, c_char, c_int, c_void};
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;
use std::ptr;

use super::ProgramRefused;

/// `ACL_TYPE_EXTENDED` of `<sys/acl.h>`: the one kind of list macOS keeps.
const ACL_TYPE_EXTENDED: c_int = 0x0000_0100;
/// `ACL_FIRST_ENTRY` and `ACL_NEXT_ENTRY`, how entries are asked for.
const ACL_FIRST_ENTRY: c_int = 0;
const ACL_NEXT_ENTRY: c_int = -1;
/// `ACL_EXTENDED_DENY`: an entry that denies.
const ACL_EXTENDED_DENY: c_int = 2;
/// `ACL_MAX_ENTRIES` of `<sys/acl.h>`: no list holds more.
const ACL_MAX_ENTRIES: usize = 128;
/// The system's numbers for "no such file" and for "this file system keeps
/// no lists" (`ENOENT`, `ENOTSUP`, `EOPNOTSUPP`).
const ENOENT: i32 = 2;
const ENOTSUP: i32 = 45;
const EOPNOTSUPP: i32 = 102;

#[allow(unsafe_code)] // declarations of the system's own functions; each call is explained where it is made
unsafe extern "C" {
    /// `acl_t acl_get_link_np(const char *path, acl_type_t type)`: the list
    /// of the file at `path`, a link itself and never where it leads.
    fn acl_get_link_np(path: *const c_char, kind: c_int) -> *mut c_void;
    /// `int acl_get_entry(acl_t acl, int entry_id, acl_entry_t *entry)`.
    fn acl_get_entry(acl: *mut c_void, entry_id: c_int, entry: *mut *mut c_void) -> c_int;
    /// `int acl_get_tag_type(acl_entry_t entry, acl_tag_t *tag)`.
    fn acl_get_tag_type(entry: *mut c_void, tag: *mut c_int) -> c_int;
    /// `int acl_free(void *object)`.
    fn acl_free(object: *mut c_void) -> c_int;
}

/// Whether no entry of the access list of `path` allows anything. A link
/// at `path` is read as itself.
///
/// # Errors
///
/// [`ProgramRefused::Unreadable`] naming `path` for an entry that allows,
/// an entry of another kind, or a list that cannot be read.
#[allow(unsafe_code)] // calls into the operating system; each is explained where it is made
pub(super) fn no_entry_allows(path: &Path) -> Result<(), ProgramRefused> {
    let refused = || ProgramRefused::Unreadable(path.to_path_buf());
    let text = CString::new(path.as_os_str().as_bytes()).map_err(|_| refused())?;
    // SAFETY: `text` is NUL-terminated and outlives the call, which only
    // reads it; it returns a list it allocates, or null.
    let list = unsafe { acl_get_link_np(text.as_ptr(), ACL_TYPE_EXTENDED) };
    if list.is_null() {
        return match io::Error::last_os_error().raw_os_error() {
            Some(ENOENT | ENOTSUP | EOPNOTSUPP) => Ok(()),
            _ => Err(refused()),
        };
    }
    let mut verdict = Err(refused());
    let mut which = ACL_FIRST_ENTRY;
    for _ in 0..=ACL_MAX_ENTRIES {
        let mut entry: *mut c_void = ptr::null_mut();
        // SAFETY: `list` is the list allocated above and not yet freed; the
        // call writes one pointer, ours, to an entry inside it.
        let got = unsafe { acl_get_entry(list, which, &raw mut entry) };
        which = ACL_NEXT_ENTRY;
        if got != 0 {
            // No entry is left.
            verdict = Ok(());
            break;
        }
        if entry.is_null() {
            break;
        }
        let mut tag: c_int = 0;
        // SAFETY: `entry` points into `list`, still allocated; the call
        // writes one integer, ours.
        let read = unsafe { acl_get_tag_type(entry, &raw mut tag) };
        if read != 0 || tag != ACL_EXTENDED_DENY {
            break;
        }
    }
    // SAFETY: `list` was allocated by `acl_get_link_np`, whose manual names
    // this call to free it; it is freed once, after the last use of `list`
    // and of the entries inside it.
    unsafe {
        acl_free(list);
    }
    verdict
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::super::{ProgramRefused, ProgramRole, ProgramTrust, check_program};

    /// A folder of the test's own under the temporary folder, removed when
    /// dropped, with a program in it.
    struct Place(PathBuf);

    impl Place {
        fn new(name: &str) -> Self {
            use std::os::unix::fs::PermissionsExt as _;
            let root = std::env::temp_dir()
                .join(format!("a2m-provider-acl-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("real")).unwrap();
            let root = std::fs::canonicalize(&root).unwrap();
            let program = root.join("real").join("example-tool");
            std::fs::write(&program, [0xcf, 0xfa, 0xed, 0xfe]).unwrap();
            for (path, mode) in [
                (&root, 0o755),
                (&root.join("real"), 0o755),
                (&program, 0o755),
            ] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
            }
            Self(root)
        }

        fn program(&self) -> PathBuf {
            self.0.join("real").join("example-tool")
        }
    }

    impl Drop for Place {
        fn drop(&mut self) {
            let _ = Command::new("/bin/chmod")
                .args(["-R", "-N"])
                .arg(&self.0)
                .output();
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Adds an entry to the access list of `path`, in the form `chmod +a`
    /// takes.
    fn add(path: &Path, entry: &str) {
        let made = Command::new("/bin/chmod")
            .arg("+a")
            .arg(entry)
            .arg(path)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
    }

    fn program_check(path: &Path) -> Result<(), ProgramRefused> {
        check_program(path, &ProgramTrust::default(), ProgramRole::Program)
    }

    /// An entry that lets everyone write the program, or add to a folder
    /// on the way, leaves the mode as it was and is refused; an entry that
    /// denies is passed over (finding D3, decision 0271).
    #[test]
    fn an_entry_that_allows_is_refused_and_one_that_denies_is_not() {
        let place = Place::new("allow");
        assert_eq!(program_check(&place.program()), Ok(()));
        add(&place.program(), "everyone deny delete");
        add(&place.0, "everyone deny delete");
        assert_eq!(program_check(&place.program()), Ok(()));
        add(&place.program(), "everyone allow write");
        assert_eq!(
            program_check(&place.program()),
            Err(ProgramRefused::Unreadable(place.program()))
        );

        let place = Place::new("folder");
        let holder = place.0.join("real");
        add(&holder, "everyone allow add_file");
        assert_eq!(
            program_check(&place.program()),
            Err(ProgramRefused::Unreadable(holder))
        );
    }
}
