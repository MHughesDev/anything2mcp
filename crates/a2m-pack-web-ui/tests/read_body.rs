//! A `page_http` read that sends a declared body (decision 0392, point 6;
//! decision 0999): from inside a page of the application, by the asserted
//! read's own method, with a body built only from the binding's map, one
//! expression for each field the entry's closed `request_schema` declares.
//! The driver asks it only where the run hands the observing tab that entry,
//! as decisions 0410 and 0475 say, and only with a body that fits the entry,
//! its `enum` included; the tab's guard judges it again. Against the test
//! application's `POST /api/cases/rows/count`, which counts with
//! `op: count` and archives with `op: archive`, from its quiet page
//! `/cases/rows-count`; every proof asks the control interface what it holds
//! and what it heard.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{CompiledTarget, Environment, FailureClass, NoSecrets, Value, Values};
use a2m_provider::{ConfirmedReads, Posture, Sent, StepFailure};
use common::{Scene, Stand, compile, compile_changed, page_http_mode, shown};
use example_app::harness::TestApp;

const FIXTURE: &str = "counts";
const ENTRY: &str = "page_requests/cases_rows_count.yaml";
const OPERATION: &str = "operations/counts.yaml";
const COUNT: &str = "example_case_count";
const ROUTE: &str = "POST /api/cases/rows/count";

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

/// The same compiled target as a published version's.
fn published(mut compiled_target: CompiledTarget) -> CompiledTarget {
    compiled_target.header.version = a2m_core::Version::new(1, 0, 0);
    compiled_target
}

fn count_confirmed() -> ConfirmedReads {
    ConfirmedReads::of([(
        "page_request".to_owned(),
        "app".to_owned(),
        "cases_rows_count".to_owned(),
    )])
}

/// A `page_http` count, prepared in `posture` in the environment named.
async fn count(
    app: &TestApp,
    compiled_target: CompiledTarget,
    environment: &str,
    posture: Posture,
    confirmed: ConfirmedReads,
    inputs: &Values,
) -> Result<common::Ran, StepFailure> {
    let mode = page_http_mode();
    let stand = Stand::new("read-body", Arc::new(NoSecrets));
    let mut scene = Scene::over(stand, compiled_target, &app.url(""));
    scene.surface.environment = Environment::new(environment).unwrap();
    scene.confirmed = confirmed;
    let (handle, mut prepared) = scene
        .try_prepare_in(&mode, None, posture)
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    let ran = scene
        .read_in(&mode, &handle, &mut prepared, COUNT, inputs)
        .await;
    assert!(handle.release(prepared, ran.is_err()).await.complete);
    scene.stand.end().await;
    ran
}

/// What the control interface holds for a status: how many rows, and the
/// first key.
async fn held(app: &TestApp, status: &str) -> (Value, Value) {
    let cases: Vec<String> = app
        .case_rows()
        .await
        .unwrap()
        .rows
        .into_iter()
        .filter(|row| row.status == status)
        .map(|row| row.case_id)
        .collect();
    (
        Value::Int(i64::try_from(cases.len()).unwrap()),
        Value::from(cases.first().map_or("", String::as_str)),
    )
}

fn as_read(ran: &common::Ran) -> (Value, Value) {
    (
        ran.values.get("count").cloned().unwrap(),
        ran.values.get("first").cloned().unwrap(),
    )
}

async fn heard(app: &TestApp) -> (usize, Vec<String>) {
    let seen = app.seen().await.unwrap();
    (
        seen.iter().filter(|line| *line == ROUTE).count(),
        app.case_rows().await.unwrap().received,
    )
}

#[test]
fn a_page_http_read_sends_a_body_only_to_an_asserted_read_and_only_its_fields() {
    let stand = Stand::new("read-body-compile", Arc::new(NoSecrets));
    let compiled = compile(&stand.registry, FIXTURE).compiled_target;
    let operation = compiled
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == COUNT)
        .unwrap();
    let request = &operation.bindings[0].act[1];
    let names: Vec<&str> = request.args.iter().map(|arg| arg.name.as_str()).collect();
    assert_eq!(names, ["body.status", "body.op"]);
    assert!(!request.commits);

    let refused = |path: &str, change: &dyn Fn(&str) -> String, rule: &str| {
        let problems = compile_changed(&stand.registry, FIXTURE, path, change)
            .err()
            .unwrap_or_else(|| panic!("`{rule}`: it compiled"));
        assert!(
            problems.find(rule).is_some(),
            "{rule}:\n{}",
            shown(&problems)
        );
    };
    // Every field the entry requires, and none it does not declare.
    refused(
        OPERATION,
        &|text| {
            text.replace(
                "    body:\n      status: input.status\n      op: \"'count'\"\n",
                "",
            )
        },
        "web.body_incomplete",
    );
    refused(
        OPERATION,
        &|text| {
            text.replace(
                "      op: \"'count'\"\n",
                "      op: \"'count'\"\n      extra: input.status\n",
            )
        },
        "web.body_undeclared",
    );
    // Each value of its field's kind.
    refused(
        ENTRY,
        &|text| {
            text.replace(
                "status: {type: string, enum: [open, closed]}",
                "status: {type: integer}",
            )
        },
        "web.value_type",
    );
    // A read by `GET`, or an asserted read that declares no body, sends none.
    refused(
        ENTRY,
        &|text| {
            let at = text.find("request_encoding:").unwrap();
            let end = text.find("response_schema:").unwrap();
            format!("{}{}", &text[..at], &text[end..])
        },
        "web.read_acts",
    );
    refused(
        ENTRY,
        &|text| {
            let at = text.find("read_asserted:").unwrap();
            let end = text.find("response_schema:").unwrap();
            format!("{}{}", &text[..at], &text[end..]).replace("method: POST", "method: GET")
        },
        "web.read_acts",
    );
}

/// Live, and in a try or a shadow of a published version in an environment
/// of kind `test`, the count returns what the control interface holds, and
/// the application hears one count for each read and no archive.
#[tokio::test(flavor = "multi_thread")]
async fn a_search_that_takes_its_filter_in_a_body_returns_from_inside_the_page_what_the_application_holds()
 {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::new("read-body-targets", Arc::new(NoSecrets));
    let draft = compile(&stand.registry, FIXTURE).compiled_target;
    let sandbox = Posture::Sandbox {
        test_records: false,
    };
    for (compiled_target, environment, posture, confirmed) in [
        (
            published(draft.clone()),
            "test",
            Posture::Live,
            ConfirmedReads::none(),
        ),
        (
            published(draft.clone()),
            "production",
            Posture::Live,
            ConfirmedReads::none(),
        ),
        (
            published(draft.clone()),
            "test",
            sandbox,
            ConfirmedReads::none(),
        ),
        // Under the drafts, a try in `test` of the entry its person
        // confirmed (decision 0475).
        (draft.clone(), "test", sandbox, count_confirmed()),
    ] {
        app.reset().await.unwrap();
        for status in ["open", "closed"] {
            let ran = count(
                &app,
                compiled_target.clone(),
                environment,
                posture,
                confirmed.clone(),
                &Values::new().with("status", status),
            )
            .await
            .unwrap_or_else(|failure| panic!("{environment} {posture:?} {status}: {failure}"));
            assert_eq!(as_read(&ran), held(&app, status).await, "{posture:?}");
        }
        assert_eq!(
            heard(&app).await,
            (2, vec!["count".to_owned(), "count".to_owned()]),
            "{environment} {posture:?}"
        );
    }
    assert!(app.writes().await.unwrap().is_empty());
    stand.end().await;
}

/// Under the drafts unconfirmed, outside the places a published version's
/// asserted read is handed, and in a discovery, nothing is sent: the stop
/// names the entry, and the application hears nothing.
#[tokio::test(flavor = "multi_thread")]
async fn under_the_drafts_the_page_sends_nothing_and_the_stop_names_the_entry() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::new("read-body-drafts", Arc::new(NoSecrets));
    let draft = compile(&stand.registry, FIXTURE).compiled_target;
    let sandbox = Posture::Sandbox {
        test_records: false,
    };
    for (compiled_target, environment, posture, confirmed, next) in [
        (
            draft.clone(),
            "test",
            Posture::Live,
            ConfirmedReads::none(),
            "assertion.confirm",
        ),
        (
            draft.clone(),
            "test",
            sandbox,
            ConfirmedReads::none(),
            "assertion.confirm",
        ),
        (
            draft.clone(),
            "production",
            sandbox,
            count_confirmed(),
            "assertion.confirm",
        ),
        (
            draft.clone(),
            "test",
            Posture::Discovery,
            count_confirmed(),
            "assertion.confirm",
        ),
        (
            published(draft.clone()),
            "production",
            sandbox,
            ConfirmedReads::none(),
            "kind `test`",
        ),
    ] {
        app.reset().await.unwrap();
        let seeded = app.case_rows().await.unwrap();
        let stopped = count(
            &app,
            compiled_target,
            environment,
            posture,
            confirmed,
            &Values::new().with("status", "open"),
        )
        .await
        .unwrap_err();
        assert_eq!(
            class_and_code(&stopped),
            (FailureClass::Policy, "page_write_refused"),
            "{environment} {posture:?}: {stopped}"
        );
        assert_eq!(stopped.sent, Sent::NotSent, "{environment} {posture:?}");
        let said = stopped.failure.next_step();
        assert!(
            said.contains("cases_rows_count") && said.contains(next),
            "{environment} {posture:?}: {said}"
        );
        assert_eq!(
            heard(&app).await,
            (0, Vec::new()),
            "{environment} {posture:?}"
        );
        assert_eq!(app.case_rows().await.unwrap(), seeded);
    }
    stand.end().await;
}

/// No input changes the body past what the person asserted: a value outside
/// a field's `enum` is refused before it is asked, live as well, and the
/// application hears nothing and stores nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_value_the_assertion_does_not_name_is_never_asked_from_the_page() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::new("read-body-values", Arc::new(NoSecrets));
    let draft = published(compile(&stand.registry, FIXTURE).compiled_target);
    let by_input = published(
        compile_changed(&stand.registry, FIXTURE, OPERATION, |text| {
            text.replace("op: \"'count'\"", "op: input.op").replace(
                "  status: {type: text, description: \"open or closed\"}\n",
                "  status: {type: text, description: \"open or closed\"}\n  op: {type: text}\n",
            )
        })
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)))
        .compiled_target,
    );
    for (compiled_target, inputs) in [
        (draft.clone(), Values::new().with("status", "archived")),
        (
            draft.clone(),
            Values::new().with("status", "open\",\"op\":\"archive"),
        ),
        (
            by_input.clone(),
            Values::new().with("status", "open").with("op", "archive"),
        ),
    ] {
        app.reset().await.unwrap();
        let seeded = app.case_rows().await.unwrap();
        let stopped = count(
            &app,
            compiled_target,
            "test",
            Posture::Live,
            ConfirmedReads::none(),
            &inputs,
        )
        .await
        .unwrap_err();
        assert_eq!(
            class_and_code(&stopped),
            (FailureClass::Policy, "page_write_refused"),
            "{inputs:?}: {stopped}"
        );
        assert_eq!(stopped.sent, Sent::NotSent);
        assert!(!stopped.failure.message().contains("archive"), "{stopped}");
        assert_eq!(heard(&app).await, (0, Vec::new()), "{inputs:?}");
        assert_eq!(app.case_rows().await.unwrap(), seeded);
    }
    // The same binding with values its `enum`s name reads.
    app.reset().await.unwrap();
    let ran = count(
        &app,
        by_input,
        "test",
        Posture::Live,
        ConfirmedReads::none(),
        &Values::new().with("status", "closed").with("op", "count"),
    )
    .await
    .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(as_read(&ran), held(&app, "closed").await);
    stand.end().await;
}
