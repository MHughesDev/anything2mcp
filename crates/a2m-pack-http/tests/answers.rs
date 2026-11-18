//! What a safety audit found in the HTTP pack (decision 0249), each proven
//! against the test application through the driver's handle: a hostile
//! answer is refused in time, a request that may write is sent only in a
//! posture that may write, a value is data with no control character in
//! it, and a body that holds a registered secret gives nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use a2m_core::{Decimal, Environment, FailureClass, NoSecrets, REDACTED, Redact, Value, Values};
use a2m_provider::{Posture, Sent, StepFailure};
use common::{Scene, key, record, running, store, token};
use example_app::answers::ECHOED_SECRET;
use example_app::harness::TestApp;

const ANSWERS: &str = "answers";
const LONG: Duration = Duration::from_secs(30);

fn class_and_code(failure: &StepFailure) -> (FailureClass, String) {
    (
        failure.failure.class(),
        failure.failure.code().as_str().to_owned(),
    )
}

/// A redactor that knows one secret.
struct One(&'static str);

impl Redact for One {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, REDACTED)
    }
}

/// The `answers` scene, in the environment named.
fn scene_in(app: &TestApp, environment: &str, redact: Arc<dyn Redact>) -> Scene {
    let mut scene = Scene::with_redactor(ANSWERS, &app.url(""), redact);
    scene.surface.environment = Environment::new(environment).unwrap();
    scene
}

async fn read_in(
    scene: &Scene,
    posture: Posture,
    operation: &str,
    inputs: &Values,
) -> Result<Values, StepFailure> {
    let (handle, mut prepared) = scene.try_prepare(None, None, posture).await.unwrap();
    let read = scene.read(&handle, &mut prepared, operation, inputs).await;
    handle.release(prepared, false).await;
    read.map(|ran| ran.values)
}

/// The audit's document, through the whole driver: refused for the length
/// of its namespace's address, long before the step's time is out. It was
/// once a copy of 4 MB for each of 99,999 elements, on an async thread.
#[tokio::test]
async fn an_answer_whose_namespace_address_is_past_its_bound_is_refused_in_time() {
    let app = TestApp::start().await.unwrap();
    let scene = scene_in(&app, "test", Arc::new(NoSecrets));
    let began = Instant::now();
    let failure = read_in(
        &scene,
        Posture::Live,
        "example_namespace_get",
        &Values::new(),
    )
    .await
    .unwrap_err();
    assert!(
        began.elapsed() < Duration::from_secs(20),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(
        class_and_code(&failure),
        (
            FailureClass::UnknownVariant,
            "response_too_large".to_owned()
        )
    );
    assert!(failure.failure.message().contains("namespace's address"));
}

const DRAFTS: &[&str] = &["A-100", "A-102"];

fn drafts() -> Value {
    Value::List(DRAFTS.iter().map(|id| Value::from(*id)).collect())
}

/// The same scene, its plan carrying a release version: the plan of a
/// published version, as the driver sees one. A draft is always compiled
/// as the draft version, `0.0.0` (decision 0161).
fn published(mut scene: Scene) -> Scene {
    scene.plan.header.version = a2m_core::Version::new(1, 0, 0);
    scene
}

/// A search behind `POST`, which the developer asserts only reads: sent in
/// a live run anywhere, and outside one only when the plan is a published
/// version's and the run's environment is of kind `test` in it. Under the
/// drafts, whose environment kind and assertion are the drafts' own word,
/// it is not sent outside a live run anywhere (decision 0270). A discovery
/// sends it nowhere. A request the guard refuses never reaches the system.
#[tokio::test]
async fn a_request_that_may_write_is_sent_only_where_the_posture_lets_it() {
    let app = TestApp::start().await.unwrap();
    let search = Values::new().with("status", "draft");
    let drafted_test = scene_in(&app, "test", Arc::new(NoSecrets));
    let drafted_production = scene_in(&app, "production", Arc::new(NoSecrets));
    let test = published(scene_in(&app, "test", Arc::new(NoSecrets)));
    let production = published(scene_in(&app, "production", Arc::new(NoSecrets)));
    let searched = |seen: Vec<String>| {
        seen.iter()
            .filter(|line| *line == "POST /views/search/orders")
            .count()
    };

    for (scene, posture) in [
        (&production, Posture::Live),
        (&test, Posture::Live),
        (&drafted_production, Posture::Live),
        (&drafted_test, Posture::Live),
        (
            &test,
            Posture::Sandbox {
                test_records: false,
            },
        ),
        (&test, Posture::Sandbox { test_records: true }),
    ] {
        app.reset().await.unwrap();
        let found = read_in(scene, posture, "example_order_search", &search)
            .await
            .unwrap_or_else(|failure| panic!("{posture:?}: {failure:?}"));
        assert_eq!(found.get("orders"), Some(&drafts()), "{posture:?}");
        assert_eq!(searched(app.seen().await.unwrap()), 1, "{posture:?}");
    }

    for (scene, posture) in [
        (
            &production,
            Posture::Sandbox {
                test_records: false,
            },
        ),
        (&production, Posture::Sandbox { test_records: true }),
        (&production, Posture::Discovery),
        (&test, Posture::Discovery),
        // The drafts' word that the environment is a test copy, and that
        // the request only reads, is not enough outside a live run.
        (
            &drafted_test,
            Posture::Sandbox {
                test_records: false,
            },
        ),
        (&drafted_test, Posture::Sandbox { test_records: true }),
        (&drafted_test, Posture::Discovery),
    ] {
        app.reset().await.unwrap();
        let failure = read_in(scene, posture, "example_order_search", &search)
            .await
            .unwrap_err();
        assert_eq!(
            class_and_code(&failure),
            (FailureClass::Policy, "read_only_posture".to_owned()),
            "{posture:?}"
        );
        assert_eq!(failure.sent, Sent::NotSent, "{posture:?}");
        assert_eq!(searched(app.seen().await.unwrap()), 0, "{posture:?}");
    }

    // A request by a method HTTP marks safe is sent in every posture.
    for posture in [
        Posture::Discovery,
        Posture::Sandbox {
            test_records: false,
        },
    ] {
        let status = read_in(
            &production,
            posture,
            "example_status_text_get",
            &Values::new(),
        )
        .await
        .unwrap();
        assert_eq!(status.get("orders"), Some(&Value::Int(3)), "{posture:?}");
    }
}

/// The scene with one entry of the endpoint `status_text_get` changed in
/// the compiled plan, as a plan the compiler of today refuses would hold
/// it: `part` is `headers` or `query`.
fn overridden(mut scene: Scene, part: &str, name: &str, value: &str) -> Scene {
    for section in &mut scene.plan.packs {
        if section.pack != a2m_pack_http::PACK {
            continue;
        }
        let mut payload: serde_json::Value = section.payload.decode().unwrap();
        let entry = payload["status_text_get"][part].as_object_mut().unwrap();
        entry.insert(name.to_owned(), serde_json::Value::from(value));
        section.payload = a2m_core::PackPayload::encode(&payload).unwrap();
    }
    scene
}

/// A request by a safe method that carries a method override, in a header
/// or as the `_method` query entry, is judged by the method the override
/// names: one that names a method HTTP does not mark safe is not sent
/// outside a live run, and the system hears nothing of it (decision
/// 0270). It is sent in a live run, as any request of a published read.
#[tokio::test]
async fn a_method_override_is_judged_as_the_method_it_names() {
    let app = TestApp::start().await.unwrap();
    let status = |seen: Vec<String>| {
        seen.iter()
            .filter(|line| *line == "GET /views/text/status")
            .count()
    };
    for (part, name, value) in [
        ("headers", "x-http-method-override", "DELETE"),
        ("headers", "x-http-method", "put"),
        ("headers", "x-method-override", "PATCH"),
        ("query", "_method", "DELETE"),
        ("query", "_METHOD", "POST"),
    ] {
        let scene = overridden(
            published(scene_in(&app, "test", Arc::new(NoSecrets))),
            part,
            name,
            value,
        );
        for posture in [
            Posture::Sandbox {
                test_records: false,
            },
            Posture::Discovery,
        ] {
            app.reset().await.unwrap();
            let failure = read_in(&scene, posture, "example_status_text_get", &Values::new())
                .await
                .unwrap_err();
            assert_eq!(
                class_and_code(&failure),
                (FailureClass::Policy, "read_only_posture".to_owned()),
                "{name} {posture:?}"
            );
            assert_eq!(failure.sent, Sent::NotSent, "{name} {posture:?}");
            assert_eq!(status(app.seen().await.unwrap()), 0, "{name} {posture:?}");
        }
        app.reset().await.unwrap();
        read_in(
            &scene,
            Posture::Live,
            "example_status_text_get",
            &Values::new(),
        )
        .await
        .unwrap_or_else(|failure| panic!("{name}: {failure:?}"));
        assert_eq!(status(app.seen().await.unwrap()), 1, "{name}");
    }
    // An override that names a safe method changes nothing.
    let scene = overridden(
        published(scene_in(&app, "test", Arc::new(NoSecrets))),
        "headers",
        "x-http-method-override",
        "GET",
    );
    app.reset().await.unwrap();
    read_in(
        &scene,
        Posture::Discovery,
        "example_status_text_get",
        &Values::new(),
    )
    .await
    .unwrap();
    assert_eq!(status(app.seen().await.unwrap()), 1);
}

/// The guard is lifted for one step only: the commit point of a write to a
/// declared test record, in the context the engine prepared for it. In any
/// other sandbox context the handle and the guard both refuse it, and the
/// application stores nothing.
#[tokio::test]
async fn a_write_to_a_declared_test_record_is_sent_and_no_other_outside_a_live_run() {
    let app = TestApp::start().await.unwrap();
    let scene = Scene::new("example", &app.url(""));
    let record_key = key(&["A-102", "L-900"]);
    let inputs = Values::new()
        .with("order_id", "A-102")
        .with("item_code", "L-900")
        .with("description", "Corner post")
        .with("quantity", 3)
        .with("length_in", Decimal::parse("150.50").unwrap());
    let binding = scene.binding("example_line_item_upsert").clone();

    for (posture, writes) in [
        (
            Posture::Sandbox {
                test_records: false,
            },
            false,
        ),
        (Posture::Discovery, false),
        (Posture::Sandbox { test_records: true }, true),
    ] {
        let (store, _vault) = store().await;
        let fence = running(&store, std::slice::from_ref(&record_key)).await;
        let intent = token(&store, fence, 0, &record_key).await;
        let write = Some((&intent, record(&fence, 0), &record_key));
        let (handle, mut prepared) = scene.try_prepare(None, Some(fence), posture).await.unwrap();
        // The record is looked up first: the write sends the version the
        // pass read, or none for a record found absent.
        let looked = scene
            .run(
                &handle,
                &mut prepared,
                &binding.read_back,
                &inputs,
                write,
                LONG,
            )
            .await
            .unwrap();
        assert!(looked.absent, "{posture:?}");
        let ran = scene
            .run(&handle, &mut prepared, &binding.act, &inputs, write, LONG)
            .await;
        handle.release(prepared, false).await;
        let stored = app.stored_item("A-102", "L-900").await.unwrap();
        if writes {
            ran.unwrap_or_else(|failure| panic!("{posture:?}: {failure:?}"));
            assert_eq!(stored.map(|item| item.quantity), Some(3));
        } else {
            let failure = ran.unwrap_err();
            assert_eq!(
                class_and_code(&failure),
                (FailureClass::Policy, "read_only_posture".to_owned()),
                "{posture:?}"
            );
            assert!(stored.is_none(), "{posture:?}");
        }
    }
    assert_eq!(app.write_count("A-102", "L-900").await.unwrap(), 1);
}

/// What the pack hands the engine for an XML text, a pattern's group, and
/// a whole text body: plain text values, passed through the redactor, no
/// longer than the body, with no control character but a tab or a line
/// break; and the operation marks the free text among them as from the
/// system. The same answers with an escape character in them stop, and
/// the stop's message holds no character of the answer.
#[tokio::test]
async fn a_value_is_bounded_data_and_a_control_character_in_any_form_stops() {
    let app = TestApp::start().await.unwrap();
    let scene = scene_in(&app, "test", Arc::new(One("Customer One")));
    let order = Values::new().with("order_id", "A-100");
    for (operation, inputs) in [
        ("example_order_customer_get", &order),
        ("example_status_text_get", &Values::new()),
    ] {
        let read = read_in(&scene, Posture::Live, operation, inputs)
            .await
            .unwrap();
        for (name, value) in read.iter() {
            if let Value::Text(text) = value {
                assert!(!text.contains("Customer One"), "{operation} {name}");
                assert!(text.len() <= 8_000_000, "{operation} {name}");
                assert!(
                    !text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')),
                    "{operation} {name}"
                );
            }
        }
        assert!(read.get("customer").is_some(), "{operation}");
    }
    let marked: Vec<bool> = ["example_order_customer_get", "example_status_text_get"]
        .iter()
        .flat_map(|name| {
            let operation = scene
                .plan
                .operations
                .iter()
                .find(|operation| operation.name.as_str() == *name)
                .unwrap();
            operation
                .outputs
                .iter()
                .filter(|output| output.name == "customer")
                .map(|output| {
                    let field_type = output.field_type.unwrap();
                    scene.plan.field_types[field_type.get()].untrusted
                })
        })
        .collect();
    assert_eq!(marked, [true, true]);

    for operation in ["example_control_get", "example_control_text_get"] {
        let failure = read_in(&scene, Posture::Live, operation, &Values::new())
            .await
            .unwrap_err();
        assert_eq!(
            class_and_code(&failure),
            (FailureClass::Drift, "response_schema_mismatch".to_owned()),
            "{operation}"
        );
        let message = failure.failure.message();
        assert!(
            message.contains("control character"),
            "{operation}: {message}"
        );
        assert!(
            !message.contains('\u{1b}') && !message.contains("Customer"),
            "{message}"
        );
    }
}

/// A binary answer that carries a registered secret gives neither its
/// length nor its digest, which would let a caller confirm a guess; with
/// no secret registered, it gives both.
#[tokio::test]
async fn a_body_that_holds_a_registered_secret_is_not_confirmable_through_its_digest() {
    let app = TestApp::start().await.unwrap();
    let knowing = scene_in(&app, "test", Arc::new(One(ECHOED_SECRET)));
    let failure = read_in(
        &knowing,
        Posture::Live,
        "example_secret_label_get",
        &Values::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (
            FailureClass::UnknownVariant,
            "answer_holds_secret".to_owned()
        )
    );
    assert!(!failure.failure.message().contains(ECHOED_SECRET));

    let plain = scene_in(&app, "test", Arc::new(NoSecrets));
    let label = read_in(
        &plain,
        Posture::Live,
        "example_secret_label_get",
        &Values::new(),
    )
    .await
    .unwrap();
    let length = i64::try_from(5 + ECHOED_SECRET.len() + 1).unwrap();
    assert_eq!(label.get("length"), Some(&Value::Int(length)));
}
