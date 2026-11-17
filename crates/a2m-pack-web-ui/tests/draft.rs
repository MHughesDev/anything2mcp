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
    Address, Coverage, EvidenceLevel, FailureClass, Finding, FindingKind, LimitationCode, NoSecrets,
    Plan, ProducedBy, Source, Subject, Timestamp, Value, Values,
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
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("findings")
            .join("order_screen.json"),
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
    let (handle, mut prepared) = scene
        .try_prepare(
            None,
            Posture::Sandbox {
                test_records: false,
            },
        )
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    let read = scene
        .read(&handle, &mut prepared, operation, &Values::new())
        .await;
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
    assert_eq!(written, [OPERATION, SCREEN], "{report:?}");
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
        Value::List(stored.items.values().map(|item| Value::from(of(item))).collect())
    };
    assert_eq!(stored.items.len(), 3);
    assert_eq!(ran.values.get("item_code"), Some(&column(&|item| item.item_code.clone())));
    assert_eq!(ran.values.get("description"), Some(&column(&|item| item.description.clone())));
    assert_eq!(ran.values.get("quantity"), Some(&column(&|item| item.quantity.to_string())));
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
    assert!(screen.contains("note:") && !screen.contains("code:"), "{screen}");
    // The two tables of one name are one question, and no read.
    assert_eq!(report.open_questions.len(), 1, "{report:?}");
    assert_eq!(report.open_questions[0].file.as_str(), "screens/twice_lines.yaml");
    assert!(report.open_questions[0].question.contains("more than one table"));
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
                ("reach", DraftValue::map(vec![("url", DraftValue::text("/form"))])),
                ("ready", DraftValue::map(vec![("visible", locator("form", ""))])),
                (
                    "actions",
                    DraftValue::map(vec![(
                        "save",
                        DraftValue::map(vec![
                            ("locate", DraftValue::List(vec![locator("button", "Save")])),
                            ("confirm", DraftValue::map(vec![("visible", locator("status", ""))])),
                        ]),
                    )]),
                ),
            ]),
        }])
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_may_only_look_has_a_tab_that_cannot_write() {
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
    // The browser fails the page's write before it leaves, and the run
    // stops: the page did something no screen declares.
    let stopped = look(&scene, "example_activity_get").await.unwrap_err();
    assert_eq!(
        class_and_code(&stopped),
        (FailureClass::UnknownVariant, "unexpected_page"),
        "{stopped}"
    );
    assert!(app.trap_writes().await.unwrap().is_empty());
    assert!(app.writes().await.unwrap().is_empty());
    scene.stand.end().await;
}
