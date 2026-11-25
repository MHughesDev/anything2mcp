//! How a target signs in is found, and a mode and a provider are
//! recommended, through the product (package 2.6, decisions 0321 and
//! 0367).
//!
//! `auth.detect` runs each surface's discoverers in the engine's sandbox
//! and gives the fixed rules of `a2m-auth` what they found. The three
//! variants of the test application's case 13 are each classified from the
//! page's structure and where it ended, and each is recommended: a password
//! form, a move to a separate identity page on another origin, and the same
//! form with the one-time code step switched on, which the page does not
//! show before a password is sent and which is therefore named unknown. The
//! application is asked afterwards, and was sent no sign-in form, no code,
//! and no sign-in at the identity page. A description's security schemes
//! are classified the same way for an HTTP surface. The recommendation is
//! marked `inferred` and is never what the target declares.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use common::Machine;
use example_app::auth::AuthSettings;
use example_app::harness::TestApp;
use serde_json::Value as Json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// A relay that gives the stand's sign-in entry an origin of its own: a
/// request for the page `/` is sent to the stand as a request for its
/// `start`, and every other request, and every answer, goes through as it
/// is. A surface is an origin, and its start page is the page `/`; the
/// stand keeps its sign-in under a path. Nothing is read or kept but the
/// request line.
struct Front {
    base: String,
    server: JoinHandle<()>,
}

impl Front {
    fn base(&self) -> &str {
        &self.base
    }

    async fn start(upstream: &str, start: &str) -> Self {
        let upstream = upstream
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_owned();
        let start = start.to_owned();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                tokio::spawn(relay(client, upstream.clone(), start.clone()));
            }
        });
        Self { base, server }
    }
}

impl Drop for Front {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn relay(mut client: TcpStream, upstream: String, start: String) {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 64 * 1024 || client.read(&mut byte).await.unwrap_or(0) == 0 {
            return;
        }
        head.push(byte[0]);
    }
    let Ok(text) = String::from_utf8(head) else {
        return;
    };
    let mut lines: Vec<String> = text
        .trim_end_matches("\r\n")
        .split("\r\n")
        .map(str::to_owned)
        .collect();
    let Some(first) = lines.first_mut() else {
        return;
    };
    let mut parts = first.splitn(3, ' ');
    let (Some(method), Some(path), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return;
    };
    let path = if path == "/" { start.as_str() } else { path };
    *first = format!("{method} {path} {version}");
    lines.retain(|line| !line.to_ascii_lowercase().starts_with("connection:"));
    lines.push("connection: close".to_owned());
    let head = format!("{}\r\n\r\n", lines.join("\r\n"));
    let Ok(mut stand) = TcpStream::connect(&upstream).await else {
        return;
    };
    if stand.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut stand).await;
}

/// The address every project of the tests names, which a test replaces.
const STAND: &str = "http://127.0.0.1:4010";

/// Runs `auth.detect` on a project whose one surface is at `address`, and
/// gives the one surface's entry.
async fn detect(project: &str, address: &str) -> Json {
    let machine = Machine::new(project, &[(STAND, address)]);
    // The browsers' profiles live in the data directory's own folder,
    // which the product hands the web pack only when it exists.
    std::fs::create_dir_all(a2m_actions::Paths::under(machine.home.path()).profiles).unwrap();
    let world = machine.open().await;
    let result = world.ok(&["auth", "detect", "example"]).await;
    world.close().await;
    let surfaces = result["surfaces"].as_array().unwrap();
    assert_eq!(surfaces.len(), 1, "{result}");
    surfaces[0].clone()
}

/// The application was sent no sign-in form, no code, and no sign-in at its
/// identity page.
async fn nothing_sent(app: &TestApp) {
    let report = app.auth_report().await.unwrap();
    assert_eq!(report.sign_in_attempts, 0, "{report:?}");
    assert_eq!(report.failed_codes, 0, "{report:?}");
    assert_eq!(report.sign_ins, 0, "{report:?}");
    assert_eq!(report.identity_provider_sign_ins, 0, "{report:?}");
    assert!(app.writes().await.unwrap().is_empty());
}

/// The recommendation of a surface's entry: inferred, never declared.
#[track_caller]
fn recommendation(surface: &Json) -> &Json {
    assert_eq!(surface["level"], "inferred", "{surface}");
    assert!(
        surface["declared"].as_array().is_none_or(Vec::is_empty),
        "{surface}"
    );
    assert_eq!(surface["method"], Json::Null, "{surface}");
    let recommended = &surface["recommendation"];
    assert_eq!(recommended["level"], "inferred", "{recommended}");
    recommended
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_sign_in_variant_of_case_13_is_classified_and_recommended_and_nothing_is_sent() {
    // A password form under `/secure`: the person signs in on a page of the
    // application, which `form_login` can do with a stored credential.
    let app = TestApp::start().await.unwrap();
    let front = Front::start(&app.url(""), "/secure").await;
    let form = detect("sign_in_detect", front.base()).await;
    let form = recommendation(&form);
    assert_eq!(form["provider"], "form_login", "{form}");
    assert_eq!(form["mode"], "managed_session", "{form}");
    assert_eq!(form["choices"][0]["sign"], "password_form", "{form}");
    assert_eq!(form["choices"][0]["also"][0], "window_login", "{form}");
    assert!(
        form["unknown"]
            .as_array()
            .unwrap()
            .contains(&Json::from("second_factor")),
        "{form}"
    );
    nothing_sent(&app).await;

    // The same form with the one-time code step on. The code is asked for
    // only after a password is sent, so the page shows no sign of it: the
    // second factor stays unknown, and a person says (decision 0321).
    app.set_auth_settings(&AuthSettings {
        second_factor: true,
        ..AuthSettings::default()
    })
    .await
    .unwrap();
    let stepped = detect("sign_in_detect", front.base()).await;
    let stepped = recommendation(&stepped);
    assert_eq!(stepped["choices"][0]["sign"], "password_form", "{stepped}");
    assert_eq!(stepped["provider"], "form_login", "{stepped}");
    assert!(
        stepped["unknown"]
            .as_array()
            .unwrap()
            .contains(&Json::from("second_factor")),
        "{stepped}"
    );
    nothing_sent(&app).await;

    // A move to a separate identity page on another origin: reported as
    // OpenID Connect, because no `SAMLRequest` form post was seen.
    let sso = TestApp::start_with_identity_provider().await.unwrap();
    let front = Front::start(&sso.url(""), "/secure/sso").await;
    let moved = detect("sign_in_detect", front.base()).await;
    let moved = recommendation(&moved);
    assert_eq!(moved["provider"], "sso_oidc", "{moved}");
    assert_eq!(moved["mode"], "managed_session", "{moved}");
    assert_eq!(moved["choices"][0]["sign"], "other_origin", "{moved}");
    nothing_sent(&sso).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_descriptions_security_schemes_are_classified_for_an_http_surface() {
    let app = TestApp::start().await.unwrap();
    let surface = detect("orders", &app.url("")).await;
    let recommended = recommendation(&surface);
    let providers: Vec<&str> = recommended["choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| choice["provider"].as_str().unwrap())
        .collect();
    // The description the test application publishes declares a bearer
    // token and an API key, and the recommendation holds both, for a
    // person to choose between; each is a dedicated credential.
    assert_eq!(providers, ["api_key", "bearer_static"], "{recommended}");
    assert_eq!(recommended["choices"][0]["mode"], "custom", "{recommended}");
    // Nothing of a scheme's own text reaches the answer: not the name of
    // the cookie, nor of the header, the description gives its keys.
    let shown = surface.to_string().to_ascii_lowercase();
    for text in ["example_session", "x-api-key"] {
        assert!(!shown.contains(text), "{text}: {shown}");
    }
    nothing_sent(&app).await;
}

#[test]
fn the_signs_the_web_pack_names_are_signs_the_rules_know() {
    for name in a2m_pack_web_probe::SIGN_IN_SIGNS {
        assert!(a2m_auth::SignInSign::parse(name).is_some(), "{name}");
    }
}
