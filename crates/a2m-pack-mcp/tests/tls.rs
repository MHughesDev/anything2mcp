//! The driver over HTTPS, behind the feature `tls`: the MCP stand is
//! reached through the test application's TLS front, with certificates a
//! test-only authority makes when the test runs, trusted through the
//! engine's setting for further roots. Every write is proven by asking the
//! stand what it stored.

#![cfg(feature = "tls")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::OnceLock;

use a2m_core::{CompiledTarget, FailureClass, Value, Values};
use a2m_provider::{Authority, Posture, Registry, Sent, StepFailure, StepOutcome, SurfaceRef};
use common::{
    Folder, compiled_fixture, fence_of, input, key, prepare, record, registry, running, steps,
    store, surface, token,
};
use example_app::tls::{ServerCertificate, TestAuthority, TlsFront};
use example_mcp::faults::{Fault, FaultKind};
use example_mcp::harness::HttpStand;

/// The authority this test binary's engine trusts, named the way a person
/// names one: the PEM text of `network.trust_roots`. It is installed before
/// any client of the engine is made.
fn trusted() -> &'static TestAuthority {
    static AUTHORITY: OnceLock<TestAuthority> = OnceLock::new();
    AUTHORITY.get_or_init(|| {
        let authority = TestAuthority::new("example test authority").unwrap();
        let trust = a2m_auth::tls::Trust::with_named_roots(&[authority.pem().as_bytes()]).unwrap();
        a2m_auth::tls::install(trust).unwrap();
        authority
    })
}

struct Scene {
    _folder: Folder,
    registry: Registry,
    compiled_target: CompiledTarget,
    stand: HttpStand,
    _front: TlsFront,
    surface: SurfaceRef,
}

/// The stand behind a TLS front that shows a certificate of `shape`, made
/// by `authority`; the surface is the front's `https` address.
async fn scene(authority: &TestAuthority, shape: ServerCertificate) -> Scene {
    let server = authority.server(shape).unwrap();
    let folder = Folder::new("tls");
    let registry = registry(&folder);
    let compiled_target = compiled_fixture(&registry);
    let stand = HttpStand::start().await.unwrap();
    let plain = stand.url();
    let upstream = plain
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let front = TlsFront::start(server, upstream).await.unwrap();
    let address = format!("https://{}/mcp", front.address());
    let surface = surface(&compiled_target, "orders", Some(&address));
    Scene {
        _folder: folder,
        registry,
        compiled_target,
        stand,
        _front: front,
        surface,
    }
}

impl Scene {
    async fn get(&self, order: &str) -> Result<StepOutcome, StepFailure> {
        let (handle, mut prepared) = prepare(
            &self.registry,
            &self.compiled_target,
            &self.surface,
            fence_of(7),
            Posture::Live,
            None,
        )
        .await?;
        let step = steps(&self.compiled_target, "example_order_get").act;
        let args = Values::new().with("order_id", order);
        let result = handle
            .act(
                &mut prepared,
                &step,
                &input(&args, None, None, None),
                Authority::Read,
            )
            .await;
        handle.release(prepared, result.is_err()).await;
        result
    }
}

fn failed(outcome: Result<StepOutcome, StepFailure>) -> (FailureClass, String, Sent) {
    let failure = outcome.expect_err("the step must fail");
    (
        failure.failure.class(),
        failure.failure.code().as_str().to_owned(),
        failure.sent,
    )
}

#[tokio::test]
async fn a_read_and_a_verified_write_over_https_trust_the_named_authority() {
    let scene = scene(trusted(), ServerCertificate::Good).await;
    let read = match scene.get("A-100").await {
        Ok(StepOutcome::Done(observed)) => observed,
        other => panic!("the read was not done: {other:?}"),
    };
    assert_eq!(read.values.get("status"), Some(&Value::from("draft")));

    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["A-100"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.compiled_target,
        &scene.surface,
        fence,
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let update = steps(&scene.compiled_target, "example_order_note_update");
    let args = Values::new()
        .with("order_id", "A-100")
        .with("field", "note")
        .with("value", "Leave at the gate");
    let order = key("A-100");
    let intent = token(&ledger, fence, 0, "A-100").await;
    let written = handle
        .act(
            &mut prepared,
            &update.act,
            &input(&args, Some(record(&fence, 0)), Some(&order), None),
            Authority::Write(&intent),
        )
        .await;
    assert!(matches!(written, Ok(StepOutcome::Done(_))), "{written:?}");
    handle.release(prepared, false).await;

    // The stand, not the driver, says what was stored and how often.
    let control = scene.stand.control();
    assert_eq!(control.write_count("A-100").unwrap(), 1);
    let stored = control.stored_order("A-100").unwrap().unwrap();
    assert_eq!(stored.note, "Leave at the gate");
}

#[tokio::test]
async fn each_certificate_made_wrong_is_named_and_nothing_reaches_the_server() {
    let stranger = TestAuthority::new("an authority nobody trusts").unwrap();
    // On macOS the system's verifier refuses an unknown authority without
    // saying which check failed; everywhere else it names it.
    let untrusted_codes: &[&str] = if cfg!(target_vendor = "apple") {
        &["certificate_untrusted", "certificate_not_verified"]
    } else {
        &["certificate_untrusted"]
    };
    let cases = [
        (&stranger, ServerCertificate::Good, untrusted_codes),
        (
            trusted(),
            ServerCertificate::WrongName,
            &["certificate_name_mismatch"][..],
        ),
        (
            trusted(),
            ServerCertificate::Expired,
            &["certificate_expired"][..],
        ),
    ];
    for (authority, shape, codes) in cases {
        let scene = scene(authority, shape).await;
        let (class, code, sent) = failed(scene.get("A-100").await);
        assert_eq!(class, FailureClass::UnknownVariant, "{code}");
        assert!(codes.contains(&code.as_str()), "{code}");
        assert_eq!(sent, Sent::NotSent);
        // Not even the handshake of the protocol reached the server.
        assert_eq!(
            scene.stand.control().request_count("initialize").unwrap(),
            0
        );
        assert!(scene.stand.control().writes().unwrap().is_empty());
    }
}

#[tokio::test]
async fn a_write_lost_after_it_was_sent_over_https_is_in_doubt() {
    let scene = scene(trusted(), ServerCertificate::Good).await;
    let control = scene.stand.control();
    control
        .set_fault(Fault::once(FaultKind::DropAfterCommit).on_tool("orders_create"))
        .unwrap();
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["new"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.compiled_target,
        &scene.surface,
        fence,
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let create = steps(&scene.compiled_target, "example_order_create");
    let args = Values::new()
        .with("customer", "Fourth Example Customer")
        .with("note", Value::Null);
    let intent = token(&ledger, fence, 0, "new").await;
    let lost = handle
        .act(
            &mut prepared,
            &create.act,
            &input(&args, Some(record(&fence, 0)), None, Some("STAMP-0001")),
            Authority::Write(&intent),
        )
        .await;
    assert_eq!(
        failed(lost),
        (
            FailureClass::PossibleCommit,
            a2m_provider::codes::NO_CONFIRMATION.as_str().to_owned(),
            Sent::Unknown
        )
    );
    handle.release(prepared, true).await;
    assert_eq!(control.write_count("A-103").unwrap(), 1);
}
