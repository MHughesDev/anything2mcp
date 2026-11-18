//! The failure codes this crate produces, each with its one class. The
//! readers' error, [`crate::NetError`], names them as its associated
//! constants.

a2m_core::failure_codes! {
    NET_FACT_REFUSED = EdgeBlocked, "net_fact_refused",
        meaning "The system refused this fact to this user.",
        next "Read the fact in the system's own network settings, as a person who may see it.";
    NET_READER_NOT_BUILT = UnknownVariant, "net_reader_not_built",
        meaning "No reader of this fact is built for this operating system.",
        next "Read the fact in the system's own network settings; this reader is not built for this system yet.";
    NET_ANSWER_UNREADABLE = UnknownVariant, "net_answer_unreadable",
        meaning "The system's answer has a shape this reader does not know, so it stops rather than guess.",
        next "Report the operating system and its version.";
    NET_READ_FAILED = Transient, "net_read_failed",
        meaning "The system's call to read the fact failed.",
        next "Try again; if it fails again, report the operating system and the error number.";
}
