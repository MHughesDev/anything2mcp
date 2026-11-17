//! The failure codes this crate produces. Each belongs to exactly one class.
//!
//! A message is built from names the engine knows, never from a value a
//! system returned.

use a2m_core::{Failure, FailureClass, FailureCode};

/// The input breaks the action's schema, or no action has that name. Class
/// `input`.
pub const SCHEMA_INVALID: FailureCode = FailureCode::SCHEMA_INVALID;
/// The action is in the catalog and its handler is not built yet. Class
/// `input`.
pub const ACTION_NOT_BUILT: FailureCode = FailureCode::fixed("action_not_built");
/// A valid input names something that does not exist. Class `input`. The
/// same code the store answers with.
pub const NOT_FOUND: FailureCode = a2m_ledger::codes::NOT_FOUND;
/// A project or file path is outside the allowed roots. Class `input`.
pub const PATH_NOT_ALLOWED: FailureCode = FailureCode::fixed("path_not_allowed");
/// A settings file, variable, or flag cannot be loaded. Class `input`.
pub const SETTINGS_INVALID: FailureCode = FailureCode::fixed("settings_invalid");
/// A layer that may only tighten tried to loosen a protected setting, or a
/// layer set a key it may not set. Class `policy`.
pub const SETTING_PROTECTED: FailureCode = FailureCode::fixed("setting_protected");
/// A higher layer tried to change a key the installation file locks. Class
/// `policy`.
pub const SETTING_LOCKED: FailureCode = FailureCode::fixed("setting_locked");
/// The registry of actions cannot be built: a duplicate name, or a
/// provider's action outside its name space. Class `input`.
pub const REGISTRY_INVALID: FailureCode = FailureCode::fixed("action_registry_invalid");
/// A person's act was asked for by a caller that cannot give one. Class
/// `policy`.
pub const PERSON_REQUIRED: FailureCode = FailureCode::fixed("person_required");
/// The caller's credential is not one this engine accepts. Class `policy`.
pub const CALLER_NOT_VERIFIED: FailureCode = FailureCode::fixed("caller_not_verified");
/// The health state forbids the action. Class `busy`. The same code the
/// scheduler answers with.
pub const ENGINE_DEGRADED: FailureCode = a2m_sched::codes::ENGINE_DEGRADED;
/// The action is carried out by the client end, which did not do so. Class
/// `input`.
pub const CLIENT_ACTION: FailureCode = FailureCode::fixed("client_action");

/// A step may take longer than a job's lease lasts. Class `busy`.
pub const LEASE_TOO_SHORT: FailureCode = FailureCode::fixed("lease_too_short");
/// What a run was called with is missing from the store or cannot be read.
pub const RUN_CALL_UNREADABLE: FailureCode = FailureCode::fixed("run_call_unreadable");
/// The in-process runner loop was handed a job that is not a run's.
pub const SYSTEM_JOB_NOT_CARRIED: FailureCode = FailureCode::fixed("system_job_not_carried");
/// A protected setting is set to a value this build cannot put in force.
pub const SETTING_NOT_IN_FORCE: FailureCode = FailureCode::fixed("setting_not_in_force");
/// A published tool changed between the decision about a call and the call.
pub const TOOL_CHANGED: FailureCode = FailureCode::fixed("tool_changed");
/// An idempotency key that is not 1 to 200 printable characters, or two
/// different keys on one request. Class `input`. The same code the store
/// and the REST door answer with.
pub const IDEMPOTENCY_KEY_INVALID: FailureCode = a2m_ledger::IDEMPOTENCY_KEY_INVALID;
/// An idempotency key the caller already gave with another call: another
/// tool, environment, input, or source. Class `input`.
pub const IDEMPOTENCY_KEY_REUSED: FailureCode = FailureCode::fixed("idempotency_key_reused");

/// The drafts of a publish a person was shown changed before they pressed:
/// they compile to another plan. Class `stale`.
pub const DRAFTS_CHANGED: FailureCode = FailureCode::fixed("drafts_changed");

/// The setting `authoring.scout.run` is `off`, and `scout.run` was called.
/// Class `policy`.
pub const SCOUT_OFF: FailureCode = FailureCode::fixed("scout_off");

pub(crate) fn stale(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Stale, code, message, next)
}

pub(crate) fn input(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Input, code, message, next)
}

pub(crate) fn policy(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Policy, code, message, next)
}

pub(crate) fn busy(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Busy, code, message, next)
}

/// A caller's text, cut and cleaned, for a message. Never a secret: the
/// dispatcher passes every message through the redactor as well.
pub(crate) fn shown(text: &str) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control()).take(80).collect();
    clean
}

/// The known names nearest to `wrong`, at most three, for a fix.
pub(crate) fn nearest<'a>(wrong: &str, known: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut scored: Vec<(usize, &str)> = known
        .map(|name| (distance(wrong, name), name))
        .filter(|(score, name)| *score <= (name.len().max(wrong.len()) / 3).max(2))
        .collect();
    scored.sort();
    scored.into_iter().take(3).map(|(_, name)| name).collect()
}

/// The edit distance between two short texts.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let next = (previous + cost).min(row[j] + 1).min(row[j + 1] + 1);
            previous = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::nearest;

    #[test]
    fn the_nearest_names_are_offered_and_far_ones_are_not() {
        let known = ["tool.call", "tool.search", "run.get", "docs.get"];
        assert_eq!(
            nearest("tool.cal", known.iter().copied()),
            vec!["tool.call"]
        );
        assert!(nearest("zzzzzzzzzzzz", known.iter().copied()).is_empty());
    }
}
