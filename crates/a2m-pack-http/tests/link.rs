//! A credential never crosses an unencrypted link to another machine
//! (decision 0188): the test application's bearer-token routes, reached
//! over plain `http` at an address that is not a loopback address, with
//! and without the surface's declaration. The application is asked what it
//! saw.
//!
//! "Another machine" is an address the rule cannot take for this one: the
//! name `localhost`, which the rule never takes for a loopback address
//! (the machine's resolver decides where a name leads), and so reaches the
//! application only when the surface accepts it; and a documentation
//! address, which nothing answers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{FailureClass, Value, Values};
use a2m_provider::{Posture, Sent};
use common::{Folder, Scene, Signed, compile_changed_files, registry};
use example_app::harness::{BEARER_TOKEN, TestApp};

const AUTH: &str = "auth:
  - surfaces: [api]
    mode: custom
    provider: bearer_static
    probe: secure_order_get
    credentials:
      token: \"vault://example/service/token\"
    attempt_limit: 1
";

const ENDPOINT: &str = "endpoint: secure_order_get
surface: api
method: GET
path: \"/secure/bearer/api/orders/{order_id}\"
auth: token
effect: read
headers: {accept: \"application/json\"}
responses: {absent: [404]}
response_schema:
  type: object
  required: [order_id, status]
  properties:
    order_id: {type: string}
    status: {type: string}
";

const OPERATION: &str = "operation: example_secure_order_get
title: Get an order, signed in
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
outputs:
  status: {type: identifier}
bindings:
  - mode: http
    verb: read
    status: verified
    endpoint: secure_order_get
description: >
  Reads the status of one order through the routes that take a bearer token.
";

/// The fixture target with the signed-in read, its surface accepting an
/// unencrypted link or not, at `address`.
fn scene_of(accepted: bool, address: &str) -> (Scene, a2m_files::Diagnostics) {
    let folder = Folder::new("link");
    let signed_registry = registry(&folder, std::sync::Arc::new(a2m_core::NoSecrets));
    let target = move |text: &str| {
        if accepted {
            text.replace(
                "    kind: http\n",
                "    kind: http\n    unencrypted_credentials: accepted\n",
            )
        } else {
            text.to_owned()
        }
    };
    let compiled = compile_changed_files(
        &signed_registry,
        "example",
        &[
            ("target.yaml", &target),
            ("auth.yaml", &|_: &str| AUTH.to_owned()),
            ("endpoints/secure.yaml", &|_: &str| ENDPOINT.to_owned()),
            ("operations/secure.yaml", &|_: &str| OPERATION.to_owned()),
        ],
    )
    .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)));
    (
        Scene::over(folder, signed_registry, compiled.plan, address),
        compiled.warnings,
    )
}

fn warns(warnings: &a2m_files::Diagnostics) -> bool {
    warnings.find("http.unencrypted_credentials").is_some()
}

#[tokio::test]
async fn a_credential_crosses_an_unencrypted_link_to_another_machine_only_where_the_surface_accepts_it()
 {
    let app = TestApp::start().await.unwrap();
    let signed = Signed::new("bearer_static", &[], "token", BEARER_TOKEN).await;
    let lease = signed.lease().await;
    let order = Values::new().with("order_id", "A-100");
    let loopback = app.url("");
    let by_name = loopback.replace("127.0.0.1", "localhost");
    assert_ne!(by_name, loopback);

    // Not accepted: to a name, and to a documentation address, the pass is
    // refused before anything is sent, and the application saw nothing.
    for address in [by_name.as_str(), "http://192.0.2.10:9"] {
        let (scene, warnings) = scene_of(false, address);
        assert!(!warns(&warnings));
        let refused = scene
            .try_prepare(Some(&lease), None, Posture::Live)
            .await
            .err()
            .unwrap();
        assert_eq!(
            (refused.failure.class(), refused.failure.code().as_str()),
            (FailureClass::Policy, "link_not_encrypted"),
            "{address}"
        );
        assert_eq!(refused.sent, Sent::NotSent);
        assert!(!format!("{refused} {refused:?}").contains(BEARER_TOKEN));
    }
    assert!(app.seen().await.unwrap().is_empty());
    assert_eq!(app.auth_report().await.unwrap().refused_tokens, 0);

    // Over the loopback address nothing changes: the read goes through,
    // and its evidence says nothing of the link.
    let (scene, warnings) = scene_of(false, &loopback);
    assert!(!warns(&warnings));
    let (handle, mut prepared) = scene
        .try_prepare(Some(&lease), None, Posture::Live)
        .await
        .unwrap();
    let read = scene
        .read(&handle, &mut prepared, "example_secure_order_get", &order)
        .await
        .unwrap();
    handle.release(prepared, false).await;
    assert_eq!(read.values.get("status"), Some(&Value::from("draft")));
    assert!(!read.capture("acting").contains("unencrypted"));

    // Accepted: the lint warns, the read goes through to the name, the
    // application saw the request with its token, and the evidence of the
    // request says that its credential crossed an unencrypted link.
    let (scene, warnings) = scene_of(true, &by_name);
    assert!(warns(&warnings), "{}", common::shown(&warnings));
    let (handle, mut prepared) = scene
        .try_prepare(Some(&lease), None, Posture::Live)
        .await
        .unwrap();
    let read = scene
        .read(&handle, &mut prepared, "example_secure_order_get", &order)
        .await
        .unwrap();
    handle.release(prepared, false).await;
    assert_eq!(read.values.get("status"), Some(&Value::from("draft")));
    let acting = read.capture("acting");
    assert!(
        acting.contains("unencrypted_credentials_accepted"),
        "{acting}"
    );
    assert!(!acting.contains(BEARER_TOKEN));
    let seen = app.seen().await.unwrap();
    assert_eq!(
        seen.iter()
            .filter(|line| line.as_str() == "GET /secure/bearer/api/orders/A-100")
            .count(),
        2,
        "{seen:?}"
    );
    assert_eq!(app.auth_report().await.unwrap().refused_tokens, 0);
}
