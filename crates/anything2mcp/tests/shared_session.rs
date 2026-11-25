//! Package 1.8: two clients of one engine share one signed-in session.
//!
//! The person signs in once, on the engine's page, to the test
//! application's own sign-in page. Then a command and an MCP client, each
//! its own process of the real program, read a signed-in page through the
//! one engine, one after the other and then both at once. The application
//! counts one sign-in: neither client signs in again, and the second rides
//! on the session the first was given.
//!
//! The engine is the same `LocalEngine` that `serve` starts, run in this
//! process because the program ships no presence prompt a test can pass
//! (as in `renewal.rs`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::sync::Arc;

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::served::{StdioProxy, object, output, stateless};
use common::{Machine, PASSPHRASE, Surface, USER};
use example_app::harness::{PASSWORD, TestApp, USERNAME};
use serde_json::{Value as Json, json};

/// The placeholder origin in the project's target files.
const PLACEHOLDER: &str = "http://127.0.0.1:4010";
/// The read both clients make once the person is signed in.
const READ: &str = "example_order_list";

/// A presence prompt that a person passes.
struct Passed;

impl PresencePrompt for Passed {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async { Availability::ready(PromptName::Polkit, "The prompt of a test.") })
    }

    fn ask(
        &self,
        _text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async { PromptAnswer::Present(PromptName::Polkit) })
    }
}

/// The engine as `serve` starts it on `machine`.
async fn engine(machine: &Machine) -> LocalEngine {
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        std::collections::BTreeMap::from([
            (
                "A2M_HOME".to_owned(),
                machine.home.path().display().to_string(),
            ),
            ("USER".to_owned(), USER.to_owned()),
            (PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned()),
        ]),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    LocalEngine::start(options, Arc::new(Passed) as Arc<dyn PresencePrompt>)
        .await
        .unwrap()
}

/// A command of the real program, run to its end as an agent runs it: a
/// thin client of the engine, holding no key.
async fn command(machine: &Machine, args: &[&str]) -> (Option<i32>, Json) {
    let mut program = machine.program(&[&["--json"], args].concat());
    program.env_remove(PASSPHRASE_VARIABLE);
    let done = output(program).await;
    (done.status.code(), object(&done))
}

/// An HTTP client for the engine's own page, as the person's browser.
fn browser() -> reqwest::Client {
    a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap()
}

/// The page's one-time value.
fn once_of(page: &str) -> String {
    let marker = "name=\"once\" value=\"";
    let from = page.find(marker).unwrap() + marker.len();
    page[from..from + page[from..].find('"').unwrap()].to_owned()
}

/// The person presses Carry out on the page at `link`, with `fields`
/// typed into its form. Gives what the page said.
async fn carry_out(link: &str, fields: &str) -> String {
    let http = browser();
    let text = http.get(link).send().await.unwrap().text().await.unwrap();
    let origin = &link[..link.find("/act/").unwrap()];
    http.post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={}{fields}", once_of(&text)))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

/// What a command's read gave, after checking it completed.
fn read_by_command(code: Option<i32>, read: &Json) -> Json {
    assert_eq!(code, Some(0), "{read}");
    assert_eq!(read["result"]["status"], "completed", "{read}");
    read["result"]["result"].clone()
}

/// What an MCP client's read gave, after checking it completed.
fn read_by_stdio_proxy(called: &Json) -> Json {
    assert_ne!(called["result"]["isError"], true, "{called}");
    let content = &called["result"]["structuredContent"];
    assert_eq!(content["status"], "completed", "{called}");
    content["result"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_command_and_an_mcp_client_read_through_one_engine_and_the_application_counts_one_sign_in()
 {
    let app = TestApp::start().await.unwrap();
    let origin = app.url("");
    let machine = Machine::new(
        "signed_in_pages",
        &[(PLACEHOLDER, origin.trim_end_matches('/'))],
    )
    .with_passphrase();
    let local = engine(&machine).await;
    let serving = tokio::spawn(local.serve());

    // The person publishes the target and signs in once, on the engine's
    // page.
    let (code, asked) = command(&machine, &["publish", "example", "--version", "1.0.0"]).await;
    assert_eq!(code, Some(3), "{asked}");
    let said = carry_out(asked["result"]["link"].as_str().unwrap(), "").await;
    assert!(said.contains("was carried out as you, once"), "{said}");
    let (code, asked) = command(&machine, &["session", "login", "example"]).await;
    assert_eq!(code, Some(3), "{asked}");
    let said = carry_out(
        asked["result"]["link"].as_str().unwrap(),
        &format!("&secret.user={USERNAME}&secret.password={PASSWORD}"),
    )
    .await;
    assert!(said.contains("was carried out as you, once"), "{said}");
    let signed_in = app.auth_report().await.unwrap();
    assert_eq!(
        (signed_in.sign_in_attempts, signed_in.sign_ins),
        (1, 1),
        "{signed_in:?}"
    );

    // A command reads the signed-in page.
    let call = ["call", READ, "--input", "{}"];
    let (code, read) = command(&machine, &call).await;
    let by_command = read_by_command(code, &read);
    let held = app.stored_order("A-100").await.unwrap().unwrap();
    let orders = by_command["order_ids"].as_array().unwrap();
    let at = orders.iter().position(|order| order == "A-100").unwrap();
    assert_eq!(
        by_command["customers"][at],
        held.customer.as_str(),
        "{read}"
    );

    // An MCP client, through `stdio`, reads the same page through the same
    // engine, and is given what the command was given.
    let mut stdio = StdioProxy::start(&machine, &["stdio"]);
    let ask = |id: u64| stateless(id, "tools/call", json!({"name": READ, "arguments": {}}));
    let by_bridge = read_by_stdio_proxy(&stdio.ask(&ask(1)));
    assert_eq!(by_bridge, by_command);

    // Then both at once.
    let at_once = tokio::spawn({
        let program = machine.program(&[&["--json"][..], &call].concat());
        async move {
            let mut program = program;
            program.env_remove(PASSPHRASE_VARIABLE);
            let done = output(program).await;
            (done.status.code(), object(&done))
        }
    });
    let by_bridge = tokio::task::spawn_blocking(move || {
        let answer = stdio.ask(&ask(2));
        (stdio, answer)
    });
    let (code, read) = at_once.await.unwrap();
    assert_eq!(read_by_command(code, &read), by_command);
    let (stdio, answer) = by_bridge.await.unwrap();
    assert_eq!(read_by_stdio_proxy(&answer), by_command);

    // One sign-in, the person's, and nothing written.
    let after = app.auth_report().await.unwrap();
    assert_eq!(
        (
            after.sign_in_attempts,
            after.sign_ins,
            after.failed_sign_ins
        ),
        (1, 1, 0),
        "{after:?}"
    );
    assert!(app.writes().await.unwrap().is_empty());

    let (code, _, said) = stdio.close();
    assert_eq!(code, Some(0), "{said}");
    let (code, _) = command(&machine, &["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}
