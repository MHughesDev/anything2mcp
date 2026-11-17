//! The failure codes this crate produces. Each belongs to exactly one class.
//!
//! A message is built from names the engine knows, never from what a peer
//! sent.

use a2m_core::{Failure, FailureClass, FailureCode};

/// An engine already holds the engine lock. Class `busy`.
pub const ENGINE_RUNNING: FailureCode = FailureCode::fixed("engine_running");
/// Nothing answers on the local channel. Class `busy`.
pub const ENGINE_UNREACHABLE: FailureCode = FailureCode::fixed("engine_unreachable");
/// The channel, a lock file, or the loopback listener cannot be opened on
/// this machine. Class `busy`.
pub const CHANNEL_UNAVAILABLE: FailureCode = FailureCode::fixed("channel_unavailable");
/// The connection to the engine ended before a request was sent, so the
/// engine never had it. Class `transient`.
pub const CONNECTION_LOST: FailureCode = FailureCode::fixed("connection_lost");
/// The engine may have had a request, and its answer was lost: what the
/// request asked may have been done, a write among it. Class
/// `possible_commit`. The same code, class, and meaning as the stdio
/// proxy's.
pub const ANSWER_LOST: FailureCode = FailureCode::fixed("answer_lost");
/// A connection from another user, or an engine owned by another user.
/// Class `policy`.
pub const PEER_NOT_ALLOWED: FailureCode = FailureCode::fixed("peer_not_allowed");
/// The person dismissed or failed the presence prompt, or this machine has
/// none. Class `policy`.
pub const PRESENCE_FAILED: FailureCode = FailureCode::fixed("presence_failed");
/// A presence prompt is already up on this machine, and one is raised at
/// a time. Class `busy`.
pub const PROMPT_BUSY: FailureCode = FailureCode::fixed("prompt_busy");
/// A connection's first request was not a well-formed hello. Class `input`.
pub const HELLO_INVALID: FailureCode = FailureCode::fixed("hello_invalid");
/// The client and the engine speak different versions of the local API.
/// Class `input`.
pub const LOCAL_API_MISMATCH: FailureCode = FailureCode::fixed("local_api_mismatch");
/// The person surface cannot show a page: it holds no approval desk. Class
/// `busy`.
pub const SURFACE_UNAVAILABLE: FailureCode = FailureCode::fixed("surface_unavailable");

pub(crate) fn busy(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Busy, code, message, next)
}

pub(crate) fn policy(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Policy, code, message, next)
}

pub(crate) fn input(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Input, code, message, next)
}

/// The connection ended before the request was sent: nothing reached the
/// engine, so the request may be made again.
pub(crate) fn lost() -> Failure {
    Failure::new(
        FailureClass::Transient,
        CONNECTION_LOST,
        "The connection to the engine ended before the request was sent, so the engine did not have it.",
        "Run the command again once an engine answers (`anything2mcp serve` starts one).",
    )
}

/// The request was handed to the engine, and no whole answer came back.
/// The channel cannot know what the request was, so it never says that
/// sending it again is safe: a write sent twice may be written twice.
pub(crate) fn answer_lost() -> Failure {
    Failure::new(
        FailureClass::PossibleCommit,
        ANSWER_LOST,
        "The engine may have had the request, and its answer was lost: what it asked may have been done, and a write may have started a run.",
        "If the request carried an idempotency key, send the same request again with the same key and nothing changed: it gives back the run the first one started, finished or not, and starts no other. If it carried none, do not send it again, because a second request may start a second run and write twice: find its run first with the run list (`anything2mcp run list --since <ms>`, with `--tool <tool>` for a tool call), from one minute before the request was sent, and do what that run's `next` says.",
    )
}

/// A problem of this machine: a directory, a lock file, or a listener that
/// cannot be opened. Only the kind of the error is told, never its text.
pub(crate) fn unavailable(what: &str, error: &std::io::Error) -> Failure {
    busy(
        CHANNEL_UNAVAILABLE,
        format!(
            "{what} cannot be opened on this machine ({}).",
            error.kind()
        ),
        "Check that the engine's runtime and data directories exist and belong to you, then try again.",
    )
}

#[cfg(test)]
mod tests {
    use a2m_core::FailureClass;

    use super::{ANSWER_LOST, answer_lost};

    #[test]
    fn a_lost_answer_says_both_ways_to_go_on_and_never_that_sending_again_is_safe() {
        let failure = answer_lost();
        assert_eq!(failure.class(), FailureClass::PossibleCommit);
        assert_eq!(failure.code(), &ANSWER_LOST);
        let next = failure.next_step();
        assert!(
            next.contains(
                "carried an idempotency key, send the same request again with the same key"
            ),
            "{next}"
        );
        assert!(
            next.contains("carried none, do not send it again"),
            "{next}"
        );
        assert!(
            next.contains("`anything2mcp run list --since <ms>`"),
            "{next}"
        );
        assert!(
            next.contains("one minute before the request was sent"),
            "{next}"
        );
        assert!(next.contains("what that run's `next` says"), "{next}");
    }
}
