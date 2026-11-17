//! What the control interface reports: the truth about one entry on disk, and
//! the stand's ledger of how each file changed between two looks.
//!
//! Everything here comes from the bytes and the metadata the operating system
//! gives for a path. Nothing is cached, and no parser is involved.

use std::collections::BTreeMap;
use std::fs::Metadata;

use sha2::{Digest as _, Sha256};

/// The SHA-256 digest of these bytes, as 64 lowercase hexadecimal characters.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// What sits at a name below the root. A link is never followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// Nothing is there.
    Absent,
    /// A regular file.
    File,
    /// A folder.
    Folder,
    /// A link to somewhere else, whatever it points at.
    Link,
    /// Something else.
    Other,
}

/// What tells one file from another that took its name. On Unix this is the
/// device and the file number; a file that was replaced has another one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    /// The device the file is on.
    pub device: u64,
    /// The file's number on that device.
    pub file: u64,
}

/// The truth about one name below the root, read from disk when it was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTruth {
    /// The name that was asked about.
    pub name: String,
    /// What is there.
    pub kind: EntryKind,
    /// The size in bytes of a regular file; zero for anything else.
    pub size: u64,
    /// The SHA-256 digest of a regular file's bytes; `None` for anything else.
    pub sha256: Option<String>,
    /// Whether the entry is marked read-only.
    pub read_only: bool,
    /// The file's identity. `None` where the platform does not give one to safe
    /// code: everywhere but Unix.
    pub identity: Option<FileIdentity>,
    /// How many names the file has. `None` everywhere but Unix.
    pub links: Option<u64>,
    /// The numbers of the entry's owner and group. `None` everywhere but Unix.
    pub owner: Option<(u32, u32)>,
    /// The entry's permission bits, such as `0o644`. `None` everywhere but Unix.
    pub mode: Option<u32>,
}

impl FileTruth {
    /// Whether anything at all sits at the name.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.kind != EntryKind::Absent
    }
}

#[cfg(unix)]
pub(crate) fn identity_and_links(metadata: &Metadata) -> (Option<FileIdentity>, Option<u64>) {
    use std::os::unix::fs::MetadataExt;
    let identity = FileIdentity {
        device: metadata.dev(),
        file: metadata.ino(),
    };
    (Some(identity), Some(metadata.nlink()))
}

#[cfg(not(unix))]
pub(crate) fn identity_and_links(_metadata: &Metadata) -> (Option<FileIdentity>, Option<u64>) {
    (None, None)
}

#[cfg(unix)]
pub(crate) fn owner_and_mode(metadata: &Metadata) -> (Option<(u32, u32)>, Option<u32>) {
    use std::os::unix::fs::MetadataExt;
    (
        Some((metadata.uid(), metadata.gid())),
        Some(metadata.mode() & 0o7777),
    )
}

#[cfg(not(unix))]
pub(crate) fn owner_and_mode(_metadata: &Metadata) -> (Option<(u32, u32)>, Option<u32>) {
    (None, None)
}

/// How one file differs between two looks of the stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observed {
    /// The file was not there at the last look and is there now.
    Appeared,
    /// The file was there at the last look and is gone.
    Vanished,
    /// The file's bytes differ. `replaced` says whether it is another file under
    /// the same name, where the platform can tell; `Some(false)` means the same
    /// file was edited in place.
    ContentChanged {
        /// Whether the file is a new one. `None` where the stand cannot tell.
        replaced: Option<bool>,
    },
    /// The bytes are the same and the file is another one. Seen only on Unix.
    ReplacedWithSameBytes,
}

/// One difference the stand saw between two looks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    /// The file's name below the root.
    pub name: String,
    /// What happened to it.
    pub what: Observed,
}

/// How many times the stand saw one file change since the last reset.
///
/// The stand has no watcher. It counts a difference between two looks as one
/// change, so two saves between two looks count once, and a save of the same
/// bytes is seen only where the file's identity is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteCount {
    /// Looks at which the file's bytes differed from the look before.
    pub content_changes: u64,
    /// Looks at which the file was another file under the same name. `None`
    /// everywhere but Unix, where the stand cannot tell a replace from an edit
    /// in place.
    pub replacements: Option<u64>,
    /// Looks at which the file was there and had not been.
    pub appearances: u64,
    /// Looks at which the file was gone and had been there.
    pub disappearances: u64,
}

impl Default for WriteCount {
    fn default() -> Self {
        Self {
            content_changes: 0,
            replacements: cfg!(unix).then_some(0),
            appearances: 0,
            disappearances: 0,
        }
    }
}

impl WriteCount {
    fn add(&mut self, what: Observed) {
        let mut replaced = || {
            if let Some(count) = self.replacements.as_mut() {
                *count += 1;
            }
        };
        match what {
            Observed::Appeared => self.appearances += 1,
            Observed::Vanished => self.disappearances += 1,
            Observed::ContentChanged {
                replaced: was_replaced,
            } => {
                if was_replaced == Some(true) {
                    replaced();
                }
                self.content_changes += 1;
            }
            Observed::ReplacedWithSameBytes => replaced(),
        }
    }
}

/// What the stand remembers of one regular file from its last look.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) sha256: String,
    pub(crate) identity: Option<FileIdentity>,
}

/// The stand's ledger: what it saw at its last look, and the counts so far.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    seen: BTreeMap<String, Seen>,
    counts: BTreeMap<String, WriteCount>,
}

impl Ledger {
    /// A ledger that starts from this look, with every count at zero.
    pub(crate) fn starting_from(seen: BTreeMap<String, Seen>) -> Self {
        Self {
            seen,
            counts: BTreeMap::new(),
        }
    }

    /// Takes a new look, counts every difference from the last, and returns them
    /// in order of name.
    pub(crate) fn look(&mut self, now: BTreeMap<String, Seen>) -> Vec<Observation> {
        let mut found = Vec::new();
        for (name, before) in &self.seen {
            let what = match now.get(name) {
                None => Observed::Vanished,
                Some(after) => {
                    let replaced = match (before.identity, after.identity) {
                        (Some(old), Some(new)) => Some(old != new),
                        _ => None,
                    };
                    if before.sha256 != after.sha256 {
                        Observed::ContentChanged { replaced }
                    } else if replaced == Some(true) {
                        Observed::ReplacedWithSameBytes
                    } else {
                        continue;
                    }
                }
            };
            found.push(Observation {
                name: name.clone(),
                what,
            });
        }
        for name in now.keys() {
            if !self.seen.contains_key(name) {
                found.push(Observation {
                    name: name.clone(),
                    what: Observed::Appeared,
                });
            }
        }
        found.sort_by(|a, b| a.name.cmp(&b.name));
        for observation in &found {
            self.counts
                .entry(observation.name.clone())
                .or_default()
                .add(observation.what);
        }
        self.seen = now;
        found
    }

    /// Takes a new look without counting anything: what the stand itself just did
    /// is not a write by whatever is under test.
    pub(crate) fn accept(&mut self, now: BTreeMap<String, Seen>) {
        self.seen = now;
    }

    pub(crate) fn count(&self, name: &str) -> WriteCount {
        self.counts.get(name).copied().unwrap_or_default()
    }

    pub(crate) fn counts(&self) -> BTreeMap<String, WriteCount> {
        self.counts.clone()
    }
}
