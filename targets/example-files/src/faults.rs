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
    /// These bytes are added at the end, as a program that keeps a log adds
    /// a line. Saved in place, the file keeps its identity.
    Appended(Vec<u8>),
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
    /// Standing: the file is marked as one whose bytes are not on this
    /// machine, as a sync program marks a file it keeps elsewhere: on
    /// Windows by the offline attribute, which a program of the user may
    /// set. The bytes stay on disk, so the stand can still tell that
    /// nothing changed them. Windows only: a macOS dataless file is made
    /// only by a file provider, and Linux has no such mark.
    NotOnThisMachine {
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
    /// Act: a link appears below the root that points at another name below
    /// the root. Nothing need be at that other name. Unix, and Windows where
    /// the user may make links.
    LinkInside {
        /// The link's name below the root. Nothing is there before.
        at: String,
        /// The name below the root the link points at.
        to: String,
    },
    /// Act: a file or a folder is moved to another name below the root, and a
    /// link that points at it is left under its old name. Unix, and Windows
    /// where the user may make links.
    MovedBehindLink {
        /// The name below the root of what is moved, and then of the link.
        name: String,
        /// The name below the root it is moved to.
        to: String,
    },
    /// Act: the file is given to another owner and another group. Unix only,
    /// and only when the tests run as the superuser, who alone may give a
    /// file away.
    OwnedByAnother {
        /// The file's name below the root.
        file: String,
    },
    /// Standing: the file has exactly these permission bits. With bits that
    /// leave this user no right to write the file, the stand tries to open it
    /// for writing itself, and calls the fault unsupported if it can: the
    /// superuser is not held by the bits. Unix only.
    FileMode {
        /// The file's name below the root.
        file: String,
        /// The permission bits, such as `0o640`.
        mode: u32,
        /// Whether the bits are meant to stop this user writing the file.
        denies_this_user: bool,
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
                | Self::NotOnThisMachine { .. }
                | Self::ReadOnlyFile { .. }
                | Self::ReadOnlyFolder { .. }
                | Self::FileMode { .. }
        )
    }

    /// Whether this kind of fault exists on the operating system the test was
    /// built for. A fault that exists may still be refused when it is set: a
    /// link by a user who may not make one, a read-only folder by the superuser.
    #[must_use]
    pub fn exists_here(&self) -> bool {
        match self {
            Self::HeldWithoutSharing { .. } | Self::NotOnThisMachine { .. } => cfg!(windows),
            Self::ReadOnlyFolder { .. } | Self::OwnedByAnother { .. } | Self::FileMode { .. } => {
                cfg!(unix)
            }
            Self::LinkOut { .. }
            | Self::FolderLinkOut { .. }
            | Self::LinkInside { .. }
            | Self::MovedBehindLink { .. } => cfg!(any(unix, windows)),
            _ => true,
        }
    }
}
