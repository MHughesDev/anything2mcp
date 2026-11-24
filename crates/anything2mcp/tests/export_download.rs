//! Package 9.3s: a declared download comes back as a reference, never
//! inline (spec section 502). The test application's export page offers a
//! file whose bytes hold a marker. An agent calls the tool whose screen
//! declares the download, a person approves it as a thing that cannot be
//! taken back, and the call returns the file's name; the marker is in no
//! field of the result, the run's row, its trace, or any file of the
//! engine's home or the project other than the downloaded file itself,
//! which lies in the profile's `downloads` folder under a name the browser
//! made. The application is asked what it sent, and it sent the file once.
//! It needs a browser, as the web pack's tests find one.
//!
//! The second test holds the other half of the row: the call returns the
//! file's name and size, the outputs the screen's download fills, which the
//! read-back does not read (no page holds them).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::{Path, PathBuf};

use a2m_actions::PresenceProof;
use a2m_core::RunId;
use a2m_policy::{Channel, PresenceCheck};
use common::{Machine, Ran, World};
use example_app::EXPORT_MARKER;
use example_app::harness::TestApp;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const TOOL: &str = "example_export_file";

/// The person approves, at the desk, the diff they were shown.
async fn approve(world: &World, run: &str) {
    let run_id: RunId = run.parse().unwrap();
    let asked = world.surface.last_ask();
    assert_eq!(asked.run, run_id);
    let person = world
        .engine
        .caller(a2m_actions::FrontDoor::Command, false)
        .principal
        .principal()
        .clone();
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person.id, world.clock.now());
    world
        .surface
        .desk()
        .approve(run_id, asked.diff_hash, person, Channel::LocalPrompt, proof)
        .await
        .unwrap();
}

/// Every file under `folder`.
fn files(folder: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, into);
        } else {
            into.push(path);
        }
    }
}

/// What one export leaves behind.
struct Exported {
    machine: Machine,
    world: World,
    app: TestApp,
    done: Ran,
    run: String,
}

/// The tool is published, called by an agent, approved by the person, and
/// resumed to its end.
async fn exported() -> Exported {
    let app = TestApp::start().await.unwrap();
    let machine = Machine::new("exports", &[(PLACEHOLDER, &app.url(""))]).on_disk();
    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let tested = world.ok(&["test", "run", "example"]).await;
    assert_eq!(tested["passed"], true, "{tested}");
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);

    let rested = world.agent(&["tool", "call", TOOL]).await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{:?}",
        rested.finished
    );
    assert!(app.case_events().await.unwrap().is_empty());
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(&world, &run).await;
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    assert_eq!(done.result()["status"], "completed", "{}", done.json);
    assert_eq!(
        app.case_events().await.unwrap(),
        ["export: marked file requested"]
    );
    Exported {
        machine,
        world,
        app,
        done,
        run,
    }
}

/// The files of the profile's downloads folder.
fn downloaded(machine: &Machine) -> Vec<PathBuf> {
    let mut everything = Vec::new();
    files(machine.home.path(), &mut everything);
    everything.retain(|path| {
        path.parent()
            .is_some_and(|folder| folder.ends_with("downloads"))
    });
    everything
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declared_download_lies_under_the_profile_and_its_bytes_are_in_no_result_trace_or_log() {
    let Exported {
        machine,
        world,
        app: _app,
        done,
        run,
    } = exported().await;

    // The file lies in the profile's downloads folder, whole, under a name
    // the browser made: not the name the page suggested.
    let kept = downloaded(&machine);
    assert_eq!(kept.len(), 1, "{kept:?}");
    let name = kept[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains("export") && !name.contains('.'), "{name}");
    let text = std::fs::read_to_string(&kept[0]).unwrap();
    assert_eq!(text, format!("order,quantity\nA-100,4\n{EXPORT_MARKER}\n"));

    // Its bytes are in no field of the result, the run's row, or its trace.
    let row = world.ok(&["run", "get", &run]).await;
    let trace = world.ok(&["trace", "show", &run]).await;
    for (what, shown) in [
        ("the result", done.json.to_string()),
        ("the row", row.to_string()),
        ("the trace", trace.to_string()),
    ] {
        assert!(!shown.contains(EXPORT_MARKER), "{what}: {shown}");
        assert!(!shown.contains("A-100,4"), "{what}: {shown}");
    }
    world.close().await;

    // No other file of the engine's home or the project, the logs and the
    // stored traces among them, holds the marker.
    let mut everything = Vec::new();
    files(machine.home.path(), &mut everything);
    files(machine.project.path(), &mut everything);
    let mut looked = 0;
    for path in &everything {
        if path == &kept[0] {
            continue;
        }
        let Ok(content) = std::fs::read(path) else {
            continue;
        };
        looked += 1;
        assert!(
            !content
                .windows(EXPORT_MARKER.len())
                .any(|window| window == EXPORT_MARKER.as_bytes()),
            "the marker is in {}",
            path.display()
        );
    }
    assert!(looked > 10, "only {looked} files were looked at");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declared_download_comes_back_as_its_name_and_size() {
    let Exported {
        machine,
        world,
        done,
        ..
    } = exported().await;
    let kept = downloaded(&machine);
    assert_eq!(kept.len(), 1, "{kept:?}");
    let name = kept[0].file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        done.result()["result"]["file"].as_str(),
        Some(name.as_str()),
        "{}",
        done.json
    );
    let size = std::fs::metadata(&kept[0]).unwrap().len();
    assert_eq!(
        done.result()["result"]["size"].as_u64(),
        Some(size),
        "{}",
        done.json
    );
    world.close().await;
}
