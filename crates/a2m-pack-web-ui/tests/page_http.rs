//! The `page_http` mode, from the requests a page made by itself to a read
//! that runs through the driver's handle in a run that may only look,
//! against the test application (decision 0190). What the read returns is
//! compared with what the application's control interface says it holds,
//! and what the application saw is asked of it.
//!
//! The findings are the ones the web pack's discoverers give for the pages
//! that ask for an order's totals by themselves, over two orders: one asks
//! with nothing a read from inside the page would not send, and one with a
//! query and headers its script set (decision 0195). A test of
//! `a2m-pack-web-probe` holds that discovery gives exactly them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use a2m_core::{
    Address, CompiledTarget, Coverage, EvidenceLevel, FailureClass, Finding, FindingKind,
    LimitationCode, NoSecrets, ProducedBy, Source, Subject, Timestamp, Value, Values,
};
use a2m_files::{CompileMode, Diagnostics, DraftReport, Drafter, RelPath, TargetRoot};
use a2m_provider::{Posture, Sent, StepFailure};
use common::{Folder, Ran, Scene, Stand, fixture, page_http_mode, shown, ui_mode};
use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;
use example_app::store::format_length_in;

/// The drafted read.
const TOTALS: &str = "example_api_orders_totals_get";
/// The made-up cookie and token the page of an order's totals sets and
/// sends. Neither may appear in anything the pack gives.
const PAGE_SECRETS: &[&str] = &[
    "secret-for-tests-cookie-4c1d",
    "example-token-for-tests-9e2b",
];

fn finding(origin: &str, kind: &str, file: &str) -> Finding {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("findings")
            .join(file),
    )
    .unwrap();
    let shared: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(shared["kind"], kind);
    Finding::about(
        Subject::Address(Address::new(origin, shared["path"].as_str().unwrap()).unwrap()),
        FindingKind::new(kind).unwrap(),
        shared["found"].as_str().unwrap(),
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
    .limitation(LimitationCode::Sampled, "Matched from two requests.")
    .build()
    .unwrap()
}

/// The `exchange` finding of the page of an order's totals, matched over
/// two orders: its request carries a query and headers its script set.
fn totals_exchange(origin: &str) -> Finding {
    finding(origin, "exchange", "order_totals_exchange.json")
}

/// The `exchange` finding of the page of an order's summary, matched over
/// two orders: its request is the totals with nothing more.
fn summary_exchange(origin: &str) -> Finding {
    finding(origin, "exchange", "order_summary_exchange.json")
}

/// The summary's finding with one part of what it found changed.
fn changed(origin: &str, change: impl Fn(&mut serde_json::Value)) -> Finding {
    let base = summary_exchange(origin);
    let mut found: serde_json::Value = serde_json::from_str(base.found()).unwrap();
    change(&mut found);
    let Subject::Address(address) = base.subject() else {
        unreachable!()
    };
    Finding::about(
        Subject::Address(address.clone()),
        FindingKind::new("exchange").unwrap(),
        found.to_string(),
        Source::LiveSystem,
        base.produced_by().clone(),
        Timestamp::from_unix_millis(0),
    )
    .level(EvidenceLevel::Discovered)
    .coverage(Coverage::Partial)
    .limitation(LimitationCode::Sampled, "Matched from two requests.")
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

/// The part of a person who answers the held draft's one question about
/// what the answer may leave out (decisions 0322 and 0341): each held file
/// moves into place with the answer's `required` listing every value its
/// `properties` declare. The drafter never states it from a capture.
fn answered(folder: &Folder, report: &DraftReport) -> DraftReport {
    let mut report = report.clone();
    for held in std::mem::take(&mut report.held) {
        let from = folder.path().join(held.as_str());
        let name = held.as_str().strip_prefix("held/").unwrap().to_owned();
        let mut text = std::fs::read_to_string(&from).unwrap();
        if let Some(start) = text.find("required: {") {
            let end = start + text[start..].find('\n').unwrap();
            let properties = text.find("properties:\n").unwrap();
            let names: Vec<&str> = text[properties..]
                .lines()
                .skip(1)
                .take_while(|line| line.starts_with("    "))
                .filter(|line| !line.starts_with("     "))
                .map(|line| line.trim().trim_end_matches(':'))
                .collect();
            text.replace_range(start..end, &format!("required: [{}]", names.join(", ")));
        }
        let to = folder.path().join(&name);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&to, text).unwrap();
        std::fs::remove_file(&from).unwrap();
        report.written.push(RelPath::new(&name).unwrap());
    }
    report.written.sort();
    report.open_questions.clear();
    report
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
) -> Result<CompiledTarget, Diagnostics> {
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
        .map(|compiled| compiled.compiled_target)
}

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

/// Runs one read of a compiled target in a mode, in a run that may only look,
/// from the preparing of its tab to its release.
async fn look_in(
    scene: &Scene,
    mode: &a2m_core::Mode,
    operation: &str,
    inputs: &Values,
) -> Result<Ran, StepFailure> {
    let (handle, mut prepared) = scene
        .try_prepare_in(
            mode,
            None,
            Posture::Sandbox {
                test_records: false,
            },
        )
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    let read = scene
        .read_in(mode, &handle, &mut prepared, operation, inputs)
        .await;
    assert!(handle.release(prepared, read.is_err()).await.complete);
    read
}

/// The totals of an order as the control interface reports what it holds.
/// The order's key is the read's input, and not one of its outputs.
async fn totals_of(app: &TestApp, order: &str) -> Values {
    let stored = app.stored_order(order).await.unwrap().unwrap();
    let quantity: u64 = stored
        .items
        .values()
        .map(|item| u64::from(item.quantity))
        .sum();
    let sixteenths: u64 = stored
        .items
        .values()
        .map(|item| u64::from(item.quantity) * u64::from(item.length_sixteenths))
        .sum();
    Values::new()
        .with("lines", i64::try_from(stored.items.len()).unwrap())
        .with("total_quantity", i64::try_from(quantity).unwrap())
        .with(
            "total_length_in",
            format_length_in(u32::try_from(sixteenths).unwrap()),
        )
}

/// Every text of a folder and below, read as bytes and joined.
fn bytes_below(folder: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    for (_, bytes) in files_of(folder) {
        out.extend(bytes);
        out.push(b'\n');
    }
    out
}

fn holds(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_the_page_made_drafts_a_page_http_read_that_reads_each_order_by_key() {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let stand = Stand::new("page-http", Arc::new(NoSecrets));
    let folder = new_target("page-http", &origin);
    let findings = [
        summary_exchange(&origin),
        finding(&origin, "screen", "orders_screen.json"),
    ];
    let report = draft(&stand, &folder, &findings);
    // The request's entry asks which values its answer may leave out, and
    // is held with the read over it; it never states `required` from what
    // the capture saw. The screen's reads stay in place.
    let held: Vec<&str> = report.held.iter().map(RelPath::as_str).collect();
    assert_eq!(
        held,
        [
            "held/operations/api_orders_totals_request.yaml",
            "held/page_requests/api_orders_totals.yaml",
        ],
        "{report:?}"
    );
    assert_eq!(report.open_questions.len(), 1, "{report:?}");
    assert!(report.refused.is_empty(), "{report:?}");
    let held_entry = std::fs::read_to_string(
        folder
            .path()
            .join("held/page_requests/api_orders_totals.yaml"),
    )
    .unwrap();
    assert!(held_entry.contains("required: {"), "{held_entry}");
    assert!(!held_entry.contains("required: ["), "{held_entry}");
    // A person's answer moves the pair into place.
    let report = answered(&folder, &report);
    let written: Vec<&str> = report.written.iter().map(RelPath::as_str).collect();
    // The request's read and the screen's reads have names of their own:
    // one from the request's path, the others from the page's.
    assert_eq!(
        written,
        [
            "operations/api_orders_totals_request.yaml",
            "operations/orders_line_items_list.yaml",
            "operations/orders_values_get.yaml",
            "page_requests/api_orders_totals.yaml",
            "screens/orders_line_items.yaml",
            "screens/orders_values.yaml",
        ],
        "{report:?}"
    );
    let entry = std::fs::read_to_string(folder.path().join("page_requests/api_orders_totals.yaml"))
        .unwrap();
    for wanted in [
        "method: \"GET\"",
        "path: \"/api/orders/{order_id}/totals\"",
        "page: \"/orders/{order_id}/summary\"",
        "effect: \"read\"",
    ] {
        assert!(entry.contains(wanted), "{wanted}: {entry}");
    }
    let read = std::fs::read_to_string(
        folder
            .path()
            .join("operations/api_orders_totals_request.yaml"),
    )
    .unwrap();
    for wanted in [
        "mode: \"page_http\"",
        "key: \"order_id\"",
        "untrusted_text",
        "integer",
    ] {
        assert!(read.contains(wanted), "{wanted}: {read}");
    }
    // No example, no record's number, and no value of the page.
    for text in [&entry, &read] {
        assert!(!text.contains("A-10"), "{text}");
        assert!(!text.contains("examples"), "{text}");
    }
    // Drafting twice gives the same bytes.
    let again = new_target("page-http-again", &origin);
    let again_report = draft(&stand, &again, &findings);
    answered(&again, &again_report);
    assert_eq!(files_of(folder.path()), files_of(again.path()));

    let compiled_target = compile_drafts(&stand, &folder, &report, "", str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    let operation = compiled_target
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == TOTALS)
        .unwrap();
    assert_eq!(operation.effect, a2m_core::EffectClass::Read);
    let kinds: Vec<&str> = operation.bindings[0]
        .act
        .iter()
        .map(|step| step.kind.as_str())
        .collect();
    assert_eq!(kinds, ["goto", "request"]);
    assert!(operation.bindings[0].act.iter().all(|step| !step.commits));

    let scene = Scene::over(stand, compiled_target, &origin);
    let mode = page_http_mode();
    let mut said = Vec::new();
    for order in ["A-100", "A-101"] {
        let key = Values::new().with("order_id", order);
        let before = app.seen().await.unwrap().len();
        let ran = look_in(&scene, &mode, TOTALS, &key).await.unwrap();
        assert_eq!(ran.values, totals_of(&app, order).await, "{order}");
        // The page, its own read, and the read made from inside it.
        let seen = app.seen().await.unwrap();
        let wanted = format!("GET /api/orders/{order}/totals");
        assert!(
            seen[before..].contains(&format!("GET /orders/{order}/summary")),
            "{seen:?}"
        );
        assert!(
            seen[before..]
                .iter()
                .filter(|line| **line == wanted)
                .count()
                >= 1,
            "{seen:?}"
        );
        assert!(
            seen[before..].iter().all(|line| line.starts_with("GET ")),
            "{seen:?}"
        );
        said.push(format!("{:?} {}", ran.values, ran.means_text()));
        for capture in &ran.captures {
            said.push(String::from_utf8_lossy(&capture.bytes).into_owned());
        }
    }

    // A key that is not one plain segment is refused before any request:
    // the application hears nothing.
    for refused in [
        "A-100/x",
        "..",
        "A-100?view=all",
        "%2e%2e",
        "A 100",
        "A-100#x",
    ] {
        let before = app.seen().await.unwrap();
        let key = Values::new().with("order_id", refused);
        let stopped = look_in(&scene, &mode, TOTALS, &key).await.unwrap_err();
        assert_eq!(
            class_and_code(&stopped),
            (FailureClass::Input, "address_value_refused"),
            "{refused:?}: {stopped}"
        );
        assert_eq!(stopped.sent, Sent::NotSent);
        assert_eq!(
            app.heard_since(&before).await.unwrap(),
            Vec::<String>::new(),
            "{refused:?}"
        );
    }

    // An answer whose shape the stand changed stops by name, and so does
    // an answer of a status the entry does not declare. Nothing is
    // returned.
    let key = Values::new().with("order_id", "A-100");
    let faults = [
        (
            FaultKind::ChangedShape {
                field: "total_quantity".to_owned(),
                renamed_to: "quantity".to_owned(),
            },
            (FailureClass::Drift, "response_schema_mismatch"),
        ),
        (
            FaultKind::ChangedShape {
                field: "lines".to_owned(),
                renamed_to: "order_id".to_owned(),
            },
            (FailureClass::Drift, "response_schema_mismatch"),
        ),
        (
            FaultKind::Status { status: 503 },
            (FailureClass::UnknownVariant, "unexpected_status"),
        ),
    ];
    for (kind, wanted) in faults {
        app.set_fault(&Fault {
            kind,
            path_prefix: Some("/api/orders/A-100/totals".to_owned()),
            times: None,
        })
        .await
        .unwrap();
        let stopped = look_in(&scene, &mode, TOTALS, &key).await.unwrap_err();
        assert_eq!(class_and_code(&stopped), wanted, "{stopped}");
        said.push(stopped.to_string());
        app.clear_faults().await.unwrap();
    }

    // Nothing was written, and neither the cookie nor the token the page
    // holds is in the results, the findings, the drafts, or the trace.
    assert!(app.writes().await.unwrap().is_empty());
    let mut everything = said.join("\n").into_bytes();
    for one in &findings {
        everything.extend(one.found().as_bytes());
    }
    everything.extend(bytes_below(folder.path()));
    // The trace is the telemetry's folder; the browser's own profile, where
    // the page's cookie lives as the browser keeps it, is not looked in.
    everything.extend(bytes_below(&scene.stand.folder.path().join("logs")));
    for secret in PAGE_SECRETS {
        assert!(!holds(&everything, secret), "{secret}");
    }
    scene.stand.end().await;
}

/// A page request and a read over it written by hand, as the drafting rule
/// once drafted them from a request whose query or headers it dropped: the
/// read asks for the total length of an order with nothing more.
fn lengths_read(page: &str) -> (String, &'static str) {
    (
        format!(
            "endpoint: api_orders_lengths\nsurface: app\nmethod: GET\n\
             path: \"/api/orders/{{order_id}}/lengths\"\npage: \"{page}\"\neffect: read\n\
             response_schema:\n  type: object\n  required: [order_id, total_length]\n\
             \x20 properties:\n    order_id: {{type: string}}\n    total_length: {{type: string}}\n"
        ),
        "operation: example_api_orders_lengths_get\neffect: read\nkey: order_id\n\
         inputs: {order_id: {type: text}}\noutputs: {total_length: {type: untrusted_text}}\n\
         bindings:\n  - {mode: page_http, verb: read, endpoint: api_orders_lengths}\n\
         description: Reads the total length of an order.\n",
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_the_driver_cannot_make_again_exactly_drafts_a_question_and_no_read() {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let stand = Stand::new("page-http-unmade", Arc::new(NoSecrets));

    // First, why: a read that leaves out what the page sent is answered
    // with another variant of the same shape. The page asks for the total
    // length in centimetres, by a query or by a header its script sets;
    // the read, which sends neither, is answered in inches, its shape is
    // the same, and nothing stops it.
    let stored = app.stored_order("A-100").await.unwrap().unwrap();
    let sixteenths: u64 = stored
        .items
        .values()
        .map(|item| u64::from(item.quantity) * u64::from(item.length_sixteenths))
        .sum();
    let inches = format_length_in(u32::try_from(sixteenths).unwrap());
    let centimetres = (sixteenths * 254 / 1600).to_string();
    assert_ne!(inches, centimetres);
    for page in [
        "/orders/{order_id}/lengths-by-query",
        "/orders/{order_id}/lengths-by-header",
    ] {
        let folder = new_target("page-http-lengths", &origin);
        std::fs::create_dir_all(folder.path().join("page_requests")).unwrap();
        std::fs::create_dir_all(folder.path().join("operations")).unwrap();
        let (entry, read) = lengths_read(page);
        std::fs::write(folder.path().join("page_requests/lengths.yaml"), entry).unwrap();
        std::fs::write(folder.path().join("operations/lengths.yaml"), read).unwrap();
        let lengths = Stand::new("page-http-lengths", Arc::new(NoSecrets));
        let compiled_target = compile_drafts(
            &lengths,
            &folder,
            &DraftReport::default(),
            "",
            str::to_owned,
        )
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
        let scene = Scene::over(lengths, compiled_target, &origin);
        let key = Values::new().with("order_id", "A-100");
        let ran = look_in(
            &scene,
            &page_http_mode(),
            "example_api_orders_lengths_get",
            &key,
        )
        .await
        .unwrap();
        assert_eq!(
            ran.values.get("total_length"),
            Some(&Value::from(inches.clone())),
            "{page}"
        );
        scene.stand.end().await;
    }

    // An answer that is a redirect is the application's, as a session that
    // expired answers: it stops as an answer no entry declares, never as a
    // lost connection to be tried again.
    {
        let folder = new_target("page-http-redirect", &origin);
        std::fs::create_dir_all(folder.path().join("page_requests")).unwrap();
        std::fs::create_dir_all(folder.path().join("operations")).unwrap();
        std::fs::write(
            folder.path().join("page_requests/start.yaml"),
            "endpoint: start\nsurface: app\nmethod: GET\npath: \"/\"\npage: \"/orders\"\n\
             effect: read\nresponse_schema:\n  type: object\n  required: [order_id]\n\
             \x20 properties:\n    order_id: {type: string}\n",
        )
        .unwrap();
        std::fs::write(
            folder.path().join("operations/start.yaml"),
            "operation: example_start_get\neffect: read\ninputs: {}\n\
             outputs: {order_id: {type: untrusted_text}}\n\
             bindings:\n  - {mode: page_http, verb: read, endpoint: start}\n\
             description: Reads the start page.\n",
        )
        .unwrap();
        let redirected = Stand::new("page-http-redirect", Arc::new(NoSecrets));
        let compiled_target = compile_drafts(
            &redirected,
            &folder,
            &DraftReport::default(),
            "",
            str::to_owned,
        )
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
        let scene = Scene::over(redirected, compiled_target, &origin);
        let stopped = look_in(
            &scene,
            &page_http_mode(),
            "example_start_get",
            &Values::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            class_and_code(&stopped),
            (FailureClass::UnknownVariant, "unexpected_status"),
            "{stopped}"
        );
        scene.stand.end().await;
    }

    // So the rule drafts none. The page of an order's totals sends a query
    // and headers its script set: the entry is drafted with a question that
    // names them, by names alone, and no read is drafted over it.
    let folder = new_target("page-http-unmade", &origin);
    let report = draft(&stand, &folder, &[totals_exchange(&origin)]);
    // The entry holds the question, so it waits in the holding folder and is
    // not also in place (decision 0298).
    assert!(report.written.is_empty(), "{report:?}");
    let held: Vec<&str> = report.held.iter().map(RelPath::as_str).collect();
    assert_eq!(
        held,
        ["held/page_requests/api_orders_totals.yaml"],
        "{report:?}"
    );
    assert!(
        !folder
            .path()
            .join("page_requests/api_orders_totals.yaml")
            .exists()
    );
    // Two questions: which request answers what the page shows, and which
    // values its answer may leave out.
    assert_eq!(report.open_questions.len(), 2, "{report:?}");
    let question = &report.open_questions[0];
    let quoting = question.quoting.as_ref().unwrap();
    assert_eq!(
        quoting.words(),
        [
            "/api/orders/{order_id}/totals",
            "lang, view",
            "accept, x-csrf-token"
        ]
    );
    assert!(
        !quoting.sentence().contains("x-csrf"),
        "{}",
        quoting.sentence()
    );
    let everything = bytes_below(folder.path());
    for secret in PAGE_SECRETS.iter().chain(&["summary", "=en"]) {
        assert!(!holds(&everything, secret), "{secret}");
    }

    // Each part alone is enough, and a finding that does not say what the
    // page sent is taken as not able to be made again.
    let cases: Vec<(&str, Finding)> = vec![
        (
            "a query",
            changed(&origin, |found| {
                found["query"] = serde_json::json!({"unit": "unknown"})
            }),
        ),
        (
            "a bare entry of a query",
            changed(&origin, |found| found["query_left_out"] = true.into()),
        ),
        (
            "a header the script set",
            changed(&origin, |found| {
                found["set_headers"] = serde_json::json!(["x-unit"])
            }),
        ),
        (
            "a finding that does not say",
            changed(&origin, |found| {
                found.as_object_mut().unwrap().remove("set_headers");
            }),
        ),
    ];
    for (why, one) in cases {
        let folder = new_target("page-http-unmade-case", &origin);
        let report = draft(&stand, &folder, &[one]);
        assert!(report.written.is_empty(), "{why}: {report:?}");
        let held: Vec<&str> = report.held.iter().map(RelPath::as_str).collect();
        assert_eq!(
            held,
            ["held/page_requests/api_orders_totals.yaml"],
            "{why}: {report:?}"
        );
        assert_eq!(report.open_questions.len(), 2, "{why}: {report:?}");
    }
    assert!(app.writes().await.unwrap().is_empty());
    stand.end().await;
}

#[test]
fn nothing_is_drafted_for_a_request_that_could_write_is_not_json_or_went_elsewhere() {
    let origin = "http://127.0.0.1:4010";
    let stand = Stand::new("page-http-never", Arc::new(NoSecrets));
    let cases: Vec<(&str, Finding)> = vec![
        (
            "a method other than GET",
            changed(origin, |found| found["method"] = "POST".into()),
        ),
        (
            "an answer that is not JSON",
            changed(origin, |found| found["content"] = "document".into()),
        ),
        (
            "a request no page made",
            changed(origin, |found| {
                found.as_object_mut().unwrap().remove("made_by");
            }),
        ),
        ("another origin", summary_exchange("http://127.0.0.2:4010")),
    ];
    for (why, one) in cases {
        let folder = new_target("page-http-never", origin);
        let report = draft(&stand, &folder, &[one]);
        assert!(report.written.is_empty(), "{why}: {report:?}");
    }
}

#[test]
fn a_page_http_binding_on_an_operation_that_writes_is_refused_by_name() {
    let origin = "http://127.0.0.1:4010";
    let stand = Stand::new("page-http-writes", Arc::new(NoSecrets));
    let folder = new_target("page-http-writes", origin);
    let report = draft(&stand, &folder, &[summary_exchange(origin)]);
    let report = answered(&folder, &report);
    let path = "operations/api_orders_totals_request.yaml";
    // An operation declared to write, through a read of the page: refused
    // by this pack's own name.
    let problems = compile_drafts(&stand, &folder, &report, path, |text| {
        text.replace("effect: \"read\"", "effect: \"write\"")
    })
    .unwrap_err();
    assert!(
        shown(&problems).contains("web.page_http_writes"),
        "{}",
        shown(&problems)
    );
    // A page request whose path holds a word of an action is refused: a
    // GET there may act.
    let problems = compile_drafts(
        &stand,
        &folder,
        &report,
        "page_requests/api_orders_totals.yaml",
        |text| text.replace("/totals\"", "/purge\""),
    )
    .unwrap_err();
    assert!(
        shown(&problems).contains("web.request_path_invalid"),
        "{}",
        shown(&problems)
    );
    // A write verb the mode does not serve is the compiler's refusal.
    let problems = compile_drafts(&stand, &folder, &report, path, |text| {
        text.replace("verb: \"read\"", "verb: \"create\"")
    })
    .unwrap_err();
    assert!(
        shown(&problems).contains("unknown_verb"),
        "{}",
        shown(&problems)
    );
    // An `update` through an entry that reads is refused by this pack's
    // name: a write is made only through an entry a person wrote for it.
    let problems = compile_drafts(&stand, &folder, &report, path, |text| {
        text.replace("verb: \"read\"", "verb: \"update\"")
            .replace("effect: \"read\"", "effect: \"write\"")
    })
    .unwrap_err();
    assert!(
        shown(&problems).contains("web.page_http_writes"),
        "{}",
        shown(&problems)
    );
}

/// A screen of the page of an order's totals, written by hand, and a `ui`
/// read of what the page shows once its own request has answered.
const TOTALS_SCREEN: &str = "screen: order_totals_page
surface: app
reach: {url: \"/orders/{order_id}/totals\"}
ready: {text: {locate: {text: \"Loaded.\"}, equals: \"Loaded.\"}}
fields:
  total_quantity:
    type: untrusted_text
    control: text
    read: text
    locate:
      - {role: status, name: \"Total quantity\"}
    fingerprint: {section: {role: main, name: \"\"}}
";
const TOTALS_UI_READ: &str = "operation: example_orders_totals_get
effect: read
key: order_id
inputs: {order_id: {type: text}}
outputs: {total_quantity: {type: untrusted_text}}
bindings:
  - {mode: ui, verb: read, screen: order_totals_page}
description: Reads the total quantity the page of an order's totals shows.
";

#[tokio::test(flavor = "multi_thread")]
async fn the_page_http_read_and_the_screen_read_of_one_order_agree_and_are_measured() {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let stand = Stand::new("page-http-measured", Arc::new(NoSecrets));
    let folder = new_target("page-http-measured", &origin);
    let report = draft(&stand, &folder, &[summary_exchange(&origin)]);
    let report = answered(&folder, &report);
    std::fs::create_dir_all(folder.path().join("screens")).unwrap();
    std::fs::write(
        folder.path().join("screens/order_totals_page.yaml"),
        TOTALS_SCREEN,
    )
    .unwrap();
    std::fs::write(
        folder.path().join("operations/orders_totals_get.yaml"),
        TOTALS_UI_READ,
    )
    .unwrap();
    let compiled_target = compile_drafts(&stand, &folder, &report, "", str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    let scene = Scene::over(stand, compiled_target, &origin);
    let key = Values::new().with("order_id", "A-100");
    let reads = [
        (ui_mode(), "example_orders_totals_get"),
        (page_http_mode(), TOTALS),
    ];
    let mut results = Vec::new();
    let mut times = Vec::new();
    // The stand answers an order's totals slowly by nature; measured once
    // as it is, and once with no delay, so that what each mode adds shows.
    for slow in [None, Some(0)] {
        if let Some(ms) = slow {
            app.set_slow_ms(ms).await.unwrap();
        }
        for (mode, operation) in &reads {
            // One warm run, then ten measured, each from the preparing of its
            // tab to its release, as a run does; and, inside each, the steps
            // after the tab is open, and the last step alone, on a page that
            // has loaded.
            let warm = look_in(&scene, mode, operation, &key).await.unwrap();
            let (mut whole, mut steps, mut last) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..10 {
                let started = Instant::now();
                let (handle, mut prepared) = scene
                    .try_prepare_in(
                        mode,
                        None,
                        Posture::Sandbox {
                            test_records: false,
                        },
                    )
                    .await
                    .unwrap();
                let act = &scene.binding_in(operation, mode).act;
                let (before, end) = act.split_at(act.len() - 1);
                let open = Instant::now();
                scene
                    .run(&handle, &mut prepared, before, &key, None)
                    .await
                    .unwrap();
                let at_last = Instant::now();
                let ran = scene
                    .run(&handle, &mut prepared, end, &key, None)
                    .await
                    .unwrap();
                let done = Instant::now();
                assert!(handle.release(prepared, false).await.complete);
                whole.push(started.elapsed());
                steps.push(done - open);
                last.push(done - at_last);
                let mut values = ran.values;
                if values.is_empty() {
                    values = warm.values.clone();
                }
                assert_eq!(values, warm.values);
            }
            for each in [&mut whole, &mut steps, &mut last] {
                each.sort();
            }
            let label = match slow {
                None => format!("{mode} (the stand's own delay)"),
                Some(_) => format!("{mode} (no delay)"),
            };
            times.push((label, whole, steps, last));
            results.push(warm.values);
        }
    }
    // Where the two reads overlap, they agree: the page shows the total as
    // text, and the request gives it as a whole number.
    let shown_total = results[0].get("total_quantity").unwrap();
    let asked_total = results[1].get("total_quantity").unwrap();
    let Value::Int(asked) = asked_total else {
        panic!("{asked_total:?}")
    };
    assert_eq!(shown_total, &Value::from(asked.to_string()));
    assert_eq!(
        results[1].get("total_quantity"),
        totals_of(&app, "A-100").await.get("total_quantity")
    );
    let median = |each: &[Duration]| each[each.len() / 2];
    for (mode, whole, steps, last) in &times {
        eprintln!(
            "measured {mode}: a whole run median {:?} ({:?} to {:?}); its steps median {:?} ({:?} to {:?}); its last step median {:?} ({:?} to {:?})",
            median(whole),
            whole[0],
            whole[whole.len() - 1],
            median(steps),
            steps[0],
            steps[steps.len() - 1],
            median(last),
            last[0],
            last[last.len() - 1],
        );
    }
    assert!(app.writes().await.unwrap().is_empty());
    scene.stand.end().await;
}

/// A value of a page's request that is named as a secret is neither a
/// drafted output nor declared by the entry; the others are read, and the
/// invented value is in no file (decision 0302).
#[test]
fn an_answer_value_named_as_a_secret_is_never_a_drafted_output() {
    const INVENTED: &str = "example-not-a-secret-0002";
    let origin = "http://127.0.0.1:4010";
    let stand = Stand::new("page-http-secrets", Arc::new(NoSecrets));
    let folder = new_target("page-http-secrets", origin);
    let one = changed(origin, |found| {
        found["response"]["fields"]["access_token"] = serde_json::json!({"type": "text"});
        found["response"]["fields"]["clientSecret"] = serde_json::json!({"type": "text"});
        // Never a value in a finding; a made-up one here proves none is kept.
        found["note"] = INVENTED.into();
    });
    let report = draft(&stand, &folder, &[one]);
    let report = answered(&folder, &report);
    let written: Vec<&str> = report.written.iter().map(RelPath::as_str).collect();
    assert_eq!(
        written,
        [
            "operations/api_orders_totals_request.yaml",
            "page_requests/api_orders_totals.yaml",
        ],
        "{report:?}"
    );
    let read = std::fs::read_to_string(
        folder
            .path()
            .join("operations/api_orders_totals_request.yaml"),
    )
    .unwrap();
    assert!(read.contains("total_quantity:"), "{read}");
    assert!(
        read.contains("`access_token`") && read.contains("`clientsecret`"),
        "{read}"
    );
    assert!(
        read.contains("so that a secret is never an output"),
        "{read}"
    );
    for (path, bytes) in files_of(folder.path()) {
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains(INVENTED), "{path:?}");
        for key in ["access_token:", "client_secret:", "clientSecret"] {
            assert!(!text.contains(key), "{path:?} holds {key}: {text}");
        }
    }
    compile_drafts(&stand, &folder, &report, "", str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
}

/// A placeholder of the request's path named as a secret would be an input
/// that is a secret: nothing is drafted.
#[test]
fn a_request_placeholder_named_as_a_secret_drafts_nothing() {
    let origin = "http://127.0.0.1:4010";
    let stand = Stand::new("page-http-secret-hole", Arc::new(NoSecrets));
    let folder = new_target("page-http-secret-hole", origin);
    let one = changed(origin, |found| {
        found["path"] = "/api/orders/{api_key}/totals".into();
        found["made_by"] = "/orders/{api_key}/summary".into();
    });
    let report = draft(&stand, &folder, &[one]);
    assert!(report.written.is_empty(), "{report:?}");
}
