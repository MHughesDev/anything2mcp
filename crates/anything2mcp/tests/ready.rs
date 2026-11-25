//! `ready` (decisions 0401 and 0443), through the command face only: from
//! an empty folder, `init`, `add`, `ready`, the press of the person on the
//! engine's page, and `call` give a published read that returns what the
//! test application holds. No step is left for the agent to guess: each
//! answer's `next` is the call that moves the work on, and it is followed
//! as it is given.
//!
//! On the way, a named write and a named read with no completed trial are
//! each refused before anything is done, with their codes; a read whose
//! fixture cannot be recorded is reported with that failure while the next
//! is still made ready; and the write and the read the scout failed are
//! left alone, each with its reason, until the drafts with no offline test
//! are removed as `next` says.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER};
use example_app::harness::TestApp;
use serde_json::Value as Json;

/// The read the developer calls once it is published.
const READ: &str = "example_api_orders_get";

/// The read the scout tried and did not prove: the stand fails every third
/// call of it.
const FAILED_READ: &str = "example_api_stock_get";

/// The read whose fixture cannot be recorded: a folder of its fixture's
/// name is there already, and `fixture.record` never writes over one.
const BLOCKED_READ: &str = "example_orders_shipment_get";
const BLOCKED_FOLDER: &str = "fixtures/orders_shipment_get";

/// A write the developer drafted by hand, in place of the scout's read of
/// a line item, whose request its read-back states: the scout never drafts
/// a write.
const REPLACED_READ: &str = "operations/orders_items_get.yaml";
const WRITE: &str = "example_item_put";
const WRITE_ENDPOINT: &str = "endpoints/item_put.yaml";
const WRITE_OPERATION: &str = "operations/item_put.yaml";

const WRITE_ENDPOINT_TEXT: &str = r#"endpoint: item_read_back
surface: http
method: GET
path: "/api/orders/{order_id}/items/{item_code}"
auth: none
effect: read
headers: {accept: "application/json"}
responses: {absent: [404]}
version:
  read: {field: version}
response_schema:
  type: object
  required: [order_id, item_code, description, quantity, length_in, version]
  properties:
    order_id: {type: string}
    item_code: {type: string}
    description: {type: string}
    quantity: {type: integer}
    length_in: {type: string}
    version: {type: integer}
---
endpoint: item_put
surface: http
method: PUT
path: "/api/orders/{order_id}/items/{item_code}"
auth: none
effect: write
headers: {accept: "application/json"}
version:
  send: {body: version}
request_schema:
  type: object
  required: [description, quantity, length_in]
  properties:
    description: {type: string}
    quantity: {type: integer}
    length_in: {type: string}
    version: {type: integer}
responses:
  ok: [200, 201]
  refused:
    - {status: 422, when: {error: validation}}
    - {status: 409, when: {error: order_not_draft}}
  conflict:
    - {status: 409, when: {error: version_conflict}, current: current_version}
response_schema:
  type: object
  required: [order_id, item_code, description, quantity, length_in, version]
  properties:
    order_id: {type: string}
    item_code: {type: string}
    description: {type: string}
    quantity: {type: integer}
    length_in: {type: string}
    version: {type: integer}
"#;

const WRITE_OPERATION_TEXT: &str = r#"operation: example_item_put
title: Create or update a line item
effect: write
key: [order_id, item_code]
commit_points: 1
inputs:
  order_id: {type: identifier, description: "Order number"}
  item_code: {type: identifier, description: "Item code"}
  description: {type: text, description: "What the item is"}
  quantity: {type: integer, description: "How many"}
  length_in: {type: length_ft_in, description: "Length, in inches"}
outputs:
  quantity: {type: integer}
  length_in: {type: length_ft_in}
bindings:
  - mode: http
    verb: upsert
    endpoint: item_put
    body:
      description: input.description
      quantity: input.quantity
      length_in: input.length_in
    read_back: {endpoint: item_read_back}
description: >
  Creates or updates one line item on a draft order.
"#;

/// A presence prompt that a person passes, and that counts.
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

/// The developer's machine: a home for the engine and an empty folder.
struct Desk {
    home: Folder,
    project: Folder,
}

impl Desk {
    fn new() -> Self {
        let home = Folder::new("home");
        let installation = Paths::under(home.path()).installation_config;
        std::fs::create_dir_all(&installation).unwrap();
        std::fs::write(
            installation.join("settings.toml"),
            "[vault]\nbackend = \"passphrase\"\n",
        )
        .unwrap();
        Self {
            home,
            project: Folder::new("ready-project"),
        }
    }

    fn target(&self) -> std::path::PathBuf {
        self.project.path().join("targets").join("example")
    }

    /// Runs the real program in the project folder, as an agent runs it,
    /// and gives its exit code and its object.
    async fn run(&self, args: &[&str]) -> (Option<i32>, Json) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
        command
            .arg("--json")
            .args(args)
            .current_dir(self.project.path())
            .env_remove(PASSPHRASE_VARIABLE)
            .env("A2M_HOME", self.home.path())
            .env("USER", USER)
            .env(IN_PROCESS_VARIABLE, "true")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let done: Output = tokio::task::spawn_blocking(move || command.output().unwrap())
            .await
            .unwrap();
        let object = serde_json::from_slice(&done.stdout).unwrap_or_else(|_| {
            panic!(
                "`{}` printed no single object: {} / {}",
                args.join(" "),
                String::from_utf8_lossy(&done.stdout),
                String::from_utf8_lossy(&done.stderr)
            )
        });
        (done.status.code(), object)
    }

    async fn ok(&self, args: &[&str]) -> Json {
        let (code, object) = self.run(args).await;
        assert_eq!(code, Some(0), "`{}`: {object}", args.join(" "));
        object
    }
}

/// The names of the tools a list of `ready`'s result holds.
fn names(list: &Json) -> Vec<String> {
    let mut names: Vec<String> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["tool"].as_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// The entry of `tool` in a list of `ready`'s result.
fn entry<'a>(list: &'a Json, tool: &str) -> &'a Json {
    list.as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["tool"] == tool)
        .unwrap_or_else(|| panic!("{tool} is not in {list}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ready_takes_a_tried_read_to_a_publish_and_leaves_the_rest_alone() {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new();
    desk.ok(&["init"]).await;

    // The engine, as `serve` starts it in this project, with a person's
    // page and a presence prompt a person passes.
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let mut options = EngineOptions::of_machine(
        Paths::under(desk.home.path()),
        std::collections::BTreeMap::from([
            (
                "A2M_HOME".to_owned(),
                desk.home.path().display().to_string(),
            ),
            ("USER".to_owned(), USER.to_owned()),
            (PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned()),
        ]),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(desk.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());

    // `add`: the target, the scout, and its report. The scout is continued
    // as its `next` says until the report is there.
    let address = app.url("");
    let mut added = desk
        .ok(&["add", "example", "--address", &address, "--kind", "http"])
        .await;
    for _ in 0..20 {
        if added["result"].get("report_hash").is_some()
            || added["result"]["content"]["tried"].is_object()
        {
            break;
        }
        added = desk
            .ok(&["scout", "run", "example", "--wait-seconds", "10"])
            .await;
    }

    // A write the developer drafted by hand, in place of a read.
    let removed = desk
        .ok(&["draft", "delete", "example", "--file", REPLACED_READ])
        .await;
    assert_eq!(removed["result"]["removed"], true, "{removed}");
    for (file, text) in [
        (WRITE_ENDPOINT, WRITE_ENDPOINT_TEXT),
        (WRITE_OPERATION, WRITE_OPERATION_TEXT),
    ] {
        let written = desk
            .ok(&[
                "draft",
                "write",
                "example",
                "--file",
                file,
                "--content",
                text,
            ])
            .await;
        assert_eq!(written["result"]["ok"], true, "{written}");
    }

    // Named, a write is refused before anything is done, with its reason
    // and a true next step.
    let (code, refused) = desk
        .run(&["ready", "example", "--tools", WRITE, "--tools", READ])
        .await;
    assert_eq!(code, Some(1), "{refused}");
    assert_eq!(
        refused["error"]["code"], "not_made_ready_not_a_read",
        "{refused}"
    );
    assert_eq!(
        entry(&refused["result"]["left_alone"], WRITE)["reason"],
        "write"
    );
    assert_eq!(refused["next"]["action"], "test.run", "{refused}");
    assert!(names(&refused["result"]["ready"]).is_empty(), "{refused}");
    // And so is a read no trial completed since the scout.
    let (code, refused) = desk
        .run(&["ready", "example", "--tools", FAILED_READ])
        .await;
    assert_eq!(code, Some(1), "{refused}");
    assert_eq!(
        refused["error"]["code"], "not_made_ready_untried",
        "{refused}"
    );
    let failed = entry(&refused["result"]["left_alone"], FAILED_READ);
    assert_eq!(failed["reason"], "trial_failed", "{refused}");
    assert_eq!(refused["next"]["action"], "draft.try", "{refused}");
    assert_eq!(refused["next"]["args"]["tool"], FAILED_READ, "{refused}");
    // Nothing was written by either.
    assert!(!desk.target().join("tests").exists());
    assert!(!desk.target().join("fixtures").exists());

    // A read whose fixture cannot be recorded: a folder of its name is in
    // the way.
    std::fs::create_dir_all(desk.target().join(BLOCKED_FOLDER)).unwrap();

    // `ready`: every proven read made ready but the blocked one, which is
    // reported with `fixture.record`'s own failure; the write and the read
    // the scout failed left alone. The gates do not pass, and no person is
    // asked.
    let (code, first) = desk.run(&["ready", "example"]).await;
    assert_eq!(code, Some(1), "{first}");
    let result = &first["result"];
    let made = names(&result["ready"]);
    assert!(made.contains(&READ.to_owned()), "{first}");
    assert!(!made.contains(&BLOCKED_READ.to_owned()), "{first}");
    for tool in &made {
        let ready = entry(&result["ready"], tool);
        assert_eq!(ready["fixture"], "recorded", "{ready}");
        assert_eq!(ready["binding"], "promoted", "{ready}");
        assert_eq!(ready["recorded"]["test_state"], "passed", "{ready}");
    }
    let blocked = entry(&result["failed"], BLOCKED_READ);
    assert_eq!(blocked["step"], "fixture.record", "{blocked}");
    assert_eq!(blocked["error"]["code"], "schema_invalid", "{blocked}");
    assert_eq!(
        names(&result["left_alone"]),
        [FAILED_READ, WRITE],
        "{first}"
    );
    assert_eq!(result["publish"]["state"], "not_asked", "{first}");
    assert_eq!(result["gates"]["passed"], false, "{first}");
    // What moves it on: the failure corrected, and `ready` again.
    assert_eq!(first["next"]["action"], "draft.ready", "{first}");
    // The read made ready is verified, through the checked write.
    assert!(
        std::fs::read_to_string(desk.target().join("operations").join("api_orders_get.yaml"))
            .unwrap()
            .contains("status: \"verified\"")
    );

    // The folder in the way is taken out, and `ready` again: what was done
    // stays done, and the blocked read is made ready now. What is left has
    // no offline test, and `next` names its removal.
    std::fs::remove_dir(desk.target().join(BLOCKED_FOLDER)).unwrap();
    let (code, second) = desk.run(&["ready", "example"]).await;
    assert_eq!(code, Some(1), "{second}");
    let result = &second["result"];
    assert_eq!(
        entry(&result["ready"], BLOCKED_READ)["fixture"],
        "recorded",
        "{second}"
    );
    assert_eq!(entry(&result["ready"], READ)["fixture"], "recorded_before");
    assert_eq!(entry(&result["ready"], READ)["binding"], "verified_before");
    assert!(names(&result["failed"]).is_empty(), "{second}");
    assert_eq!(
        names(&result["left_alone"]),
        [FAILED_READ, WRITE],
        "{second}"
    );
    let next = &second["next"];
    assert_eq!(next["action"], "draft.delete", "{second}");
    let files: Vec<String> = next["args"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file.as_str().unwrap().to_owned())
        .collect();
    assert!(files.contains(&WRITE_OPERATION.to_owned()), "{second}");
    assert_eq!(files.len(), 2, "{second}");

    // The removal, as `next` gives it.
    let mut removal = vec!["draft", "delete", "example"];
    for file in &files {
        removal.push("--files");
        removal.push(file);
    }
    let removed = desk.ok(&removal).await;
    assert_eq!(removed["result"]["removed"], true, "{removed}");

    // `ready` once more: the gates pass, and a person is asked to publish
    // on the engine's page. Nothing else is left to do until they have.
    let (code, asked) = desk.run(&["ready", "example", "--version", "1.0.0"]).await;
    assert_eq!(code, Some(3), "{asked}");
    let result = &asked["result"];
    assert_eq!(result["gates"]["passed"], true, "{asked}");
    assert!(names(&result["left_alone"]).is_empty(), "{asked}");
    assert_eq!(result["publish"]["state"], "needs_person", "{asked}");
    let link = result["publish"]["link"].as_str().unwrap().to_owned();
    // `next` is the call of a published read, with the input its trial
    // read through.
    let call = asked["next"]["args"].clone();
    assert_eq!(asked["next"]["action"], "tool.call", "{asked}");
    assert_eq!(call["tool"], READ, "{asked}");
    let order = call["input"]["order_id"].as_str().unwrap().to_owned();
    assert_eq!(prompt.0.load(Ordering::SeqCst), 0);

    // The person's press, on the page's own origin.
    let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let text = http.get(&link).send().await.unwrap().text().await.unwrap();
    assert!(
        text.contains("version.publish") && text.contains(READ),
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
    let said = pressed.text().await.unwrap();
    assert!(said.contains("was carried out as you, once"), "{said}");
    assert_eq!(prompt.0.load(Ordering::SeqCst), 1);

    // Asked again, `ready` says the version is published.
    let again = desk.ok(&["ready", "example", "--version", "1.0.0"]).await;
    assert_eq!(again["result"]["publish"]["state"], "published", "{again}");
    assert_eq!(again["result"]["publish"]["version"], "1.0.0", "{again}");
    assert_eq!(again["next"]["action"], "tool.call", "{again}");

    assert_eq!(again["next"]["args"], call, "{again}");

    // The published read, called as `next` gives it, returns what the
    // application holds.
    let held = app.stored_order(&order).await.unwrap().unwrap();
    let called = desk
        .ok(&["call", READ, "--input", &call["input"].to_string()])
        .await;
    assert_eq!(called["result"]["status"], "completed", "{called}");
    assert_eq!(
        called["result"]["result"]["customer"],
        held.customer.as_str()
    );
    assert_eq!(
        called["result"]["result"]["order_id"],
        held.order_id.as_str()
    );

    // Nothing in all of it wrote to the application.
    assert!(app.writes().await.unwrap().is_empty());
    let (code, _) = desk.run(&["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}
