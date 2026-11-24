//! Case 5, for the HTTP pack (decision 0357): a write that needs the
//! anti-forgery token of a page fetches the page in the same pass, takes the
//! one token the endpoint names, sends it once, and never quotes it. Each
//! write is proven by asking the test application what it stored, how often,
//! and how many tokens it gave and took back.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{FailureClass, NoSecrets, RecordKey, Value, Values};
use a2m_ledger::{Fence, IntentToken, Store};
use a2m_provider::{Posture, Sent, StepFailure};
use common::{
    Folder, Ran, Scene, compile_changed, key, record, registry, running, shown, store, token,
};
use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;

const LONG: Duration = Duration::from_secs(30);
const FORGERY: &str = "forgery";
const NOTES: &str = "endpoints/notes.yaml";
/// What every token the test application gives begins with.
const TOKEN_PREFIX: &str = "example-forgery-token-";

fn class_and_code(failure: &StepFailure) -> (FailureClass, String) {
    (
        failure.failure.class(),
        failure.failure.code().as_str().to_owned(),
    )
}

/// A write run for one record, with its intent stored: what the engine has
/// in hand when it reaches a commit point.
struct Write {
    _store: Store,
    fence: Fence,
    token: IntentToken,
    key: RecordKey,
}

async fn write_of(order: &str) -> Write {
    let (store, _vault) = store().await;
    let key = key(&[order]);
    let fence = running(&store, std::slice::from_ref(&key)).await;
    let token = token(&store, fence, 0, &key).await;
    Write {
        _store: store,
        fence,
        token,
        key,
    }
}

impl Write {
    async fn run(
        &self,
        scene: &Scene,
        handle: &a2m_provider::DriverHandle,
        prepared: &mut a2m_provider::Prepared,
        steps: &[a2m_core::Step],
        inputs: &Values,
    ) -> Result<Ran, StepFailure> {
        let write = Some((&self.token, record(&self.fence, 0), &self.key));
        scene
            .run(handle, prepared, steps, inputs, write, LONG)
            .await
    }
}

fn note(order: &str, text: &str) -> Values {
    Values::new().with("order_id", order).with("note", text)
}

async fn scene() -> (TestApp, Scene) {
    let app = TestApp::start().await.unwrap();
    let scene = Scene::new(FORGERY, &app.url(""));
    (app, scene)
}

/// The same target with one file changed.
fn changed(app: &TestApp, change: impl Fn(&str) -> String) -> Scene {
    let folder = Folder::new("forgery-changed");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let compiled_target = compile_changed(&registry, FORGERY, NOTES, change)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)))
        .compiled_target;
    Scene::over(folder, registry, compiled_target, &app.url(""))
}

fn kinds(steps: &[a2m_core::Step]) -> Vec<String> {
    steps.iter().map(|step| step.kind.to_string()).collect()
}

/// Whether anything in what a run returned holds a token, in any case.
fn holds_a_token(ran: &Ran) -> bool {
    let in_values = format!("{:?}", ran.values).to_ascii_lowercase();
    in_values.contains(TOKEN_PREFIX)
        || ran.captures.iter().any(|capture| {
            String::from_utf8_lossy(&capture.bytes)
                .to_ascii_lowercase()
                .contains(TOKEN_PREFIX)
        })
}

#[tokio::test]
async fn a_write_carries_the_token_its_page_gave_once_and_the_application_stored_it() {
    let (app, scene) = scene().await;

    // The three places a token is taken from and the two it is sent in.
    let cases = [
        ("example_note_put", "A-100", "Leave at the side door"),
        ("example_note_field_put", "A-101", "Ring twice"),
        ("example_note_cookie_put", "A-102", "Call on arrival"),
    ];
    for (operation, order, text) in cases {
        let binding = scene.binding(operation);
        // The page is fetched in the same pass, before the write, and only
        // the write is a commit point.
        assert_eq!(kinds(&binding.act), ["token", "request"], "{operation}");
        assert!(!binding.act[0].commits && binding.act[1].commits);
        assert!(!binding.act[0].lookup);

        let write = write_of(order).await;
        let (handle, mut prepared) = scene
            .try_prepare(None, Some(write.fence), Posture::Live)
            .await
            .unwrap();
        let inputs = note(order, text);
        let mut every_run = Vec::new();
        for steps in [&binding.act, &binding.confirm] {
            every_run.push(
                write
                    .run(&scene, &handle, &mut prepared, steps, &inputs)
                    .await
                    .unwrap_or_else(|failure| panic!("{operation}: {failure}")),
            );
        }
        let read_back = write
            .run(&scene, &handle, &mut prepared, &binding.read_back, &inputs)
            .await
            .unwrap();
        every_run.push(read_back);
        assert_eq!(
            every_run[2].values.get("note"),
            Some(&Value::from(text)),
            "{operation}"
        );
        // No token is in a value, a capture, or a trace of any step.
        assert!(
            every_run.iter().all(|ran| !holds_a_token(ran)),
            "{operation}"
        );

        // The token is used up: the write step alone cannot be sent again.
        let again = write
            .run(&scene, &handle, &mut prepared, &binding.act[1..], &inputs)
            .await
            .unwrap_err();
        assert_eq!(
            class_and_code(&again),
            (FailureClass::Stale, "token_not_read".to_owned())
        );
        assert_eq!(again.sent, Sent::NotSent);
        handle.release(prepared, false).await;
    }

    // What the application says it stored, and how often.
    let report = app.stored_notes().await.unwrap();
    for (_, order, text) in cases {
        assert_eq!(report.notes.get(order).map(String::as_str), Some(text));
        assert_eq!(report.writes.get(order), Some(&1), "{order}");
    }
    assert_eq!(report.forgery_refused, 0);
    assert_eq!(report.tokens_issued, 3, "one page for each write");
    assert_eq!(report.tokens_used, 3);
}

#[tokio::test]
async fn a_write_whose_page_gives_no_token_is_not_sent() {
    let (app, scene) = scene().await;
    let before = app.stored_notes().await.unwrap();
    let write = write_of("A-100").await;
    let steps = &scene.binding("example_note_put").act;
    let inputs = note("A-100", "Leave at the side door");

    // The page holds no meta element of the name the endpoint says.
    let wrong = changed(&app, |text| {
        text.replacen("meta: csrf-token", "meta: no-such-token", 1)
    });
    let wrong_steps = &wrong.binding("example_note_put").act;
    let (handle, mut prepared) = wrong
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let missing = write
        .run(&wrong, &handle, &mut prepared, wrong_steps, &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&missing),
        (FailureClass::Drift, "anti_forgery_not_found".to_owned())
    );
    assert_eq!(missing.sent, Sent::NotSent);

    // A page that is not there: the order does not exist.
    let none = write
        .run(
            &wrong,
            &handle,
            &mut prepared,
            wrong_steps,
            &note("A-999", "x"),
        )
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&none),
        (FailureClass::Drift, "anti_forgery_not_found".to_owned())
    );
    handle.release(prepared, false).await;

    // A limit in front of the page is the edge's, and is not tried again.
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    app.set_fault(&Fault::once(FaultKind::Status { status: 429 }).on_paths("/orders"))
        .await
        .unwrap();
    let limited = write
        .run(&scene, &handle, &mut prepared, steps, &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&limited),
        (FailureClass::EdgeBlocked, "rate_limited_by_edge".to_owned())
    );
    assert_eq!(limited.sent, Sent::NotSent);
    handle.release(prepared, false).await;

    // Three pages were asked for, none was a write, and nothing is stored.
    let report = app.stored_notes().await.unwrap();
    assert_eq!(report.notes, before.notes);
    assert!(report.writes.is_empty(), "{report:?}");
    assert_eq!((report.forgery_refused, report.tokens_used), (0, 0));
}

#[tokio::test]
async fn what_the_application_refuses_after_the_token_is_named_and_never_quotes_the_token() {
    let (app, scene) = scene().await;
    let before = app.stored_notes().await.unwrap();
    let too_long = "x".repeat(201);

    // The token goes in the header, and in the body: the application quotes
    // the body it refused, with the token in capitals, and none comes out.
    for (operation, order) in [
        ("example_note_put", "A-100"),
        ("example_note_field_put", "A-101"),
    ] {
        let write = write_of(order).await;
        let (handle, mut prepared) = scene
            .try_prepare(None, Some(write.fence), Posture::Live)
            .await
            .unwrap();
        let binding = scene.binding(operation);
        // The write is sent and answered; the refusal is read by the step
        // that judges the answer.
        let inputs = note(order, &too_long);
        write
            .run(&scene, &handle, &mut prepared, &binding.act, &inputs)
            .await
            .unwrap();
        let refused = write
            .run(&scene, &handle, &mut prepared, &binding.confirm, &inputs)
            .await
            .unwrap_err();
        handle.release(prepared, false).await;
        assert_eq!(
            class_and_code(&refused),
            (FailureClass::Rejected, "app_validation_error".to_owned()),
            "{operation}"
        );
        assert_eq!(refused.sent, Sent::Answered);
        let said = format!(
            "{refused} {} {}",
            refused.failure.message(),
            refused.failure.said().unwrap_or_default()
        )
        .to_ascii_lowercase();
        assert!(!said.contains("forgery-token"), "{operation}: {said}");
    }
    // The application stored nothing, and took both tokens back.
    let report = app.stored_notes().await.unwrap();
    assert_eq!(report.notes, before.notes);
    assert!(report.writes.is_empty(), "{report:?}");
    assert_eq!((report.tokens_issued, report.tokens_used), (2, 2));
    assert_eq!(report.forgery_refused, 0);
}

#[tokio::test]
async fn a_write_whose_answer_is_lost_is_in_doubt_and_is_not_sent_again_with_its_token() {
    let (app, scene) = scene().await;
    let write = write_of("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let binding = scene.binding("example_note_put");
    let inputs = note("A-100", "Leave at the side door");
    app.set_fault(&Fault::once(FaultKind::LostAfterCommit).on_paths("/api/orders"))
        .await
        .unwrap();
    let lost = write
        .run(&scene, &handle, &mut prepared, &binding.act, &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&lost),
        (FailureClass::PossibleCommit, "no_confirmation".to_owned())
    );
    assert_eq!(lost.sent, Sent::Unknown);

    // The write step on its own is refused: the token was sent with the
    // write, and a second page is a second pass.
    let again = write
        .run(&scene, &handle, &mut prepared, &binding.act[1..], &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&again),
        (FailureClass::Stale, "token_not_read".to_owned())
    );
    handle.release(prepared, true).await;

    // The application did store the write, once; and took one token.
    let report = app.stored_notes().await.unwrap();
    assert_eq!(
        report.notes.get("A-100").map(String::as_str),
        Some("Leave at the side door")
    );
    assert_eq!(report.writes.get("A-100"), Some(&1));
    assert_eq!((report.tokens_issued, report.tokens_used), (1, 1));
}

#[tokio::test]
async fn without_the_ledgers_token_the_page_is_read_and_nothing_is_written() {
    let (app, scene) = scene().await;
    let before = app.stored_notes().await.unwrap();
    let (handle, mut prepared) = scene.reader().await;
    let steps = &scene.binding("example_note_put").act;
    let refused = scene
        .run(
            &handle,
            &mut prepared,
            steps,
            &note("A-100", "Leave at the side door"),
            None,
            LONG,
        )
        .await
        .unwrap_err();
    handle.release(prepared, false).await;
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Policy, "intent_missing".to_owned())
    );
    assert_eq!(refused.sent, Sent::NotSent);
    let report = app.stored_notes().await.unwrap();
    assert_eq!(report.notes, before.notes);
    assert!(report.writes.is_empty(), "{report:?}");
    assert_eq!((report.tokens_issued, report.tokens_used), (1, 0));
}

/// Compiles the target with its note endpoint changed, and gives the first
/// problem with the code given.
fn refused(change: impl Fn(&str) -> String, code: &str) -> a2m_files::Diagnostic {
    let folder = Folder::new("forgery-refused");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let problems = compile_changed(&registry, FORGERY, NOTES, change)
        .err()
        .unwrap_or_else(|| panic!("the change compiled; `{code}` was expected"));
    problems
        .find(code)
        .unwrap_or_else(|| panic!("no `{code}` in:\n{}", shown(&problems)))
        .clone()
}

#[test]
fn a_bootstrap_that_cannot_work_is_refused_with_a_file_a_line_and_a_fix() {
    let rule = "http.anti_forgery_invalid";
    let page = "  page: \"/orders/{order_id}/note\"\n";
    for (what, change) in [
        (
            "a page placeholder the endpoint's path lacks",
            Box::new(|text: &str| {
                text.replacen("page: \"/orders/{order_id}", "page: \"/orders/{other}", 1)
            }) as Box<dyn Fn(&str) -> String>,
        ),
        (
            "no page",
            Box::new(move |text: &str| text.replacen(page, "", 1)),
        ),
        (
            "a header the session owns",
            Box::new(|text: &str| text.replacen("header: X-CSRF-Token", "header: Cookie", 1)),
        ),
        (
            "a header and a field",
            Box::new(|text: &str| {
                text.replacen(
                    "header: X-CSRF-Token",
                    "header: X-CSRF-Token\n  field: tok",
                    1,
                )
            }),
        ),
        (
            "two places on the page",
            Box::new(|text: &str| {
                text.replacen(
                    "from: {meta: csrf-token}",
                    "from: {meta: csrf-token, input: tok}",
                    1,
                )
            }),
        ),
        (
            "a place the page cannot hold a token in",
            Box::new(|text: &str| {
                text.replacen("from: {meta: csrf-token}", "from: {storage: csrf}", 1)
            }),
        ),
        (
            "a field the body already declares",
            Box::new(|text: &str| text.replacen("field: authenticity_token", "field: note", 1)),
        ),
        (
            "a key of the page_http form that has no meaning here",
            Box::new(|text: &str| {
                text.replacen("request_schema:", "  refused: []\nrequest_schema:", 1)
            }),
        ),
    ] {
        let found = refused(change, rule);
        assert_eq!(found.at.file.as_str(), NOTES, "{what}");
        assert!(!found.fix.is_empty(), "{what}");
    }

    // `none` says the endpoint checks no token: the write is one step.
    let folder = Folder::new("forgery-none");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let none = compile_changed(&registry, FORGERY, NOTES, |text| {
        text.replacen(
            "anti_forgery:\n  page: \"/orders/{order_id}/note\"\n  from: {meta: csrf-token}\n  header: X-CSRF-Token\n",
            "anti_forgery: none\n",
            1,
        )
    })
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    let operation = none
        .compiled_target
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_note_put")
        .unwrap();
    assert_eq!(kinds(&operation.bindings[0].act), ["request"]);
}
