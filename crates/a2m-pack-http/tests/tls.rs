//! The `http` driver over HTTPS, behind the feature `tls`: the test
//! application is served over TLS with certificates a test-only authority
//! makes when the test runs, trusted through the engine's setting for
//! further roots. Every write is proven by asking the application, over its
//! plain control address, what it stored.

#![cfg(feature = "tls")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::OnceLock;
use std::time::Duration;

use a2m_core::{Decimal, FailureClass, Value, Values};
use a2m_ledger::{Fence, IntentToken, Store};
use a2m_provider::{Posture, Sent, StepFailure};
use common::{Ran, Scene, key, record, running, store, token};
use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;
use example_app::tls::{ServerCertificate, TestAuthority};

const LONG: Duration = Duration::from_secs(30);

/// The authority this test binary's engine trusts, named the way a person
/// names one: the PEM text of `network.trust_roots`. It is installed before
/// any client of the engine is made.
fn trusted() -> &'static TestAuthority {
    static AUTHORITY: OnceLock<TestAuthority> = OnceLock::new();
    AUTHORITY.get_or_init(|| {
        let authority = TestAuthority::new("example test authority").unwrap();
        let trust = a2m_auth::tls::Trust::with_named_roots(&[authority.pem().as_bytes()]).unwrap();
        assert_eq!(trust.named_roots(), 1);
        a2m_auth::tls::install(trust).unwrap();
        authority
    })
}

async fn served(shape: ServerCertificate) -> (TestApp, Scene) {
    let app = TestApp::start_tls(trusted().server(shape).unwrap())
        .await
        .unwrap();
    let scene = Scene::new("example", &app.secure_url(""));
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

struct Write {
    _store: Store,
    fence: Fence,
    token: IntentToken,
    key: a2m_core::RecordKey,
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
async fn a_read_and_a_verified_write_over_https_trust_the_named_authority() {
    let (app, scene) = served(ServerCertificate::Good).await;
    assert!(scene.surface.surface.address.starts_with("https://"));

    let (handle, mut prepared) = scene.reader().await;
    let order = Values::new().with("order_id", "A-100");
    let read = scene
        .read(&handle, &mut prepared, "example_order_get", &order)
        .await
        .unwrap();
    assert_eq!(read.values.get("status"), Some(&Value::from("draft")));
    handle.release(prepared, false).await;

    let write = write_of("A-102", "L-900").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-102", "L-900", "Corner post", 3, "150.5");
    for list in ["read_back", "act", "confirm"] {
        write
            .run(&scene, &handle, &mut prepared, list, &inputs)
            .await
            .unwrap();
    }
    let after = write
        .run(&scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    handle.release(prepared, false).await;
    assert_eq!(after.values.get("quantity"), Some(&Value::Int(3)));

    // The application, asked over its plain control address, stored it once.
    let stored = app.stored_item("A-102", "L-900").await.unwrap().unwrap();
    assert_eq!((stored.quantity, stored.length_sixteenths), (3, 2408));
    assert_eq!(app.write_count("A-102", "L-900").await.unwrap(), 1);
}

#[tokio::test]
async fn each_certificate_made_wrong_is_named_and_nothing_is_sent() {
    // An authority nobody trusts stands for an unknown one.
    let stranger = TestAuthority::new("an authority nobody trusts").unwrap();
    let untrusted = stranger.server(ServerCertificate::Good).unwrap();
    // On macOS the system's verifier refuses an unknown authority without
    // saying which check failed; everywhere else it names it.
    let untrusted_codes: &[&str] = if cfg!(target_vendor = "apple") {
        &["certificate_untrusted", "certificate_not_verified"]
    } else {
        &["certificate_untrusted"]
    };
    let cases = [
        (untrusted, untrusted_codes),
        (
            trusted().server(ServerCertificate::WrongName).unwrap(),
            &["certificate_name_mismatch"][..],
        ),
        (
            trusted().server(ServerCertificate::Expired).unwrap(),
            &["certificate_expired"][..],
        ),
    ];
    for (server, codes) in cases {
        let app = TestApp::start_tls(server).await.unwrap();
        let scene = Scene::new("example", &app.secure_url(""));

        let (handle, mut prepared) = scene.reader().await;
        let order = Values::new().with("order_id", "A-100");
        let refused = scene
            .read(&handle, &mut prepared, "example_order_get", &order)
            .await
            .unwrap_err();
        let (class, code) = class_and_code(&refused);
        assert_eq!(class, FailureClass::UnknownVariant, "{refused}");
        assert!(codes.contains(&code), "{code}: {refused}");
        assert_eq!(refused.sent, Sent::NotSent);
        handle.release(prepared, true).await;

        // A write's lookup is refused the same way, so its write is never
        // sent, and nothing is tried again: the application stored nothing.
        let write = write_of("A-102", "L-900").await;
        let (handle, mut prepared) = scene
            .try_prepare(None, Some(write.fence), Posture::Live)
            .await
            .unwrap();
        let inputs = item("A-102", "L-900", "Corner post", 3, "150.5");
        let refused = write
            .run(&scene, &handle, &mut prepared, "read_back", &inputs)
            .await
            .unwrap_err();
        assert!(codes.contains(&class_and_code(&refused).1), "{refused}");
        assert_eq!(refused.sent, Sent::NotSent);
        let refused = write
            .run(&scene, &handle, &mut prepared, "act", &inputs)
            .await
            .unwrap_err();
        assert_eq!(refused.sent, Sent::NotSent);
        handle.release(prepared, true).await;
        assert!(app.writes().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn a_write_lost_after_it_was_sent_over_https_is_in_doubt() {
    let (app, scene) = served(ServerCertificate::Good).await;
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
    app.set_fault(&Fault::once(FaultKind::LostAfterCommit))
        .await
        .unwrap();
    let lost = write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&lost),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert_eq!(lost.sent, Sent::Unknown);
    handle.release(prepared, true).await;
    assert_eq!(app.write_count("A-102", "L-900").await.unwrap(), 1);
}

#[tokio::test]
async fn a_redirect_from_https_to_plain_http_is_not_followed() {
    let (app, scene) = served(ServerCertificate::Good).await;
    let write = write_of("A-102", "L-900").await;
    let (handle, mut prepared) = scene
        .try_prepare(None, Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let inputs = item("A-102", "L-900", "Corner post", 3, "150.5");
    write
        .run(&scene, &handle, &mut prepared, "read_back", &inputs)
        .await
        .unwrap();
    // The same application at its plain address: a redirect there would
    // carry the request, and any credential, off TLS.
    let location = app.url("/api/orders/A-102/items/L-900");
    app.set_fault(&Fault::once(FaultKind::RedirectAfterCommit { location }))
        .await
        .unwrap();
    let redirected = write
        .run(&scene, &handle, &mut prepared, "act", &inputs)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&redirected),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    handle.release(prepared, true).await;
    // Stored once, by the request that was sent. The redirect was not
    // followed: the write is in doubt, to be settled by reading it back
    // over HTTPS, and the client sends nothing that is not `https`.
    assert_eq!(app.write_count("A-102", "L-900").await.unwrap(), 1);
}
