//! The discoverer `desktop_probe` against the stand (Linux, Windows, and
//! macOS; decision 0999): one `surface` finding with the program, the
//! digest of its file, its windows and controls, how many have no name,
//! and a support tier; the stand counts no press and no member that acts,
//! and no value read.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use common::Scene;
use serde_json::Value as Json;

#[tokio::test(flavor = "multi_thread")]
async fn the_probe_names_the_program_its_digest_and_a_tier_and_changes_nothing() {
    let Some(scene) = Scene::start("probe").await else {
        return;
    };
    let findings = scene.discover_with("desktop_probe").await.unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    let finding = &findings[0];
    assert_eq!(finding.kind().as_str(), "surface");
    let found: Json = serde_json::from_str(finding.found()).unwrap();
    assert_eq!(found["program"], Json::from(scene.program.as_str()));
    // The digest is of the file the stand's process runs, as the test
    // measures it itself.
    let digest = {
        use sha2::Digest as _;
        let bytes = std::fs::read(&scene.program).unwrap();
        let hex: String = sha2::Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("sha256:{hex}")
    };
    assert_eq!(found["digest"], Json::from(digest));
    assert!(found["version"].is_null());
    assert!(found["windows"].as_u64().unwrap() >= 1, "{found}");
    assert!(found["controls"].as_u64().unwrap() >= 1, "{found}");
    // The stand's window has a control with no name: it cannot be
    // located, so the tier is partial.
    assert!(found["unnamed_controls"].as_u64().unwrap() >= 1, "{found}");
    assert_eq!(found["tier"], Json::from("partial"));
    // Nothing of a value the window shows.
    assert!(!finding.found().contains(example_desk::SECRET));
    assert!(!finding.found().contains("Example Supplies"));
    let truth = scene.desk().truth();
    assert!(!truth.acted(), "{:?}", truth.asked);
    assert_eq!(truth.values_read(), 0, "{:?}", truth.asked);
    assert_eq!(truth.writes, 0);
}
