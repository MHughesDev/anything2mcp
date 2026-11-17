//! Faults a test can switch on, so that a failure is reproduced on demand.
//!
//! Every fault is a setting given to [`crate::harness::FileStand::set_fault`].
//! Some are **standing**: they hold until [`crate::harness::FileStand::clear_faults`]
//! or a reset. The others are **acts of another writer**: they happen once, at the
//! moment they are set, and what they left on disk stays until a reset.

use crate::seed;

/// How another writer saves a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveStyle {
    /// It writes over the file where it is. The file keeps its identity.
    InPlace,
    /// It writes a new file beside the old one and puts it in the old one's
    /// place in one step. The file has a new identity.
    Replace,
}

/// What another writer changes in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// The value at the place the stand's cases write to. The README names the
    /// place for each file that has one.
    ThePlace,
    /// A value somewhere else in the file, the place left as it was.
    Elsewhere,
    /// The layout: a column, a key, or a sheet gets another name.
    Layout,
    /// The first occurrence of `find` becomes `replace`.
    Bytes {
        /// The bytes to look for.
        find: Vec<u8>,
        /// The bytes to put there.
        replace: Vec<u8>,
    },
    /// The whole file becomes these bytes.
    Whole(Vec<u8>),
}

/// One fault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Act: another writer changes a file. Set it between the read and the write
    /// of whatever is under test.
    ChangedByAnother {
        /// The file's name below the root.
        file: String,
        /// What changes.
        change: Change,
        /// How the other writer saves.
        how: SaveStyle,
    },
    /// Act: a file is removed.
    FileRemoved {
        /// The file's name below the root.
        file: String,
    },
    /// Act: a folder disappears, with everything in it.
    FolderRemoved {
        /// The folder's name below the root.
        folder: String,
    },
    /// Act: a partial write is left behind. A file named `temp_name` appears in
    /// the same folder as `beside`, holding the first half of its bytes.
    PartialWrite {
        /// The file the unfinished save was meant for.
        beside: String,
        /// The left-over file's own name, with no folder.
        temp_name: String,
    },
    /// Standing: the lock signal is present. An empty file with this name exists.
    LockSignal {
        /// The signal file's name below the root.
        sibling: String,
    },
    /// Standing: a program holds the file open and shares it with nobody.
    /// Windows only.
    HeldWithoutSharing {
        /// The file's name below the root.
        file: String,
    },
    /// Standing: the file is marked read-only. On Windows this also stops the
    /// file being replaced; on Unix it does not, because the folder decides that.
    ReadOnlyFile {
        /// The file's name below the root.
        file: String,
    },
    /// Standing: nothing can be made, replaced, or removed in the folder. This
    /// is how the stand shows a save that fails for want of permission. Unix
    /// only, and not when the tests run as the superuser.
    ReadOnlyFolder {
        /// The folder's name below the root.
        folder: String,
    },
    /// Act: a link appears below the root that points at the file outside it.
    /// Unix, and Windows where the user may make links.
    LinkOut {
        /// The link's name below the root.
        at: String,
    },
    /// Act: a link appears below the root that points at the folder outside it.
    /// Unix, and Windows where the user may make links.
    FolderLinkOut {
        /// The link's name below the root.
        at: String,
    },
    /// Act: a file gets a second name, so that it has two links.
    SecondLink {
        /// The file's name below the root.
        file: String,
        /// The second name below the root.
        at: String,
    },
}

impl Fault {
    /// Another writer changes `file`, saving the way a careful program does: a
    /// new file put in the old one's place.
    #[must_use]
    pub fn changed(file: &str, change: Change) -> Self {
        Self::ChangedByAnother {
            file: file.to_owned(),
            change,
            how: SaveStyle::Replace,
        }
    }

    /// The lock signal for `file`: a sibling named as the file with `.lock` added.
    #[must_use]
    pub fn lock_signal_for(file: &str) -> Self {
        Self::LockSignal {
            sibling: format!("{file}.lock"),
        }
    }

    /// The seed workbook saved by another writer with a formula where the unit
    /// price of item `EX-1002` is declared to be a value.
    #[must_use]
    pub fn formula_at_declared_value() -> Self {
        Self::changed(
            seed::PRICE_LIST_WORKBOOK,
            Change::Whole(seed::workbook(&seed::Workbook {
                formula_at_declared_value: true,
                ..seed::Workbook::default()
            })),
        )
    }

    /// Whether the fault holds until it is cleared, as opposed to happening once.
    #[must_use]
    pub fn is_standing(&self) -> bool {
        matches!(
            self,
            Self::LockSignal { .. }
                | Self::HeldWithoutSharing { .. }
                | Self::ReadOnlyFile { .. }
                | Self::ReadOnlyFolder { .. }
        )
    }

    /// Whether this kind of fault exists on the operating system the test was
    /// built for. A fault that exists may still be refused when it is set: a
    /// link by a user who may not make one, a read-only folder by the superuser.
    #[must_use]
    pub fn exists_here(&self) -> bool {
        match self {
            Self::HeldWithoutSharing { .. } => cfg!(windows),
            Self::ReadOnlyFolder { .. } => cfg!(unix),
            Self::LinkOut { .. } | Self::FolderLinkOut { .. } => cfg!(any(unix, windows)),
            _ => true,
        }
    }
}
