//! The failure codes this crate can produce. Each has exactly one class.

a2m_core::failure_codes! {
    STORAGE_FULL = Busy, "storage_full",
        meaning "The log folder cannot be written. The engine keeps running and its logs go to standard error, redacted.",
        next "Free space in the data directory or make its logs folder writable, then restart the engine.",
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
