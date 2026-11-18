//! The trust roots an installation names (decision 0166): read when the
//! engine opens, through the folders `files.allowed_roots` allows, and
//! trusted by every HTTP client of the product. Each command is a process
//! of its own with its own trust.
//!
//! `network.trust_roots` is a protected setting, and only an installation
//! file that no account but an administrator can change may name a root
//! (decision 0241). The test's installation folder is in a temporary folder
//! its own account owns, so the real program, rightly, takes no root from
//! it. Each command is therefore run in a process of this test binary's
//! own, which opens the engine as the program does, inside itself, with
//! the installation's file taken as an administrator's
//! (`EngineOptions::with_installation_kept_by_an_administrator_for_tests`),
//! and runs the one command. The shipped program has no such way.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use a2m_actions::{Paths, PersonSurface};
use anything2mcp::{Engine, EngineOptions};
use common::{Machine, Surface};
use serde_json::{Value as Json, json};

/// The variable that makes this binary's `one_command_in_a_process_of_its_own`
/// run a command: what to run, as JSON. Read by this test file alone.
const CHILD: &str = "A2M_TRUST_ROOTS_TEST_CHILD";

const PLACEHOLDER: &str = "http://127.0.0.1:4010";

/// What one command gave: its exit code and what it printed.
#[derive(Debug)]
struct Output {
    code: i64,
    printed: Json,
}

fn object(output: &Output) -> Json {
    output.printed.clone()
}

/// Writes the installation's settings file: the vault's passphrase route,
/// and the trust roots named.
fn name_roots(machine: &Machine, roots: &[&std::path::Path]) {
    let listed: Vec<String> = roots
        .iter()
        .map(|path| format!("{:?}", path.display().to_string()))
        .collect();
    let installation = Paths::under(machine.home.path()).installation_config;
    std::fs::write(
        installation.join("settings.toml"),
        format!(
            "[vault]\nbackend = \"passphrase\"\n\n[network]\ntrust_roots = [{}]\n",
            listed.join(", ")
        ),
    )
    .unwrap();
}

/// Gives the machine's home to another account where the test runs as the
/// superuser, whose folders would otherwise be the administrator's: as a
/// person's own home is. Elsewhere the test's account owns it already.
fn persons_own(machine: &Machine) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let home = machine.home.path();
        if std::fs::metadata(home).unwrap().uid() == 0 {
            std::os::unix::fs::chown(home, Some(65_534), Some(65_534)).unwrap();
        }
    }
    #[cfg(not(unix))]
    let _ = machine;
}

/// Runs one command in a process of this test binary's own, which opens
/// the engine on the machine's folders and runs it.
async fn run(machine: &Machine, args: &[&str]) -> Output {
    let out = machine.home.path().join("child-output.json");
    let _ = std::fs::remove_file(&out);
    let spec = json!({
        "home": machine.home.path(),
        "project": machine.project.path(),
        "environment": machine.environment(),
        "args": args,
        "out": out,
    });
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "one_command_in_a_process_of_its_own",
            "--test-threads=1",
        ])
        .env(CHILD, spec.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let ran = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    assert!(ran.status.success(), "the child process failed: {ran:?}");
    let written: Json = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    Output {
        code: written["code"].as_i64().unwrap(),
        printed: written["printed"].clone(),
    }
}

/// The other end of [`run`]: with the variable set, opens the engine and
/// runs the command; without it, which is every ordinary run of the
/// tests, does nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_command_in_a_process_of_its_own() {
    let Ok(spec) = std::env::var(CHILD) else {
        return;
    };
    let spec: Json = serde_json::from_str(&spec).unwrap();
    let path = |key: &str| PathBuf::from(spec[key].as_str().unwrap());
    let environment: BTreeMap<String, String> =
        serde_json::from_value(spec["environment"].clone()).unwrap();
    let args: Vec<String> = serde_json::from_value(spec["args"].clone()).unwrap();
    let mut options = EngineOptions::of_machine(
        Paths::under(&path("home")),
        environment,
        Surface::new() as std::sync::Arc<dyn PersonSurface>,
    )
    .with_installation_kept_by_an_administrator_for_tests();
    options.project = Some(path("project"));
    let written = match Engine::open(options).await {
        // As the program prints a failure to open the engine.
        Err(failure) => json!({
            "code": 1,
            "printed": {"error": {
                "code": failure.code().to_string(),
                "message": failure.message(),
            }},
        }),
        Ok(engine) => {
            let finished = anything2mcp::cli::run_on(&engine, &args, true, true).await;
            engine.close().await;
            json!({
                "code": finished.code,
                "printed": serde_json::from_str::<Json>(finished.stdout.trim())
                    .unwrap_or_else(|_| panic!("not one JSON object: {finished:?}")),
            })
        }
    };
    std::fs::write(path("out"), written.to_string()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trust_root_outside_the_allowed_folders_stops_the_engine_by_name() {
    let machine = Machine::new("orders", &[(PLACEHOLDER, PLACEHOLDER)]).with_passphrase();
    let outside = common::Folder::new("outside-roots");
    let elsewhere = outside.path().join("authority.pem");
    std::fs::write(&elsewhere, "made-up certificate text").unwrap();
    name_roots(&machine, &[&elsewhere]);

    // The real program takes no root from an installation file the
    // person's own account can change: the loosening is refused by name.
    persons_own(&machine);
    let mut program = machine.program(&["--json", "engine", "status"]);
    let real = tokio::task::spawn_blocking(move || program.output().unwrap())
        .await
        .unwrap();
    assert_eq!(real.status.code(), Some(1), "{real:?}");
    let printed: Json = serde_json::from_slice(&real.stdout).unwrap();
    assert_eq!(printed["error"]["code"], "setting_protected", "{printed}");
    assert!(
        printed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("network.trust_roots"),
        "{printed}"
    );

    let refused = run(&machine, &["engine", "status"]).await;
    assert_eq!(refused.code, 1, "{refused:?}");
    let error = &object(&refused)["error"];
    assert_eq!(error["code"], "trust_roots_unreadable", "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("files.allowed_roots"),
        "{error}"
    );

    // Inside the project folder, which `<project>` allows, a file with no
    // certificate is refused for that; a build without TLS refuses any.
    let inside = machine.project.path().join("authority.pem");
    std::fs::write(&inside, "made-up certificate text").unwrap();
    name_roots(&machine, &[&inside]);
    let refused = run(&machine, &["engine", "status"]).await;
    assert_eq!(refused.code, 1, "{refused:?}");
    let expected = if cfg!(feature = "tls") {
        "trust_roots_unreadable"
    } else {
        "tls_not_enabled"
    };
    assert_eq!(object(&refused)["error"]["code"], expected, "{refused:?}");
}

#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_over_https_is_trusted_through_the_authority_the_installation_names() {
    use example_app::harness::TestApp;
    use example_app::tls::{ServerCertificate, TestAuthority};

    let authority = TestAuthority::new("example test authority").unwrap();
    let app = TestApp::start_tls(authority.server(ServerCertificate::Good).unwrap())
        .await
        .unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.secure_url(""))]).with_passphrase();
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;
    let read = [
        "tool",
        "call",
        "example_order_get",
        "--input",
        r#"{"order_id": "A-100"}"#,
    ];

    // Not named, the authority is nobody's: the certificate is refused by
    // name, and nothing was read.
    let untrusted = run(&machine, &read).await;
    let response = object(&untrusted);
    // On macOS the system's verifier refuses an unknown authority without
    // saying which check failed; everywhere else it names it.
    let untrusted_codes: &[&str] = if cfg!(target_vendor = "apple") {
        &["certificate_untrusted", "certificate_not_verified"]
    } else {
        &["certificate_untrusted"]
    };
    assert!(
        untrusted_codes
            .iter()
            .any(|code| response["error"]["code"] == *code),
        "{response}"
    );

    // Named, inside the project folder, the same read is made over TLS.
    let pem = machine.project.path().join("authority.pem");
    std::fs::write(&pem, authority.pem()).unwrap();
    name_roots(&machine, &[&pem]);
    // Another order, so that the call is not the stopped run of the first.
    let other = [
        "tool",
        "call",
        "example_order_get",
        "--input",
        r#"{"order_id": "A-101"}"#,
    ];
    let trusted = run(&machine, &other).await;
    assert_eq!(trusted.code, 0, "{trusted:?}");
    let response = object(&trusted);
    assert_eq!(response["result"]["status"], "completed", "{response}");
    assert_eq!(
        response["result"]["result"]["customer"],
        "Example Customer Two"
    );
}
