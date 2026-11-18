//! The failure codes this crate can produce. Each has exactly one class.
//!
//! `storage_full` is declared by the ledger and the file pack too, and the
//! technical reference shows this crate's row for it, the lowest layer's;
//! so its meaning and next step are worded for every crate that declares
//! it. The log writer's own failure says that it is the log folder.

a2m_core::failure_codes! {
    STORAGE_FULL = Busy, "storage_full",
        meaning "Something cannot be written, such as a log, a record of the ledger, or a file: the disk or the folder it goes to is full, or does not take the write.",
        next "Free space on the disk the message points to, or make its folder writable, then try again.",
        passes not_by_itself;
    TRACE_HOLDS_SECRET = Input, "trace_holds_secret",
        meaning "A finished trace still held a registered secret at the check on the way out. The trace is dropped and never reaches the store.",
        next "Register every secret with the vault before the run uses it, then run again.";
    TELEMETRY_SETTINGS_INVALID = Input, "telemetry_settings_invalid",
        meaning "The settings telemetry was started with cannot be used, such as a log file size of zero.",
        next "Set the log file size to at least one byte, then start the engine again.";
    LOG_UNREADABLE = Busy, "log_unreadable",
        meaning "A file of the engine's log is there and cannot be read.",
        next "Make the logs folder and its files readable by your user, then ask for the log again.",
        passes not_by_itself;
}
