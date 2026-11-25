//! The page a person publishes from, as an agent's publish brings it up
//! (decisions 0252 and 0264). One target of the imaginary `example` system
//! has a program surface, read through the command pack, and an HTTP
//! surface that takes a bearer token. The page shows the sign-in before
//! the tools, the program each tool starts by its path and digest, the
//! variables it is given, and where the token is sent, as the session
//! keeper states it; and never the value of a stored secret.
//!
//! The command pack has no secret variable yet (no credential reaches a
//! program), so the secret here is the stored token, and one of the
//! program's plain variables quotes it: the redactor, which every text of
//! the review passes, keeps it off the page. The engine is the same
//! `LocalEngine` that `serve` starts, run in this process, and the page is
//! fetched as a browser would. Nothing is pressed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{
    Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER, copy_changed, project_source,
};
use serde_json::Value as Json;

/// A made-up token, stored as the sign-in's credential.
const TOKEN: &str = "example-token-for-tests-publish-41c7";
/// The program the command target names, and the digest its file pins.
const PROGRAM: &str = "/opt/example/bin/example-cli";
const DIGEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";
/// The HTTP surface's address: where the token goes.
const API: &str = "http://127.0.0.1:4010";

/// A presence prompt that is never asked: nothing is pressed here.
struct Unasked;

impl PresencePrompt for Unasked {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async { Availability::ready(PromptName::Polkit, "The prompt of a test.") })
    }

    fn ask(
        &self,
        _text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async { PromptAnswer::Dismissed })
    }
}

/// One target with both surfaces: the inventory tool's program, with one
/// variable added whose value quotes the token, and the order system's
/// bearer-token interface with its sign-in.
fn project(folder: &Path) {
    let target = folder.join("targets").join("example");
    let inventory = project_source("inventory").join("targets").join("example");
    let secured = project_source("secured").join("targets").join("example");
    copy_changed(
        &inventory,
        &target,
        &[(
            "EXAMPLE_CLI_HOME: '/var/lib/example/home'}",
            &format!("EXAMPLE_CLI_HOME: '/var/lib/example/home', EXAMPLE_GREETING: '{TOKEN}'}}"),
        )],
    );
    for part in ["endpoints", "fixtures", "operations", "tests"] {
        copy_changed(&secured.join(part), &target.join(part), &[]);
    }
    std::fs::copy(secured.join("auth.yaml"), target.join("auth.yaml")).unwrap();
    std::fs::write(
        target.join("target.yaml"),
        format!(
            "schema_version: 1\ntarget: example\ntitle: Example inventory and orders\nsurfaces:\n  tool:\n    kind: program\n    path: '{PROGRAM}'\n  api:\n    kind: http\n    base: \"{API}\"\nenvironments:\n  test:\n    kind: test\n    test_records: [\"SK-1001\"]\ndefault_environment: test\nenvironment: {{locale: en-US, timezone: UTC}}\ndrift_risk: low\n"
        ),
    )
    .unwrap();
    std::fs::write(
        folder.join("anything2mcp.toml"),
        "# The project of the imaginary `example` system.\n",
    )
    .unwrap();
}

/// The real program, run in the project folder as an agent runs it.
async fn run(home: &Path, project: &Path, args: &[&str]) -> (Option<i32>, Json) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
    command
        .arg("--json")
        .args(args)
        .current_dir(project)
        .env_remove(PASSPHRASE_VARIABLE)
        .env("A2M_HOME", home)
        .env("USER", USER)
        .env(IN_PROCESS_VARIABLE, "true")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let done = tokio::task::spawn_blocking(move || command.output().unwrap())
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agents_publish_page_shows_what_each_tool_runs_and_where_its_sign_in_sends() {
    let home = Folder::new("home");
    let project_folder = Folder::new("publish-page");
    project(project_folder.path());
    let installation = Paths::under(home.path()).installation_config;
    std::fs::create_dir_all(&installation).unwrap();
    std::fs::write(
        installation.join("settings.toml"),
        "[vault]\nbackend = \"passphrase\"\n",
    )
    .unwrap();
    let mut options = EngineOptions::of_machine(
        Paths::under(home.path()),
        std::collections::BTreeMap::from([
            ("A2M_HOME".to_owned(), home.path().display().to_string()),
            ("USER".to_owned(), USER.to_owned()),
            (PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned()),
        ]),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(project_folder.path().to_path_buf());
    let local = LocalEngine::start(options, Arc::new(Unasked) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());
    let (home_path, project_path) = (home.path(), project_folder.path());

    // The token is stored, and so registered with the redactor.
    let (code, stored) = run(
        home_path,
        project_path,
        &["secret", "set", "example/service/token", "--value", TOKEN],
    )
    .await;
    assert_eq!(code, Some(0), "{stored}");

    // The agent asks to publish; a person's page waits.
    let (code, asked) = run(
        home_path,
        project_path,
        &["publish", "example", "--version", "1.0.0"],
    )
    .await;
    assert_eq!(code, Some(3), "{asked}");
    assert_eq!(asked["result"]["state"], "needs_person", "{asked}");
    let link = asked["result"]["link"].as_str().unwrap().to_owned();
    let http = a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap();
    let page = http.get(&link).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    let text = page.text().await.unwrap();

    // The program the command tool starts, by its path and its digest, and
    // the variable it is given that a person may read.
    assert!(
        text.contains(&format!(
            "Starts the program <code>{PROGRAM}</code>, pinned to the digest <code>{DIGEST}</code>."
        )),
        "{text}"
    );
    assert!(
        text.contains("<code>EXAMPLE_CLI_HOME</code>: set to <code>/var/lib/example/home</code>"),
        "{text}"
    );
    // The stored secret is nowhere on the page, though a variable quotes it.
    assert!(text.contains("<code>EXAMPLE_GREETING</code>"), "{text}");
    assert!(!text.contains(TOKEN), "{text}");

    // The sign-in comes before the tools, and names where the token goes,
    // as the session keeper states it.
    let at = |heading: &str| {
        text.find(heading)
            .unwrap_or_else(|| panic!("{heading}: {text}"))
    };
    let sign_ins = at("<h2>How it signs in</h2>");
    let tools = at("<h2>What each tool may do</h2>");
    assert!(sign_ins < tools, "{text}");
    let section = &text[sign_ins..tools];
    assert!(section.contains("<code>bearer_static</code>"), "{section}");
    assert!(
        section.contains("<code>vault://example/service/token</code>"),
        "{section}"
    );
    // Once in each environment: `test`, and `production`, which the
    // compiler implies for a target that declares none.
    let environments = section.matches("<li>In the environment ").count();
    assert_eq!(environments, 2, "{section}");
    assert_eq!(section.matches(API).count(), environments, "{section}");
    assert!(
        section.contains(&format!("<code>{API}</code>, the surface `api`")),
        "{section}"
    );
    // The HTTP tool sends to its surface only; the command tool does not
    // say where its program sends.
    assert!(
        text[tools..].contains(a2m_files::REACH_NOT_STATED),
        "{text}"
    );

    let (code, _) = run(home_path, project_path, &["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}
