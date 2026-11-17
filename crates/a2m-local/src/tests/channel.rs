//! The channel: two clients and one engine, one engine per data directory,
//! the check of the user at both ends, the hello, the idle stop, and a
//! read through the channel to the real kernel.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use a2m_actions::{
    BoxFuture, Caller, CloseReason, FrontDoor, Kernel, Paths, Request as ActionRequest,
};
use a2m_core::{Failure, FailureClass};
use axum::body::{Body, Bytes};
use axum::extract::Path;
use axum::http::{Method, Request, StatusCode};
use axum::routing::{get, post};
use axum::{Extension, Router};
use serde_json::{Value as Json, json};
use tokio::task::JoinHandle;

use super::world::engine::ORDER_GET;
use super::world::{TestPrompt, VERSION, World, home, within};
use crate::channel::refusal;
use crate::{
    CLIENT_LABEL_HEADER, CLIENT_VERSION_HEADER, Channel, ChannelControl, ClientLabel, Connector,
    EngineLock, HELLO_PATH, IdleHolds, IdlePolicy, LOCAL_API_HEADER, Link, OsUser, Peer,
    PromptAnswer, StopReason, codes,
};

/// A stand-in for the composed service: it says who called, and counts.
fn who(hits: Arc<AtomicU32>, instance: u32) -> Router {
    Router::new().route(
        "/who",
        get(move |Extension(peer): Extension<Peer>| {
            let hits = Arc::clone(&hits);
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                json!({
                    "instance": instance,
                    "user": peer.user.to_string(),
                    "label": peer.label.as_text(),
                })
                .to_string()
            }
        }),
    )
}

struct Engine {
    control: ChannelControl,
    hits: Arc<AtomicU32>,
    serving: JoinHandle<(StopReason, EngineLock)>,
}

impl Engine {
    fn start(channel: Channel, instance: u32, idle: IdlePolicy) -> Self {
        let hits = Arc::new(AtomicU32::new(0));
        let control = channel.control();
        let serving = tokio::spawn(channel.serve(who(Arc::clone(&hits), instance), idle));
        Self {
            control,
            hits,
            serving,
        }
    }

    async fn stop(self) -> StopReason {
        self.control.stop(StopReason::Asked);
        // The lock that comes back is dropped here, which releases it.
        within("the engine's stop", self.serving).await.unwrap().0
    }
}

fn get_request(path: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

/// Connects, or fails by name when the connect does not end.
async fn connect(connector: &Connector) -> Result<Link, Failure> {
    within("the client's connect", connector.connect()).await
}

async fn json_of(link: &Link, request: Request<Body>) -> (StatusCode, Json) {
    let response = within("the request", link.send(request)).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

fn assert_failure(failure: &Failure, class: FailureClass, code: &a2m_core::FailureCode) {
    assert_eq!(failure.class(), class, "{failure:?}");
    assert_eq!(failure.code(), code, "{failure:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_clients_share_one_engine_and_their_requests_reach_the_service() {
    let (_folder, paths) = home("two");
    let engine = Engine::start(
        Channel::bind(&paths, VERSION).await.unwrap(),
        7,
        IdlePolicy::never(),
    );
    let command = Connector::new(&paths, VERSION, ClientLabel::CommandLine).unwrap();
    let agent = Connector::new(&paths, VERSION, ClientLabel::mcp("example client")).unwrap();
    // Both at the same moment.
    let (first, second) = tokio::join!(connect(&command), connect(&agent));
    let (first, second) = (first.unwrap(), second.unwrap());
    assert_eq!(first.engine_version(), VERSION);

    let me = OsUser::current().unwrap().to_string();
    let (status, said) = json_of(&first, get_request("/who")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(said, json!({"instance": 7, "user": me, "label": "cli"}));
    let (_, said) = json_of(&second, get_request("/who")).await;
    assert_eq!(
        said,
        json!({"instance": 7, "user": me, "label": "mcp:example client"})
    );
    // A link carries one request after another.
    let (_, again) = json_of(&first, get_request("/who")).await;
    assert_eq!(again["instance"], 7);
    assert_eq!(engine.hits.load(Ordering::SeqCst), 3);
    assert_eq!(engine.control.refused(), 0);

    // While it serves, the engine lock is held; a stopped engine leaves
    // nothing that answers and no lock.
    assert!(EngineLock::held(&paths).unwrap());
    drop((first, second));
    assert_eq!(engine.stop().await, StopReason::Asked);
    let gone = connect(&command).await.unwrap_err();
    assert_failure(&gone, FailureClass::Busy, &codes::ENGINE_UNREACHABLE);
    assert!(!EngineLock::held(&paths).unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_engine_on_the_same_data_directory_is_refused() {
    let (folder, paths) = home("lock");
    let first = Channel::bind(&paths, VERSION).await.unwrap();

    let second = Channel::bind(&paths, VERSION).await.unwrap_err();
    assert_failure(&second, FailureClass::Busy, &codes::ENGINE_RUNNING);
    // Another runtime directory that leads to the same data directory is
    // the same engine's ground.
    let other_runtime = Paths {
        runtime: folder.path().join("run2"),
        ..paths.clone()
    };
    let third = Channel::bind(&other_runtime, VERSION).await.unwrap_err();
    assert_failure(&third, FailureClass::Busy, &codes::ENGINE_RUNNING);
    assert!(EngineLock::held(&paths).unwrap());

    // The lock goes with its holder, and the next engine binds.
    drop(first);
    assert!(!EngineLock::held(&paths).unwrap());
    let next = Engine::start(
        Channel::bind(&paths, VERSION).await.unwrap(),
        1,
        IdlePolicy::never(),
    );
    assert_eq!(next.stop().await, StopReason::Asked);
}

/// A socket file that a crashed engine left is not an engine: a client
/// finds nothing running, and the next engine binds over it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_socket_is_not_an_engine_and_does_not_block_the_next() {
    let (_folder, paths) = home("stale");
    // An engine binds, and is gone without removing its socket.
    let me = OsUser::current().unwrap();
    let place = crate::channel::place(&paths, &me);
    drop(Channel::bind(&paths, VERSION).await.unwrap());
    drop(std::os::unix::net::UnixListener::bind(&place.socket).unwrap());
    assert!(place.socket.exists());

    let connector = Connector::new(&paths, VERSION, ClientLabel::CommandLine).unwrap();
    let nothing = connect(&connector).await.unwrap_err();
    assert_failure(&nothing, FailureClass::Busy, &codes::ENGINE_UNREACHABLE);

    let engine = Engine::start(
        Channel::bind(&paths, VERSION).await.unwrap(),
        2,
        IdlePolicy::never(),
    );
    let link = connect(&connector).await.unwrap();
    let (_, said) = json_of(&link, get_request("/who")).await;
    assert_eq!(said["instance"], 2);
    drop(link);
    engine.stop().await;
    assert!(!place.socket.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_users_connection_is_closed_unread_and_another_users_engine_is_not_spoken_to() {
    // An engine that expects another user sees this user's connection as
    // a stranger's: it is closed before a byte is read.
    let (_folder, paths) = home("peer");
    let stranger = OsUser::named("someone-else");
    let engine = Engine::start(
        Channel::bind_expecting(&paths, VERSION, stranger.clone())
            .await
            .unwrap(),
        1,
        IdlePolicy::never(),
    );
    let connector = Connector::new(&paths, VERSION, ClientLabel::CommandLine).unwrap();
    let refused = connect(&connector).await.unwrap_err();
    assert_failure(&refused, FailureClass::Transient, &codes::CONNECTION_LOST);
    assert_eq!(engine.control.refused(), 1);
    assert_eq!(engine.hits.load(Ordering::SeqCst), 0);
    engine.stop().await;

    // A client that expects another user's engine refuses this one, and
    // sends it nothing.
    let engine = Engine::start(
        Channel::bind(&paths, VERSION).await.unwrap(),
        1,
        IdlePolicy::never(),
    );
    let wary = connector.clone().expecting(stranger);
    let refused = connect(&wary).await.unwrap_err();
    assert_failure(&refused, FailureClass::Policy, &codes::PEER_NOT_ALLOWED);
    assert_eq!(engine.hits.load(Ordering::SeqCst), 0);
    assert_eq!(engine.control.refused(), 0);
    engine.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_is_not_a_well_formed_hello_is_refused_by_name_and_reaches_no_service() {
    let (_folder, paths) = home("hello");
    let engine = Engine::start(
        Channel::bind(&paths, VERSION).await.unwrap(),
        1,
        IdlePolicy::never(),
    );
    let connector = Connector::new(&paths, VERSION, ClientLabel::CommandLine).unwrap();
    let link = connector.open().await.unwrap();

    // A request before the hello.
    let (status, said) = json_of(&link, get_request("/who")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(said["class"], "input");
    assert_eq!(said["code"], "hello_invalid");
    // The refusal is its four parts and nothing else.
    let mut parts: Vec<&str> = said
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    parts.sort_unstable();
    assert_eq!(parts, ["class", "code", "fix", "message"]);

    let hello = |version: &str, api: &str, label: &str| {
        Request::builder()
            .method(Method::GET)
            .uri(HELLO_PATH)
            .header(CLIENT_VERSION_HEADER, version)
            .header(LOCAL_API_HEADER, api)
            .header(CLIENT_LABEL_HEADER, label)
            .body(Body::empty())
            .unwrap()
    };
    // A hello with a label that is not a label, and one with a version
    // that is not a version.
    for bad in [
        hello("0.0.1", "1", "mcp:<script>"),
        hello("newest", "1", "cli"),
    ] {
        let (status, said) = json_of(&link, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(said["code"], "hello_invalid");
    }
    // A client that speaks another local API is told so.
    let (status, said) = json_of(&link, hello("9.0.0", "999", "cli")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(said["code"], "local_api_mismatch");
    assert_eq!(said["class"], "input");
    // None of it reached the service, and the connection is still unnamed.
    let (status, _) = json_of(&link, get_request("/who")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(engine.hits.load(Ordering::SeqCst), 0);

    // After a well-formed hello the same connection is served.
    let (status, said) = json_of(&link, hello("0.0.1", "1", "sdk:python")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(said["engine_version"], VERSION.to_string());
    let (status, said) = json_of(&link, get_request("/who")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(said["label"], "sdk:python");
    assert_eq!(engine.hits.load(Ordering::SeqCst), 1);
    drop(link);
    engine.stop().await;
}

/// Says it is busy the first times it is asked.
struct BusyAtFirst {
    asked: AtomicU32,
    busy_for: u32,
}

impl IdleHolds for BusyAtFirst {
    fn busy(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move { self.asked.fetch_add(1, Ordering::SeqCst) < self.busy_for })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_engine_with_no_connection_and_nothing_held_stops_and_a_hold_keeps_it() {
    let (_folder, paths) = home("idle");
    let holds = Arc::new(BusyAtFirst {
        asked: AtomicU32::new(0),
        busy_for: 2,
    });
    let channel = Channel::bind(&paths, VERSION).await.unwrap();
    let idle = IdlePolicy::after(Duration::from_millis(20), Arc::clone(&holds) as _);
    let engine = Engine::start(channel, 1, idle);
    // Nobody asks it to stop: it stops by itself, and only once the hold
    // let go.
    let (stopped, lock) = within("the idle stop", engine.serving).await.unwrap();
    assert_eq!(stopped, StopReason::Idle);
    assert_eq!(holds.asked.load(Ordering::SeqCst), 3);
    // The channel no longer serves, and the engine lock is still held: it
    // came back to the caller, who closes the kernel before letting go.
    // Until then no second engine can bind.
    assert!(EngineLock::held(&paths).unwrap());
    let second = Channel::bind(&paths, VERSION).await.unwrap_err();
    assert_eq!(*second.code(), codes::ENGINE_RUNNING);
    drop(lock);
    assert!(!EngineLock::held(&paths).unwrap());
}

/// A stand-in for the REST face, which the product crate composes: the
/// peer's credential goes to the kernel's one verifier, and the action is
/// dispatched.
fn door(kernel: Kernel) -> Router {
    Router::new().route(
        "/api/v1/actions/{name}",
        post(
            move |Extension(peer): Extension<Peer>, Path(name): Path<String>, body: Bytes| {
                let kernel = kernel.clone();
                async move {
                    let principal = match kernel.verify_caller(peer.credential()).await {
                        Ok(principal) => principal,
                        Err(failure) => return refusal(StatusCode::FORBIDDEN, &failure),
                    };
                    let input: Json = serde_json::from_slice(&body).unwrap_or(Json::Null);
                    let caller = Caller::new(principal, FrontDoor::Rest, true);
                    let response = kernel
                        .dispatch(ActionRequest::new(&name, input, caller))
                        .await;
                    axum::response::IntoResponse::into_response(response.to_json().to_string())
                }
            },
        ),
    )
}

fn action(name: &str, input: &Json) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/actions/{name}"))
        .body(Body::from(input.to_string()))
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_goes_through_the_channel_to_the_kernel_and_a_secret_does_not_come_back() {
    let world = World::ready(
        TestPrompt::answering(PromptAnswer::Dismissed),
        Duration::from_secs(900),
    )
    .await;
    let channel = Channel::bind(&world.paths, VERSION).await.unwrap();
    let control = channel.control();
    let serving = tokio::spawn(channel.serve(door(world.kernel.clone()), IdlePolicy::never()));
    let connector = Connector::new(&world.paths, VERSION, ClientLabel::CommandLine).unwrap();
    let link = connect(&connector).await.unwrap();

    // The value the test application holds is released as a secret first.
    let secret = "Example Customer One";
    let (_, stored) = json_of(
        &link,
        action(
            "secret.set",
            &json!({"name": "example_password", "value": secret}),
        ),
    )
    .await;
    assert_eq!(stored["ok"], true, "{stored}");

    let read = action(
        "tool.call",
        &json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
    );
    let (status, said) = json_of(&link, read).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(said["ok"], true, "{said}");
    assert_eq!(said["result"]["status"], "completed");
    assert_eq!(said["result"]["result"]["order_id"], "A-100");
    let text = said.to_string();
    assert!(!text.contains(secret), "{text}");
    assert!(text.contains(a2m_core::REDACTED));

    // A refused call comes back as its failure, through the same channel.
    let (_, refused) = json_of(&link, action("no.such_action", &json!({}))).await;
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["error"]["class"], "input");
    assert!(refused["error"]["fix"].is_string());

    drop(link);
    control.stop(StopReason::Asked);
    let (stopped, lock) = within("the engine's stop", serving).await.unwrap();
    assert_eq!(stopped, StopReason::Asked);
    world.kernel.close(CloseReason::Asked).await;
    drop(lock);
}

/// An engine that closes a connection at any moment of the client's hello
/// leaves the client with a failure, never with a wait that does not end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_whose_connection_is_closed_is_told_so_every_time() {
    let (_folder, paths) = home("drop");
    let engine = Engine::start(
        Channel::bind_expecting(&paths, VERSION, OsUser::named("someone-else"))
            .await
            .unwrap(),
        1,
        IdlePolicy::never(),
    );
    let connector = Connector::new(&paths, VERSION, ClientLabel::CommandLine).unwrap();
    for _ in 0..200 {
        let refused = connect(&connector).await.unwrap_err();
        assert_failure(&refused, FailureClass::Transient, &codes::CONNECTION_LOST);
    }
    assert_eq!(engine.control.refused(), 200);
    assert_eq!(engine.hits.load(Ordering::SeqCst), 0);
    engine.stop().await;
}

#[test]
fn the_engine_lock_says_whether_it_is_still_the_file_at_its_place() {
    let (_folder, paths) = home("p");
    let lock = EngineLock::take(&paths).unwrap();
    let place = lock.place();
    assert!(place.holds());
    // The data directory is removed while the lock is held. On Windows a
    // folder that holds an open file can seldom be removed, so this part
    // is for the systems that allow it.
    #[cfg(unix)]
    {
        std::fs::remove_dir_all(&paths.data).unwrap();
        assert!(!place.holds());
        // Made again, with a lock file at the same name: it is another
        // file, and not the lock this engine holds.
        std::fs::create_dir_all(&paths.data).unwrap();
        std::fs::write(paths.data.join(crate::ENGINE_LOCK_NAME), b"").unwrap();
        assert!(!place.holds());
    }
    drop(lock);
}

/// A start that would start the test program itself, which ends at once
/// having run no test: an engine that ends before it answers.
fn ends_at_once(paths: &Paths) -> crate::Launch {
    crate::Launch {
        program: std::env::current_exe().unwrap(),
        args: vec!["--exact".to_owned(), "no_test_has_this_name".to_owned()],
        cwd: std::env::temp_dir(),
        environment: std::collections::BTreeMap::new(),
        log: crate::Launch::log_of(paths),
        input: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_engine_that_binds_and_stalls_is_waited_for_no_longer_than_the_patience() {
    let (_folder, paths) = home("s");
    // Bound and never served: the engine of a store that takes for ever to
    // open. A connection is taken into the backlog, and no hello is
    // answered.
    let stalled = Channel::bind(&paths, VERSION).await.unwrap();
    let log = crate::Launch::log_of(&paths).display().to_string();

    let connector = Connector::new(&paths, VERSION, ClientLabel::CommandLine)
        .unwrap()
        .hello_patience(Duration::from_secs(1));
    let failure = within("a hello to a stalled engine", connector.connect())
        .await
        .unwrap_err();
    assert_eq!(failure.code(), &codes::ENGINE_UNREACHABLE, "{failure:?}");
    assert!(failure.next_step().contains(&log), "{failure:?}");

    // A client that would start an engine waits no longer than its
    // patience either, and starts none while one holds the channel.
    let connector = Connector::new(&paths, VERSION, ClientLabel::CommandLine).unwrap();
    let failure = within(
        "a start on demand beside a stalled engine",
        connector.connect_or_start(&paths, ends_at_once(&paths), Duration::from_secs(2)),
    )
    .await
    .unwrap_err();
    assert_eq!(failure.code(), &codes::ENGINE_UNREACHABLE, "{failure:?}");
    assert!(failure.next_step().contains(&log), "{failure:?}");
    drop(stalled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_engine_that_stops_while_starting_is_told_as_one_and_points_at_its_log() {
    let (_folder, paths) = home("f");
    let stalled = Channel::bind(&paths, VERSION).await.unwrap();
    let log = crate::Launch::log_of(&paths).display().to_string();

    // A plain connect whose hello the engine never answers, because it
    // stops while starting: nothing was sent, and the log is named.
    let connector = Connector::new(&paths, VERSION, ClientLabel::CommandLine).unwrap();
    let asked = tokio::spawn(async move { connector.connect().await });
    // The engine fails after its bind (an unreadable trust root, say)
    // while the hello waits: half a second leaves the client in its hello
    // whatever the machine, and an earlier stop is answered the same way.
    tokio::time::sleep(Duration::from_millis(500)).await;
    drop(stalled);
    let failure = within("the hello", asked).await.unwrap().unwrap_err();
    assert!(
        failure.code() == &codes::CONNECTION_LOST || failure.code() == &codes::ENGINE_UNREACHABLE,
        "{failure:?}"
    );
    assert_ne!(failure.class(), FailureClass::PossibleCommit);
    if failure.code() == &codes::CONNECTION_LOST {
        assert!(
            failure.message().contains("stopped while starting"),
            "{failure:?}"
        );
        assert!(failure.next_step().contains(&log), "{failure:?}");
    }

    // A client that starts engines on demand is not told to run the
    // command again: it looks on, and the engine it starts ends too, so it
    // is told that the engine ended before it answered, with the log.
    let stalled = Channel::bind(&paths, VERSION).await.unwrap();
    let connector = Connector::new(&paths, VERSION, ClientLabel::CommandLine).unwrap();
    let start = ends_at_once(&paths);
    let asked = tokio::spawn({
        let paths = paths.clone();
        async move {
            connector
                .connect_or_start(&paths, start, Duration::from_secs(20))
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    drop(stalled);
    let failure = within("the start", asked).await.unwrap().unwrap_err();
    assert_eq!(failure.code(), &codes::ENGINE_UNREACHABLE, "{failure:?}");
    assert!(
        failure.message().contains("ended before it answered"),
        "{failure:?}"
    );
    assert!(failure.next_step().contains(&log), "{failure:?}");
}

/// The output of an engine started on demand is never written through a
/// link planted at its name, and a file others may read is narrowed.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_started_engines_output_is_not_written_through_a_link_and_is_kept_to_the_user() {
    use std::os::unix::fs::PermissionsExt as _;

    let (folder, paths) = home("o");
    let log = crate::Launch::log_of(&paths);
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    let elsewhere = folder.path().join("elsewhere.txt");
    std::fs::write(&elsewhere, b"kept\n").unwrap();
    std::os::unix::fs::symlink(&elsewhere, &log).unwrap();
    let connector = Connector::new(&paths, VERSION, ClientLabel::CommandLine).unwrap();
    let started = within(
        "a start through a planted link",
        connector.connect_or_start(&paths, ends_at_once(&paths), Duration::from_secs(20)),
    )
    .await
    .unwrap();
    assert!(matches!(started, crate::Started::Refused(_)), "{started:?}");
    assert_eq!(std::fs::read(&elsewhere).unwrap(), b"kept\n");

    // A plain file of the user's that others may read is used, and closed
    // to them.
    std::fs::remove_file(&log).unwrap();
    std::fs::write(&log, b"").unwrap();
    std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o644)).unwrap();
    let failure = within(
        "a start that ends at once",
        connector.connect_or_start(&paths, ends_at_once(&paths), Duration::from_secs(20)),
    )
    .await
    .unwrap_err();
    assert_eq!(failure.code(), &codes::ENGINE_UNREACHABLE, "{failure:?}");
    let mode = std::fs::metadata(&log).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
}
