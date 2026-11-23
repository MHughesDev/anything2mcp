//! The failure codes this crate produces, each with its one class. A refusal
//! tells the caller the code and what to ask for; it never holds the content
//! of a rule.

a2m_core::failure_codes! {
    NOT_GRANTED = Policy, "not_granted",
        meaning "No permit applies to the caller, the tool, or the action.",
        next "Ask an administrator for a grant that covers the caller, the tool, and the action.";
    IDENTITY_NOT_ALLOWED = Policy, "identity_not_allowed",
        meaning "The run may not act as the identity, including every attempt to use another person's session.",
        next "Use a toolset whose identity is the caller's own, or ask the person to delegate.";
    EFFECT_TOO_HIGH = Policy, "effect_too_high",
        meaning "A grant exists, with a lower highest effect class.",
        next "Ask an administrator to raise the grant's highest effect class.";
    APPROVAL_MISSING = Policy, "approval_missing",
        meaning "A write reached the write phase with no usable approval.",
        next "Have a person approve the run's diff, then resume the run.";
    APPROVAL_STALE = Policy, "approval_stale",
        meaning "The approval is expired, used, declined, revoked, or for another diff.",
        next "Preview the run again and have a person approve the new diff.";
    APPROVAL_REFUSED = Policy, "approval_refused",
        meaning "The person or the caller may not approve this run.",
        next "Have a person who may approve this run approve it on the approval surface.";
    LIMIT_EXCEEDED = Policy, "limit_exceeded",
        meaning "A call is over the records allowed in one run, or over a quota.",
        next "Send fewer records in one run, or wait for the next period of the quota.";
    KILL_SWITCH_ON = Policy, "kill_switch_on",
        meaning "A kill switch covers the request.",
        next "Ask an administrator why the kill switch is set, and to release it.";
    POLICY_EVALUATION_FAILED = Policy, "policy_evaluation_failed",
        meaning "An error while deciding, or no valid policy set is loaded, so the answer is no.",
        next "Check the engine's status; if no valid policy set is loaded, publish one.";
    PRESENCE_REQUIRED = Policy, "presence_required",
        meaning "A change to a protected setting that only a person, present, may make.",
        next "Have the person make the change and pass the user-presence prompt.";
    SECOND_ADMIN_REQUIRED = Policy, "second_admin_required",
        meaning "A policy set that needs a second administrator to publish it was offered without one, or the second one may not review it.",
        next "Have a second administrator, a person other than the publisher and the author, review and publish it with you.";
    POLICY_TESTS_FAILED = Input, "policy_tests_failed",
        meaning "A policy set whose own test table does not pass.",
        next "Correct the set or its tests until its test table passes, then publish it again.";
    DECISION_NOT_FOUND = Input, "decision_not_found",
        meaning "No decision with that identifier is recorded.",
        next "Give the identifier of a recorded decision; it starts with `d_`.";
}
