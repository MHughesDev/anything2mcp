//! The states of a run, a record, a session, a binding, a provider, and a
//! subsystem.

use crate::vocabulary::vocabulary;

vocabulary! {
    /// Where a run stands (spec section 208). This is the `status` a caller
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
        /// A declared run question waits for a person.
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
    /// Where one record of a run stands in the ledger (spec section 208).
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
    /// Where a signed-in session stands (spec section 215).
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

vocabulary! {
    /// The one state an availability check ends in (spec section 203): can
    /// this provider do its work here, now. The check inspects the machine
    /// and starts nothing.
    ///
    /// A check that could not be made is `unknown`, which is the default and
    /// is never treated as `ready`.
    #[derive(Default)]
    pub enum Availability: "availability state" {
        /// Everything it needs is present and matches.
        Ready => "ready",
        /// Installed and switched off.
        NotEnabled => "not_enabled",
        /// An outside program or runtime it needs was not found.
        ComponentMissing => "component_missing",
        /// The program was found and its digest is not a known one.
        ComponentUnverified => "component_unverified",
        /// This operating system or architecture is not supported.
        UnsupportedHost => "unsupported_host",
        /// A needed program is present in a version the provider does not
        /// support.
        UnsupportedVersion => "unsupported_version",
        /// A setting it needs is absent or wrong.
        Misconfigured => "misconfigured",
        /// The check could not be made.
        #[default]
        Unknown => "unknown",
    }
}

impl Availability {
    /// Whether the provider may be used. Only `ready` is; `unknown` is not.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

vocabulary! {
    /// The health of a long-lived connection to a subsystem (spec section
    /// 204). The supervisor gives one with a [`SubsystemMove`].
    pub enum SubsystemState: "subsystem state" {
        /// Running, and serving nothing.
        Idle => "idle",
        /// Running, and serving a request.
        Busy => "busy",
        /// The process has ended.
        Exited => "exited",
        /// Nothing answers, and whether it still runs is not known.
        Unreachable => "unreachable",
    }
}

vocabulary! {
    /// What to do about a subsystem in its present state (spec section 204).
    pub enum SubsystemMove: "subsystem move" {
        /// Ask again.
        Retry => "retry",
        /// Let the request in progress finish.
        Wait => "wait",
        /// Stop it and start it again.
        Restart => "restart",
    }
}

vocabulary! {
    /// What `doctor.run` reports for each provider and each subsystem it
    /// needs (spec section 204). What cannot be established is `unknown`,
    /// which is the default and is never rounded up to `healthy`.
    #[derive(Default)]
    pub enum DoctorState: "doctor state" {
        /// Present, and working.
        Healthy => "healthy",
        /// An outside program or runtime it needs was not found.
        ComponentMissing => "component_missing",
        /// It started, and failed while running.
        RuntimeFailure => "runtime_failure",
        /// This operating system or architecture is not supported.
        UnsupportedHost => "unsupported_host",
        /// The system it would reach is one it does not support.
        UnsupportedTarget => "unsupported_target",
        /// Its settings no longer match what was recorded.
        ConfigDrift => "config_drift",
        /// It could not be established.
        #[default]
        Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::{Availability, DoctorState, RunState};

    #[test]
    fn what_is_not_established_is_unknown_and_never_ready() {
        assert_eq!(Availability::default(), Availability::Unknown);
        assert_eq!(DoctorState::default(), DoctorState::Unknown);
        let ready: Vec<&str> = Availability::ALL
            .iter()
            .filter(|state| state.is_ready())
            .map(|state| state.as_str())
            .collect();
        assert_eq!(ready, ["ready"]);
    }

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
