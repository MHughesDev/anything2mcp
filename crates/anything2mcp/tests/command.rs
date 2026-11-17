//! The command line through the built binaries: both names are the same
//! program, the command tree is the action registry and nothing else, the
//! output contract holds for every command, and the exit code follows the
//! failure.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::collections::BTreeSet;
use std::process::{Command, Output, Stdio};

use a2m_actions::{PROCESS_COMMANDS, SHORTCUTS, command_faces};
use common::{Folder, Machine};
use serde_json::Value as Json;

/// Both names of the command.
const COMMANDS: [&str; 2] = [
    env!("CARGO_BIN_EXE_anything2mcp"),
    env!("CARGO_BIN_EXE_a2m"),
];

/// A machine with nothing on it: a home directory and an empty project.
///
/// Its installation chooses the passphrase route for the vault, so that no
/// test reads or writes the key store of the machine the tests run on, and
/// the engine behaves the same on every operating system. With no
/// passphrase given the engine is degraded: it answers what needs no
/// secret and writes nothing.
struct Bare {
    home: Folder,
    project: Folder,
    passphrase: bool,
}

impl Bare {
    fn new() -> Self {
        let bare = Self {
            home: Folder::new("bare-home"),
            project: Folder::new("bare-project"),
            passphrase: false,
        };
        std::fs::write(bare.project.path().join("anything2mcp.toml"), "").unwrap();
        bare.installation("");
        bare
    }

    /// The same machine, started with the vault's passphrase.
    fn with_passphrase() -> Self {
        let mut bare = Self::new();
        bare.passphrase = true;
        bare
    }

    /// Writes the installation's settings file: the vault's route, and
    /// `more`.
    fn installation(&self, more: &str) {
        let folder = self.home.path().join("installation");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("settings.toml"),
            format!("[vault]\nbackend = \"passphrase\"\n{more}"),
        )
        .unwrap();
    }

    fn command(&self, program: &str, args: &[&str]) -> Command {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(self.project.path())
            .env("A2M_HOME", self.home.path())
            .env_remove(anything2mcp::PASSPHRASE_VARIABLE)
            // The engine runs inside each command: none is started and
            // left running by a test of the command line.
            .env(common::IN_PROCESS_VARIABLE, "true")
            .stdin(Stdio::null());
        if self.passphrase {
            command.env(anything2mcp::PASSPHRASE_VARIABLE, common::PASSPHRASE);
        }
        command
    }

    fn run_as(&self, program: &str, args: &[&str]) -> Output {
        self.command(program, args).output().unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_as(COMMANDS[0], args)
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Standard output as the one JSON object of the output contract.
fn object(output: &Output) -> Json {
    let stdout = text(&output.stdout);
    assert_eq!(stdout.trim_end().lines().count(), 1, "{stdout}");
    let json: Json = serde_json::from_str(stdout.trim_end()).unwrap();
    assert!(json.is_object(), "{stdout}");
    json
}

#[test]
fn the_version_is_printed_and_no_arguments_gives_help() {
    let bare = Bare::new();
    let version = format!("anything2mcp {}", env!("CARGO_PKG_VERSION"));
    for program in COMMANDS {
        for flag in ["--version", "-V"] {
            let output = bare.run_as(program, &[flag]);
            assert!(output.status.success(), "{flag} failed");
            assert_eq!(text(&output.stdout).trim_end(), version);
        }
        let output = bare.run_as(program, &[]);
        assert!(output.status.success());
        let help = text(&output.stdout);
        assert_eq!(help.lines().next(), Some(version.as_str()));
        assert!(help.contains("anything2mcp <noun> <verb>"), "{help}");
    }
}

#[test]
fn both_names_are_the_same_program() {
    let bare = Bare::new();
    for args in [
        vec![],
        vec!["--version"],
        vec!["--help"],
        vec!["tool", "--help"],
        vec!["tool", "call", "--help"],
        vec!["lint", "--help"],
        vec!["nothing"],
        vec!["tool", "cal"],
        vec!["tool", "call"],
        vec!["runner"],
        vec!["serve", "--help"],
        vec!["bridge", "--toolset", "admin"],
        vec!["init"],
        vec!["docs", "get", "approvals"],
    ] {
        let long = bare.run_as(COMMANDS[0], &args);
        let short = bare.run_as(COMMANDS[1], &args);
        assert_eq!(long.status.code(), short.status.code(), "{args:?}");
        assert_eq!(text(&long.stderr), text(&short.stderr), "{args:?}");
        // A dispatched action carries the identifier of its recorded
        // decision, which differs from call to call; nothing else may.
        if args == ["docs", "get", "approvals"] {
            let (mut long, mut short) = (object(&long), object(&short));
            for json in [&mut long, &mut short] {
                json.as_object_mut().unwrap().remove("decision");
            }
            assert_eq!(long, short);
        } else {
            assert_eq!(text(&long.stdout), text(&short.stdout), "{args:?}");
        }
        // The output names the product, whatever name started it.
        assert!(!text(&short.stdout).contains("a2m "), "{args:?}");
    }
}

/// The nouns and verbs the program's own help lists.
fn listed_commands(bare: &Bare) -> (BTreeSet<(String, String)>, BTreeSet<String>) {
    let help = text(&bare.run(&["--help"]).stdout);
    let mut commands = BTreeSet::new();
    let mut words = BTreeSet::new();
    let mut section = "";
    for line in help.lines() {
        if line.starts_with("Commands, by noun") {
            section = "nouns";
        } else if line.starts_with("Shortcuts") {
            section = "shortcuts";
        } else if line.starts_with("Process commands") {
            section = "process";
        } else if line.starts_with("Global flags") {
            section = "";
        } else if let Some(rest) = line.strip_prefix("  ") {
            match section {
                "nouns" => {
                    let (noun, verbs) = rest.split_once(' ').unwrap();
                    for verb in verbs.split(',') {
                        commands.insert((noun.to_owned(), verb.trim().to_owned()));
                    }
                }
                "shortcuts" => {
                    words.insert(rest.split_whitespace().next().unwrap().to_owned());
                }
                "process" => {
                    words.extend(rest.split(',').map(|word| word.trim().to_owned()));
                }
                _ => {}
            }
        }
    }
    (commands, words)
}

#[test]
fn the_command_tree_is_the_action_registry_and_nothing_else() {
    let bare = Bare::new();
    let registry = anything2mcp::action_registry().unwrap();
    let faces = command_faces(&registry);
    assert!(faces.len() > 80, "{}", faces.len());

    // Every action with a command face is a command, and no command exists
    // that the registry does not describe.
    let described: BTreeSet<(String, String)> = faces
        .iter()
        .map(|face| (face.noun.clone(), face.verb.clone()))
        .collect();
    let (commands, words) = listed_commands(&bare);
    assert_eq!(commands, described);
    let allowed: BTreeSet<String> = SHORTCUTS
        .iter()
        .map(|shortcut| shortcut.word.to_owned())
        .chain(PROCESS_COMMANDS.iter().map(|word| (*word).to_owned()))
        .collect();
    assert_eq!(words, allowed);

    // The help of every command is built from its description: the usage
    // line, the summary, and every argument.
    for face in &faces {
        let output = bare.run(&[&face.noun, &face.verb, "--help"]);
        assert!(output.status.success(), "{}", face.action);
        let help = text(&output.stdout);
        assert!(help.starts_with(&face.usage), "{}: {help}", face.action);
        assert!(help.contains(&face.help), "{}", face.action);
        for arg in &face.args {
            let shown = if arg.positional {
                format!("<{}>", arg.field)
            } else {
                arg.flag.clone()
            };
            assert!(help.contains(&shown), "{}: {shown}", face.action);
            // No argument of an action is taken for a global flag.
            for global in [
                "--json",
                "--project",
                "--in-process",
                "--idempotency-key",
                "--help",
            ] {
                assert_ne!(arg.flag, global, "{}", face.action);
            }
        }
    }
    assert_eq!(
        anything2mcp::cli::usage_lines().unwrap(),
        faces
            .iter()
            .map(|face| face.usage.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn output_that_is_not_a_terminal_is_one_json_object_for_every_action() {
    // Every command of the tree, with no argument at all: one that needs
    // an argument is refused by the tree, and the others are dispatched,
    // built or not. Each prints exactly one JSON object and ends with a
    // code of the output contract.
    let bare = Bare::new();
    let registry = anything2mcp::action_registry().unwrap();
    let mut codes = BTreeSet::new();
    for face in command_faces(&registry) {
        let output = bare.run(&[&face.noun, &face.verb]);
        let json = object(&output);
        let code = output.status.code().unwrap();
        // `engine start`, which this machine cannot carry out with no
        // passphrase for its vault, ends as an engine not started.
        let highest = if face.action == "engine.start" { 4 } else { 3 };
        assert!((0..=highest).contains(&code), "{}: {code}", face.action);
        assert_eq!(json["ok"] == true, code == 0, "{}: {json}", face.action);
        if code == 0 {
            assert!(json["result"].is_object(), "{}: {json}", face.action);
            assert!(text(&output.stderr).is_empty(), "{}", face.action);
        } else {
            // A failure carries its class, code, message, and next step,
            // in the object and, for a person, on standard error.
            let error = &json["error"];
            for part in ["class", "code", "message", "fix"] {
                assert!(error[part].is_string(), "{}: {json}", face.action);
            }
            let stderr = text(&output.stderr);
            assert!(
                stderr.contains(error["message"].as_str().unwrap()),
                "{}: {stderr}",
                face.action
            );
            assert!(stderr.contains(error["code"].as_str().unwrap()));
        }
        codes.insert(code);
    }
    assert!(
        codes.contains(&0) && codes.contains(&1) && codes.contains(&2),
        "{codes:?}"
    );
}

#[test]
fn the_exit_code_follows_the_failure() {
    let bare = Bare::new();
    // 0: the action did what was asked.
    let status = bare.run(&["engine", "status"]);
    assert_eq!(status.status.code(), Some(0));
    assert_eq!(object(&status)["action"], "engine.status");

    // 1: the action failed. The input was well formed and named nothing.
    let missing = bare.run(&["provider", "describe", "nothing_here"]);
    assert_eq!(missing.status.code(), Some(1));
    let error = &object(&missing)["error"];
    assert_eq!(
        (error["class"].as_str(), error["code"].as_str()),
        (Some("input"), Some("not_found"))
    );

    // 2: malformed, by the tree (an unknown word, a missing argument, an
    // unknown flag, a value of the wrong type) or by the dispatcher (a
    // word outside a closed list).
    for args in [
        vec!["nothing"],
        vec!["tool", "describe"],
        vec!["tool", "search", "--nope"],
        vec![
            "tool",
            "call",
            "example_order_get",
            "--wait-seconds",
            "soon",
        ],
        vec!["tool", "call", "example_order_get", "--input", "not json"],
        vec!["killswitch", "set", "--scope", "somewhere"],
    ] {
        let output = bare.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(
            object(&output)["error"]["code"],
            "schema_invalid",
            "{args:?}"
        );
    }

    // A machine whose key store does not answer is degraded: it reads,
    // and refuses everything that writes, with the way out.
    let degraded = bare.run(&["target", "add", "example"]);
    assert_eq!(degraded.status.code(), Some(1));
    assert_eq!(object(&degraded)["error"]["code"], "engine_degraded");
    let doctor = text(&bare.run(&["doctor", "run"]).stdout);
    assert!(
        doctor.contains(anything2mcp::PASSPHRASE_VARIABLE),
        "{doctor}"
    );

    // 3: a person is needed. Nobody is at a command whose input and output
    // are not a terminal, so a person's act is refused, and the program
    // never waits for an answer.
    let bare = Bare::with_passphrase();
    let publish = bare.run(&["version", "publish", "example"]);
    assert_eq!(publish.status.code(), Some(3));
    assert_eq!(object(&publish)["error"]["code"], "person_required");

    // The process command of a server's role is named and not built; a
    // process command given a word it does not take is malformed, and the
    // bridge says so on standard error alone, because its standard output
    // carries protocol messages and nothing else. A settings file that
    // does not load stops the command with the file named.
    let runner = bare.run(&["runner"]);
    assert_eq!(runner.status.code(), Some(1));
    assert_eq!(object(&runner)["error"]["code"], "action_not_built");
    let serve = bare.run(&["serve", "now"]);
    assert_eq!(serve.status.code(), Some(2));
    assert_eq!(object(&serve)["error"]["code"], "schema_invalid");
    let bridge = bare.run(&["bridge", "--toolset", "admin"]);
    assert_eq!(bridge.status.code(), Some(2));
    assert!(bridge.stdout.is_empty(), "{bridge:?}");
    assert!(text(&bridge.stderr).contains("admin"), "{bridge:?}");
    std::fs::write(
        bare.project.path().join("anything2mcp.toml"),
        "[tools]\nno_such_key = 1\n",
    )
    .unwrap();
    let broken = bare.run(&["engine", "status"]);
    assert_eq!(broken.status.code(), Some(1));
    let error = &object(&broken)["error"];
    assert_eq!(error["code"], "settings_invalid", "{error}");
    assert!(
        error["message"].as_str().unwrap().contains("no_such_key"),
        "{error}"
    );
}

#[test]
fn a_pack_that_is_switched_off_is_listed_and_registers_nothing() {
    let names = [
        "command",
        "data",
        "device",
        "file",
        "http",
        "mcp",
        "stream",
        "web",
        "web_probe",
    ];
    // The web pack's two providers need a browser on the machine, so only
    // the others are held to `ready` here.
    let needs_a_program = ["web", "web_probe"];
    for off in names {
        let bare = Bare::new();
        bare.installation(&format!("[providers]\ndisabled = [\"{off}\"]\n"));
        let listed = object(&bare.run(&["provider", "list"]));
        let providers = listed["result"]["providers"].as_array().unwrap().clone();
        let found: Vec<&str> = providers
            .iter()
            .map(|provider| provider["name"].as_str().unwrap())
            .collect();
        assert_eq!(found, names, "every first-party pack is listed");
        for provider in &providers {
            let name = provider["name"].as_str().unwrap();
            if name == off {
                assert_eq!(provider["registered"], false, "{provider}");
                assert_eq!(provider["availability"], "not_enabled", "{provider}");
                assert!(
                    provider["fix"]
                        .as_str()
                        .unwrap()
                        .contains("provider enable"),
                    "{provider}"
                );
            } else if !needs_a_program.contains(&name) {
                assert_eq!(provider["registered"], true, "{provider}");
                assert_eq!(provider["availability"], "ready", "{provider}");
            }
            // A pack that reads a kind whose owner is switched off stays,
            // with the problem named and the way out: never a crash.
            if off == "http" && name == "web_probe" {
                let problem = &provider["problems"][0];
                assert_eq!(problem["code"], "kind_owner_missing", "{provider}");
                assert!(problem["fix"].as_str().unwrap().contains("`http`"));
            } else {
                assert_eq!(provider["problems"], serde_json::json!([]), "{provider}");
            }
        }
        // The pack is still described, from its manifest.
        let described = object(&bare.run(&["provider", "describe", off]));
        assert_eq!(described["result"]["name"], off, "{described}");
    }
}

#[test]
fn a_new_project_gets_a_target_and_a_linted_draft_with_the_shortcuts_alone() {
    let bare = Bare::with_passphrase();
    // `init` has no action beneath it yet, and says what to do instead.
    let init = bare.run(&["init"]);
    assert_eq!(init.status.code(), Some(1));
    assert!(
        object(&init)["error"]["fix"]
            .as_str()
            .unwrap()
            .contains("anything2mcp.toml")
    );

    // `add` registers the target, then stops at the first of its actions
    // that is not built, and says which.
    let add = bare.run(&["add", "example"]);
    let added = object(&add);
    assert_eq!(add.status.code(), Some(1), "{added}");
    assert_eq!(added["action"], "target.probe");
    assert_eq!(added["error"]["code"], "action_not_built");
    // The step it stopped before was never begun, and the note says what
    // was done and what was not.
    let note = added["error"]["message"].as_str().unwrap();
    assert!(note.contains("stopped before `target.probe`"), "{note}");
    assert!(note.contains("Done: `target.add`"), "{note}");
    assert!(note.contains("`session.login`"), "{note}");
    let file = bare
        .project
        .path()
        .join("targets")
        .join("example")
        .join("target.yaml");
    assert!(file.is_file());

    // `lint` checks the draft that `add` wrote.
    let lint = bare.run(&["lint", "example"]);
    let linted = object(&lint);
    assert_eq!(lint.status.code(), Some(0), "{linted}");
    assert_eq!(linted["action"], "draft.lint");
    assert_eq!(linted["result"]["target"], "example");

    // The same command from a folder inside the project finds the project;
    // from outside any project it is refused, and `--project` names one.
    let inside = bare
        .command(COMMANDS[0], &["lint", "example"])
        .current_dir(bare.project.path().join("targets").join("example"))
        .output()
        .unwrap();
    assert_eq!(object(&inside)["result"], linted["result"]);
    let outside = Folder::new("outside");
    let run_outside = |args: &[&str]| {
        bare.command(COMMANDS[0], args)
            .current_dir(outside.path())
            .output()
            .unwrap()
    };
    let refused = run_outside(&["lint", "example"]);
    assert_eq!(object(&refused)["error"]["code"], "path_not_allowed");
    let project = bare.project.path().display().to_string();
    let named = run_outside(&["lint", "example", "--project", &project]);
    assert_eq!(object(&named)["result"], linted["result"]);

    // `doctor` checks the machine and prints one object.
    let doctor = bare.run(&["doctor"]);
    assert_eq!(object(&doctor)["action"], "doctor.run");
}

/// Runs the program with `input` on its standard input, and waits for it.
fn run_with_stdin(mut command: Command, input: &'static [u8]) -> Output {
    use std::io::Write;
    let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
    // The pipe is closed when the handle is dropped: the program reads to
    // the end of its input.
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_secret_typed_on_standard_input_is_stored_and_is_in_no_output() {
    use example_app::harness::{BEARER_TOKEN, TestApp};

    let app = TestApp::start().await.unwrap();
    let machine =
        Machine::new("secured", &[("http://127.0.0.1:4010", &app.url(""))]).with_passphrase();
    // A person publishes the target; the engine is then closed, so that
    // each command below is the real program with an engine of its own.
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;

    let run = |args: &'static [&'static str], input: &'static [u8]| {
        let command = machine.program(args);
        tokio::task::spawn_blocking(move || run_with_stdin(command, input))
    };
    let mut outputs = Vec::new();

    // Both ways at once are refused, by the names of the two arguments.
    // What was given is in neither output.
    let both = run(
        &[
            "secret",
            "set",
            "example/service/token",
            "--stdin",
            "--value",
            "example-value-given-as-an-argument",
        ],
        b"example-value-given-on-standard-input\n",
    )
    .await
    .unwrap();
    assert_eq!(both.status.code(), Some(2), "{both:?}");
    let refused = object(&both);
    assert_eq!(refused["error"]["code"], "schema_invalid");
    let message = refused["error"]["message"].as_str().unwrap();
    assert!(message.contains("--stdin") && message.contains("--value"));
    for given in [
        "example-value-given-as-an-argument",
        "example-value-given-on-standard-input",
    ] {
        assert!(!text(&both.stdout).contains(given), "{both:?}");
        assert!(!text(&both.stderr).contains(given), "{both:?}");
    }

    // The secret, typed on standard input with a line ending after it.
    let token: &'static [u8] = Box::leak(
        format!("{BEARER_TOKEN}\r\n")
            .into_bytes()
            .into_boxed_slice(),
    );
    let stored = run(
        &["secret", "set", "example/service/token", "--stdin"],
        token,
    )
    .await
    .unwrap();
    assert_eq!(stored.status.code(), Some(0), "{stored:?}");
    let answer = object(&stored);
    assert_eq!(answer["action"], "secret.set");
    assert_eq!(answer["result"]["stored"], true);
    outputs.push(stored);

    // The test application takes the token the vault now holds: the read
    // is signed in, and the application refused no token. So what was
    // stored is the text that was typed, without its line ending.
    let read = run(
        &[
            "tool",
            "call",
            "example_order_get",
            "--input",
            r#"{"order_id": "A-100"}"#,
        ],
        b"",
    )
    .await
    .unwrap();
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let result = object(&read)["result"].clone();
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(result["result"]["customer"], "Example Customer One");
    assert_eq!(app.auth_report().await.unwrap().refused_tokens, 0);
    outputs.push(read);

    for output in &outputs {
        assert!(!text(&output.stdout).contains(BEARER_TOKEN), "{output:?}");
        assert!(!text(&output.stderr).contains(BEARER_TOKEN), "{output:?}");
    }
}
