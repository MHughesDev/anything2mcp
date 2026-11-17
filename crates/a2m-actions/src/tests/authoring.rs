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
