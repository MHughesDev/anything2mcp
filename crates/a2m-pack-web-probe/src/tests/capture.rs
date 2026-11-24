//! Traffic capture through the provider contract (decision 0399), against
//! the test application with one page of this file's own beside it: a
//! page whose script sets a cookie and makes its own requests with a
//! bearer token in a header, a token in the address, and a secret in the
//! query. What the capture yields, what `web_capture` finds over it, and
//! what was logged hold none of them.

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Environment, PackPayload, Redact, SurfaceKind, SurfaceName, SurfacePlan, TargetName,
};
use a2m_provider::{CAPTURE_PLACEHOLDER, Deadline, SurfaceRef};
use axum::Router;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::get;

use super::{Bench, Look, SECRET, must, page};
use crate::SURFACE_KIND;

/// A made-up secret in the form of a plain word: a rule of form keeps it
/// as a path's segment, and only the redactor knows it is one.
const WORD_SECRET: &str = "plainsecretword";

/// A redactor that knows both planted secrets.
struct KnowsBoth;

impl Redact for KnowsBoth {
    fn redact(&self, text: &str) -> String {
        text.replace(SECRET, "[redacted]")
            .replace(WORD_SECRET, "[redacted]")
    }
}

fn capture_pages() -> Router {
    Router::new()
        .route(
            "/capture/page",
            get(|| async {
                page(
                    "Makes its own requests",
                    &format!(
                        "<script>document.cookie = 'sid={SECRET}; path=/'; \
                         fetch('/capture/{WORD_SECRET}/data.json?key={SECRET}&{SECRET}&view=full', \
                         {{headers: {{'Authorization': 'Bearer {SECRET}', 'X-View': 'full'}}}}); \
                         fetch('/capture/t/{SECRET}/data.json');</script>"
                    ),
                )
            }),
        )
        .route("/capture/{word}/data.json", get(|| async { data() }))
        .route("/capture/t/{token}/data.json", get(|| async { data() }))
}

fn data() -> axum::response::Response {
    (
        [(CONTENT_TYPE, "application/json")],
        format!("{{\"id\":7,\"token\":\"{SECRET}\",\"items\":[{{\"name\":\"{SECRET}\"}}]}}"),
    )
        .into_response()
}

/// The test application with the page above, on a free port.
async fn serve() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = example_app::router().merge(capture_pages());
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (base, server)
}

fn surface(address: &str) -> SurfaceRef {
    SurfaceRef {
        target: TargetName::new("example").unwrap(),
        environment: Environment::new("test").unwrap(),
        surface: SurfacePlan {
            name: SurfaceName::new("app").unwrap(),
            kind: SurfaceKind::new(SURFACE_KIND).unwrap(),
            address: address.to_owned(),
            same_session_as: None,
            stands: None,
            rate: None,
            programs: Vec::new(),
            payload: PackPayload::none(),
        },
    }
}

/// Every file under the bench's folder of logs, as text.
fn logs(bench: &Bench) -> String {
    let mut all = String::new();
    let mut folders = vec![bench._folder.0.join("logs")];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                all.push_str(&String::from_utf8_lossy(&std::fs::read(&path).unwrap()));
            }
        }
    }
    all
}

#[tokio::test(flavor = "multi_thread")]
async fn a_capture_of_a_pages_own_requests_yields_structure_and_no_planted_secret() {
    let (base, server) = serve().await;
    let bench = Bench::new("capture", Arc::new(KnowsBoth));
    // The bench holds this pack alone, so the owner of the kind it reads is
    // missing; the pack is registered all the same.
    assert!(
        bench.problems.iter().all(|problem| problem.registered),
        "{:?}",
        bench.problems
    );

    // The pack declares the capability for `web` surfaces, and is the one
    // asked.
    let handles = bench
        .registry
        .captures_for(&SurfaceKind::new(SURFACE_KIND).unwrap());
    assert_eq!(handles.len(), 1);
    assert_eq!(handles[0].provider().as_str(), crate::PACK);

    let page_address = format!("{base}/capture/page");
    let capturing = must(
        handles[0]
            .open(
                &surface(&page_address),
                Deadline::after(Duration::from_secs(60)),
            )
            .await,
    );
    let taken = capturing
        .take(Deadline::after(Duration::from_secs(10)))
        .await;
    let text = format!("{taken:?}");
    for planted in [SECRET, WORD_SECRET] {
        assert!(!text.contains(planted), "{text}");
    }
    assert_eq!((taken.awaited, taken.withheld), (0, 0), "{taken:?}");
    let mut paths: Vec<&str> = taken.requests.iter().map(|r| r.path.as_str()).collect();
    paths.sort_unstable();
    let placeheld = format!("/capture/{CAPTURE_PLACEHOLDER}/data.json");
    let token_path = format!("/capture/t/{CAPTURE_PLACEHOLDER}/data.json");
    let mut wanted = vec![placeheld.as_str(), token_path.as_str()];
    wanted.sort_unstable();
    assert_eq!(paths, wanted, "{taken:?}");
    let first = taken.requests.iter().find(|r| r.path == placeheld).unwrap();
    assert_eq!(first.method, "GET");
    assert_eq!(first.origin, base);
    assert_eq!(first.status, 200);
    assert_eq!(first.media_type, "application/json");
    assert_eq!(first.query, vec!["key".to_owned(), "view".to_owned()]);
    assert!(first.query_left_out);
    assert!(
        first
            .credential_headers
            .contains(&"authorization".to_owned()),
        "{first:?}"
    );
    assert!(
        first.credential_headers.contains(&"cookie".to_owned()),
        "{first:?}"
    );
    assert!(
        first.set_headers.contains(&"x-view".to_owned()),
        "{first:?}"
    );
    let fields = first.fields.as_ref().unwrap();
    assert_eq!(fields.get("id").map(String::as_str), Some("integer"));
    assert_eq!(fields.get("token").map(String::as_str), Some("string"));
    assert_eq!(fields.get("items").map(String::as_str), Some("array"));
    drop(capturing);

    // The `web_capture` discoverer reads the same page's requests through
    // the contract: its findings hold the structure and no planted secret.
    let look = Look {
        paths: vec!["/capture/page"],
        depth: Some(0),
        ..Look::default()
    };
    let seen = must(
        bench
            .run(crate::WEB_CAPTURE, &format!("{base}/"), &look)
            .await,
    );
    let whole = serde_json::to_string(&seen.findings).unwrap();
    for planted in [SECRET, WORD_SECRET] {
        assert!(!whole.contains(planted), "{whole}");
    }
    let (_, found) = seen.one("exchange", &placeheld);
    assert_eq!(found["method"], "GET");
    assert_eq!(found["made_by"], "/capture/page");
    assert_eq!(found["query_left_out"], true);
    let _ = seen.one("exchange", &token_path);

    let logged = logs(&bench);
    for planted in [SECRET, WORD_SECRET] {
        assert!(!logged.contains(planted), "a planted secret is in the logs");
    }
    bench.end().await;
    server.abort();
}
