//! Decision 0309, guard's third check, through the product's own runner:
//! the runner fills the job's `sessions` from the session keeper when it
//! claims the job, so a call whose session has ended stops at guard with
//! `needs_login`, before the policy engine is asked.

use a2m_actions::Response;
use a2m_core::FailureClass;
use a2m_ledger::{Durability, Owner, SqlValue};
use a2m_policy::Scope;
use serde_json::json;

use super::kernel::World;
use super::kernel::engine::SECURE_GET;

impl World {
    /// Keeps a session of the caller's, in the state named, for the example
    /// target's test environment: the row the session keeper would hold
    /// after a sign-in that ended.
    async fn keep_a_session(&self, state: &'static str) {
        let workspace = self.caller.principal.workspace();
        let person = self.caller.principal.principal().id;
        let now = self.now();
        self.kernel
            .store()
            .unwrap()
            .write(Durability::Synced, move |tx| {
                tx.tables(Owner::Auth)
                    .execute(
                        "INSERT INTO sessions
                            (id, workspace_id, target, environment, identity_ref,
                             owner_principal_id, provider, state, epoch, movable, state_since,
                             stored_attempts)
                         VALUES (?1, ?2, 'example', 'test', ?3, ?4, 'http_basic', ?5, 1, 1, ?6, 0)",
                        &[
                            SqlValue::from("s_01a1ec44a011702e856226a0336b125a"),
                            SqlValue::from(workspace.to_string()),
                            SqlValue::from(format!("person:{person}")),
                            SqlValue::from(person.to_string()),
                            SqlValue::from(state),
                            SqlValue::from(now),
                        ],
                    )
                    .map(|_| ())
            })
            .await
            .unwrap();
    }
}

/// The class and code the run ended with, as the response shows them.
fn failed_with(response: &Response) -> (FailureClass, String) {
    let shown = response
        .result
        .as_ref()
        .and_then(|result| result.get("failure"))
        .cloned()
        .or_else(|| {
            response
                .error
                .as_ref()
                .map(|error| json!({"class": error.class.to_string(), "code": error.code}))
        })
        .unwrap_or_else(|| panic!("no failure in {response:?}"));
    (
        shown["class"].as_str().unwrap().parse().unwrap(),
        shown["code"].as_str().unwrap().to_owned(),
    )
}

/// Policy here grants no tool, so a call that reaches the policy engine is
/// refused by it; the call that stops with `needs_login` never got there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_whose_session_has_ended_stops_at_guard_before_policy_is_asked() {
    let world = World::ready().await;
    world
        .publish(|layers| {
            for grant in &mut layers.grants {
                grant.scope = Scope::Pattern("example_orders_order_update".to_owned());
            }
        })
        .await;
    let read = json!({"tool": SECURE_GET, "input": {"order_id": "A-100"}});

    // With no session kept, the lease decides, and the call reaches the
    // policy engine, which refuses it.
    let asked = world.call("tool.call", read.clone()).await;
    let reached = failed_with(&asked);
    assert_eq!(reached.0, FailureClass::Policy, "{asked:?}");

    // With an ended session the call stops at guard, by name.
    world.keep_a_session("revoked").await;
    let stopped = world.call("tool.call", read).await;
    assert_eq!(
        failed_with(&stopped),
        (FailureClass::SessionExpired, "needs_login".to_owned()),
        "{stopped:?}"
    );
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

/// A session that is absent or has expired is left to the lease: a stored
/// credential may sign it in again, so guard does not stop the call, and it
/// goes on to the policy engine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_the_lease_may_sign_in_again_does_not_stop_the_call_at_guard() {
    for state in ["absent", "expired"] {
        let world = World::ready().await;
        world
            .publish(|layers| {
                for grant in &mut layers.grants {
                    grant.scope = Scope::Pattern("example_orders_order_update".to_owned());
                }
            })
            .await;
        world.keep_a_session(state).await;
        let read = json!({"tool": SECURE_GET, "input": {"order_id": "A-100"}});
        let reached = world.call("tool.call", read).await;
        assert_eq!(
            failed_with(&reached).0,
            FailureClass::Policy,
            "{state}: {reached:?}"
        );
        world.kernel.close(a2m_actions::CloseReason::Asked).await;
    }
}
