//! The `http` driver against the built-in test application, reached only
//! through the driver handle. Every write is proven by asking the
//! application's control interface what it stored.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{Decimal, FailureClass, NoSecrets, RecordKey, Value, Values};
use a2m_ledger::{Fence, IntentToken, Store};
use a2m_provider::{Destinations, DriverHealth, Posture, Sent, StepFailure};
use common::{Folder, Ran, Scene, compile_changed, key, record, registry, running, store, token};
use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const LONG: Duration = Duration::from_secs(30);

async fn scene() -> (TestApp, Scene) {
    let app = TestApp::start().await.unwrap();
    let scene = Scene::new("example", &app.url(""));
    (app, scene)
}

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
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
async fn each_read_verb_reads_and_a_record_that_is_not_there_is_absent() {
    let (app, scene) = scene().await;
    let (handle, mut prepared) = scene.reader().await;

    let order = Values::new().with("order_id", "A-100");
    let read = scene
        .read(&handle, &mut prepared, "example_order_get", &order)
        .await
        .unwrap();
    assert_eq!(read.values.get("status"), Some(&Value::from("draft")));
    assert_eq!(
        read.values.get("customer"),
        Some(&Value::from("Example Customer One"))
    );

    let listed = scene
        .read(&handle, &mut prepared, "example_order_list", &Values::new())
        .await
        .unwrap();
    let expected: Vec<Value> = ["A-100", "A-101", "A-102"]
        .into_iter()
        .map(Value::from)
        .collect();
    assert_eq!(listed.values.get("order_ids"), Some(&Value::List(expected)));

    // A length arrives as an exact number, never through a float.
    let line = Values::new()
        .with("order_id", "A-100")
        .with("item_code", "L-101");
    let read = scene
        .read(&handle, &mut prepared, "example_line_item_get", &line)
        .await
        .unwrap();
    assert_eq!(read.values.get("quantity"), Some(&Value::Int(2)));
    assert_eq!(
        read.values.get("length_in"),
        Some(&Value::Decimal(Decimal::parse("150.5").unwrap()))
    );

    let missing = Values::new().with("order_id", "A-999");
    let read = scene
        .read(&handle, &mut prepared, "example_order_get", &missing)
        .await
        .unwrap();
    assert!(read.absent && read.values.is_empty());

    assert!(app.writes().await.unwrap().is_empty());
    assert!(handle.release(prepared, false).await.complete);
}

#[tokio::test]
async fn five_runs_of_one_read_do_the_same_thing() {
    let (_app, scene) = scene().await;
    let order = Values::new().with("order_id", "A-100");
    let mut means = Vec::new();
    for _ in 0..5 {
        let (handle, mut prepared) = scene.reader().await;
        let read = scene
            .read(&handle, &mut prepared, "example_order_get", &order)
            .await
            .unwrap();
        let acting: a2m_telemetry::Acting = serde_json::from_str(&read.capture("acting")).unwrap();
        // What was done names the template, never the value that filled it.
        assert!(
            acting
                .means
                .iter()
                .any(|detail| detail.value == "/api/orders/{order_id}")
        );
        means.push((acting.means, read.values));
        handle.release(prepared, false).await;
    }
    assert!(means.windows(2).all(|pair| pair[0] == pair[1]), "{means:?}");
}

/// A write run for one record, with its intent stored: what the engine has
/// in hand when it reaches a commit point.
struct Write {
    _store: Store,
    fence: Fence,
    token: IntentToken,
    key: RecordKey,
}

async fn write_of(order: &str, code: &str) -> Write {
    let (store, _vault) = store().await;
    let key = key(&[order, code]);
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
    /// One list of the upsert's binding, carried out for this record.
    async fn run(
        &self,
        scene: &Scene,
        handle: &a2m_provider::DriverHandle,
        prepared: &mut a2m_provider::Prepared,
        list: &str,
        inputs: &Values,
    ) -> Result<Ran, StepFailure> {
        let binding = scene.binding("example_line_item_upsert");
        let steps = match list {
            "act" => &binding.act,
            "confirm" => &binding.confirm,
            _ => &binding.read_back,
        };
        let write = Some((&self.token, record(&self.fence, 0), &self.key));
        scene
            .run(handle, prepared, steps, inputs, write, LONG)
            .await
    }
}

#[tokio::test]
async fn a_write_is_sent_once_confirmed_and_read_back_and_the_application_stored_it() {
    let (app, scene) = scene().await;

    // A new record: looked up, found absent, written, confirmed, read back.
    let write = write_of("A-102", "L-900").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-102", "L-900", "Corner post", 3, "150.50");
    let before = write
        .run(&scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    assert!(before.absent);
    write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap();
    write
        .run(&scene, &handle, &mut prepared, "confirm", &inputs)
        .await
        .unwrap();
    let after = write
        .run(&scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    handle.release(prepared, false).await;

    let stored = app.stored_item("A-102", "L-900").await.unwrap().unwrap();
    assert_eq!(
        (stored.quantity, stored.length_sixteenths, stored.version),
        (3, 2408, 1)
    );
    assert_eq!(stored.description, "Corner post");
    assert_eq!(app.write_count("A-102", "L-900").await.unwrap(), 1);
    // The application stores sixteenths and shows `150.5`; what was sent as
    // `150.50` reads back as the same number, and not as another.
    let read_back = after.values.get("length_in").unwrap();
    assert_eq!(read_back, inputs.get("length_in").unwrap());
    assert_ne!(
        read_back,
        &Value::Decimal(Decimal::parse("150.4375").unwrap())
    );
    assert_eq!(after.values.get("quantity"), Some(&Value::Int(3)));

    // An existing record: the version read by the lookup goes back with
    // the write, and the application takes it.
    let write = write_of("A-100", "L-101").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-100", "L-101", "Left rail", 5, "150.5");
    let before = write
        .run(&scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    assert_eq!(before.values.get("quantity"), Some(&Value::Int(2)));
    write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap();
    write
        .run(&scene, &handle, &mut prepared, "confirm", &inputs)
        .await
        .unwrap();
    handle.release(prepared, false).await;
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!((stored.quantity, stored.version), (5, 2));
    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 1);
}

#[tokio::test]
async fn a_write_without_the_ledgers_token_never_reaches_the_application() {
    let (app, scene) = scene().await;
    let (handle, mut prepared) = scene.reader().await;
    let inputs = item("A-102", "L-900", "Corner post", 3, "150.5");
    let act = &scene.binding("example_line_item_upsert").act;
    let refused = scene
        .run(&handle, &mut prepared, act, &inputs, None, LONG)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Policy, "intent_missing")
    );
    assert_eq!(refused.sent, Sent::NotSent);

    // The driver itself refuses an endpoint that writes from a step that
    // is not a commit point, so no read path can be bent into a write.
    let mut unmarked = act.clone();
    unmarked[0].commits = false;
    let refused = scene
        .run(&handle, &mut prepared, &unmarked, &inputs, None, LONG)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Input, "step_not_usable")
    );
    assert_eq!(refused.sent, Sent::NotSent);
    assert!(app.writes().await.unwrap().is_empty());
}

#[tokio::test]
async fn what_the_application_refuses_is_named_and_nothing_is_stored() {
    let (app, scene) = scene().await;

    // The application's own validation: `rejected`, answered.
    let write = write_of("A-102", "L-900").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-102", "L-900", "Corner post", 0, "150.5");
    for list in ["read_back", "act"] {
        write
            .run(&scene, &handle, &mut prepared, list, &inputs)
            .await
            .unwrap();
    }
    let refused = write
        .run(&scene, &handle, &mut prepared, "confirm", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Rejected, "app_validation_error")
    );
    assert_eq!(refused.sent, Sent::Answered);
    // The system's own words are carried apart from the product's sentence.
    assert_eq!(refused.failure.said(), Some("The line item was not saved."));
    assert!(!refused.failure.message().contains("was not saved"));
    assert_eq!(app.stored_item("A-102", "L-900").await.unwrap(), None);

    // A record changed by someone else since it was read: the version the
    // lookup read goes with the write, and the application refuses it.
    let write = write_of("A-100", "L-102").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-100", "L-102", "Right rail", 9, "150.5");
    write
        .run(&scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    let other = serde_json::json!(
        {"description": "Right rail", "quantity": 7, "length_in": "150.5", "version": 1}
    );
    let changed = app
        .http()
        .put(app.url("/api/orders/A-100/items/L-102"))
        .json(&other)
        .send()
        .await
        .unwrap();
    assert!(changed.status().is_success());
    write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap();
    let stale = write
        .run(&scene, &handle, &mut prepared, "confirm", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&stale),
        (FailureClass::Stale, "version_conflict")
    );
    assert_eq!(stale.sent, Sent::Answered);
    let message = stale.failure.message();
    assert!(
        message.contains("Version 1 was sent; the record is at 2."),
        "{message}"
    );
    let stored = app.stored_item("A-100", "L-102").await.unwrap().unwrap();
    assert_eq!((stored.quantity, stored.version), (7, 2));

    // An order that is closed: a refusal in another declared shape.
    let write = write_of("A-101", "L-201").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-101", "L-201", "Base plate", 2, "24");
    for list in ["read_back", "act"] {
        write
            .run(&scene, &handle, &mut prepared, list, &inputs)
            .await
            .unwrap();
    }
    let closed = write
        .run(&scene, &handle, &mut prepared, "confirm", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&closed),
        (FailureClass::Rejected, "app_validation_error")
    );

    // The one write the application holds is the other caller's.
    assert_eq!(app.writes().await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_faults_of_the_application_are_each_named() {
    let (app, scene) = scene().await;
    let (handle, mut prepared) = scene.reader().await;

    // A declared "not now": every third call of the stock endpoint.
    let stock = Values::new().with("item_code", "L-101");
    for _ in 0..2 {
        let read = scene
            .read(&handle, &mut prepared, "example_stock_get", &stock)
            .await
            .unwrap();
        assert_eq!(read.values.get("in_stock"), Some(&Value::Int(25)));
    }
    let busy = scene
        .read(&handle, &mut prepared, "example_stock_get", &stock)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&busy),
        (FailureClass::Transient, "upstream_unavailable")
    );
    assert_eq!(busy.sent, Sent::Answered);

    // A status the endpoint does not declare is never guessed at.
    let fault = Fault::once(FaultKind::Status { status: 500 }).on_paths("/api/orders");
    app.set_fault(&fault).await.unwrap();
    let order = Values::new().with("order_id", "A-100");
    let unknown = scene
        .read(&handle, &mut prepared, "example_order_get", &order)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&unknown),
        (FailureClass::UnknownVariant, "unexpected_status")
    );

    // An endpoint that does not answer in the step's time. The application
    // waits five seconds; the step is given a fraction of one.
    app.set_slow_ms(5_000).await.unwrap();
    let totals = &scene.binding("example_order_totals_get").act;
    let slow = scene
        .run(
            &handle,
            &mut prepared,
            totals,
            &order,
            None,
            Duration::from_millis(300),
        )
        .await
        .unwrap_err();
    assert_eq!(class_and_code(&slow), (FailureClass::Transient, "timeout"));
    assert_eq!(slow.sent, Sent::Unknown);
    handle.release(prepared, true).await;

    // Drift: the application's answer no longer fits the declared shape.
    let folder = Folder::new("drift");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let compiled_target = compile_changed(&registry, "example", "endpoints/orders.yaml", |text| {
        text.replace("enum: [draft, submitted]", "enum: [open, closed]")
    })
    .unwrap()
    .compiled_target;
    let drifted = Scene::over(folder, registry, compiled_target, &app.url(""));
    let (handle, mut prepared) = drifted.reader().await;
    let drift = drifted
        .read(&handle, &mut prepared, "example_order_get", &order)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&drift),
        (FailureClass::Drift, "response_schema_mismatch")
    );
    assert!(
        drift.failure.message().contains("the body.status"),
        "{drift}"
    );
    // The message names the place and never the value found there.
    assert!(!drift.failure.message().contains("draft"), "{drift}");
}

/// Reads one HTTP message with a `Content-Length`, or with no body.
async fn message(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map_or(0, |value| value.trim().parse::<usize>().unwrap());
            if bytes.len() >= end + 4 + length {
                return bytes;
            }
        }
        let read = stream.read(&mut chunk).await.unwrap();
        if read == 0 {
            return bytes;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

/// Carries one request to the application and its answer back. With
/// `loses_put`, the answer to a PUT is lost after the application gave it.
async fn carry(mut client: TcpStream, upstream: String, loses_put: bool) {
    let request = message(&mut client).await;
    if request.is_empty() {
        return;
    }
    let mut server = TcpStream::connect(upstream).await.unwrap();
    server.write_all(&request).await.unwrap();
    let answer = message(&mut server).await;
    if loses_put && request.starts_with(b"PUT ") {
        // The application answered; the caller never hears it.
        return;
    }
    // One request per connection, so that none is ever reused.
    let text = String::from_utf8(answer).unwrap();
    let closing = text.replacen("\r\n", "\r\nconnection: close\r\n", 1);
    client.write_all(closing.as_bytes()).await.unwrap();
    client.shutdown().await.unwrap();
}

/// A link in front of the application, on a listener the test made. It
/// passes every request on. Stopping the task it returns closes the
/// listener, so that the next connection is refused.
fn link(listener: TcpListener, upstream: String, loses_put: bool) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Ok((client, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(carry(client, upstream.clone(), loses_put));
        }
    })
}

/// A link in front of the application that passes every request on, and
/// loses the answer to every PUT after the application has given it.
async fn losing_link(upstream: String) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    link(listener, upstream, true);
    address
}

#[tokio::test]
async fn a_write_whose_answer_is_lost_is_a_possible_commit_and_is_not_sent_again() {
    let app = TestApp::start().await.unwrap();
    let upstream = app.url("").trim_start_matches("http://").to_owned();
    let scene = Scene::new("example", &losing_link(upstream).await);

    let write = write_of("A-102", "L-900").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-102", "L-900", "Corner post", 3, "150.5");
    let before = write
        .run(&scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    assert!(before.absent);
    let lost = write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&lost),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert_eq!(lost.sent, Sent::Unknown);

    // The application did store the write, exactly once: the driver sent
    // it once and did not try again. Reconcile is the engine's, by reading.
    assert_eq!(app.write_count("A-102", "L-900").await.unwrap(), 1);
    let found = write
        .run(&scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    assert_eq!(found.values.get("quantity"), Some(&Value::Int(3)));
    assert_eq!(app.write_count("A-102", "L-900").await.unwrap(), 1);
    handle.release(prepared, true).await;
    // A connection that cannot be made at all is not a possible commit;
    // `a_write_that_was_not_sent_keeps_its_version_and_one_with_no_version_read_is_refused`
    // shows that case.
}

#[tokio::test]
async fn no_input_changes_where_a_request_goes() {
    let (app, scene) = scene().await;
    let (handle, mut prepared) = scene.reader().await;

    // A separator or a dot segment in a path value is refused unsent.
    for bad in ["../admin", "A-100/items/L-101", "..", "a\\b", "a\nb"] {
        let order = Values::new().with("order_id", bad);
        let refused = scene
            .read(&handle, &mut prepared, "example_order_get", &order)
            .await
            .unwrap_err();
        assert_eq!(
            class_and_code(&refused),
            (FailureClass::Input, "path_value_refused"),
            "{bad:?}"
        );
        assert_eq!(refused.sent, Sent::NotSent, "{bad:?}");
    }
    // A host, an encoded newline, a query: each stays one segment of the
    // path, the request still goes to the application, and the application
    // knows no such order.
    for odd in [
        "other.example.test:80",
        "A-100%0d%0aHost: other.example.test",
        "A-100?x=1#y",
        "@other",
        "%2e%2e",
        "..%2fadmin",
        "%5c%5cother.example.test",
        "A-100;x=1",
    ] {
        let order = Values::new().with("order_id", odd);
        let read = scene
            .read(&handle, &mut prepared, "example_order_get", &order)
            .await
            .unwrap();
        assert!(read.absent, "{odd}");
    }
    // The same holds for a write: its record is read first, and that read
    // is refused before anything is sent.
    let write = write_of("A-102", "x/../../L-1").await;
    let inputs = item("A-102", "x/../../L-1", "Corner post", 3, "150.5");
    let (writer, mut writing) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let refused = write
        .run(&scene, &writer, &mut writing, "read_back", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Input, "path_value_refused")
    );
    assert_eq!(refused.sent, Sent::NotSent);
    // The record could not be read, so its write is not sent either.
    let refused = write
        .run(&scene, &writer, &mut writing, "act", &inputs)
        .await
        .unwrap_err();
    assert_eq!(refused.sent, Sent::NotSent);
    assert!(app.writes().await.unwrap().is_empty());

    // An address the run may not reach is refused when the driver prepares.
    let mut elsewhere = Scene::new("example", &app.url(""));
    elsewhere.destinations = Destinations::new(["http://other.example.test".to_owned()]);
    let refused = elsewhere
        .try_prepare(None, None, Posture::Live)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Policy, "surface_not_declared")
    );

    // Without TLS in this build, an HTTPS surface is refused by name.
    #[cfg(not(feature = "tls"))]
    {
        let secure = Scene::new("example", "https://app.example.test");
        let refused = secure
            .try_prepare(None, None, Posture::Live)
            .await
            .unwrap_err();
        assert_eq!(
            class_and_code(&refused),
            (FailureClass::Input, "tls_not_enabled")
        );
        assert_eq!(refused.sent, Sent::NotSent);
        assert_eq!(
            secure.handle().health(&secure.surface),
            DriverHealth::Unavailable(a2m_pack_http::codes::TLS_NOT_ENABLED)
        );
    }
    assert_eq!(scene.handle().health(&scene.surface), DriverHealth::Ready);
}

/// The classes after which the ledger takes a record as settled, so that
/// nothing reconciles it. No failure of a write that was sent may be one of
/// them, unless it is the application's own verdict.
fn settled(failure: &StepFailure) -> bool {
    matches!(
        failure.failure.class(),
        FailureClass::Policy | FailureClass::Input | FailureClass::Stale
    )
}

/// Saves one new line item of the order `A-102`, with a fault switched on
/// for the save when one is given, and gives how the save ended: the
/// failure of its commit step, or of its confirm. Whatever the driver said,
/// the application is asked what it stored: the write is there once,
/// reading the record back finds it, and nothing was followed or sent again.
async fn saved_under(
    app: &TestApp,
    scene: &Scene,
    code: &str,
    fault: Option<FaultKind>,
) -> StepFailure {
    let write = write_of("A-102", code).await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-102", code, "Corner post", 3, "150.5");
    let before = write
        .run(scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    assert!(before.absent, "{code}");
    if let Some(fault) = fault {
        app.set_fault(&Fault::once(fault).on_paths("/api/orders/A-102/items"))
            .await
            .unwrap();
    }
    let failure = match write
        .run(scene, &handle, &mut prepared, "act", &inputs)
        .await
    {
        Err(failure) => failure,
        Ok(_) => write
            .run(scene, &handle, &mut prepared, "confirm", &inputs)
            .await
            .unwrap_err(),
    };
    app.clear_faults().await.unwrap();
    assert_eq!(app.write_count("A-102", code).await.unwrap(), 1, "{code}");
    let found = write
        .run(scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    assert_eq!(found.values.get("quantity"), Some(&Value::Int(3)), "{code}");
    assert_eq!(app.write_count("A-102", code).await.unwrap(), 1, "{code}");
    handle.release(prepared, true).await;
    failure
}

#[tokio::test]
async fn a_write_whose_answer_is_not_the_applications_verdict_is_in_doubt_and_never_settled() {
    let (app, scene) = scene().await;
    let in_doubt = |failure: &StepFailure, what: &str| {
        assert_eq!(
            class_and_code(failure),
            (FailureClass::PossibleCommit, "no_confirmation"),
            "{what}"
        );
        assert_eq!(failure.sent, Sent::Unknown, "{what}");
    };

    // The application makes the write and answers with a redirect to another
    // origin, as a posted form often is. The redirect is not followed, and
    // the write is in doubt: it is never reported as refused by policy,
    // which the ledger takes as "nothing was written".
    for (code, location) in [
        ("L-901", "http://other.example.test/done?token=PLANTED"),
        ("L-902", "//other.example.test/done"),
        ("L-903", "/\\other.example.test/done"),
    ] {
        let fault = FaultKind::RedirectAfterCommit {
            location: location.to_owned(),
        };
        let doubt = saved_under(&app, &scene, code, Some(fault)).await;
        in_doubt(&doubt, location);
        // The address the system named is not quoted in the failure.
        let message = doubt.failure.message();
        assert!(
            !message.contains("other.example") && !message.contains("PLANTED"),
            "{message}"
        );
    }

    // A redirect within the surface is a status like any other. The
    // endpoint does not declare it, so the confirm stops, in a class the
    // ledger keeps in doubt.
    let fault = FaultKind::RedirectAfterCommit {
        location: "/api/orders/A-102".to_owned(),
    };
    let within = saved_under(&app, &scene, "L-904", Some(fault)).await;
    assert_eq!(
        class_and_code(&within),
        (FailureClass::UnknownVariant, "unexpected_status")
    );
    assert_eq!(within.sent, Sent::Answered);
    assert!(!settled(&within), "{within}");

    // An answer to a write that is larger than its endpoint allows was not
    // read: the write is in doubt.
    let folder = Folder::new("small");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let compiled_target =
        compile_changed(&registry, "example", "endpoints/line_items.yaml", |text| {
            text.replacen("effect: write", "effect: write\nmax_response_bytes: 8", 1)
        })
        .unwrap()
        .compiled_target;
    let small = Scene::over(folder, registry, compiled_target, &app.url(""));
    let large = saved_under(&app, &small, "L-905", None).await;
    in_doubt(&large, "an answer over the limit");

    // A step that was to judge a write's answer and cannot be carried out
    // leaves the write in doubt too: its verdict was never read.
    let write = write_of("A-102", "L-906").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-102", "L-906", "Corner post", 3, "150.5");
    for list in ["read_back", "act"] {
        write
            .run(&scene, &handle, &mut prepared, list, &inputs)
            .await
            .unwrap();
    }
    let binding = scene.binding("example_line_item_upsert");
    let mut bent = binding.confirm.clone();
    bent[0]
        .refers_to
        .clone_from(&binding.read_back[0].refers_to);
    let record = Some((&write.token, record(&write.fence, 0), &write.key));
    let unread = scene
        .run(&handle, &mut prepared, &bent, &inputs, record, LONG)
        .await
        .unwrap_err();
    in_doubt(&unread, "a confirm that cannot be carried out");
    assert_eq!(app.write_count("A-102", "L-906").await.unwrap(), 1);
}

#[tokio::test]
async fn a_write_that_was_not_sent_keeps_its_version_and_one_with_no_version_read_is_refused() {
    let app = TestApp::start().await.unwrap();
    let upstream = app.url("").trim_start_matches("http://").to_owned();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let scene = Scene::new("example", &format!("http://127.0.0.1:{port}"));
    let up = link(listener, upstream.clone(), false);

    // The record is read, and its version with it.
    let write = write_of("A-100", "L-102").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-100", "L-102", "Right rail", 9, "150.5");
    let before = write
        .run(&scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    assert!(!before.absent);

    // Someone else changes the record after it was read.
    let other = serde_json::json!(
        {"description": "Right rail", "quantity": 7, "length_in": "150.5", "version": 1}
    );
    let changed = app
        .http()
        .put(app.url("/api/orders/A-100/items/L-102"))
        .json(&other)
        .send()
        .await
        .unwrap();
    assert!(changed.status().is_success());

    // The system cannot be reached: the first attempt of the write is not
    // sent, which is what the engine tries again.
    up.abort();
    let _ = up.await;
    let unsent = write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&unsent),
        (FailureClass::Transient, "connection_lost")
    );
    assert_eq!(unsent.sent, Sent::NotSent);

    // The system is back, and the write is tried again. It still carries
    // the version that was read, so the application refuses it, and the
    // other caller's change is not overwritten.
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    link(listener, upstream, false);
    write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap();
    let stale = write
        .run(&scene, &handle, &mut prepared, "confirm", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&stale),
        (FailureClass::Stale, "version_conflict")
    );
    let message = stale.failure.message();
    assert!(
        message.contains("Version 1 was sent; the record is at 2."),
        "{message}"
    );
    let stored = app.stored_item("A-100", "L-102").await.unwrap().unwrap();
    assert_eq!((stored.quantity, stored.version), (7, 2));
    assert_eq!(app.write_count("A-100", "L-102").await.unwrap(), 1);
    handle.release(prepared, false).await;

    // A write whose endpoint sends a version, in a pass that read none for
    // the record, is refused by the driver before anything is sent.
    let direct = Scene::new("example", &app.url(""));
    let write = write_of("A-100", "L-101").await;
    let (handle, mut prepared) = direct
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-100", "L-101", "Left rail", 5, "150.5");
    let refused = write
        .run(&direct, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Stale, "version_not_read")
    );
    assert_eq!(refused.sent, Sent::NotSent);

    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 0);
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!((stored.quantity, stored.version), (2, 1));
}
