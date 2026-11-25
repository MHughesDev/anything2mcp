//! The short path (decision 0306), from an empty folder to a value an agent
//! reads, through the command face only: `init`, `add` with the test
//! application's published description, and one trial of a read, called as
//! the report says to call it. No person, no publish.
//!
//! The time and the count of commands are printed, never asserted: the
//! target is three commands, and a budget is set from the first three
//! measurements.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::process::{Command, Output};
use std::time::Instant;

use anything2mcp::PASSPHRASE_VARIABLE;
use common::{Folder, IN_PROCESS_VARIABLE, PASSPHRASE, USER};
use example_app::harness::TestApp;
use serde_json::Value as Json;

/// Where the test application serves its description, and the file the
/// test keeps it in: the file a person has, in the project's `inputs`
const DESCRIPTION_PATH: &str = "/api/openapi.json";
const DESCRIPTION_FILE: &str = "inputs/example-openapi.json";

/// The read the report is asked to name a call for: one that takes an
/// order from the seed.
const READ: &str = "example_api_orders_get";

/// One developer's machine: the program is run in the project folder as an
/// agent runs it, with an engine inside each command.
struct Desk {
    home: Folder,
    project: Folder,
    commands: u32,
}

impl Desk {
    fn new() -> Self {
        let home = Folder::new("home");
        let installation = a2m_actions::Paths::under(home.path()).installation_config;
        std::fs::create_dir_all(&installation).unwrap();
        std::fs::write(
            installation.join("settings.toml"),
            "[vault]\nbackend = \"passphrase\"\n",
        )
        .unwrap();
        Self {
            home,
            project: Folder::new("short-project"),
            commands: 0,
        }
    }

    async fn run(&mut self, args: &[&str]) -> (Option<i32>, Json) {
        self.commands += 1;
        let mut command = Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
        command
            .arg("--json")
            .args(args)
            .current_dir(self.project.path())
            .env("A2M_HOME", self.home.path())
            .env("USER", USER)
            .env(IN_PROCESS_VARIABLE, "true")
            .env(PASSPHRASE_VARIABLE, PASSPHRASE)
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

    async fn ok(&mut self, args: &[&str]) -> Json {
        let (code, object) = self.run(args).await;
        assert_eq!(code, Some(0), "`{}`: {object}", args.join(" "));
        object
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_an_empty_folder_an_agent_reads_a_value_through_a_trial() {
    let app = TestApp::start().await.unwrap();
    let mut desk = Desk::new();
    let description = app
        .http()
        .get(app.url(DESCRIPTION_PATH))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(description.contains("\"openapi\""), "{description}");
    let began = Instant::now();

    // 1. `init`.
    desk.ok(&["init"]).await;
    // The description is the file a person has: it is put in the project's
    // `inputs` folder, by hand, as the person would.
    std::fs::create_dir_all(desk.project.path().join("inputs")).unwrap();
    std::fs::write(desk.project.path().join(DESCRIPTION_FILE), description).unwrap();

    // 2. `add`, handed the description. It ends at the report.
    let address = app.url("");
    let added = desk
        .ok(&[
            "add",
            "example",
            "--address",
            &address,
            "--kind",
            "http",
            "--description",
            DESCRIPTION_FILE,
        ])
        .await;
    assert_eq!(added["action"], "scout.report", "{added}");
    let content = &added["result"]["content"];
    assert_eq!(
        content["files"],
        serde_json::json!({"description": DESCRIPTION_FILE}),
        "{added}"
    );
    // Nothing waits for a sign-in, so none is asked for.
    assert_ne!(added["next"]["action"], "session.login", "{added}");

    // The report carries the call that reads each proven read.
    let proven = content["tried"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["tool"] == READ && tool["state"] == "proven")
        .unwrap_or_else(|| panic!("{READ} is not proven: {added}"));
    let call = &proven["call"];
    assert_eq!(call["action"], "draft.try", "{proven}");
    let input = &call["input"];
    assert_eq!(input["target"], "example", "{proven}");
    assert_eq!(input["tool"], READ, "{proven}");

    // 3. The call, copied from the report, reads a value.
    let given = input
        .get("input")
        .map(Json::to_string)
        .unwrap_or_else(|| "{}".to_owned());
    let tried = desk
        .ok(&[
            "draft",
            "try",
            input["target"].as_str().unwrap(),
            "--tool",
            input["tool"].as_str().unwrap(),
            "--input",
            &given,
        ])
        .await;
    let result = &tried["result"];
    assert_eq!(result["status"], "completed", "{tried}");
    assert!(result["result"].is_object(), "{tried}");
    assert_eq!(result["sandbox"], true, "{tried}");
    assert_eq!(result["published"], false, "{tried}");
    assert!(
        result["note"].as_str().unwrap().contains("`tool.call`"),
        "{tried}"
    );
    // Nothing was written to the system.
    assert!(app.writes().await.unwrap().is_empty());
    println!(
        "short path: {} commands, {:?} from an empty folder to a value read",
        desk.commands,
        began.elapsed()
    );
}
