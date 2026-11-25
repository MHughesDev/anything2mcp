//! The words a person reads on the first hour's path (decision 0485,
//! packages FU1 and FU2): the README's "Try it from source", walked through
//! the command face at a terminal, so that every answer comes in its text
//! form, as a person sees it.
//!
//! Each command is `run_with` with standard output a terminal, a thin
//! client of one served engine: the same `LocalEngine` that `serve` starts,
//! run in this process with a presence prompt that can never be raised, as
//! in a container. So the publish waits for its person, and nothing here
//! can carry it out: the path stops where the README says a run by hand
//! stops on most machines.
//!
//! What is held: no answer names a step as a dotted action; each `next`
//! on the path leads to `draft ready` or to the call; the first `ready`
//! says what is ready first and the step after its failure; `target
//! status` counts the scout's trials and `ready`'s tests and names the
//! publish that waits; `call` before the press says the publish waits;
//! `init` gives `add`; `doctor` is not `ok` with no presence prompt and
//! prints no empty `fix`; `engine status` and `engine stop` with no engine
//! start none and succeed; and a scout of an address that does not answer
//! says so in its first line, with the command to run again.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use a2m_actions::{Paths, PersonSurface};
use a2m_local::{EngineLock, NoPrompt, PresencePrompt, StopReason};
use anything2mcp::cli::{Finished, Host, run_with};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{Folder, PASSPHRASE, Surface, USER};
use example_app::harness::TestApp;

/// The read the README tries and calls.
const READ: &str = "example_api_orders_get";
/// The draft the stand fails every third call of, which `ready` leaves
/// alone.
const LEFT_ALONE: &str = "operations/api_stock_get.yaml";

/// A person's machine: the engine's home and an empty project folder.
struct Desk {
    home: Folder,
    project: Folder,
    /// Every action with a command face, by name: what no answer may name
    /// as a step.
    actions: BTreeSet<String>,
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
        let registry = anything2mcp::action_registry().unwrap();
        let actions = a2m_actions::command_faces(&registry)
            .into_iter()
            .map(|face| face.action)
            .collect();
        Self {
            home,
            project: Folder::new("first-project"),
            actions,
        }
    }

    fn environment(&self, passphrase: bool) -> BTreeMap<String, String> {
        let mut environment = BTreeMap::from([
            (
                "A2M_HOME".to_owned(),
                self.home.path().display().to_string(),
            ),
            ("USER".to_owned(), USER.to_owned()),
        ]);
        if passphrase {
            environment.insert(PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned());
        }
        environment
    }

    /// One command at a terminal. It starts no engine: it asks the one
    /// served, or, with `passphrase`, runs one inside itself.
    async fn at_terminal(&self, args: &[&str], passphrase: bool) -> Finished {
        let host = Host {
            cwd: self.project.path().to_path_buf(),
            environment: self.environment(passphrase),
            session_folder: None,
            stdout_is_terminal: true,
            person_at_terminal: false,
            person: None,
            key_store: None,
            program: None,
            prompt: None,
        };
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let finished = run_with(&args, &host).await;
        self.names_no_action(args.join(" "), &finished);
        finished
    }

    /// The same, asking the engine that is served.
    async fn run(&self, args: &[&str]) -> Finished {
        self.at_terminal(args, false).await
    }

    /// No span between backticks, and no line, of what a person reads
    /// names an action as `noun.verb`: each step is named as its command
    /// (decision 0485, point 1).
    fn names_no_action(&self, command: String, finished: &Finished) {
        for text in [&finished.stdout, &finished.stderr] {
            for (at, span) in text.split('`').enumerate() {
                assert!(
                    at % 2 == 0 || !self.actions.contains(span),
                    "`{command}` names the action `{span}`:\n{text}"
                );
            }
            for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || "._".contains(c))) {
                assert!(
                    !self.actions.contains(word.trim_end_matches('.')) || !word.contains('.'),
                    "`{command}` names the action `{word}`:\n{text}"
                );
            }
        }
    }
}

/// The step a text answer gives last: the command under its `next` line.
fn next_command(finished: &Finished) -> String {
    let both = format!("{}{}", finished.stdout, finished.stderr);
    let at = both
        .rfind("\nnext, ")
        .unwrap_or_else(|| panic!("no next: {finished:?}"));
    both[at..]
        .lines()
        .nth(2)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// Whether a store's database file is anywhere under `folder`.
fn holds_a_store(folder: &std::path::Path) -> bool {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                holds_a_store(&path)
            } else {
                path.file_name().and_then(|name| name.to_str()) == Some("store.sqlite3")
            }
        })
}

/// An address on this machine at which nothing answers: a port that was
/// free a moment ago.
fn silent_address() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}/")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_hours_answers_name_commands_and_lead_the_short_way() {
    let app = TestApp::start().await.unwrap();
    let desk = Desk::new();

    // `init` gives `add`, with the flags it needs (point 4).
    let made = desk.run(&["init"]).await;
    assert_eq!(made.code, 0, "{made:?}");
    assert!(
        made.stdout
            .contains("\n  anything2mcp add <name> --address <address> --kind <kind>\n"),
        "{made:?}"
    );

    // The engine, as `serve` starts it, on a machine with no presence
    // prompt.
    let mut options = EngineOptions::of_machine(
        Paths::under(desk.home.path()),
        desk.environment(true),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(desk.project.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::new(NoPrompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());

    // `add`: the scout's report, which leads to `ready`. The questions the
    // comparison of the description with the observation opens are counted
    // apart, and stand after the way on is clear.
    let address = app.url("");
    let added = desk
        .run(&["add", "example", "--address", &address, "--kind", "http"])
        .await;
    assert_eq!(added.code, 0, "{added:?}");
    assert!(added.stdout.starts_with("Scout of `example`"), "{added:?}");
    assert!(
        added
            .stdout
            .contains("`anything2mcp scout report example --part drafted.written`"),
        "{added:?}"
    );
    assert!(added.stdout.contains("hold back no draft"), "{added:?}");
    assert_eq!(next_command(&added), "anything2mcp draft ready example");

    let tried = desk
        .run(&[
            "try",
            "example",
            "--tool",
            READ,
            "--input",
            r#"{"order_id": "A-100"}"#,
        ])
        .await;
    assert_eq!(tried.code, 0, "{tried:?}");

    // `status` after the scout: the trials are counted, and the way on is
    // `ready`.
    let status = desk.run(&["status", "example"]).await;
    assert_eq!(status.code, 0, "{status:?}");
    assert!(
        status
            .stdout
            .contains("tried:\n  by_state:\n    failed: 1\n    proven: 7\n"),
        "{status:?}"
    );
    assert_eq!(next_command(&status), "anything2mcp draft ready example");

    // The first `ready`: what is ready first, then the failure, then the
    // removal as the step (point 3). The exit code stays 1.
    let first = desk.run(&["ready", "example"]).await;
    assert_eq!(first.code, 1, "{first:?}");
    assert!(first.stdout.starts_with("Ready: `"), "{first:?}");
    assert!(
        first
            .stdout
            .contains("Left alone: `example_api_stock_get`."),
        "{first:?}"
    );
    assert!(!first.stdout.contains("\nnext, "), "{first:?}");
    let error_at = first.stderr.find("code: publish_gates_not_passed").unwrap();
    let next_at = first.stderr.find("\nnext, ").unwrap();
    assert!(error_at < next_at, "{first:?}");
    assert!(
        first.stderr.contains("No publish was asked for yet"),
        "{first:?}"
    );
    assert_eq!(
        next_command(&first),
        format!("anything2mcp draft delete example --files {LEFT_ALONE}")
    );

    // After the removal, and after the gates by hand, the way on is
    // `ready` again (point 2).
    let removed = desk
        .run(&["draft", "delete", "example", "--files", LEFT_ALONE])
        .await;
    assert_eq!(removed.code, 0, "{removed:?}");
    assert_eq!(next_command(&removed), "anything2mcp draft ready example");
    let tested = desk.run(&["test", "example"]).await;
    assert_eq!(tested.code, 0, "{tested:?}");
    assert_eq!(next_command(&tested), "anything2mcp draft ready example");

    // The second `ready` asks for the publish, which waits for its person.
    let asked = desk.run(&["ready", "example", "--version", "1.0.0"]).await;
    assert_eq!(asked.code, 3, "{asked:?}");
    assert!(asked.stdout.starts_with("Succeeded: "), "{asked:?}");
    assert!(
        next_command(&asked).starts_with(&format!("anything2mcp tool call {READ} --input ")),
        "{asked:?}"
    );

    // `status` names the publish that waits, with its page, and counts the
    // tests `ready` recorded; the way on is the same `ready`.
    let waiting = desk.run(&["status", "example"]).await;
    assert_eq!(waiting.code, 0, "{waiting:?}");
    assert!(waiting.stdout.contains("\nwaiting:\n"), "{waiting:?}");
    assert!(waiting.stdout.contains("/act/"), "{waiting:?}");
    assert!(waiting.stdout.contains("offline_tests: 7"), "{waiting:?}");
    assert!(
        waiting.stdout.contains("unknown:\n  - signed_in\n"),
        "{waiting:?}"
    );
    assert_eq!(
        next_command(&waiting),
        "anything2mcp draft ready example --version 1.0.0"
    );

    // `call` before the press: no tool yet, and the publish that waits.
    let called = desk
        .run(&["call", READ, "--input", r#"{"order_id": "A-100"}"#])
        .await;
    assert_eq!(called.code, 1, "{called:?}");
    assert!(called.stderr.contains("code: not_found"), "{called:?}");
    assert!(
        called
            .stderr
            .contains("a publish of `example` waits for its person"),
        "{called:?}"
    );

    // `doctor` with no presence prompt is not `ok`, and prints no empty
    // `fix` (point 5).
    let doctor = desk.run(&["doctor"]).await;
    assert_eq!(doctor.code, 0, "{doctor:?}");
    assert!(
        doctor.stdout.contains("\nstate: component_missing\n"),
        "{doctor:?}"
    );
    assert!(
        !doctor.stdout.lines().any(|line| line.trim() == "fix:"),
        "{doctor:?}"
    );

    let stopped = desk.run(&["engine", "stop"]).await;
    assert_eq!(stopped.code, 0, "{stopped:?}");
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_no_engine_status_and_stop_start_none_and_say_so() {
    let desk = Desk::new();
    let made = desk.run(&["init"]).await;
    assert_eq!(made.code, 0, "{made:?}");
    let paths = Paths::under(desk.home.path());
    for _ in 0..2 {
        let status = desk.at_terminal(&["engine", "status"], true).await;
        assert_eq!(status.code, 0, "{status:?}");
        assert!(status.stdout.starts_with("No engine runs"), "{status:?}");
        assert!(!status.stdout.contains("in_process"), "{status:?}");
        let stop = desk.at_terminal(&["engine", "stop"], true).await;
        assert_eq!(stop.code, 0, "{stop:?}");
        assert!(stop.stdout.contains("nothing to stop"), "{stop:?}");
        // Nothing was started: no engine holds the lock, and no store was
        // made.
        assert!(!EngineLock::held(&paths).unwrap());
        assert!(!holds_a_store(desk.home.path()), "a store was made");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scout_of_an_address_that_does_not_answer_says_so_first() {
    let desk = Desk::new();
    let made = desk.run(&["init"]).await;
    assert_eq!(made.code, 0, "{made:?}");
    let address = silent_address();
    let added = desk
        .at_terminal(
            &["add", "example", "--address", &address, "--kind", "http"],
            true,
        )
        .await;
    assert_eq!(added.code, 0, "{added:?}");
    let first = added.stdout.lines().next().unwrap_or_default();
    assert!(
        first.starts_with("The system did not answer")
            && first.contains("`anything2mcp scout run example --fresh`"),
        "{added:?}"
    );
    assert_eq!(
        next_command(&added),
        "anything2mcp scout run example --fresh"
    );
}
