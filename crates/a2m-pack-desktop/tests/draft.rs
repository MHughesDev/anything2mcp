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
        .into_iter()
        .find(|found| found.declared().name.as_str() == "desktop_snapshot")
        .unwrap()
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
            // Nothing was left out for its name, so no description says
            // that something was (decision 0302).
            "Left out because",
            "never an output: .",
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
    scene.compiled_target = match compile_at(&scene.registry, root.path()) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("the drafts do not compile:\n{}", common::shown(&problems)),
    };
    scene.surface = common::surface_of(&scene.compiled_target, &scene.program);
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

/// A window finding written by hand, with a table column and two labelled
/// values named as secrets (decision 0302), one of them carrying an
/// invented value as its accessible name. It needs no session: the rules
/// read the finding alone. The drafted reads hold no such output, say what
/// they left out, hold the value in no file, and the target compiles.
#[test]
fn a_column_or_a_control_named_as_a_secret_is_never_a_drafted_output() {
    use a2m_core::{Address, Coverage, FindingKind, ProducedBy, Source, Subject, Timestamp};
    use a2m_files::{Drafter, TargetRoot};
    const INVENTED: &str = "example-not-a-secret-0002";
    let folder = common::Folder::new("named-secrets");
    let engine = common::Folder::new("named-secrets-engine");
    let registry = common::registry(engine.path(), None);
    let element = |depth: u64, role: &str, name: Option<&str>| {
        let mut one = serde_json::json!({"depth": depth, "role": role});
        if let Some(name) = name {
            one["name"] = name.into();
        }
        one
    };
    // A snapshot never keeps a value; one here proves that nothing which
    // reads the finding copies a field it does not know.
    let mut secret = element(0, "entry", Some("Client secret"));
    secret["value"] = INVENTED.into();
    let found = serde_json::json!({
        "format": "1",
        "cut": false,
        "program": "/opt/example/bin/example-desk",
        "window": {"role": "window", "title": "Example order"},
        "elements": [
            element(0, "entry", Some("Order number")),
            secret,
            element(0, "entry", Some("Password")),
            element(0, "table", Some("Line items")),
            element(1, "table_row", None),
            element(2, "table_column_header", Some("Item code")),
            element(2, "table_column_header", Some("API key")),
            element(1, "table_row", None),
            element(2, "table_cell", None),
            element(2, "table_cell", None),
        ],
    });
    let finding = a2m_core::Finding::about(
        Subject::Address(Address::new("desktop://desk", "/window/0").unwrap()),
        FindingKind::new("window").unwrap(),
        found.to_string(),
        Source::LiveSystem,
        ProducedBy {
            provider: "desktop".to_owned(),
            version: "0".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(0),
    )
    .level(EvidenceLevel::Discovered)
    .coverage(Coverage::Complete)
    .build()
    .unwrap();
    std::fs::copy(
        common::fixture().join("target.yaml"),
        folder.path().join("target.yaml"),
    )
    .unwrap();
    let report = Drafter::new(registry.compiler_set(), &TargetRoot::new(folder.path()))
        .draft(&[finding])
        .unwrap();
    let written: Vec<&str> = report.written.iter().map(|path| path.as_str()).collect();
    assert!(
        written.contains(&"operations/example_order_values_get.yaml")
            && written.contains(&"operations/example_order_line_items_list.yaml"),
        "{written:?} {report:?}"
    );
    let read = |file: &str| std::fs::read_to_string(folder.path().join(file)).unwrap();
    let values = read("operations/example_order_values_get.yaml");
    let list = read("operations/example_order_line_items_list.yaml");
    assert!(values.contains("order_number:"), "{values}");
    assert!(list.contains("item_code:"), "{list}");
    assert!(values.contains("`client_secret`"), "{values}");
    assert!(list.contains("`api_key`"), "{list}");
    for text in [&values, &list] {
        assert!(
            text.contains("so that a secret is never an output"),
            "{text}"
        );
    }
    for (path, text) in every_file(folder.path()) {
        assert!(!text.contains(INVENTED), "{path:?}");
        for key in ["client_secret:", "api_key:", "password:"] {
            assert!(!text.contains(key), "{path:?} holds {key}");
        }
    }
    common::compile_at(&registry, folder.path())
        .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)));
}

/// Field test 10's window, twice: windows of one record each, titled by
/// the order they show, as a GTK 3 dialog program shows one (findings 57,
/// 59, and 60; decision 0999). The two titles that differ in one word make
/// one screen reached by a pattern; the table with no name and no rows is
/// drafted and read by the column count it states; the labelled spin
/// button is drafted; and each read takes the order as its input and reads
/// only that order's window.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_window_of_one_record_is_drafted_whole_and_read_by_the_pattern_its_titles_show() {
    use example_desk::RecordWindow;
    let Some(mut scene) = Scene::start("record-window").await else {
        return;
    };
    let first = RecordWindow::order("1001", "Ash and Co", "Northtown");
    let second = RecordWindow::order("1002", "Birch Ltd", "Southtown");
    scene.desk().open_record_window(&first).await.unwrap();
    scene.desk().open_record_window(&second).await.unwrap();
    let findings = scene.discover().await;
    // The stand's own window, and one finding for the two record windows.
    assert_eq!(findings.len(), 2, "{findings:?}");
    let record = findings
        .iter()
        .find(|finding| finding.found().contains("Order {order_id}"))
        .unwrap_or_else(|| panic!("no window titled by a pattern in {findings:?}"));
    let found = record.found();
    assert!(found.contains(r#""windows":2"#), "{found}");
    assert!(found.contains(r#""columns":3"#), "{found}");
    for value in ["1001", "1002", "Ash and Co", "Northtown", "1001-A"] {
        assert!(!found.contains(value), "{value} in {found}");
    }
    let (root, report) = scene.draft(&findings);
    let written: Vec<&str> = report.written.iter().map(|path| path.as_str()).collect();
    for expected in [
        "screens/order_values.yaml",
        "screens/order_table.yaml",
        "operations/order_values_get.yaml",
        "operations/order_table_list.yaml",
    ] {
        assert!(written.contains(&expected), "{expected} not in {written:?}");
    }
    let read = |file: &str| std::fs::read_to_string(root.path().join(file)).unwrap();
    let values = read("screens/order_values.yaml");
    assert!(values.contains("spin_button"), "{values}");
    assert!(values.contains("Order {order_id}"), "{values}");
    let table = read("screens/order_table.yaml");
    // No name to locate it by: its headings find it.
    assert!(!table.contains("locate"), "{table}");
    let get = read("operations/order_values_get.yaml");
    assert!(get.contains("order_id"), "{get}");
    assert!(!get.contains("1001"), "{get}");
    scene.compiled_target = match compile_at(&scene.registry, root.path()) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("the drafts do not compile:\n{}", common::shown(&problems)),
    };
    scene.surface = common::surface_of(&scene.compiled_target, &scene.program);
    let order = |number: &str| a2m_core::Values::new().with("order_id", number);
    let text = |values: &a2m_core::Values, name: &str| {
        values
            .get(name)
            .and_then(Value::as_text)
            .unwrap_or_else(|| panic!("no `{name}` in {values:?}"))
            .to_owned()
    };
    for window in [&first, &second] {
        let number = window.title.trim_start_matches("Order ");
        let got = scene
            .read_with("example_order_get", &order(number), Duration::from_secs(10))
            .await
            .unwrap_or_else(|failure| panic!("{}", failure.failure));
        assert_eq!(text(&got, "customer"), window.customer);
        assert_eq!(text(&got, "city"), window.city);
        assert_eq!(text(&got, "lines"), window.lines);
        let rows = scene
            .read_with(
                "example_order_table_list",
                &order(number),
                Duration::from_secs(10),
            )
            .await
            .unwrap_or_else(|failure| panic!("{}", failure.failure));
        let items: Vec<String> = rows
            .get("item")
            .and_then(Value::as_list)
            .unwrap()
            .iter()
            .map(|value| value.as_text().unwrap().to_owned())
            .collect();
        let want: Vec<String> = window.rows.iter().map(|row| row[1].clone()).collect();
        assert_eq!(items, want);
    }
    // An order no window shows is a stop, never another order's window.
    let none = scene
        .read_with("example_order_get", &order("1003"), Duration::from_secs(2))
        .await
        .unwrap_err();
    assert_eq!(none.failure.code().as_str(), "unexpected_page");
    let truth = scene.desk().truth();
    // The column count is the one the table stated, asked of the stand.
    assert!(
        truth.asked.contains_key("Table.NColumns"),
        "{:?}",
        truth.asked
    );
    assert_eq!(truth.writes, 0);
    assert!(!truth.acted(), "{:?}", truth.asked);
}

/// The same window once, its table stating no column count: one title is
/// never a pattern, the table is not drafted, and a screen written by hand
/// that reads it by its headings stops and says why (finding 57; rule 7).
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_table_whose_cells_are_its_own_children_and_that_states_no_column_count_is_never_read() {
    use example_desk::RecordWindow;
    let Some(mut scene) = Scene::start("record-silent").await else {
        return;
    };
    let mut window = RecordWindow::order("1001", "Ash and Co", "Northtown");
    window.states_columns = false;
    scene.desk().open_record_window(&window).await.unwrap();
    let findings = scene.discover().await;
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(
        findings
            .iter()
            .any(|finding| finding.found().contains(r#""title":"Order 1001""#)),
        "{findings:?}"
    );
    let (root, report) = scene.draft(&findings);
    let written: Vec<&str> = report.written.iter().map(|path| path.as_str()).collect();
    assert!(
        written.contains(&"operations/order_1001_values_get.yaml"),
        "{written:?}"
    );
    assert!(
        !written.iter().any(|path| path.contains("order_1001_table")),
        "{written:?}"
    );
    let get =
        std::fs::read_to_string(root.path().join("operations/order_1001_values_get.yaml")).unwrap();
    assert!(get.contains("has a number in it"), "{get}");
    std::fs::write(
        root.path().join("screens/order_rows.yaml"),
        "screen: order_rows\nsurface: desk\nreach:\n  window: {role: frame, title: Order 1001}\nready:\n  visible: {role: table_column_header, name: Line}\nrows:\n  header: [Line, Item, Count]\n  columns:\n    item: {header: Item, type: untrusted_text}\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("operations/order_rows_list.yaml"),
        "operation: example_order_rows_list\neffect: read\ninputs: {}\noutputs:\n  item: {type: untrusted_text, list: true}\nbindings:\n  - mode: desktop\n    verb: list\n    screen: order_rows\ndescription: Lists the items of the order window.\n",
    )
    .unwrap();
    scene.compiled_target = match compile_at(&scene.registry, root.path()) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("the files do not compile:\n{}", common::shown(&problems)),
    };
    scene.surface = common::surface_of(&scene.compiled_target, &scene.program);
    let stopped = scene
        .read("example_order_rows_list", Duration::from_secs(5))
        .await
        .unwrap_err();
    assert_eq!(stopped.failure.code().as_str(), "fingerprint_mismatch");
    assert!(
        stopped
            .failure
            .message()
            .contains("does not state how many columns"),
        "{}",
        stopped.failure
    );
    assert!(!scene.desk().truth().acted());
}

/// Every finding the window rules cover and draft nothing from is listed
/// with a reason, for each early stop (finding 58).
#[test]
fn every_window_finding_the_rules_draft_nothing_from_says_why() {
    use a2m_core::{Address, Coverage, FindingKind, ProducedBy, Source, Subject, Timestamp};
    use a2m_files::{Drafter, TargetRoot};
    let folder = common::Folder::new("reasons");
    let engine = common::Folder::new("reasons-engine");
    let registry = common::registry(engine.path(), None);
    std::fs::copy(
        common::fixture().join("target.yaml"),
        folder.path().join("target.yaml"),
    )
    .unwrap();
    let finding =
        |at: usize, cut: bool, program: &str, title: &str, elements: serde_json::Value| {
            let found = serde_json::json!({
                "format": "1",
                "cut": cut,
                "program": program,
                "window": {"role": "frame", "title": title},
                "elements": elements,
            });
            a2m_core::Finding::about(
                Subject::Address(Address::new("desktop://desk", &format!("/window/{at}")).unwrap()),
                FindingKind::new("window").unwrap(),
                found.to_string(),
                Source::LiveSystem,
                ProducedBy {
                    provider: "desktop".to_owned(),
                    version: "0".to_owned(),
                    component: None,
                },
                Timestamp::from_unix_millis(0),
            )
            .level(EvidenceLevel::Discovered)
            .coverage(Coverage::Complete)
            .build()
            .unwrap()
        };
    let program = "/opt/example/bin/example-desk";
    // A table of cells with no rows and no column count, as field test 10
    // saw, and nothing else.
    let silent_table = serde_json::json!([
        {"depth": 0, "role": "panel"},
        {"depth": 1, "role": "table"},
        {"depth": 2, "role": "table_column_header", "name": "Order"},
        {"depth": 2, "role": "table_cell"},
    ]);
    let entry = serde_json::json!([{"depth": 0, "role": "entry", "name": "Customer"}]);
    let numbers = serde_json::json!([{"depth": 0, "role": "spin_button", "name": "Lines"}]);
    let findings = [
        finding(0, false, program, "Open orders", silent_table),
        finding(1, true, program, "Open orders", entry.clone()),
        finding(
            2,
            false,
            "/opt/example/bin/another",
            "Open orders",
            entry.clone(),
        ),
        finding(3, false, program, "\u{202e}Orders", entry),
        finding(4, false, program, "Order lines", numbers),
    ];
    let report = Drafter::new(registry.compiler_set(), &TargetRoot::new(folder.path()))
        .draft(&findings)
        .unwrap();
    assert!(report.written.is_empty(), "{report:?}");
    for (finding, words) in findings.iter().zip([
        "states how many columns",
        "cut short",
        "`desktop` surface",
        "title",
        "no value as text",
    ]) {
        let reasons: Vec<&str> = report
            .not_drafted
            .iter()
            .filter(|left| left.finding == finding.id())
            .map(|left| left.reason.as_str())
            .collect();
        // Both rules of the pack cover the finding; each says why.
        assert_eq!(reasons.len(), 2, "{:?}", report.not_drafted);
        assert!(
            reasons.iter().all(|reason| reason.contains(words)),
            "`{words}` not in {reasons:?}"
        );
    }
}
