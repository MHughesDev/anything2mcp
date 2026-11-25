//! A read binding that sends a declared body (decision 0392, point 6;
//! decision 0999): a search that takes its filter in the body of a `POST`,
//! asserted to change nothing. The body is built only from the binding's
//! map, one expression for each field the endpoint's closed
//! `request_schema` declares; it is checked against that schema, its `enum`
//! included, before it is sent, in every posture; and outside a live run it
//! is sent only from a published version against an environment of kind
//! `test`. Against the test application's `POST /api/cases/rows/count`,
//! which counts with `op: count` and archives with `op: archive`; every
//! proof asks the application's control interface what it holds and what
//! it heard.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{Environment, FailureClass, Value, Values};
use a2m_provider::{Posture, Sent, StepFailure};
use common::{Folder, Scene, compile, compile_changed, registry, shown};
use example_app::harness::TestApp;

const FIXTURE: &str = "bodies";
const ENDPOINT: &str = "endpoints/cases.yaml";
const OPERATION: &str = "operations/cases.yaml";
const COUNT: &str = "example_case_count";
const ROUTE: &str = "POST /api/cases/rows/count";

fn class_and_code(failure: &StepFailure) -> (FailureClass, String) {
    (
        failure.failure.class(),
        failure.failure.code().as_str().to_owned(),
    )
}

fn scene_in(app: &TestApp, environment: &str) -> Scene {
    let mut scene = Scene::new(FIXTURE, &app.url(""));
    scene.surface.environment = Environment::new(environment).unwrap();
    scene
}

/// The scene over a fixture with one file changed.
fn changed_scene(app: &TestApp, path: &str, change: impl Fn(&str) -> String) -> Scene {
    let folder = Folder::new("bodies-changed");
    let registry = registry(&folder, std::sync::Arc::new(a2m_core::NoSecrets));
    let compiled = compile_changed(&registry, FIXTURE, path, change)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    Scene::over(folder, registry, compiled.compiled_target, &app.url(""))
}

/// The compiled target as a published version's, which the driver tells by
/// its version (decision 0270).
fn published(mut scene: Scene) -> Scene {
    scene.compiled_target.header.version = a2m_core::Version::new(1, 0, 0);
    scene
}

async fn count_in(scene: &Scene, posture: Posture, inputs: &Values) -> Result<Values, StepFailure> {
    let (handle, mut prepared) = scene.try_prepare(None, None, posture).await.unwrap();
    let read = scene.read(&handle, &mut prepared, COUNT, inputs).await;
    handle.release(prepared, false).await;
    read.map(|ran| ran.values)
}

/// What the control interface holds for a status: how many rows, the first
/// key, and every key.
async fn held(app: &TestApp, status: &str) -> (i64, String, Vec<String>) {
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
        i64::try_from(cases.len()).unwrap(),
        cases.first().cloned().unwrap_or_default(),
        cases,
    )
}

fn as_read(values: &Values) -> (i64, String, Vec<String>) {
    let Some(Value::Int(count)) = values.get("count") else {
        panic!("no count in {values:?}");
    };
    let Some(Value::Text(first)) = values.get("first") else {
        panic!("no first in {values:?}");
    };
    let Some(Value::List(cases)) = values.get("cases") else {
        panic!("no cases in {values:?}");
    };
    let cases = cases
        .iter()
        .map(|case| match case {
            Value::Text(text) => text.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    (*count, first.clone(), cases)
}

async fn heard(app: &TestApp) -> (usize, Vec<String>) {
    let seen = app.seen().await.unwrap();
    (
        seen.iter().filter(|line| *line == ROUTE).count(),
        app.case_rows().await.unwrap().received,
    )
}

#[test]
fn a_read_sends_a_body_only_to_an_asserted_endpoint_whose_schema_names_every_field() {
    let folder = Folder::new("bodies-compile");
    let registry = registry(&folder, std::sync::Arc::new(a2m_core::NoSecrets));
    let compiled = compile(&registry, FIXTURE);
    let operation = compiled
        .compiled_target
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == COUNT)
        .unwrap();
    let request = &operation.bindings[0].act[0];
    let names: Vec<&str> = request.args.iter().map(|arg| arg.name.as_str()).collect();
    assert_eq!(names, ["body.status", "body.op"]);
    assert!(!request.commits);

    let refused = |path: &str, change: &dyn Fn(&str) -> String, rule: &str| {
        let problems = compile_changed(&registry, FIXTURE, path, change)
            .err()
            .unwrap_or_else(|| panic!("`{rule}`: it compiled"));
        assert!(
            problems.find(rule).is_some(),
            "{rule}:\n{}",
            shown(&problems)
        );
    };
    // A read by `GET` sends no body.
    refused(
        ENDPOINT,
        &|text| {
            let schema = text.find("request_schema:").unwrap();
            let answer = text.find("response_schema:").unwrap();
            format!("{}{}", &text[..schema], &text[answer..])
                .replace("method: POST", "method: GET")
                .replace("read_asserted: true\n", "")
        },
        "http.read_body_unasserted",
    );
    // A body the assertion leaves open would carry fields nobody named.
    refused(
        ENDPOINT,
        &|text| text.replace("additionalProperties: false", "additionalProperties: true"),
        "http.read_body_unasserted",
    );
    refused(
        ENDPOINT,
        &|text| text.replace("  additionalProperties: false\n", ""),
        "http.read_body_unasserted",
    );
    // No field the endpoint does not declare, and every one it requires.
    refused(
        OPERATION,
        &|text| {
            text.replace(
                "      op: \"'count'\"\n",
                "      op: \"'count'\"\n      extra: input.status\n",
            )
        },
        "http.body_undeclared",
    );
    refused(
        OPERATION,
        &|text| text.replace("      op: \"'count'\"\n", ""),
        "http.body_field_missing",
    );
    refused(
        OPERATION,
        &|text| {
            text.replace(
                "    body:\n      status: input.status\n      op: \"'count'\"\n",
                "",
            )
        },
        "http.body_field_missing",
    );
    // Each value of the kind its field declares.
    refused(
        ENDPOINT,
        &|text| {
            text.replace(
                "status: {type: string, enum: [open, closed]}",
                "status: {type: integer}",
            )
        },
        "http.position_type",
    );
}

/// The search returns, live and in a try or a shadow of a published version
/// against an environment of kind `test`, what the control interface holds,
/// and the application hears one count for each read and no archive.
#[tokio::test]
async fn a_search_that_takes_its_filter_in_a_body_returns_what_the_application_holds() {
    let app = TestApp::start().await.unwrap();
    let test = published(scene_in(&app, "test"));
    let production = published(scene_in(&app, "production"));
    for (scene, posture) in [
        (&test, Posture::Live),
        (&production, Posture::Live),
        (
            &test,
            Posture::Sandbox {
                test_records: false,
            },
        ),
        (&test, Posture::Sandbox { test_records: true }),
    ] {
        app.reset().await.unwrap();
        for status in ["open", "closed"] {
            let found = count_in(scene, posture, &Values::new().with("status", status))
                .await
                .unwrap_or_else(|failure| panic!("{posture:?} {status}: {failure:?}"));
            assert_eq!(as_read(&found), held(&app, status).await, "{posture:?}");
        }
        assert_eq!(
            heard(&app).await,
            (2, vec!["count".to_owned(), "count".to_owned()]),
            "{posture:?}"
        );
    }

    // What the read returns follows what the application holds.
    app.reset().await.unwrap();
    let archived = reqwest::Client::new()
        .post(app.url("/api/cases/rows/count"))
        .json(&serde_json::json!({"status": "open", "op": "archive"}))
        .send()
        .await
        .unwrap();
    assert!(archived.status().is_success());
    let found = count_in(&test, Posture::Live, &Values::new().with("status", "open"))
        .await
        .unwrap();
    let after = held(&app, "open").await;
    assert_eq!(after.0, 1);
    assert_eq!(as_read(&found), after);
}

/// Under the drafts, outside a live run, and in a discovery, nothing is
/// sent: the application hears nothing.
#[tokio::test]
async fn under_the_drafts_the_search_is_not_sent_and_the_application_hears_nothing() {
    let app = TestApp::start().await.unwrap();
    let drafted = scene_in(&app, "test");
    let production = published(scene_in(&app, "production"));
    let test = published(scene_in(&app, "test"));
    for (scene, posture) in [
        (
            &drafted,
            Posture::Sandbox {
                test_records: false,
            },
        ),
        (&drafted, Posture::Sandbox { test_records: true }),
        (&drafted, Posture::Discovery),
        (
            &production,
            Posture::Sandbox {
                test_records: false,
            },
        ),
        (&test, Posture::Discovery),
    ] {
        app.reset().await.unwrap();
        let seeded = app.case_rows().await.unwrap();
        let failure = count_in(scene, posture, &Values::new().with("status", "open"))
            .await
            .unwrap_err();
        assert_eq!(
            class_and_code(&failure),
            (FailureClass::Policy, "read_only_posture".to_owned()),
            "{posture:?}"
        );
        assert_eq!(failure.sent, Sent::NotSent, "{posture:?}");
        assert_eq!(heard(&app).await, (0, Vec::new()), "{posture:?}");
        assert_eq!(app.case_rows().await.unwrap(), seeded, "{posture:?}");
    }
}

/// No input changes the body past what the person asserted: a value outside
/// a field's `enum` is refused before it is sent, live as well, and the
/// application hears nothing and stores nothing.
#[tokio::test]
async fn a_value_the_assertion_does_not_name_is_refused_before_it_is_sent_in_any_posture() {
    let app = TestApp::start().await.unwrap();
    let test = published(scene_in(&app, "test"));
    // A field written from an input: only its `enum`'s texts go out.
    let by_input = published(changed_scene(&app, OPERATION, |text| {
        text.replace("op: \"'count'\"", "op: input.op")
            .replace(
                "  status: {type: identifier, description: \"open or closed\"}\n",
                "  status: {type: identifier, description: \"open or closed\"}\n  op: {type: identifier}\n",
            )
    }));
    for (scene, inputs) in [
        (&test, Values::new().with("status", "archived")),
        (
            &test,
            Values::new().with("status", "open\",\"op\":\"archive"),
        ),
        (
            &by_input,
            Values::new().with("status", "open").with("op", "archive"),
        ),
    ] {
        app.reset().await.unwrap();
        let seeded = app.case_rows().await.unwrap();
        let failure = count_in(scene, Posture::Live, &inputs).await.unwrap_err();
        assert_eq!(
            class_and_code(&failure),
            (FailureClass::Policy, "body_not_asserted".to_owned()),
            "{inputs:?}"
        );
        assert_eq!(failure.sent, Sent::NotSent);
        assert!(
            !failure.failure.message().contains("archive"),
            "{}",
            failure.failure.message()
        );
        assert_eq!(heard(&app).await, (0, Vec::new()));
        assert_eq!(app.case_rows().await.unwrap(), seeded);
    }
    // The same binding with a value its `enum` names reads.
    app.reset().await.unwrap();
    let found = count_in(
        &by_input,
        Posture::Live,
        &Values::new().with("status", "closed").with("op", "count"),
    )
    .await
    .unwrap();
    assert_eq!(as_read(&found), held(&app, "closed").await);
}
