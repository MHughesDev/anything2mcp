//! Saving: the new bytes go to a temporary file in the same folder, are
//! flushed to disk, and then take the old file's name in one call of the
//! operating system. Before that call the old file is untouched; after it
//! the new file is whole. The file at the path is never a mixture.
//!
//! Every way this can stop is a named failure that says whether the replace
//! was called. Where the operating system's answer leaves it in doubt, the
//! file is read once more and its digest decides.
//!
//! The save removes nothing it did not make. A temporary file gets a name
//! nobody can know beforehand and is made only if nothing has that name;
//! whatever has it is left exactly as it is. What the save made is known by
//! its identity, and only a file with that identity is ever removed.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::PathBuf;

use a2m_core::Digest;
use a2m_provider::StepFailure;

use crate::codes::{self, stop};
use crate::open::{held_by_another, locked, not_allowed, unavailable};
use crate::place::{Folder, Identity, Named, Open};

/// How many names are tried for a temporary file before the save stops.
const NAMES_TRIED: usize = 4;

/// What one save needs.
pub(crate) struct SaveJob {
    /// The file to replace, by its own name.
    pub target: Named,
    /// The file's name, which a temporary file's name starts with.
    pub name: String,
    /// Sixteen characters from the digest of the record's idempotency
    /// token: they say, to a person who finds a temporary file left by a
    /// save that was killed, which record it was for.
    pub token: String,
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

/// A temporary file this driver made: where, and which file it is.
#[derive(Debug)]
pub(crate) struct Made {
    path: PathBuf,
    identity: Option<Identity>,
}

impl Made {
    fn of(file: &File, path: PathBuf) -> Self {
        Self {
            path,
            identity: file
                .metadata()
                .ok()
                .and_then(|metadata| Identity::of(&metadata)),
        }
    }

    /// Removes the temporary file, when what is at its name is still the
    /// file this driver made. A link, a folder, another file that took the
    /// name, and a file whose identity the system does not give are left.
    pub(crate) fn remove(&self) {
        let Some(identity) = self.identity else {
            return;
        };
        if let Ok(found) = fs::symlink_metadata(&self.path)
            && found.file_type().is_file()
            && Identity::of(&found) == Some(identity)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// How one save ended.
pub(crate) struct Saved {
    /// Whether the file was replaced.
    pub outcome: Result<(), StepFailure>,
    /// A temporary file that may still be there: only when the fate of the
    /// replace is unknown. `release` removes it, by its identity.
    pub left: Option<Made>,
}

fn full(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::StorageFull | ErrorKind::QuotaExceeded)
}

fn storage_full() -> StepFailure {
    stop(
        codes::STORAGE_FULL,
        "The disk or the folder is full, so the new file could not be written. The old file is untouched.",
        "Free space in the folder, then try again.",
    )
}

/// Sixteen characters nobody can know beforehand, from the operating
/// system's randomness.
fn random_part() -> Result<String, StepFailure> {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| {
        StepFailure::not_sent(unavailable(
            "cannot be saved, because the operating system gave no randomness for a temporary file's name",
        ))
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Makes the temporary file: a new name each try, and a file only where
/// nothing has the name. The call that makes it follows no link at the
/// name, and what is found there is never removed, opened, or written. The
/// new file can be read and written by the engine's user alone until it is
/// given the old file's permissions.
fn create_temporary(
    folder: &Folder,
    name: &str,
    token: &str,
    random: &mut dyn FnMut() -> Result<String, StepFailure>,
) -> Result<(File, PathBuf), StepFailure> {
    for _ in 0..NAMES_TRIED {
        let path = folder.beside(&format!("{name}.a2m-{token}-{}.tmp", random()?))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((file, path)),
            // Something has the name. It is not this save's: try another.
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) if full(error.kind()) => return Err(storage_full()),
            Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                return Err(StepFailure::not_sent(not_allowed(
                    "its folder cannot be written to",
                )));
            }
            Err(_) => {
                return Err(StepFailure::not_sent(unavailable(
                    "cannot get a temporary file beside it",
                )));
            }
        }
    }
    Err(StepFailure::not_sent(unavailable(
        "cannot get a temporary file beside it: every name tried was taken, and the file pack removes nothing it did not make",
    )))
}

/// Gives the new file the old file's owner, group, and permission bits,
/// through its open handle, and then looks that it has them.
#[cfg(unix)]
fn carry(new: &File, old: &Metadata) -> std::io::Result<bool> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    const BITS: u32 = 0o7777;
    let made = new.metadata()?;
    let owner = (made.uid() != old.uid()).then_some(old.uid());
    let group = (made.gid() != old.gid()).then_some(old.gid());
    if owner.is_some() || group.is_some() {
        // Only one who may give the file to that owner and group can.
        std::os::unix::fs::fchown(new, owner, group)?;
    }
    new.set_permissions(fs::Permissions::from_mode(old.mode() & BITS))?;
    let kept = new.metadata()?;
    Ok(kept.uid() == old.uid()
        && kept.gid() == old.gid()
        && kept.mode() & BITS == old.mode() & BITS)
}

/// Gives the new file the old file's permissions. Outside Unix that is the
/// read-only mark alone, and a file that has it is never replaced.
#[cfg(not(unix))]
fn carry(new: &File, old: &Metadata) -> std::io::Result<bool> {
    new.set_permissions(old.permissions())?;
    Ok(true)
}

fn not_kept() -> StepFailure {
    stop(
        codes::METADATA_NOT_KEPT,
        "The old file's owner, group, or permissions cannot be carried to the new file. Nothing was written.",
        "Check who owns the folder and the file, and whether the engine's user may give a file to that owner and group.",
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

/// Replaces the file. Blocking; never retried.
pub(crate) fn save(job: &SaveJob) -> Saved {
    let mut left = None;
    let outcome = replace(job, &mut random_part, &mut left);
    Saved { outcome, left }
}

fn replace(
    job: &SaveJob,
    random: &mut dyn FnMut() -> Result<String, StepFailure>,
    left: &mut Option<Made>,
) -> Result<(), StepFailure> {
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
    // more: the right to write the file itself. Nothing is written here.
    drop(
        job.target
            .open(Open::WriteProbe)
            .map_err(gone_is_changed)?,
    );
    held(job)?;

    let (mut file, path) = create_temporary(folder, &job.name, &job.token, random)?;
    // Owner, group, and permissions first, while the file is empty: the new
    // bytes are never readable by anyone the old file was not readable by.
    let prepared = match carry(&file, &old) {
        Ok(true) => file
            .write_all(&job.bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                if full(error.kind()) {
                    storage_full()
                } else {
                    StepFailure::not_sent(unavailable("could not be written beside the old one"))
                }
            }),
        Ok(false) | Err(_) => Err(not_kept()),
    };
    let made = Made::of(&file, path);
    drop(file);
    if let Err(failure) = prepared {
        made.remove();
        return Err(failure);
    }

    // Everything is ready. What follows is the last look at the lock
    // signal, the comparison, and the replace, with nothing else between
    // them: the file at the name must still be, byte for byte, the file
    // that was read.
    if let Err(failure) = held(job) {
        made.remove();
        return Err(failure);
    }
    if job.target.digest(job.max_bytes) != Some(job.before) {
        made.remove();
        return Err(changed());
    }
    // The commit point: one call, and the name is the new file's. The call
    // follows no link at either name.
    match fs::rename(&made.path, job.target.path()) {
        Ok(()) => {
            folder.flush();
            Ok(())
        }
        Err(error) => {
            // The system said no. The file's own bytes say what happened.
            match after_refusal(
                job.target.digest(job.max_bytes),
                job.before,
                job.intended,
            ) {
                Refused::Landed => {
                    folder.flush();
                    Ok(())
                }
                Refused::Unknown => {
                    *left = Some(made);
                    Err(StepFailure::unknown(unavailable(
                        "could not be replaced, and it is no longer the file that was read",
                    )))
                }
                Refused::NotReplaced => {
                    made.remove();
                    Err(StepFailure::answered(if held_by_another(&error) {
                        locked()
                    } else if error.kind() == ErrorKind::PermissionDenied && cfg!(windows) {
                        // Windows refuses a replace this way too while
                        // another program holds the file.
                        locked()
                    } else if error.kind() == ErrorKind::PermissionDenied {
                        not_allowed("its folder cannot be written to")
                    } else if full(error.kind()) {
                        codes::failure(
                            codes::STORAGE_FULL,
                            "The disk or the folder is full, so the file could not be replaced. The old file is untouched.",
                            "Free space in the folder, then try again.",
                        )
                    } else {
                        unavailable("could not be replaced; the old file is untouched")
                    }))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use a2m_provider::{Access, RootName, Roots, Sent};

    use super::*;

    /// A folder of its own in the system's temporary folder, removed when
    /// dropped, and the same folder as the pack is given one.
    struct Scratch {
        path: PathBuf,
        folder: Folder,
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
            let name = RootName::new("files").unwrap();
            let mut roots = Roots::none();
            roots.add(name.clone(), &path, Access::Write).unwrap();
            let root = roots.folder(&name, Access::Write).unwrap();
            let folder = Folder::new(root.clone(), &root);
            Self { path, folder }
        }

        fn at(&self, name: &str) -> PathBuf {
            // The root as the roots resolved it, so that a link in the way
            // to the system's temporary folder plays no part.
            self.folder.beside(name).unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn names(parts: &[&str]) -> impl FnMut() -> Result<String, StepFailure> {
        let mut parts: Vec<String> = parts.iter().rev().map(|part| (*part).to_owned()).collect();
        move || Ok(parts.pop().unwrap_or_else(|| "spent".to_owned()))
    }

    #[cfg(unix)]
    fn link(to: &Path, at: &Path) -> bool {
        std::os::unix::fs::symlink(to, at).is_ok()
    }

    #[cfg(windows)]
    fn link(to: &Path, at: &Path) -> bool {
        // Only a user who may make links can; elsewhere the test goes on
        // with a plain file at the name.
        std::os::windows::fs::symlink_file(to, at).is_ok()
    }

    #[cfg(not(any(unix, windows)))]
    fn link(_to: &Path, _at: &Path) -> bool {
        false
    }

    #[test]
    fn a_name_that_is_taken_is_left_as_it_is_and_another_is_tried() {
        let scratch = Scratch::new();
        let other = scratch.at("other.csv");
        fs::write(&other, b"another file").unwrap();
        // A link to another file at the first name, a file at the second.
        let first = scratch.at("stock.csv.a2m-token-one.tmp");
        if !link(&other, &first) {
            fs::write(&first, b"first").unwrap();
        }
        let before = fs::symlink_metadata(&first).unwrap().file_type();
        let second = scratch.at("stock.csv.a2m-token-two.tmp");
        fs::write(&second, b"second").unwrap();

        let mut random = names(&["one", "two", "three"]);
        let (file, path) =
            create_temporary(&scratch.folder, "stock.csv", "token", &mut random).unwrap();
        assert_eq!(path, scratch.at("stock.csv.a2m-token-three.tmp"));
        // Nothing that was there was removed, replaced, or written through.
        assert_eq!(fs::read(&other).unwrap(), b"another file");
        assert_eq!(fs::symlink_metadata(&first).unwrap().file_type(), before);
        assert_eq!(fs::read(&second).unwrap(), b"second");
        // Until it is given the old file's permissions, the new file is the
        // engine's user's alone.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(file.metadata().unwrap().permissions().mode() & 0o077, 0);
        }
        drop(file);

        // Every name tried is taken: the save stops, and removes nothing.
        let mut random = names(&["one", "two", "three", "two"]);
        let stopped = create_temporary(&scratch.folder, "stock.csv", "token", &mut random)
            .err()
            .unwrap();
        assert_eq!(stopped.sent, Sent::NotSent);
        assert_eq!(
            stopped.failure.code(),
            &a2m_provider::codes::FILE_UNAVAILABLE
        );
        assert_eq!(fs::read(&other).unwrap(), b"another file");
        assert!(path.exists() && second.exists());

        // A name that is not one name in the folder is refused before
        // anything is made.
        let mut random = names(&["../outside"]);
        assert!(create_temporary(&scratch.folder, "stock.csv", "token", &mut random).is_err());
    }

    #[test]
    fn only_the_file_this_driver_made_is_removed() {
        let scratch = Scratch::new();
        let mut random = names(&["one"]);
        let (file, path) =
            create_temporary(&scratch.folder, "stock.csv", "token", &mut random).unwrap();
        let made = Made::of(&file, path.clone());
        drop(file);

        // Another file takes the name, while the one that was made lives on
        // under another: the name no longer proves anything.
        let aside = scratch.at("moved-aside.tmp");
        fs::rename(&path, &aside).unwrap();
        fs::write(&path, b"somebody else's").unwrap();
        made.remove();
        assert_eq!(fs::read(&path).unwrap(), b"somebody else's");

        // A link at the name is left, and so is what it points at.
        fs::remove_file(&path).unwrap();
        if link(&aside, &path) {
            made.remove();
            assert!(fs::symlink_metadata(&path).is_ok() && aside.exists());
            fs::remove_file(&path).unwrap();
        }

        // The file that was made, back under its name, is removed.
        fs::rename(&aside, &path).unwrap();
        made.remove();
        assert!(fs::symlink_metadata(&path).is_err());
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
