//! The REST interface over HTTP, against a real kernel with memory key
//! stores, the engine's test pack, and the built-in test application: a
//! read end to end, a refusal and its parts, input that is not well formed
//! stopped at the door, a planted secret, no way to approve, and the
//! OpenAPI document against the routes the router serves.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests; the helpers they share do not carry the test attribute"
)]

#[path = "../../a2m-engine/tests/common/mod.rs"]
mod engine;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Caller, CallerCredential, CloseReason, FrontDoor,
    HealthState, Kernel, KernelParts, Paths, PersonSurface, PresenceAsk, PresenceProof, SessionAsk,
    Settings, SignInAsk, StorePlace, SurfaceLink,
};
use a2m_api::{ApiSettings, OPENAPI_PATH, openapi, router, served_routes};
use a2m_auth::PersonAnswer;
use a2m_core::{Failure, FailureClass, Timestamp};
use a2m_provider::{RegistryBuilder, Roots, Supervisor, SupervisorLimits};
use a2m_telemetry::{Clock, ManualClock};
use a2m_vault::MemoryKeyStore;
use engine::{AppPack, Counted, Folder, ITEM_UPSERT, ORDER_GET, START};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

const USER: &str = "pat";

/// A person surface that keeps the desk and answers no prompt.
struct Surface {
    desk: Mutex<Option<ApprovalDesk>>,
}

impl PersonSurface for Surface {
    fn take_desk(&self, desk: ApprovalDesk) {
        *self.desk.lock().unwrap() = Some(desk);
    }

    fn ask_approval(&self, ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            Ok(SurfaceLink {
                url: None,
                text: format!("Approve run {} at the machine.", ask.run),
            })
        })
    }

    fn ask_sign_in(&self, _ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            Ok(SurfaceLink {
                url: None,
                text: "Sign in at the machine.".to_owned(),
            })
        })
    }

    fn confirm_presence(&self, _ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            Err(Failure::new(
                FailureClass::Policy,
                a2m_policy_presence(),
                "Nobody answered the presence prompt.",
                "Answer the prompt at the machine.",
            ))
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        Box::pin(async move { PersonAnswer::NoAnswer })
    }
}

fn a2m_policy_presence() -> a2m_core::FailureCode {
    a2m_core::FailureCode::fixed("presence_required")
}

struct World {
    app: TestApp,
    kernel: Kernel,
    base: String,
    http: reqwest::Client,
    server: tokio::task::JoinHandle<()>,
    _folder: Folder,
}

impl World {
    /// A healthy kernel with the `example` target published, its router
    /// served on a port of the loopback address with a verified caller on
    /// every request, as the local channel would put it there.
    async fn ready() -> Self {
        let app = TestApp::start().await.unwrap();
        let folder = Folder::new("api");
        let clock: Arc<dyn Clock> =
            Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
        let prepared = Kernel::prepare(KernelParts {
            settings: Settings::defaults(),
            paths: Paths::under(folder.path()),
            place: StorePlace::Memory,
            key_store: Arc::new(MemoryKeyStore::with_random(Counted::new())),
            clock,
            random: Counted::new(),
            os_user: USER.to_owned(),
        })
        .await;
        let host =
            prepared.host_services(Supervisor::new(SupervisorLimits::default()), Roots::none());
        let mut builder = RegistryBuilder::new(prepared.settings().provider_settings(), host);
        builder.add_pack(&AppPack {
            hooks: Arc::new(engine::Hooks::default()),
            discoverer: Arc::new(engine::WritingDiscoverer {
                results: Mutex::new(Vec::new()),
            }),
        });
        let (registry, problems) = builder.build();
        assert!(problems.is_empty(), "{problems:?}");
        let surface = Arc::new(Surface {
            desk: Mutex::new(None),
        });
        let kernel = Kernel::open(prepared, registry, problems, surface).await;
        assert_eq!(kernel.health().state, HealthState::Ok);
        kernel.start_runner_loop(2).await.unwrap();
        kernel
            .publish_plan(&engine::plan(&app.url("")))
            .await
            .unwrap();

        let principal = kernel
            .verify_caller(CallerCredential::LocalUser(USER.to_owned()))
            .await
            .unwrap();
        // The door is set by the router whatever the mount said.
        let caller = Caller::new(principal, FrontDoor::Mcp, true);
        let api = router(kernel.clone(), ApiSettings::default()).unwrap();
        let open = router(kernel.clone(), ApiSettings::default()).unwrap();
        let served = axum::Router::new()
            .nest_service("/bare", open)
            .merge(api.layer(axum::Extension(caller)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, served).await.unwrap();
        });
        Self {
            app,
            kernel,
            base,
            http: reqwest::Client::new(),
            server,
            _folder: folder,
        }
    }

    async fn post(&self, path: &str, body: &Json) -> (u16, Json) {
        let answer = self
            .http
            .post(format!("{}{path}", self.base))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .unwrap();
        Self::read(answer).await
    }

    async fn get(&self, path: &str) -> (u16, Json) {
        let answer = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        Self::read(answer).await
    }

    async fn raw(&self, request: reqwest::RequestBuilder) -> (u16, Json) {
        Self::read(request.send().await.unwrap()).await
    }

    async fn read(answer: reqwest::Response) -> (u16, Json) {
        let status = answer.status().as_u16();
        assert_eq!(
            answer.headers()["content-type"],
            "application/json",
            "every answer is JSON"
        );
        (status, answer.json().await.unwrap())
    }

    async fn close(self) {
        self.server.abort();
        self.kernel.close(CloseReason::Asked).await;
    }
}

/// A failure's body: exactly the four parts and whether a retry is safe,
/// and no result.
fn assert_refusal(body: &Json, class: &str, code: &str) {
    assert_eq!(body["ok"], false, "{body}");
    assert!(body.get("result").is_none(), "{body}");
    let error = body["error"].as_object().unwrap();
    let keys: BTreeSet<&str> = error.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        BTreeSet::from(["class", "code", "message", "fix", "retry_safe"])
    );
    assert_eq!(error["class"], class, "{body}");
    assert_eq!(error["code"], code, "{body}");
    assert!(!error["message"].as_str().unwrap().is_empty());
    assert!(!error["fix"].as_str().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_goes_end_to_end_through_the_action_route_and_the_aliases() {
    let world = World::ready().await;

    let (status, body) = world
        .post(
            "/api/v1/actions/tool.call",
            &json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["action"], "tool.call");
    // The value is what the test application holds.
    assert_eq!(body["result"]["result"]["order_id"], "A-100");
    assert_eq!(body["result"]["result"]["customer"], "Example Customer One");
    assert!(body["decision"].is_string());
    let run = body["result"]["run_id"].as_str().unwrap().to_owned();

    // The alias of run.get, its field from the path.
    let (status, again) = world.get(&format!("/api/v1/runs/{run}")).await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["action"], "run.get");
    assert_eq!(again["result"]["status"], "completed");
    assert_eq!(again["result"]["result"], body["result"]["result"]);

    // The alias of tool.search with no words, and of session.status.
    let (status, tools) = world.get("/api/v1/tools").await;
    assert_eq!(status, 200, "{tools}");
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == ORDER_GET)
    );
    let (status, sessions) = world.get("/api/v1/sessions").await;
    assert_eq!(status, 200, "{sessions}");
    assert_eq!(sessions["action"], "session.status");

    // A field in the body that contradicts the path is refused.
    let (status, clash) = world
        .post(
            &format!("/api/v1/runs/{run}/cancel"),
            &json!({"run_id": "run_other"}),
        )
        .await;
    assert_eq!(status, 400);
    assert_refusal(&clash, "input", "path_field_conflict");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refusal_carries_its_class_code_message_and_fix_and_nothing_else() {
    let world = World::ready().await;

    // The dispatcher's refusal: a tool nobody published.
    let (status, body) = world
        .post(
            "/api/v1/actions/tool.call",
            &json!({"tool": "example_nothing_get", "input": {}}),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_refusal(&body, "input", "not_found");
    assert!(body["decision"].is_string(), "the dispatcher answered");

    // No caller on the request: 401, and the dispatcher is not reached.
    let (status, body) = world
        .raw(
            world
                .http
                .post(format!("{}/bare/api/v1/actions/tool.search", world.base))
                .header("content-type", "application/json")
                .body("{}"),
        )
        .await;
    assert_eq!(status, 401);
    assert_refusal(&body, "policy", "caller_missing");
    assert!(body.get("decision").is_none());

    // No such route, and a route that does not take the method.
    let (status, body) = world.get("/api/v1/actions/tool.call.again").await;
    assert_eq!(status, 404);
    assert_refusal(&body, "input", "route_not_found");
    let (status, body) = world.get("/api/v1/actions/tool.call").await;
    assert_eq!(status, 405);
    assert_refusal(&body, "input", "method_not_allowed");
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn input_that_is_not_well_formed_is_refused_at_the_door() {
    let world = World::ready().await;
    let url = format!("{}/api/v1/actions/tool.search", world.base);
    let cases: Vec<(reqwest::RequestBuilder, u16, &str)> = vec![
        (
            world
                .http
                .post(&url)
                .header("content-type", "application/json")
                .body("{\"query\": "),
            400,
            "schema_invalid",
        ),
        (
            world
                .http
                .post(&url)
                .header("content-type", "application/json")
                .body("[1, 2]"),
            400,
            "schema_invalid",
        ),
        (
            world
                .http
                .post(&url)
                .header("content-type", "text/plain")
                .body("{}"),
            415,
            "content_type_unsupported",
        ),
        (
            world
                .http
                .post(&url)
                .header("content-type", "application/json")
                .body(format!(
                    "{{\"query\": \"{}\"}}",
                    "a".repeat(2 * 1024 * 1024)
                )),
            413,
            "body_too_large",
        ),
        (
            world
                .http
                .post(format!("{url}?query=order"))
                .header("content-type", "application/json")
                .body("{}"),
            400,
            "query_invalid",
        ),
        (
            world
                .http
                .post(&url)
                .header("content-type", "application/json")
                .header("idempotency-key", "two words")
                .body("{}"),
            400,
            "idempotency_key_invalid",
        ),
        (
            world
                .http
                .get(format!("{}/api/v1/tools?query=a&query=b", world.base)),
            400,
            "query_invalid",
        ),
    ];
    for (request, want_status, want_code) in cases {
        let (status, body) = world.raw(request).await;
        assert_eq!(status, want_status, "{body}");
        assert_refusal(&body, "input", want_code);
        // The dispatcher records a decision for every request it is
        // handed; none was made.
        assert!(body.get("decision").is_none(), "{body}");
    }
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_planted_secret_appears_in_nothing_the_door_sends() {
    let world = World::ready().await;
    let secret = "Example Customer One";
    let (status, stored) = world
        .post(
            "/api/v1/actions/secret.set",
            &json!({"name": "example_password", "value": secret}),
        )
        .await;
    assert_eq!(status, 200, "{stored}");
    assert!(!stored.to_string().contains(secret));

    let (status, read) = world
        .post(
            "/api/v1/actions/tool.call",
            &json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert_eq!(status, 200, "{read}");
    let run = read["result"]["run_id"].as_str().unwrap().to_owned();
    let mut sent = vec![read];
    sent.push(world.get(&format!("/api/v1/runs/{run}")).await.1);
    // A body that is not JSON and quotes the secret; a query that does.
    sent.push(
        world
            .raw(
                world
                    .http
                    .post(format!("{}/api/v1/actions/docs.get", world.base))
                    .header("content-type", "application/json")
                    .body(format!("{{\"topic\": \"{secret}\" oops")),
            )
            .await
            .1,
    );
    sent.push(
        world
            .post("/api/v1/actions/docs.get", &json!({"topic": secret}))
            .await
            .1,
    );
    for body in sent {
        let text = body.to_string();
        assert!(!text.contains(secret), "{text}");
    }
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_door_offers_no_way_to_approve() {
    let world = World::ready().await;
    // No route of everything the router serves approves, decides, or
    // answers for a person.
    for route in served_routes(world.kernel.actions()) {
        let words = format!("{} {}", route.path, route.action).to_lowercase();
        for word in ["approv", "decide", "answer", "desk"] {
            assert!(!words.contains(word), "{words}");
        }
    }
    // A write rests at 202 until a person approves; resuming it through
    // the door, with or without a key, does not write.
    let input = json!({
        "order_id": "A-102",
        "item_code": "L-300",
        "description": "New rail",
        "quantity": 3,
        "length_in": "150.5",
    });
    let (status, rested) = world
        .post(
            "/api/v1/actions/tool.call",
            &json!({"tool": ITEM_UPSERT, "input": input}),
        )
        .await;
    assert_eq!(status, 202, "{rested}");
    assert_eq!(rested["result"]["status"], "needs_approval");
    let run = rested["result"]["run_id"].as_str().unwrap().to_owned();
    let (status, still) = world
        .post(&format!("/api/v1/runs/{run}/resume"), &json!({}))
        .await;
    assert_eq!(status, 202, "{still}");
    assert_eq!(still["result"]["status"], "needs_approval");
    // The test application confirms nothing was written.
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 0);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_openapi_document_describes_every_route_and_nothing_else() {
    let world = World::ready().await;
    let (status, doc) = world.get(OPENAPI_PATH).await;
    assert_eq!(status, 200);
    assert_eq!(doc, openapi(world.kernel.actions()));
    assert_eq!(doc["openapi"], "3.1.0");

    let mut described = BTreeSet::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            described.insert((method.to_uppercase(), path.clone()));
            let ids = doc["paths"]
                .as_object()
                .unwrap()
                .values()
                .flat_map(|item| item.as_object().unwrap().values())
                .filter(|other| other["operationId"] == op["operationId"])
                .count();
            assert_eq!(ids, 1, "operation ids are unique");
        }
    }
    let served: BTreeSet<(String, String)> = served_routes(world.kernel.actions())
        .into_iter()
        .map(|route| (route.method.to_owned(), route.path))
        .collect();
    assert_eq!(described, served);

    // Every operation the document lists is called, with a field its
    // action does not have: the router reaches that action, whose
    // dispatcher refuses the input before anything runs.
    for (method, path) in &described {
        if path == OPENAPI_PATH {
            continue;
        }
        let op = &doc["paths"][path][method.to_lowercase()];
        let concrete = path.replace("{run_id}", "run_absent");
        let (status, body) = if method == "GET" {
            world.get(&format!("{concrete}?zz_not_a_field=1")).await
        } else {
            world.post(&concrete, &json!({"zz_not_a_field": 1})).await
        };
        assert_eq!(status, 400, "{method} {path}: {body}");
        assert_eq!(body["action"], op["x-action"], "{method} {path}");
        assert_eq!(body["error"]["class"], "input", "{method} {path}: {body}");
    }
    world.close().await;
}
