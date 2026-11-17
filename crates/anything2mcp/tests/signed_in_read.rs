//! Milestone 2, the signed-in read: a surface that needs a credential. The
//! secret is stored with `secret set`, placed by the session keeper, and
//! appears in no output, log, or trace, although the system itself echoes
//! it in its answer.

mod common;

use common::service::Service;
use common::{Machine, World};

const PLACEHOLDER: &str = "http://127.0.0.1:4011";
/// A made-up token.
const TOKEN: &str = "example-token-for-tests-7f3a9c";
const TOOL: &str = "example_account_get";

async fn published(machine: &Machine) -> World {
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world
}

/// Every file under a folder, as text; a file that is not text is read
/// lossily, which keeps any text inside it.
fn texts_under(folder: &std::path::Path, out: &mut Vec<(std::path::PathBuf, String)>) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in listing {
        let path = entry.unwrap().path();
        if path.is_dir() {
            texts_under(&path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            out.push((path, String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_that_needs_a_credential_is_signed_in_and_the_secret_is_in_nothing() {
    let service = Service::start(TOKEN).await;
    // The store is on disk, so that what the engine wrote can be searched.
    let machine = Machine::new("account", &[(PLACEHOLDER, service.url())]).on_disk();
    let world = published(&machine).await;

    // Without the credential the run rests, waiting for a sign-in, and the
    // service was never sent a request without one.
    let before = world.agent(&["tool", "call", TOOL]).await;
    assert_ne!(before.finished.code, 0, "{:?}", before.finished);
    assert!(
        service.seen().iter().all(|seen| seen.ends_with("signed_in=true")),
        "{:?}",
        service.seen()
    );

    // The secret is stored. Storing it gives nothing back.
    let stored = world
        .agent(&["secret", "set", "example/service/token", "--value", TOKEN])
        .await;
    assert_eq!(stored.finished.code, 0, "{:?}", stored.finished);
    assert!(!stored.finished.stdout.contains(TOKEN));
    assert!(!stored.finished.stderr.contains(TOKEN));

    // The read is signed in, and the value the system echoed is masked.
    let ran = world.agent(&["tool", "call", TOOL]).await;
    assert_eq!(ran.finished.code, 0, "{:?}", ran.finished);
    assert_eq!(ran.result()["status"], "completed");
    assert_eq!(ran.result()["result"]["name"], "service");
    assert_eq!(ran.result()["result"]["echo"], a2m_core::REDACTED);
    assert!(
        service
            .seen()
            .contains(&"GET /account HTTP/1.1 signed_in=true".to_owned()),
        "{:?}",
        service.seen()
    );

    // The secret is in no output of any command about the run.
    let run = ran.result()["run_id"].as_str().unwrap().to_owned();
    let mut outputs = vec![ran.finished.clone()];
    for args in [
        vec!["run", "get", run.as_str()],
        vec!["trace", "show", run.as_str()],
        vec!["session", "status"],
        vec!["audit", "query"],
        vec!["plan", "show", "example"],
        vec!["engine", "status"],
        vec!["doctor", "run"],
        vec!["config", "show"],
    ] {
        outputs.push(world.agent(&args).await.finished);
    }
    for output in &outputs {
        assert!(!output.stdout.contains(TOKEN), "{output:?}");
        assert!(!output.stderr.contains(TOKEN), "{output:?}");
    }
    world.close().await;

    // Nor in anything the engine left on disk: the store, the evidence,
    // the trace, and the logs.
    let mut files = Vec::new();
    texts_under(machine.home.path(), &mut files);
    texts_under(machine.project.path(), &mut files);
    assert!(!files.is_empty());
    for (path, text) in &files {
        assert!(!text.contains(TOKEN), "{}", path.display());
    }
}
