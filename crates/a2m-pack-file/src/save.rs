//! Saving: the new bytes go to a temporary file in the same folder, are
//! flushed to disk, and then take the old file's name in one call of the
//! operating system. Before that call the old file is untouched; after it
//! the new file is whole. The file at the path is never a mixture.
//!
//! Every way this can stop is a named failure that says whether the replace
//! was called. Where the operating system's answer leaves it in doubt, the
//! file is read once more and its digest decides.
//!
//! The temporary file is the provider contract's `Temporary`, made by
//! `CheckedPath::create_temporary` (decisions 0162, 0176, and 0182): under
//! a name of 128 random bits, only where nothing stands and never through a
//! link, readable and writable by the engine's user alone when it is made,
//! given the old file's owner, group, and permission bits through its own
//! handle before a byte is written (`keep_owner_and_mode`), read back
//! through that handle, put in place by a rename that replaces only what
//! stands at the name and refuses a link there, and removed, when the save
//! does not finish, only while its name still holds the file it made. The
//! folder is flushed after the replace (`CheckedPath::flush_folder`). The
//! contract's codes are mapped to the pack's own.

use a2m_core::{Digest, Failure};
use a2m_provider::{StepFailure, Temporary};

use crate::codes::{self, stop};
use crate::open::{locked, missing, not_allowed, unavailable};
use crate::place::{Named, not_writable};

/// What one save needs.
pub(crate) struct SaveJob {
    /// The file to replace, by its own name.
    pub target: Named,
    /// The sibling whose presence shows an application holds the file.
    pub lock_signal: Option<String>,
    /// The bytes to save.
    pub bytes: Vec<u8>,
    /// The digest the file had when it was read.
    pub before: Digest,
    /// The digest the file has once the save lands.
    pub intended: Digest,
    /// The most bytes read when the file is digested again.
    pub max_bytes: u64,
}

fn storage_full() -> StepFailure {
    stop(
        codes::STORAGE_FULL,
        "The disk or the folder is full, so the new file could not be written. The old file is untouched.",
        "Free space in the folder, then try again.",
    )
}

/// A failure of a call of the provider contract, under the pack's own code
/// where the pack has one that means the same: a file that is not there is
/// `file_missing`, one the engine's user may not write `file_protected`,
/// one another program holds `file_locked`, and a full disk
/// `storage_full`. The contract's other codes, such as `path_not_allowed`
/// and `file_unavailable`, are the pack's own words too and pass as they
/// are. So no two codes of the pack mean one thing.
pub(crate) fn from_contract(failure: Failure) -> StepFailure {
    let code = failure.code();
    if *code == a2m_provider::codes::PATH_MISSING {
        missing()
    } else if *code == a2m_provider::codes::PATH_DENIED {
        not_writable()
    } else if *code == a2m_provider::codes::PATH_LOCKED {
        StepFailure::not_sent(locked())
    } else if *code == a2m_provider::codes::DISK_FULL {
        storage_full()
    } else {
        StepFailure::not_sent(failure)
    }
}

/// The temporary file could not be made: a folder the engine's user may
/// not write is the folder's, not the file's.
fn not_made(failure: Failure) -> StepFailure {
    if *failure.code() == a2m_provider::codes::PATH_DENIED {
        StepFailure::not_sent(not_allowed("its folder cannot be written to"))
    } else {
        gone_is_changed(from_contract(failure))
    }
}

/// Gives the temporary file the old file's owner, group, and permission
/// bits, through its own handle, and looks that it has them.
///
/// A `false` stops the save by name, unsent, on every system. On Unix the
/// old file carries a set-user, set-group, or sticky bit, which is never
/// copied onto a file the engine wrote, or the system refuses the owner or
/// the group to the engine's user. On Windows the new file takes the access
/// list its folder passes down, and the answer is `true` only where the old
/// file, opened itself, has that very owner and list: a file with an access
/// list of its own is never replaced with a wider one (decisions 0193 and
/// 0194).
fn keep(temporary: &mut Temporary) -> Result<(), StepFailure> {
    match temporary.keep_owner_and_mode() {
        Ok(true) => Ok(()),
        Ok(false) | Err(_) => Err(not_kept()),
    }
}

fn not_kept() -> StepFailure {
    stop(
        codes::METADATA_NOT_KEPT,
        "The old file's owner, group, or permissions cannot be carried to the new file. Nothing was written.",
        "Check who owns the folder and the file, whether the engine's user may give a file to that owner and group, whether the file carries a set-user, set-group, or sticky bit, and, on Windows, whether the file has an access list of its own rather than its folder's.",
    )
}

fn changed() -> StepFailure {
    stop(
        codes::CHANGED_SINCE_PREVIEW,
        "The file changed after it was read, so it was not replaced.",
        "Read the record again, look at the new preview, and approve again.",
    )
}

/// A file that was there when it was read and is gone now changed.
fn gone_is_changed(failure: StepFailure) -> StepFailure {
    if failure.failure.code() == &codes::FILE_MISSING {
        changed()
    } else {
        failure
    }
}

fn held(job: &SaveJob) -> Result<(), StepFailure> {
    match &job.lock_signal {
        Some(sibling) if job.target.folder().holds(sibling)? => {
            Err(StepFailure::not_sent(locked()))
        }
        _ => Ok(()),
    }
}

/// Whether the temporary file holds, from its start, exactly the bytes the
/// save built: read back through the handle it was written with, so no
/// other file can stand in for it.
fn holds_what_was_built(
    temporary: &mut Temporary,
    intended: Digest,
    max_bytes: u64,
) -> Result<(), StepFailure> {
    let back = temporary.read_back(max_bytes).map_err(|_| {
        StepFailure::not_sent(unavailable("could not be read back beside the old one"))
    })?;
    if Digest::of(&back) == intended {
        Ok(())
    } else {
        Err(StepFailure::not_sent(unavailable(
            "was not written beside the old one as it was built; the old file is untouched",
        )))
    }
}

/// After the replace: the folder's list of names is written to the disk, by
/// the contract's call. Where that fails, the new file has its name and the
/// name may not outlast a loss of power, so the save is not reported done:
/// its fate is unknown, and reconcile reads the file.
fn flushed(job: &SaveJob) -> Result<(), StepFailure> {
    job.target.checked().flush_folder().map_err(|_| {
        StepFailure::unknown(unavailable(
            "was replaced, and its folder could not be written to the disk",
        ))
    })
}

/// After the replace: the path must lead to the very file the save wrote,
/// so the rename acted in the folder at the path, and the folder flushed is
/// the one whose names changed. Where it does not, the folder was swapped
/// in the calls between: the write landed somewhere, and not at the path,
/// so its fate is unknown, and reconcile reads the file.
fn placed_at_the_path(job: &SaveJob, made: a2m_provider::FileIdentity) -> Result<(), StepFailure> {
    if job.target.checked().identity().ok() != Some(made) {
        return Err(StepFailure::unknown(unavailable(
            "was replaced in a folder that no longer stands at its path",
        )));
    }
    flushed(job)
}

/// A look of the contract at the file: one that finds it gone is a change.
fn gone_is_changed_contract(failure: Failure) -> StepFailure {
    gone_is_changed(from_contract(failure))
}

/// What the file's own bytes say after the operating system refused the
/// replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refused {
    /// The file is the new one: the replace took place all the same.
    Landed,
    /// The file is the one that was read: nothing was replaced.
    NotReplaced,
    /// The file is neither, or cannot be read: the fate is unknown.
    Unknown,
}

fn after_refusal(now: Option<Digest>, before: Digest, intended: Digest) -> Refused {
    if now == Some(intended) && intended != before {
        Refused::Landed
    } else if now == Some(before) {
        Refused::NotReplaced
    } else {
        Refused::Unknown
    }
}

/// What a refused replace that left the old file in place is told as, by
/// the contract's code for the refusal.
fn refusal(failure: Failure) -> Failure {
    let code = failure.code();
    if *code == a2m_provider::codes::PATH_LOCKED
        || (*code == a2m_provider::codes::PATH_DENIED && cfg!(windows))
    {
        // Windows refuses a replace as a denial too while another program
        // holds the file.
        locked()
    } else if *code == a2m_provider::codes::PATH_DENIED {
        not_allowed("its folder cannot be written to")
    } else if *code == a2m_provider::codes::DISK_FULL {
        codes::failure(
            codes::STORAGE_FULL,
            "The disk or the folder is full, so the file could not be replaced. The old file is untouched.",
            "Free space in the folder, then try again.",
        )
    } else if *code == a2m_provider::codes::PATH_NOT_ALLOWED {
        failure
    } else {
        unavailable("could not be replaced; the old file is untouched")
    }
}

/// Replaces the file. Blocking; never retried.
pub(crate) fn save(job: &SaveJob) -> Result<(), StepFailure> {
    save_with(job, || {})
}

/// The same, with `before_last_look` called once the temporary file is
/// ready and just before the last look: where a test puts what another
/// process may do in that moment.
fn save_with(job: &SaveJob, before_last_look: impl FnOnce()) -> Result<(), StepFailure> {
    let folder = job.target.folder();
    // The file is looked at again, as the name itself: a regular file with
    // one name, in a folder no link leads to, that its owner did not mark
    // read-only, that the engine's user may write, and that nobody holds.
    folder.reached_by_no_link()?;
    let old = job.target.regular().map_err(gone_is_changed)?;
    if old.permissions().readonly() {
        return Err(stop(
            codes::FILE_PROTECTED,
            "The file is marked read-only, and the file pack replaces no file its owner protected.",
            "Remove the read-only mark if the file is meant to be changed.",
        ));
    }
    // A replace needs only the right to write the folder. The pack asks for
    // more: the right to write the file itself, which the contract asks of
    // the operating system, following no link. Nothing is written.
    match job.target.checked().may_write() {
        Ok(true) => {}
        Ok(false) => return Err(not_writable()),
        Err(failure) => return Err(gone_is_changed(from_contract(failure))),
    }
    held(job)?;

    // The file the name holds now, by identity. The temporary file is made
    // in the folder the path leads to now, through that folder's handle,
    // and the rename goes through the same handle; the lock signal and the
    // digest are looked at by path. Each later look is held to this file,
    // so that they all see the one folder the rename acts in (FI-1,
    // decision 0194).
    let identity = job
        .target
        .checked()
        .identity()
        .map_err(gone_is_changed_contract)?;

    // Dropped on any way out below before the replace, the temporary file
    // is removed, while its name still holds the file it made.
    let mut temporary = job.target.checked().create_temporary().map_err(not_made)?;
    let made = temporary.identity().map_err(from_contract)?;
    // Owner, group, and permissions first, while the file is empty: the new
    // bytes are never readable by anyone the old file was not readable by.
    keep(&mut temporary)?;
    temporary.write(&job.bytes).map_err(from_contract)?;
    // Writing can take bits off a file. They are set again, and the file is
    // looked at once more, before it is flushed.
    keep(&mut temporary)?;
    temporary.flush().map_err(from_contract)?;
    // What the temporary file holds is read back through its own handle
    // and must be, byte for byte, what was built.
    holds_what_was_built(&mut temporary, job.intended, job.max_bytes)?;

    // Everything is ready. What follows is the last look at the lock
    // signal, the comparison, and the replace, with nothing else between
    // them: the file at the name must still be, byte for byte, the file
    // that was read.
    before_last_look();
    held(job)?;
    if job.target.digest(job.max_bytes) != Some(job.before) {
        return Err(changed());
    }
    // The path still leads to the file the save began with: a folder
    // moved away and a copy put at its name since is a change, unsent.
    if job.target.checked().identity().ok() != Some(identity) {
        return Err(changed());
    }
    // The commit point: one call, and the name is the new file's. The call
    // follows no link at either name, and refuses one at the target's.
    match temporary.replace() {
        Ok(()) => placed_at_the_path(job, made),
        Err(failure) => {
            // The system said no. The file's own bytes say what happened.
            match after_refusal(job.target.digest(job.max_bytes), job.before, job.intended) {
                Refused::Landed => placed_at_the_path(job, made),
                Refused::Unknown => Err(StepFailure::unknown(unavailable(
                    "could not be replaced, and it is no longer the file that was read",
                ))),
                Refused::NotReplaced => Err(StepFailure::answered(refusal(failure))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use a2m_provider::{Access, CheckedPath, RootName, Roots, Sent};

    use super::*;

    /// A folder of its own in the system's temporary folder, removed when
    /// dropped, and a file in it as the roots' check gives it.
    struct Scratch {
        path: PathBuf,
        roots: Roots,
    }

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "a2m-pack-file-save-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            let mut roots = Roots::none();
            roots
                .add(RootName::new("files").unwrap(), &path, Access::Write)
                .unwrap();
            Self { path, roots }
        }

        fn file(&self, name: &str) -> CheckedPath {
            self.roots
                .check(&RootName::new("files").unwrap(), name, Access::Write)
                .unwrap()
        }

        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.path)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn the_contracts_codes_come_back_as_the_packs_own() {
        use a2m_core::FailureClass;
        let mapped = |class, code| {
            let failure = from_contract(Failure::new(class, code, "a message", "a next step"));
            (
                failure.failure.class(),
                failure.failure.code().clone(),
                failure.sent,
            )
        };
        let provider = |text: &str| a2m_core::FailureCode::new(text).unwrap();
        for (contract, class, code) in [
            (
                a2m_provider::codes::PATH_MISSING,
                FailureClass::Drift,
                codes::FILE_MISSING,
            ),
            (
                a2m_provider::codes::PATH_DENIED,
                FailureClass::Policy,
                codes::FILE_PROTECTED,
            ),
            (
                a2m_provider::codes::PATH_LOCKED,
                FailureClass::Busy,
                codes::FILE_LOCKED,
            ),
            (
                a2m_provider::codes::DISK_FULL,
                FailureClass::Busy,
                codes::STORAGE_FULL,
            ),
        ] {
            assert_eq!(
                mapped(FailureClass::Input, contract),
                (class, code, Sent::NotSent)
            );
        }
        // The contract's words that are the pack's own too pass as they are.
        assert_eq!(
            mapped(FailureClass::Input, a2m_provider::codes::PATH_NOT_ALLOWED),
            (
                FailureClass::Input,
                provider("path_not_allowed"),
                Sent::NotSent
            )
        );
        // A refused replace that left the old file is told by the pack's
        // own words, and a link refused at the name as itself.
        let told = |code| {
            refusal(Failure::new(FailureClass::Input, code, "m", "n"))
                .code()
                .clone()
        };
        assert_eq!(told(a2m_provider::codes::PATH_LOCKED), codes::FILE_LOCKED);
        assert_eq!(told(a2m_provider::codes::DISK_FULL), codes::STORAGE_FULL);
        assert_eq!(
            told(a2m_provider::codes::PATH_NOT_ALLOWED),
            provider("path_not_allowed")
        );
    }

    #[test]
    fn a_temporary_file_is_used_only_when_it_holds_what_was_built_and_is_removed_unused() {
        let scratch = Scratch::new();
        fs::write(scratch.path.join("stock.csv"), b"the old bytes").unwrap();
        let target = scratch.file("stock.csv");
        let mut temporary = target.create_temporary().unwrap();
        // Made for the engine's user alone, before anything is written.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let made: Vec<String> = scratch
                .names()
                .into_iter()
                .filter(|name| name != "stock.csv")
                .collect();
            let [made] = made.as_slice() else {
                panic!("{made:?}");
            };
            let mode = fs::metadata(scratch.path.join(made))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
        temporary.write(b"the new bytes").unwrap();
        assert!(holds_what_was_built(&mut temporary, Digest::of(b"the new bytes"), 1024).is_ok());
        let other =
            holds_what_was_built(&mut temporary, Digest::of(b"other bytes"), 1024).unwrap_err();
        assert_eq!(other.sent, Sent::NotSent);
        // More than may be read back is not what was built.
        assert!(holds_what_was_built(&mut temporary, Digest::of(b"the new bytes"), 4).is_err());
        // Unused, it is removed, and the old file is as it was.
        drop(temporary);
        assert_eq!(scratch.names(), ["stock.csv"]);
        assert_eq!(
            fs::read(scratch.path.join("stock.csv")).unwrap(),
            b"the old bytes"
        );
    }

    /// A folder moved away while a save is ready, and a copy put at its
    /// name, with the same file and the same bytes: the save stops unsent,
    /// and neither the moved folder's file nor the copy is changed (FI-1).
    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_before_the_last_look_stops_the_save_unsent() {
        let scratch = Scratch::new();
        let prices = scratch.path.join("prices");
        fs::create_dir_all(&prices).unwrap();
        fs::write(prices.join("stock.csv"), b"the old bytes").unwrap();
        let name = RootName::new("files").unwrap();
        let root = scratch.roots.folder(&name, Access::Write).unwrap();
        let folder = crate::place::Folder::new(
            scratch.roots.check(&name, "prices", Access::Write).unwrap(),
            &root,
        );
        let job = SaveJob {
            target: folder.file("stock.csv").unwrap(),
            lock_signal: None,
            bytes: b"the new bytes".to_vec(),
            before: Digest::of(b"the old bytes"),
            intended: Digest::of(b"the new bytes"),
            max_bytes: 1024,
        };
        let moved = scratch.path.join("moved");
        let stopped = save_with(&job, || {
            fs::rename(&prices, &moved).unwrap();
            fs::create_dir_all(&prices).unwrap();
            fs::write(prices.join("stock.csv"), b"the old bytes").unwrap();
        })
        .unwrap_err();
        assert_eq!(stopped.failure.code(), &codes::CHANGED_SINCE_PREVIEW);
        assert_eq!(stopped.sent, Sent::NotSent);
        assert_eq!(fs::read(moved.join("stock.csv")).unwrap(), b"the old bytes");
        assert_eq!(
            fs::read(prices.join("stock.csv")).unwrap(),
            b"the old bytes"
        );
        // Nothing was left behind in either folder.
        assert_eq!(fs::read_dir(&moved).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&prices).unwrap().count(), 1);
    }

    #[test]
    fn after_a_refused_replace_the_files_own_bytes_decide() {
        let before = Digest::of(b"before");
        let intended = Digest::of(b"intended");
        let other = Digest::of(b"another writer's");
        // The new file is there: the replace took place.
        assert_eq!(
            after_refusal(Some(intended), before, intended),
            Refused::Landed
        );
        // The old file is there: nothing was replaced, and the caller is
        // told so with the request answered.
        assert_eq!(
            after_refusal(Some(before), before, intended),
            Refused::NotReplaced
        );
        // Neither, or no file that can be read: a write in doubt, never
        // "not written".
        assert_eq!(
            after_refusal(Some(other), before, intended),
            Refused::Unknown
        );
        assert_eq!(after_refusal(None, before, intended), Refused::Unknown);
        // A save that changes no byte proves nothing by its digest.
        assert_eq!(
            after_refusal(Some(before), before, before),
            Refused::NotReplaced
        );
    }
}
