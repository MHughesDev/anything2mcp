//! The engine's home is its user's alone (decision 0241): an engine opened
//! on a fresh home leaves every entry under its own folders out of other
//! accounts' reach, and `doctor.run` names a folder others can enter, with
//! the fix.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_actions::Paths;
use common::Machine;
use serde_json::Value as Json;

/// The doctor's check of the home.
async fn home_check(machine: &Machine) -> Json {
    let world = machine.open().await;
    let doctor = world.agent(&["doctor", "run"]).await;
    world.close().await;
    doctor.result()["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["check"] == "home")
        .cloned()
        .unwrap_or_else(|| panic!("no check of the home: {}", doctor.json))
}

/// Every entry under `folder` lies inside a folder, at or below `folder`,
/// that no group and no other account may enter.
#[cfg(unix)]
fn every_entry_is_closed(folder: &std::path::Path) {
    use std::os::unix::fs::MetadataExt as _;
    let closed =
        |path: &std::path::Path| std::fs::symlink_metadata(path).unwrap().mode() & 0o077 == 0;
    assert!(closed(folder), "{} is open", folder.display());
    let mut stack = vec![folder.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            let reachable = path
                .ancestors()
                .skip(1)
                .take_while(|above| above.starts_with(folder))
                .all(|above| !closed(above));
            assert!(!reachable, "{} is open to others", path.display());
            if std::fs::symlink_metadata(&path).unwrap().is_dir() {
                stack.push(path);
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fresh_home_is_closed_to_other_accounts_and_the_doctor_names_one_that_is_not() {
    let machine = Machine::new("orders", &[]).on_disk();
    let check = home_check(&machine).await;
    let paths = Paths::under(machine.home.path());
    if cfg!(unix) {
        assert_eq!(check["state"], "healthy", "{check}");
    } else {
        assert_eq!(check["state"], "unknown", "{check}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for folder in [&paths.data, &paths.config, &paths.cache] {
            every_entry_is_closed(folder);
        }
        // A folder opened by hand is named, with the fix; the next start
        // closes it again.
        std::fs::set_permissions(&paths.data, std::fs::Permissions::from_mode(0o755)).unwrap();
        let world = machine.open().await;
        world.close().await;
        every_entry_is_closed(&paths.data);
    }
    let _ = paths;
}

/// The doctor's own reading, of a folder made open after the engine
/// started: the engine closes its folders at start, so the folder is
/// opened while the engine runs.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_doctor_names_a_folder_of_the_engines_that_others_can_enter() {
    use std::os::unix::fs::PermissionsExt as _;
    let machine = Machine::new("orders", &[]).on_disk();
    let paths = Paths::under(machine.home.path());
    let world = machine.open().await;
    std::fs::set_permissions(&paths.data, std::fs::Permissions::from_mode(0o755)).unwrap();
    let doctor = world.agent(&["doctor", "run"]).await;
    world.close().await;
    let check = doctor.result()["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["check"] == "home")
        .cloned()
        .unwrap();
    assert_eq!(check["state"], "config_drift", "{check}");
    assert!(
        check["detail"]
            .as_str()
            .unwrap()
            .contains(&paths.data.display().to_string()),
        "{check}"
    );
    assert!(
        check["fix"].as_str().unwrap().contains("chmod 700"),
        "{check}"
    );
}
