//! The room the evidence store has (spec section 224): below the setting
//! `store.free_space_floor`, the kernel is `degraded` with the part
//! `evidence_space`, so writes stop and reads keep the lowest capture
//! level.

use std::path::Path;

/// How many bytes are free to this process on the volume that holds a
/// folder. The kernel measures through one of these, so that a test can
/// say how much room there is without filling a disk.
pub trait FreeSpace: Send + Sync {
    /// The free bytes on the volume of `folder`, or of its nearest folder
    /// that exists; `None` when it cannot be measured.
    fn free_bytes(&self, folder: &Path) -> Option<u64>;
}

/// The volume's own count, from the operating system. Measured on Linux
/// and macOS; on Windows it is not measured yet, and the kernel is never
/// `degraded` for want of room there.
#[derive(Debug, Clone, Copy, Default)]
pub struct VolumeFreeSpace;

impl FreeSpace for VolumeFreeSpace {
    fn free_bytes(&self, folder: &Path) -> Option<u64> {
        let mut at = folder;
        while !at.exists() {
            at = at.parent()?;
        }
        measured(at)
    }
}

#[cfg(unix)]
fn measured(at: &Path) -> Option<u64> {
    // The blocks free to a process that is not the administrator's, in
    // units of the fragment size.
    let stat = rustix::fs::statvfs(at).ok()?;
    Some(stat.f_bavail.saturating_mul(stat.f_frsize))
}

#[cfg(not(unix))]
fn measured(_at: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::{FreeSpace, VolumeFreeSpace};

    #[test]
    fn the_volume_of_a_folder_that_does_not_exist_yet_is_measured_at_its_parent() {
        let folder = std::env::temp_dir()
            .join("a2m-space-test-not-made")
            .join("blobs");
        let measured = VolumeFreeSpace.free_bytes(&folder);
        if cfg!(unix) {
            assert!(measured.is_some_and(|bytes| bytes > 0), "{measured:?}");
        } else {
            assert_eq!(measured, None);
        }
    }
}
