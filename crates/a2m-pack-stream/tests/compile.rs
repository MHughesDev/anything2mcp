//! The pack's registration and its file kinds: a registry that holds only
//! the stream pack, the example target compiled through it, and what the
//! compiler refuses.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use a2m_core::{
    AccessKind, EffectClass, Hazard, Mode, RecordMatching, SurfaceKind, Values, WriteEvidence,
};
use a2m_provider::{Manifest, ProviderName, ProviderSettings};
use common::{Folder, Planted, compile, compiled_target, edited, registry, registry_with};

fn mode() -> Mode {
    Mode::new("stream").unwrap()
}

#[test]
fn a_registry_with_only_the_stream_pack_builds_and_its_digest_is_stable() {
    let folder = Folder::new("registry");
    let first = registry(&folder);
    let second = registry(&folder);
    assert_eq!(first.digest(), second.digest());
    assert_eq!(first.access_kind(&mode()), Some(AccessKind::Stream));
    let handle = first.driver(&mode()).unwrap();
    assert_eq!(
        handle.choice().provider,
        ProviderName::new("stream").unwrap()
    );
    let broker = first
        .surface_kind(&SurfaceKind::new("broker").unwrap())
        .unwrap();
    assert!(broker.session);
    let set = first.compiler_set();
    assert_eq!(set.modes(), vec![&mode()]);
    let verbs: Vec<&str> = set
        .verbs(&mode())
        .iter()
        .map(|verb| verb.as_str())
        .collect();
    assert_eq!(verbs, ["await", "list", "publish", "read", "take"]);
    assert_eq!(set.file_kinds().count(), 2);
    assert_eq!(set.drafting_rules().count(), 3);

    // A setting of the pack's own group changes the digest.
    let mut settings = ProviderSettings::default();
    settings.packs.insert(
        ProviderName::new("stream").unwrap(),
        Values::new().with("wait_ceiling_seconds", 5_i64),
    );
    let tighter = registry_with(&folder, settings, Arc::new(Planted));
    assert_ne!(tighter.digest(), first.digest());
}

#[test]
fn the_manifest_declares_every_code_the_driver_returns_and_no_other() {
    let manifest = Manifest::parse(a2m_pack_stream::MANIFEST).unwrap();
    let declared: BTreeSet<(String, String)> = manifest
        .failure_codes
        .iter()
        .map(|code| (code.class.to_string(), code.code.to_string()))
        .collect();
    // The codes the provider contract and the session keeper define are
    // returned without being declared again.
    let contract = a2m_pack_stream::codes::CONTRACT_CODES;
    let returned: BTreeSet<(String, String)> = a2m_pack_stream::codes::ALL
        .iter()
        .filter(|(_, code)| !contract.contains(code))
        .map(|(class, code)| (class.to_string(), (*code).to_owned()))
        .chain([("input".to_owned(), "payload_unreadable".to_owned())])
        .collect();
    assert_eq!(declared, returned);
    assert_eq!(
        manifest.provider.version.to_string(),
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn the_example_target_compiles_twice_to_the_same_bytes() {
    let folder = Folder::new("compile");
    let registry = registry(&folder);
    let first = compiled_target(&registry).encode().unwrap();
    let second = compiled_target(&registry).encode().unwrap();
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.hash, second.hash);

    let compiled_target = compiled_target(&registry);
    let operation = |name: &str| {
        compiled_target
            .operations
            .iter()
            .find(|operation| operation.name.as_str() == name)
            .unwrap()
    };
    let kinds = |steps: &[a2m_core::Step]| -> Vec<String> {
        steps.iter().map(|step| step.kind.to_string()).collect()
    };

    // A publish to a retained state channel nothing outside reads is an
    // ordinary write, read back by the latest value.
    let set = operation("example_order_status_set");
    assert_eq!((set.effect, set.hazard), (EffectClass::Write, Hazard::None));
    assert_eq!(set.identity.strategy, RecordMatching::Natural);
    let binding = &set.bindings[0];
    assert_eq!(kinds(&binding.act), ["publish"]);
    assert!(binding.act[0].commits && !binding.act[0].idempotent);
    assert_eq!(kinds(&binding.read_back), ["latest"]);
    assert!(binding.read_back[0].lookup && !binding.read_back[0].commits);
    assert_eq!(binding.evidence, WriteEvidence::ReadBack);

    // A publish to a log is irreversible, and is found again by its stamp.
    let record = operation("example_order_event_record");
    assert_eq!(
        (record.effect, record.hazard, record.identity.strategy),
        (
            EffectClass::Irreversible,
            Hazard::SendsOutside,
            RecordMatching::Stamped
        )
    );
    assert_eq!(kinds(&record.bindings[0].read_back), ["scan"]);
    assert_eq!(kinds(&record.bindings[0].lookup), ["scan"]);
    // Where the log ends is read, and recorded, before the publish is sent:
    // a scan of a resumed run starts there. A keyed publish to a log is
    // read back by that scan for its token, so its evidence is the read.
    assert_eq!(kinds(&record.bindings[0].act), ["position", "publish"]);
    let commits: Vec<bool> = record.bindings[0]
        .act
        .iter()
        .map(|step| step.commits)
        .collect();
    assert_eq!(commits, [false, true]);
    assert_eq!(record.bindings[0].evidence, WriteEvidence::ReadBack);

    // A queue keeps nothing to read back; the broker refuses a repeat.
    // Its evidence is the broker's confirmation alone, and the operation is
    // irreversible; so is a notice to a topic, which keeps nothing.
    let add = operation("example_order_work_add");
    assert!(add.bindings[0].read_back.is_empty());
    assert!(add.bindings[0].act[0].idempotent);
    assert_eq!(add.bindings[0].evidence, WriteEvidence::Acknowledged);
    assert_eq!(add.effect, EffectClass::Irreversible);
    let notice = operation("example_order_notice_send");
    assert!(notice.bindings[0].read_back.is_empty());
    assert_eq!(notice.bindings[0].evidence, WriteEvidence::Acknowledged);
    assert_eq!(notice.effect, EffectClass::Irreversible);

    // A wait is two steps, and its limit is in the compiled target.
    let wait = operation("example_order_event_await");
    assert_eq!(kinds(&wait.bindings[0].act), ["subscribe", "await"]);
    assert!(wait.bindings[0].act[1].timeout_ms > 1_000);

    // A take acknowledges last, and only that step commits.
    let take = operation("example_order_work_take");
    assert_eq!(
        kinds(&take.bindings[0].act),
        ["subscribe", "await", "acknowledge"]
    );
    let commits: Vec<bool> = take.bindings[0]
        .act
        .iter()
        .map(|step| step.commits)
        .collect();
    assert_eq!(commits, [false, false, true]);
    assert_eq!(take.commit_points, 1);
    // Its evidence is the acknowledgement. Its one read is a look at the
    // queue for the message it recorded, which settles a take in doubt.
    assert_eq!(take.bindings[0].evidence, WriteEvidence::Acknowledged);
    assert_eq!(take.effect, EffectClass::Irreversible);
    assert_eq!(kinds(&take.bindings[0].read_back), ["taken"]);
    assert!(take.bindings[0].read_back[0].lookup && !take.bindings[0].read_back[0].commits);

    // Every step names its channel by reference, never by an argument.
    for operation in &compiled_target.operations {
        for binding in &operation.bindings {
            for step in binding
                .act
                .iter()
                .chain(&binding.read_back)
                .chain(&binding.lookup)
            {
                assert!(step.refers_to.is_some(), "{}", operation.name);
                assert!(step.args.iter().all(|arg| arg.name != "channel"));
            }
        }
    }
}

/// Compiles the example target with one file changed and gives the first
/// problem with the code.
fn refused(file: &str, text: &str, code: &str) -> a2m_files::Diagnostic {
    let folder = Folder::new("refused");
    let registry = registry(&folder);
    let root = edited(&folder, file, text);
    let problems = compile(&registry, &root).expect_err("the edited target compiles");
    let found = problems
        .find(code)
        .unwrap_or_else(|| panic!("no `{code}` among:\n{problems}"))
        .clone();
    assert_eq!(found.at.file.as_str(), file, "{found}");
    assert!(found.at.line >= 1 && !found.fix.is_empty(), "{found}");
    found
}

const NOTICE: &str = "operation: example_extra\neffect: read\ninputs: {}\noutputs: {}\ndescription: An extra operation.\nbindings:\n";

#[test]
fn an_unknown_key_or_word_in_a_channel_file_is_refused_with_a_place_and_a_fix() {
    let channel = "channel: extra.one\naddress: example.extra\nshape: log\nrole: events\n";
    let found = refused(
        "channels/extra.yaml",
        &format!("{channel}partitions: 3\n"),
        "kernel.unknown_key",
    );
    assert_eq!(found.at.line, 5);
    let found = refused(
        "channels/extra.yaml",
        "channel: extra.one\naddress: example.extra\nshape: stack\nrole: events\n",
        "kernel.unknown_value",
    );
    assert_eq!(found.at.line, 3);
    // A retained channel with no key, and a wait above the ceiling.
    refused(
        "channels/extra.yaml",
        "channel: extra.one\naddress: example.extra\nshape: retained\nrole: state\n",
        "stream.channel_invalid",
    );
    refused(
        "channels/extra.yaml",
        &format!("{channel}wait: {{max: 10m}}\n"),
        "stream.channel_invalid",
    );
    // A schema nobody defines.
    refused(
        "channels/extra.yaml",
        &format!("{channel}schema: missing\n"),
        "stream.unknown_schema",
    );
}

#[test]
fn a_binding_the_channel_cannot_carry_is_refused() {
    let bind = |rest: &str| format!("{NOTICE}  - mode: stream\n{rest}");
    // A channel that no file defines.
    refused(
        "operations/extra.yaml",
        &bind("    verb: list\n    channel: orders.nowhere\n"),
        "stream.unknown_channel",
    );
    // `read` is the latest value for a key: not on a log.
    refused(
        "operations/extra.yaml",
        &bind("    verb: read\n    channel: orders.events\n    key: {order_id: '\"A-100\"'}\n"),
        "stream.verb_needs_shape",
    );
    // A topic cannot be looked at.
    refused(
        "operations/extra.yaml",
        &bind("    verb: list\n    channel: orders.notices\n"),
        "stream.verb_needs_shape",
    );
    // A wait above the channel's longest.
    refused(
        "operations/extra.yaml",
        &bind("    verb: await\n    channel: orders.events\n    wait: 30s\n"),
        "stream.wait_unbounded",
    );
    // Waiting on a queue would take what others expect.
    refused(
        "operations/extra.yaml",
        &bind("    verb: await\n    channel: orders.work\n"),
        "stream.verb_needs_shape",
    );
    // A field the verb cannot use: no free text reaches a look.
    refused(
        "operations/extra.yaml",
        &bind("    verb: list\n    channel: orders.status\n    body: {x: '\"y\"'}\n"),
        "stream.binding_invalid",
    );
    // A key the binding's mode does not know.
    refused(
        "operations/extra.yaml",
        &bind("    verb: list\n    channel: orders.status\n    topic: other\n"),
        "kernel.unknown_key",
    );
}

#[test]
fn a_publish_must_declare_what_its_channel_makes_of_it() {
    let publish = |head: &str, channel: &str| {
        format!(
            "operation: example_extra\n{head}inputs:\n  order_id: {{type: identifier}}\noutputs: {{}}\ndescription: An extra operation.\nbindings:\n  - mode: stream\n    verb: publish\n    channel: {channel}\n    key: {{order_id: input.order_id}}\n    body: {{order_id: input.order_id}}\n"
        )
    };
    // To a log others read: not an ordinary write, and not without the hazard.
    refused(
        "operations/extra.yaml",
        &publish("effect: irreversible\n", "orders.events"),
        "stream.publish_effect",
    );
    // To a queue: a natural key finds nothing again.
    refused(
        "operations/extra.yaml",
        &publish(
            "effect: irreversible\nhazard: sends_outside\nkey: order_id\n",
            "orders.work",
        ),
        "stream.publish_effect",
    );
    // To a queue as an ordinary write: its only evidence is the broker's
    // confirmation. The pack refuses it before the kernel's own rule, that
    // an acknowledged write is irreversible, is reached.
    refused(
        "operations/extra.yaml",
        &publish("effect: write\n", "orders.work"),
        "stream.publish_effect",
    );
    // The token's header belongs to the ledger.
    let folder = Folder::new("token");
    let registry = registry(&folder);
    let text = publish(
        "effect: irreversible\nhazard: sends_outside\n",
        "orders.work",
    ) + "    headers: {idempotency-token: input.order_id}\n";
    let root = edited(&folder, "operations/extra.yaml", &text);
    let problems = compile(&registry, &root).expect_err("a caller's token compiles");
    assert!(
        problems.find("stream.binding_invalid").is_some(),
        "{problems}"
    );
}

#[test]
fn a_take_compiles_only_on_a_queue_the_runtime_owns() {
    let take = "operation: example_extra\neffect: irreversible\ninputs: {}\noutputs: {}\ndescription: An extra operation.\nbindings:\n  - mode: stream\n    verb: take\n    channel: orders.events\n";
    refused("operations/extra.yaml", take, "stream.ack_not_owned");

    let folder = Folder::new("shared");
    let registry = registry(&folder);
    let shared = std::fs::read_to_string(
        common::fixture_root()
            .path()
            .join("channels")
            .join("orders.yaml"),
    )
    .unwrap()
    .replace("ownership: runtime_owned", "ownership: shared");
    let root = edited(&folder, "channels/orders.yaml", &shared);
    let problems = compile(&registry, &root).expect_err("a take on a shared queue compiles");
    let found = problems
        .find("stream.ack_not_owned")
        .expect("the rule's own code");
    assert_eq!(found.at.file.as_str(), "operations/order_work.yaml");
}

#[test]
fn the_settings_ceiling_bounds_every_wait() {
    let folder = Folder::new("ceiling");
    let mut settings = ProviderSettings::default();
    settings.packs.insert(
        ProviderName::new("stream").unwrap(),
        Values::new().with("wait_ceiling_seconds", 5_i64),
    );
    let registry = registry_with(&folder, settings, Arc::new(Planted));
    // The example channels allow waits of ten seconds: above this ceiling.
    let problems = compile(&registry, &common::fixture_root()).expect_err("over the ceiling");
    let found = problems.find("stream.channel_invalid").unwrap();
    assert!(found.message.contains("ceiling"), "{found}");
}

#[test]
fn text_a_broker_returns_is_an_output_only_as_untrusted_text() {
    let read = |outputs: &str, channel: &str| {
        format!(
            "operation: example_extra\neffect: read\ninputs: {{}}\noutputs: {outputs}\ndescription: An extra operation.\nbindings:\n  - mode: stream\n    verb: list\n    channel: {channel}\n"
        )
    };
    // The queue names no schema: a task is whatever a producer wrote.
    let found = refused(
        "operations/extra.yaml",
        &read("{task: {type: text}}", "orders.work"),
        "stream.untrusted_unmarked",
    );
    assert!(found.message.contains("`task`"), "{found}");
    // A field the schema does not hold to listed texts is free text too.
    refused(
        "operations/extra.yaml",
        &read("{order_id: {type: text}}", "orders.status"),
        "stream.untrusted_unmarked",
    );
    // Marked untrusted, or held to an `enum` by the channel's schema, the
    // same outputs compile.
    for (outputs, channel) in [
        ("{task: {type: broker_text}}", "orders.work"),
        ("{status: {type: text}}", "orders.status"),
    ] {
        let folder = Folder::new("marked");
        let registry = registry(&folder);
        let root = edited(&folder, "operations/extra.yaml", &read(outputs, channel));
        if let Err(problems) = compile(&registry, &root) {
            panic!("{outputs} on {channel} does not compile:\n{problems}");
        }
    }
}

/// Decision 0315: an output is certain only where the pack knows every
/// result holds it: a body field the channel's schema requires, on a verb
/// that returns the message, and a list's count. Anything else, a field a
/// message may lack, a place the broker may not give, or what a wait gives,
/// may be absent, so a `null` there does not stop the read.
#[test]
fn an_output_is_certain_only_where_the_channels_schema_or_the_verb_makes_it_so() {
    let registry_with_folder = registry;
    let folder = Folder::new("absent");
    let compiled_target = compiled_target(&registry_with_folder(&folder));
    let absent = |operation: &str, output: &str| {
        compiled_target
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap()
            .outputs
            .iter()
            .find(|found| found.name == output)
            .unwrap()
            .may_be_absent
    };
    // Required by the schema, read from the message: certain.
    assert!(!absent("example_order_status_get", "status"));
    // A list's count is always there.
    assert!(!absent("example_order_status_list", "count"));
    // The place a log gives a publish, and what a wait gives, may be absent.
    assert!(absent("example_order_event_record", "position"));
    assert!(absent("example_order_status_await", "arrived"));

    // A field the schema does not require, and one on a channel with no
    // schema, may be absent.
    let read = |outputs: &str, channel: &str| {
        format!(
            "operation: example_extra\neffect: read\ninputs: {{}}\noutputs: {outputs}\ndescription: An extra operation.\nbindings:\n  - mode: stream\n    verb: list\n    channel: {channel}\n"
        )
    };
    for (outputs, channel, name) in [
        (
            "{task: {type: broker_text, list: true}}",
            "orders.work",
            "task",
        ),
        (
            "{note: {type: broker_text, list: true}}",
            "orders.status",
            "note",
        ),
    ] {
        let folder = Folder::new("absent-extra");
        let extra_registry = registry_with_folder(&folder);
        let root = edited(&folder, "operations/extra.yaml", &read(outputs, channel));
        if name == "note" {
            let schemas = root.path().join("schemas").join("orders.yaml");
            let text = std::fs::read_to_string(&schemas).unwrap();
            std::fs::write(
                &schemas,
                text.replacen(
                    "  status: {type: string, enum: [draft, submitted, shipped]}\n",
                    "  status: {type: string, enum: [draft, submitted, shipped]}\n  note: {type: string}\n",
                    1,
                ),
            )
            .unwrap();
        }
        let compiled =
            compile(&extra_registry, &root).unwrap_or_else(|problems| panic!("{problems}"));
        let extra = compiled
            .compiled_target
            .operations
            .iter()
            .find(|found| found.name.as_str() == "example_extra")
            .unwrap();
        assert!(extra.outputs[0].may_be_absent, "{name}");
    }
}

#[test]
fn a_body_may_not_use_the_name_of_the_brokers_own_facts() {
    refused(
        "operations/extra.yaml",
        "operation: example_extra\neffect: irreversible\nhazard: sends_outside\ninputs: {}\noutputs: {}\ndescription: An extra operation.\nbindings:\n  - mode: stream\n    verb: publish\n    channel: orders.notices\n    body: {message_meta: '\"mine\"'}\n",
        "stream.binding_invalid",
    );
}

#[test]
fn a_write_never_compiles_as_a_read_whatever_the_file_says() {
    // A publish that says `effect: read` is refused by what the pack says
    // the verb does. (A take is held to `irreversible` by the pack's own
    // rule, `stream.publish_effect`, first.)
    let publish = "operation: example_extra\neffect: read\ninputs:\n  order_id: {type: identifier}\noutputs: {}\ndescription: An extra operation.\nbindings:\n  - mode: stream\n    verb: publish\n    channel: orders.status\n    key: {order_id: input.order_id}\n    body: {order_id: input.order_id}\n";
    refused(
        "operations/extra.yaml",
        publish,
        "kernel.effect_lower_than_derived",
    );
}

#[test]
fn every_tool_states_that_it_starts_nothing_and_sends_only_to_its_broker() {
    use a2m_files::{BindingStates, Reach};
    let folder = Folder::new("states");
    let registry = registry(&folder);
    let compiled = compile(&registry, &common::fixture_root()).unwrap();
    assert!(!compiled.review.items.is_empty());
    for item in &compiled.review.items {
        let Reach::Stated { groups, .. } = &item.reach else {
            panic!("`{}` does not state where it sends", item.tool);
        };
        let addresses: Vec<&str> = groups
            .iter()
            .flat_map(|group| group.addresses.iter().map(String::as_str))
            .collect();
        assert_eq!(addresses, ["broker.example.test:7001"], "{}", item.tool);
        for binding in &item.bindings {
            let BindingStates::Stated { runs, environment } = &binding.states else {
                panic!("`{}` is not stated", item.tool);
            };
            assert!(runs.is_empty() && environment.is_empty(), "{}", item.tool);
        }
    }
}
