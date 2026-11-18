//! The failure codes this crate produces. Each belongs to exactly one class.
//!
//! A message is built from names the engine knows, never from what a peer
//! sent.

use crate::connector::Seen;
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
/// The data directory is no longer the one the engine opened: its engine
/// lock is gone, or another file stands in its place. The engine refuses
/// the request and stops; the next client starts a new engine. Class
/// `busy`.
pub const DATA_DIRECTORY_GONE: FailureCode = FailureCode::fixed("data_directory_gone");
/// The engine's socket would have a path longer than the system takes for
/// a socket's address, so the channel can be neither made nor reached
/// there. Class `input`.
pub const SOCKET_PATH_TOO_LONG: FailureCode = FailureCode::fixed("socket_path_too_long");
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
    lost_seeing(Seen::NoDetail)
}

/// As [`lost`], saying what the client saw, in a word of the closed list.
pub(crate) fn lost_seeing(seen: Seen) -> Failure {
    Failure::new(
        FailureClass::Transient,
        CONNECTION_LOST,
        format!(
            "The connection to the engine ended before the request was sent, so the engine did not have it. What the client saw: {}.",
            seen.words()
        ),
        "Run the command again once an engine answers (`anything2mcp serve` starts one).",
    )
}

/// The request was handed to the engine, and no whole answer came back.
/// The channel cannot know what the request was, so it never says that
/// sending it again is safe: a write sent twice may be written twice.
pub(crate) fn answer_lost(seen: Seen) -> Failure {
    Failure::new(
        FailureClass::PossibleCommit,
        ANSWER_LOST,
        format!(
            "The engine may have had the request, and its answer was lost: what it asked may have been done, and a write may have started a run. What the client saw: {}.",
            seen.words()
        ),
        "If the request carried an idempotency key, send the same request again with the same key and nothing changed: it gives back the run the first one started, finished or not, and starts no other. If it carried none, do not send it again, because a second request may start a second run and write twice: find its run first with the run list (`anything2mcp run list --since <ms>`, with `--tool <tool>` for a tool call), from one minute before the request was sent, and do what that run's `next` says.",
    )
}

/// A problem of this machine: a directory, a lock file, or a listener that
/// cannot be opened. The error's own text is never told, because it may
/// hold a path. What is told is the system's error by its number and the
/// system's fixed words for that number, which name no path, so that a
/// person can look it up; for an error that has no number, its kind.
pub(crate) fn unavailable(what: &str, error: &std::io::Error) -> Failure {
    busy(
        CHANNEL_UNAVAILABLE,
        format!(
            "{what} cannot be opened on this machine ({}).",
            error_words(error)
        ),
        "Check that the engine's runtime and data directories exist and belong to you, then try again.",
    )
}

/// The words for an error of the operating system, as [`unavailable`] tells
/// them: `system error 231: All pipe instances are busy.`, or the kind.
fn error_words(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        // Made again from the number alone, so that nothing the error
        // carried besides it is told.
        Some(number) => {
            let said = std::io::Error::from_raw_os_error(number).to_string();
            let words = said
                .strip_suffix(&format!(" (os error {number})"))
                .unwrap_or(&said)
                .trim();
            if words.is_empty() {
                format!("system error {number}")
            } else {
                format!("system error {number}: {words}")
            }
        }
        None => error.kind().to_string(),
    }
}

/// The socket at `socket` is `length` bytes long and the system takes no
/// more than `longest`: the engine cannot make its channel there, and no
/// client can reach one.
pub(crate) fn socket_path_too_long(
    socket: &std::path::Path,
    length: usize,
    longest: usize,
) -> Failure {
    // The socket is `run/engine.sock` inside the folder `A2M_HOME` names.
    let inside = "/run/engine.sock".len();
    input(
        SOCKET_PATH_TOO_LONG,
        format!(
            "The engine's socket would be `{}`, which is {length} bytes long, and this system takes the path of a socket of no more than {longest} bytes, so the local channel cannot be made there.",
            crate::connector::shown(socket)
        ),
        format!(
            "Set `{}` to a folder whose full path is at most {} bytes long, so that the engine's folders, and its socket, `run/engine.sock` inside it, lie there; then run the command again.",
            a2m_actions::HOME_VARIABLE,
            longest.saturating_sub(inside)
        ),
    )
}

/// What a request is told by an engine whose data directory is gone. The
/// request was not handed to the kernel, so nothing of it was done.
#[must_use]
pub fn data_directory_gone() -> Failure {
    busy(
        DATA_DIRECTORY_GONE,
        "The engine's data directory was removed or replaced while it ran, so the engine took no request and is stopping. Nothing of this request was done.",
        "Run the command again: a new engine starts on the data directory as it is now. Runs and published targets that were in the removed directory are not in a new one.",
    )
}

#[cfg(test)]
mod tests {
    use a2m_core::FailureClass;

    use super::{ANSWER_LOST, CHANNEL_UNAVAILABLE, answer_lost, unavailable};

    #[test]
    fn a_refusal_of_the_system_is_told_by_its_number_and_never_as_uncategorized() {
        // 231 is a busy pipe on Windows, and a number with no kind of its
        // own on every system: the case that was told as "uncategorized".
        for number in [231, 2, 13] {
            let failure = unavailable(
                "The local channel",
                &std::io::Error::from_raw_os_error(number),
            );
            assert_eq!(failure.code(), &CHANNEL_UNAVAILABLE);
            let message = failure.message();
            assert!(
                message.starts_with(&format!(
                    "The local channel cannot be opened on this machine (system error {number}"
                )),
                "{message}"
            );
            assert!(message.ends_with(")."), "{message}");
            assert!(!message.contains("uncategorized"), "{message}");
            assert!(!message.contains("os error"), "{message}");
        }
        // Nothing an error carries besides its number is told: a path in
        // it stays out.
        let carried = std::io::Error::other("C:\\Users\\pat\\secret");
        let failure = unavailable("The start lock's file", &carried);
        assert!(!failure.message().contains("pat"), "{}", failure.message());
        assert!(
            failure.message().contains("(other error)"),
            "{}",
            failure.message()
        );
    }

    #[test]
    fn a_lost_answer_says_both_ways_to_go_on_and_never_that_sending_again_is_safe() {
        let failure = answer_lost(crate::connector::Seen::NoDetail);
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
