//! What the web pack drafts from the findings of a capture of a person's
//! use (package E2; decision 0392, points 3 and 4; decision 0394): a
//! request by a method that may write, seen at least twice in one shape, is
//! a held page request with a question in place of its effect and a closed
//! body with no value, and nothing else; one seen once, or in two shapes,
//! or whose body's names were withheld, is nothing, with the reason; a read
//! by `GET` a capture saw once is nothing; and a screen seen in a person's
//! use is drafted held.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{
    Address, Coverage, EvidenceLevel, Finding, FindingKind, LimitationCode, NoSecrets, ProducedBy,
    Source, Subject, Timestamp,
};
use a2m_files::{DraftReport, Drafter, TargetRoot};
use common::{Folder, Stand, fixture};
use serde_json::{Value as Json, json};

const ORIGIN: &str = "http://127.0.0.1:4010";

fn finding(kind: &str, path: &str, found: &Json) -> Finding {
    Finding::about(
        Subject::Address(Address::new(ORIGIN, path).unwrap()),
        FindingKind::new(kind).unwrap(),
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
    .limitation(LimitationCode::Sampled, "Seen in one capture.")
    .build()
    .unwrap()
}

/// The search the page of cases sends, as a capture of a person's use
/// gives it, with `change` made to it.
fn search(change: impl Fn(&mut Json)) -> Finding {
    let mut found = json!({
        "answered": true,
        "body": {
            "fields": {"op": "string", "status": "string"},
            "media_type": "application/json",
            "names_withheld": false,
        },
        "content": "json",
        "credential_headers": [],
        "format": "1",
        "made_by": "/cases/rows-by-post",
        "method": "POST",
        "path": "/api/cases/rows/search",
        "query": {},
        "query_left_out": false,
        "response": {"fields": {"rows": {"type": "list"}}, "type": "object"},
        "seen": 2,
        "set_headers": ["content-type"],
        "shapes": 1,
        "status": 200,
    });
    change(&mut found);
    finding("exchange", "/api/cases/rows/search", &found)
}

/// A read by `GET` a page made by itself, as a capture gives it.
fn totals(seen: u64) -> Finding {
    finding(
        "exchange",
        "/api/orders/{order_id}/totals",
        &json!({
            "answered": true,
            "content": "json",
            "credential_headers": [],
            "format": "1",
            "made_by": "/orders/{order_id}/summary",
            "method": "GET",
            "path": "/api/orders/{order_id}/totals",
            "query": {},
            "query_left_out": false,
            "response": {"fields": {"lines": {"type": "integer"}}, "type": "object"},
            "seen": seen,
            "set_headers": [],
            "shapes": 1,
            "status": 200,
        }),
    )
}

fn draft(name: &str, findings: &[Finding]) -> (Folder, DraftReport) {
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
    (folder, report)
}

fn paths(report: &DraftReport) -> Vec<String> {
    report
        .written
        .iter()
        .chain(&report.held)
        .map(|path| path.as_str().to_owned())
        .collect()
}

#[test]
fn a_request_by_a_method_that_may_write_seen_twice_in_one_shape_is_a_held_page_request_and_nothing_more()
 {
    let (folder, report) = draft("captured-search", &[search(|_| {})]);
    assert_eq!(
        paths(&report),
        ["held/page_requests/api_cases_rows_search.yaml"],
        "{report:?}"
    );
    let text = std::fs::read_to_string(
        folder
            .path()
            .join("held/page_requests/api_cases_rows_search.yaml"),
    )
    .unwrap();
    let keys: Vec<&str> = text
        .lines()
        .map(|line| line.trim_start().split(':').next().unwrap_or_default())
        .collect();
    for key in ["request_encoding", "request_schema", "additionalProperties"] {
        assert!(keys.contains(&key), "{key}: {text}");
    }
    for key in ["read_asserted", "enum", "operation"] {
        assert!(!keys.contains(&key), "{key}: {text}");
    }
    assert!(
        text.contains(
            "effect: {open_question: \"A capture of a person's use saw this request 2 times"
        ),
        "{text}"
    );
    assert!(
        text.contains("is not evidence that a request of this shape never does"),
        "{text}"
    );
    // The question is asked once, for the effect, and once for what the
    // answer always holds.
    assert_eq!(report.open_questions.len(), 2, "{report:?}");
}

#[test]
fn a_request_by_a_method_that_may_write_seen_once_in_two_shapes_or_with_names_withheld_is_nothing()
{
    for (name, change, why) in [
        (
            "captured-once",
            Box::new(|found: &mut Json| found["seen"] = json!(1)) as Box<dyn Fn(&mut Json)>,
            "seen once",
        ),
        (
            "captured-shapes",
            Box::new(|found: &mut Json| found["shapes"] = json!(2)),
            "more than one shape",
        ),
        (
            "captured-withheld",
            Box::new(|found: &mut Json| found["body"]["names_withheld"] = json!(true)),
            "withheld",
        ),
        (
            "captured-refused",
            Box::new(|found: &mut Json| found["status"] = json!(400)),
            "success",
        ),
    ] {
        let (_folder, report) = draft(name, &[search(change)]);
        assert!(paths(&report).is_empty(), "{name}: {report:?}");
        assert!(
            report
                .not_drafted
                .iter()
                .any(|left| left.reason.contains(why)),
            "{name}: {:?}",
            report.not_drafted
        );
    }
    // A finding of exploration says nothing of how often: a request by a
    // method that may write is never drafted from it.
    let (_folder, report) = draft(
        "explored-search",
        &[search(|found| {
            let found = found.as_object_mut().unwrap();
            found.remove("seen");
            found.remove("shapes");
        })],
    );
    assert!(paths(&report).is_empty(), "{report:?}");
}

#[test]
fn a_read_by_get_a_capture_saw_once_is_nothing_and_one_seen_twice_is_drafted_held() {
    let (_folder, report) = draft("captured-get-once", &[totals(1)]);
    assert!(paths(&report).is_empty(), "{report:?}");
    assert!(
        report
            .not_drafted
            .iter()
            .any(|left| left.reason.contains("seen once")),
        "{:?}",
        report.not_drafted
    );
    let (_folder, report) = draft("captured-get-twice", &[totals(2)]);
    assert_eq!(
        paths(&report),
        [
            "held/operations/api_orders_totals_request.yaml",
            "held/page_requests/api_orders_totals.yaml"
        ],
        "{report:?}"
    );
}

#[test]
fn a_screen_seen_in_a_persons_use_is_drafted_held_with_its_readiness_asked() {
    let shared: Json = serde_json::from_str(
        &std::fs::read_to_string(fixture("findings").join("orders_screen.json")).unwrap(),
    )
    .unwrap();
    let mut found: Json = serde_json::from_str(shared["found"].as_str().unwrap()).unwrap();
    let explored = finding("screen", shared["path"].as_str().unwrap(), &found);
    let (_folder, report) = draft("explored-screen", &[explored]);
    assert!(!report.written.is_empty(), "{report:?}");
    found["seen_by"] = json!("person");
    let watched = finding("screen", shared["path"].as_str().unwrap(), &found);
    let (folder, report) = draft("watched-screen", &[watched]);
    assert!(report.written.is_empty(), "{report:?}");
    assert!(!report.held.is_empty(), "{report:?}");
    let screen = report
        .held
        .iter()
        .find(|path| path.as_str().starts_with("held/screens/"))
        .unwrap();
    let text = std::fs::read_to_string(folder.path().join(screen.as_str())).unwrap();
    assert!(text.contains("ready: {open_question:"), "{text}");
}
