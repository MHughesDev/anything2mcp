//! The failure codes this crate produces, each with its one class, in one
//! table (decision 0258).
//!
//! A message is built from names the engine knows, never from what a peer
//! sent. `answer_lost` is the same code, with the same class and meaning,
//! as the stdio proxy's in `a2m-mcp`; `connection_lost` is the same code and
//! class as the provider contract's.
//!
//! Whether a `busy` code passes by itself, and why:
//!
//! - `engine_running` does not: the engine that holds the lock goes on
//!   running until someone stops it, so a second start is refused again.
//! - `engine_unreachable` does not: most often no engine runs, or the one
//!   that holds the channel is stuck, and a person starts it or reads its
//!   log.
//! - `channel_unavailable` does not: a directory, a lock file, or the
//!   listener that cannot be opened stays so until a person repairs it.
//! - `prompt_busy` does: the prompt or page that is up closes when its
//!   person answers it or when its window ends, with nobody else acting.
//! - `data_directory_gone` does not: the engine is stopping, and what a
//!   new engine finds in the directory, without the runs and published
//!   targets that were in the removed one, is for a person to look at.
//! - `surface_unavailable` does not: the engine cannot record approvals
//!   until what `anything2mcp doctor run` names is repaired.

use crate::connector::Seen;
use a2m_core::{CodeEntry, Failure};

a2m_core::failure_codes! {
    ENGINE_RUNNING = Busy, "engine_running",
        meaning "An engine already holds the engine lock.",
        next "Use the running engine, or stop it with `anything2mcp engine stop` first.",
        passes not_by_itself;
    ENGINE_UNREACHABLE = Busy, "engine_unreachable",
        meaning "Nothing answers on the local channel.",
        next "Start the engine with `anything2mcp serve`, then run the command again. If an engine runs and does not answer, read its log.",
        passes not_by_itself;
    CHANNEL_UNAVAILABLE = Busy, "channel_unavailable",
        meaning "The channel, a lock file, or the loopback listener cannot be opened on this machine.",
        next "Check that the engine's runtime and data directories exist and belong to you, then try again.",
        passes not_by_itself;
    CONNECTION_LOST = Transient, "connection_lost",
        meaning "The connection to the engine ended before a request was sent, so the engine never had it.",
        next "Run the command again once an engine answers (`anything2mcp serve` starts one).";
    ANSWER_LOST = PossibleCommit, "answer_lost",
        meaning "The engine may have had a request, such as a tool call, and its answer was lost: what the request asked may have been done, a write among it.",
        next "If the request carried an idempotency key, send the same request again with the same key and nothing changed: it gives back the run the first one started, finished or not, and starts no other. If it carried none, do not send it again, because a second request may start a second run and write twice: find its run first with the run list (`anything2mcp run list --since <ms>`, with `--tool <tool>` for a tool call), from one minute before the request was sent, and do what that run's `next` says.";
    PEER_NOT_ALLOWED = Policy, "peer_not_allowed",
        meaning "A connection from another user, or an engine owned by another user.",
        next "Check who owns the engine's runtime directory, remove what is not yours to trust, and start the engine again.";
    PRESENCE_FAILED = Policy, "presence_failed",
        meaning "The person dismissed or failed the presence prompt, or this machine has none.",
        next "Run the command again and answer the prompt yourself; where this machine has no presence prompt, make the change in the installation's settings file.";
    PROMPT_BUSY = Busy, "prompt_busy",
        meaning "A presence prompt is already up on this machine, and one is raised at a time.",
        next "Answer or close that prompt, or let its page close, then run the command again.",
        passes by_itself;
    HELLO_INVALID = Input, "hello_invalid",
        meaning "A connection's first request was not a well-formed hello.",
        next "Reach the engine through `anything2mcp` or its SDK bridge, which say hello first; stop whatever else holds the engine's channel.";
    LOCAL_API_MISMATCH = Input, "local_api_mismatch",
        meaning "The client and the engine speak different versions of the local API.",
        next "Install the same version of the client and the engine, then run the command again.";
    DATA_DIRECTORY_GONE = Busy, "data_directory_gone",
        meaning "The data directory is no longer the one the engine opened: its engine lock is gone, or another file stands in its place. The engine refuses the request and stops; the next client starts a new engine.",
        next "Run the command again: a new engine starts on the data directory as it is now. Runs and published targets that were in the removed directory are not in a new one.",
        passes not_by_itself;
    SOCKET_PATH_TOO_LONG = Input, "socket_path_too_long",
        meaning "The engine's socket would have a path longer than the system takes for a socket's address, so the channel can be neither made nor reached there.",
        next "Set `A2M_HOME` to a folder with a shorter full path, then run the command again.";
    REQUEST_NOT_FROM_PAGE = Policy, "request_not_from_page",
        meaning "A request to the loopback listener did not come from the page this engine served: it named another host or origin, or a press came without the one-time value of the page as it was last shown, or with one already spent.",
        next "Open the approval link in your own browser on this machine and press there; a program cannot approve.";
    PAGE_NOT_FOUND = Input, "page_not_found",
        meaning "No page waits at that address on the loopback listener: it was answered, its window ended, the engine started again, or the address never held one.",
        next "Look at the run with `anything2mcp run get`; if it still waits, resume it for a new link.";
    CHANGE_NOT_SHOWN_WHOLE = Policy, "change_not_shown_whole",
        meaning "A change with more records than its page can show was pressed to approve, and a change not shown in full is not approved on the page.",
        next "Decline it from the page, and call the tool again with fewer records in one run.";
    SURFACE_UNAVAILABLE = Busy, "surface_unavailable",
        meaning "The person surface cannot show a page: it holds no approval desk.",
        next "Run `anything2mcp doctor run` for the reason.",
        passes not_by_itself;
}

/// A failure of the code of `row`, in the class its table gives it.
pub(crate) fn failure(
    row: &CodeEntry,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    row.failure(message, next)
}

/// The connection ended before the request was sent: nothing reached the
/// engine, so the request may be made again.
pub(crate) fn lost() -> Failure {
    lost_seeing(Seen::NoDetail)
}

/// As [`lost`], saying what the client saw, in a word of the closed list.
pub(crate) fn lost_seeing(seen: Seen) -> Failure {
    row::CONNECTION_LOST.failure(
        format!(
            "The connection to the engine ended before the request was sent, so the engine did not have it. What the client saw: {}.",
            seen.words()
        ),
        row::CONNECTION_LOST.next,
    )
}

/// The request was handed to the engine, and no whole answer came back.
/// The channel cannot know what the request was, so it never says that
/// sending it again is safe: a write sent twice may be written twice.
pub(crate) fn answer_lost(seen: Seen) -> Failure {
    row::ANSWER_LOST.failure(
        format!(
            "The engine may have had the request, and its answer was lost: what it asked may have been done, and a write may have started a run. What the client saw: {}.",
            seen.words()
        ),
        row::ANSWER_LOST.next,
    )
}

/// A problem of this machine: a directory, a lock file, or a listener that
/// cannot be opened. The error's own text is never told, because it may
/// hold a path. What is told is the system's error by its number and the
/// system's fixed words for that number, which name no path, so that a
/// person can look it up; for an error that has no number, its kind.
pub(crate) fn unavailable(what: &str, error: &std::io::Error) -> Failure {
    failure(
        &row::CHANNEL_UNAVAILABLE,
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
    failure(
        &row::SOCKET_PATH_TOO_LONG,
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
    failure(
        &row::DATA_DIRECTORY_GONE,
        "The engine's data directory was removed or replaced while it ran, so the engine took no request and is stopping. Nothing of this request was done.",
        "Run the command again: a new engine starts on the data directory as it is now. Runs and published targets that were in the removed directory are not in a new one.",
    )
}

#[cfg(test)]
mod tests {
    use a2m_core::FailureClass;

    use super::{ANSWER_LOST, CHANNEL_UNAVAILABLE, TABLE, answer_lost, failure, unavailable};

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

    #[test]
    fn the_design_lists_every_code_of_the_table_under_its_class() {
        let design = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("docs")
            .join("architecture")
            .join("crates")
            .join(format!("{}.md", env!("CARGO_PKG_NAME")));
        let design = std::fs::read_to_string(design).unwrap();
        for (class, code) in super::ALL {
            let row = format!("| `{class}` | `{code}` |");
            assert!(
                design.lines().any(|line| line.starts_with(&row)),
                "the design does not list `{code}` under `{class}`"
            );
        }
    }

    /// No source of this crate gives a failure a class of its own: each is
    /// made from a row of a table, which gives it its class. `start.rs`
    /// remakes the failure an engine wrote as it stopped, in the class it
    /// has, in two places: the line the engine writes, and the failure the
    /// client gives when it reads that line. A third place fails this test.
    #[test]
    fn every_failure_takes_its_class_from_a_table() {
        let found = a2m_core::testing::classes_of_their_own(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &[("start.rs", 2)],
        )
        .unwrap();
        assert!(
            found.is_empty(),
            "made with a class of their own: {found:#?}"
        );
        for row in TABLE {
            let made = failure(row, "A case.", row.next);
            assert_eq!((made.class(), made.code().as_str()), (row.class, row.code));
        }
    }
}
