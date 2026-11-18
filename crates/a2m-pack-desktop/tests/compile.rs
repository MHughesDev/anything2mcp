//! The pack's compiler side, on every system: the fixture compiles into
//! the same plan each time; a desktop binding on an operation that writes
//! is refused by name; a locator is a known role and a name, nothing else.

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
fn a_desktop_binding_on_an_operation_that_writes_is_refused_by_name() {
    let folder = Folder::new("writes");
    let registry = registry(folder.path(), None);
    let root = folder.path().join("target");
    fixture_copy(&root, "operations/order.yaml", |text| {
        text.replacen("effect: read", "effect: write", 1)
    });
    let problems = compile_at(&registry, &root).unwrap_err();
    assert!(
        shown(&problems).contains("desktop.writes_not_built"),
        "{}",
        shown(&problems)
    );
    // A write verb is not one the mode serves at all.
    let root = folder.path().join("verb");
    fixture_copy(&root, "operations/order.yaml", |text| {
        text.replacen("verb: read", "verb: update", 1)
    });
    assert!(compile_at(&registry, &root).is_err());
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
