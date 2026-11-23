//! The pack's compiler side, on every system: the fixture compiles into
//! the same plan each time; a write lowers into `set` steps, one committing
//! `invoke`, and a read-back on a screen reached afresh; each refusal of a
//! write is by name; a locator is a known role and a name, nothing else.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use common::{Folder, compile_at, fixture, fixture_copy, registry, shown};

#[test]
fn the_fixture_compiles_into_the_same_plan_each_time() {
    let folder = Folder::new("compile");
    let registry = registry(folder.path(), None);
    let first = compile_at(&registry, &fixture()).unwrap_or_else(|p| panic!("{}", shown(&p)));
    let again = compile_at(&registry, &fixture()).unwrap();
    assert_eq!(first.plan, again.plan);
    let binding = common::binding_of(&first.plan, "example_order_get");
    let kinds: Vec<&str> = binding.act.iter().map(|step| step.kind.as_str()).collect();
    assert_eq!(kinds, ["goto", "wait", "read"]);
    assert!(binding.act.iter().all(|step| !step.commits));
    assert!(binding.confirm.is_empty() && binding.read_back.is_empty());
}

#[test]
fn a_write_lowers_into_sets_one_committing_invoke_and_a_read_back_afresh() {
    let folder = Folder::new("write");
    let registry = registry(folder.path(), None);
    let plan = compile_at(&registry, &fixture())
        .unwrap_or_else(|p| panic!("{}", shown(&p)))
        .plan;
    for operation in ["example_order_update", "example_order_upsert"] {
        let binding = common::binding_of(&plan, operation);
        let kinds: Vec<&str> = binding.act.iter().map(|step| step.kind.as_str()).collect();
        let sets = if operation == "example_order_update" {
            2
        } else {
            1
        };
        let mut want = vec!["goto", "wait"];
        want.extend(std::iter::repeat_n("set", sets));
        want.push("invoke");
        assert_eq!(kinds, want, "{operation}");
        // One commit point: the invoke of Save, and nothing else.
        let committing: Vec<&str> = binding
            .act
            .iter()
            .filter(|step| step.commits)
            .map(|step| step.kind.as_str())
            .collect();
        assert_eq!(committing, ["invoke"]);
        assert!(binding.confirm.is_empty());
        // The read-back is a new visit to the screen that shows what is
        // stored, never the one the values were typed into; and the wait
        // of each visit carries the record's key.
        let back: Vec<&str> = binding
            .read_back
            .iter()
            .map(|step| step.kind.as_str())
            .collect();
        assert_eq!(back, ["goto", "wait", "read"]);
        assert!(binding.read_back.iter().all(|step| !step.commits));
        let screen = |steps: &[a2m_core::Step]| steps[0].refers_to.clone().unwrap().entry;
        assert_eq!(screen(&binding.act), "order_edit");
        assert_eq!(screen(&binding.read_back), "order");
        for wait in [&binding.act[1], &binding.read_back[1]] {
            let args: Vec<&str> = wait.args.iter().map(|arg| arg.name.as_str()).collect();
            assert_eq!(args, ["key.order_number"]);
        }
    }
    // A write holds the person's screen alone (spec section 305).
    let defaults = registry.mode_defaults(&common::mode()).unwrap();
    assert_eq!(defaults.exclusive, a2m_provider::Exclusive::Screen);
}

#[test]
fn each_write_the_pack_cannot_verify_is_refused_by_name() {
    let folder = Folder::new("refused");
    let registry = registry(folder.path(), None);
    let update = "operations/order_write.yaml";
    for (case, (file, from, to, code)) in [
        // A password is never set from an input.
        (
            update,
            "      status: input.status\n",
            "      status: input.status\n      access_code: input.customer\n",
            "desktop.password_set",
        ),
        // A value written and never read back: only as `irreversible`.
        (
            update,
            "  status: {type: untrusted_text}\n",
            "",
            "desktop.unreadable_write",
        ),
        // A write names the action that saves.
        (update, "    action: save\n", "", "desktop.action_missing"),
        (update, "action: save", "action: send", "desktop.unknown_action"),
        // The window must show the record's key.
        (
            "screens/order_edit.yaml",
            "  order_number: {type: untrusted_text, control: desktop_text, locate: [{role: entry, name: \"Order number\"}]}\n",
            "",
            "desktop.key_unshown",
        ),
        // A read verb on an operation that writes.
        (
            "operations/order.yaml",
            "effect: read",
            "effect: write",
            "desktop.read_acts",
        ),
        // A verb the pack does not build is not one the mode serves.
        (update, "verb: update", "verb: create", "create"),
    ]
    .into_iter()
    .enumerate()
    {
        let root = folder.path().join(format!("case-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        fixture_copy(&root, file, |text| {
            assert!(text.contains(from), "{file} has no `{from}`");
            text.replacen(from, to, 1)
        });
        let problems = compile_at(&registry, &root)
            .err()
            .unwrap_or_else(|| panic!("case {case} compiled"));
        assert!(
            shown(&problems).contains(code),
            "case {case}, {code}: {}",
            shown(&problems)
        );
    }
    // Declared `irreversible`, a value that is not read back compiles.
    let root = folder.path().join("irreversible");
    fixture_copy(&root, update, |text| {
        text.replacen("  status: {type: untrusted_text}\n", "", 1)
            .replacen("effect: write", "effect: irreversible", 1)
    });
    if let Err(problems) = compile_at(&registry, &root) {
        panic!("{}", shown(&problems));
    }
}

#[test]
fn a_locator_is_a_known_role_and_a_name_and_nothing_else() {
    let folder = Folder::new("locators");
    let registry = registry(folder.path(), None);
    for (case, (from, to, code)) in [
        (r#"name: "Customer""#, r#"name: """#, "desktop.unnamed"),
        (
            r#"{role: entry, name: "Customer"}"#,
            r#"{role: textbox, name: "Customer"}"#,
            "desktop.role_unknown",
        ),
        (
            r#"{role: entry, name: "Customer"}"#,
            r##"{role: entry, name: "Customer", css: "#customer"}"##,
            "",
        ),
        (
            r#"{role: entry, name: "Customer"}"#,
            r#"{role: entry, name: "Customer", x: 10}"#,
            "",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let root = folder.path().join(format!("case-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        fixture_copy(&root, "screens/order.yaml", |text| text.replace(from, to));
        let problems = compile_at(&registry, &root)
            .err()
            .unwrap_or_else(|| panic!("`{to}` compiled"));
        assert!(
            shown(&problems).contains(code),
            "{to}: {}",
            shown(&problems)
        );
    }
}

#[test]
fn a_property_of_a_service_lowers_into_one_step_that_only_reads() {
    let folder = Folder::new("property");
    let registry = registry(folder.path(), None);
    let plan = compile_at(&registry, &fixture())
        .unwrap_or_else(|p| panic!("{}", shown(&p)))
        .plan;
    let binding = common::binding_of(&plan, "example_orders_total_get");
    let kinds: Vec<&str> = binding.act.iter().map(|step| step.kind.as_str()).collect();
    assert_eq!(kinds, ["property"]);
    assert!(
        binding
            .act
            .iter()
            .all(|step| !step.commits && step.idempotent)
    );
    assert!(binding.confirm.is_empty() && binding.read_back.is_empty());
}

#[test]
fn a_property_is_only_ever_read_and_each_refusal_is_by_name() {
    let folder = Folder::new("property-refused");
    let registry = registry(folder.path(), None);
    let file = "operations/service.yaml";
    for (case, (from, to, code)) in [
        // Setting a property is not built: no verb but `read` names one.
        (
            "    verb: read\n    property:",
            "    verb: update\n    property:",
            "desktop.property_reads_only",
        ),
        // An operation that changes something is not a read of a property.
        (
            "effect: read\ninputs: {}\noutputs:\n  total:",
            "effect: write\ninputs: {}\noutputs:\n  total:",
            "desktop.read_acts",
        ),
        // A binding that names a property names nothing a window holds.
        (
            "      name: Total\n",
            "      name: Total\n    action: save\n",
            "desktop.read_acts",
        ),
        // The path, interface, and name are ones a bus accepts.
        (
            "path: /org/example/Desk\n      interface: org.example.Desk.Orders\n      name: Total",
            "path: /org/example/Desk/../x\n      interface: org.example.Desk.Orders\n      name: Total",
            "desktop.property_invalid",
        ),
        (
            "interface: org.example.Desk.Orders\n      name: Total",
            "interface: org.example.Desk.Orders\n      name: \"Total; x\"",
            "desktop.property_invalid",
        ),
        // One output: the property's value as text.
        (
            "  total: {type: untrusted_text}\n",
            "  total: {type: untrusted_text}\n  other: {type: untrusted_text}\n",
            "desktop.property_output",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let root = folder.path().join(format!("case-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        fixture_copy(&root, file, |text| {
            assert!(text.contains(from), "{file} has no `{from}`");
            text.replacen(from, to, 1)
        });
        let problems = compile_at(&registry, &root)
            .err()
            .unwrap_or_else(|| panic!("case {case} compiled"));
        assert!(
            shown(&problems).contains(code),
            "case {case}, {code}: {}",
            shown(&problems)
        );
    }
}

#[test]
fn every_tool_states_that_it_starts_nothing_and_sends_to_no_address() {
    use a2m_files::{BindingStates, Reach};
    let folder = Folder::new("states");
    let registry = registry(folder.path(), None);
    let compiled = compile_at(&registry, &fixture()).unwrap_or_else(|p| panic!("{}", shown(&p)));
    assert!(!compiled.review.items.is_empty());
    for item in &compiled.review.items {
        let Reach::Stated { groups, .. } = &item.reach else {
            panic!("`{}` does not state where it sends", item.tool);
        };
        assert!(groups.is_empty(), "{}", item.tool);
        assert!(!item.bindings.is_empty());
        for binding in &item.bindings {
            let BindingStates::Stated { runs, environment } = &binding.states else {
                panic!("`{}` is not stated", item.tool);
            };
            assert!(runs.is_empty() && environment.is_empty(), "{}", item.tool);
        }
    }
}

#[test]
fn a_management_namespace_on_another_machine_is_not_stated() {
    use a2m_files::Reach;
    let folder = Folder::new("remote-namespace");
    let registry = registry(folder.path(), None);
    let root = folder.path().join("remote");
    fixture_copy(&root, "target.yaml", |text| {
        text.replace(
            "    service: org.example.Desk\n",
            "    service: org.example.Desk\n    management_namespace: '//other/root'\n",
        )
    });
    let compiled = compile_at(&registry, &root).unwrap_or_else(|p| panic!("{}", shown(&p)));
    assert!(!compiled.review.items.is_empty());
    for item in &compiled.review.items {
        assert!(matches!(item.reach, Reach::Partly { .. }), "{}", item.tool);
    }
}
