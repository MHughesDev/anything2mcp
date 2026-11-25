//! The front doors, joined: `serve` as the local engine, a command as its
//! thin client, and `stdio` as the stdio proxy of an MCP client.
//!
//! The first two tests run the real program: `serve` is a child process on
//! a temporary data directory, and commands and a stdio proxy are further
//! children that find it over the local channel. The third runs the same
//! local engine inside the test, with the real channel, the real person
//! surface, both routers, and the kernel, because the program ships no
//! presence prompt yet and the test must answer one to approve a write.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{
    Availability, EngineLock, NoPrompt, PresencePrompt, PromptAnswer, PromptFor, PromptName,
    PromptText, StopReason,
};
use anything2mcp::cli::{Finished, Host, run_with};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::served::{PATIENCE, Served, StdioProxy, object, output, request, stateless};
use common::{Machine, Surface};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const ORDER_GET: &str = "example_order_get";
const UPSERT: &str = "example_line_item_upsert";
const READ_INPUT: &str = r#"{"order_id": "A-100"}"#;
const OTHER_INPUT: &str = r#"{"order_id": "A-101"}"#;
/// The length is written as a person writes it, in feet and inches.
const WRITE_INPUT: &str = r#"{"order_id": "A-102", "item_code": "L-300", "description": "New rail", "quantity": 3, "length_in": "12' 6 1/2\""}"#;

/// A machine whose store is on disk and holds the published `example`
/// target. A person publishes on an engine in this process, which then
/// closes: the program itself has no way for a test to be the person.
async fn published(app: &TestApp) -> Machine {
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]).with_passphrase();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;
    // The engine has let its lock go, but a child that another test of this
    // process forked while the lock was held keeps a copy of it until that
    // child execs: an engine started on demand is forked, to give it a
    // session of its own. The next engine here takes the lock without
    // waiting, so the test waits until no copy is left.
    lock_comes_free(&machine).await;
    machine
}

/// A response without what differs from call to call: the identifier of
/// the recorded decision.
fn settled(mut response: Json) -> Json {
    response.as_object_mut().unwrap().remove("decision");
    response
}

/// An engine whose data directory is removed while it runs fails closed
/// (spec section 224): the next request is refused by name and nothing of
/// it is done, and the engine stops and lets go of its lock, so that the
/// next client starts a new one (decision 0181). On Windows a folder that
/// holds an open file can seldom be removed, and the test is not run.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_engine_whose_data_directory_is_removed_refuses_the_next_request_by_name_and_stops() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let mut served = Served::start(&machine);
    let without_key = |args: &[&str]| {
        let mut command = machine.program(args);
        command.env_remove(PASSPHRASE_VARIABLE);
        command
    };
    let read = ["tool", "call", ORDER_GET, "--input", READ_INPUT];
    let first = output(without_key(&read)).await;
    assert_eq!(first.status.code(), Some(0), "{first:?}");

    let paths = Paths::under(machine.home.path());
    std::fs::remove_dir_all(&paths.data).unwrap();
    let refused = output(without_key(&read)).await;
    // A failure says what the engine did, as well as what the command saw.
    assert_eq!(
        refused.status.code(),
        Some(1),
        "{refused:?}; {}",
        served.so_far()
    );
    let response = object(&refused);
    assert_eq!(
        response["error"]["code"],
        "data_directory_gone",
        "{response}; {}",
        served.so_far()
    );
    assert_eq!(response["error"]["class"], "busy", "{response}");
    assert!(
        response.get("result").is_none_or(Json::is_null),
        "{response}"
    );

    // The engine's process ends by itself, by name, and the lock is free.
    let (code, printed) = served.ended();
    assert_eq!(code, Some(1), "{printed}");
    let last: Json = serde_json::from_str(printed.trim()).unwrap();
    assert_eq!(last["action"], "serve", "{last}");
    assert_eq!(last["error"]["code"], "data_directory_gone", "{last}");
    assert!(!EngineLock::held(&paths).unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_commands_share_one_engine_a_second_engine_is_refused_and_a_stop_leaves_nothing() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let mut served = Served::start(&machine);

    // A command that does not hold the vault's passphrase cannot run the
    // engine itself: whatever it reads, the engine read.
    let without_key = |args: &[&str]| {
        let mut command = machine.program(args);
        command.env_remove(PASSPHRASE_VARIABLE);
        command
    };
    let read = ["tool", "call", ORDER_GET, "--input", READ_INPUT];
    let other = ["tool", "call", ORDER_GET, "--input", OTHER_INPUT];

    // Two commands at once, one engine, two reads.
    let (first, second) = tokio::join!(output(without_key(&read)), output(without_key(&other)));
    let mut runs = Vec::new();
    for (done, customer) in [
        (&first, "Example Customer One"),
        (&second, "Example Customer Two"),
    ] {
        assert_eq!(done.status.code(), Some(0), "{done:?}");
        let response = object(done);
        let result = &response["result"];
        assert_eq!(result["status"], "completed", "{response}");
        assert_eq!(result["result"]["customer"], customer);
        runs.push(result["run_id"].as_str().unwrap().to_owned());
    }
    assert_ne!(runs[0], runs[1]);

    // What the engine answers, a command ends on: the same exit code and
    // the same one object as a command that runs the engine itself.
    let asked: [&[&str]; 5] = [
        &["docs", "get", "approvals"],
        &["run", "get", &runs[0]],
        &["run", "get", "no-such-run"],
        &["version", "publish", "example"],
        &["draft", "lint", "example"],
    ];
    let mut through_engine = Vec::new();
    for args in asked {
        let done = output(without_key(args)).await;
        through_engine.push((done.status.code(), settled(object(&done))));
    }
    assert_eq!(through_engine[1].0, Some(0), "{through_engine:?}");
    assert_eq!(through_engine[1].1["result"]["status"], "completed");
    // A person's act, asked by a program: a person is needed, on the page
    // the engine serves for it (decision 0163).
    assert_eq!(through_engine[3].0, Some(3), "{through_engine:?}");
    assert_eq!(through_engine[3].1["result"]["state"], "needs_person");
    let link = through_engine[3].1["result"]["link"].as_str().unwrap();
    assert!(link.starts_with("http://127.0.0.1:"), "{link}");
    // The engine works on the project folder the command was run in.
    assert_eq!(through_engine[4].0, Some(0), "{through_engine:?}");
    assert_eq!(through_engine[4].1["result"]["ok"], true);

    // A command cannot run the engine inside itself while one runs.
    let inside = output(machine.program(&["--in-process", "engine", "status"])).await;
    assert_eq!(inside.status.code(), Some(1), "{inside:?}");
    assert_eq!(object(&inside)["error"]["code"], "engine_running");

    // A second engine on the same data directory is refused by name, and
    // the first goes on answering.
    let second = output(machine.program(&["serve"])).await;
    assert_eq!(second.status.code(), Some(1), "{second:?}");
    assert_eq!(object(&second)["error"]["code"], "engine_running");
    let still = output(without_key(&read)).await;
    assert_eq!(still.status.code(), Some(0), "{still:?}");

    // Asked to stop, the engine closes and its process ends by itself.
    // The engine answers the stop before it stops taking requests, so the
    // answer is the engine's own, with no warning of a lost one.
    let stop = output(without_key(&["engine", "stop"])).await;
    assert_eq!(stop.status.code(), Some(0), "{stop:?}; {}", served.so_far());
    assert_eq!(object(&stop)["result"]["stopping"], true);
    assert_eq!(object(&stop)["warnings"], json!([]), "{stop:?}");
    let (code, printed) = served.ended();
    assert_eq!(code, Some(0), "{printed}");
    let last: Json = serde_json::from_str(printed.trim()).unwrap();
    assert_eq!(last["action"], "serve");
    assert_eq!(last["result"]["stopped"], "asked");

    // Nothing is left: the runtime directory holds the lock file and no
    // socket, and the lock is free.
    let paths = Paths::under(machine.home.path());
    let left: Vec<String> = std::fs::read_dir(&paths.runtime)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, [a2m_local::ENGINE_LOCK_NAME], "{left:?}");
    assert!(!EngineLock::held(&paths).unwrap());

    // With no engine, the command that holds no passphrase is on its own,
    // with no vault: the reads above were the engine's. A command that
    // holds it runs the engine inside itself, as before, and says the
    // same as the engine said.
    let alone = output(without_key(&read)).await;
    assert_eq!(alone.status.code(), Some(1), "{alone:?}");
    // The machine's vault takes a passphrase, and the refusal names the
    // missing one (decision 0220).
    assert_eq!(
        object(&alone)["error"]["code"],
        "vault_passphrase_missing",
        "{alone:?}"
    );
    for (args, through) in asked.iter().zip(&through_engine) {
        let done = output(machine.program(args)).await;
        assert_eq!(done.status.code(), through.0, "{args:?}");
        if args[..2] == ["version", "publish"] {
            // In one process there is no page: the terminal is asked, and
            // with none the act is refused. A person is needed either way.
            assert_eq!(object(&done)["error"]["code"], "person_required");
            continue;
        }
        assert_eq!(settled(object(&done)), through.1, "{args:?}");
    }
    let inside = output(machine.program(&["--in-process", "engine", "status"])).await;
    assert_eq!(inside.status.code(), Some(0), "{inside:?}");
}

fn assert_read(answer: &Json) {
    let result = &answer["result"];
    assert_ne!(result["isError"], true, "{answer}");
    let run = &result["structuredContent"];
    assert_eq!(run["status"], "completed", "{answer}");
    assert_eq!(run["result"]["customer"], "Example Customer One");
}

fn assert_lists_tools(answer: &Json) {
    let tools = answer["result"]["tools"].as_array().unwrap();
    assert!(
        tools.iter().any(|tool| tool["name"] == "a2m_tool_call"),
        "{answer}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stdio_proxy_answers_with_the_failure_until_an_engine_runs_and_then_reads_in_both_revisions()
 {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let read = json!({"name": ORDER_GET, "arguments": {"order_id": "A-100"}});

    // No engine runs. The stdio proxy stays, and answers a request with the
    // failure and what to do, so that its caller is not left waiting.
    let mut stdio = StdioProxy::start(&machine, &["stdio"]);
    let refused = stdio.ask(&stateless(1, "tools/list", json!({})));
    assert_eq!(refused["error"]["code"], -32000, "{refused}");
    let data = &refused["error"]["data"];
    assert_eq!(data["code"], "engine_unreachable", "{refused}");
    assert!(data.to_string().contains("anything2mcp serve"), "{refused}");

    // An engine is started. The same stdio proxy carries the next request.
    let served = Served::start(&machine);

    // The stateless revision: no handshake, every request names it.
    assert_lists_tools(&stdio.ask(&stateless(2, "tools/list", json!({}))));
    assert_read(&stdio.ask(&stateless(3, "tools/call", read.clone())));

    // The handshake revision, on a stdio proxy of its own, as a client of that
    // revision starts one.
    let mut older = StdioProxy::start(&machine, &["stdio", "--toolset", "use"]);
    let hello = older.ask(&request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "raw", "version": "0"},
        }),
    ));
    assert_eq!(hello["result"]["protocolVersion"], "2025-11-25", "{hello}");
    older.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    assert_lists_tools(&older.ask(&request(2, "tools/list", json!({}))));
    assert_read(&older.ask(&request(3, "tools/call", read)));

    // The other built-in toolset is served at its own endpoint.
    let mut build = StdioProxy::start(&machine, &["stdio", "--toolset=build"]);
    let listed = build.ask(&stateless(1, "tools/list", json!({})));
    assert!(listed["result"]["tools"].is_array(), "{listed}");

    // When its input ends a stdio proxy ends, with nothing more on standard
    // output. The one that started with no engine said so on standard
    // error, and nowhere else.
    let (code, left, said) = stdio.close();
    assert_eq!(code, Some(0), "{said}");
    assert!(left.is_empty(), "{left:?}");
    assert!(said.contains("anything2mcp serve"), "{said}");
    for stdio in [older, build] {
        let (code, left, said) = stdio.close();
        assert_eq!(code, Some(0), "{said}");
        assert!(left.is_empty(), "{left:?}");
        assert!(said.is_empty(), "{said}");
    }

    let stop = output(machine.program(&["engine", "stop"])).await;
    assert_eq!(stop.status.code(), Some(0), "{stop:?}");
    assert_eq!(served.ended().0, Some(0));
}

/// The presence prompt of a test: the person passes it, and it counts how
/// often it was raised.
struct Passed(AtomicU32);

impl PresencePrompt for Passed {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async { Availability::ready(PromptName::Polkit, "The prompt of a test.") })
    }

    fn ask(
        &self,
        _text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            PromptAnswer::Present(PromptName::Polkit)
        })
    }
}

/// Waits, for a bounded time, until the engine lock of the machine is free.
///
/// The engine in this process let go of it when it closed. Another test in
/// this process may have started a child process at that moment, and a
/// child holds a copy of every file this process has open from its start
/// until it runs its program; a lock file is among them. So the lock is
/// looked at until it is free, and the test fails by name if it never is.
async fn lock_comes_free(machine: &Machine) {
    let paths = Paths::under(machine.home.path());
    let give_up = tokio::time::Instant::now() + PATIENCE;
    while EngineLock::held(&paths).unwrap() {
        assert!(
            tokio::time::Instant::now() < give_up,
            "the engine lock stayed held after the engine closed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A command of a program, run as the command line runs it, in this
/// process: nobody is at a terminal, so the caller is an agent.
async fn command(machine: &Machine, args: &[&str]) -> (Finished, Json) {
    let host = Host {
        cwd: machine.project.path().to_path_buf(),
        environment: machine.environment(),
        session_folder: None,
        stdout_is_terminal: false,
        person_at_terminal: false,
        person: None,
        key_store: None,
        program: None,
        prompt: None,
    };
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let finished = run_with(&args, &host).await;
    let json = serde_json::from_str(finished.stdout.trim())
        .unwrap_or_else(|_| panic!("not one JSON object: {finished:?}"));
    (finished, json)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_through_the_served_engine_rests_is_approved_on_its_page_and_is_written_once() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;

    // The local engine as `serve` runs it, in this process, so that the
    // test can be the person at the presence prompt.
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    assert!(local.engine().runner_problem().is_none());
    let serving = tokio::spawn(local.serve());

    // Called by an agent through the engine, the write rests, with a link
    // to its page on the loopback address.
    let (rested, response) =
        command(&machine, &["tool", "call", UPSERT, "--input", WRITE_INPUT]).await;
    assert_eq!(rested.code, 3, "{rested:?}");
    let result = &response["result"];
    assert_eq!(result["status"], "needs_approval", "{response}");
    let run = result["run_id"].as_str().unwrap().to_owned();
    let link = result["link"].as_str().unwrap().to_owned();
    assert!(link.starts_with("http://127.0.0.1:"), "{link}");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 0);

    // Resuming approves nothing: the run rests on.
    let (still, response) = command(&machine, &["run", "resume", &run]).await;
    assert_eq!(still.code, 3, "{still:?}");
    assert_eq!(response["result"]["status"], "needs_approval");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 0);

    // The page shows the change itself, and the prompt this machine has.
    let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let page = http.get(&link).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    assert_eq!(page.headers()["referrer-policy"], "same-origin");
    let text = page.text().await.unwrap();
    assert!(text.contains(UPSERT) && text.contains(&run), "{text}");
    assert!(text.contains("<th scope=\"row\">quantity</th>"), "{text}");
    assert!(text.contains("The prompt of a test."), "{text}");
    // This write is read back after it is sent, so the page does not say
    // that it is not (decision 0181).
    assert!(!text.contains("not read back"), "{text}");

    // A press that does not come from the page's own origin raises nothing
    // and approves nothing.
    let marker = "name=\"once\" value=\"";
    let from = text.find(marker).unwrap() + marker.len();
    let once = &text[from..from + text[from..].find('"').unwrap()];
    for origin in ["null", "http://127.0.0.1:1"] {
        let forged = http
            .post(format!("{link}/approve"))
            .header("origin", origin)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("once={once}"))
            .send()
            .await
            .unwrap();
        assert_eq!(forged.status().as_u16(), 403, "{origin}");
    }
    assert_eq!(prompt.0.load(Ordering::SeqCst), 0);

    // The person opens the link in a real browser, reads the page, and
    // presses Approve, as a person does; the presence prompt is raised, and
    // passed.
    let person = common::browser::Person::new();
    let (title, detail) = person.press(&link, "Approve").await;
    person.end().await;
    assert_eq!(title, "Approved", "{detail}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 0);

    // `run resume` completes it. The kernel's `run.resume` now and then
    // answers in the moment between the runner's claim of the job and its
    // work on it, with the run still at rest; a caller then resumes again.
    let mut tries = 0;
    let (done, response) = loop {
        let (done, response) = command(&machine, &["run", "resume", &run]).await;
        tries += 1;
        if response["result"]["status"] != "needs_approval" || tries == 5 {
            break (done, response);
        }
    };
    assert_eq!(done.code, 0, "{done:?}");
    assert_eq!(response["result"]["status"], "completed", "{response}");

    // The test application holds exactly one write, with the values; and
    // resuming again writes nothing.
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);
    let stored = app.stored_item("A-102", "L-300").await.unwrap().unwrap();
    assert_eq!(stored.description, "New rail");
    assert_eq!(stored.quantity, 3);
    assert_eq!(stored.length_sixteenths, 2408);
    let (again, _) = command(&machine, &["run", "resume", &run]).await;
    assert_eq!(again.code, 0, "{again:?}");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 1);

    // Asked to stop through the channel, the engine stops serving and
    // closes, and the lock is free.
    let (stop, _) = command(&machine, &["engine", "stop"]).await;
    assert_eq!(stop.code, 0, "{stop:?}");
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    lock_comes_free(&machine).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_no_presence_prompt_a_write_through_the_served_engine_rests_and_its_page_says_why() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;

    // The local engine as `serve` runs it on an operating system whose
    // presence prompt this build does not have.
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::new(NoPrompt))
        .await
        .unwrap();
    // What `serve` says before it is ready: the kind, the reason, and
    // what to do.
    let line = local.presence_line().await;
    let none = local.surface().presence().await;
    assert!(
        line.starts_with("anything2mcp: approvals on this machine: not_available, not_built. "),
        "{line}"
    );
    assert!(
        line.contains(&none.message) && line.contains(&none.remedy),
        "{line}"
    );
    let serving = tokio::spawn(local.serve());

    // The write rests, with a link to its page.
    let (rested, response) =
        command(&machine, &["tool", "call", UPSERT, "--input", WRITE_INPUT]).await;
    assert_eq!(rested.code, 3, "{rested:?}");
    let result = &response["result"];
    assert_eq!(result["status"], "needs_approval", "{response}");
    let run = result["run_id"].as_str().unwrap().to_owned();
    let link = result["link"].as_str().unwrap().to_owned();

    // The page says plainly that no presence prompt is available on this
    // machine, and why, before anything is pressed.
    let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let text = http.get(&link).send().await.unwrap().text().await.unwrap();
    assert!(
        text.contains("No presence prompt is available on this machine"),
        "{text}"
    );
    assert!(text.contains(&none.message), "{text}");

    // A press of Approve says the same and approves nothing.
    let marker = "name=\"once\" value=\"";
    let from = text.find(marker).unwrap() + marker.len();
    let once = &text[from..from + text[from..].find('"').unwrap()];
    let origin = &link[..link.find("/approve/").unwrap()];
    let pressed = http
        .post(format!("{link}/approve"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap();
    assert_eq!(pressed.status().as_u16(), 403);
    let said = pressed.text().await.unwrap();
    assert!(said.contains("Nothing was approved"), "{said}");
    assert!(said.contains(&none.message), "{said}");
    let (still, response) = command(&machine, &["run", "resume", &run]).await;
    assert_eq!(still.code, 3, "{still:?}");
    assert_eq!(response["result"]["status"], "needs_approval");
    assert_eq!(app.write_count("A-102", "L-300").await.unwrap(), 0);

    let (stop, _) = command(&machine, &["engine", "stop"]).await;
    assert_eq!(stop.code, 0, "{stop:?}");
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    lock_comes_free(&machine).await;
}

/// A command sends its action to the action's own REST route. Every
/// action with a command face has one, so no command is left that only a
/// one-shot engine could carry out.
#[test]
fn every_action_command_has_the_rest_route_a_thin_client_sends_it_to() {
    let registry = anything2mcp::action_registry().unwrap();
    let routes: std::collections::BTreeSet<String> = a2m_actions::rest_routes(&registry)
        .into_iter()
        .filter(|route| !route.alias && route.method == "POST")
        .map(|route| route.path)
        .collect();
    for face in a2m_actions::command_faces(&registry) {
        let path = format!("{}/actions/{}", a2m_actions::REST_PREFIX, face.action);
        assert!(routes.contains(&path), "{}", face.action);
    }
}

/// Nothing a program says of itself on the local channel makes it a
/// person: not the label `cli` of its hello, not a header that says a
/// person is at its terminal, and not a terminal of its own. A person's
/// act asked through the channel is refused as an agent's, and no presence
/// prompt is raised for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_program_that_says_it_is_a_person_at_a_terminal_is_an_agent_and_raises_no_prompt() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]).with_passphrase();
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());

    // A program writes what the command line used to: the label `cli`,
    // the header, and the project folder.
    let connector = a2m_local::Connector::new(
        &Paths::under(machine.home.path()),
        anything2mcp::version().parse().unwrap(),
        a2m_local::ClientLabel::CommandLine,
    )
    .unwrap();
    let link = connector.connect().await.unwrap();
    // The folder is named as the command line names it: a path on any
    // system may hold what a header cannot carry as it is.
    let folder = anything2mcp::wire::encode_path(machine.project.path()).unwrap();
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(format!(
            "{}/actions/version.publish",
            a2m_actions::REST_PREFIX
        ))
        .header("content-type", "application/json")
        .header("a2m-person-at-terminal", "true")
        .header(anything2mcp::wire::PROJECT_HEADER, folder)
        .body(axum::body::Body::from(r#"{"target": "example"}"#))
        .unwrap();
    let answer = link.send(request).await.unwrap();
    let body = axum::body::to_bytes(answer.into_body(), 1 << 20)
        .await
        .unwrap();
    let response: Json = serde_json::from_slice(&body).unwrap();
    // Asked for, the act waits on its page; a prompt rises only when
    // someone presses there (decision 0163).
    assert_eq!(response["result"]["state"], "needs_person", "{response}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 0);

    // The command line itself, started at a terminal, is the same through
    // the engine: what it sees of its own terminal is not sent.
    let host = Host {
        cwd: machine.project.path().to_path_buf(),
        environment: machine.environment(),
        session_folder: None,
        stdout_is_terminal: false,
        person_at_terminal: true,
        person: None,
        key_store: None,
        program: None,
        prompt: None,
    };
    let args: Vec<String> = ["version", "publish", "example"]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
    let finished = run_with(&args, &host).await;
    assert_eq!(finished.code, 3, "{finished:?}");
    let response: Json = serde_json::from_str(finished.stdout.trim()).unwrap();
    // Asked for, the act waits on its page; a prompt rises only when
    // someone presses there (decision 0163).
    assert_eq!(response["result"]["state"], "needs_person", "{response}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 0);

    let (stop, _) = command(&machine, &["engine", "stop"]).await;
    assert_eq!(stop.code, 0, "{stop:?}");
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}

/// A project folder travels to the engine as the command line names it,
/// whatever its path holds: here a space and a letter outside ASCII, as a
/// Windows path holds `\` and `:`. The engine opened in no project, so the
/// folder the command was run in reaches it only through the channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_project_folder_whose_name_holds_a_space_and_a_letter_outside_ascii_reaches_the_engine() {
    let app = TestApp::start().await.unwrap();
    let folder = common::Folder::new("caf\u{e9} orders");
    assert!(folder.path().to_str().unwrap().contains("caf\u{e9} orders"));
    let machine =
        Machine::in_folder("orders", &[(PLACEHOLDER, &app.url(""))], folder).with_passphrase();
    let options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    assert!(options.project.is_none());
    let local = LocalEngine::start(options, Arc::new(NoPrompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());

    let (linted, response) = command(&machine, &["draft", "lint", "example"]).await;
    assert_eq!(linted.code, 0, "{linted:?}");
    assert_eq!(response["result"]["ok"], true, "{response}");

    let (stop, _) = command(&machine, &["engine", "stop"]).await;
    assert_eq!(stop.code, 0, "{stop:?}");
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}

/// A person's act through a running engine, end to end (decision 0163):
/// the real program, as an agent, asks to publish the drafted target; the
/// engine answers that a person is needed, with the page's link; a person
/// presses Carry out on the page and passes the presence prompt; the
/// version is published once, and the same call then gives back what
/// became of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agents_publish_waits_on_its_page_and_a_persons_press_publishes_it_once() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]).with_passphrase();
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());
    let read = ["tool", "call", ORDER_GET, "--input", READ_INPUT];

    // Nothing is published yet, so the read finds no tool.
    let before = output(machine.program(&read)).await;
    assert_ne!(before.status.code(), Some(0), "{before:?}");

    // The real program, as an agent, asks; a person is needed, on a page.
    let publish = ["version", "publish", "example"];
    let asked = output(machine.program(&publish)).await;
    assert_eq!(asked.status.code(), Some(3), "{asked:?}");
    let response = object(&asked);
    assert_eq!(response["result"]["state"], "needs_person", "{response}");
    let link = response["result"]["link"].as_str().unwrap().to_owned();
    assert!(link.starts_with("http://127.0.0.1:"), "{link}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 0);

    // The page shows what will be done; a person presses Carry out, from
    // the page's own origin, and passes the prompt.
    let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let page = http.get(&link).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    let text = page.text().await.unwrap();
    assert!(text.contains("version.publish"), "{text}");
    // The page says where the version leads, by address, in each environment.
    assert!(
        text.contains(&format!(
            "<h2>Where it leads</h2><ul><li><code>api</code>, a surface of the kind <code>http</code>, leads to <code>{}</code> in the environment <code>test</code>;",
            app.url("").trim_end_matches('/')
        )),
        "{text}"
    );
    let marker = "name=\"once\" value=\"";
    let from = text.find(marker).unwrap() + marker.len();
    let once = &text[from..from + text[from..].find('"').unwrap()];
    let origin = &link[..link.find("/act/").unwrap()];
    let pressed = http
        .post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap();
    let said = pressed.status().as_u16();
    let body = pressed.text().await.unwrap();
    assert_eq!(said, 200, "{body}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);

    // Published: the read is answered.
    let after = output(machine.program(&read)).await;
    assert_eq!(after.status.code(), Some(0), "{after:?}");
    assert_eq!(object(&after)["result"]["status"], "completed");

    // The same call gives back what became of the act, and publishes
    // nothing more: no second page, and no second prompt.
    let again = output(machine.program(&publish)).await;
    let response = object(&again);
    assert_ne!(response["result"]["state"], "needs_person", "{response}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);
    // A second press on the same page finds nothing to carry out: the page
    // is refused, or its listener has closed with nothing left waiting.
    let twice = http
        .post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={once}"))
        .send()
        .await;
    assert!(
        twice
            .as_ref()
            .map_or(true, |answer| answer.status().as_u16() != 200),
        "{twice:?}"
    );
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);

    let stop = output(machine.program(&["engine", "stop"])).await;
    assert_eq!(stop.status.code(), Some(0), "{stop:?}");
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    lock_comes_free(&machine).await;
}

/// A presence prompt that keeps what it was asked for, and passes.
struct Recording(std::sync::Mutex<Vec<PromptFor>>);

impl PresencePrompt for Recording {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async { Availability::ready(PromptName::Polkit, "The prompt of a test.") })
    }

    fn ask(
        &self,
        text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            self.0.lock().unwrap().push(text.purpose);
            PromptAnswer::Present(PromptName::Polkit)
        })
    }
}

/// Decision 0310: `serve` run with a `Host` that carries a presence prompt
/// asks that prompt, and asks it for an approval with the effect class of
/// the operation, which a prompt that must refuse an irreversible one reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_asks_the_prompt_of_its_host_for_an_approval_with_the_operations_effect_class() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let prompt = Arc::new(Recording(std::sync::Mutex::new(Vec::new())));
    let host = Host {
        cwd: machine.project.path().to_path_buf(),
        environment: machine.environment(),
        session_folder: None,
        stdout_is_terminal: false,
        person_at_terminal: false,
        person: None,
        key_store: None,
        program: None,
        prompt: Some(Arc::clone(&prompt) as Arc<dyn PresencePrompt>),
    };
    let serving = tokio::spawn({
        let args = vec!["serve".to_owned()];
        async move { run_with(&args, &host).await }
    });
    // The test waits until the engine answers on its channel. It does not
    // ask for the engine lock: a question of the lock takes the lock for a
    // moment, and a serve that asks for it in that moment is refused.
    let connector = a2m_local::Connector::new(
        &Paths::under(machine.home.path()),
        anything2mcp::version().parse().unwrap(),
        a2m_local::ClientLabel::CommandLine,
    )
    .unwrap();
    let give_up = tokio::time::Instant::now() + PATIENCE;
    loop {
        match connector.connect().await {
            Ok(link) => {
                drop(link);
                break;
            }
            Err(failure) if *failure.code() == a2m_local::codes::ENGINE_UNREACHABLE => {}
            Err(failure) => panic!("the engine refused the connection: {failure:?}"),
        }
        // A serve that ended before it answered says why it ended.
        if serving.is_finished() {
            panic!("serve ended before it answered: {:?}", serving.await);
        }
        assert!(
            tokio::time::Instant::now() < give_up,
            "serve never answered"
        );
        // A pause before asking again, not a wait for a moment.
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let (rested, response) =
        command(&machine, &["tool", "call", UPSERT, "--input", WRITE_INPUT]).await;
    assert_eq!(rested.code, 3, "{rested:?}");
    let link = response["result"]["link"].as_str().unwrap().to_owned();
    let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let text = http.get(&link).send().await.unwrap().text().await.unwrap();
    let marker = "name=\"once\" value=\"";
    let from = text.find(marker).unwrap() + marker.len();
    let once = &text[from..from + text[from..].find('"').unwrap()];
    let origin = &link[..link.find("/approve/").unwrap()];
    let pressed = http
        .post(format!("{link}/approve"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        pressed.status().as_u16(),
        200,
        "{}",
        pressed.text().await.unwrap()
    );
    assert_eq!(
        *prompt.0.lock().unwrap(),
        [PromptFor::Approval(Some(a2m_core::EffectClass::Write))]
    );

    let (stop, _) = command(&machine, &["engine", "stop"]).await;
    assert_eq!(stop.code, 0, "{stop:?}");
    let ended = serving.await.unwrap();
    assert_eq!(ended.code, 0, "{ended:?}");
    lock_comes_free(&machine).await;
}

/// A read through the served engine leaves one run row and the events of
/// that run: the engine's own record, read back by the commands that
/// ask for it (roadmap package 1.4).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_leaves_one_run_row_and_its_events() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let served = Served::start(&machine);
    let without_key = |args: &[&str]| {
        let mut command = machine.program(args);
        command.env_remove(PASSPHRASE_VARIABLE);
        command
    };

    let read = output(without_key(&[
        "tool", "call", ORDER_GET, "--input", READ_INPUT,
    ]))
    .await;
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let result = object(&read)["result"].clone();
    assert_eq!(result["status"], "completed");
    let run = result["run_id"].as_str().unwrap().to_owned();

    // The row: this run, completed, with the value the read returned.
    let row = output(without_key(&["run", "get", &run])).await;
    assert_eq!(row.status.code(), Some(0), "{row:?}");
    let row = object(&row)["result"].clone();
    assert_eq!(row["run_id"], run.as_str());
    assert_eq!(row["status"], "completed");
    assert_eq!(row["result"], result["result"]);

    // Exactly one row for the one read.
    let listed = output(without_key(&["run", "list"])).await;
    assert_eq!(listed.status.code(), Some(0), "{listed:?}");
    let listed = object(&listed)["result"]["runs"].clone();
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["run_id"], run.as_str());

    // Its events, in order, ending on the state change to completed.
    let trace = output(without_key(&["trace", "show", &run])).await;
    assert_eq!(trace.status.code(), Some(0), "{trace:?}");
    let trace = object(&trace)["result"].clone();
    let kinds: Vec<&str> = trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"binding_chosen"), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&"state_changed"), "{kinds:?}");

    let stop = output(without_key(&["engine", "stop"])).await;
    assert_eq!(stop.status.code(), Some(0), "{stop:?}");
    let (code, printed) = served.ended();
    assert_eq!(code, Some(0), "{printed}");
}

/// An MCP client, through `stdio`, reads what a command reads, and the
/// tool list it is shown is the registry's built-in tools and the
/// published tools of the machine, and nothing hand-written (roadmap
/// package 1.5; the MCP part of the parity check of spec section 220).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_mcp_client_reads_what_a_command_reads_and_its_tool_list_is_the_registrys() {
    let app = TestApp::start().await.unwrap();
    let machine = published(&app).await;
    let served = Served::start(&machine);

    // The command's read.
    let mut command = machine.program(&["tool", "call", ORDER_GET, "--input", READ_INPUT]);
    command.env_remove(PASSPHRASE_VARIABLE);
    let done = output(command).await;
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    let by_command = object(&done)["result"]["result"].clone();

    // The same read, by an MCP client.
    let mut stdio = StdioProxy::start(&machine, &["stdio"]);
    let called = stdio.ask(&stateless(
        1,
        "tools/call",
        json!({"name": ORDER_GET, "arguments": {"order_id": "A-100"}}),
    ));
    assert_ne!(called["result"]["isError"], true, "{called}");
    assert_eq!(
        called["result"]["structuredContent"]["result"], by_command,
        "{called}"
    );

    // The tool list: every built-in tool is the face of an action in the
    // registry, with that face's description and input schema, and the
    // others are the machine's published tools, each named for its target.
    let registry = anything2mcp::action_registry().unwrap();
    let faces = a2m_actions::mcp_tools(&registry, a2m_actions::BuiltInToolset::Use);
    let listed = stdio.ask(&stateless(2, "tools/list", json!({})));
    let tools = listed["result"]["tools"].as_array().unwrap();
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        if let Some(face) = faces.iter().find(|face| face.name == name) {
            assert_eq!(tool["description"], face.description.as_str(), "{name}");
            assert_eq!(tool["inputSchema"], face.input_schema, "{name}");
        } else {
            assert!(
                !name.starts_with("a2m_") && name.starts_with("example_"),
                "a tool the registry does not describe: {name}"
            );
        }
    }
    for face in &faces {
        assert!(
            tools.iter().any(|tool| tool["name"] == face.name.as_str()),
            "the registry's tool is not listed: {}",
            face.name
        );
    }

    let (code, _, said) = stdio.close();
    assert_eq!(code, Some(0), "{said}");
    let stop = output(machine.program(&["engine", "stop"])).await;
    assert_eq!(stop.status.code(), Some(0), "{stop:?}");
    assert_eq!(served.ended().0, Some(0));
}

/// Two commands at once, each a read through the pages of the test
/// application, are served by one engine through one browser: the browser
/// pack starts one program for the profile, and the second read uses it
/// again (roadmap package 1.4). The browser's launches are counted by the
/// browser pack's test equipment under this machine's own home; the engine
/// runs in this process so that the count is this process's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_commands_at_once_read_through_one_browser() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("pages", &[(PLACEHOLDER, &app.url(""))]).with_passphrase();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;
    lock_comes_free(&machine).await;

    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    assert!(local.engine().runner_problem().is_none());
    let serving = tokio::spawn(local.serve());
    let launches = || a2m_pack_web_browser::test_support::launches_under(machine.home.path());
    assert_eq!(launches(), 0);

    let input = |item: &str| format!(r#"{{"order_id": "A-100", "item_code": "{item}"}}"#);
    let (left, right) = (input("L-101"), input("L-102"));
    let read = |input: &str| {
        let args = ["tool", "call", "example_line_item_get", "--input", input];
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let machine = &machine;
        async move {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            command(machine, &args).await
        }
    };
    let ((first, one), (second, two)) = tokio::join!(read(&left), read(&right));
    for (done, response, item) in [(&first, &one, "L-101"), (&second, &two, "L-102")] {
        assert_eq!(done.code, 0, "{done:?}");
        let result = &response["result"];
        assert_eq!(result["status"], "completed", "{response}");
        assert_eq!(result["mode"], "ui", "{response}");
        let held = app.stored_item("A-100", item).await.unwrap().unwrap();
        assert_eq!(
            result["result"]["description"],
            held.description.as_str(),
            "{response}"
        );
    }
    assert_ne!(one["result"]["run_id"], two["result"]["run_id"]);
    // One browser served both reads.
    assert_eq!(launches(), 1);

    let (stopped, _) = command(&machine, &["engine", "stop"]).await;
    assert_eq!(stopped.code, 0, "{stopped:?}");
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}

/// The address of no stand: a second place a draft can reach.
const ELSEWHERE: &str = "http://127.0.0.1:9";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_that_reaches_a_new_address_is_published_only_from_the_page_that_names_it() {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.url(""))]).with_passphrase();
    let file = machine
        .project
        .path()
        .join("targets")
        .join("example")
        .join("target.yaml");
    let written = std::fs::read_to_string(&file).unwrap();
    std::fs::remove_file(&file).unwrap();

    // A person adds the target at the test application's address.
    let world = machine.open().await;
    let address = app.url("");
    let added = world
        .person(&[
            "target",
            "add",
            "example",
            "--address",
            &address,
            "--kind",
            "http",
        ])
        .await;
    assert_eq!(added.finished.code, 0, "{:?}", added.finished);

    // The target's file as it was, at that address, and a draft that adds
    // a surface at another.
    let widened = written.replace(
        "environments:",
        &format!("  spare:\n    kind: http\n    base: \"{ELSEWHERE}\"\nenvironments:"),
    );
    assert_ne!(widened, written);
    std::fs::write(&file, &widened).unwrap();

    // A person's publish from the command line stops, naming the new
    // address, and nothing is published.
    let refused = world.person(&["version", "publish", "example"]).await;
    assert_ne!(refused.finished.code, 0, "{:?}", refused.finished);
    assert_eq!(
        refused.error()["code"],
        "address_unregistered",
        "{:?}",
        refused.finished
    );
    assert!(
        refused.error()["message"]
            .as_str()
            .unwrap()
            .contains(ELSEWHERE),
        "{:?}",
        refused.finished
    );
    let unread = world
        .agent(&["tool", "call", ORDER_GET, "--input", READ_INPUT])
        .await;
    assert_eq!(unread.error()["code"], "not_found", "{:?}", unread.finished);
    world.close().await;

    // Asked for on its page, the publish names the new address there, and
    // a person's press publishes it.
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());
    let asked = output(machine.program(&["version", "publish", "example"])).await;
    assert_eq!(asked.status.code(), Some(3), "{asked:?}");
    let link = object(&asked)["result"]["link"]
        .as_str()
        .unwrap()
        .to_owned();
    let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let text = http.get(&link).send().await.unwrap().text().await.unwrap();
    assert!(
        text.contains(&format!(
            "<li>New address: the drafts reach `{ELSEWHERE}`, which no person registered for this target. Publishing registers it.</li>"
        )),
        "{text}"
    );
    let marker = "name=\"once\" value=\"";
    let from = text.find(marker).unwrap() + marker.len();
    let once = &text[from..from + text[from..].find('"').unwrap()];
    let origin = &link[..link.find("/act/").unwrap()];
    let pressed = http
        .post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap();
    let said = pressed.status().as_u16();
    let body = pressed.text().await.unwrap();
    assert_eq!(said, 200, "{body}");
    let read = output(machine.program(&["tool", "call", ORDER_GET, "--input", READ_INPUT])).await;
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let stop = output(machine.program(&["engine", "stop"])).await;
    assert_eq!(stop.status.code(), Some(0), "{stop:?}");
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
    lock_comes_free(&machine).await;

    // A draft that adds no address publishes from the command line with no
    // further question.
    std::fs::write(
        &file,
        widened.replace(
            "title: Example order entry",
            "title: Example order entry, renamed",
        ),
    )
    .unwrap();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    assert_eq!(
        published.result()["current"],
        true,
        "{:?}",
        published.finished
    );
    world.close().await;
}
