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

/// The failure codes the pack's driver returns. Each belongs to one class.
///
/// Whether a `busy` code passes by itself, and why: `store_locked` does,
/// because the other connection that holds the store's lock lets it go when
/// its work is done.
///
/// The codes only a build with the server store returns are in a table of
/// their own, [`server`], which exists only in that build, so that `ALL`
/// stays what the embedded store's manifest declares.
pub mod codes {
    use a2m_core::{FailureClass, FailureCode};

    a2m_core::failure_codes! {
        UPSTREAM_UNAVAILABLE = Transient, "upstream_unavailable",
            meaning "The store's file cannot be opened.",
            next "Check that the store's file exists and can be read, then try again.";
        STORE_LOCKED = Busy, "store_locked",
            meaning "Another connection held the store's lock for the whole time limit.",
            next "Try again when the other program has finished its work.",
            passes by_itself;
        CONSTRAINT_VIOLATED = Rejected, "constraint_violated",
            meaning "A unique, check, not-null, or foreign-key constraint refused a write.",
            next "Correct the values so that they satisfy the store's constraints; nothing was written.";
        DENIED_BY_STORE = Policy, "denied_by_store",
            meaning "The store refused a write it holds no right to make.",
            next "Ask the store's owner for the right to write, or use a route that has it.";
        READ_STATEMENT_WROTE = Policy, "read_statement_wrote",
            meaning "A statement declared a read tried to change data.",
            next "Correct the statement or declare it `effect: write`; do not use the binding as a read.";
        STATEMENT_NOT_ALLOWED = Policy, "statement_not_allowed",
            meaning "A statement is not one plain read or one keyed write: it attaches a file, changes a setting or the schema, ends a transaction, is several statements, or is a read that returns no column.",
            next "Correct the statement in the statements file: a read is one `SELECT`, and a write is written by the pack from its record.";
        SCHEMA_CHANGED = Drift, "schema_changed",
            meaning "A table, column, parameter, or trigger differs from the plan.",
            next "Run discovery again, compare the store's catalog with the statements file, and publish a corrected version.";
        KEY_NOT_UNIQUE = Drift, "key_not_unique",
            meaning "A key matched more than one row.",
            next "Correct the record's key in the statements file; nothing was written.";
        READ_NOT_BY_KEY = Drift, "read_not_by_key",
            meaning "A read of one record by its key gave a record with another key, or does not return the key.",
            next "Compare the record's key and columns in the statements file with the store's catalog, and publish a corrected version.";
        CHANGED_SINCE_PREVIEW = Stale, "changed_since_preview",
            meaning "A write changed no row: the record is gone or its key changed.",
            next "Preview the run again and approve the new diff.";
        VERSION_CONFLICT = Stale, "version_conflict",
            meaning "A write changed no row: the version guard did not hold.",
            next "Preview the run again, so that the diff holds the record's current version, and approve it.";
        UNEXPECTED_STATUS = UnknownVariant, "unexpected_status",
            meaning "The store answered with an error no rule covers.",
            next "Look at the store's own log, then report the code to the pack's author.";
        VALUE_KIND_UNSUPPORTED = UnknownVariant, "value_kind_unsupported",
            meaning "A stored value is a float or bytes, which no rule reads.",
            next "Read the column through a statement that returns it as exact text, or leave the field out.";
        RESULT_TOO_LARGE = UnknownVariant, "result_too_large",
            meaning "A statement's result holds more text than the limit allows.",
            next "Lower the statement's `max_rows`, or raise the pack's `result_bytes` setting.";
        CATALOG_TOO_LARGE = UnknownVariant, "catalog_too_large",
            meaning "The store's catalog holds more tables than one read takes, so what a write reaches cannot be shown.",
            next "Declare `hazard: data_elsewhere` on the statement and on its operation, so that a person agrees to each write; or keep the tables in a smaller store.";
        VALUE_NOT_BINDABLE = Input, "value_not_bindable",
            meaning "An argument is missing, or is a list or a record.",
            next "Pass a text, a whole number, a decimal, a date, or a truth value for the parameter.";
        STORE_FILE_REPLACED = Drift, "store_file_replaced",
            meaning "The embedded store's file was replaced by another file at its name after the run was prepared.",
            next "Check what replaced the file, then start the run again.";
    }

    /// The store's connection ended before `commit` was sent. The provider
    /// contract's own code.
    pub const CONNECTION_LOST: FailureCode = a2m_provider::codes::CONNECTION_LOST;
    /// A statement ran past its time limit. The provider contract's own code.
    pub const TIMEOUT: FailureCode = a2m_provider::codes::TIMEOUT;
    /// A write was sent and nothing says whether it happened. The provider
    /// contract's own code.
    pub const NO_CONFIRMATION: FailureCode = a2m_provider::codes::NO_CONFIRMATION;
    /// A write was asked for where nothing may be written. The provider
    /// contract's own code.
    pub const READ_ONLY_POSTURE: FailureCode = a2m_provider::codes::READ_ONLY_POSTURE;
    /// The run was given no checked path for the store's file. The provider
    /// contract's own code.
    pub const PATH_NOT_ALLOWED: FailureCode = a2m_provider::codes::PATH_NOT_ALLOWED;
    /// The plan's section does not decode. The core's own code.
    pub const PAYLOAD_UNREADABLE: FailureCode = FailureCode::PAYLOAD_UNREADABLE;

    /// The class of [`CONNECTION_LOST`] and [`TIMEOUT`].
    pub const TRANSIENT: FailureClass = FailureClass::Transient;

    /// The codes only a build with the server store returns, in a table of
    /// their own.
    #[cfg(feature = "postgres")]
    pub mod server {
        a2m_core::failure_codes! {
            LINK_NOT_ENCRYPTED = Policy, "link_not_encrypted",
                meaning "The server store is not on this machine, and this build makes no encrypted link to one: no unencrypted link is ever made.",
                next "Reach the store at `127.0.0.1` or `::1`, written as an address and not a name.";
            IDENTITY_IS_ADMINISTRATOR = Policy, "identity_is_administrator",
                meaning "The identity the lease signs in as is a superuser, bypasses row security, may replicate or make roles, or owns tables of the schema.",
                next "Sign in as a role that owns nothing and holds only the rights to read and write the rows the target names.";
            CREDENTIAL_NOT_CARRIED = Input, "credential_not_carried",
                meaning "The session's credential is not a user and a password, which is all a server store is signed in with.",
                next "Sign in to the target with a provider that gives a user and a password.";
            CONNECTION_LEADS_ELSEWHERE = Policy, "connection_leads_elsewhere",
                meaning "A connection, or the server it reaches, does not lead where the plan's surface names: another address, port, database, schema, `search_path`, link mode, or role. It is closed and never used, and nothing is sent.",
                next "Check the surface's address, port, database, and schema, and what else may change a connection's settings; then start the run again.";
            SURFACE_NOT_DECLARED = Policy, "surface_not_declared",
                meaning "An address outside the target's declared surfaces was asked for.",
                next "Declare the address as a surface of the target in the run's environment, then try again.";
        }
    }

    #[cfg(feature = "postgres")]
    pub use server::{
        CONNECTION_LEADS_ELSEWHERE, CREDENTIAL_NOT_CARRIED, IDENTITY_IS_ADMINISTRATOR,
        LINK_NOT_ENCRYPTED, SURFACE_NOT_DECLARED,
    };

    /// The codes only a build with the server store returns, with their class.
    #[cfg(feature = "postgres")]
    pub const SERVER: &[(FailureClass, &str)] = server::ALL;
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
