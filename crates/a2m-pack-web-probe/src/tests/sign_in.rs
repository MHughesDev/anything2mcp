//! `web_auth` against the three sign-in variants of the test application's
//! case 13: a password form under `/secure`, a move to a separate identity
//! page on another origin, and the same form with the one-time code step
//! switched on. Each is classified from the page's structure and where it
//! ended; the application is asked afterwards how many sign-in forms and
//! codes it was sent, and it was sent none.

use a2m_core::{EvidenceLevel, FailureClass};
use example_app::auth::AuthSettings;
use example_app::harness::TestApp;
use serde_json::json;

use super::*;
use crate::codes;

/// The one sign-in finding of a discovery, parsed.
#[track_caller]
fn sign_in_of(seen: &Seen) -> Json {
    let found: Vec<&Finding> = seen
        .findings
        .iter()
        .filter(|finding| finding.kind().as_str() == "sign_in_method")
        .collect();
    assert_eq!(found.len(), 1, "{:#?}", seen.paths());
    assert_eq!(found[0].level(), EvidenceLevel::Inferred);
    assert!(!found[0].limitations().is_empty());
    serde_json::from_str(found[0].found()).unwrap()
}

/// The application was sent no sign-in form and no code.
async fn nothing_sent(app: &TestApp) {
    let report = app.auth_report().await.unwrap();
    assert_eq!(report.sign_in_attempts, 0, "{report:?}");
    assert_eq!(report.failed_codes, 0, "{report:?}");
    assert_eq!(report.sign_ins, 0, "{report:?}");
    assert_eq!(report.identity_provider_sign_ins, 0, "{report:?}");
}

/// Pages beside the test application, on an origin of their own: a page
/// that asks for a one-time code alone, and a sign-in page that shows no
/// field any rule classifies.
async fn beside() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route(
            "/code",
            get(|| async {
                page(
                    "One-time code",
                    "<form method=\"post\" action=\"/code\"><label for=\"c\">Code</label> \
                     <input id=\"c\" name=\"code\" inputmode=\"numeric\" autocomplete=\"one-time-code\"> \
                     <button type=\"submit\">Continue</button></form>",
                )
            }),
        )
        .route(
            "/sign-in",
            get(|| async {
                page(
                    "Sign in",
                    "<p>Use the key on your device.</p><button type=\"button\">Continue</button>",
                )
            }),
        );
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (base, server)
}

#[tokio::test(flavor = "multi_thread")]
async fn each_sign_in_variant_of_case_13_is_classified_and_nothing_is_sent() {
    let bench = Bench::new("sign-in", Arc::new(NoSecrets));

    // A password form: the signed-in part sends a visitor to its sign-in
    // page, on its own origin.
    let app = TestApp::start().await.unwrap();
    let form = sign_in_of(&must(
        bench
            .run("web_auth", &app.url("/secure"), &Look::default())
            .await,
    ));
    assert_eq!(form["sign_in"], "password_form", "{form}");
    assert_eq!(form["second_factor"], "unknown", "{form}");
    assert_eq!(form["redirected"], true, "{form}");
    assert_eq!(form["identity_origin"], Json::Null);
    assert_eq!(form["submitted"], false);
    nothing_sent(&app).await;

    // The same form with the code step switched on: the code is asked for
    // only after a password is sent, so the page shows no sign of it, and
    // it is not guessed (decision 0321).
    app.set_auth_settings(&AuthSettings {
        second_factor: true,
        ..AuthSettings::default()
    })
    .await
    .unwrap();
    let stepped = sign_in_of(&must(
        bench
            .run("web_auth", &app.url("/secure"), &Look::default())
            .await,
    ));
    assert_eq!(stepped["sign_in"], "password_form", "{stepped}");
    assert_eq!(stepped["second_factor"], "unknown", "{stepped}");
    nothing_sent(&app).await;

    // A move to a separate identity page on another origin: the browser
    // ends the move there, and nothing of that origin is fetched.
    let sso = TestApp::start_with_identity_provider().await.unwrap();
    let moved = sign_in_of(&must(
        bench
            .run("web_auth", &sso.url("/secure/sso"), &Look::default())
            .await,
    ));
    assert_eq!(moved["sign_in"], "other_origin", "{moved}");
    let provider = sso.provider_url("").unwrap();
    assert_eq!(
        moved["identity_origin"],
        json!(provider.trim_end_matches('/')),
        "{moved}"
    );
    nothing_sent(&sso).await;

    // A page that asks for a one-time code is a code step; a page that reads
    // as a sign-in and shows nothing any rule classifies is a named stop.
    let (base, server) = beside().await;
    let code = sign_in_of(&must(
        bench
            .run("web_auth", &format!("{base}/code"), &Look::default())
            .await,
    ));
    assert_eq!(code["sign_in"], "code_step", "{code}");
    assert_eq!(code["second_factor"], "asked", "{code}");
    let Err(unknown) = bench
        .run("web_auth", &format!("{base}/sign-in"), &Look::default())
        .await
    else {
        panic!("a sign-in no rule covers was classified");
    };
    assert_eq!(unknown.class(), FailureClass::UnknownVariant);
    assert_eq!(unknown.code(), &codes::SIGN_IN_UNRECOGNIZED);
    server.abort();

    // An open page gives no sign-in finding.
    let open = must(bench.run("web_auth", &app.url("/"), &Look::default()).await);
    assert!(open.findings.is_empty(), "{:#?}", open.paths());
    bench.end().await;
}
