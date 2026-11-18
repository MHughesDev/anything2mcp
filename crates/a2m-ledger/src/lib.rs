//! The store: runs, the ledger of commit points with its nine rules and
//! record locks, run events, evidence blobs, findings and open questions,
//! published versions, the audit chain, watches, and the vault's wrapped
//! keys.
//! Everything in the kernel that persists goes through this crate.
//!
//! The store is SQLite. The database crate blocks, so every database call
//! runs on a thread of the store's own: a caller hands a closure to
//! [`Store::write`] or [`Store::read`], the closure is given a [`Tx`], and
//! everything it does commits together or not at all.
//!
//! A crate above this one owns the meaning of its own tables. Its migrations
//! are files in this crate's `migrations/` folder, named for it; its
//! statements run through [`Tx::tables`], which refuses a statement that
//! names a table of another crate (decision 0080).
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 and 2.

mod audit;
mod blob;
mod codec;
mod failure;
mod finding;
mod keys;
mod ledger;
mod listing;
mod migrate;
mod shadow;
mod sqlite;
mod store;
#[cfg(feature = "testing")]
pub mod testing;
mod tx;
mod version;
mod watch;
mod watch_compare;

pub use audit::{Audit, AuditEntry, AuditRow, AuditSeq, ChainCheck};
pub use blob::{BlobKind, BlobRef, Blobs, NewBlob, RetentionClass};
pub use finding::{
    FINDING_FORMAT, Findings, NewUnknown, Settlement, UnknownId, UnknownRow, UnknownStatus,
};
pub use keys::StoreKeyRecords;
pub use ledger::{
    AcknowledgedInDiff, Acted, ActedToken, EventKind, Fence, IntentToken, Ledger, LockHeld,
    LockRow, NewRun, Opened, PlannedRecord, ReadBack, Reconciled, RecordIntent, RecordRef,
    RecordRow, RecordTally, Replayed, RunEvent, RunRow, Settled, ToolKind, WriteClearance, replay,
};
pub use listing::{
    CALLER_KEY_LONGEST, CallerKey, CallerKeyRow, IDEMPOTENCY_KEY_INVALID, RUN_PAGE_MOST, RunQuery,
};
pub use migrate::{Migration, migrations, table_owner};
pub use shadow::{
    Difference, DifferingOutput, LastMismatch, NotPromotable, Promotable, PromotionRow,
    ShadowComparison, ShadowOutcome, ShadowPromotion, ShadowStanding, ShadowSubject,
    ShadowThreshold, Shadows,
};
pub use store::{
    BLOB_FOLDER_NAME, COMMIT_SPAN, DATABASE_FILE_NAME, Store, StoreConfig, StoreHealth,
};
pub use tx::{Durability, Owner, PolicyKey, Row, SqlValue, Tables, Tx};
pub use version::{PublishedVersion, VersionRow, Versions};
pub use watch::{
    ChangeCounts, ChangeKind, ChangeRow, CheckEnded, CheckOutcome, CheckRow, Due, Missed, NewWatch,
    Observed, ObservedRecord, WatchId, WatchReason, WatchRow, WatchState, Watches,
};
pub use watch_compare::{
    Baseline, ChangedRecord, CheckDigests, CheckFault, Comparison, Held, MissingRecord,
    RecordDigests, VALUE_DOMAIN, WatchKey, compare, value_purpose,
};

/// The failure codes this crate produces. Each belongs to exactly one class.
pub mod codes {
    pub use crate::failure::{
        APPROVAL_MISSING, APPROVAL_STALE, CHANGED_SINCE_PREVIEW, LEDGER_STATE_UNEXPECTED,
        NOT_FOUND, READ_BACK_DIFFERS, RECONCILE_REQUIRED, RECORD_LOCKED, RUNNER_LOST, STORAGE_FULL,
        STORE_CORRUPT, STORE_UNAVAILABLE, STORE_VERSION_UNSUPPORTED, TABLE_NOT_OWNED,
    };
    pub use crate::listing::IDEMPOTENCY_KEY_INVALID;
    pub use crate::shadow::{SHADOW_NOT_COUNTED, SHADOW_NOT_PROMOTABLE, SHADOW_WRITE_NOT_BUILT};
    pub use crate::watch::{
        ALL as WATCH_CODES, WATCH_CHECK_NOT_COMPLETED, WATCH_CHECK_RUNNING, WATCH_CHECK_UNFINISHED,
        WATCH_DUE_MISSED, WATCH_ENDED, WATCH_NEEDS_CONFIRMATION, WATCH_NOT_A_READ,
        WATCH_NOT_CONFIRMED, WATCH_NOT_YOURS, WATCH_PAUSED, WATCH_RUN_NOT_ITS_OWN,
    };
    pub use crate::watch_compare::{WATCH_KEY_REPEATED, WATCH_SHAPES_MIXED};
}
