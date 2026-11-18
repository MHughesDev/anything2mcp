//! Sessions, sign-in signals, limits, and discovery: the `http` driver
//! against a small signed-in service that runs inside the test, and the
//! discoverers against it and against the built-in test application.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_core::{
    Archetype, EvidenceLevel, Failure, FailureClass, Finding, Redact, Source, Value, Values,
};
use a2m_files::{Drafter, TargetRoot};
use a2m_provider::{
    Access, CheckedPaths, Deadline, DescribeForm, DiscoveryScope, FindingSink, Posture,
    ReadOnlyPort, RootName, Roots, SandboxPort, Sent, StepFailure, StepInput, SurfaceDescription,
};
use common::{Folder, Scene, Signed, fixture};
use example_app::harness::TestApp;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const DESCRIPTION: &str = include_str!("fixtures/description/openapi.json");

/// One request the service saw: its first line, and the credential headers
/// it carried.
type Seen = (String, Vec<String>);

/// One provider's case: its name, settings, credential role, secret, and
/// the header and value the service must be given.
type Case<'a> = (
    &'a str,
    &'a [(&'a str, &'a str)],
    &'a str,
    &'a str,
    &'a str,
    &'a str,
);

/// The imaginary service: what it accepts as a credential, and what it saw.
struct Service {
    address: String,
    /// Every request: its first line, and the credential headers it carried.
    seen: Arc<Mutex<Vec<Seen>>>,
    /// When set, the service refuses the credential it accepted before.
    revoked: Arc<AtomicBool>,
}

impl Service {
    /// Starts the service. It accepts the request header `header` with
    /// exactly the value `value`.
    async fn start(header: &str, value: &str) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let revoked = Arc::new(AtomicBool::new(false));
        let (log, off) = (Arc::clone(&seen), Arc::clone(&revoked));
        let expected = format!("{}: {value}", header.to_ascii_lowercase());
        let echo = value.to_owned();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let (log, off) = (Arc::clone(&log), Arc::clone(&off));
                let (expected, echo) = (expected.clone(), echo.clone());
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let mut chunk = [0_u8; 4096];
                    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                        let read = stream.read(&mut chunk).await.unwrap();
                        if read == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&chunk[..read]);
                    }
                    let head = String::from_utf8_lossy(&bytes).into_owned();
                    let mut lines = head.lines();
                    let first = lines.next().unwrap_or_default().to_owned();
                    let headers: Vec<String> = lines
                        .take_while(|line| !line.is_empty())
                        .map(|line| {
                            let (name, value) = line.split_once(':').unwrap_or((line, ""));
                            format!("{}: {}", name.to_ascii_lowercase(), value.trim())
                        })
                        .collect();
                    let credentials: Vec<String> = headers
                        .iter()
                        .filter(|line| {
                            ["authorization:", "cookie:", "x-api-key:"]
                                .iter()
                                .any(|name| line.starts_with(name))
                        })
                        .cloned()
                        .collect();
                    let signed_in = headers.contains(&expected) && !off.load(Ordering::SeqCst);
                    log.lock()
                        .unwrap()
                        .push((first.clone(), credentials.clone()));
                    let json = "content-type: application/json";
                    let (status, extra, body) = match first.as_str() {
                        "GET /account HTTP/1.1" if signed_in => (
                            "200 OK",
                            format!("{json}\r\nx-echo: {echo}"),
                            format!(r#"{{"name":"service","echo":"{echo}"}}"#),
                        ),
                        "GET /account HTTP/1.1" => (
                            "401 Unauthorized",
                            format!("{json}\r\nwww-authenticate: Bearer realm=\"example\""),
                            r#"{"error":"unauthorized"}"#.to_owned(),
                        ),
                        "GET /notice HTTP/1.1" => (
                            "200 OK",
                            json.to_owned(),
                            format!(
                                r#"{{"name":"notice","echo":"{} credentials"}}"#,
                                credentials.len()
                            ),
                        ),
                        "GET /moved/login HTTP/1.1" => (
                            "302 Found",
                            "location: http://login.example.test/sign-in".to_owned(),
                            String::new(),
                        ),
                        "GET /moved/other HTTP/1.1" => (
                            "302 Found",
                            "location: http://other.example.test/elsewhere".to_owned(),
                            String::new(),
                        ),
                        // A sign-in redirect written without a scheme.
                        "GET /moved/relative HTTP/1.1" => (
                            "302 Found",
                            "location: //login.example.test/sign-in".to_owned(),
                            String::new(),
                        ),
                        // Another host, written the way a browser also reads.
                        "GET /moved/slashes HTTP/1.1" => (
                            "302 Found",
                            "location: /\\other.example.test/elsewhere".to_owned(),
                            String::new(),
                        ),
                        "GET /moved/within HTTP/1.1" => {
                            ("302 Found", "location: /account".to_owned(), String::new())
                        }
                        // A sign-in page answered with 200, as some systems do.
                        "GET /page HTTP/1.1" => (
                            "200 OK",
                            "content-type: text/html".to_owned(),
                            "<html><body>Sign in to continue</body></html>".to_owned(),
                        ),
                        "GET /large HTTP/1.1" => (
                            "200 OK",
                            json.to_owned(),
                            format!("\"{}\"", "x".repeat(10_000)),
                        ),
                        "GET /openapi.json HTTP/1.1" => {
                            ("200 OK", json.to_owned(), DESCRIPTION.to_owned())
                        }
                        "GET / HTTP/1.1" => (
                            "401 Unauthorized",
                            format!(
                                "{json}\r\nwww-authenticate: Bearer realm=\"example\"\r\nset-cookie: visit=secret-for-tests-cookie-0009"
                            ),
                            r#"{"error":"unauthorized"}"#.to_owned(),
                        ),
                        "OPTIONS / HTTP/1.1" => (
                            "204 No Content",
                            "allow: GET, HEAD, OPTIONS".to_owned(),
                            String::new(),
                        ),
                        _ => (
                            "404 Not Found",
                            json.to_owned(),
                            r#"{"error":"not_found"}"#.to_owned(),
                        ),
                    };
                    let answer = format!(
                        "HTTP/1.1 {status}\r\n{extra}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(answer.as_bytes()).await.unwrap();
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            address,
            seen,
            revoked,
        }
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let group = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let number = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
        for position in 0..4 {
            if position <= chunk.len() {
                out.push(char::from(
                    ALPHABET[(number >> (18 - 6 * position) & 63) as usize],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[tokio::test]
async fn a_credential_reaches_only_the_request_it_is_for_and_appears_nowhere_else() {
    let token = "secret-for-tests-bearer-0002";
    let service = Service::start("authorization", &format!("Bearer {token}")).await;
    let signed = Signed::new("bearer_static", &[], "token", token).await;
    let redact: Arc<dyn Redact> = signed.vault.redactor();
    let scene = Scene::with_redactor("secured", &service.address, redact);
    let lease = signed.lease().await;
    let (handle, mut prepared) = scene
        .try_prepare(Some(&lease), None, Posture::Live)
        .await
        .unwrap();
    let none = Values::new();

    // The endpoint that takes the session's credential is given it, and
    // the service answers. The service plants the token in a response
    // header and in the body: it comes back in no value and no capture.
    let read = scene
        .read(&handle, &mut prepared, "example_account_get", &none)
        .await
        .unwrap();
    assert_eq!(read.values.get("name"), Some(&Value::from("service")));
    let echoed = read.values.get("echo").and_then(Value::as_text).unwrap();
    assert!(!echoed.contains(token), "{echoed}");
    let exchange = read.capture("exchange");
    assert!(
        exchange.contains(r#"{"name":"authorization"}"#),
        "{exchange}"
    );
    assert!(exchange.contains(r#"{"name":"x-echo"}"#), "{exchange}");
    for text in [
        &exchange,
        &read.capture("acting"),
        &format!("{:?}", read.values),
    ] {
        assert!(
            !text.contains(token) && !text.contains("secret-for-tests"),
            "{text}"
        );
    }

    // An endpoint that takes no credential is sent none.
    let read = scene
        .read(&handle, &mut prepared, "example_notice_get", &none)
        .await
        .unwrap();
    assert_eq!(read.values.get("echo"), Some(&Value::from("0 credentials")));
    let seen = service.seen();
    assert_eq!(seen[0].1, vec![format!("authorization: Bearer {token}")]);
    assert!(seen[1].1.is_empty());

    // The service refuses the credential it took before: the provider
    // judges the sign, the step stops, and nothing is renewed or retried.
    service.revoked.store(true, Ordering::SeqCst);
    let before = service.seen().len();
    let expired = scene
        .read(&handle, &mut prepared, "example_account_get", &none)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&expired),
        (FailureClass::SessionExpired, "needs_login")
    );
    assert_eq!(expired.sent, Sent::Answered);
    assert!(!format!("{expired} {expired:?}").contains(token));
    assert_eq!(service.seen().len(), before + 1);

    // A redirect is never followed. To the sign-in host the target
    // declares, it is a sign of expiry; to any other host, a destination
    // the plan does not list.
    let before = service.seen().len();
    let moved = scene
        .read(
            &handle,
            &mut prepared,
            "example_moved_get",
            &Values::new().with("place", "login"),
        )
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&moved),
        (FailureClass::SessionExpired, "needs_login")
    );
    let moved = scene
        .read(
            &handle,
            &mut prepared,
            "example_moved_get",
            &Values::new().with("place", "other"),
        )
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&moved),
        (FailureClass::Policy, "surface_not_declared")
    );
    assert_eq!(moved.sent, Sent::Answered);
    // The same holds however the address is written: with no scheme, or
    // with the slashes a browser also takes for the start of a host. One
    // within the surface is a status like any other, and is not followed.
    for (place, class, code) in [
        ("relative", FailureClass::SessionExpired, "needs_login"),
        ("slashes", FailureClass::Policy, "surface_not_declared"),
        ("within", FailureClass::UnknownVariant, "unexpected_status"),
    ] {
        let moved = scene
            .read(
                &handle,
                &mut prepared,
                "example_moved_get",
                &Values::new().with("place", place),
            )
            .await
            .unwrap_err();
        assert_eq!(class_and_code(&moved), (class, code), "{place}");
        assert_eq!(moved.sent, Sent::Answered, "{place}");
    }
    assert_eq!(service.seen().len(), before + 5);

    // A body that is not JSON is never read as nothing, whatever shape the
    // endpoint declares: a sign-in page answered with 200 stops as drift,
    // and the page's text is not quoted.
    let page = scene
        .read(&handle, &mut prepared, "example_page_get", &none)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&page),
        (FailureClass::Drift, "response_schema_mismatch")
    );
    assert!(!page.failure.message().contains("Sign in"), "{page}");

    // An answer larger than its endpoint allows is not read to the end.
    let large = scene
        .read(&handle, &mut prepared, "example_large_get", &none)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&large),
        (FailureClass::UnknownVariant, "response_too_large")
    );
    handle.release(prepared, false).await;

    // With no session, an endpoint that takes one is not sent at all.
    let before = service.seen().len();
    let (handle, mut prepared) = scene.reader().await;
    let unsigned = scene
        .read(&handle, &mut prepared, "example_account_get", &none)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&unsigned),
        (FailureClass::SessionExpired, "needs_login")
    );
    assert_eq!(unsigned.sent, Sent::NotSent);
    assert_eq!(service.seen().len(), before);
}

/// A header whose name begins `a2m-` is one of the product's own carriers
/// between its parts, such as the browser's state a web sign-in places;
/// it never goes on the wire. A provider that places one stops the pass
/// before anything is sent, and the service hears nothing (decision 0270).
#[tokio::test]
async fn a_header_of_the_products_own_is_never_sent() {
    let none = Values::new();
    for name in ["a2m-browser-state", "A2M-Browser-State", "A2m-anything"] {
        let secret = "secret-for-tests-state-0270";
        let service = Service::start(name, secret).await;
        let signed = Signed::new("api_key", &[("header", name)], "key", secret).await;
        let scene = Scene::with_redactor("secured", &service.address, signed.vault.redactor());
        let lease = signed.lease().await;
        let refused = match scene.try_prepare(Some(&lease), None, Posture::Live).await {
            Err(refused) => refused,
            Ok((handle, mut prepared)) => {
                let read = scene
                    .read(&handle, &mut prepared, "example_account_get", &none)
                    .await;
                handle.release(prepared, false).await;
                panic!("{name}: prepared, and the read answered: {}", read.is_ok());
            }
        };
        assert_eq!(
            class_and_code(&refused),
            (FailureClass::Policy, "product_header_refused"),
            "{name}"
        );
        assert_eq!(refused.sent, Sent::NotSent, "{name}");
        assert!(!refused.failure.message().contains(secret), "{name}");
        assert!(service.seen().is_empty(), "{name}");
    }
}

#[tokio::test]
async fn each_scheme_the_driver_can_carry_is_placed_and_the_others_are_refused_by_name() {
    let none = Values::new();
    let basic = format!("Basic {}", base64(b"robot:secret-for-tests-basic-0003"));
    let cases: [Case<'_>; 2] = [
        (
            "api_key",
            &[("header", "X-Api-Key"), ("prefix", "Key ")],
            "key",
            "secret-for-tests-key-0001",
            "x-api-key",
            "Key secret-for-tests-key-0001",
        ),
        (
            "http_basic",
            &[("user", "robot")],
            "password",
            "secret-for-tests-basic-0003",
            "authorization",
            basic.as_str(),
        ),
    ];
    for (provider, settings, role, secret, header, value) in cases {
        let service = Service::start(header, value).await;
        let signed = Signed::new(provider, settings, role, secret).await;
        let scene = Scene::with_redactor("secured", &service.address, signed.vault.redactor());
        let lease = signed.lease().await;
        let (handle, mut prepared) = scene
            .try_prepare(Some(&lease), None, Posture::Live)
            .await
            .unwrap();
        let read = scene
            .read(&handle, &mut prepared, "example_account_get", &none)
            .await
            .unwrap_or_else(|failure| panic!("{provider}: {failure}"));
        assert_eq!(
            read.values.get("name"),
            Some(&Value::from("service")),
            "{provider}"
        );
        assert_eq!(
            service.seen()[0].1,
            vec![format!("{header}: {value}")],
            "{provider}"
        );
        handle.release(prepared, false).await;
    }

    // A scheme whose exchange is not built stops before anything is sent.
    let service = Service::start("authorization", "none").await;
    let signed = Signed::new(
        "http_digest",
        &[("user", "robot")],
        "password",
        "secret-for-tests-digest-0004",
    )
    .await;
    let scene = Scene::with_redactor("secured", &service.address, signed.vault.redactor());
    let lease = signed.lease().await;
    let (handle, mut prepared) = scene
        .try_prepare(Some(&lease), None, Posture::Live)
        .await
        .unwrap();
    let refused = scene
        .read(&handle, &mut prepared, "example_account_get", &none)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Input, "auth_scheme_not_built")
    );
    assert_eq!(refused.sent, Sent::NotSent);
    assert!(service.seen().is_empty());
}

#[derive(Default)]
struct Collected(Vec<Finding>);

impl FindingSink for Collected {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// Runs every discoverer of the pack that suits the scene's surface, each
/// through a port that can only read.
async fn discover(scene: &Scene, focus: &Values) -> Vec<(String, Result<Vec<Finding>, Failure>)> {
    discover_with(scene, focus, &CheckedPaths::none(), &a2m_core::NoSecrets).await
}

/// The same, with the files the discoverers may read and the redactor
/// they are given.
async fn discover_with(
    scene: &Scene,
    focus: &Values,
    paths: &CheckedPaths,
    redact: &dyn Redact,
) -> Vec<(String, Result<Vec<Finding>, Failure>)> {
    let description = SurfaceDescription {
        kind: scene.surface.surface.kind.clone(),
        archetype: Archetype::Request,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let mut results = Vec::new();
    for discoverer in scene.registry.discoverers_for(&description) {
        let (handle, prepared) = scene
            .try_prepare(None, None, Posture::Discovery)
            .await
            .unwrap();
        let port = ReadOnlyPort::new(handle, prepared);
        let scope = DiscoveryScope {
            surface: &scene.surface,
            bounds: discoverer.declared().bounds,
            deadline: Deadline::after(Duration::from_secs(30)),
            paths,
            redact,
            focus,
        };
        let mut sink = Collected::default();
        let result = discoverer.discover(&port, &scope, &mut sink).await;
        results.push((
            discoverer.declared().name.to_string(),
            result.map(|_| sink.0),
        ));
        let (handle, prepared) = port.into_parts();
        handle.release(prepared, false).await;
    }
    results
}

#[tokio::test]
async fn discovery_observes_and_cannot_write() {
    let app = TestApp::start().await.unwrap();
    let scene = Scene::new("example", &app.url(""));

    // Both discoverers run against the application: the surface is seen to
    // answer, and the description it serves at `/api/openapi.json`, one of
    // the places looked in by convention, is read as claims.
    let results = discover(&scene, &Values::new()).await;
    assert_eq!(results.len(), 2);
    for (name, result) in &results {
        match (name.as_str(), result) {
            ("http_surface", Ok(findings)) => {
                assert_eq!(findings.len(), 2);
                for finding in findings {
                    assert_eq!(finding.level(), EvidenceLevel::Discovered);
                    assert_eq!(finding.source(), Source::LiveSystem);
                    assert_eq!(finding.kind().as_str(), "endpoint");
                }
            }
            ("http_description", Ok(findings)) => {
                assert_eq!(findings.len(), 8);
                for finding in findings {
                    assert_eq!(finding.level(), EvidenceLevel::Inferred);
                    assert_eq!(finding.source(), Source::PublishedDescription);
                }
            }
            other => panic!("{other:?}"),
        }
    }

    // A write cannot pass the port, whatever a discoverer builds.
    let (handle, prepared) = scene
        .try_prepare(None, None, Posture::Discovery)
        .await
        .unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let inputs = Values::new()
        .with("order_id", "A-102")
        .with("item_code", "L-900")
        .with("description", "Corner post")
        .with("quantity", 3_i64)
        .with("length_in", a2m_core::Decimal::parse("150.5").unwrap());
    let save = scene.binding("example_line_item_upsert").act[0].clone();
    let mut unmarked = save.clone();
    unmarked.commits = false;
    let mut posting = scene.binding("example_order_list").act[0].clone();
    posting.kind = a2m_pack_http::PROBE;
    posting.refers_to = None;
    posting.payload = a2m_core::PackPayload::encode(
        &serde_json::json!({"method": "POST", "path": "/api/orders"}),
    )
    .unwrap();
    let expected = [
        (save, FailureClass::Policy, "read_only_posture"),
        (unmarked, FailureClass::Input, "step_not_usable"),
        (posting, FailureClass::Input, "step_not_usable"),
    ];
    for (step, class, code) in expected {
        let args = scene.args(&step, &inputs);
        let input = StepInput::new(&args, Deadline::after(Duration::from_secs(30)));
        let refused = port.act(&step, &input).await.unwrap_err();
        assert_eq!(class_and_code(&refused), (class, code));
        assert_eq!(refused.sent, Sent::NotSent);
    }

    // The application was read, and nothing in it was written.
    assert!(app.writes().await.unwrap().is_empty());
    assert_eq!(app.stored_item("A-102", "L-900").await.unwrap(), None);
}

#[tokio::test]
async fn a_published_description_becomes_claims_and_a_read_is_drafted_only_for_a_safe_method() {
    let service = Service::start("authorization", "none").await;
    let scene = Scene::new("secured", &service.address);
    let results = discover(&scene, &Values::new()).await;
    let findings: Vec<Finding> = results
        .into_iter()
        .filter(|(name, _)| name == "http_description")
        .flat_map(|(_, result)| result.unwrap())
        .collect();
    assert_eq!(findings.len(), 3);
    for finding in &findings {
        // A description is a claim: never stronger than `inferred`.
        assert_eq!(finding.level(), EvidenceLevel::Inferred);
        assert_eq!(finding.source(), Source::PublishedDescription);
        assert_eq!(finding.kind().as_str(), "described_operation");
    }
    // Discovery sent GET and OPTIONS only, with no credential, and what
    // the service planted in a cookie is in no finding.
    for (line, credentials) in service.seen() {
        assert!(
            line.starts_with("GET ") || line.starts_with("OPTIONS "),
            "{line}"
        );
        assert!(credentials.is_empty(), "{line}");
    }

    // The drafting rule, through the kernel's drafter, into a copy of the
    // target whose surface is at the service's address.
    let folder = Folder::new("draft");
    let target = std::fs::read_to_string(fixture("secured").join("target.yaml")).unwrap();
    std::fs::write(
        folder.path().join("target.yaml"),
        target.replace("http://127.0.0.1:4011", &service.address),
    )
    .unwrap();
    let root = TargetRoot::new(folder.path());
    let set = scene.registry.compiler_set();
    let report = Drafter::new(set, &root).draft(&findings).unwrap();
    assert!(report.conflicts.is_empty(), "{report:?}");
    let written: Vec<&str> = report
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .collect();
    // One read operation, for the GET whose path holds no placeholder: the
    // other GET's placeholder is a parameter the description does not
    // state, so its read is left to the developer.
    assert_eq!(
        written,
        [
            "endpoints/get_account.yaml",
            "endpoints/get_order.yaml",
            "operations/account_get.yaml"
        ]
    );
    let draft =
        |name: &str| std::fs::read_to_string(folder.path().join("endpoints").join(name)).unwrap();
    let read = draft("get_order.yaml");
    assert!(
        read.contains("effect: \"read\"") && read.contains("surface: \"api\""),
        "{read}"
    );
    assert!(
        read.contains("/orders/{order_id}") && read.contains("absent"),
        "{read}"
    );
    // The rule drafts the DELETE as what it is, a write, and the file kind
    // says so: the drafter refuses it and writes nothing for it. A write
    // is written by the developer.
    let refused: Vec<&str> = report
        .refused
        .iter()
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(refused, ["endpoints/delete_order.yaml"], "{report:?}");
    assert!(
        !folder
            .path()
            .join("endpoints")
            .join("delete_order.yaml")
            .exists()
    );
    // Drafting the same findings again gives the same bytes.
    let again = Drafter::new(set, &root).draft(&findings).unwrap();
    assert!(again.conflicts.is_empty(), "{again:?}");
    assert_eq!(draft("get_order.yaml"), read);

    // The describer counts, and keeps the system's own text apart.
    let description = scene.registry.discoverers_for(&SurfaceDescription {
        kind: scene.surface.surface.kind.clone(),
        archetype: Archetype::Request,
        address: service.address.clone(),
        facts: Values::new(),
    })[0]
        .describe(&findings, DescribeForm::Summary);
    assert!(
        description
            .text
            .starts_with("3 endpoints: 2 of a method HTTP marks safe, 1 that may write.")
    );
    assert!(!description.text.contains("/orders"));
    assert_eq!(description.untrusted.len(), 3);
}

/// The test application's description, found where it serves it, drafted
/// into a new target: endpoint entries and one read operation for each GET
/// it describes, which compile in draft mode and, run through the driver's
/// handle with the description's own example, read what the application
/// stores. No write is drafted and nothing is written.
#[tokio::test]
async fn the_test_applications_description_drafts_reads_that_read_what_it_stores() {
    let app = TestApp::start().await.unwrap();
    let address = app.url("");
    let scene = Scene::new("example", &address);
    let findings: Vec<Finding> = discover(&scene, &Values::new())
        .await
        .into_iter()
        .filter(|(name, _)| name == "http_description")
        .flat_map(|(_, result)| result.unwrap())
        .collect();
    assert_eq!(findings.len(), 8, "{findings:?}");

    let folder = Folder::new("drafted-reads");
    let target = std::fs::read_to_string(fixture("example").join("target.yaml")).unwrap();
    std::fs::write(
        folder.path().join("target.yaml"),
        target.replace("http://127.0.0.1:4010", &address),
    )
    .unwrap();
    let root = TargetRoot::new(folder.path());
    let set = scene.registry.compiler_set();
    let report = Drafter::new(set, &root).draft(&findings).unwrap();
    assert!(
        report.kept_failing.is_empty() && report.open_questions.is_empty(),
        "{report:?}"
    );
    let operations: Vec<&str> = report
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .filter(|path| path.starts_with("operations/"))
        .collect();
    assert_eq!(
        operations,
        [
            "operations/api_orders_get.yaml",
            "operations/api_orders_list.yaml",
            "operations/api_stock_get.yaml",
            "operations/orders_delivery_get.yaml",
            "operations/orders_items_get.yaml",
            "operations/orders_totals_get.yaml",
        ]
    );
    // The two PUTs are writes: their entries are refused, and no operation
    // is drafted for them.
    let refused: Vec<&str> = report
        .refused
        .iter()
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(
        refused,
        ["endpoints/put_delivery.yaml", "endpoints/put_item.yaml"]
    );

    let sources = a2m_files::load(&root, set).unwrap();
    let plan = a2m_files::compile(
        &sources,
        set,
        a2m_files::CompileMode::Draft,
        a2m_files::DRAFT_VERSION,
    )
    .unwrap()
    .plan;
    for operation in &plan.operations {
        assert_eq!(operation.effect, a2m_core::EffectClass::Read);
    }
    let examples = |name: &str| {
        plan.operations
            .iter()
            .find(|operation| operation.name.as_str() == name)
            .unwrap()
            .examples
            .first()
            .cloned()
    };
    // The list needs no input; each read by key has the description's own
    // example, and the outputs that came as text are marked untrusted.
    assert_eq!(examples("example_api_orders_list"), None);
    assert_eq!(
        examples("example_orders_items_get").unwrap(),
        Values::new()
            .with("order_id", "A-100")
            .with("item_code", "L-101")
    );
    let item = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_orders_items_get")
        .unwrap();
    let description = item
        .outputs
        .iter()
        .find(|output| output.name == "description")
        .unwrap();
    assert!(plan.field_types[description.field_type.unwrap().get()].untrusted);

    let reading = Scene::over(
        Folder::new("drafted-run"),
        common::registry(
            &Folder::new("drafted-registry"),
            Arc::new(a2m_core::NoSecrets),
        ),
        plan.clone(),
        &address,
    );
    let (handle, mut prepared) = reading.reader().await;
    let mut got = std::collections::BTreeMap::new();
    for name in [
        "example_orders_items_get",
        "example_api_orders_get",
        "example_api_orders_list",
    ] {
        let inputs = examples(name).unwrap_or_default();
        let ran = reading.read(&handle, &mut prepared, name, &inputs).await;
        got.insert(name, ran.unwrap().values);
    }
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    let item = &got["example_orders_items_get"];
    assert_eq!(
        item.get("description").and_then(Value::as_text),
        Some(stored.description.as_str())
    );
    assert_eq!(
        item.get("quantity"),
        Some(&Value::Int(i64::from(stored.quantity)))
    );
    let order = app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(
        got["example_api_orders_get"]
            .get("customer")
            .and_then(Value::as_text),
        Some(order.customer.as_str())
    );
    let listed = got["example_api_orders_list"]
        .get("order_id")
        .and_then(Value::as_list)
        .unwrap();
    assert!(listed.contains(&Value::from("A-100")), "{got:?}");
    handle.release(prepared, false).await;

    // The application was read, and nothing in it was written.
    assert!(app.writes().await.unwrap().is_empty());
}

/// A redactor that knows one secret.
struct Knows(&'static str);

impl Redact for Knows {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, a2m_core::REDACTED)
    }
}

#[tokio::test]
async fn the_description_reader_reads_the_file_it_is_handed_under_its_declared_name() {
    let service = Service::start("authorization", "none").await;
    let scene = Scene::new("handed", &service.address);
    let description = SurfaceDescription {
        kind: scene.surface.surface.kind.clone(),
        archetype: Archetype::Request,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    // The manifest declares the one name, and the registry gives it as
    // what a discovery of the surface may be handed (decision 0263).
    let reader = scene
        .registry
        .discoverers_for(&description)
        .into_iter()
        .find(|found| found.declared().name.as_str() == "http_description")
        .unwrap();
    assert_eq!(reader.declared().files, [a2m_pack_http::DESCRIPTION_FILE]);
    let taken: Vec<String> = scene
        .registry
        .files_taken(&description)
        .into_iter()
        .collect();
    assert_eq!(taken, [a2m_pack_http::DESCRIPTION_FILE]);

    // A file handed under that name is what the reader reads.
    let folder = Folder::new("handed");
    std::fs::write(folder.path().join("example-openapi.json"), DESCRIPTION).unwrap();
    let root = RootName::new("sources").unwrap();
    let mut roots = Roots::none();
    roots
        .add(root.clone(), folder.path(), Access::Read)
        .unwrap();
    let mut paths = CheckedPaths::none();
    paths.insert(
        taken[0].clone(),
        roots
            .check(&root, "example-openapi.json", Access::Read)
            .unwrap(),
    );
    let results = discover_with(&scene, &Values::new(), &paths, &a2m_core::NoSecrets).await;
    let findings: Vec<Finding> = results
        .into_iter()
        .filter(|(name, _)| name == "http_description")
        .flat_map(|(_, result)| result.unwrap())
        .collect();
    assert_eq!(findings.len(), 3);
    assert!(
        service
            .seen()
            .iter()
            .all(|(line, _)| !line.contains("openapi")),
    );
}

#[tokio::test]
async fn a_description_given_as_a_file_passes_the_redactor_before_anything_is_made_of_it() {
    // A description that holds a registered secret where a finding would
    // quote it: in an operation's name, a path, and a word of a shape.
    let secret = "secret-for-tests-described-0007";
    let planted = DESCRIPTION
        .replacen("getOrder", &format!("get{secret}"), 1)
        .replacen("/account", &format!("/account/{secret}"), 1)
        .replacen("\"draft\"", &format!("\"{secret}\""), 1);
    assert_eq!(planted.matches(secret).count(), 3);
    let folder = Folder::new("described");
    std::fs::write(folder.path().join("openapi.json"), planted).unwrap();
    let root = RootName::new("import").unwrap();
    let mut roots = Roots::none();
    roots
        .add(root.clone(), folder.path(), Access::Read)
        .unwrap();
    let mut paths = CheckedPaths::none();
    paths.insert(
        a2m_pack_http::DESCRIPTION_FILE,
        roots.check(&root, "openapi.json", Access::Read).unwrap(),
    );

    let service = Service::start("authorization", "none").await;
    let scene = Scene::new("secured", &service.address);
    let results = discover_with(&scene, &Values::new(), &paths, &Knows(secret)).await;
    let findings: Vec<Finding> = results
        .into_iter()
        .filter(|(name, _)| name == "http_description")
        .flat_map(|(_, result)| result.unwrap())
        .collect();
    assert_eq!(findings.len(), 3);
    for finding in &findings {
        let text = format!("{finding:?}");
        assert!(!text.contains("secret-for-tests"), "{text}");
    }
    // The file was read, not the surface: only `http_surface` sent anything.
    assert!(
        service
            .seen()
            .iter()
            .all(|(line, _)| !line.contains("openapi")),
    );
}
