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
use a2m_pack_http::codes;
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
    write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap();
    let refused = write
        .run(&scene, &handle, &mut prepared, "confirm", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Rejected, "app_validation_error")
    );
    assert_eq!(refused.sent, Sent::Answered);
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
    write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap();
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
    let plan = compile_changed(&registry, "example", "endpoints/orders.yaml", |text| {
        text.replacen(
            "enum: [draft, submitted]}\n    items:",
            "enum: [open, closed]}\n    items:",
            1,
        )
    })
    .unwrap()
    .plan;
    let drifted = Scene::over(folder, registry, plan, &app.url(""));
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

/// A link in front of the application that passes every request on, and
/// loses the answer to every PUT after the application has given it.
async fn losing_link(upstream: String) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut client, _)) = listener.accept().await else {
                return;
            };
            let upstream = upstream.clone();
            tokio::spawn(async move {
                let request = message(&mut client).await;
                if request.is_empty() {
                    return;
                }
                let mut server = TcpStream::connect(upstream).await.unwrap();
                server.write_all(&request).await.unwrap();
                let answer = message(&mut server).await;
                if request.starts_with(b"PUT ") {
                    // The application answered; the caller never hears it.
                    return;
                }
                // One request per connection, so that none is ever reused.
                let text = String::from_utf8(answer).unwrap();
                let closing = text.replacen("\r\n", "\r\nconnection: close\r\n", 1);
                client.write_all(closing.as_bytes()).await.unwrap();
                client.shutdown().await.unwrap();
            });
        }
    });
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

    // A connection that cannot be made at all is not a possible commit:
    // nothing was sent.
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let closed = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let nowhere = Scene::new("example", &closed);
    let write = write_of("A-102", "L-901").await;
    let (handle, mut prepared) = nowhere
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let unsent = write
        .run(&nowhere, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&unsent),
        (FailureClass::Transient, "connection_lost")
    );
    assert_eq!(unsent.sent, Sent::NotSent);
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
    ] {
        let order = Values::new().with("order_id", odd);
        let read = scene
            .read(&handle, &mut prepared, "example_order_get", &order)
            .await
            .unwrap();
        assert!(read.absent, "{odd}");
    }
    // The same holds for a write: refused before the ledger's token is used.
    let write = write_of("A-102", "x/../../L-1").await;
    let inputs = item("A-102", "x/../../L-1", "Corner post", 3, "150.5");
    let (writer, mut writing) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let refused = write
        .run(&scene, &writer, &mut writing, "act", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Input, "path_value_refused")
    );
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

    // TLS is not in this build: an HTTPS surface is refused by name.
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
        DriverHealth::Unavailable(codes::TLS_NOT_ENABLED)
    );
    assert_eq!(scene.handle().health(&scene.surface), DriverHealth::Ready);
}
