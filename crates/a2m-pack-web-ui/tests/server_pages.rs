//! What the web pack drafts from a screen drawn on the server that names
//! little (field test 11, findings 61 and 62; decisions 0999 and 0998): a
//! list of links in a list, a record headed by its value whose values have
//! no labels, and a table with no accessible name whose headings carry a
//! table library's sort words. Each is drafted held, saying why; a person's
//! release of it compiles, and its read, run through the `ui` driver on the
//! test application, returns what the application's control interface
//! says it holds. A list whose items differ in shape, and a heading with no
//! words left once the sort words are off, stop by name.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::sync::Arc;

use a2m_core::{
    Address, CompiledTarget, Coverage, EvidenceLevel, Finding, FindingKind, LimitationCode,
    NoSecrets, ProducedBy, Source, Subject, Timestamp, Value, Values,
};
use a2m_files::{CompileMode, DraftReport, Drafter, RelPath, TargetRoot};
use common::{Folder, Scene, Stand, fixture};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

const ORIGIN: &str = "http://127.0.0.1:4010";
const LIST: &str = "example_orders_listed_items_list";
const RECORD: &str = "example_orders_record_get";
const TABLE: &str = "example_cases_rows_kept_table_list";

fn finding(path: &str, found: &Json) -> Finding {
    Finding::about(
        Subject::Address(Address::new(ORIGIN, path).unwrap()),
        FindingKind::new("screen").unwrap(),
        found.to_string(),
        Source::LiveSystem,
        ProducedBy {
            provider: "web_probe".to_owned(),
            version: "0.0.1".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(0),
    )
    .level(EvidenceLevel::Discovered)
    .coverage(Coverage::Partial)
    .limitation(LimitationCode::SingleVariant, "Seen in one capture.")
    .build()
    .unwrap()
}

fn screen(elements: Json) -> Json {
    json!({"format": "1", "title": "", "cut": false, "seen_by": "person", "elements": elements})
}

/// The page of orders as a list of links, as the probe keeps it: the links
/// are in a list's items, so their names are kept as types.
fn listed(items: &[Json]) -> Finding {
    let mut elements = vec![
        json!({"depth": 0, "role": "main", "name": ""}),
        json!({"depth": 1, "role": "heading", "name": "Orders", "level": 1}),
        json!({"depth": 1, "role": "list", "name_type": "text"}),
    ];
    for item in items {
        elements.push(json!({"depth": 2, "role": "listitem"}));
        for (depth, role) in item.as_array().unwrap().iter().map(|one| {
            (
                one[0].as_u64().unwrap(),
                one[1].as_str().unwrap().to_owned(),
            )
        }) {
            elements.push(json!({"depth": 2 + depth, "role": role, "name_type": "code"}));
        }
    }
    finding("/orders/listed", &screen(Json::Array(elements)))
}

fn link() -> Json {
    json!([[1, "link"]])
}

/// The pages of two orders' records, joined: the heading the pages did not
/// share is kept as its type, with `varies`.
fn record() -> Finding {
    let mut found = screen(json!([
        {"depth": 0, "role": "main", "name": ""},
        {"depth": 1, "role": "heading", "name_type": "text", "level": 1, "varies": true},
        {"depth": 1, "role": "paragraph"},
        {"depth": 2, "role": "text", "name_type": "code"},
        {"depth": 1, "role": "paragraph"},
        {"depth": 2, "role": "text", "name_type": "text"},
    ]));
    found["pages"] = json!(["/orders/A-100/record", "/orders/A-101/record"]);
    finding("/orders/{order_id}/record", &found)
}

/// The table with no accessible name, as the probe keeps it.
fn kept_table(second: &str) -> Finding {
    finding(
        "/cases/rows-kept",
        &screen(json!([
            {"depth": 0, "role": "main", "name": ""},
            {"depth": 1, "role": "heading", "name": "Kept orders", "level": 1},
            {"depth": 1, "role": "table", "name": ""},
            {"depth": 2, "role": "rowgroup"},
            {"depth": 3, "role": "row"},
            {"depth": 4, "role": "columnheader", "name": "Order Order: Activate to invert sorting"},
            {"depth": 4, "role": "columnheader", "name": second},
            {"depth": 2, "role": "rowgroup"},
            {"depth": 3, "role": "row"},
            {"depth": 4, "role": "cell", "name_type": "code"},
            {"depth": 4, "role": "cell", "name_type": "text"},
            {"depth": 1, "role": "paragraph"},
            {"depth": 2, "role": "text", "name_type": "text"},
        ])),
    )
}

fn draft(name: &str, findings: &[Finding]) -> (Folder, Stand, DraftReport) {
    let stand = Stand::new(name, Arc::new(NoSecrets));
    let folder = Folder::new(name);
    std::fs::copy(
        fixture("example").join("target.yaml"),
        folder.path().join("target.yaml"),
    )
    .unwrap();
    let report = Drafter::new(
        stand.registry.compiler_set(),
        &TargetRoot::new(folder.path()),
    )
    .draft(findings)
    .unwrap();
    (folder, stand, report)
}

fn read(folder: &Path, file: &str) -> String {
    std::fs::read_to_string(folder.join(file)).unwrap()
}

/// The person's release of every held draft: each line that holds an open
/// question is given the answer of its key, and the file is moved to its
/// place (decision 0298, point 5).
fn release(folder: &Path, report: &DraftReport, answers: &[(&str, &str)]) {
    for held in &report.held {
        let from = folder.join(held.as_str());
        let text = std::fs::read_to_string(&from).unwrap();
        let mut out = String::new();
        for line in text.lines() {
            let key = line.trim_start().split(':').next().unwrap_or_default();
            if line.contains("open_question") {
                let indent = &line[..line.len() - line.trim_start().len()];
                let answer = answers
                    .iter()
                    .find(|(asked, _)| *asked == key)
                    .unwrap_or_else(|| panic!("no answer for `{key}`: {text}"))
                    .1;
                out.push_str(&format!("{indent}{key}: {answer}\n"));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        let to = folder.join(held.as_str().strip_prefix("held/").unwrap());
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&to, out).unwrap();
        std::fs::remove_file(&from).unwrap();
    }
}

fn compiled(stand: &Stand, folder: &Path) -> CompiledTarget {
    fn walk(root: &Path, at: &Path, out: &mut Vec<(RelPath, Vec<u8>)>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let entry = entry.unwrap().path();
            if entry.is_dir() {
                walk(root, &entry, out);
            } else {
                let parts: Vec<String> = entry
                    .strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push((
                    RelPath::new(&parts.join("/")).unwrap(),
                    std::fs::read(&entry).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    walk(folder, folder, &mut files);
    let set = stand.registry.compiler_set();
    let sources = a2m_files::load_files(&TargetRoot::new(folder), &files, set)
        .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)));
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
        .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)))
        .compiled_target
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_of_links_and_a_record_headed_by_its_value_are_drafted_held_released_and_read() {
    let (folder, stand, report) = draft(
        "server-pages",
        &[listed(&[link(), link(), link()]), record()],
    );
    assert!(report.written.is_empty(), "{report:?}");
    let held: Vec<&str> = report.held.iter().map(RelPath::as_str).collect();
    for file in [
        "held/screens/orders_listed_items.yaml",
        "held/operations/orders_listed_items_list.yaml",
        "held/screens/orders_record.yaml",
        "held/operations/orders_record_get.yaml",
    ] {
        assert!(held.contains(&file), "{file}: {held:?}");
    }
    // Each says why it waits: the list has no name, and the record's
    // heading is its value and its values have no labels.
    let list = read(folder.path(), "held/screens/orders_listed_items.yaml");
    assert!(list.contains("gives the list no accessible name"), "{list}");
    assert!(list.contains("items: \"listitem\""), "{list}");
    let record_text = read(folder.path(), "held/screens/orders_record.yaml");
    assert!(
        record_text.contains("the heading is the record's own value"),
        "{record_text}"
    );
    assert!(record_text.contains("have no labels"), "{record_text}");
    assert!(
        record_text.contains("items: \"paragraph\""),
        "{record_text}"
    );

    // A person releases them, writing the locators each question names.
    release(
        folder.path(),
        &report,
        &[
            ("ready", "{visible: {role: main, name: \"\"}}"),
            ("locate", "[{role: list, name: \"\"}]"),
        ],
    );
    // The record's heading is found by its level, not by the list's.
    let record_file = folder.path().join("screens/orders_record.yaml");
    let text = std::fs::read_to_string(&record_file).unwrap().replace(
        "locate: [{role: list, name: \"\"}]",
        "locate: [{heading: 1}]",
    );
    std::fs::write(&record_file, text).unwrap();
    let compiled_target = compiled(&stand, folder.path());

    // Read through the `ui` driver, on the test application.
    let app = TestApp::start().await.unwrap();
    let scene = Scene::over(stand, compiled_target, &app.url(""));
    let (handle, mut prepared) = scene.reader().await;
    let listed = scene
        .read(&handle, &mut prepared, LIST, &Values::new())
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    let stored: Vec<Value> = app
        .stored_orders()
        .await
        .unwrap()
        .iter()
        .map(|order| Value::from(order.order_id.as_str()))
        .collect();
    assert_eq!(listed.values.get("items"), Some(&Value::List(stored)));

    let order = app.stored_order("A-101").await.unwrap().unwrap();
    let got = scene
        .read(
            &handle,
            &mut prepared,
            RECORD,
            &Values::new().with("order_id", "A-101"),
        )
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(
        got.values.get("heading"),
        Some(&Value::from(order.customer.as_str()))
    );
    assert_eq!(
        got.values.get("values"),
        Some(&Value::List(vec![
            Value::from(order.order_id.as_str()),
            Value::from(example_app::listed::status_words(order.status)),
        ]))
    );
    assert!(handle.release(prepared, false).await.complete);
    // A read writes nothing.
    assert!(app.writes().await.unwrap().is_empty());
}

#[test]
fn a_list_whose_items_differ_in_shape_stops_by_name() {
    let mixed = json!([[1, "link"], [1, "text"]]);
    let (_folder, _stand, report) = draft("server-mixed", &[listed(&[link(), mixed, link()])]);
    assert!(
        report.written.is_empty() && report.held.is_empty(),
        "{report:?}"
    );
    assert!(
        report
            .not_drafted
            .iter()
            .any(|left| left.reason.contains("items that differ in shape")),
        "{:?}",
        report.not_drafted
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_table_with_no_name_is_drafted_by_its_headings_as_a_person_reads_them_held() {
    let (folder, stand, report) = draft(
        "server-unnamed",
        &[kept_table("Customer Customer: Activate to invert sorting")],
    );
    assert!(report.written.is_empty(), "{report:?}");
    let screen_file = "held/screens/cases_rows_kept_table.yaml";
    let text = read(folder.path(), screen_file);
    assert!(
        text.contains("  header:\n    - \"Order\"\n    - \"Customer\"\n"),
        "{text}"
    );
    assert!(!text.contains("Activate"), "{text}");
    assert!(text.contains("gives no accessible name"), "{text}");
    assert!(text.contains("without the sort words"), "{text}");
    release(
        folder.path(),
        &report,
        &[
            ("ready", "{visible: {role: main, name: \"\"}}"),
            ("locate", "[{role: table, name: \"\"}]"),
        ],
    );
    let compiled_target = compiled(&stand, folder.path());
    assert!(
        compiled_target
            .operations
            .iter()
            .any(|operation| operation.name.as_str() == TABLE),
        "{:?}",
        compiled_target.operations
    );
}

#[test]
fn a_heading_with_no_words_left_once_the_sort_words_are_off_stops_by_name() {
    let (_folder, _stand, report) = draft("server-no-words", &[kept_table("Activate to sort")]);
    assert!(
        report.written.is_empty() && report.held.is_empty(),
        "{report:?}"
    );
    assert!(
        report
            .not_drafted
            .iter()
            .any(|left| left.reason.contains("no words of its own")),
        "{:?}",
        report.not_drafted
    );
}
