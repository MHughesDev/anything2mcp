//! Every action of the action catalog, as data.
//!
//! One entry per row of `docs/reference/action-catalog.md`, in its order. A
//! test reads that document and holds the two together. An entry says what
//! the action is; whether it is served is decided by `handlers`, which
//! names a function for each action that is built.

use a2m_core::EffectClass;
use a2m_files::JsonSchema;
use a2m_policy::Role;

use crate::def::{ActionDef, Effect, FieldType, HealthNeed, Listed, PersonNeed, Shape, Who, act};

const TEXT: FieldType = FieldType::Text;
const BOOL: FieldType = FieldType::Bool;
const LIST: FieldType = FieldType::TextList;
const MAP: FieldType = FieldType::Map;
const COUNT: FieldType = FieldType::Int { min: 1, max: 1_000 };
const OFFSET: FieldType = FieldType::Int {
    min: 0,
    max: 1_000_000,
};

const VIEWER: Who = Who::Role(Role::Viewer);
const OPERATOR: Who = Who::Role(Role::Operator);
const BUILDER: Who = Who::Role(Role::Builder);
const PUBLISHER: Who = Who::Role(Role::Publisher);
const ADMIN: Who = Who::Role(Role::Admin);
const AUDITOR: Who = Who::Role(Role::Auditor);

const TARGET: &str = "The target's name.";
const RUN: &str = "The run's identifier, as an earlier result gave it.";
const LIMIT: &str = "The most entries to return.";
const OFFSET_HELP: &str = "How many entries to skip.";

fn text() -> JsonSchema {
    JsonSchema::text()
}

fn list_of(item: JsonSchema) -> JsonSchema {
    JsonSchema::list(item)
}

fn any_map() -> JsonSchema {
    JsonSchema::map_of(JsonSchema::any())
}

/// The shape of a run, as `tool.call`, `run.get`, and `run.resume` return it.
fn run_output() -> JsonSchema {
    JsonSchema::object()
        .required("run_id", text())
        .required("tool", text())
        .required("status", text())
        .required("summary", text())
        .optional("mode", text())
        .optional("result", JsonSchema::any())
        .optional("failure", any_map())
        .optional("diff", any_map())
        .optional("trace", text())
        .optional("link", text())
        .optional("person", text())
        .optional("approved", JsonSchema::bool())
        .optional("events", JsonSchema::int())
        .optional("repeated", JsonSchema::bool())
        .optional("not_read_back", JsonSchema::int())
        .optional("earlier_run", JsonSchema::bool())
}

/// A published version, as `version.publish` returns it.
fn published() -> JsonSchema {
    JsonSchema::object()
        .required("target", text())
        .required("version", text())
        .required("plan_hash", text())
        .required("tools", list_of(text()))
        .required("current", JsonSchema::bool())
}

/// One run of a listing: what a caller needs to decide what to do next.
fn listed_run() -> JsonSchema {
    JsonSchema::object()
        .required("run_id", text())
        .required("tool", text())
        .required("target", text())
        .required("status", text())
        .required("created_at", JsonSchema::int())
        .optional("started_at", JsonSchema::int())
        .optional("ended_at", JsonSchema::int())
        .required("in_doubt", JsonSchema::bool())
        .required("waits_for_person", JsonSchema::bool())
        .optional("failure", any_map())
        .required("next", any_map())
}

pub(crate) fn definitions() -> Vec<ActionDef> {
    use Listed::{Build, No, Use, UseAndBuild};
    use Shape::{Change, Compare, Inspect, List, Observe, Trace};

    vec![
        // Using tools.
        act("tool.search", "Find operations and pipelines of the published targets by words or module. Returns names, one-line descriptions, and effects; call tool.describe for a schema.", VIEWER, Use, "1", List)
            .opt_pos("query", TEXT, "Words to look for in names and descriptions. Leave out to list everything.")
            .opt("target", TEXT, TARGET)
            .opt("module", TEXT, "A module of the target.")
            .opt("kind", FieldType::Word(&["operation", "pipeline"]), "Only operations, or only pipelines. Both when left out.")
            .opt("limit", COUNT, LIMIT)
            .opt("offset", OFFSET, OFFSET_HELP)
            .opt("full", BOOL, "Return each tool in full, as tool.describe gives it: title, hazard, plan hash, input schema, output schema, and annotations. For listing a toolset in one call.")
            .output(JsonSchema::object()
                .required("tools", list_of(JsonSchema::object()
                    .required("name", text())
                    .required("target", text())
                    .required("version", text())
                    .required("kind", text())
                    .required("effect", text())
                    .required("description", text())
                    .optional("plan_hash", text())
                    .optional("hazard", text())
                    .optional("title", text())
                    .optional("input_schema", any_map())
                    .optional("output_schema", any_map())
                    .optional("annotations", any_map())))
                .required("total", JsonSchema::int())
                .required("complete", JsonSchema::bool()))
            .example(r#"{"query": "order"}"#)
            .alias("GET", "/tools")
            .done(),
        act("tool.describe", "The full description, input schema, and output schema of one published tool, by its name.", VIEWER, Use, "1", Inspect)
            .pos("tool", TEXT, "The tool's full name.")
            .output(JsonSchema::object()
                .required("name", text())
                .required("target", text())
                .required("version", text())
                .required("kind", text())
                .required("effect", text())
                .required("hazard", text())
                .required("plan_hash", text())
                .required("title", text())
                .required("description", text())
                .required("input_schema", any_map())
                .required("output_schema", any_map())
                .required("annotations", any_map()))
            .example(r#"{"tool": "example_orders_order_get"}"#)
            .done(),
        act("tool.call", "Call one published operation or pipeline by name with its input. Returns the result of a read, or the run's resting state (such as needs_approval) with the next step. A write never proceeds without a person's approval.", Who::ByEffect, Use, "1", Change)
            .pos("tool", TEXT, "The tool's full name.")
            .opt("input", MAP, "The tool's own input, as its schema describes.")
            .opt("source_file", TEXT, "For a tool that reads a source: the file, by its path inside the project folder. It is read once; a resumed run reads the same bytes.")
            .opt("source_rows", FieldType::MapList, "For a tool that reads a source, in place of a file: the rows themselves, for a small input. A list of records, read as a JSON document.")
            .opt("environment", TEXT, "The environment to run in. The target's first production environment when left out.")
            .opt("wait_seconds", FieldType::Int { min: 0, max: 600 }, "How long to wait for the run to rest or end before returning its run_id.")
            .opt("idempotency_key", TEXT, "A key of your own, 1 to 200 printable characters with no space, that makes a repeated call the same call: the same call with the same key gives back the run the first one started, unfinished or finished, and starts no other. The same key with another call is refused. Send one with every call that may write, and after a lost answer make the same call again with the same key.")
            .effect(Effect::OfTool)
            .needs(HealthNeed::Reads)
            .long_running()
            .output(run_output())
            .example(r#"{"tool": "example_orders_order_get", "input": {"order_id": "A-100"}}"#)
            .done(),
        act("run.get", "The status, progress, result, and trace reference of one run.", VIEWER, Use, "1", Trace)
            .pos("run_id", TEXT, RUN)
            .output(run_output())
            .example(r#"{"run_id": "run_01h0000000000000000000000"}"#)
            .alias("GET", "/runs/{run_id}")
            .done(),
        act("run.list", "The caller's recent runs, newest first: each run's state, whether a record of it is in doubt, whether it waits for a person, and the next step. After a lost answer, list the runs since the call was sent to find its run before calling again.", VIEWER, No, "3", List)
            .opt("state", LIST, "Only runs in these states, such as `stopped` or `needs_approval`.")
            .opt("tool", TEXT, "Only runs of this tool, by its full name.")
            .opt("target", TEXT, TARGET)
            .opt("since", FieldType::Int { min: 0, max: 99_999_999_999_999 }, "Only runs started at or after this moment, in whole milliseconds since 1970 in UTC.")
            .opt("idempotency_key", TEXT, "Only the run the call with this idempotency key started.")
            .opt("limit", FieldType::Int { min: 1, max: 100 }, "The most runs to return: 20 when left out, at most 100.")
            .opt("after", TEXT, "The `run_id` the previous page ended with: the page starts after it.")
            .output(JsonSchema::object()
                .required("runs", list_of(listed_run()))
                .required("complete", JsonSchema::bool())
                .optional("after", text()))
            .example(r#"{"since": 1767225600000, "tool": "example_orders_item_upsert"}"#)
            .alias("GET", "/runs")
            .done(),
        act("run.resume", "Continue a run that rests or stopped. The engine reconciles every doubtful record first; an approved write goes on from its checkpoint.", OPERATOR, Use, "3", Change)
            .pos("run_id", TEXT, RUN)
            .opt("wait_seconds", FieldType::Int { min: 0, max: 600 }, "How long to wait for the run to rest or end before returning.")
            .effect(Effect::Fixed(EffectClass::Write))
            .needs(HealthNeed::Reads)
            .long_running()
            .output(run_output())
            .alias("POST", "/runs/{run_id}/resume")
            .done(),
        act("run.cancel", "Stop a running or waiting run. Says what was completed before it stopped.", OPERATOR, Use, "3", Change)
            .pos("run_id", TEXT, RUN)
            .writes()
            .alias("POST", "/runs/{run_id}/cancel")
            .done(),
        act("run.undo", "Build the reverse diff of a completed write run, for a person's approval.", OPERATOR, No, "9", Change)
            .pos("run_id", TEXT, RUN)
            .writes()
            .done(),
        act("run.trace", "A summary of one run's trace, with references to its kept artifacts.", VIEWER, No, "1", Trace)
            .pos("run_id", TEXT, RUN)
            .done(),
        act("session.status", "Whether each target is signed in for the caller. Shows states and times, never a credential.", VIEWER, Use, "2", Inspect)
            .opt_pos("target", TEXT, TARGET)
            .output(JsonSchema::object()
                .required("sessions", list_of(any_map()))
                .required("complete", JsonSchema::bool()))
            .alias("GET", "/sessions")
            .done(),
        act("session.login", "Ask the caller's person to sign in to one surface of a target. Returns a link for the person; the agent never sees a credential.", VIEWER, Use, "2", Change)
            .pos("target", TEXT, TARGET)
            .opt("surface", TEXT, "The surface to sign in to. The target's first surface that needs a sign-in when left out.")
            .opt("environment", TEXT, "The environment.")
            .writes()
            .done(),
        act("session.revoke", "End one of the caller's own sessions.", VIEWER, No, "2", Change)
            .pos("session_id", TEXT, "The session's identifier.")
            .writes()
            .done(),
        // Building a target.
        act("target.add", "Register a target in the project: makes its folder and a first target file to fill in.", BUILDER, No, "5", Change)
            .pos("name", TEXT, "The new target's name: lowercase letters, digits, and underscores.")
            .opt("address", TEXT, "The address of the system, kept in the target file for a person to review.")
            .writes()
            .needs(HealthNeed::Reads)
            .output(JsonSchema::object()
                .required("target", text())
                .required("folder", text())
                .required("files", list_of(text())))
            .example(r#"{"name": "example"}"#)
            .done(),
        act("target.probe", "The platform, framework, support tier, and surfaces at an address or in a running application. Read only.", BUILDER, Build, "5", Observe)
            .pos("target", TEXT, TARGET)
            .opt("surface", TEXT, "One surface. Every surface when left out.")
            .opt("wait_seconds", FieldType::Int { min: 0, max: 600 }, "How long to wait for each run to end before returning.")
            .long_running()
            .done(),
        act("target.status", "The target's build state and the next action to take.", BUILDER, Build, "5", Inspect)
            .pos("target", TEXT, TARGET)
            .done(),
        act("target.list", "The targets in the project and the versions published from them.", BUILDER, No, "5", List)
            .output(JsonSchema::object()
                .required("targets", list_of(any_map())))
            .done(),
        act("auth.detect", "The sign-in method of a surface, with evidence, and a recommended identity mode. Read only.", BUILDER, Build, "2", Observe)
            .pos("target", TEXT, TARGET)
            .opt("surface", TEXT, "The surface to look at.")
            .opt("wait_seconds", FieldType::Int { min: 0, max: 600 }, "How long to wait for each run to end before returning.")
            .long_running()
            .done(),
        act("screen.snapshot", "A compact snapshot of what a surface shows now, with stable references to its elements.", BUILDER, Build, "1", Observe)
            .pos("target", TEXT, TARGET)
            .opt("surface", TEXT, "The surface to look at.")
            .opt("limit", COUNT, LIMIT)
            .opt("offset", OFFSET, OFFSET_HELP)
            .done(),
        act("capture.start", "Begin recording actions and traffic on a surface, redacted as it is recorded.", BUILDER, Build, "4", Observe)
            .pos("target", TEXT, TARGET)
            .opt("surface", TEXT, "The surface to record.")
            .writes()
            .done(),
        act("capture.stop", "End the recording and store it.", BUILDER, Build, "4", Observe)
            .pos("target", TEXT, TARGET)
            .writes()
            .done(),
        act("endpoints.infer", "Endpoint templates, schemas, and dependencies worked out from stored recordings.", BUILDER, Build, "4", Inspect)
            .pos("target", TEXT, TARGET)
            .done(),
        act("spec.import", "Operations drafted from a description the system publishes about itself, given as a file in the project.", BUILDER, Build, "4", Change)
            .pos("target", TEXT, TARGET)
            .req("file", TEXT, "The description's path inside the project.")
            .writes()
            .done(),
        act("draft.read", "Read one draft file of a target; stands in for a file system.", BUILDER, Build, "5", Inspect)
            .pos("target", TEXT, TARGET)
            .req("file", TEXT, "The file's path inside the target's folder.")
            .done(),
        act("draft.write", "Write one draft file of a target after validating and linting it.", BUILDER, Build, "5", Change)
            .pos("target", TEXT, TARGET)
            .req("file", TEXT, "The file's path inside the target's folder.")
            .req("content", TEXT, "The file's whole new text.")
            .writes()
            .done(),
        act("draft.delete", "Remove one draft file of a target from the project; a published version keeps its own copy.", BUILDER, Build, "5", Change)
            .pos("target", TEXT, TARGET)
            .req("file", TEXT, "The file's path inside the target's folder.")
            .output(JsonSchema::object()
                .required("target", text())
                .required("file", text())
                .required("removed", JsonSchema::bool())
                .required("digest", text())
                .required("ok", JsonSchema::bool())
                .required("problems", list_of(any_map())))
            .writes()
            .example(r#"{"target": "example", "file": "operations/stock_get.yaml"}"#)
            .done(),
        act("draft.lint", "Lint and compile a target's draft files and report every problem with its file and line. Changes nothing.", BUILDER, Build, "1", Inspect)
            .pos("target", TEXT, TARGET)
            .output(JsonSchema::object()
                .required("target", text())
                .required("ok", JsonSchema::bool())
                .required("problems", list_of(any_map()))
                .optional("plan_hash", text())
                .optional("tools", list_of(text())))
            .example(r#"{"target": "example"}"#)
            .done(),
        act("draft.try", "Run one draft binding in the sandbox, where writes go only to declared test records.", BUILDER, Build, "5", Observe)
            .pos("target", TEXT, TARGET)
            .req("tool", TEXT, "The drafted tool's name.")
            .opt("input", MAP, "The tool's own input.")
            .long_running()
            .done(),
        act("test.run", "Run the publish gates on a target's drafts and return the evidence.", BUILDER, Build, "3", Observe)
            .pos("target", TEXT, TARGET)
            .long_running()
            .done(),
        act("fixture.record", "Record an offline test's fixture from one real read, every value scrubbed by rule, and the test that replays it.", BUILDER, No, "3", Change)
            .pos("target", TEXT, TARGET)
            .req("tool", TEXT, "The drafted or published read to record, by name.")
            .opt("input", MAP, "The read's input, as for `tool.call`.")
            .opt("name", TEXT, "The fixture's and the test's name; the tool's name without the target's when left out.")
            .long_running()
            .writes()
            .needs(HealthNeed::Reads)
            .output(JsonSchema::object()
                .required("target", text())
                .required("tool", text())
                .required("fixture", text())
                .required("test", text())
                .required("files", list_of(text()))
                .required("scrubbed", list_of(any_map()))
                .required("test_state", text())
                .required("review", text()))
            .example(r#"{"target": "example", "tool": "example_order_get", "input": {"order_id": "A-100"}}"#)
            .done(),
        act("shadow.report", "How often a shadow binding matched the verified one, and where it differed.", BUILDER, Build, "4", Compare)
            .pos("target", TEXT, TARGET)
            .opt("tool", TEXT, "One tool's name.")
            .done(),
        act("plan.show", "Print a compiled plan as text: its tools, steps, surfaces, and permissions.", BUILDER, No, "1", Inspect)
            .pos("target", TEXT, TARGET)
            .opt("version", TEXT, "A published version. The current one when left out.")
            .done(),
        act("version.diff", "Compare two versions of a target and classify each change.", BUILDER, No, "5", Compare)
            .pos("target", TEXT, TARGET)
            .req("from", TEXT, "The earlier version.")
            .req("to", TEXT, "The later version.")
            .done(),
        act("publish.request", "Submit a target's drafts for a person's review. Publishing itself is a person's act.", BUILDER, Build, "5", Change)
            .pos("target", TEXT, TARGET)
            .opt("version", TEXT, "The version's label. The next number when left out.")
            .writes()
            .output(published())
            .asks_a_person()
            .example(r#"{"target": "example"}"#)
            .done(),
        // Providers and discovery.
        act("provider.list", "Every pack and provider: installed, enabled, ready, or what it lacks and how to get it.", VIEWER, Build, "Stage B", List)
            .needs(HealthNeed::Nothing)
            .output(JsonSchema::object()
                .required("providers", list_of(JsonSchema::object()
                    .required("name", text())
                    .required("version", text())
                    .required("kind", text())
                    .required("availability", text())
                    .required("registered", JsonSchema::bool())
                    .required("fix", text())
                    .required("problems", list_of(any_map())))))
            .example("{}")
            .done(),
        act("provider.describe", "One provider's manifest: what it supplies, its effects, its requirements, and its settings.", VIEWER, No, "Stage B", Inspect)
            .pos("provider", TEXT, "The provider's name.")
            .needs(HealthNeed::Nothing)
            .output(JsonSchema::object()
                .required("name", text())
                .required("version", text())
                .required("kind", text())
                .required("publisher", text())
                .required("availability", text())
                .required("modes", list_of(text()))
                .required("surface_kinds", list_of(text()))
                .required("actions", list_of(text()))
                .required("settings", list_of(any_map()))
                .required("requirements", JsonSchema::int())
                .required("problems", list_of(any_map())))
            .example(r#"{"provider": "example"}"#)
            .done(),
        act("provider.enable", "Switch a provider on. A protected setting on one machine where the provider starts processes or reaches a network.", ADMIN, No, "Stage B", Change)
            .pos("provider", TEXT, "The provider's name.")
            .writes()
            .person(PersonNeed::PresenceWhenLoosening)
            .done(),
        act("provider.disable", "Switch a provider off.", ADMIN, No, "Stage B", Change)
            .pos("provider", TEXT, "The provider's name.")
            .writes()
            .done(),
        act("component.list", "The outside programs providers need: found or not, with path and digest.", VIEWER, No, "Stage B", List)
            .needs(HealthNeed::Nothing)
            .done(),
        act("discover.run", "Run the discoverers that suit a target's surfaces, in the sandbox. Returns findings; never writes.", BUILDER, Build, "Stage E", Observe)
            .pos("target", TEXT, TARGET)
            .opt("surface", TEXT, "One surface. Every surface when left out.")
            .long_running()
            .done(),
        act("finding.list", "Findings for a target, by level, kind, or source.", BUILDER, Build, "Stage E", List)
            .pos("target", TEXT, TARGET)
            .opt("kind", TEXT, "A finding kind.")
            .opt("limit", COUNT, LIMIT)
            .opt("offset", OFFSET, OFFSET_HELP)
            .done(),
        act("finding.describe", "One finding in full, with its evidence references.", BUILDER, Build, "Stage E", Inspect)
            .pos("finding", TEXT, "The finding's identifier.")
            .req("target", TEXT, TARGET)
            .done(),
        act("unknown.list", "The open questions about a target.", BUILDER, Build, "Stage E", List)
            .pos("target", TEXT, TARGET)
            .done(),
        act("unknown.settle", "Settle an open question about a target, with evidence.", BUILDER, No, "Stage E", Change)
            .pos("unknown", TEXT, "The open question's identifier.")
            .req("evidence", TEXT, "What settles it: a finding or a document, by reference.")
            .writes()
            .done(),
        act("scout.run", "Identify a target, run the discoverers that suit it, draft what follows by rule, and try the drafted reads. Resumable; never writes to the system.", BUILDER, Build, "Stage E", Observe)
            .pos("target", TEXT, TARGET)
            .opt("fresh", BOOL, "Start a new scout instead of continuing the last one that did not finish.")
            .opt("wait_seconds", FieldType::Int { min: 0, max: 600 }, "How long to wait for each run to end before returning.")
            .long_running()
            .done(),
        act("scout.report", "The last scout's report on a target: found, drafted, proven, open, and what to look at first.", BUILDER, Build, "Stage E", Inspect)
            .pos("target", TEXT, TARGET)
            .done(),
        // Publishing and versions.
        act("version.publish", "Publish a target's drafts as an immutable version, compiled to a plan. A person's act, never an agent's.", PUBLISHER, No, "5", Change)
            .pos("target", TEXT, TARGET)
            .opt("version", TEXT, "The version's label. The next number when left out.")
            .opt("plan_hash", TEXT, "The hash of the plan the drafts must compile to. A publish shown to a person on a page carries the one they read.")
            .writes()
            .person(PersonNeed::Presence)
            .output(published())
            .example(r#"{"target": "example"}"#)
            .done(),
        act("binding.promote", "Make the first binding of a drafted operation verified, on the evidence of its offline tests and a trial. For an operation that writes, a person's act.", PUBLISHER, No, "4", Change)
            .pos("target", TEXT, TARGET)
            .req("tool", TEXT, "The tool's name.")
            .req("mode", TEXT, "The binding's mode.")
            .opt("run_id", TEXT, "A trial of the read that completed, to show it was proven; the newest scout's proof is used without one.")
            .output(JsonSchema::object()
                .required("target", text())
                .required("tool", text())
                .required("mode", text())
                .required("file", text())
                .required("promoted", JsonSchema::bool())
                .required("status", text())
                .required("evidence", any_map()))
            .writes()
            .person(PersonNeed::PresenceWhenLoosening)
            .example(r#"{"target": "example", "tool": "example_order_get", "mode": "http"}"#)
            .done(),
        act("version.activate", "Move a toolset to a version, through the candidate stage.", PUBLISHER, No, "6", Change)
            .pos("target", TEXT, TARGET)
            .req("version", TEXT, "The version.")
            .opt("toolset", TEXT, "The toolset.")
            .writes()
            .done(),
        act("version.rollback", "Make an earlier version of a target current.", PUBLISHER, No, "5", Change)
            .pos("target", TEXT, TARGET)
            .req("version", TEXT, "The earlier version.")
            .writes()
            .done(),
        act("version.bundle", "Pack a published version into a signed file.", PUBLISHER, No, "6", Change)
            .pos("target", TEXT, TARGET)
            .opt("version", TEXT, "The version. The current one when left out.")
            .done(),
        act("version.import", "Import a signed bundle as a published version.", PUBLISHER, No, "6", Change)
            .pos("file", TEXT, "The bundle's path inside an allowed root.")
            .writes()
            .done(),
        act("version.migrate", "Recompile stored sources for a newer engine.", PUBLISHER, No, "9", Change)
            .pos("target", TEXT, TARGET)
            .writes()
            .done(),
        // Administration.
        act("toolset.create", "Create a toolset: which tools, which version, which identity mode.", ADMIN, No, "6", Change)
            .pos("name", TEXT, "The toolset's name.")
            .req("tools", LIST, "The tools it holds, by name or pattern.")
            .writes()
            .done(),
        act("toolset.update", "Change a toolset.", ADMIN, No, "6", Change)
            .pos("name", TEXT, "The toolset's name.")
            .opt("tools", LIST, "The tools it holds, by name or pattern.")
            .writes()
            .done(),
        act("toolset.list", "The toolsets of the workspace.", ADMIN, No, "6", List)
            .done(),
        act("grant.add", "Grant a principal or a group the use of tools up to an effect class.", ADMIN, No, "6", Change)
            .pos("who", TEXT, "The principal or group.")
            .req("scope", TEXT, "The tools, by name or pattern.")
            .req("max_effect", FieldType::Word(&["read", "write", "irreversible"]), "The highest effect class granted.")
            .writes()
            .done(),
        act("grant.remove", "Remove a grant.", ADMIN, No, "6", Change)
            .pos("who", TEXT, "The principal or group.")
            .req("scope", TEXT, "The tools, by name or pattern.")
            .writes()
            .done(),
        act("grant.list", "The grants of the workspace.", ADMIN, No, "6", List)
            .done(),
        act("policy.publish", "Publish a policy set after its own tests pass.", ADMIN, No, "6", Change)
            .pos("file", TEXT, "The policy set's path inside an allowed root.")
            .writes()
            .done(),
        act("policy.test", "Run a policy set's decision table and report each failure.", ADMIN, No, "6", Inspect)
            .pos("file", TEXT, "The policy set's path inside an allowed root.")
            .done(),
        act("policy.explain", "Why a past call was allowed or denied: the decision, the policy version, and the rules that made it.", AUDITOR, No, "6", Trace)
            .pos("decision", TEXT, "The decision's identifier.")
            .output(JsonSchema::object()
                .required("decision", text())
                .required("answer", text())
                .required("question", text())
                .required("resource", text())
                .required("principal", text())
                .required("summary", text())
                .required("rules", list_of(any_map()))
                .optional("version", text())
                .optional("refusal_code", text()))
            .done(),
        act("session.list", "The sessions of a workspace, without their contents.", ADMIN, No, "6", List)
            .done(),
        act("session.end", "End someone's session. It cannot be used to enter one.", ADMIN, No, "6", Change)
            .pos("session_id", TEXT, "The session's identifier.")
            .writes()
            .done(),
        act("runner.enroll", "Enroll a runner on another machine.", ADMIN, No, "7", Change)
            .pos("name", TEXT, "A name for the runner.")
            .writes()
            .done(),
        act("runner.list", "The runners, what each serves, and when each was last heard from.", ADMIN, No, "7", List)
            .done(),
        act("runner.drain", "Stop giving a runner new jobs.", ADMIN, No, "7", Change)
            .pos("runner", TEXT, "The runner's identifier.")
            .writes()
            .done(),
        act("runner.revoke", "Revoke a runner's enrollment.", ADMIN, No, "7", Change)
            .pos("runner", TEXT, "The runner's identifier.")
            .writes()
            .done(),
        act("killswitch.set", "Stop, or release, a target, a workspace, or the whole system. Releasing needs a person present.", ADMIN, No, "3", Change)
            .pos("scope", FieldType::Word(&["everything", "workspace", "target"]), "What to stop.")
            .req("on", BOOL, "True to stop, false to release.")
            .opt("target", TEXT, "The target, for the scope `target`.")
            .writes()
            .needs(HealthNeed::Reads)
            .person(PersonNeed::PresenceWhenLoosening)
            .output(JsonSchema::object()
                .required("scope", text())
                .required("on", JsonSchema::bool())
                .required("epoch", JsonSchema::int()))
            .done(),
        act("audit.query", "Read the audit log, oldest first, and check its chain.", AUDITOR, No, "6", Trace)
            .opt("limit", COUNT, LIMIT)
            .opt("offset", OFFSET, OFFSET_HELP)
            .output(JsonSchema::object()
                .required("entries", list_of(any_map()))
                .required("chain", text())
                .required("complete", JsonSchema::bool()))
            .done(),
        act("secret.set", "Store a secret under a name, taken from a prompt or standard input. Nothing reads it back.", ADMIN, No, "2", Change)
            .pos("name", TEXT, "The name the secret is stored under: lowercase letters, digits, and underscores.")
            .opt("value", TEXT, "The secret. On a command line an argument is visible to other programs while the command runs: give `--stdin` there instead.")
            .stdin_flag("stdin", "value", "Read the secret from standard input, to its end, without the last line ending. Command line only.")
            .writes()
            .output(JsonSchema::object()
                .required("name", text())
                .required("stored", JsonSchema::bool()))
            .done(),
        act("vault.rotate", "Rotate one layer of the vault's keys.", ADMIN, No, "9", Change)
            .pos("layer", FieldType::Word(&["root", "workspace", "owner"]), "The key layer.")
            .writes()
            .done(),
        act("delegation.grant", "A person lets an agent act for them.", Who::Person, No, "7", Change)
            .pos("agent", TEXT, "The agent's principal.")
            .writes()
            .person(PersonNeed::Presence)
            .done(),
        act("delegation.revoke", "A person withdraws a delegation.", Who::Person, No, "7", Change)
            .pos("agent", TEXT, "The agent's principal.")
            .writes()
            .done(),
        act("delegation.list", "The delegations a person has given.", Who::Person, No, "7", List)
            .done(),
        // The machine itself.
        act("engine.status", "Whether the local engine runs, its health state, and the cause of anything but `ok`.", Who::LocalUser, No, "1", Inspect)
            .needs(HealthNeed::Nothing)
            .output(JsonSchema::object()
                .required("state", text())
                .required("reasons", list_of(any_map()))
                .required("runner", any_map()))
            .example("{}")
            .done(),
        act("engine.start", "Start the local engine.", Who::LocalUser, No, "1", Change)
            .client()
            .done(),
        act("engine.stop", "Stop the local engine after saving its sessions.", Who::LocalUser, No, "1", Change)
            .client()
            .done(),
        act("engine.logs", "The local engine's recent log lines.", Who::LocalUser, No, "1", Trace)
            .opt("lines", COUNT, "How many lines.")
            .client()
            .done(),
        act("config.show", "The effective settings, and for each the layer that set it. With a key, that key's documentation.", Who::LocalUser, No, "1", Inspect)
            .opt_pos("key", TEXT, "One key, such as `policy.max_records_per_run`.")
            .needs(HealthNeed::Nothing)
            .output(JsonSchema::object()
                .required("settings", list_of(JsonSchema::object()
                    .required("key", text())
                    .required("value", JsonSchema::any())
                    .required("layer", text())
                    .required("protected", JsonSchema::bool())
                    .required("locked", JsonSchema::bool())
                    .optional("origin", text())
                    .optional("doc", text())
                    .optional("in_force", JsonSchema::bool())
                    .optional("overridden", list_of(any_map()))))
                .required("presets", list_of(text()))
                .required("digest", text()))
            .example("{}")
            .done(),
        act("config.set", "Change one setting in a settings file. Loosening a protected setting needs a person present.", Who::LocalUser, No, "3", Change)
            .pos("key", TEXT, "The key.")
            .req("value", TEXT, "The new value, as it would be written in the file.")
            .opt("layer", FieldType::Word(&["user", "project"]), "The file to change. The user's when left out.")
            .writes()
            .person(PersonNeed::PresenceWhenLoosening)
            .done(),
        act("doctor.run", "Check the settings, the store, the key store, the providers, and the clock, and give a fix for each problem. Makes no outside connection.", Who::LocalUser, No, "2", Inspect)
            .needs(HealthNeed::Nothing)
            .output(JsonSchema::object()
                .required("state", text())
                .required("checks", list_of(JsonSchema::object()
                    .required("check", text())
                    .required("state", text())
                    .required("detail", text())
                    .required("fix", text()))))
            .example("{}")
            .done(),
        act("browser.install", "Fetch a pinned browser for the pack that drives one.", Who::LocalUser, No, "1", Change)
            .writes()
            .done(),
        act("plugin.install", "Install a plug-in from a file, after checking its manifest and signature.", ADMIN, No, "8", Change)
            .pos("file", TEXT, "The plug-in's path inside an allowed root.")
            .writes()
            .done(),
        act("plugin.list", "The plug-ins found on this machine.", ADMIN, No, "8", List)
            .needs(HealthNeed::Nothing)
            .done(),
        act("client.connect", "Write the configuration an MCP client needs to reach this engine.", Who::LocalUser, No, "5", Change)
            .pos("client", TEXT, "The client's profile name.")
            .writes()
            .done(),
        act("trace.show", "View one run's evidence: its trace lines and artifact references.", AUDITOR, No, "1", Trace)
            .pos("run_id", TEXT, RUN)
            .output(JsonSchema::object()
                .required("run_id", text())
                .required("events", list_of(any_map()))
                .optional("trace", text())
                .optional("trace_check", text())
                .optional("signature", text())
                .optional("records", JsonSchema::int_between(0, i64::from(u32::MAX))))
            .done(),
        act("trace.export", "Export one run's evidence to a file in the project.", AUDITOR, No, "1", Trace)
            .pos("run_id", TEXT, RUN)
            .done(),
        act("action.search", "Find actions by words. Returns names and one-line summaries only.", Who::Any, UseAndBuild, "5", List)
            .opt_pos("query", TEXT, "Words to look for. Leave out to list every action.")
            .needs(HealthNeed::Nothing)
            .output(JsonSchema::object()
                .required("actions", list_of(JsonSchema::object()
                    .required("name", text())
                    .required("summary", text())
                    .required("built", JsonSchema::bool())))
                .required("total", JsonSchema::int()))
            .example(r#"{"query": "publish"}"#)
            .done(),
        act("action.describe", "One action in full: its summary, input schema, output schema, examples, effect, and role.", Who::Any, UseAndBuild, "5", Inspect)
            .pos("action", TEXT, "The action's name, such as `draft.lint`.")
            .needs(HealthNeed::Nothing)
            .output(any_map())
            .example(r#"{"action": "draft.lint"}"#)
            .done(),
        act("action.run", "Run any action by name with its input. The action's own role, schema, and policy decision apply.", Who::Any, UseAndBuild, "5", Change)
            .pos("action", TEXT, "The action's name.")
            .opt("input", MAP, "The action's own input.")
            .needs(HealthNeed::Nothing)
            .output(any_map())
            .example(r#"{"action": "draft.lint", "input": {"target": "example"}}"#)
            .done(),
        act("docs.get", "A built-in reference page, by topic or action name. With no topic, the list of topics.", Who::Any, No, "5", Inspect)
            .opt_pos("topic", TEXT, "A topic, or an action's name.")
            .needs(HealthNeed::Nothing)
            .output(JsonSchema::object()
                .required("topic", text())
                .required("text", text())
                .required("topics", list_of(text())))
            .example(r#"{"topic": "draft.lint"}"#)
            .done(),
        act("self.update", "Update a standalone install to the newest release of its channel.", Who::LocalUser, No, "9", Change)
            .client()
            .done(),
    ]
}
