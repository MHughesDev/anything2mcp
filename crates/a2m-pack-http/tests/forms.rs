//! Answers that are not JSON (decision 0217): an XML view, a plain-text
//! status line, an old character set, and a binary label, each read end to
//! end against the test application through the driver's handle, by the
//! same public entry the product uses. Every value is compared with what
//! the application's control interface says it stores, and the write is
//! proven by asking the application, not by reading it back.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{Decimal, Digest, FailureClass, NoSecrets, REDACTED, Redact, Value, Values};
use a2m_provider::{Posture, StepFailure};
use common::{
    Folder, Scene, compile, compile_changed, key, record, registry, running, store, token,
};
use example_app::harness::TestApp;

const FORMS: &str = "forms";
const XML: &str = "endpoints/xml.yaml";
const OTHER: &str = "endpoints/other.yaml";

async fn scene() -> (TestApp, Scene) {
    let app = TestApp::start().await.unwrap();
    let scene = Scene::new(FORMS, &app.url(""));
    (app, scene)
}

/// The scene of the `forms` fixture with one file changed.
fn changed(app: &TestApp, path: &str, change: impl Fn(&str) -> String) -> Scene {
    let folder = Folder::new("forms-changed");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let compiled_target = compile_changed(&registry, FORMS, path, change)
        .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)))
        .compiled_target;
    Scene::over(folder, registry, compiled_target, &app.url(""))
}

async fn read(scene: &Scene, operation: &str, inputs: &Values) -> Result<Values, StepFailure> {
    let (handle, mut prepared) = scene.reader().await;
    let read = scene.read(&handle, &mut prepared, operation, inputs).await;
    handle.release(prepared, false).await;
    read.map(|ran| ran.values)
}

fn class_and_code(failure: &StepFailure) -> (FailureClass, String) {
    (
        failure.failure.class(),
        failure.failure.code().as_str().to_owned(),
    )
}

fn texts(values: &[&str]) -> Value {
    Value::List(values.iter().map(|text| Value::from(*text)).collect())
}

#[tokio::test]
async fn each_form_is_read_end_to_end_and_gives_what_the_application_stores() {
    let (app, scene) = scene().await;
    let order = app.stored_order("A-100").await.unwrap().unwrap();

    // XML: attributes, an element's text, a repeated element, a namespace
    // the document writes with prefixes of its own.
    let read_order = read(
        &scene,
        "example_order_xml_get",
        &Values::new().with("order_id", "A-100"),
    )
    .await
    .unwrap();
    assert_eq!(read_order.get("status"), Some(&Value::from("draft")));
    assert_eq!(
        read_order.get("customer"),
        Some(&Value::from(order.customer.as_str()))
    );
    let codes: Vec<&str> = order.items.keys().map(String::as_str).collect();
    assert_eq!(read_order.get("item_codes"), Some(&texts(&codes)));
    let quantities: Vec<Value> = order
        .items
        .values()
        .map(|item| Value::Int(i64::from(item.quantity)))
        .collect();
    assert_eq!(read_order.get("quantities"), Some(&Value::List(quantities)));

    // An order with no lines gives an empty list; one that is not there is
    // absent, as with JSON.
    let empty = read(
        &scene,
        "example_order_xml_get",
        &Values::new().with("order_id", "A-102"),
    )
    .await
    .unwrap();
    assert_eq!(empty.get("item_codes"), Some(&Value::List(Vec::new())));
    let (handle, mut prepared) = scene.reader().await;
    let missing = scene
        .read(
            &handle,
            &mut prepared,
            "example_order_xml_get",
            &Values::new().with("order_id", "A-999"),
        )
        .await
        .unwrap();
    assert!(missing.absent && missing.values.is_empty());
    handle.release(prepared, false).await;

    // Text: the whole body, and the named groups of its pattern, typed.
    let status = read(&scene, "example_status_get", &Values::new())
        .await
        .unwrap();
    assert_eq!(status.get("orders"), Some(&Value::Int(3)));
    assert_eq!(status.get("draft"), Some(&Value::Int(2)));
    assert_eq!(status.get("submitted"), Some(&Value::Int(1)));
    assert_eq!(status.get("newest"), Some(&Value::from("A-102")));
    let mut stored = Vec::new();
    for id in ["A-100", "A-101", "A-102"] {
        stored.push(app.stored_order(id).await.unwrap().unwrap());
    }
    let line = example_app::views::status_line(&stored.iter().collect::<Vec<_>>());
    assert_eq!(status.get("text"), Some(&Value::from(line.as_str())));

    // Text in the character set an old device sends, decoded by the one
    // the endpoint declares.
    let legacy = read(&scene, "example_legacy_status_get", &Values::new())
        .await
        .unwrap();
    assert_eq!(legacy.get("orders"), Some(&Value::Int(3)));
    assert_eq!(legacy.get("state"), Some(&Value::from("ready")));

    // Bytes: the length and the digest of the label, and nothing else.
    let label = read(
        &scene,
        "example_order_label_get",
        &Values::new().with("order_id", "A-100"),
    )
    .await
    .unwrap();
    let expected = example_app::views::label(&order);
    assert_eq!(
        label.get("length"),
        Some(&Value::Int(i64::try_from(expected.len()).unwrap()))
    );
    assert_eq!(
        label.get("digest"),
        Some(&Value::from(Digest::of(&expected).to_string().as_str()))
    );
    assert_eq!(label.len(), 2);

    assert!(app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_path_that_matches_nothing_or_more_than_once_is_the_response_shape_failure() {
    let app = TestApp::start().await.unwrap();
    let order = Values::new().with("order_id", "A-100");

    // The order has no note: optional, it reads as nothing; required, the
    // answer is not the one the endpoint declares.
    let scene = changed(&app, XML, |text| {
        text.replace(
            r#"note: {path: "o:order/o:note", required: false}"#,
            r#"note: {path: "o:order/o:note"}"#,
        )
    });
    let failure = read(&scene, "example_order_xml_get", &order)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Drift, "response_schema_mismatch".to_owned())
    );
    assert!(failure.failure.message().contains("`note`"), "{failure:?}");

    // One value declared where the order has three lines.
    let scene = changed(&app, XML, |text| {
        text.replace(
            r#"order_id: {path: "o:order/@id"}"#,
            r#"order_id: {path: "o:order/o:items/o:item/@code"}"#,
        )
    });
    let failure = read(&scene, "example_order_xml_get", &order)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Drift, "response_schema_mismatch".to_owned())
    );
    assert!(
        failure.failure.message().contains("more than one"),
        "{failure:?}"
    );
    // The message names the field and never a value of the answer.
    assert!(!failure.failure.message().contains("L-10"), "{failure:?}");
}

#[tokio::test]
async fn an_answer_that_declares_a_document_type_is_refused_before_anything_is_read() {
    let (_app, scene) = scene().await;
    let failure = read(&scene, "example_declared_get", &Values::new())
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (
            FailureClass::UnknownVariant,
            "document_type_refused".to_owned()
        )
    );
    assert!(
        !failure.failure.message().contains("Customer"),
        "{failure:?}"
    );
}

#[tokio::test]
async fn the_bounds_on_an_xml_answer_are_named() {
    let app = TestApp::start().await.unwrap();
    let order = Values::new().with("order_id", "A-100");
    for (from, to) in [
        ("max_elements: 1000", "max_elements: 5"),
        ("max_depth: 64", "max_depth: 3"),
    ] {
        let scene = changed(&app, XML, |text| text.replace(from, to));
        let failure = read(&scene, "example_order_xml_get", &order)
            .await
            .unwrap_err();
        assert_eq!(
            class_and_code(&failure),
            (
                FailureClass::UnknownVariant,
                "response_too_large".to_owned()
            ),
            "{to}"
        );
        // Within the bound, the order with no lines reads.
        let fits = read(
            &scene,
            "example_order_xml_get",
            &Values::new().with("order_id", "A-102"),
        )
        .await;
        assert!(fits.is_ok(), "{to}: {fits:?}");
    }

    // The answer's ceiling holds for every form.
    let scene = changed(&app, OTHER, |text| {
        text.replace(
            "path: \"/views/text/status\"\n",
            "path: \"/views/text/status\"\nmax_response_bytes: 16\n",
        )
    });
    let failure = read(&scene, "example_status_get", &Values::new())
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (
            FailureClass::UnknownVariant,
            "response_too_large".to_owned()
        )
    );
}

#[tokio::test]
async fn an_answer_of_another_content_type_or_that_does_not_decode_is_the_response_shape_failure() {
    let (app, scene) = scene().await;
    // The JSON interface's answer, declared as XML: its content type is not
    // one of the form's, and nothing of it is read.
    let failure = read(
        &scene,
        "example_order_json_as_xml_get",
        &Values::new().with("order_id", "A-100"),
    )
    .await
    .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Drift, "response_schema_mismatch".to_owned())
    );
    assert!(
        failure.failure.message().contains("content type"),
        "{failure:?}"
    );

    // The old device's line, declared as UTF-8: it does not decode, and no
    // character is replaced to make it.
    let scene = changed(&app, OTHER, |text| {
        text.replace("  charset: iso-8859-1\n", "")
    });
    let failure = read(&scene, "example_legacy_status_get", &Values::new())
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Drift, "response_schema_mismatch".to_owned())
    );
    assert!(failure.failure.message().contains("decode"), "{failure:?}");

    // A text that does not match the anchored pattern.
    let scene = changed(&app, OTHER, |text| {
        text.replace("(?P<state>[a-z]+)\\n", "(?P<state>[a-z]+)")
    });
    let failure = read(&scene, "example_legacy_status_get", &Values::new())
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Drift, "response_schema_mismatch".to_owned())
    );
}

/// A redactor that knows one secret.
struct Knows(&'static str);

impl Redact for Knows {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, REDACTED)
    }
}

#[tokio::test]
async fn a_registered_secret_in_an_xml_or_a_text_answer_comes_out_redacted() {
    let app = TestApp::start().await.unwrap();
    // The newest order's customer stands for a secret the system holds.
    let scene = Scene::with_redactor(FORMS, &app.url(""), Arc::new(Knows("Customer Three")));

    let order = read(
        &scene,
        "example_order_xml_get",
        &Values::new().with("order_id", "A-102"),
    )
    .await
    .unwrap();
    let redacted = format!("Example {REDACTED}");
    assert_eq!(order.get("customer"), Some(&Value::from(redacted.as_str())));

    let status = read(&scene, "example_status_get", &Values::new())
        .await
        .unwrap();
    assert_eq!(
        status.get("customer"),
        Some(&Value::from(redacted.as_str()))
    );
    let whole = status.get("text").and_then(Value::as_text).unwrap();
    assert!(whole.contains(REDACTED) && !whole.contains("Customer Three"));
}

fn item(order: &str, code: &str, description: &str, quantity: i64, length: &str) -> Values {
    Values::new()
        .with("order_id", order)
        .with("item_code", code)
        .with("description", description)
        .with("quantity", quantity)
        .with("length_in", Decimal::parse(length).unwrap())
}

#[tokio::test]
async fn a_json_write_is_read_back_through_its_xml_view_and_the_application_stored_it() {
    let (app, scene) = scene().await;
    let binding = scene.binding("example_line_item_upsert");
    let (ledger, _vault) = store().await;
    let key = key(&["A-100", "L-101"]);
    let fence = running(&ledger, std::slice::from_ref(&key)).await;
    let token = token(&ledger, fence, 0, &key).await;
    let write = Some((&token, record(&fence, 0), &key));
    let long = Duration::from_secs(30);

    let (handle, mut prepared) = scene
        .try_prepare(None, Some(fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-100", "L-101", "Left rail & cap", 5, "150.50");
    // The read before the write is the XML view, and the version it reads
    // goes back with the JSON write.
    let before = scene
        .run(
            &handle,
            &mut prepared,
            &binding.read_back,
            &inputs,
            write,
            long,
        )
        .await
        .unwrap();
    assert_eq!(before.values.get("quantity"), Some(&Value::Int(2)));
    scene
        .run(&handle, &mut prepared, &binding.act, &inputs, write, long)
        .await
        .unwrap();
    scene
        .run(
            &handle,
            &mut prepared,
            &binding.confirm,
            &inputs,
            write,
            long,
        )
        .await
        .unwrap();
    let after = scene
        .run(
            &handle,
            &mut prepared,
            &binding.read_back,
            &inputs,
            write,
            long,
        )
        .await
        .unwrap();
    handle.release(prepared, false).await;

    // What the application stored, by its control interface.
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!(
        (stored.quantity, stored.length_sixteenths, stored.version),
        (5, 2408, 2)
    );
    assert_eq!(stored.description, "Left rail & cap");
    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 1);
    // The read-back, typed from XML text, is the value that was sent, by
    // the same normalization as a JSON text of that type.
    assert_eq!(after.values.get("length_in"), inputs.get("length_in"));
    assert_eq!(after.values.get("quantity"), Some(&Value::Int(5)));
    assert_eq!(
        after.values.get("description"),
        Some(&Value::from("Left rail & cap"))
    );
}

#[test]
fn the_forms_compile_through_the_public_entry_twice_to_the_same_bytes_and_json_is_unchanged() {
    let folder = Folder::new("forms-compile");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let first = compile(&registry, FORMS);
    let second = compile(&registry, FORMS);
    assert_eq!(first.encoded.bytes, second.encoded.bytes);

    // A JSON endpoint that declares nothing of its answer is written into
    // the compiled target as before this form existed.
    let plain = compile(&registry, "example").compiled_target;
    let section = |compiled_target: &a2m_core::CompiledTarget| {
        let section = compiled_target
            .packs
            .iter()
            .find(|section| section.pack == "http")
            .unwrap();
        String::from_utf8(section.payload.as_bytes().to_vec()).unwrap()
    };
    assert!(!section(&plain).contains("\"answer\""));
    assert!(section(&first.compiled_target).contains("\"xml\""));
}

/// Compiles the `forms` fixture with one change, and gives the codes of
/// its problems.
fn problems(path: &str, change: impl Fn(&str) -> String) -> Vec<String> {
    let folder = Folder::new("forms-refused");
    let registry = registry(&folder, Arc::new(NoSecrets));
    compile_changed(&registry, FORMS, path, change)
        .err()
        .map(|problems| problems.iter().map(|p| p.code.to_string()).collect())
        .unwrap_or_default()
}

#[test]
fn an_answer_declared_outside_its_small_language_is_refused_where_it_is_written() {
    let refused = |from: &str, to: &str| {
        let codes = problems(XML, |text| text.replacen(from, to, 1));
        assert!(
            codes.iter().any(|code| code == "http.answer_invalid"),
            "{to}: {codes:?}"
        );
    };
    // A prefix the catalog does not declare; a predicate; a function; an
    // attribute that is not the last step; a nested `each`.
    refused(r#""o:order/@id""#, r#""p:order/@id""#);
    refused(r#""o:order/@id""#, r#""o:order[1]/@id""#);
    refused(r#""o:order/o:customer""#, r#""o:order/text()""#);
    refused(r#""o:order/@status""#, r#""@status/o:order""#);
    refused(
        "quantity: {path: \"o:quantity\", type: integer}",
        "quantity: {each: \"o:quantity\"}",
    );
    // A shape of JSON beside an answer that is not JSON.
    refused(
        "answer:\n  form: xml\n  namespaces: {o: \"urn:example:orders\"}\n  max_depth",
        "response_schema: {type: any}\nanswer:\n  form: xml\n  namespaces: {o: \"urn:example:orders\"}\n  max_depth",
    );
    // A pattern's field that is not one of its groups.
    let codes = problems(OTHER, |text| {
        text.replacen("orders: {type: integer}", "count: {type: integer}", 1)
    });
    assert!(
        codes.iter().any(|code| code == "http.answer_invalid"),
        "{codes:?}"
    );
    // An output that is not a field of the record.
    let codes = problems(XML, |text| {
        text.replacen(r#"status: {path: "o:order/@status"}"#, "", 1)
    });
    assert!(
        codes.iter().any(|code| code == "http.extract_unknown"),
        "{codes:?}"
    );
}
