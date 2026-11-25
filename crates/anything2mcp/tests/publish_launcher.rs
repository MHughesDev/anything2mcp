//! The page a person publishes from, for a server started through a
//! launcher (package L4, decision 0398). One target of the imaginary
//! `example` system has an MCP surface started by the JavaScript runtime,
//! given the installed entry file of the MCP test stand. The page shows the
//! launcher, the entry file's path and digest, and the sentence that says
//! what nothing pins; and never calls the entry file a script the target
//! keeps. The engine is the same `LocalEngine` that `serve` starts, run in
//! this process, and the page is fetched as a browser would.

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
use example_mcp::harness::StdioStand;
use serde_json::Value as Json;

/// A text as a YAML scalar in single quotes, in which nothing is an escape.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

/// The JavaScript runtime, found on the search path, as the file it leads
/// to, or a copy of it in a folder of the test's own where the folder it
/// lies in is not closed to other users. Elsewhere than CI the test is
/// skipped when there is none.
fn launcher() -> Option<(std::path::PathBuf, Option<Folder>)> {
    let file = format!("node{}", std::env::consts::EXE_SUFFIX);
    let found = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|folder| folder.join(&file))
            .find(|path| path.is_file())
            .and_then(|path| std::fs::canonicalize(path).ok())
    });
    let Some(found) = found else {
        assert!(
            std::env::var_os("CI").is_none(),
            "no `{file}` on the search path of this CI machine"
        );
        println!("skipped: no `{file}` on the search path of this machine");
        return None;
    };
    let trust = a2m_provider::ProgramTrust::default();
    if a2m_provider::check_program(&found, &trust, a2m_provider::ProgramRole::Interpreter).is_ok() {
        return Some((found, None));
    }
    let folder = Folder::new("node");
    let copy = folder.path().join(&file);
    std::fs::copy(&found, &copy).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    Some((copy, Some(folder)))
}

fn digest_of(path: &Path) -> String {
    a2m_core::Digest::of(&std::fs::read(path).unwrap()).to_string()
}

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

/// The upstream project with its surface started through `launcher`, given
/// `entry` as the installed entry file and the stand's program as its first
/// argument.
fn project(folder: &Path, launcher: &Path, entry: &Path, stand: &StdioStand) {
    let target = folder.join("targets").join("example");
    let upstream = project_source("upstream").join("targets").join("example");
    copy_changed(
        &upstream,
        &target,
        &[(
            "address: \"http://127.0.0.1:9/mcp\"",
            &format!(
                "address: stdio\n    program: {}\n    digest: \"sha256:{}\"\n    environment: {{EXAMPLE_MCP_STATE: {}}}\n    entry: {}\n    entry_digest: \"sha256:{}\"\n    args: [{}]",
                quoted(launcher),
                digest_of(launcher),
                quoted(stand.state_folder()),
                quoted(entry),
                digest_of(entry),
                quoted(stand.program()),
            ),
        )],
    );
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
async fn the_publish_page_shows_a_launched_server_as_a_launcher_and_says_what_nothing_pins() {
    let Some((node, _node_folder)) = launcher() else {
        return;
    };
    let stand = StdioStand::new().unwrap();
    let entry = stand.launcher_entry().unwrap();
    let home = Folder::new("home");
    let project_folder = Folder::new("publish-launcher");
    project(project_folder.path(), &node, &entry, &stand);
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

    // The launcher, the entry file's path and digest, and the sentence.
    assert!(
        text.contains(&format!(
            "Starts the launcher <code>{}</code>",
            node.display()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "on the entry file <code>{}</code>, pinned to the digest <code>{}</code>.",
            entry.display(),
            digest_of(&entry)
        )),
        "{text}"
    );
    assert!(
        text.contains("whatever the entry file loads, and whatever that starts, is not pinned"),
        "{text}"
    );
    // It is not worded as a script the target keeps.
    assert!(!text.contains("a script this target keeps"), "{text}");
    assert!(!text.contains("Starts the interpreter"), "{text}");

    let (code, _) = run(home_path, project_path, &["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}
