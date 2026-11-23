//! Discovery of the stand's window and the reads drafted from it. The
//! stand's control interface shows that discovery only looked; the drafted
//! reads compile, leaving out the file that holds a question, and return
//! what the stand holds; the password field's value is in nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{EvidenceLevel, Value};
use a2m_provider::DescribeForm;
use common::{Scene, compile_at, every_file};
use example_desk::{Order, SECRET};

#[tokio::test]
async fn a_snapshot_is_the_windows_shape_and_changes_nothing() {
    let Some(scene) = Scene::start("snapshot").await else {
        return;
    };
    let findings = scene.discover().await;
    assert_eq!(findings.len(), 1, "{findings:?}");
    let window = &findings[0];
    assert_eq!(window.kind().as_str(), "window");
    assert_eq!(window.level(), EvidenceLevel::Discovered);
    let found = window.found();
    // The shape: roles, the names that are its shape, structure.
    for shape in [
        r#""title":"Example order""#,
        r#""name":"Order number""#,
        r#""name":"Line items""#,
        r#""name":"Item code""#,
        r#""role":"password_text""#,
        r#""secret":true"#,
        r#""unnamed":true"#,
    ] {
        assert!(found.contains(shape), "{shape} not in {found}");
    }
    // Never a value the window shows.
    let order = Order::example();
    for value in [
        order.number.as_str(),
        order.customer.as_str(),
        "\"open\"",
        "L-101",
        "Hinge, steel",
        "R-1",
        "a value nothing labels",
        SECRET,
    ] {
        assert!(!found.contains(value), "{value} in {found}");
    }
    let truth = scene.desk().truth();
    assert_eq!(truth.writes, 0);
    assert!(!truth.acted(), "{:?}", truth.asked);
    assert_eq!(truth.secret_reads, 0);
    assert_eq!(truth.values_read(), 0, "{:?}", truth.asked);
    // Drafting does not draft writes: no value was set into any field, and
    // nothing was stored.
    assert!(truth.sets.is_empty(), "{:?}", truth.sets);
    assert_eq!(truth.stores, 0);

    let description = scene
        .registry
        .discoverers_for(&a2m_provider::SurfaceDescription {
            kind: a2m_core::SurfaceKind::new("desktop").unwrap(),
            archetype: a2m_core::Archetype::Interface,
            address: scene.surface.surface.address.clone(),
            facts: a2m_core::Values::new(),
        })
        .remove(0)
        .describe(&findings, DescribeForm::Full);
    assert!(
        description.text.contains("1 windows"),
        "{}",
        description.text
    );
    assert!(
        description.text.contains("1 hold a secret"),
        "{}",
        description.text
    );
    assert_eq!(description.untrusted, ["Example order"]);
    // The same window gives the same finding.
    let again = scene.discover().await;
    assert_eq!(again[0].found(), found);
}

#[tokio::test]
async fn the_drafted_reads_compile_and_return_what_the_stand_holds() {
    let Some(mut scene) = Scene::start("drafted").await else {
        return;
    };
    let findings = scene.discover().await;
    let (root, report) = scene.draft(&findings);
    let written: Vec<&str> = report.written.iter().map(|path| path.as_str()).collect();
    // What the drafts were made from, for a message that must explain a
    // run on a machine nobody can look at.
    let seen: String = findings
        .iter()
        .map(|finding| finding.found().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(12_000)
        .collect();
    for expected in [
        "screens/example_order_values.yaml",
        "screens/example_order_line_items.yaml",
        "operations/example_order_values_get.yaml",
        "operations/example_order_line_items_list.yaml",
    ] {
        assert!(
            written.contains(&expected),
            "{expected} not in {written:?}; drafted from:\n{seen}"
        );
    }
    assert_eq!(written.len(), 4, "{written:?}; drafted from:\n{seen}");
    // The one screen that holds a question waits in the holding folder and
    // is not also in place; the four files that name nothing of it are in
    // place (decision 0298).
    let held: Vec<&str> = report.held.iter().map(|path| path.as_str()).collect();
    assert_eq!(
        held,
        ["held/screens/example_order_values_undecided.yaml"],
        "{report:?}"
    );
    assert!(
        !root
            .path()
            .join("screens")
            .join("example_order_values_undecided.yaml")
            .exists()
    );
    assert!(
        report.refused.is_empty() && report.conflicts.is_empty(),
        "{report:?}"
    );
    // The one question: the name two controls share, quoted apart.
    assert_eq!(
        report.open_questions.len(),
        2,
        "{:?}",
        report.open_questions
    );
    assert!(report.open_questions.iter().all(
        |question| question.file.as_str() == "held/screens/example_order_values_undecided.yaml"
    ));
    // Nothing drafted presses, types, or reads the password field.
    for (path, text) in every_file(root.path()) {
        for never in [
            "push_button",
            "Save",
            "password_text",
            "Access code",
            "action",
            "commits",
            SECRET,
        ] {
            assert!(
                !text.contains(never),
                "`{never}` in {}:\n{text}",
                path.display()
            );
        }
        assert!(!text.contains("type: \"text\""), "{}", path.display());
    }
    let values = std::fs::read_to_string(
        root.path()
            .join("screens")
            .join("example_order_values.yaml"),
    )
    .unwrap();
    assert!(values.contains("untrusted_text"), "{values}");
    assert!(!values.contains("Reference"), "{values}");

    // The file that holds a question is in the holding folder, which the
    // loader does not read, and the rest compile with it there.
    assert!(
        root.path()
            .join("held")
            .join("screens")
            .join("example_order_values_undecided.yaml")
            .is_file()
    );
    scene.plan = match compile_at(&scene.registry, root.path()) {
        Ok(compiled) => compiled.plan,
        Err(problems) => panic!("the drafts do not compile:\n{}", common::shown(&problems)),
    };
    scene.surface = common::surface_of(&scene.plan, &scene.program);
    let order = Order::example();
    let got = scene
        .read_or_explain("example_example_order_get", Duration::from_secs(10))
        .await;
    let text = |name: &str| got.get(name).and_then(Value::as_text).unwrap().to_owned();
    assert_eq!(text("order_number"), order.number);
    assert_eq!(text("customer"), order.customer);
    assert_eq!(text("status"), order.status);
    assert_eq!(text("tracking"), scene.desk().truth().tracking);
    let lines = scene
        .read_or_explain(
            "example_example_order_line_items_list",
            Duration::from_secs(10),
        )
        .await;
    let column = |name: &str| -> Vec<String> {
        lines
            .get(name)
            .and_then(Value::as_list)
            .unwrap()
            .iter()
            .map(|value| value.as_text().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        column("description"),
        order
            .lines
            .iter()
            .map(|line| line.description.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        column("item_code"),
        order
            .lines
            .iter()
            .map(|line| line.code.clone())
            .collect::<Vec<_>>()
    );
    let truth = scene.desk().truth();
    assert_eq!(truth.writes, 0);
    assert!(!truth.acted(), "{:?}", truth.asked);
    assert_eq!(truth.secret_reads, 0);
    // The password field's value is in no finding, result, or log.
    assert!(
        findings
            .iter()
            .all(|finding| !finding.found().contains(SECRET))
    );
    assert!(!format!("{got:?}{lines:?}").contains(SECRET));
    for (path, text) in every_file(scene.folder.path()) {
        assert!(!text.contains(SECRET), "{}", path.display());
    }
}
