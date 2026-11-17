//! The names the pack registers, and its failure and lint codes. Each is
//! written once, here.

use a2m_core::{Failure, FileKindName, FindingKind, Mode, StepKind, SurfaceKind, Verb};

/// The pack's own name, as its manifest states it.
pub(crate) const PACK: &str = "data";
/// The pack's version, as its manifest states it.
pub(crate) const PACK_VERSION: &str = "0.0.1";

/// The surface kind of a store in a file.
pub(crate) const DATABASE: SurfaceKind = SurfaceKind::fixed("database");
/// The surface kind of a store on a server.
#[cfg(feature = "postgres")]
pub(crate) const DATABASE_SERVER: SurfaceKind = SurfaceKind::fixed("database_server");

/// The surface kinds the pack serves in this build.
pub(crate) fn surface_kinds() -> Vec<SurfaceKind> {
    vec![
        DATABASE,
        #[cfg(feature = "postgres")]
        DATABASE_SERVER,
    ]
}

/// Whether a surface kind is one the pack serves in this build.
pub(crate) fn is_database(kind: &SurfaceKind) -> bool {
    surface_kinds().contains(kind)
}
/// The mode the pack drives, as a target file writes it.
pub(crate) const PACK_MODE: &str = "sql";
/// The file kind the pack owns.
pub(crate) const STATEMENT: FileKindName = FileKindName::fixed("statement");
/// The folder of the file kind.
pub(crate) const FOLDER: &str = "statements";

/// Opens a transaction with a declared access mode.
pub(crate) const BEGIN: StepKind = StepKind::fixed("begin");
/// Runs one prepared statement with bound parameters.
pub(crate) const EXECUTE: StepKind = StepKind::fixed("execute");
/// Turns the rows a statement returned into typed values.
pub(crate) const FETCH: StepKind = StepKind::fixed("fetch");
/// Holds the count of rows a write changed to the declared one.
pub(crate) const EXPECT_ROWS: StepKind = StepKind::fixed("expect_rows");
/// Commits the open transaction: the commit point.
pub(crate) const COMMIT: StepKind = StepKind::fixed("commit");
/// Reads the store's own catalog; sent only by the discoverer.
pub(crate) const CATALOG: StepKind = StepKind::fixed("catalog");

pub(crate) const READ: Verb = Verb::fixed("read");
pub(crate) const LIST: Verb = Verb::fixed("list");
pub(crate) const SEARCH: Verb = Verb::fixed("search");
pub(crate) const CREATE: Verb = Verb::fixed("create");
pub(crate) const UPDATE: Verb = Verb::fixed("update");
pub(crate) const DELETE: Verb = Verb::fixed("delete");

/// The names that cannot be made in a `const`: made once when the pack
/// registers, and handed to each object that needs them.
#[derive(Debug, Clone)]
pub(crate) struct Names {
    /// The mode `sql`.
    pub mode: Mode,
    /// The finding kind `table`.
    pub table: FindingKind,
}

impl Names {
    pub(crate) fn new() -> Result<Self, Failure> {
        Ok(Self {
            mode: Mode::new(PACK_MODE)?,
            table: FindingKind::new("table")?,
        })
    }
}

macro_rules! codes {
    ($( $(#[$doc:meta])* $name:ident = $class:ident, $text:literal; )+) => {
        $( $(#[$doc])* pub const $name: FailureCode = FailureCode::fixed($text); )+

        /// Every failure code of this pack with its one class, as the
        /// manifest declares them.
        pub const ALL: &[(FailureClass, &str)] = &[ $( (FailureClass::$class, $text) ),+ ];
    };
}

/// The failure codes the pack's driver returns. Each belongs to one class.
pub mod codes {
    use a2m_core::{FailureClass, FailureCode};

    codes! {
        /// The store's file cannot be opened.
        UPSTREAM_UNAVAILABLE = Transient, "upstream_unavailable";
        /// Another connection held the store's lock for the whole time limit.
        STORE_LOCKED = Busy, "store_locked";
        /// A unique, check, not-null, or foreign-key constraint refused a write.
        CONSTRAINT_VIOLATED = Rejected, "constraint_violated";
        /// The store refused a write it holds no right to make.
        DENIED_BY_STORE = Policy, "denied_by_store";
        /// A statement declared a read tried to change data.
        READ_STATEMENT_WROTE = Policy, "read_statement_wrote";
        /// A statement is not one plain read or one keyed write: it attaches
        /// a file, changes a setting or the schema, ends a transaction, is
        /// several statements, or is a read that returns no column.
        STATEMENT_NOT_ALLOWED = Policy, "statement_not_allowed";
        /// A table, column, parameter, or trigger differs from the plan.
        SCHEMA_CHANGED = Drift, "schema_changed";
        /// A key matched more than one row.
        KEY_NOT_UNIQUE = Drift, "key_not_unique";
        /// A read of one record by its key gave a record with another key,
        /// or does not return the key.
        READ_NOT_BY_KEY = Drift, "read_not_by_key";
        /// A write changed no row: the record is gone or its key changed.
        CHANGED_SINCE_PREVIEW = Stale, "changed_since_preview";
        /// A write changed no row: the version guard did not hold.
        VERSION_CONFLICT = Stale, "version_conflict";
        /// The store answered with an error no rule covers.
        UNEXPECTED_STATUS = UnknownVariant, "unexpected_status";
        /// A stored value is a float or bytes, which no rule reads.
        VALUE_KIND_UNSUPPORTED = UnknownVariant, "value_kind_unsupported";
        /// A statement's result holds more text than the limit allows.
        RESULT_TOO_LARGE = UnknownVariant, "result_too_large";
        /// The store's catalog holds more tables than one read takes, so
        /// what a write reaches cannot be shown.
        CATALOG_TOO_LARGE = UnknownVariant, "catalog_too_large";
        /// An argument is missing, or is a list or a record.
        VALUE_NOT_BINDABLE = Input, "value_not_bindable";
        /// The embedded store's file was replaced by another file at its
        /// name after the run was prepared.
        STORE_FILE_REPLACED = Drift, "store_file_replaced";
    }

    /// The store's connection ended before `commit` was sent. The provider
    /// contract's own code.
    pub const CONNECTION_LOST: FailureCode = FailureCode::fixed("connection_lost");
    /// A statement ran past its time limit. The provider contract's own code.
    pub const TIMEOUT: FailureCode = FailureCode::fixed("timeout");
    /// A write was sent and nothing says whether it happened. The provider
    /// contract's own code.
    pub const NO_CONFIRMATION: FailureCode = FailureCode::fixed("no_confirmation");
    /// A write was asked for where nothing may be written. The provider
    /// contract's own code.
    pub const READ_ONLY_POSTURE: FailureCode = FailureCode::fixed("read_only_posture");
    /// The run was given no checked path for the store's file. The provider
    /// contract's own code.
    pub const PATH_NOT_ALLOWED: FailureCode = FailureCode::fixed("path_not_allowed");
    /// The plan's section does not decode. The core's own code.
    pub const PAYLOAD_UNREADABLE: FailureCode = FailureCode::PAYLOAD_UNREADABLE;

    /// The class of [`CONNECTION_LOST`] and [`TIMEOUT`].
    pub const TRANSIENT: FailureClass = FailureClass::Transient;

    /// The server store is not on this machine, and this build makes no
    /// encrypted link to one: no unencrypted link is ever made.
    #[cfg(feature = "postgres")]
    pub const LINK_NOT_ENCRYPTED: FailureCode = FailureCode::fixed("link_not_encrypted");
    /// The identity the lease signs in as is a superuser, bypasses row
    /// security, may replicate or make roles, or owns tables of the schema.
    #[cfg(feature = "postgres")]
    pub const IDENTITY_IS_ADMINISTRATOR: FailureCode =
        FailureCode::fixed("identity_is_administrator");
    /// The session's credential is not a user and a password, which is all a
    /// server store is signed in with.
    #[cfg(feature = "postgres")]
    pub const CREDENTIAL_NOT_CARRIED: FailureCode = FailureCode::fixed("credential_not_carried");
    /// A connection, or the server it reaches, does not lead where the plan's
    /// surface names: another address, port, database, schema, `search_path`,
    /// link mode, or role. It is closed and never used, and nothing is sent.
    #[cfg(feature = "postgres")]
    pub const CONNECTION_LEADS_ELSEWHERE: FailureCode =
        FailureCode::fixed("connection_leads_elsewhere");
    /// The server store's address is not among the run's destinations.
    #[cfg(feature = "postgres")]
    pub const SURFACE_NOT_DECLARED: FailureCode = FailureCode::fixed("surface_not_declared");

    /// The codes only a build with the server store returns, with their class.
    #[cfg(feature = "postgres")]
    pub const SERVER: &[(FailureClass, &str)] = &[
        (FailureClass::Policy, "link_not_encrypted"),
        (FailureClass::Policy, "identity_is_administrator"),
        (FailureClass::Input, "credential_not_carried"),
        (FailureClass::Policy, "connection_leads_elsewhere"),
        (FailureClass::Policy, "surface_not_declared"),
    ];
}

/// The lint codes the pack reports.
pub(crate) mod lint {
    use a2m_core::LintCode;

    macro_rules! lint_codes {
        ($( $name:ident = $rule:literal; )+) => {
            $( pub(crate) const $name: LintCode = LintCode::fixed(concat!("data.", $rule)); )+
        };
    }

    lint_codes! {
        // Registered rules.
        HAZARD_NOT_DECLARED = "hazard_not_declared";
        STATEMENT_UNUSED = "statement_unused";
        // Reported while a statements file is compiled.
        UNKNOWN_RECORD = "unknown_record";
        UNKNOWN_FIELD = "unknown_field";
        STATEMENT_INVALID = "statement_invalid";
        SURFACE_INVALID = "surface_invalid";
        STORE_NOT_BUILT = "store_not_built";
        LIMIT_INVALID = "limit_invalid";
        // Reported while a binding is lowered.
        UNKNOWN_STATEMENT = "unknown_statement";
        PARAM_UNBOUND = "param_unbound";
        VERB_MISMATCH = "verb_mismatch";
        WRITE_NEEDS_STANDS_ALONE = "write_needs_stands_alone";
        READ_BACK_INVALID = "read_back_invalid";
    }
}
