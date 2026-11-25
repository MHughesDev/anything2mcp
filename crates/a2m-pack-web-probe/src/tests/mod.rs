//! Tests. The ones in `scouting` need a browser and the test application;
//! a test that needs a browser fails with `browser_unavailable` when none
//! is found, and never passes by skipping. On a machine whose browser is
//! not in a known place, the two variables of the base's `test_support`
//! say where it is.
//!
//! The stand is the test application with a few pages of this crate's own
//! served beside it, at the same origin: pages that try to write when
//! they load, from a worker, and over a socket, a link that looks like an
//! action, a sign-in wall, and pages that hold a planted secret. Whether
//! anything was written is asked of the test application's control
//! interface, never of what discovery read.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_core::{
    AccessKind, CompiledSurface, Digest, Environment, Failure, Finding, NoSecrets, PackPayload,
    Redact, SurfaceKind, SurfaceName, TargetName, Value, Values,
};
use a2m_logs::LogSettings;
use a2m_pack_web_browser::{PROFILE_ROOT, SharedBrowsers, test_support};
use a2m_provider::{
    Access, BoxFuture, CheckedPaths, Deadline, DiscovererHandle, DiscoveryReport, DiscoveryScope,
    FindingSink, HostServices, ProviderSettings, Registry, RegistryBuilder, RegistryProblem,
    RootName, Roots, SandboxPort, StepFailure, StepInput, StepOutcome, Supervisor,
    SupervisorLimits, SurfaceDescription, SurfaceRef,
};
use axum::Router;
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::get;
use serde_json::Value as Json;

use crate::{SURFACE_KIND, WebProbePack};

mod capture;
mod rules;
mod scouting;
mod sign_in;

/// A made-up secret, in a form no real one has.
const SECRET: &str = "secret-for-tests-7f3a9c";

/// A folder under the system's temporary folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-probe-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A redactor that knows one secret.
struct Knows(&'static str);

impl Redact for Knows {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, "[redacted]")
    }
}

/// The pack as the product makes it, with the one change a test machine
/// needs: the browser program the test environment names is added to the
/// places the manifest declares, and the settings are the test's.
fn test_pack(shared: SharedBrowsers) -> WebProbePack {
    let mut pack = crate::pack(shared);
    if let Some(program) = test_support::browser_program() {
        let with = pack.manifest.replacen(
            "paths = [",
            &format!("paths = [\n    '{}',", program.display()),
            1,
        );
        assert_ne!(with, pack.manifest);
        pack.manifest = Box::leak(with.into_boxed_str());
    }
    pack.settings = test_support::settings();
    pack
}

/// A registry that holds only this pack.
struct Bench {
    registry: Registry,
    problems: Vec<RegistryProblem>,
    supervisor: Supervisor,
    shared: SharedBrowsers,
    redact: Arc<dyn Redact>,
    _folder: Folder,
}

impl Bench {
    fn new(name: &str, redact: Arc<dyn Redact>) -> Self {
        let folder = Folder::new(name);
        let profiles = folder.0.join("profiles");
        let logs = folder.0.join("logs");
        std::fs::create_dir_all(&profiles).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        let mut roots = Roots::none();
        roots
            .add(
                RootName::new(PROFILE_ROOT).unwrap(),
                &profiles,
                Access::Write,
            )
            .unwrap();
        let supervisor = Supervisor::new(SupervisorLimits::default());
        let host = HostServices {
            supervisor: supervisor.clone(),
            roots,
            redact: Arc::clone(&redact),
            logs_and_traces: a2m_logs::start(&LogSettings::default(), Arc::clone(&redact), &logs)
                .unwrap(),
        };
        let shared = SharedBrowsers::default();
        let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
        builder.add_pack(&test_pack(shared.clone()));
        let (registry, problems) = builder.build();
        Self {
            registry,
            problems,
            supervisor,
            shared,
            redact,
            _folder: folder,
        }
    }

    fn description(address: &str) -> SurfaceDescription {
        SurfaceDescription {
            kind: SurfaceKind::new(SURFACE_KIND).unwrap(),
            access_kind: AccessKind::Interface,
            address: address.to_owned(),
            facts: Values::new(),
        }
    }

    fn discoverer(&self, name: &str, address: &str) -> DiscovererHandle {
        self.registry
            .discoverers_for(&Self::description(address))
            .into_iter()
            .find(|handle| handle.declared().name.as_str() == name)
            .unwrap()
    }

    /// Runs one discoverer on a surface at `address`.
    async fn run(&self, name: &str, address: &str, look: &Look) -> Result<Seen, Failure> {
        let discoverer = self.discoverer(name, address);
        let surface = SurfaceRef {
            target: TargetName::new("example").unwrap(),
            environment: Environment::new("test").unwrap(),
            surface: CompiledSurface {
                name: SurfaceName::new("app").unwrap(),
                kind: SurfaceKind::new(SURFACE_KIND).unwrap(),
                address: address.to_owned(),
                same_session_as: None,
                stands: None,
                rate: None,
                programs: Vec::new(),
                payload: PackPayload::none(),
            },
        };
        let mut focus = Values::new();
        if !look.paths.is_empty() {
            let paths: Vec<Value> = look.paths.iter().map(|path| Value::from(*path)).collect();
            focus.insert(crate::FOCUS_PATHS, paths);
        }
        if let Some(text) = look.ready_text {
            focus.insert(crate::FOCUS_READY_TEXT, text);
        }
        if look.tier_two {
            focus.insert(crate::FOCUS_TIER_TWO, true);
        }
        let mut bounds = discoverer.declared().bounds;
        if let Some(depth) = look.depth {
            bounds.depth = depth;
        }
        let scope = DiscoveryScope {
            surface: &surface,
            bounds,
            deadline: Deadline::after(look.within),
            paths: &CheckedPaths::none(),
            redact: &*self.redact,
            focus: &focus,
        };
        let mut kept = Kept(Vec::new());
        let report = discoverer.discover(&NoPort, &scope, &mut kept).await?;
        Ok(Seen {
            findings: kept.0,
            report,
        })
    }

    /// Closes every browser and checks that the supervisor holds none.
    async fn end(self) {
        if let Some(all) = self.shared.get() {
            let cleanup = all.shutdown().await;
            assert!(cleanup.complete, "{cleanup:?}");
        }
        assert!(self.supervisor.list().is_empty());
    }
}

/// What one discovery is asked to look at.
struct Look {
    paths: Vec<&'static str>,
    ready_text: Option<&'static str>,
    tier_two: bool,
    depth: Option<u32>,
    within: Duration,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            ready_text: None,
            tier_two: false,
            depth: None,
            within: Duration::from_secs(90),
        }
    }
}

/// What one discovery gave.
struct Seen {
    findings: Vec<Finding>,
    report: DiscoveryReport,
}

impl Seen {
    fn ids(&self) -> BTreeSet<Digest> {
        self.findings.iter().map(Finding::id).collect()
    }

    /// The findings of a kind at a path, with what was found, parsed.
    fn at(&self, kind: &str, path: &str) -> Vec<(&Finding, Json)> {
        self.findings
            .iter()
            .filter(|finding| {
                finding.kind().as_str() == kind
                    && matches!(finding.subject(), a2m_core::Subject::Address(address) if address.path() == path)
            })
            .map(|finding| (finding, serde_json::from_str(finding.found()).unwrap()))
            .collect()
    }

    /// The one finding of a kind at a path.
    #[track_caller]
    fn one(&self, kind: &str, path: &str) -> (&Finding, Json) {
        let mut all = self.at(kind, path);
        assert_eq!(all.len(), 1, "`{kind}` at {path}: {:#?}", self.paths());
        all.remove(0)
    }

    fn paths(&self) -> Vec<String> {
        self.findings
            .iter()
            .map(|finding| match finding.subject() {
                a2m_core::Subject::Address(address) => {
                    format!("{} {}", finding.kind().as_str(), address.path())
                }
                _ => finding.kind().as_str().to_owned(),
            })
            .collect()
    }
}

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// The port a discoverer of this crate is handed and never uses: it has
/// no driver to send a step to.
struct NoPort;

impl SandboxPort for NoPort {
    fn act<'a>(
        &'a self,
        _step: &'a a2m_core::Step,
        _input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async { panic!("a discoverer of this crate sends no step") })
    }
}

// ---- the stand ----

/// What the stand's own pages need: where the application is, to write to
/// it when one of the routes that must never be reached is reached.
#[derive(Clone)]
struct Own {
    base: Arc<String>,
    http: reqwest::Client,
}

impl Own {
    /// A real write to the test application, as a route that acts would
    /// make.
    async fn write(&self, item: &str) -> u16 {
        let stored: Json = self
            .http
            .get(format!("{}/__control/orders/A-100/items/{item}", self.base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let body = serde_json::json!({
            "description": "Written by a route that acts",
            "quantity": 1,
            "length_in": "1",
            "version": stored["stored"]["version"],
        });
        self.http
            .put(format!("{}/api/orders/A-100/items/{item}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{title}</title>\n</head>\n<body>\n<main>\n<h1>{title}</h1>\n{body}\n</main>\n</body>\n</html>\n"
    ))
}

/// The script of a page that says when what it tried has settled.
const SETTLED: &str = "const settled = (how) => { const line = document.createElement('p'); \
     line.textContent = 'settled ' + how; document.querySelector('main').append(line); };";

/// A write, as a page's own script makes it.
fn put(item: &str) -> String {
    format!(
        "fetch('/api/orders/A-100/items/{item}', {{method: 'PUT', headers: {{'Content-Type': 'application/json'}}, \
         body: JSON.stringify({{description: 'Written by the page', quantity: 1, length_in: '1', version: 1}})}})"
    )
}

fn own_pages(own: Own) -> Router {
    Router::new()
        .route(
            "/probe/posts",
            get(|| async {
                page(
                    "Posts on load",
                    &format!(
                        "<script>{SETTLED} {}.then((r) => settled('sent ' + r.status), () => settled('failed'));</script>",
                        put("L-101")
                    ),
                )
            }),
        )
        .route(
            "/probe/worker",
            get(|| async {
                page(
                    "Writes from a worker",
                    &format!(
                        "<script>{SETTLED} const worker = new Worker('/probe/worker.js'); \
                         worker.onmessage = (event) => settled(event.data); \
                         worker.onerror = () => settled('worker error');</script>"
                    ),
                )
            }),
        )
        .route(
            "/probe/worker.js",
            get(|| async {
                (
                    [(CONTENT_TYPE, "text/javascript")],
                    format!(
                        "{}.then((r) => postMessage('sent ' + r.status), () => postMessage('failed'));",
                        put("L-102")
                    ),
                )
            }),
        )
        .route(
            "/probe/socket",
            get(|| async {
                page(
                    "Opens a socket",
                    &format!(
                        "<script>{SETTLED} try {{ new WebSocket('ws://' + location.host + '/probe/ws'); settled('made'); }} \
                         catch (error) {{ settled('refused'); }}</script>"
                    ),
                )
            }),
        )
        // The handshake of a socket is a request to this route. Reaching it
        // writes.
        .route(
            "/probe/ws",
            get(|State(own): State<Own>| async move { own.write("L-103").await.to_string() }),
        )
        .route(
            "/probe/links",
            get(|| async {
                page(
                    "Links",
                    "<p><a href=\"/orders/A-102\">Open order</a></p>\n\
                     <p><a href=\"/probe/items/L-101/remove\">Take line off</a></p>\n\
                     <p><a href=\"/probe/plain\">Sign out</a></p>\n\
                     <p><a href=\"https://elsewhere.example.com/\">Elsewhere</a></p>",
                )
            }),
        )
        // A link that acts when it is followed.
        .route(
            "/probe/items/L-101/remove",
            get(|State(own): State<Own>| async move {
                page("Removed", &format!("<p>{}</p>", own.write("L-101").await))
            }),
        )
        .route("/probe/plain", get(|| async { page("Plain", "<p>Nothing here.</p>") }))
        .route("/probe/private", get(|| async { Redirect::to("/probe/wall") }))
        .route(
            "/probe/wall",
            get(|| async {
                page(
                    "Sign in",
                    "<form><p><label for=\"user\">Name</label> <input id=\"user\" name=\"user\"></p>\n\
                     <p><label for=\"word\">Password</label> <input id=\"word\" name=\"word\" type=\"password\"></p>\n\
                     <p><button type=\"submit\">Sign in</button></p></form>",
                )
            }),
        )
        .route(
            "/probe/secret",
            get(|| async {
                page(
                    &format!("Report {SECRET}"),
                    &format!(
                        "<p><a href=\"/orders\">{SECRET}</a></p>\n\
                         <form><p><label for=\"key\">Key</label> <input id=\"key\" name=\"key\" value=\"{SECRET}\"></p></form>\n\
                         <table><caption>Keys</caption><thead><tr><th>Key</th></tr></thead>\
                         <tbody><tr><td>{SECRET}</td></tr></tbody></table>"
                    ),
                )
            }),
        )
        .route(
            "/probe/secret.json",
            get(|| async {
                (
                    [(CONTENT_TYPE, "application/json")],
                    format!(
                        "{{\"token\":\"{SECRET}\",\"by_key\":{{\"{SECRET}\":1}},\"items\":[{{\"id\":7,\"note\":\"{SECRET}\"}},{{\"id\":8}}]}}"
                    ),
                )
                    .into_response()
            }),
        )
        .with_state(own)
}

/// The test application, with this crate's pages beside it, on a free
/// port, stopped when dropped.
struct Site {
    own: Own,
    server: tokio::task::JoinHandle<()>,
}

impl Site {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let own = Own {
            base: Arc::new(format!("http://{}", listener.local_addr().unwrap())),
            http: example_app::harness::client_builder().build().unwrap(),
        };
        let router = example_app::router().merge(own_pages(own.clone()));
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Self { own, server }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.own.base)
    }

    async fn control(&self, path: &str) -> Json {
        let answer = self.own.http.get(self.url(path)).send().await.unwrap();
        assert!(answer.status().is_success(), "{path}: {}", answer.status());
        answer.json().await.unwrap()
    }

    /// Every write the application recorded since the last reset.
    async fn writes(&self) -> Vec<Json> {
        self.control("/__control/writes").await["writes"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// Exactly what the application has stored for an order.
    async fn stored_order(&self, order: &str) -> Json {
        self.control(&format!("/__control/orders/{order}")).await["stored"].clone()
    }

    async fn reset(&self) {
        let answer = self
            .own
            .http
            .post(self.url("/__control/reset"))
            .send()
            .await
            .unwrap();
        assert!(answer.status().is_success());
    }

    async fn fault(&self, fault: &Json) {
        let answer = self
            .own
            .http
            .post(self.url("/__control/faults"))
            .json(fault)
            .send()
            .await
            .unwrap();
        assert!(answer.status().is_success(), "{}", answer.status());
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// Unwraps, naming the failure's class and code.
#[track_caller]
fn must<T>(result: Result<T, Failure>) -> T {
    match result {
        Ok(value) => value,
        Err(failure) => panic!("{}/{}: {failure}", failure.class(), failure.code()),
    }
}

fn no_secrets() -> Arc<dyn Redact> {
    Arc::new(NoSecrets)
}
