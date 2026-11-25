//! `version.rollback` (decision 0999): a person makes another published
//! version current, the store is read back before the answer says so, an
//! agent's call and a call nobody answers change nothing, and a store that
//! cannot commit leaves the version in force as it was.

use a2m_actions::{CloseReason, Response};
use a2m_core::Version;
use serde_json::{Value as Json, json};

use super::kernel::{Options, World, engine};

fn code_of(response: &Response) -> String {
    response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"))
        .code
        .clone()
}

impl World {
    /// Publishes version 2.0.0 of the `example` target beside 1.0.0.
    async fn second_version(&self) {
        let mut compiled_target = engine::compiled_target(&self.app.url(""));
        compiled_target.header.version = Version::new(2, 0, 0);
        self.kernel
            .publish_compiled_target(&compiled_target)
            .await
            .unwrap();
    }

    /// The version `target.show` gives for `example`: the one that runs.
    async fn running(&self) -> Json {
        self.ok("target.show", json!({"target": "example"})).await["version"].clone()
    }

    async fn rollback(&self, version: &str) -> Response {
        self.call_as(
            &self.person(),
            "version.rollback",
            json!({"target": "example", "version": version}),
        )
        .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_person_rolls_a_target_back_and_forward_and_the_audit_says_who() {
    let world = World::ready().await;
    world.second_version().await;
    assert_eq!(world.running().await, "2.0.0");

    // An agent cannot move the version that runs: a person's act.
    let by_agent = world
        .call(
            "version.rollback",
            json!({"target": "example", "version": "1.0.0"}),
        )
        .await;
    assert_eq!(code_of(&by_agent), "person_required", "{by_agent:?}");
    assert_eq!(world.running().await, "2.0.0");

    // A person who does not answer the presence prompt changes nothing.
    *world.surface.person_present.lock().unwrap() = false;
    let unanswered = world.rollback("1.0.0").await;
    assert_eq!(code_of(&unanswered), "presence_required", "{unanswered:?}");
    assert_eq!(world.running().await, "2.0.0");
    *world.surface.person_present.lock().unwrap() = true;

    // The person rolls back: the earlier version runs, and the answer names
    // the one it replaced.
    let back = world.rollback("1.0.0").await;
    assert!(back.ok, "{back:?}");
    let result = back.result.unwrap();
    assert_eq!(result["version"], "1.0.0");
    assert_eq!(result["previous"], "2.0.0");
    assert_eq!(result["changed"], true);
    assert_eq!(result["current"], true);
    assert_eq!(world.running().await, "1.0.0");
    let tools = world.ok("tool.search", json!({"target": "example"})).await;
    let versions: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["version"].as_str().unwrap())
        .collect();
    assert!(
        !versions.is_empty() && versions.iter().all(|v| *v == "1.0.0"),
        "{tools}"
    );

    // The same call again, as after a lost answer, changes nothing and
    // says so.
    let again = world.rollback("1.0.0").await;
    assert!(again.ok, "{again:?}");
    assert_eq!(again.result.as_ref().unwrap()["changed"], false);
    assert!(again.warnings.iter().any(|said| said.contains("already")));

    // Forward again to the later version, which a person also published.
    let forward = world.rollback("2.0.0").await;
    assert!(forward.ok, "{forward:?}");
    assert_eq!(forward.result.as_ref().unwrap()["previous"], "1.0.0");
    assert_eq!(world.running().await, "2.0.0");

    // The audit holds one entry for each move, by the person, and none for
    // the calls that changed nothing.
    let workspace = world.caller.principal.workspace();
    let entries = world
        .kernel
        .store()
        .unwrap()
        .read(move |tx| tx.audit().entries(&workspace, 0, 1000))
        .await
        .unwrap();
    let moves: Vec<_> = entries
        .iter()
        .filter(|entry| entry.action == "version.rollback")
        .collect();
    assert_eq!(moves.len(), 2, "{moves:?}");
    let person = world.caller.principal.principal().id.to_string();
    assert!(moves.iter().all(|entry| entry.actor == person));
    assert!(
        moves[0].detail.contains("\"version\":\"1.0.0\""),
        "{}",
        moves[0].detail
    );
    assert!(
        moves[0].detail.contains("\"previous\":\"2.0.0\""),
        "{}",
        moves[0].detail
    );

    // A version never published, and one that is not a version, are
    // refused before anything is changed.
    assert_eq!(code_of(&world.rollback("9.9.9").await), "not_found");
    assert_eq!(code_of(&world.rollback("latest").await), "schema_invalid");
    let unknown = world
        .call_as(
            &world.person(),
            "version.rollback",
            json!({"target": "unknown", "version": "1.0.0"}),
        )
        .await;
    assert_eq!(code_of(&unknown), "not_found");
    assert_eq!(world.running().await, "2.0.0");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rollback_the_store_cannot_commit_leaves_the_version_in_force() {
    let world = World::ready_with(Options::on_disk()).await;
    world.second_version().await;

    // Another program holds the database's write lock: the rollback's own
    // transaction cannot commit.
    let file = a2m_actions::Paths::under(world.folder.path())
        .store
        .join(a2m_ledger::DATABASE_FILE_NAME);
    let other = rusqlite::Connection::open(&file).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let refused = world.rollback("1.0.0").await;
    assert!(!refused.ok, "{refused:?}");
    drop(other);

    // Nothing moved, and nothing was written to the audit for it.
    assert_eq!(world.running().await, "2.0.0");
    let workspace = world.caller.principal.workspace();
    let entries = world
        .kernel
        .store()
        .unwrap()
        .read(move |tx| tx.audit().entries(&workspace, 0, 1000))
        .await
        .unwrap();
    assert!(
        entries
            .iter()
            .all(|entry| entry.action != "version.rollback")
    );
    // The person's call again, with the store free, does it.
    let back = world.rollback("1.0.0").await;
    assert!(back.ok, "{back:?}");
    assert_eq!(world.running().await, "1.0.0");
    world.kernel.close(CloseReason::Asked).await;
}
