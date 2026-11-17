//! Whether a process group on Linux or macOS has been ended, from what the
//! system answered when the group was signalled and what is known of its
//! leader. The calls to the system are in the Unix side; the decision is
//! kept here, apart from them, so that every answer either system can give
//! is tested on every system.
//!
//! The leader is never reaped before the verdict, so the group's identifier
//! is still the supervisor's while it is signalled.

/// What the system answered when the whole group was sent the signal to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    /// The signal reached at least one member of the group.
    Signalled,
    /// There is no such group: every member has exited and been reaped.
    NoGroup,
    /// The group exists and nothing in it could be signalled. On Linux this
    /// means every living member belongs to another user. On macOS it means
    /// that, or that every member has exited or is in the middle of exiting:
    /// macOS counts no process that has begun to exit, and a leader that is
    /// in the middle of exiting cannot be waited for yet either.
    NothingSignalled,
    /// Any other answer.
    Failed,
}

/// What is known of the group's leader, which has not been reaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leader {
    /// It has exited and waits to be reaped.
    Exited,
    /// It has not been seen to exit, and has not been waited for.
    NotSeenToExit,
    /// It was waited for, for the whole grace, and did not exit.
    StayedThroughGrace,
}

/// The verdict on one signal to the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Nothing of the group is left running.
    Ended,
    /// Something of the group may still be running, and could not be ended.
    NotEnded,
    /// Not known yet: wait for the leader's exit, for at most the grace,
    /// then signal the group again and judge that answer.
    WaitForLeader,
}

/// The verdict on a signal to the group.
///
/// Only one answer needs the leader. "Nothing could be signalled" with the
/// leader exited is the ordinary end of a program that left nothing
/// running, on macOS; Linux answers success there. With the leader not yet
/// seen to exit it is not known: on macOS a leader in the middle of its exit
/// is neither signalled nor yet seen as exited, and the moment passes, so
/// the supervisor waits for the exit. A leader that does not exit within the
/// grace is one the engine's user may not signal, and the group cannot be
/// ended.
pub(crate) fn verdict(answer: Answer, leader: Leader) -> Verdict {
    match (answer, leader) {
        (Answer::Signalled | Answer::NoGroup, _) | (Answer::NothingSignalled, Leader::Exited) => {
            Verdict::Ended
        }
        (Answer::NothingSignalled, Leader::NotSeenToExit) => Verdict::WaitForLeader,
        (Answer::NothingSignalled, Leader::StayedThroughGrace) | (Answer::Failed, _) => {
            Verdict::NotEnded
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Answer, Leader, Verdict, verdict};

    const LEADERS: [Leader; 3] = [
        Leader::Exited,
        Leader::NotSeenToExit,
        Leader::StayedThroughGrace,
    ];

    #[test]
    fn a_signal_that_reached_the_group_or_found_no_group_is_its_end() {
        for leader in LEADERS {
            assert_eq!(verdict(Answer::Signalled, leader), Verdict::Ended);
            assert_eq!(verdict(Answer::NoGroup, leader), Verdict::Ended);
        }
    }

    #[test]
    fn any_other_failure_to_signal_is_a_group_that_was_not_ended() {
        for leader in LEADERS {
            assert_eq!(verdict(Answer::Failed, leader), Verdict::NotEnded);
        }
    }

    #[test]
    fn nothing_to_signal_is_the_end_once_the_leader_has_exited() {
        // A program that exited and left nothing running, on macOS.
        assert_eq!(
            verdict(Answer::NothingSignalled, Leader::Exited),
            Verdict::Ended
        );
    }

    #[test]
    fn nothing_to_signal_while_the_leader_is_not_seen_to_exit_waits_for_it() {
        // On macOS: a leader in the middle of its exit, as a browser that
        // was asked to close is when the end of its pipe is seen.
        assert_eq!(
            verdict(Answer::NothingSignalled, Leader::NotSeenToExit),
            Verdict::WaitForLeader
        );
    }

    #[test]
    fn nothing_to_signal_with_a_leader_that_stays_is_a_group_that_cannot_be_ended() {
        // A leader the engine's user may not signal, alive.
        assert_eq!(
            verdict(Answer::NothingSignalled, Leader::StayedThroughGrace),
            Verdict::NotEnded
        );
    }

    #[test]
    fn a_wait_is_asked_for_only_before_the_leader_was_waited_for() {
        // So a second signal, after the wait, always gives a verdict.
        for answer in [
            Answer::Signalled,
            Answer::NoGroup,
            Answer::NothingSignalled,
            Answer::Failed,
        ] {
            for leader in [Leader::Exited, Leader::StayedThroughGrace] {
                assert_ne!(verdict(answer, leader), Verdict::WaitForLeader);
            }
        }
    }
}
