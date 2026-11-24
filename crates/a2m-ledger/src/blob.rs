//! Evidence blobs: values, diffs, snapshots, traces. Sealed through the
//! vault before they are written, named by the workspace's keyed digest of
//! their plain content, and recorded in the store.

use std::fmt;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use a2m_core::{Digest, DigestPurpose, Failure, WorkspaceId};
use a2m_vault::{ItemId, Opened, OwnerKind, OwnerRef, SealContext, Sealed, SealedKind};

use crate::codec::parse;
use crate::failure;
use crate::store::Store;
use crate::tx::{Durability, SqlValue};

/// The name of a blob: the workspace's keyed digest of its plain content,
/// never its plain hash, so someone who can read the store cannot confirm a
/// guessed value by hashing it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobRef(Digest);

impl BlobRef {
    /// A reference read back from somewhere it was kept, such as the
    /// evidence list of a finding. It names a blob; it does not prove one
    /// exists. A row that takes a reference refuses one that names nothing.
    #[must_use]
    pub const fn from_digest(name: Digest) -> Self {
        Self(name)
    }

    /// The name, as a digest.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.0
    }
}

impl fmt::Display for BlobRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for BlobRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlobRef({})", self.0)
    }
}

impl FromStr for BlobRef {
    type Err = Failure;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(Self(text.parse::<Digest>()?))
    }
}

impl From<&BlobRef> for SqlValue {
    fn from(blob: &BlobRef) -> Self {
        Self::Text(blob.to_string())
    }
}

impl From<BlobRef> for SqlValue {
    fn from(blob: BlobRef) -> Self {
        Self::Text(blob.to_string())
    }
}

macro_rules! words {
    ($(#[$doc:meta])* $name:ident { $($(#[$value_doc:meta])* $value:ident => $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$value_doc])* $value),+
        }

        impl $name {
            /// Every value, in the order the documents list them.
            pub const ALL: &'static [Self] = &[$(Self::$value),+];

            /// The name the store keeps.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$value => $text),+
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = ::a2m_core::Failure;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|value| value.as_str() == text)
                    .ok_or_else(|| $crate::failure::invalid(concat!("an unknown ", stringify!($name))))
            }
        }
    };
}
pub(crate) use words;

words! {
    /// What a blob holds (ledger schema, `blobs`).
    BlobKind {
        /// Values read from or intended for a record.
        Values => "values",
        /// A diff.
        Diff => "diff",
        /// A snapshot of an interface.
        Snapshot => "snapshot",
        /// A finished trace.
        Trace => "trace",
        /// A screenshot.
        Screenshot => "screenshot",
        /// The inputs of a run, or a source file read once.
        Input => "input",
        /// The result of a run.
        Result => "result",
        /// A captured exchange with a system: a request and its answer.
        Exchange => "exchange",
        /// A captured tree of a window's controls.
        WindowTree => "window_tree",
    }
}

words! {
    /// How long evidence is kept (spec section 208).
    RetentionClass {
        /// Traces and screenshots.
        Short => "short",
        /// Diffs and the values seen before and after a write.
        Long => "long",
        /// The audit log's evidence.
        Longest => "longest",
    }
}

/// A blob to keep.
#[derive(Debug, Clone)]
pub struct NewBlob {
    /// The workspace it belongs to. Its name is made under this workspace's
    /// digest key.
    pub workspace: WorkspaceId,
    /// Whose it is. It is sealed for this owner.
    pub owner: OwnerRef,
    /// What it holds.
    pub kind: BlobKind,
    /// How long it is kept.
    pub class: RetentionClass,
    /// The plain content, already redacted by whoever made it.
    pub plain: Vec<u8>,
}

/// The store's evidence blobs.
pub struct Blobs<'s> {
    store: &'s Store,
}

impl Store {
    /// The evidence blobs.
    #[must_use]
    pub fn blobs(&self) -> Blobs<'_> {
        Blobs { store: self }
    }
}

fn file_of(folder: &Path, name: &str) -> PathBuf {
    // Two characters of the name make a folder, so no folder grows without bound.
    folder.join(name.get(..2).unwrap_or("00")).join(name)
}

/// Writes `bytes` to `path` and syncs them to disk before returning.
fn write_synced(path: &Path, bytes: &[u8], random: [u8; 10]) -> Result<(), Failure> {
    let cannot = |error: std::io::Error| {
        if error.kind() == std::io::ErrorKind::StorageFull {
            failure::full()
        } else {
            failure::unavailable("a blob file cannot be written")
        }
    };
    let folder = path
        .parent()
        .ok_or_else(|| failure::unavailable("a blob file has no folder"))?;
    fs::create_dir_all(folder).map_err(cannot)?;
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let partial = folder.join(format!("partial-{suffix}"));
    let mut file = fs::File::create(&partial).map_err(cannot)?;
    file.write_all(bytes).map_err(cannot)?;
    file.sync_all().map_err(cannot)?;
    drop(file);
    fs::rename(&partial, path).map_err(cannot)?;
    // The folder's own entry is synced where the system lets a folder be
    // opened; elsewhere the rename is as durable as the system makes it.
    #[cfg(unix)]
    fs::File::open(folder)
        .and_then(|folder| folder.sync_all())
        .map_err(cannot)?;
    Ok(())
}

impl Blobs<'_> {
    /// Keeps a blob and gives its name. The content is sealed for its owner,
    /// written and synced to disk, and only then recorded. The same content
    /// in the same workspace is kept once: a second `put` returns the same
    /// name and writes nothing.
    ///
    /// # Errors
    ///
    /// `busy`/`store_unavailable` while no vault is attached; what the vault
    /// returns when it cannot seal; `busy`/`storage_full`.
    pub async fn put(&self, new: NewBlob) -> Result<BlobRef, Failure> {
        self.put_as(new, Durability::Synced).await
    }

    /// [`Blobs::put`], with the commit of the blob's row pushed to disk as
    /// hard as `durability` says: `Durability::Relaxed` for the evidence of
    /// a read, whose run is kept without a disk sync of its own (spec
    /// section 208, Reads). The file itself is written and synced to disk
    /// before its row commits, either way.
    ///
    /// # Errors
    ///
    /// As [`Blobs::put`].
    pub async fn put_as(&self, new: NewBlob, durability: Durability) -> Result<BlobRef, Failure> {
        let shared = self.store.shared();
        let attached = shared.attached()?;
        let name = BlobRef(attached.digest.keyed_digest(
            &new.workspace,
            DigestPurpose::BlobName,
            &new.plain,
        ));
        let known = name.to_string();
        let exists = self
            .store
            .read(move |tx| {
                Ok(tx
                    .row("SELECT 1 FROM blobs WHERE hash = ?1", &[known.into()])?
                    .is_some())
            })
            .await?;
        if exists {
            return Ok(name);
        }

        let context = SealContext {
            workspace: new.workspace,
            owner: new.owner.clone(),
            kind: SealedKind::Blob,
            item: ItemId::new(name.to_string())?,
        };
        let sealed = attached.vault.seal(&context, &new.plain).await?;
        let key_ref = format!(
            "{}:{}",
            sealed.owner_key().id,
            sealed.owner_key().generation
        );
        let bytes = sealed.to_bytes();
        let size = i64::try_from(new.plain.len())
            .map_err(|_| failure::invalid("a blob too large to record"))?;
        let folder = shared.blob_dir.clone();

        self.store
            .write(durability, move |tx| {
                let text = name.to_string();
                // Two callers may keep the same content at once. The writer
                // is one thread, so the first to arrive here writes it.
                if tx
                    .row(
                        "SELECT 1 FROM blobs WHERE hash = ?1",
                        &[text.clone().into()],
                    )?
                    .is_some()
                {
                    return Ok(name);
                }
                let (location, inline) = match &folder {
                    Some(folder) => {
                        let random = tx.random()?;
                        write_synced(&file_of(folder, &text), &bytes, random)?;
                        ("file", None)
                    }
                    None => ("inline", Some(bytes)),
                };
                tx.exec(
                    "INSERT INTO blobs
                        (hash, workspace_id, owner_kind, owner_id, kind, size, location, sealed,
                         key_ref, retention_class, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    &[
                        text.into(),
                        new.workspace.to_string().into(),
                        new.owner.kind().as_str().into(),
                        new.owner.id().into(),
                        new.kind.as_str().into(),
                        size.into(),
                        location.into(),
                        inline.into(),
                        key_ref.into(),
                        new.class.as_str().into(),
                        tx.now().into(),
                    ],
                )?;
                Ok(name)
            })
            .await
    }

    /// Opens a blob and gives its plain content, in memory that is wiped
    /// when dropped. Whether the caller may see it is decided before this
    /// call, not here.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when no blob has this name; `busy`/`store_corrupt`
    /// when its row names a file that is missing; what the vault returns
    /// when the envelope does not open.
    pub async fn get(&self, blob: &BlobRef) -> Result<Opened, Failure> {
        let shared = self.store.shared();
        let attached = shared.attached()?;
        let name = blob.to_string();
        let folder = shared.blob_dir.clone();
        let item = name.clone();
        let (workspace, owner, sealed) = self
            .store
            .read(move |tx| {
                let row = tx
                    .row(
                        "SELECT workspace_id, owner_kind, owner_id, location, sealed
                         FROM blobs WHERE hash = ?1",
                        &[name.clone().into()],
                    )?
                    .ok_or_else(|| failure::not_found("blob"))?;
                let workspace: WorkspaceId = parse(row.text(0)?)?;
                let owner = match row.text(1)? {
                    kind if kind == OwnerKind::Person.as_str() => OwnerRef::person(row.text(2)?),
                    _ => OwnerRef::service(row.text(2)?),
                }
                .map_err(|_| failure::corrupt("a blob's owner"))?;
                let sealed = match (row.opt_bytes(4)?, &folder) {
                    (Some(bytes), _) => bytes.to_vec(),
                    (None, Some(folder)) => fs::read(file_of(folder, &name)).map_err(|_| {
                        failure::corrupt("a blob's row names a file that is missing")
                    })?,
                    (None, None) => {
                        return Err(failure::corrupt(
                            "a blob's row names a file and there is no folder",
                        ));
                    }
                };
                Ok((workspace, owner, sealed))
            })
            .await?;
        let context = SealContext {
            workspace,
            owner,
            kind: SealedKind::Blob,
            item: ItemId::new(item)?,
        };
        attached
            .vault
            .open(&context, &Sealed::from_bytes(&sealed)?)
            .await
    }

    /// Whether a blob with this name is recorded.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub async fn exists(&self, blob: &BlobRef) -> Result<bool, Failure> {
        let name = blob.to_string();
        self.store
            .read(move |tx| {
                Ok(tx
                    .row("SELECT 1 FROM blobs WHERE hash = ?1", &[name.into()])?
                    .is_some())
            })
            .await
    }
}
