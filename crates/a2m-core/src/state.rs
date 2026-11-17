//! The states of a run, a record, a session, and a binding.

use crate::vocabulary::vocabulary;

vocabulary! {
    /// Where a run stands (spec section 42). This is the `status` a caller
    /// sees.
    pub enum RunState: "run state" {
        /// Accepted, waiting for a session or a runner.
        Queued => "queued",
        /// Checking inputs, reading current values, building the diff.
        Validating => "validating",
        /// Inputs invalid; nothing was changed.
        Rejected => "rejected",
        /// The diff is ready and waits for a person.
        NeedsApproval => "needs_approval",
        /// The approval was declined or timed out.
        Expired => "expired",
        /// Steps are executing.
        Running => "running",
        /// A declared decision point waits for a person.
        NeedsInput => "needs_input",
        /// The session expired and could not renew itself.
        NeedsLogin => "needs_login",
        /// Halted with a failure class; can be resumed.
        Stopped => "stopped",
        /// Every record verified or unchanged.
        Completed => "completed",
        /// Halted and cannot be resumed.
        Failed => "failed",
        /// Stopped at a person's or a caller's request.
        Cancelled => "cancelled",
    }
}

impl RunState {
    /// Whether a run in this state can never leave it.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(
            self,
            Self::Rejected | Self::Expired | Self::Completed | Self::Failed | Self::Cancelled
        )
    }
}

vocabulary! {
    /// Where one record of a run stands in the ledger (spec section 42).
    pub enum RecordState: "record state" {
        /// Known from the source; nothing done.
        Planned => "planned",
        /// Already equals the intended values; no write is needed.
        Unchanged => "unchanged",
        /// Intended values recorded; about to act.
        Intent => "intent",
        /// The write was sent; no confirmation is recorded yet.
        Acted => "acted",
        /// Read back and matched.
        Verified => "verified",
        /// Stopped with a failure class.
        Failed => "failed",
    }
}

vocabulary! {
    /// Where a signed-in session stands (spec section 44).
    pub enum SessionState: "session state" {
        /// Never signed in, or ended.
        Absent => "absent",
        /// A person or a stored credential is signing in.
        SigningIn => "signing_in",
        /// Signed in and recently confirmed.
        Valid => "valid",
        /// One check failed; it is being checked again.
        Suspect => "suspect",
        /// A silent renewal is in progress.
        Renewing => "renewing",
        /// The identity provider demands a person.
        NeedsPerson => "needs_person",
        /// Ended, and could not be renewed.
        Expired => "expired",
        /// Ended on purpose.
        Revoked => "revoked",
    }
}

vocabulary! {
    /// How far a binding has come (file formats, "Binding").
    pub enum BindingStatus: "binding status" {
        /// Written and not yet compared with anything.
        Draft => "draft",
        /// Predicting beside the verified binding, without acting.
        Shadow => "shadow",
        /// Proven to give the same result, and allowed to act.
        Verified => "verified",
        /// No longer used.
        Retired => "retired",
    }
}

#[cfg(test)]
mod tests {
    use super::RunState;

    #[test]
    fn five_run_states_are_final() {
        let finals: Vec<&str> = RunState::ALL
            .iter()
            .filter(|state| state.is_final())
            .map(|state| state.as_str())
            .collect();
        assert_eq!(
            finals,
            ["rejected", "expired", "completed", "failed", "cancelled"]
        );
    }
}
