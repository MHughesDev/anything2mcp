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
const POSITION: FieldType = FieldType::Int { min: 1, max: 64 };
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

/// The lists a bound cut in a reading of the machine's network, as
/// `a2m_netstate::Cut` is shown.
fn net_cuts() -> JsonSchema {
    list_of(
        JsonSchema::object()
            .required("list", text())
            .required("kept", JsonSchema::int())
            .required("seen", JsonSchema::int())
            .required("table_cut", JsonSchema::bool()),
    )
}

/// Why a field of a network reading is left out: the words of
/// `a2m_netstate::AbsentReason`.
fn net_absent_reason() -> JsonSchema {
    JsonSchema::words(&[
        "not_stated",
        "needs_location_permission",
        "not_visible_to_this_user",
        "not_read",
        "may_hold_credentials",
    ])
}

/// The fields a network reading leaves out, and why.
fn net_absent() -> JsonSchema {
    list_of(
        JsonSchema::object()
            .required("field", text())
            .required("reason", net_absent_reason()),
    )
}

/// A hardware address as a network reading shows it: its length and
/// whether it is locally administered, never its octets (decision 0243).
fn net_hardware() -> JsonSchema {
    JsonSchema::object()
        .required("length", JsonSchema::int())
        .required("locally_administered", JsonSchema::bool())
}

fn net_family() -> JsonSchema {
    JsonSchema::words(&["ipv4", "ipv6"])
}

fn net_kind() -> JsonSchema {
    JsonSchema::words(&[
        "loopback", "wired", "wireless", "virtual", "tunnel", "other",
    ])
}

fn net_link() -> JsonSchema {
    JsonSchema::words(&["up", "down"])
}

fn net_prefix() -> JsonSchema {
    JsonSchema::int_between(0, 128)
}

/// One address of an interface, and its prefix length where the system
/// states one.
fn net_address() -> JsonSchema {
    JsonSchema::object()
        .required("address", text())
        .optional("prefix_length", net_prefix())
}

fn net_route() -> JsonSchema {
    JsonSchema::object()
        .required("family", net_family())
        .required("destination", text())
        .required("prefix_length", net_prefix())
        .optional("gateway", text())
        .optional("interface_index", JsonSchema::int())
        .optional("interface", text())
        .optional("metric", JsonSchema::int())
}

/// What kind of place an address is: the words of `a2m_core::AddressScope`
/// (decision 0240).
fn net_scope() -> JsonSchema {
    JsonSchema::words(a2m_core::AddressScope::NAMES)
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
        act("session.login", "Ask the caller's person to sign in to one surface of a target. A credential is never an input: where one is not stored, the person types it on the engine's own page, and carries the sign-in out there. Returns that a session exists, for whom, where, until when, and at which level; never its contents.", VIEWER, Use, "2", Change)
            .pos("target", TEXT, TARGET)
            .opt("surface", TEXT, "The surface to sign in to. The target's first surface that declares a sign-in when left out.")
            .opt("environment", TEXT, "The environment. The target's default when left out.")
            .writes()
            .output(JsonSchema::object()
                .required("state", JsonSchema::words(&["signed_in", "signing_in", "not_needed"]))
                .required("target", text())
                .optional("session", text())
                .optional("surface", text())
                .optional("environment", text())
                .optional("for", text())
                .optional("provider", text())
                .optional("session_state", text())
                .optional("signed_in_at", JsonSchema::int())
                .optional("expires_at", JsonSchema::int())
                .optional("level", JsonSchema::words(&["confirmed", "held"]))
                .optional("why", text()))
            .asks_a_person()
            .example(r#"{"target": "example"}"#)
            .done(),
        act("session.revoke", "End one of the caller's own sessions.", VIEWER, No, "2", Change)
            .pos("session_id", TEXT, "The session's identifier.")
            .writes()
            .output(JsonSchema::object()
                .required("session", text())
                .required("target", text())
                .required("environment", text())
                .required("state", text()))
            .done(),
        // Building a target.
        act("target.add", "Register a target in the project: makes its folder and a first target file, with one surface where an address and its kind are given.", BUILDER, No, "5", Change)
            .pos("name", TEXT, "The new target's name: lowercase letters, digits, and underscores.")
            .opt("address", TEXT, "Where the system is reached, in the form the surface's kind takes. Given with `kind`, the target file declares one surface of that kind at this address, and a test environment.")
            .opt("kind", TEXT, "The kind of the surface at `address`, as an enabled pack registers it. Never guessed from the address: left out with an address, the refusal lists the kinds offered.")
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
        act("draft.delete", "Remove draft files of a target from the project, one by `file` or several by `files`, all or none; a published version keeps its own copy.", BUILDER, Build, "5", Change)
            .pos("target", TEXT, TARGET)
            .opt("file", TEXT, "One file's path inside the target's folder.")
            .opt("files", LIST, "Several files' paths inside the target's folder, in place of `file`: each is checked as one is, and none is removed unless all can be.")
            .output(JsonSchema::object()
                .required("target", text())
                .optional("file", text())
                .required("files", list_of(JsonSchema::object()
                    .required("file", text())
                    .required("digest", text())))
                .required("removed", JsonSchema::bool())
                .optional("digest", text())
                .required("ok", JsonSchema::bool())
                .required("problems", list_of(any_map())))
            .writes()
            .example(r#"{"target": "example", "file": "operations/stock_get.yaml"}"#)
            .example(r#"{"target": "example", "files": ["operations/stock_get.yaml", "tests/stock_get.yaml"]}"#)
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
            .opt("mode", TEXT, "The binding to try, by its mode; the one that would act live when left out.")
            .opt("position", POSITION, "The binding's place among the operation's bindings, from 1, where two share the mode.")
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
            .opt("mode", TEXT, "The binding to record through, by its mode; the one that would act live when left out. A binding named is named in the test written.")
            .opt("position", POSITION, "The binding's place among the operation's bindings, from 1, where two share the mode.")
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
                .optional("binding", any_map())
                .optional("trial", any_map())
                .optional("limits", list_of(text()))
                .required("review", text()))
            .example(r#"{"target": "example", "tool": "example_order_get", "input": {"order_id": "A-100"}}"#)
            .done(),
        act("shadow.report", "How often a shadow binding matched the verified one, and where it differed.", BUILDER, Build, "4", Compare)
            .pos("target", TEXT, TARGET)
            .opt("tool", TEXT, "One tool's name.")
            .output(JsonSchema::object()
                .required("target", text())
                .required("bindings", list_of(JsonSchema::object()
                    .required("operation", text())
                    .required("mode", text())
                    .required("binding", text())
                    .required("matches", JsonSchema::int())
                    .required("mismatches", JsonSchema::int())
                    .required("not_compared", JsonSchema::int())
                    .required("in_a_row", JsonSchema::int())
                    .required("distinct_inputs", JsonSchema::int())
                    .required("evidence_from", list_of(text()))
                    .required("requests", JsonSchema::int())
                    .required("against_verified_now", JsonSchema::bool())
                    .required("environments", list_of(any_map()))
                    .required("threshold", JsonSchema::int())
                    .required("promotable", JsonSchema::bool())
                    .optional("not_promotable", text())
                    .required("first_at", JsonSchema::int())
                    .required("last_at", JsonSchema::int())
                    .optional("last_mismatch", any_map())
                    .optional("promoted", any_map()))))
            .example(r#"{"target": "example", "tool": "example_order_get"}"#)
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
        act("binding.promote", "Make the first binding of a drafted operation verified, on the evidence of its offline tests and a trial, or a binding in shadow verified, on its run of exact matches. For an operation that writes, and for a binding in shadow, a person's act.", PUBLISHER, No, "4", Change)
            .pos("target", TEXT, TARGET)
            .req("tool", TEXT, "The tool's name.")
            .req("mode", TEXT, "The binding's mode.")
            .opt("position", POSITION, "The binding's place among the operation's bindings, from 1, where two share the mode.")
            .opt("run_id", TEXT, "A trial of the read that completed, to show it was proven; the newest scout's proof is used without one.")
            .output(JsonSchema::object()
                .required("target", text())
                .required("tool", text())
                .required("mode", text())
                .optional("position", JsonSchema::int())
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
            .opt_pos("target", TEXT, TARGET)
            .output(JsonSchema::object()
                .required("sessions", list_of(any_map()))
                .required("complete", JsonSchema::bool()))
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
        // What this machine already knows of its own networks (decisions
        // 0222, 0243, 0260). Each is a read for the local user: on one
        // machine an agent acting for that person may call it. The default
        // health need `Reads` keeps a recorded decision on every call.
        act("net.interfaces", "This machine's network interfaces and their addresses, read from the operating system. Hardware addresses are shown as their length only. Sends nothing.", Who::LocalUser, No, "2", Inspect)
            .output(JsonSchema::object()
                .required("interfaces", list_of(JsonSchema::object()
                    .required("index", JsonSchema::int())
                    .required("name", text())
                    .optional("kind", net_kind())
                    .optional("system_kind", text())
                    .optional("state", net_link())
                    .optional("mtu", JsonSchema::int())
                    .optional("hardware_address", net_hardware())
                    .required("addresses", list_of(net_address()))
                    .optional("absent", net_absent())))
                .required("read_from", list_of(text()))
                .optional("cut", net_cuts()))
            .example("{}")
            .done(),
        act("net.routes", "This machine's routes, and each family's default routes, lowest metric first. Sends nothing.", Who::LocalUser, No, "2", Inspect)
            .output(JsonSchema::object()
                .required("routes", list_of(net_route()))
                .required("default_routes", list_of(net_route()))
                .required("read_from", list_of(text()))
                .optional("cut", net_cuts()))
            .example("{}")
            .done(),
        act("net.resolvers", "The name servers and search domains this machine uses, as the system states them. Sends nothing.", Who::LocalUser, No, "2", Inspect)
            .output(JsonSchema::object()
                .required("settings", list_of(JsonSchema::object()
                    .required("from", text())
                    .required("name_servers", list_of(text()))
                    .required("search_domains", list_of(text()))))
                .required("read_from", list_of(text()))
                .optional("absent", net_absent())
                .optional("cut", net_cuts()))
            .example("{}")
            .done(),
        act("net.neighbours", "The neighbour table this machine already holds, never refreshed by asking. Hardware addresses are shown as their length only. Sends nothing.", Who::LocalUser, No, "2", Inspect)
            .output(JsonSchema::object()
                .required("neighbours", list_of(JsonSchema::object()
                    .required("address", text())
                    .optional("interface_index", JsonSchema::int())
                    .optional("interface", text())
                    .optional("hardware_address", net_hardware())
                    .optional("state", JsonSchema::words(&[
                        "incomplete", "reachable", "stale", "delay", "probe", "failed", "no_arp", "permanent", "unreachable",
                    ]))))
                .required("read_from", list_of(text()))
                .optional("absent", net_absent())
                .optional("cut", net_cuts()))
            .example("{}")
            .done(),
        act("net.listeners", "The sockets listening on this machine, each with the kind of place its address is, and the program that holds it where this user may see it. Sends nothing.", Who::LocalUser, No, "2", Inspect)
            .output(JsonSchema::object()
                .required("listeners", list_of(JsonSchema::object()
                    .required("transport", JsonSchema::words(&["tcp", "udp"]))
                    .required("address", text())
                    .required("port", JsonSchema::int_between(0, 65_535))
                    .required("scope", net_scope())
                    .optional("owner", JsonSchema::object()
                        .required("process_id", JsonSchema::int())
                        .optional("program", text()))
                    .optional("owner_absent", net_absent_reason())))
                .required("read_from", list_of(text()))
                .optional("cut", net_cuts()))
            .example("{}")
            .done(),
        act("net.proxy", "The proxy setting this engine reads, with any credentials removed: on Linux this engine's own environment, not the caller's; on Windows the user's system setting. Read to be shown, never applied. Sends nothing.", Who::LocalUser, No, "2", Inspect)
            .output(JsonSchema::object()
                .required("settings", list_of(JsonSchema::object()
                    .required("from", text())
                    .required("applies_to", text())
                    .required("value", text())
                    .required("credentials_removed", JsonSchema::bool())))
                .optional("detects_automatically", JsonSchema::bool())
                .required("read_from", list_of(text()))
                .optional("absent", net_absent())
                .optional("cut", net_cuts()))
            .example("{}")
            .done(),
        act("net.standing", "Where this machine stands on its networks: for each interface, the kind of place each of its addresses is, and which interface carries each family's preferred default route. Sends nothing.", Who::LocalUser, Use, "2", Inspect)
            .output(JsonSchema::object()
                .required("interfaces", list_of(JsonSchema::object()
                    .required("name", text())
                    .optional("kind", net_kind())
                    .optional("state", net_link())
                    .required("default_route_for", list_of(net_family()))
                    .required("addresses", list_of(net_address()
                        .required("scope", net_scope())))))
                .required("default_routes", list_of(net_route()))
                .optional("cut", net_cuts()))
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
        act("client.connect", "Give the configuration an MCP client needs to start the bridge for this project: printed for the `generic` profile, or set in the file a client profile names, shown first and written by a local person.", Who::LocalUser, No, "5", Change)
            .opt_pos("client", TEXT, "A client profile's name, kept as data in the `clients` folder of the settings folder. `generic`, the default, prints the block and writes no file.")
            .opt("print", BOOL, "Print the block, and write no file, whatever the profile.")
            .opt("yes", BOOL, "Write the change the command showed. Without it nothing is written.")
            .opt("name", TEXT, "The server entry's name. `anything2mcp` when left out.")
            .opt("toolset", FieldType::Word(&["use", "build"]), "The built-in toolset the bridge serves. `use` when left out.")
            .output(JsonSchema::object()
                .required("client", text())
                .required("name", text())
                .required("entry", any_map())
                .required("block", any_map())
                .optional("file", text())
                .optional("before", JsonSchema::any())
                .optional("change", text())
                .required("written", JsonSchema::bool()))
            .writes()
            .example(r#"{"client": "generic"}"#)
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
