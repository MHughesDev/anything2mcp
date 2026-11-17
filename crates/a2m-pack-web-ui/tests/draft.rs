//! The drafting rules, from a snapshot of a page that shows an order to a
//! read that runs through the driver's handle in a tab that can only look,
//! against the test application. What the read returns is compared with
//! what the application's control interface says it holds.
//!
//! The snapshot is the one the web pack's discoverers take of the page:
//! the two crates of the pack share findings as data and never name each
//! other, so the finding is kept in `tests/fixtures/findings/`, and a test
//! of `a2m-pack-web-probe` holds that discovery of the page gives exactly
//! it, the same twice.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::sync::Arc;

use a2m_core::{
    Address, Coverage, EvidenceLevel, FailureClass, Finding, FindingKind, LimitationCode,
    NoSecrets, Plan, ProducedBy, Source, Subject, Timestamp, Value, Values,
};
use a2m_files::{
    CompileMode, Diagnostics, DraftContext, DraftDoc, DraftReport, DraftValue, Drafter,
    DraftingRule, RelPath, TargetRoot,
};
use a2m_provider::{Posture, StepFailure};
use common::{Folder, Scene, Stand, fixture, shown};
use example_app::harness::TestApp;
use example_app::store::format_length_in;

const READ: &str = "example_orders_a_100_line_items_list";
const SCREEN: &str = "screens/orders_a_100_line_items.yaml";
const OPERATION: &str = "operations/orders_a_100_line_items_list.yaml";

/// The snapshot of `/orders/A-100` the discoverers take, as a finding of
/// the application at `origin`.
fn order_screen(origin: &str) -> Finding {
    shared_finding(origin, "order_screen.json")
}

/// A finding the discoverers give, kept in `tests/fixtures/findings/`, as a
/// finding of the application at `origin`.
fn shared_finding(origin: &str, file: &str) -> Finding {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("findings")
            .join(file),
    )
    .unwrap();
    let shared: serde_json::Value = serde_json::from_str(&text).unwrap();
    screen_finding(
        origin,
        shared["path"].as_str().unwrap(),
        shared["found"].as_str().unwrap(),
    )
}

fn screen_finding(origin: &str, path: &str, found: &str) -> Finding {
    Finding::about(
        Subject::Address(Address::new(origin, path).unwrap()),
        FindingKind::new("screen").unwrap(),
        found,
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
    .limitation(LimitationCode::SingleVariant, "One state of the page.")
    .build()
    .unwrap()
}

/// A target folder that holds only the fixture's `target.yaml`, with the
/// surface at `origin`.
fn new_target(name: &str, origin: &str) -> Folder {
    let folder = Folder::new(name);
    let target = std::fs::read_to_string(fixture("example").join("target.yaml")).unwrap();
    std::fs::write(
        folder.path().join("target.yaml"),
        target.replace("http://127.0.0.1:4010", origin),
    )
    .unwrap();
    folder
}

fn draft(stand: &Stand, folder: &Folder, findings: &[Finding]) -> DraftReport {
    let root = TargetRoot::new(folder.path());
    Drafter::new(stand.registry.compiler_set(), &root)
        .draft(findings)
        .unwrap()
}

/// Every file of a target folder, in order of path.
fn files_of(folder: &Path) -> Vec<(RelPath, Vec<u8>)> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<(RelPath, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(at)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
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
    let mut out = Vec::new();
    walk(folder, folder, &mut out);
    out
}

/// Compiles a target folder, leaving out each file that holds an open
/// question, with `change` applied to the text of the file at `path`.
fn compile_drafts(
    stand: &Stand,
    folder: &Folder,
    report: &DraftReport,
    path: &str,
    change: impl Fn(&str) -> String,
) -> Result<Plan, Diagnostics> {
    let mut files: Vec<(RelPath, Vec<u8>)> = files_of(folder.path())
        .into_iter()
        .filter(|(file, _)| !report.open_questions.iter().any(|open| open.file == *file))
        .collect();
    for (file, bytes) in &mut files {
        if file.as_str() == path {
            *bytes = change(std::str::from_utf8(bytes).unwrap()).into_bytes();
        }
    }
    let set = stand.registry.compiler_set();
    let sources = a2m_files::load_files(&TargetRoot::new(folder.path()), &files, set)?;
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
        .map(|compiled| compiled.plan)
}

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

/// Runs one read of a plan in a run that may only look.
async fn look(scene: &Scene, operation: &str) -> Result<common::Ran, StepFailure> {
    look_with(scene, operation, &Values::new()).await
}

/// Runs one read of a plan with these inputs, in a run that may only look.
async fn look_with(
    scene: &Scene,
    operation: &str,
    inputs: &Values,
) -> Result<common::Ran, StepFailure> {
    let (handle, mut prepared) = scene
        .try_prepare(
            None,
            Posture::Sandbox {
                test_records: false,
            },
        )
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    let read = scene.read(&handle, &mut prepared, operation, inputs).await;
    assert!(handle.release(prepared, read.is_err()).await.complete);
    read
}

#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_of_a_page_that_shows_an_order_drafts_a_read_that_reads_what_it_holds() {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let finding = order_screen(&origin);
    let stand = Stand::new("drafted", Arc::new(NoSecrets));

    let folder = new_target("drafts", &origin);
    let report = draft(&stand, &folder, std::slice::from_ref(&finding));
    let written: Vec<&str> = report.written.iter().map(RelPath::as_str).collect();
    assert_eq!(
        written,
        [
            OPERATION,
            "operations/orders_a_100_values_get.yaml",
            SCREEN,
            "screens/orders_a_100_values.yaml",
        ],
        "{report:?}"
    );
    assert!(
        report.refused.is_empty()
            && report.kept_failing.is_empty()
            && report.open_questions.is_empty()
            && report.conflicts.is_empty()
            && report.not_covered.is_empty(),
        "{report:?}"
    );
    // A second discovery of the page gives the same finding (the probe's
    // test), and the same finding gives the same drafts, byte for byte.
    let other = new_target("drafts-again", &origin);
    draft(&stand, &other, &[order_screen(&origin)]);
    for path in [OPERATION, SCREEN] {
        assert_eq!(
            std::fs::read(folder.path().join(path)).unwrap(),
            std::fs::read(other.path().join(path)).unwrap(),
            "{path}"
        );
    }
    // The screen locates by role and name only, holds no field and no
    // action, and keeps no value of the page; the read is a read.
    let screen = std::fs::read_to_string(folder.path().join(SCREEN)).unwrap();
    assert!(!screen.contains("css") && !screen.contains("actions") && !screen.contains("fields"));
    let stored = app.stored_order("A-100").await.unwrap().unwrap();
    for item in stored.items.values() {
        assert!(!screen.contains(&item.description), "{screen}");
    }
    let operation = std::fs::read_to_string(folder.path().join(OPERATION)).unwrap();
    assert!(!operation.contains("examples"), "{operation}");

    let plan = compile_drafts(&stand, &folder, &report, "", str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    let read = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == READ)
        .unwrap();
    assert_eq!(read.effect, a2m_core::EffectClass::Read);
    for output in &read.outputs {
        assert!(plan.field_types[output.field_type.unwrap().get()].untrusted);
    }

    // Through the driver's handle, in a run that may only look.
    let scene = Scene::over(stand, plan.clone(), &origin);
    let ran = look(&scene, READ).await.unwrap();
    let column = |of: &dyn Fn(&example_app::store::StoredItem) -> String| {
        Value::List(
            stored
                .items
                .values()
                .map(|item| Value::from(of(item)))
                .collect(),
        )
    };
    assert_eq!(stored.items.len(), 3);
    assert_eq!(
        ran.values.get("item_code"),
        Some(&column(&|item| item.item_code.clone()))
    );
    assert_eq!(
        ran.values.get("description"),
        Some(&column(&|item| item.description.clone()))
    );
    assert_eq!(
        ran.values.get("quantity"),
        Some(&column(&|item| item.quantity.to_string()))
    );
    assert_eq!(
        ran.values.get("length_in"),
        Some(&column(&|item| format_length_in(item.length_sixteenths)))
    );

    // What stops it: a heading the page does not show is never matched to
    // the nearest one, and a page that is not the screen is no screen.
    let renamed = compile_drafts(&scene.stand, &folder, &report, SCREEN, |text| {
        text.replace("Quantity", "Qty")
    })
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    let mut changed = Scene::over(Stand::new("renamed", Arc::new(NoSecrets)), renamed, &origin);
    let drift = look(&changed, READ).await.unwrap_err();
    assert_eq!(
        class_and_code(&drift),
        (FailureClass::Drift, "fingerprint_mismatch"),
        "{drift}"
    );
    changed.plan = compile_drafts(&changed.stand, &folder, &report, SCREEN, |text| {
        text.replacen("name: \"Line items\"", "name: \"Lines\"", 1)
    })
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    for operation in &mut changed.plan.operations {
        for binding in &mut operation.bindings {
            for step in &mut binding.act {
                step.timeout_ms = 1500;
            }
        }
    }
    let strange = look(&changed, READ).await.unwrap_err();
    assert_eq!(
        class_and_code(&strange),
        (FailureClass::UnknownVariant, "unexpected_page"),
        "{strange}"
    );
    changed.stand.end().await;

    // The application was read, and nothing in it was written.
    assert!(app.writes().await.unwrap().is_empty());
    assert_eq!(app.stored_order("A-100").await.unwrap().unwrap(), stored);
    scene.stand.end().await;
}

/// The elements of a snapshot, as the discoverers write them.
fn snapshot(elements: &[(u64, &str, Option<&str>)]) -> String {
    let elements: Vec<serde_json::Value> = elements
        .iter()
        .map(|(depth, role, name)| {
            let mut one = serde_json::json!({"depth": depth, "role": role});
            if let Some(name) = name {
                one["name"] = serde_json::Value::from(*name);
            }
            one
        })
        .collect();
    serde_json::json!({"format": "1", "title": "", "cut": false, "elements": elements}).to_string()
}

#[test]
fn a_form_is_never_drafted_and_a_table_that_cannot_be_told_apart_is_a_question() {
    let origin = "http://127.0.0.1:4010";
    let stand = Stand::new("rules", Arc::new(NoSecrets));
    let rows = |at: u64| {
        vec![
            (at, "table", Some("Lines")),
            (at + 1, "row", None),
            (at + 2, "columnheader", Some("Code")),
            (at + 2, "columnheader", Some("Code")),
            (at + 2, "columnheader", Some("Note")),
            (at + 1, "row", None),
            (at + 2, "cell", None),
            (at + 2, "cell", None),
            (at + 2, "cell", None),
        ]
    };
    // A form, with its fields and its button, and a table beside it.
    let mut form = vec![
        (0, "main", Some("")),
        (1, "form", Some("")),
        (2, "textbox", Some("Code")),
        (2, "button", Some("Save")),
    ];
    form.extend(rows(1));
    // Two tables with one name, in two sections.
    let mut twice = vec![(0, "main", Some(""))];
    twice.extend(rows(1));
    twice.extend(rows(1));
    let findings = [
        screen_finding(origin, "/form", &snapshot(&form)),
        screen_finding(origin, "/twice", &snapshot(&twice)),
        // Of another origin than the surface's: nothing.
        screen_finding("http://127.0.0.1:4011", "/form", &snapshot(&form)),
    ];
    let folder = new_target("rules", origin);
    let report = draft(&stand, &folder, &findings);
    let written: Vec<&str> = report.written.iter().map(RelPath::as_str).collect();
    assert_eq!(
        written,
        [
            "operations/form_lines_list.yaml",
            "screens/form_lines.yaml",
            "screens/twice_lines.yaml",
        ],
        "{report:?}"
    );
    // The form drafts nothing of its own: no field, no action. The column
    // whose heading another shares is left out.
    let screen = std::fs::read_to_string(folder.path().join("screens/form_lines.yaml")).unwrap();
    assert!(!screen.contains("fields") && !screen.contains("actions") && !screen.contains("Save"));
    assert!(
        screen.contains("note:") && !screen.contains("code:"),
        "{screen}"
    );
    // The two tables of one name are one question, and no read.
    assert_eq!(report.open_questions.len(), 1, "{report:?}");
    assert_eq!(
        report.open_questions[0].file.as_str(),
        "screens/twice_lines.yaml"
    );
    assert!(
        report.open_questions[0]
            .question
            .contains("more than one table")
    );
    // What leaves the questions out compiles.
    compile_drafts(&stand, &folder, &report, "", str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));

    // The drafter's refusal holds for this rule's kind: a screen drafted
    // with an action is one that writes, and is never written.
    let stand = Stand::with_rules(
        "refused",
        Arc::new(NoSecrets),
        vec![Arc::new(WithAnAction(
            FindingKind::new("screen_and_action").unwrap(),
        ))],
    );
    let folder = new_target("refused", origin);
    let report = draft(
        &stand,
        &folder,
        &[Finding::about(
            Subject::Address(Address::new(origin, "/form").unwrap()),
            FindingKind::new("screen_and_action").unwrap(),
            "{}",
            Source::LiveSystem,
            ProducedBy {
                provider: "web".to_owned(),
                version: "0.0.1".to_owned(),
                component: None,
            },
            Timestamp::from_unix_millis(0),
        )
        .build()
        .unwrap()],
    );
    assert!(report.written.is_empty(), "{report:?}");
    assert_eq!(report.refused.len(), 1, "{report:?}");
    assert_eq!(report.refused[0].0.as_str(), "screens/form_save.yaml");
    assert!(!folder.path().join("screens/form_save.yaml").exists());
}

/// A rule of a test's own that drafts what the pack's rules never do: the
/// screen of a form, with its button as an action.
struct WithAnAction(FindingKind);

impl DraftingRule for WithAnAction {
    fn finding_kind(&self) -> FindingKind {
        self.0.clone()
    }

    fn file_kind(&self) -> a2m_core::FileKindName {
        a2m_pack_web_ui::SCREEN_KIND
    }

    fn draft(
        &self,
        _finding: &Finding,
        _cx: &DraftContext<'_>,
    ) -> Result<Vec<DraftDoc>, a2m_core::Failure> {
        let locator = |role: &str, name: &str| {
            DraftValue::map(vec![
                ("role", DraftValue::text(role)),
                ("name", DraftValue::text(name)),
            ])
        };
        Ok(vec![DraftDoc {
            path: RelPath::new("screens/form_save.yaml")?,
            body: DraftValue::map(vec![
                ("screen", DraftValue::text("form_save")),
                (
                    "reach",
                    DraftValue::map(vec![("url", DraftValue::text("/form"))]),
                ),
                (
                    "ready",
                    DraftValue::map(vec![("visible", locator("form", ""))]),
                ),
                (
                    "actions",
                    DraftValue::map(vec![(
                        "save",
                        DraftValue::map(vec![
                            ("locate", DraftValue::List(vec![locator("button", "Save")])),
                            (
                                "confirm",
                                DraftValue::map(vec![("visible", locator("status", ""))]),
                            ),
                        ]),
                    )]),
                ),
            ]),
        }])
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_never_runs_in_a_tab_that_can_write_in_any_posture() {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let folder = new_target("trap", &origin);
    std::fs::create_dir_all(folder.path().join("screens")).unwrap();
    std::fs::create_dir_all(folder.path().join("operations")).unwrap();
    // A page whose script sends a write as soon as it loads.
    std::fs::write(
        folder.path().join("screens/activity.yaml"),
        "screen: activity\nsurface: app\nreach: {url: \"/traps/on-load\"}\n\
         ready: {visible: {role: heading, name: \"Recent activity\"}}\n\
         fields:\n  note:\n    type: untrusted_text\n    control: text\n    read: text\n\
         \x20   locate:\n      - {text: \"Nothing to show.\"}\n\
         \x20   fingerprint: {section: {role: main, name: \"\"}}\n",
    )
    .unwrap();
    std::fs::write(
        folder.path().join("operations/activity.yaml"),
        "operation: example_activity_get\neffect: read\ninputs: {}\n\
         outputs:\n  note: {type: untrusted_text}\n\
         bindings:\n  - {mode: ui, verb: read, screen: activity}\n\
         description: Reads the page that writes when it loads.\n",
    )
    .unwrap();
    let stand = Stand::new("trap", Arc::new(NoSecrets));
    let plan = compile_drafts(&stand, &folder, &DraftReport::default(), "", str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    let scene = Scene::over(stand, plan, &origin);
    // In every posture, a live run and a sandbox with test records among
    // them, the read's tab cannot write: the browser fails the page's
    // write before it leaves, and the run stops on it by name.
    for posture in [
        Posture::Live,
        Posture::Sandbox { test_records: true },
        Posture::Sandbox {
            test_records: false,
        },
        Posture::Discovery,
    ] {
        let (handle, mut prepared) = scene
            .try_prepare(None, posture)
            .await
            .unwrap_or_else(|failure| panic!("{failure}"));
        let stopped = scene
            .read(&handle, &mut prepared, "example_activity_get", &Values::new())
            .await
            .unwrap_err();
        assert!(handle.release(prepared, true).await.complete);
        assert_eq!(
            class_and_code(&stopped),
            (FailureClass::Policy, "page_write_refused"),
            "{posture:?}: {stopped}"
        );
        // What was refused is the page's own write.
        let said = stopped.failure.message();
        assert!(
            said.contains("`POST`") && said.contains("/traps/write/on-load"),
            "{said}"
        );
        assert!(
            app.trap_writes().await.unwrap().is_empty(),
            "{posture:?}"
        );
    }
    assert!(app.writes().await.unwrap().is_empty());

    // That the test means something: the same page reached as the step of
    // a write before its commit point is in a tab that may write, and the
    // page's write then reaches the application.
    let mut goto = scene.binding("example_activity_get").act[0].clone();
    goto.payload = a2m_core::PackPayload::encode(&serde_json::json!({"tab": "act"})).unwrap();
    let (handle, mut prepared) = scene.try_prepare(None, Posture::Live).await.unwrap();
    scene
        .run(&handle, &mut prepared, &[goto], &Values::new(), None)
        .await
        .unwrap();
    let stored = async {
        for _ in 0..100 {
            let writes = app.trap_writes().await.unwrap();
            if !writes.is_empty() {
                return writes;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        app.trap_writes().await.unwrap()
    }
    .await;
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(stored.get("on-load"), Some(&1), "{stored:?}");
    scene.stand.end().await;
}

const BY_KEY: &str = "example_orders_line_items_list";
const VALUES: &str = "example_orders_get";

#[tokio::test(flavor = "multi_thread")]
async fn two_pages_of_one_screen_draft_reads_by_key_that_read_each_order_and_refuse_a_key_that_leaves_its_segment()
 {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let stand = Stand::new("by-key", Arc::new(NoSecrets));
    let folder = new_target("by-key", &origin);
    let report = draft(
        &stand,
        &folder,
        &[shared_finding(&origin, "orders_screen.json")],
    );
    let written: Vec<&str> = report.written.iter().map(RelPath::as_str).collect();
    assert_eq!(
        written,
        [
            "operations/orders_line_items_list.yaml",
            "operations/orders_values_get.yaml",
            "screens/orders_line_items.yaml",
            "screens/orders_values.yaml",
        ],
        "{report:?}"
    );
    assert!(
        report.open_questions.is_empty() && report.refused.is_empty(),
        "{report:?}"
    );
    // The screens are reached by the pattern, and no file holds a record's
    // number or a value of the page.
    for path in &written {
        let text = std::fs::read_to_string(folder.path().join(path)).unwrap();
        assert!(!text.contains("A-10"), "{path}: {text}");
        assert!(!text.contains("Example Customer"), "{path}: {text}");
        if path.starts_with("screens/") {
            assert!(text.contains("/orders/{order_id}"), "{path}: {text}");
            assert!(!text.contains("actions") && !text.contains("css"), "{text}");
        }
    }

    let plan = compile_drafts(&stand, &folder, &report, "", str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    for name in [BY_KEY, VALUES] {
        let read = plan
            .operations
            .iter()
            .find(|operation| operation.name.as_str() == name)
            .unwrap();
        assert_eq!(read.effect, a2m_core::EffectClass::Read);
        let inputs: Vec<&str> = read
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect();
        assert_eq!(inputs, ["order_id"], "{name}");
    }

    let scene = Scene::over(stand, plan, &origin);
    for order in ["A-100", "A-101"] {
        let key = Values::new().with("order_id", order);
        let stored = app.stored_order(order).await.unwrap().unwrap();
        let ran = look_with(&scene, BY_KEY, &key).await.unwrap();
        let column = |of: &dyn Fn(&example_app::store::StoredItem) -> String| {
            Value::List(
                stored
                    .items
                    .values()
                    .map(|item| Value::from(of(item)))
                    .collect(),
            )
        };
        assert_eq!(
            ran.values.get("item_code"),
            Some(&column(&|item| item.item_code.clone())),
            "{order}"
        );
        assert_eq!(
            ran.values.get("quantity"),
            Some(&column(&|item| item.quantity.to_string())),
            "{order}"
        );
        let ran = look_with(&scene, VALUES, &key).await.unwrap();
        assert_eq!(
            ran.values.get("customer"),
            Some(&Value::from(stored.customer.clone())),
            "{order}"
        );
        assert_eq!(
            ran.values.get("status"),
            Some(&Value::from(format!("{:?}", stored.status))),
            "{order}"
        );
    }

    // A key that is not one plain segment is refused before the tab is
    // sent anywhere: the application hears no request at all.
    for refused in [
        "A-100/items/L-101",
        "..",
        "A-100?status=x",
        "A-100#top",
        "%2e%2e",
        "A-100%2Fx",
        "https:A-100",
        "\\A-100",
        "A-100\u{0}",
        "A-100\n",
        "A 100",
    ] {
        let before = app.seen().await.unwrap();
        let key = Values::new().with("order_id", refused);
        for name in [BY_KEY, VALUES] {
            let stopped = look_with(&scene, name, &key).await.unwrap_err();
            assert_eq!(
                class_and_code(&stopped),
                (FailureClass::Input, "address_value_refused"),
                "{refused:?}: {stopped}"
            );
            assert_eq!(stopped.sent, a2m_provider::Sent::NotSent);
        }
        assert_eq!(app.seen().await.unwrap(), before, "{refused:?}");
    }

    assert!(app.writes().await.unwrap().is_empty());
    scene.stand.end().await;
}

#[test]
fn a_label_two_terms_share_is_a_question_apart_and_the_other_values_are_read() {
    let origin = "http://127.0.0.1:4010";
    let stand = Stand::new("labels", Arc::new(NoSecrets));
    let page = snapshot(&[
        (0, "main", Some("")),
        (1, "DescriptionList", None),
        (2, "term", Some("Customer")),
        (2, "definition", Some("Customer")),
        (2, "term", Some("Note")),
        (2, "definition", Some("Note")),
        (2, "term", Some("Note")),
        (2, "definition", Some("Note")),
        // A term whose value the page does not label gives nothing.
        (2, "term", Some("Total")),
        (2, "definition", None),
        // A form beside them drafts nothing.
        (1, "form", Some("")),
        (2, "textbox", Some("Customer")),
        (2, "button", Some("Save")),
    ]);
    let folder = new_target("labels", origin);
    let report = draft(&stand, &folder, &[screen_finding(origin, "/order", &page)]);
    let written: Vec<&str> = report.written.iter().map(RelPath::as_str).collect();
    assert_eq!(
        written,
        [
            "operations/order_values_get.yaml",
            "screens/order_values.yaml",
            "screens/order_values_undecided.yaml",
        ],
        "{report:?}"
    );
    let read =
        std::fs::read_to_string(folder.path().join("operations/order_values_get.yaml")).unwrap();
    assert!(read.contains("customer:") && !read.contains("note") && !read.contains("total"));
    assert!(
        report
            .open_questions
            .iter()
            .all(|open| open.file.as_str() == "screens/order_values_undecided.yaml")
    );
    assert!(
        report
            .open_questions
            .iter()
            .any(|open| open.question.contains("labelled `Note`")),
        "{report:?}"
    );
    let screen = std::fs::read_to_string(folder.path().join("screens/order_values.yaml")).unwrap();
    assert!(
        !screen.contains("Save") && !screen.contains("actions"),
        "{screen}"
    );
    // What leaves the question out compiles.
    compile_drafts(&stand, &folder, &report, "", str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
}

#[test]
fn a_page_with_a_form_and_a_button_drafts_no_operation_that_could_write_by_any_rule() {
    let origin = "http://127.0.0.1:4010";
    let stand = Stand::new("never-writes", Arc::new(NoSecrets));
    // Everything the rules read, beside a form whose fields have the names
    // of the labels and the columns, and two buttons.
    let page = snapshot(&[
        (0, "main", Some("")),
        (1, "DescriptionList", None),
        (2, "term", Some("Customer")),
        (2, "definition", Some("Customer")),
        (1, "table", Some("Lines")),
        (2, "row", None),
        (3, "columnheader", Some("Code")),
        (2, "row", None),
        (3, "cell", None),
        (1, "form", Some("Order")),
        (2, "textbox", Some("Customer")),
        (2, "textbox", Some("Code")),
        (2, "checkbox", Some("Paid")),
        (2, "button", Some("Save")),
        (2, "button", Some("Delete")),
    ]);
    let findings = [
        screen_finding(origin, "/order", &page),
        screen_finding(origin, "/orders/{order_id}", &page),
    ];
    let folder = new_target("never-writes", origin);
    let report = draft(&stand, &folder, &findings);
    assert!(report.refused.is_empty(), "{report:?}");
    let operations: Vec<&str> = report
        .written
        .iter()
        .map(RelPath::as_str)
        .filter(|path| path.starts_with("operations/"))
        .collect();
    assert_eq!(operations.len(), 4, "{report:?}");
    let plan = compile_drafts(&stand, &folder, &report, "", str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    assert_eq!(plan.operations.len(), 4);
    for operation in &plan.operations {
        assert_eq!(
            operation.effect,
            a2m_core::EffectClass::Read,
            "{}",
            operation.name
        );
        for binding in &operation.bindings {
            assert!(
                binding.act.iter().all(|step| !step.commits),
                "{}",
                operation.name
            );
            let kinds: Vec<&str> = binding.act.iter().map(|step| step.kind.as_str()).collect();
            assert!(
                kinds
                    .iter()
                    .all(|kind| ["goto", "wait", "read"].contains(kind)),
                "{}: {kinds:?}",
                operation.name
            );
        }
    }
    for (path, bytes) in files_of(folder.path()) {
        let text = String::from_utf8(bytes).unwrap();
        for word in ["actions", "Save", "Delete", "Paid", "textbox", "checkbox"] {
            assert!(!text.contains(word), "{path:?}: {text}");
        }
    }
}
