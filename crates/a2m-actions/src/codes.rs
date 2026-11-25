//! The failure codes this crate produces, each with its one class, in one
//! table (decision 0258). Four of them, `schema_invalid`, `not_found`,
//! `engine_degraded`, and `idempotency_key_invalid`, are the core's, the
//! store's, and the scheduler's, and stay their constants here.
//!
//! A message is built from names the engine knows, never from a value a
//! system returned.
//!
//! Whether a `busy` code passes by itself, and why:
//!
//! - `lease_too_short` does not: a person raises `runners.lease_seconds`
//!   first.
//! - `run_call_unreadable` does not: what the run was called with stays
//!   missing or unreadable, and calling the tool again is a new run.
//! - `system_job_not_carried` does not: it is a fault in the engine.
//! - `tool_changed` does: the call made again is decided anew, against the
//!   tool as it is published now.

use a2m_core::{Failure, FailureClass, FailureCode};

/// The input breaks the action's schema, or no action has that name. Class
/// `input`. The core's code.
pub const SCHEMA_INVALID: FailureCode = FailureCode::SCHEMA_INVALID;
/// A valid input names something that does not exist. Class `input`. The
/// same code the store answers with.
pub const NOT_FOUND: FailureCode = a2m_ledger::codes::NOT_FOUND;
/// The health state forbids the action. Class `busy`. The same code the
/// scheduler answers with.
pub const ENGINE_DEGRADED: FailureCode = a2m_sched::codes::ENGINE_DEGRADED;
/// An idempotency key that is not 1 to 200 printable characters, or two
/// different keys on one request. Class `input`. The same code the store
/// and the REST door answer with.
pub const IDEMPOTENCY_KEY_INVALID: FailureCode = a2m_ledger::IDEMPOTENCY_KEY_INVALID;

a2m_core::failure_codes! {
    ACTION_NOT_BUILT = Input, "action_not_built",
        meaning "The action is in the catalog and its handler is not built yet.",
        next "Call `action.search` to see which actions are built.";
    PATH_NOT_ALLOWED = Input, "path_not_allowed",
        meaning "A project or file path is outside the allowed roots.",
        next "Name a path inside the project folder or the target's folder, one that is no link, or run the command inside a project folder of your own, or pass `--project` with its full path.";
    PROJECT_OF_ANOTHER_STORE = Input, "project_of_another_store",
        meaning "The project folder a call names is worked from another engine's store: its mark, `.anything2mcp/engine`, names another store than this engine's, so what this engine holds is not that project's scouts, trials, or published versions (decision 0445). The call is refused whole.",
        next "Run the command with the engine home the project was worked from (`A2M_HOME`, or none); or, to work the project from this engine from now on, remove `.anything2mcp/engine` in the project folder and run it again.";
    SETTINGS_INVALID = Input, "settings_invalid",
        meaning "A settings file, variable, or flag cannot be loaded.",
        next "Correct the key or value at the place the message names, or remove it, then start again.";
    SETTING_PROTECTED = Policy, "setting_protected",
        meaning "A layer that may only tighten tried to loosen a protected setting, or a layer set a key it may not set.",
        next "Remove the key from this layer, or set it in a layer that may loosen it.";
    SETTING_LOCKED = Policy, "setting_locked",
        meaning "A higher layer tried to change a key the installation file locks.",
        next "Remove the key from this layer, or ask whoever installed the engine to change it.";
    REGISTRY_INVALID = Input, "action_registry_invalid",
        meaning "The registry of actions cannot be built: a duplicate name, or a provider's action outside its name space.",
        next "Give each action one name, starting with its provider's own name, then start again.";
    NETWORK_NOT_RECOGNIZABLE = Input, "network_not_recognizable",
        meaning "The network the machine is on states too few facts to be recognized again, so no reach can be named on it.",
        next "Connect to a network with a default route and a gateway that answers, then name it again.";
    PERSON_REQUIRED = Policy, "person_required",
        meaning "A person's act was asked for by a caller that cannot give one.",
        next "Ask the person to do it themselves, through a program that acts for them.";
    CALLER_NOT_VERIFIED = Policy, "caller_not_verified",
        meaning "The caller's credential is not one this engine accepts.",
        next "Call from the account that runs the engine.";
    CLIENT_ACTION = Input, "client_action",
        meaning "The action is carried out by the client end, which did not do so.",
        next "Run it as a command on the machine the engine runs on.";
    LEASE_TOO_SHORT = Busy, "lease_too_short",
        meaning "A step may take longer than a job's lease lasts.",
        next "Raise `runners.lease_seconds` above the tool's longest step time limits, then resume the run.",
        passes not_by_itself;
    RUN_CALL_UNREADABLE = Busy, "run_call_unreadable",
        meaning "What a run was called with is missing from the store or cannot be read.",
        next "Call the tool again; that is a new run. Call `trace.show` with this run's identifier to see what it did before.",
        passes not_by_itself;
    SYSTEM_JOB_NOT_CARRIED = Busy, "system_job_not_carried",
        meaning "The in-process runner loop was handed a job that is not a run's.",
        next "Report this; it is a fault in the engine.",
        passes not_by_itself;
    SETTING_NOT_IN_FORCE = Policy, "setting_not_in_force",
        meaning "A protected setting is set to a value this build cannot put in force.",
        next "Take the setting out of the layer the message names, or set it to the value in force, then start the engine again.";
    TOOL_CHANGED = Busy, "tool_changed",
        meaning "A published tool changed between the decision about a call and the call.",
        next "Call the tool again.",
        passes by_itself;
    IDEMPOTENCY_KEY_REUSED = Input, "idempotency_key_reused",
        meaning "An idempotency key the caller already gave with another call: another tool, environment, input, or source.",
        next "Use a new key for a new call. To get the run of the call the key was first given with, make that call again with the key, or call `run.list` with `idempotency_key`.";
    ADDRESS_UNREGISTERED = Policy, "address_unregistered",
        meaning "A publish would reach an address no person registered for the target, and it was not carried out from the publish page that names it.",
        next "Ask for the publish on its page, with `publish.request`, read the new address it names, and press there.";
    DRAFTS_CHANGED = Stale, "drafts_changed",
        meaning "The drafts of a publish a person was shown changed before they pressed: they compile to another compiled target.",
        next "Ask again, and read the new page before pressing.";
    PUBLISH_GATES_NOT_PASSED = Input, "publish_gates_not_passed",
        meaning "A publish whose offline tests did not all run and pass: a version is published only when they do (spec section 403, decision 0180).",
        next "Call `test.run` with the target to see what each test found, correct the test, its fixture, or the binding, and publish again.";
    SCOUT_OFF = Policy, "scout_off",
        meaning "The setting `authoring.scout.run` is `off`, and `scout.run` was called.",
        next "Run the stages one by one (`target.probe`, `discover.run`, `draft.write`), or ask a person to change the setting.";
    DISCOVERY_FILE_NOT_TAKEN = Input, "discovery_file_not_taken",
        meaning "A call names a file for a discovery under a name that no discoverer suiting the target's observed surfaces reads (decision 0263): a file nobody reads is a mistake, never ignored. Nothing starts.",
        next "Name each file under one of the names the message lists, or leave it out.";
    OFFLINE_WRITES_NOTHING = Policy, "offline_writes_nothing",
        meaning "An offline test asked for something that would store, lease, or clear a write; its control plane holds nothing and refuses (decision 0177).",
        next "Keep the offline test to its own tool against its fixture; a write is proven against a test stand.";
    FIXTURE_NO_SCRUB_RULE = Input, "fixture_no_scrub_rule",
        meaning "`fixture.record` met an observed value that no scrubbing rule covers, and wrote nothing (decision 0196).",
        next "Write this fixture by hand with synthetic values, or record a read whose values are text, numbers, dates, choices, true or false, or empty.";
    FIXTURE_READ_FAILED = Input, "fixture_read_failed",
        meaning "The read `fixture.record` runs did not complete in the sandbox, or gave nothing to record; nothing was written.",
        next "Correct the draft or the input and record again, or write this fixture by hand.";
    FIXTURE_NOT_REPLAYED = Input, "fixture_not_replayed",
        meaning "The test `fixture.record` made does not pass against its scrubbed fixture, or an observed value would be in a file; nothing was written.",
        next "Write this fixture by hand with synthetic values its field types take.";
    CLIENT_CONFIG_REFUSED = Input, "client_config_refused",
        meaning "`client.connect` cannot set a client's configuration: its client profile does not read, names no file for this system, or the client's file does not read as a JSON object (decision 0219). Nothing is written.",
        next "Correct the client profile or its file, as the message names, then run the command again.";
    BINDING_NOT_PROMOTABLE = Input, "binding_not_promotable",
        meaning "A binding `binding.promote` may not make verified: not a drafted first binding of its operation, or one whose file cannot be rewritten by rule (decision 0189).",
        next "Write the binding with `status: draft`, test it and try it, then call `binding.promote`.";
    PROMOTION_UNPROVEN = Input, "promotion_unproven",
        meaning "A promotion whose evidence is missing: an offline test of the operation that is missing or did not pass, or a read no trial proved at `executed` (decision 0189).",
        next "Write or correct the operation's offline test, run `test.run`, and promote again.";
    NOT_MADE_READY_NOT_A_READ = Input, "not_made_ready_not_a_read",
        meaning "`draft.ready` was named a tool that is a write or a pipeline: it makes reads ready alone, and leaves a write's test, fixture, and promote to be done by hand and by a person (decision 0443).",
        next "Write the write's offline test and fixture with `draft.write`, run `test.run`, and have a person promote its binding; or leave it out of `tools`.";
    NOT_MADE_READY_UNTRIED = Input, "not_made_ready_untried",
        meaning "`draft.ready` was named a read that no trial in this project has read a value through: the newest finished scout did not prove it, and the caller has no completed trial of it made since (decision 0443).",
        next "Try the read with `draft.try` and an input it answers, or sign in where it waits for a sign-in, then call `draft.ready` again.";
    NOTHING_ASSERTED = Input, "nothing_asserted",
        meaning "`assertion.confirm` was asked for a target whose drafts assert no read: no entry says `read_asserted: true` (decision 0999).",
        next "Assert the read in its entry with `effect: read` and `read_asserted: true`, with an `enum` on the field that tells a read from a write, then ask again.";
    ASSERTION_CHANGED = Stale, "assertion_changed",
        meaning "The reads a target's drafts assert changed between the confirmation's page and the person's press: what would be confirmed is not what the person read, so nothing was (decision 0999).",
        next "Ask for the confirmation again, and confirm the reads as they are now.";
    CAPTURE_NOT_OFFERED = Input, "capture_not_offered",
        meaning "No enabled pack watches a person's use of the surface: no discoverer a person drives suits it, so no capture of it can be asked for.",
        next "Scout the target with `scout.run` instead, or enable a pack whose discoverer a person drives.";
    CAPTURE_ENDED = Stale, "capture_ended",
        meaning "The capture was ended, by the one who asked for it, before its person began it on its page, so nothing was recorded.",
        next "Ask for a new capture with `capture.start`.";
    RUN_NOT_COMPLETED = Rejected, "run_not_completed",
        meaning "A run ended in a state other than completed, and carried no failure of its own.",
        next "Call `trace.show` with the run's identifier to see what happened.";
    EXPORT_CHANGED = Stale, "export_changed",
        meaning "`trace.export` wrote its file and, read back, the file did not hold what was written: something changed it in between.",
        next "Export again, and let nothing else write in the project's working folder meanwhile.";
    VERSION_MOVED = Stale, "version_moved",
        meaning "`version.rollback` made its version current and stored that, and when it read the store back another version was current: a publish or a rollback came between.",
        next "Call `target.status` to see which version is current, and roll back again if it is not the one wanted.";
}

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
}
