//! The write half of the `page_http` mode (decision 0206): an `update` by
//! PUT from inside the page, with the page's own anti-forgery token, against
//! the test application's anti-forgery case. What a write stored is asked
//! of the application's control interface, never read through the mode's
//! own read alone; that nothing was sent is shown by what the application
//! stored and by its count of writes, never by comparing its whole list of
//! requests across an interval.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{CompiledTarget, FailureClass, Finding, NoSecrets, Value, Values};
use a2m_files::{CompileMode, Diagnostics, DraftValue, Drafter, RelPath, TargetRoot};
use a2m_provider::{Authority, Deadline, Posture, Sent, StepFailure, StepInput};
use common::{Folder, Ran, Scene, Stand, Write, fixture, page_http_mode, shown};
use example_app::faults::{Fault, FaultKind};
use example_app::forgery::TOKEN_PREFIX;
use example_app::harness::TestApp;

const UPDATE: &str = "example_order_note_update";
const PUT_ENTRY: &str = "page_requests/order_note_put.yaml";
const OPERATION: &str = "operations/order_note.yaml";

const PUT: &str = "endpoint: example_order_note_put
surface: app
method: PUT
path: \"/api/orders/{order_id}/note\"
page: \"/orders/{order_id}/note\"
effect: write
anti_forgery:
  from: {meta: csrf-token}
  header: X-CSRF-Token
  refused:
    - {status: 403, when: {error: forgery_token_invalid}}
request_schema:
  type: object
  properties: {note: {type: string}}
  required: [note]
  additionalProperties: false
responses:
  ok: [200]
  refused:
    - {status: 422, when: {error: invalid}}
response_schema:
  type: object
  properties: {order_id: {type: string}, note: {type: string}}
  required: [order_id, note]
";

const GET: &str = "endpoint: example_order_note_get
surface: app
method: GET
path: \"/api/orders/{order_id}/note\"
page: \"/orders/{order_id}/note\"
effect: read
responses: {absent: [404]}
response_schema:
  type: object
  properties: {order_id: {type: string}, note: {type: string}}
  required: [order_id, note]
";

const NOTE_UPDATE: &str = "operation: example_order_note_update
title: Change an order's note
effect: write
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
  note: {type: text, description: \"The note\"}
outputs:
  note: {type: text}
bindings:
  - mode: page_http
    verb: update
    endpoint: example_order_note_put
    body: {note: input.note}
    read_back: {endpoint: example_order_note_get}
description: >
  Changes the note of one order from inside the page of that note.
";

/// A target folder with the fixture's `target.yaml`, at `origin`, and the
/// note's two entries and its operation, with `change` applied to the text
/// of the file at `path`.
fn target(name: &str, origin: &str, path: &str, change: impl Fn(&str) -> String) -> Folder {
    let folder = Folder::new(name);
    let target = std::fs::read_to_string(fixture("example").join("target.yaml")).unwrap();
    let files = [
        (
            "target.yaml",
            target.replace("http://127.0.0.1:4010", origin),
        ),
        (PUT_ENTRY, PUT.to_owned()),
        ("page_requests/order_note_get.yaml", GET.to_owned()),
        (OPERATION, NOTE_UPDATE.to_owned()),
    ];
    for (file, text) in files {
        let at = folder.path().join(file);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        let text = if file == path { change(&text) } else { text };
        std::fs::write(at, text).unwrap();
    }
    folder
}

fn compile(stand: &Stand, folder: &Folder) -> Result<CompiledTarget, Diagnostics> {
    let set = stand.registry.compiler_set();
    let sources = a2m_files::load(&TargetRoot::new(folder.path()), set)?;
    a2m_files::compile(&sources, set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
        .map(|compiled| compiled.compiled_target)
}

fn refused_as(stand: &Stand, path: &str, change: impl Fn(&str) -> String, rule: &str) {
    let folder = target("refused", "http://127.0.0.1:4010", path, change);
    let problems = compile(stand, &folder).unwrap_err();
    assert!(
        shown(&problems).contains(rule),
        "{rule}: {}",
        shown(&problems)
    );
}

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

fn note(order: &str, text: &str) -> Values {
    Values::new().with("order_id", order).with("note", text)
}

/// A rule of a test's own that drafts what no rule of the pack drafts: a
/// page request that writes.
struct DraftsAWrite(a2m_core::FindingKind);

impl a2m_files::DraftingRule for DraftsAWrite {
    fn finding_kind(&self) -> a2m_core::FindingKind {
        self.0.clone()
    }

    fn file_kind(&self) -> a2m_core::FileKindName {
        a2m_pack_web_ui::REQUEST_KIND
    }

    fn draft(
        &self,
        _finding: &Finding,
        _cx: &a2m_files::DraftContext<'_>,
    ) -> Result<Vec<a2m_files::DraftDoc>, a2m_core::Failure> {
        let entry = |file: &str, method: &str, effect: &str| -> Result<_, a2m_core::Failure> {
            Ok(a2m_files::DraftDoc {
                path: RelPath::new(&format!("page_requests/{file}.yaml"))?,
                body: DraftValue::map(vec![
                    ("endpoint", DraftValue::text(file)),
                    ("surface", DraftValue::text("app")),
                    ("method", DraftValue::text(method)),
                    ("path", DraftValue::text("/api/orders/{order_id}/note")),
                    ("page", DraftValue::text("/orders/{order_id}/note")),
                    ("effect", DraftValue::text(effect)),
                    ("anti_forgery", DraftValue::text("none")),
                    (
                        "response_schema",
                        DraftValue::map(vec![
                            ("type", DraftValue::text("object")),
                            ("properties", DraftValue::map(Vec::new())),
                        ]),
                    ),
                ]),
            })
        };
        Ok(vec![
            entry("tries_a_put", "PUT", "write")?,
            entry("only_reads", "GET", "read")?,
        ])
    }
}

#[test]
fn a_write_entry_states_its_token_its_answers_and_its_read_back_or_does_not_compile() {
    let stand = Stand::new("page-http-write-compile", Arc::new(NoSecrets));
    let folder = target("compiles", "http://127.0.0.1:4010", "", str::to_owned);
    let compiled_target =
        compile(&stand, &folder).unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    let operation = compiled_target
        .operations
        .iter()
        .find(|known| known.name.as_str() == UPDATE)
        .unwrap();
    assert_eq!(operation.effect, a2m_core::EffectClass::Write);
    let binding = &operation.bindings[0];
    let kinds = |steps: &[a2m_core::Step]| -> Vec<(String, bool, bool)> {
        steps
            .iter()
            .map(|step| (step.kind.as_str().to_owned(), step.commits, step.lookup))
            .collect()
    };
    // The write's own steps, the PUT its one commit point; then the
    // read-back, a lookup.
    assert_eq!(
        kinds(&binding.act),
        [
            ("goto".to_owned(), false, false),
            ("request".to_owned(), true, false)
        ]
    );
    assert_eq!(
        kinds(&binding.read_back),
        [
            ("goto".to_owned(), false, true),
            ("request".to_owned(), false, true)
        ]
    );
    // The body's one field is an argument; the token is none.
    let names: Vec<&str> = binding.act[1]
        .args
        .iter()
        .map(|arg| arg.name.as_str())
        .collect();
    assert_eq!(names, ["path.order_id", "body.note"]);

    // An endpoint that takes no token says so in the file format's word.
    let folder = target("no-token", "http://127.0.0.1:4010", PUT_ENTRY, |text| {
        text.replace(
            "anti_forgery:\n  from: {meta: csrf-token}\n  header: X-CSRF-Token\n  refused:\n    - {status: 403, when: {error: forgery_token_invalid}}\n",
            "anti_forgery: none\n",
        )
    });
    compile(&stand, &folder).unwrap_or_else(|problems| panic!("{}", shown(&problems)));

    // What does not compile, each by its own name.
    let token = "anti_forgery:\n  from: {meta: csrf-token}\n  header: X-CSRF-Token\n  refused:\n    - {status: 403, when: {error: forgery_token_invalid}}\n";
    refused_as(
        &stand,
        PUT_ENTRY,
        |text| text.replace(token, ""),
        "web.anti_forgery_undeclared",
    );
    refused_as(
        &stand,
        PUT_ENTRY,
        |text| text.replace("method: PUT", "method: POST"),
        "web.request_method_not_built",
    );
    for method in ["PATCH", "DELETE"] {
        refused_as(
            &stand,
            PUT_ENTRY,
            |text| text.replace("method: PUT", &format!("method: {method}")),
            "web.request_method_not_built",
        );
    }
    refused_as(
        &stand,
        PUT_ENTRY,
        |text| text.replace("method: PUT", "method: GET"),
        "web.request_effect_mismatch",
    );
    for header in [
        "Cookie",
        "Authorization",
        "Content-Type",
        "Sec-Token",
        "X Token",
    ] {
        refused_as(
            &stand,
            PUT_ENTRY,
            |text| text.replace("header: X-CSRF-Token", &format!("header: \"{header}\"")),
            "web.anti_forgery_invalid",
        );
    }
    refused_as(
        &stand,
        PUT_ENTRY,
        |text| text.replace("header: X-CSRF-Token", "field: note"),
        "web.anti_forgery_invalid",
    );
    refused_as(
        &stand,
        PUT_ENTRY,
        |text| {
            text.replace(
                "note: {type: string}}\n  required",
                "note: {type: number}}\n  required",
            )
        },
        "web.request_shape_invalid",
    );
    refused_as(
        &stand,
        PUT_ENTRY,
        |text| text.replace("{status: 422", "{status: 200"),
        "web.responses_overlap",
    );
    refused_as(
        &stand,
        "page_requests/order_note_get.yaml",
        |text| text.replace("responses: {absent: [404]}\n", ""),
        "web.read_back_never_absent",
    );
    refused_as(
        &stand,
        OPERATION,
        |text| text.replace("    read_back: {endpoint: example_order_note_get}\n", ""),
        "web.read_back_missing",
    );
    refused_as(
        &stand,
        OPERATION,
        |text| {
            text.replace(
                "body: {note: input.note}",
                "body: {note: input.note, other: input.note}",
            )
        },
        "web.body_undeclared",
    );
    refused_as(
        &stand,
        OPERATION,
        |text| text.replace("    body: {note: input.note}\n", ""),
        "web.body_incomplete",
    );
    refused_as(
        &stand,
        OPERATION,
        |text| {
            text.replace(
                "read_back: {endpoint: example_order_note_get}",
                "read_back: {endpoint: example_order_note_put}",
            )
        },
        "web.effect_mismatch",
    );
    // A write's own path and page are held to the read's rules.
    refused_as(
        &stand,
        PUT_ENTRY,
        |text| text.replace("/note\"\npage", "/purge\"\npage"),
        "web.request_path_invalid",
    );

    // A write is never drafted: a rule that drafts a PUT entry has it
    // refused by the drafter, and the read beside it is written.
    let kind = a2m_core::FindingKind::new("note_page").unwrap();
    let stand = Stand::with_rules(
        "page-http-write-draft",
        Arc::new(NoSecrets),
        vec![Arc::new(DraftsAWrite(kind))],
    );
    let folder = Folder::new("page-http-write-draft");
    let target = std::fs::read_to_string(fixture("example").join("target.yaml")).unwrap();
    std::fs::write(folder.path().join("target.yaml"), target).unwrap();
    let finding = Finding::about(
        a2m_core::Subject::Address(
            a2m_core::Address::new("http://127.0.0.1:4010", "/orders/A-100/note").unwrap(),
        ),
        a2m_core::FindingKind::new("note_page").unwrap(),
        "{}",
        a2m_core::Source::LiveSystem,
        a2m_core::ProducedBy {
            provider: "test".to_owned(),
            version: "0.0.1".to_owned(),
            component: None,
        },
        a2m_core::Timestamp::from_unix_millis(0),
    )
    .level(a2m_core::EvidenceLevel::Discovered)
    .coverage(a2m_core::Coverage::Partial)
    .build()
    .unwrap();
    let report = Drafter::new(
        stand.registry.compiler_set(),
        &TargetRoot::new(folder.path()),
    )
    .draft(&[finding])
    .unwrap();
    let refused: Vec<&str> = report
        .refused
        .iter()
        .map(|(path, _)| path.as_str())
        .collect();
    assert!(
        refused.contains(&"page_requests/tries_a_put.yaml"),
        "{report:?}"
    );
    assert!(
        !folder
            .path()
            .join("page_requests/tries_a_put.yaml")
            .exists()
    );
    let written: Vec<&str> = report.written.iter().map(RelPath::as_str).collect();
    assert!(
        written.contains(&"page_requests/only_reads.yaml"),
        "{report:?}"
    );
}

/// A scene over the note's target, at the address of the application a
/// test started.
fn scene(name: &str, app: &TestApp, path: &str, change: impl Fn(&str) -> String) -> Scene {
    let stand = Stand::new(name, Arc::new(NoSecrets));
    let folder = target(name, &app.url(""), path, change);
    let compiled_target =
        compile(&stand, &folder).unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    Scene::over(stand, compiled_target, &app.url(""))
}

/// Every text a step gave: its values, its captures, and its failure.
fn everything(ran: &Result<Ran, StepFailure>) -> Vec<u8> {
    let mut out = Vec::new();
    match ran {
        Ok(ran) => {
            out.extend(format!("{:?}", ran.values).into_bytes());
            for capture in &ran.captures {
                out.extend(&capture.bytes);
                out.extend(capture.name.as_bytes());
            }
        }
        Err(failed) => {
            out.extend(failed.failure.message().as_bytes());
            out.extend(failed.failure.next_step().as_bytes());
            out.extend(failed.failure.said().unwrap_or_default().as_bytes());
        }
    }
    out
}

/// Every byte of every file below a folder.
fn bytes_below(folder: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return out;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(bytes_below(&path));
        } else {
            out.extend(std::fs::read(&path).unwrap_or_default());
        }
    }
    out
}

fn holds(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_anti_forgery_case_a_write_lands_once_with_the_page_s_token_and_reads_back_equal() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("page-http-write", &app, "", str::to_owned);
    let mode = page_http_mode();
    let binding = scene.binding_in(UPDATE, &mode).clone();
    let inputs = note("A-100", "Ring twice at the gate.");

    // Without the ledger's token the committing step is refused before
    // the driver sees it: the page is reached, and nothing is written.
    let handle = scene.handle_of(&mode);
    let (_, mut prepared) = scene
        .try_prepare_in(&mode, None, Posture::Live)
        .await
        .unwrap();
    let refused = scene
        .run(&handle, &mut prepared, &binding.act, &inputs, None)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Policy, "intent_missing")
    );
    assert_eq!(refused.sent, Sent::NotSent);
    // The driver's own guard: the same request, not marked as the commit
    // point, is refused in every posture, whatever the handle let by.
    let mut unmarked = binding.act[1].clone();
    unmarked.commits = false;
    let args = scene.args(&unmarked, &inputs);
    let input = StepInput::new(&args, Deadline::after(Duration::from_secs(60)));
    let guarded = handle
        .act(&mut prepared, &unmarked, &input, Authority::Read)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&guarded),
        (FailureClass::Input, "step_not_usable")
    );
    assert_eq!(guarded.sent, Sent::NotSent);
    handle.release(prepared, true).await;
    let stored = app.stored_notes().await.unwrap();
    assert!(stored.writes.is_empty(), "{stored:?}");
    assert_eq!(stored.notes["A-100"], "Leave at the side door.");

    // With it: the page is reached in an acting tab, and the PUT carries
    // the page's own token, read from its meta element.
    let write = Write::new(&["A-100"]).await;
    let (_, mut prepared) = scene
        .try_prepare_in(&mode, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let acted = scene
        .run(
            &handle,
            &mut prepared,
            &binding.act,
            &inputs,
            Some(write.authority()),
        )
        .await;
    let mut said = everything(&acted);
    let acted = acted.unwrap();
    let means = acted.means_text();
    assert!(means.contains("tab=act"), "{means}");
    assert!(means.contains("token=meta:header"), "{means}");
    assert!(means.contains("answer=written"), "{means}");
    assert!(!means.contains("Ring twice"), "{means}");

    // The application stored it, once, with the one token the page gave.
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.notes["A-100"], "Ring twice at the gate.");
    assert_eq!(stored.writes["A-100"], 1);
    assert_eq!(stored.tokens_used, 1);
    assert_eq!(stored.forgery_refused, 0);

    // The read-back reaches the page afresh, in the observing tab, and
    // reads what the application holds, equal to what was sent.
    let back = scene
        .run(&handle, &mut prepared, &binding.read_back, &inputs, None)
        .await;
    said.extend(everything(&back));
    let back = back.unwrap();
    assert!(!back.absent);
    assert!(
        back.means_text().contains("tab=observe"),
        "{}",
        back.means_text()
    );
    assert_eq!(back.values.get("note"), inputs.get("note"));
    assert_eq!(
        back.values.get("note"),
        Some(&Value::from(stored.notes["A-100"].as_str()))
    );
    assert!(handle.release(prepared, false).await.complete);

    // A read-back of a record that is not there is absent.
    let (_, mut prepared) = scene
        .try_prepare_in(&mode, None, Posture::Live)
        .await
        .unwrap();
    let none = scene
        .run(
            &handle,
            &mut prepared,
            &binding.read_back,
            &note("A-999", "x"),
            None,
        )
        .await
        .unwrap();
    assert!(none.absent);
    handle.release(prepared, false).await;

    // The stand's own guard, with the mode's token step taken out: the same
    // write from the same page, sent without the token, is refused by the
    // application and stores nothing. The entry declares no answer for
    // that, so the write is in doubt, never "not written".
    let bare = Scene::over(
        Stand::new("page-http-write-bare", Arc::new(NoSecrets)),
        {
            let stand = Stand::new("page-http-write-bare-compile", Arc::new(NoSecrets));
            let folder = target("bare", &app.url(""), PUT_ENTRY, |text| {
                text.replace(
                    "anti_forgery:\n  from: {meta: csrf-token}\n  header: X-CSRF-Token\n  refused:\n    - {status: 403, when: {error: forgery_token_invalid}}\n",
                    "anti_forgery: none\n",
                )
            });
            compile(&stand, &folder).unwrap()
        },
        &app.url(""),
    );
    let bare_binding = bare.binding_in(UPDATE, &mode).clone();
    let handle_bare = bare.handle_of(&mode);
    let (_, mut prepared) = bare
        .try_prepare_in(&mode, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let without = bare
        .run(
            &handle_bare,
            &mut prepared,
            &bare_binding.act,
            &note("A-100", "Sent without a token"),
            Some(write.authority()),
        )
        .await;
    said.extend(everything(&without));
    let without = without.unwrap_err();
    assert_eq!(
        class_and_code(&without),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert_eq!(without.sent, Sent::Unknown);
    handle_bare.release(prepared, true).await;
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.forgery_refused, 1);
    assert_eq!(stored.writes["A-100"], 1);
    assert_eq!(stored.notes["A-100"], "Ring twice at the gate.");

    // No token of any page is in a result, a capture, a failure, or a log.
    said.extend(bytes_below(&scene.stand.folder.path().join("logs")));
    said.extend(bytes_below(&bare.stand.folder.path().join("logs")));
    assert!(!said.is_empty());
    assert!(!holds(&said, TOKEN_PREFIX));
    assert!(stored.tokens_issued >= 4, "{stored:?}");
    bare.stand.end().await;
    scene.stand.end().await;
}

/// Runs a write's own steps, and gives what the last one gave.
async fn write_in(
    scene: &Scene,
    write: &Write,
    inputs: &Values,
) -> (Result<Ran, StepFailure>, a2m_provider::Prepared) {
    let mode = page_http_mode();
    let binding = scene.binding_in(UPDATE, &mode);
    let handle = scene.handle_of(&mode);
    let (_, mut prepared) = scene
        .try_prepare_in(&mode, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let ran = scene
        .run(
            &handle,
            &mut prepared,
            &binding.act,
            inputs,
            Some(write.authority()),
        )
        .await;
    (ran, prepared)
}

#[tokio::test(flavor = "multi_thread")]
async fn each_fault_of_a_write_stops_by_name_unsent_or_in_doubt_as_the_request_left() {
    let app = TestApp::start().await.unwrap();
    let mode = page_http_mode();
    let scene = scene("page-http-write-faults", &app, "", str::to_owned);
    let handle = scene.handle_of(&mode);
    let write = Write::new(&["A-100"]).await;

    // Bad input: a key that would leave its segment is refused before the
    // tab is sent anywhere.
    let (ran, prepared) = write_in(&scene, &write, &note("A-100/../A-101", "x")).await;
    let ran = ran.unwrap_err();
    assert_eq!(
        class_and_code(&ran),
        (FailureClass::Input, "address_value_refused")
    );
    assert_eq!(ran.sent, Sent::NotSent);
    handle.release(prepared, true).await;
    // A note the application refuses, by an answer the entry declares: in
    // doubt all the same, with what the application said, because the
    // browser may have sent the write twice and the refusal may answer the
    // second sending of one that landed (decision 0245, audit finding S3).
    // The stand shows nothing stored.
    let long = "x".repeat(201);
    let (ran, prepared) = write_in(&scene, &write, &note("A-100", &long)).await;
    let ran = ran.unwrap_err();
    assert_eq!(
        class_and_code(&ran),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert_eq!(ran.sent, Sent::Unknown);
    assert!(
        ran.failure
            .said()
            .is_some_and(|said| said.contains("200 characters"))
    );
    handle.release(prepared, true).await;
    let stored = app.stored_notes().await.unwrap();
    assert!(stored.writes.is_empty(), "{stored:?}");

    // A stale page: its token was used by the write it let through, and the
    // same page sends the next write with it. The application refuses the
    // token with the answer the entry declares. That is never taken for
    // "not written": a browser sends a write again by itself (below), and
    // the token may have been used by an earlier sending of the same
    // write. The write is in doubt, and the stop says why; the stand shows
    // that nothing more was stored.
    let (ran, mut prepared) = write_in(&scene, &write, &note("A-100", "First")).await;
    ran.unwrap();
    let put = &scene.binding_in(UPDATE, &mode).act[1];
    let inputs = note("A-100", "Second, from a stale page");
    let args = scene.args(put, &inputs);
    let (token, record, key) = write.authority();
    let input = StepInput::new(&args, Deadline::after(Duration::from_secs(60))).for_record(
        record,
        Some(key),
        None,
    );
    let stale = handle
        .act(&mut prepared, put, &input, Authority::Write(token))
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&stale),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert_eq!(stale.sent, Sent::Unknown);
    assert!(
        stale.failure.message().contains("anti-forgery token"),
        "{}",
        stale.failure.message()
    );
    handle.release(prepared, true).await;
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.notes["A-100"], "First");
    assert_eq!(stored.writes["A-100"], 1);
    assert_eq!(stored.forgery_refused, 1);

    // A connection lost after the write was sent: the application stored
    // it and closed the connection with no answer. The browser itself may
    // send the PUT again when the connection it reused closed so: measured
    // here, the stand sees a second PUT, which the used token makes it
    // refuse. Either way the write is in doubt, and the reconcile's read
    // decides; the driver sends nothing again, and the record is written
    // once.
    let puts = || async {
        app.seen_all()
            .await
            .unwrap()
            .iter()
            .filter(|line| *line == "PUT /api/orders/A-100/note")
            .count()
    };
    let puts_before = puts().await;
    let refused_before = app.stored_notes().await.unwrap().forgery_refused;
    app.set_fault(&Fault::once(FaultKind::LostAfterCommit).on_paths("/api/orders/A-100/note"))
        .await
        .unwrap();
    let (ran, mut prepared) =
        write_in(&scene, &write, &note("A-100", "Lost on the way back")).await;
    let lost = ran.unwrap_err();
    assert_eq!(
        class_and_code(&lost),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert_eq!(lost.sent, Sent::Unknown);
    let sent = puts().await - puts_before;
    assert!((1..=2).contains(&sent), "{sent}");
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.forgery_refused - refused_before, sent as u64 - 1);
    eprintln!("a PUT whose connection closed unanswered was sent {sent} times by the browser");
    let binding = scene.binding_in(UPDATE, &mode);
    let back = scene
        .run(
            &handle,
            &mut prepared,
            &binding.read_back,
            &note("A-100", "Lost on the way back"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        back.values.get("note"),
        Some(&Value::from("Lost on the way back"))
    );
    handle.release(prepared, true).await;
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.writes["A-100"], 2);
    assert_eq!(stored.notes["A-100"], "Lost on the way back");
    assert!(app.faults().await.unwrap().is_empty());

    // A run that may only look writes nothing: its tab cannot act, and the
    // committing step is refused before it is sent.
    let (_, mut prepared) = scene
        .try_prepare_in(
            &mode,
            Some(write.fence),
            Posture::Sandbox {
                test_records: false,
            },
        )
        .await
        .unwrap();
    let looked = scene
        .run(
            &handle,
            &mut prepared,
            &binding.act,
            &note("A-100", "Never"),
            Some(write.authority()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&looked),
        (FailureClass::Policy, "read_only_posture")
    );
    assert_eq!(looked.sent, Sent::NotSent);
    handle.release(prepared, true).await;
    assert_eq!(app.stored_notes().await.unwrap().writes["A-100"], 2);
    scene.stand.end().await;

    // An expired session before the write: the page is answered by the
    // sign-in page, and nothing is sent from it.
    let secure = self::scene("page-http-write-expired", &app, PUT_ENTRY, |text| {
        text.replace(
            "page: \"/orders/{order_id}/note\"",
            "page: \"/secure/orders\"",
        )
    });
    let (ran, prepared) =
        write_in(&secure, &write, &note("A-100", "After the session ended")).await;
    let expired = ran.unwrap_err();
    assert_eq!(
        class_and_code(&expired),
        (FailureClass::UnknownVariant, "unexpected_page")
    );
    assert_eq!(expired.sent, Sent::NotSent);
    secure.handle_of(&mode).release(prepared, true).await;
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.writes["A-100"], 2);
    assert_eq!(stored.notes["A-100"], "Lost on the way back");
    secure.stand.end().await;
}

/// Audit finding S4 (decision 0245): an answer that quotes the page's
/// token in a spelling the page's scrub does not know, here in capitals,
/// is withheld whole in the page. The write is in doubt, and no form of the
/// token reaches the failure, what the application said, or a log.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_that_quotes_the_token_in_any_spelling_is_withheld_whole() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("page-http-echo", &app, PUT_ENTRY, |text| {
        text.replace("header: X-CSRF-Token", "field: authenticity_token")
    });
    let write = Write::new(&["A-100"]).await;
    let (ran, prepared) = write_in(&scene, &write, &note("A-100", &"x".repeat(201))).await;
    let mut said = everything(&ran);
    let ran = ran.unwrap_err();
    assert_eq!(
        class_and_code(&ran),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert!(ran.failure.said().is_none(), "{:?}", ran.failure.said());
    scene
        .handle_of(&page_http_mode())
        .release(prepared, true)
        .await;
    said.extend(bytes_below(&scene.stand.folder.path().join("logs")));
    let upper = TOKEN_PREFIX.to_uppercase();
    assert!(!holds(&said, TOKEN_PREFIX));
    assert!(!holds(&said, &upper));
    // The stand refused the note, and its token was used up by it.
    let stored = app.stored_notes().await.unwrap();
    assert!(stored.writes.is_empty(), "{stored:?}");
    assert_eq!(stored.tokens_used, 1);
    scene.stand.end().await;
}
