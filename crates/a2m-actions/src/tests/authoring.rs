//! The authoring actions that need a running engine and the test
//! application: a discovery through the engine's sandbox.

use serde_json::json;

use super::kernel::World;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_discovery_runs_in_the_sandbox_stores_its_findings_and_writes_nothing() {
    let world = World::ready().await;
    let before = world.app.writes().await.unwrap().len();

    let found = world.ok("discover.run", json!({"target": "example"})).await;
    let surfaces = found["surfaces"].as_array().unwrap();
    assert!(!surfaces.is_empty(), "{found}");
    // Every discovery ends: one surface is observed, and the one that
    // needs a session its caller does not have ends as failed, with why.
    assert_eq!(found["complete"], true, "{found}");
    assert!(
        surfaces
            .iter()
            .any(|surface| surface["status"] == "completed"),
        "{found}"
    );
    for surface in surfaces {
        let status = surface["status"].as_str().unwrap();
        assert!(
            status == "completed" || (status == "failed" && surface["failure"].is_object()),
            "{found}"
        );
    }
    let observed = surfaces
        .iter()
        .find(|surface| surface["status"] == "completed")
        .unwrap();
    assert!(found["findings"].as_u64().unwrap() >= 1, "{found}");
    assert_eq!(found["by_kind"]["call"], 1, "{found}");

    // The discoverer's planted write never reached the system: the test
    // application stored nothing.
    assert_eq!(world.app.writes().await.unwrap().len(), before);
    assert_eq!(world.app.write_count("A-100", "L-900").await.unwrap(), 0);

    // What was found is stored, and read back in full.
    let listed = world.ok("finding.list", json!({"target": "example"})).await;
    assert_eq!(listed["total"], 1, "{listed}");
    let id = listed["findings"][0]["id"].as_str().unwrap().to_owned();
    let described = world
        .ok(
            "finding.describe",
            json!({"finding": id, "target": "example"}),
        )
        .await;
    assert!(
        described["finding"].to_string().contains("/api/orders"),
        "{described}"
    );
    let unknowns = world.ok("unknown.list", json!({"target": "example"})).await;
    assert!(unknowns["unknowns"].is_array());

    // A discovery's run is the caller's, and is read like any run.
    let run = observed["run_id"].as_str().unwrap();
    let got = world.ok("run.get", json!({"run_id": run})).await;
    assert_eq!(got["status"], "completed", "{got}");

    // A surface the target does not declare is not a surface.
    let missing = world
        .call(
            "discover.run",
            json!({"target": "example", "surface": "nowhere"}),
        )
        .await;
    assert_eq!(missing.error.unwrap().code, "not_found");
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

/// The identify stage, the sign-in as claims, and a scout of a published
/// target with no project folder: every stage that touches the system runs
/// in the sandbox, the stages that draft say why they did not, and two
/// scouts of the same system in the same state give one report, by hash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scout_identifies_reports_the_same_twice_and_writes_nothing() {
    let world = World::ready().await;
    let before = world.app.writes().await.unwrap().len();

    // Identify: what the registry says of each surface, and what its
    // discovery met. A surface with no discoverer is said to have none.
    let probed = world.ok("target.probe", json!({"target": "example"})).await;
    assert_eq!(probed["complete"], true, "{probed}");
    let surfaces = probed["surfaces"].as_array().unwrap();
    assert!(!surfaces.is_empty(), "{probed}");
    let identified = surfaces
        .iter()
        .find(|surface| surface["state"] == "identified")
        .expect("one surface answers");
    assert_eq!(identified["provider"], "example_json", "{probed}");
    assert_eq!(identified["discoverers"][0]["discoverer"], "look");
    assert_eq!(
        identified["findings_by_kind"]["call"]["discovered"], 1,
        "{probed}"
    );
    assert_eq!(identified["ended"][0]["findings"], 1, "{probed}");
    for surface in surfaces {
        let state = surface["state"].as_str().unwrap();
        assert!(
            ["identified", "needs_login", "refused", "failed"].contains(&state),
            "{probed}"
        );
        assert!(
            surface.get("run_id").is_none(),
            "run identifiers are kept apart"
        );
    }

    // The sign-in, as claims for a person: nothing is recommended that
    // the target's files do not declare, and no credential is shown.
    let auth = world.ok("auth.detect", json!({"target": "example"})).await;
    for surface in auth["surfaces"].as_array().unwrap() {
        assert_eq!(surface["level"], "unknown", "{auth}");
        assert!(surface["identity_mode"].is_null(), "{auth}");
        assert!(
            ["answered", "needs_sign_in", "refused", "failed"]
                .contains(&surface["observed"].as_str().unwrap()),
            "{auth}"
        );
    }

    // The scout: no project holds the target, so nothing is drafted, and
    // the report says so.
    let first = world.ok("scout.run", json!({"target": "example"})).await;
    let hash = first["report_hash"].as_str().unwrap().to_owned();
    assert!(
        !first["content"]["found"].as_array().unwrap().is_empty(),
        "{first}"
    );
    let found = &first["content"]["found"][0];
    assert_eq!(found["kind"], "call");
    assert_eq!(found["level"], "discovered");
    // What the system said is kept apart and marked.
    assert!(found["found"]["untrusted"].is_string(), "{found}");
    assert_eq!(first["content"]["drafted"]["state"], "not_looked_at");
    let skipped = first["content"]["not_looked_at"].to_string();
    assert!(skipped.contains("stage `draft`"), "{skipped}");
    assert!(
        first["text"]
            .as_str()
            .unwrap()
            .contains("Scout of `example`")
    );
    assert!(!first["text"].as_str().unwrap().contains("/api/orders"));

    // The report is read later, and a second scout gives the same one.
    let read = world.ok("scout.report", json!({"target": "example"})).await;
    assert_eq!(read["report_hash"], hash.as_str());
    let second = world.ok("scout.run", json!({"target": "example"})).await;
    assert_ne!(second["scout_id"], first["scout_id"]);
    assert_eq!(second["report_hash"], hash.as_str(), "{first}\n{second}");

    // Nothing was written to the system by any of it.
    assert_eq!(world.app.writes().await.unwrap().len(), before);
    assert_eq!(world.app.write_count("A-100", "L-900").await.unwrap(), 0);

    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

/// Audit finding A5: what a scout's compare stage copied into the engine's
/// cache, left there by an engine that stopped before it removed it, is
/// removed when the next engine opens.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scouts_copy_left_in_the_cache_is_removed_when_the_next_engine_opens() {
    let world = super::kernel::world(super::kernel::Options::before_open(|paths| {
        let left = paths.cache.join("scout").join("r_left_by_a_crash");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::write(left.join("target.yaml"), "copied").unwrap();
    }))
    .await;
    assert!(!world.kernel.paths().cache.join("scout").exists());
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}
