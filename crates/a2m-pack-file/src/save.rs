//! Saving: the new bytes go to a temporary file in the same folder, are
//! flushed to disk, and then take the old file's name in one call of the
//! operating system. Before that call the old file is untouched; after it
//! the new file is whole. The file at the path is never a mixture.
//!
//! Every way this can stop is a named failure that says whether the replace
//! was called. Where the operating system's answer leaves it in doubt, the
//! file is read once more and its digest decides.

use std::io::{ErrorKind, Write as _};

use a2m_core::Digest;
use a2m_provider::{CheckedPath, StepFailure};

use crate::codes::{self, stop};
use crate::open::{held_by_another, locked, not_allowed, regular_file, unavailable};

/// What one save needs. Every path was made by the check against the roots.
pub(crate) struct SaveJob {
    /// The file to replace.
    pub target: CheckedPath,
    /// The temporary file, in the same folder, named with the record's token.
    pub temporary: CheckedPath,
    /// The sibling whose presence shows an application holds the file.
    pub lock_signal: Option<CheckedPath>,
    /// The bytes to save.
    pub bytes: Vec<u8>,
    /// The digest the file had when it was read.
    pub before: Digest,
    /// The digest the file has once the save lands.
    pub intended: Digest,
    /// The most bytes read when the file is digested again.
    pub max_bytes: u64,
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

/// The digest of the file at the path now, when it can be read.
fn digest_now(target: &CheckedPath, max_bytes: u64) -> Option<Digest> {
    use std::io::Read as _;
    let file = std::fs::File::open(target.as_path()).ok()?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    Some(Digest::of(&bytes))
}

fn remove(temporary: &CheckedPath) {
    // The temporary file is this save's own: it carries the record's token.
    let _ = std::fs::remove_file(temporary.as_path());
}

/// Flushes the folder, so that the new name survives a loss of power. Only
/// where the system allows a folder to be opened and flushed.
#[cfg(unix)]
fn flush_folder(target: &CheckedPath) {
    if let Some(folder) = target.as_path().parent()
        && let Ok(folder) = std::fs::File::open(folder)
    {
        let _ = folder.sync_all();
    }
}

#[cfg(not(unix))]
fn flush_folder(_target: &CheckedPath) {}

/// Writes the temporary file and flushes it to disk.
fn write_temporary(job: &SaveJob) -> Result<(), StepFailure> {
    let create = || {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(job.temporary.as_path())
    };
    let mut file = match create() {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            // Left by an earlier try of this same record, stopped half way.
            remove(&job.temporary);
            create().map_err(|_| {
                StepFailure::not_sent(unavailable(
                    "has a temporary file beside it that cannot be replaced",
                ))
            })?
        }
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
    };
    let written = file.write_all(&job.bytes).and_then(|()| file.sync_all());
    drop(file);
    written.map_err(|error| {
        remove(&job.temporary);
        if full(error.kind()) {
            storage_full()
        } else {
            StepFailure::not_sent(unavailable("could not be written beside the old one"))
        }
    })
}

/// Replaces the file. Blocking; never retried.
pub(crate) fn save(job: &SaveJob) -> Result<(), StepFailure> {
    let metadata = regular_file(&job.target).map_err(|failure| {
        // A file that was there when it was read and is gone now changed.
        if failure.failure.code() == &codes::FILE_MISSING {
            changed()
        } else {
            failure
        }
    })?;
    if metadata.permissions().readonly() {
        return Err(stop(
            codes::FILE_PROTECTED,
            "The file is marked read-only, and the file pack replaces no file its owner protected.",
            "Remove the read-only mark if the file is meant to be changed.",
        ));
    }
    if job.lock_signal.as_ref().is_some_and(CheckedPath::exists) {
        return Err(StepFailure::not_sent(locked()));
    }
    write_temporary(job)?;
    if std::fs::set_permissions(job.temporary.as_path(), metadata.permissions()).is_err() {
        remove(&job.temporary);
        return Err(stop(
            codes::METADATA_NOT_KEPT,
            "The old file's permissions cannot be carried to the new file. Nothing was written.",
            "Check who owns the folder and the file.",
        ));
    }
    if digest_now(&job.target, job.max_bytes) != Some(job.before) {
        remove(&job.temporary);
        return Err(changed());
    }
    // The commit point: one call, and the name is the new file's.
    match std::fs::rename(job.temporary.as_path(), job.target.as_path()) {
        Ok(()) => {
            flush_folder(&job.target);
            Ok(())
        }
        Err(error) => {
            // The system said no. The file's own bytes say what happened.
            let now = digest_now(&job.target, job.max_bytes);
            if now == Some(job.intended) && job.intended != job.before {
                flush_folder(&job.target);
                return Ok(());
            }
            if now != Some(job.before) {
                return Err(StepFailure::unknown(unavailable(
                    "could not be replaced, and it is no longer the file that was read",
                )));
            }
            remove(&job.temporary);
            Err(StepFailure::answered(if held_by_another(&error) {
                locked()
            } else if error.kind() == ErrorKind::PermissionDenied && cfg!(windows) {
                // Windows refuses a replace this way too while another
                // program holds the file.
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

fn changed() -> StepFailure {
    stop(
        codes::CHANGED_SINCE_PREVIEW,
        "The file changed after it was read, so it was not replaced.",
        "Read the record again, look at the new preview, and approve again.",
    )
}

/// The digest of the file at the path, for the confirm after a save.
pub(crate) fn digest_at(target: &CheckedPath, max_bytes: u64) -> Option<Digest> {
    target.size().ok()?;
    digest_now(target, max_bytes)
}
