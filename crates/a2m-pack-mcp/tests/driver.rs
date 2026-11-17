//! The driver against the HTTP form of the test stand. Every write is
//! proven by asking the stand what it stored and how often.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{FailureClass, PackPayload, Plan, Step, Value, Values};
use a2m_pack_mcp::{UpstreamMcp, codes};
use a2m_provider::{
    Authority, DriverHandle, DriverHealth, Observed, Posture, Prepared, Registry, Sent,
    StepFailure, StepOutcome, SurfaceRef,
};
use common::{
    Folder, Sessions, compiled_fixture, fence_of, input, key, prepare, record, registry,
    registry_with, running, steps, store, surface, token,
};
use example_mcp::faults::{Drift, Fault, FaultKind, Malformed};
use example_mcp::harness::HttpStand;
use example_mcp::store::INSTRUCTION_TEXT;

struct Scene {
    _folder: Folder,
    registry: Registry,
    plan: Plan,
    stand: HttpStand,
    surface: SurfaceRef,
}

async fn scene_with(registry_of: impl FnOnce(&Folder) -> Registry) -> Scene {
    let folder = Folder::new("driver");
    let registry = registry_of(&folder);
    let plan = compiled_fixture(&registry);
    let stand = HttpStand::start().await.unwrap();
    let surface = surface(&plan, "orders", Some(&stand.url()));
    Scene {
        _folder: folder,
        registry,
        plan,
        stand,
        surface,
    }
}

async fn scene() -> Scene {
    scene_with(registry).await
}

impl Scene {
    async fn prepared(&self) -> (DriverHandle, Prepared) {
        prepare(
            &self.registry,
            &self.plan,
            &self.surface,
            fence_of(7),
            Posture::Live,
            None,
        )
        .await
        .unwrap()
    }

    /// Carries out one reading step.
    async fn read(&self, step: &Step, args: Values) -> Result<StepOutcome, StepFailure> {
        let (handle, mut prepared) = self.prepared().await;
        let result = handle
            .act(&mut prepared, step, &input(&args, None, None, None), Authority::Read)
            .await;
        handle.release(prepared, result.is_err()).await;
        result
    }

    async fn get(&self, order: &str) -> Result<StepOutcome, StepFailure> {
        let step = steps(&self.plan, "example_order_get").act;
        self.read(&step, Values::new().with("order_id", order)).await
    }

    fn calls_to(&self, tool: &str) -> usize {
        self.stand.control().calls_to(tool).unwrap()
    }
}

fn done(outcome: Result<StepOutcome, StepFailure>) -> Observed {
    match outcome {
        Ok(StepOutcome::Done(observed)) => observed,
        other => panic!("the step was not done: {other:?}"),
    }
}

fn failed(outcome: Result<StepOutcome, StepFailure>) -> (FailureClass, String, Sent) {
    let failure = outcome.expect_err("the step must fail");
    (
        failure.failure.class(),
        failure.failure.code().as_str().to_owned(),
        failure.sent,
    )
}

fn code(class: FailureClass, code: &a2m_core::FailureCode, sent: Sent) -> (FailureClass, String, Sent) {
    (class, code.as_str().to_owned(), sent)
}

/// Everything a step gave a caller, as text, to look for what must not be
/// there.
fn everything(observed: &Observed) -> String {
    let captures: String = observed
        .captures
        .iter()
        .map(|capture| String::from_utf8_lossy(&capture.bytes).into_owned())
        .collect();
    format!("{:?} {captures}", observed.values)
}

#[tokio::test]
async fn a_read_returns_what_the_stand_holds_and_five_runs_observe_the_same() {
    let scene = scene().await;
    let first = done(scene.get("A-100").await);
    assert_eq!(first.values.get("status"), Some(&Value::from("draft")));
    assert_eq!(
        first.values.get("note"),
        Some(&Value::from("Deliver to dock 2"))
    );
    assert_eq!(first.captures.len(), 1);
    for _ in 0..4 {
        assert_eq!(done(scene.get("A-100").await), first);
    }
    let control = scene.stand.control();
    assert_eq!(control.calls_to("orders_get").unwrap(), 5);
    // One connection served every run of the one identity.
    assert_eq!(control.connections().unwrap().len(), 1);

    // An order that is not there is absent, not a failure.
    assert!(matches!(
        scene.get("A-999").await,
        Ok(StepOutcome::Absent(_))
    ));

    // A slow tool with no arguments, and a list read as a list.
    let report = steps(&scene.plan, "example_order_report").act;
    let counted = done(scene.read(&report, Values::new()).await);
    assert_eq!(counted.values.get("draft"), Some(&Value::Int(2)));
}

#[tokio::test]
async fn a_write_is_sent_once_with_the_ledgers_token_and_read_back_from_the_stand() {
    let scene = scene().await;
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["A-100"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.plan,
        &scene.surface,
        fence,
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let update = steps(&scene.plan, "example_order_note_update");
    let args = Values::new()
        .with("order_id", "A-100")
        .with("field", "note")
        .with("value", "  Leave at the gate  ");
    let order = key("A-100");

    // Without the ledger's token the call never leaves.
    let refused = handle
        .act(
            &mut prepared,
            &update.act,
            &input(&args, Some(record(&fence, 0)), Some(&order), None),
            Authority::Read,
        )
        .await
        .unwrap_err();
    assert_eq!(refused.sent, Sent::NotSent);
    assert_eq!(scene.calls_to("orders_update"), 0);

    let intent = token(&ledger, fence, 0, "A-100").await;
    let written = handle
        .act(
            &mut prepared,
            &update.act,
            &input(&args, Some(record(&fence, 0)), Some(&order), None),
            Authority::Write(&intent),
        )
        .await;
    done(written);

    // The stand, not the driver, says what was stored and how often.
    let control = scene.stand.control();
    assert_eq!(control.write_count("A-100").unwrap(), 1);
    assert_eq!(scene.calls_to("orders_update"), 1);
    let stored = control.stored_order("A-100").unwrap().unwrap();
    assert_eq!(stored.note, "Leave at the gate");

    // The read-back is the mapped read tool, called with the record's key.
    // The server stored the text trimmed: equal after the field type's
    // normalization, which trims; a different value is not.
    let none = Values::new();
    let read = done(
        handle
            .act(
                &mut prepared,
                &update.read_back.unwrap(),
                &input(&none, Some(record(&fence, 0)), Some(&order), None),
                Authority::Read,
            )
            .await,
    );
    let read_note = read.values.get("note").and_then(Value::as_text).unwrap();
    assert_eq!(read_note, stored.note);
    assert_eq!(read_note, "  Leave at the gate  ".trim());
    assert_ne!(read_note, "Leave at the door".trim());
    assert!(handle.release(prepared, false).await.complete);

    // The tool's own refusal is `rejected`, and nothing is stored.
    let other = running(&ledger, 2, &["A-101"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.plan,
        &scene.surface,
        other,
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let closed = key("A-101");
    let intent = token(&ledger, other, 0, "A-101").await;
    let args = Values::new()
        .with("order_id", "A-101")
        .with("field", "note")
        .with("value", "x");
    let rejected = handle
        .act(
            &mut prepared,
            &update.act,
            &input(&args, Some(record(&other, 0)), Some(&closed), None),
            Authority::Write(&intent),
        )
        .await;
    assert_eq!(
        failed(rejected),
        code(FailureClass::Rejected, &codes::APP_VALIDATION_ERROR, Sent::Answered)
    );
    assert_eq!(control.write_count("A-101").unwrap(), 0);
}

#[tokio::test]
async fn a_write_lost_in_flight_is_possible_commit_and_the_lookup_finds_the_one_write() {
    let scene = scene().await;
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["new"]).await;
    let control = scene.stand.control();
    control
        .set_fault(Fault::once(FaultKind::DropAfterCommit).on_tool("orders_create"))
        .unwrap();
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.plan,
        &scene.surface,
        fence,
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let create = steps(&scene.plan, "example_order_create");
    let args = Values::new()
        .with("customer", "Fourth Example Customer")
        .with("note", Value::Null);
    let intent = token(&ledger, fence, 0, "new").await;
    let lost = handle
        .act(
            &mut prepared,
            &create.act,
            &input(&args, Some(record(&fence, 0)), None, Some("STAMP-0001")),
            Authority::Write(&intent),
        )
        .await;
    assert_eq!(
        failed(lost),
        code(
            FailureClass::PossibleCommit,
            &a2m_provider::codes::NO_CONFIRMATION,
            Sent::Unknown
        )
    );
    handle.release(prepared, true).await;
    // The stand committed it, and its answer never left.
    assert_eq!(control.write_count("A-103").unwrap(), 1);

    // Reconcile: the lookup finds the record by its stamp. Nothing is
    // written again.
    let (handle, mut prepared) = scene.prepared().await;
    let none = Values::new();
    let found = done(
        handle
            .act(
                &mut prepared,
                &create.lookup.clone().unwrap(),
                &input(&none, None, None, Some("STAMP-0001")),
                Authority::Read,
            )
            .await,
    );
    assert_eq!(found.values.get("order_id"), Some(&Value::from("A-103")));
    assert_eq!(scene.calls_to("orders_create"), 1);
    assert_eq!(control.write_count("A-103").unwrap(), 1);
    assert_eq!(control.stored_orders().unwrap().len(), 4);

    // A stamp nothing was written under finds nothing.
    let absent = handle
        .act(
            &mut prepared,
            &create.lookup.unwrap(),
            &input(&none, None, None, Some("STAMP-0002")),
            Authority::Read,
        )
        .await;
    assert!(matches!(absent, Ok(StepOutcome::Absent(_))));

    // A read whose connection drops before the server handled it is
    // transient: nothing ran, and nothing is in doubt for a read.
    control
        .set_fault(Fault::once(FaultKind::DropBeforeHandling).on_tool("orders_get"))
        .unwrap();
    let (class, _, _) = failed(scene.get("A-100").await);
    assert_eq!(class, FailureClass::Transient);
}

#[tokio::test]
async fn a_changed_tool_is_drift_and_the_call_does_not_reach_the_stand() {
    let scene = scene().await;
    let control = scene.stand.control();
    let tool = || "orders_get".to_owned();
    for drift in [
        Drift::ToolRemoved { tool: tool() },
        Drift::InputSchemaChanged { tool: tool() },
        Drift::OutputSchemaChanged { tool: tool() },
        Drift::AnnotationsChanged { tool: tool() },
    ] {
        control.set_drift(drift.clone()).unwrap();
        assert_eq!(
            failed(scene.get("A-100").await),
            code(FailureClass::Drift, &codes::UPSTREAM_TOOL_CHANGED, Sent::NotSent),
            "{drift:?}"
        );
        control.clear_drift().unwrap();
    }
    assert_eq!(scene.calls_to("orders_get"), 0);

    // A tool that did not change still runs while another has.
    control
        .set_drift(Drift::ToolRemoved {
            tool: "orders_report".to_owned(),
        })
        .unwrap();
    done(scene.get("A-100").await);
    control.clear_drift().unwrap();

    // The server's words about a tool are not pinned and never reach a
    // caller: a changed description changes nothing.
    control
        .set_drift(Drift::DescriptionChanged { tool: tool() })
        .unwrap();
    let read = done(scene.get("A-100").await);
    assert!(!everything(&read).contains(INSTRUCTION_TEXT));
    control.clear_drift().unwrap();

    // A new version of the server, with every pin matching, still runs.
    control.set_server_version("2.0.0").unwrap();
    done(scene.get("A-100").await);
}

#[tokio::test]
async fn only_mapped_tools_are_reachable() {
    let scene = scene().await;
    let control = scene.stand.control();
    control.set_drift(Drift::ToolAdded).unwrap();
    // A step that names a tool the server lists and the mapping does not:
    // one the server always had, and one it added.
    for unmapped in ["orders_delete", "orders_archive"] {
        let mut step = steps(&scene.plan, "example_order_get").act;
        step.payload = PackPayload::encode(&serde_json::json!({
            "surface": "orders",
            "tool": unmapped,
        }))
        .unwrap();
        assert_eq!(
            failed(
                scene
                    .read(&step, Values::new().with("order_id", "A-100"))
                    .await
            ),
            code(FailureClass::Policy, &codes::NOT_GRANTED, Sent::NotSent)
        );
    }
    // A tool mapped as a write, in a step that does not commit.
    let mut write = steps(&scene.plan, "example_order_note_update").act;
    write.commits = false;
    let args = Values::new()
        .with("order_id", "A-100")
        .with("field", "note")
        .with("value", "x");
    assert_eq!(
        failed(scene.read(&write, args).await),
        code(FailureClass::Policy, &codes::NOT_GRANTED, Sent::NotSent)
    );
    // Arguments that do not fit the pinned input schema never leave.
    let get = steps(&scene.plan, "example_order_get").act;
    assert_eq!(
        failed(scene.read(&get, Values::new().with("order_id", 100_i64)).await),
        code(FailureClass::Input, &codes::ARGUMENTS_INVALID, Sent::NotSent)
    );
    assert!(control.calls().unwrap().is_empty());
    assert_eq!(control.request_count("tools/call").unwrap(), 0);
}

#[tokio::test]
async fn what_the_server_gets_wrong_is_a_named_stop() {
    let scene = scene().await;
    let control = scene.stand.control();
    let on_get = |kind: FaultKind| Fault::once(kind).on_tool("orders_get");

    for shape in [Malformed::NotJson, Malformed::NotAResponse] {
        control.set_fault(on_get(FaultKind::Malformed { shape })).unwrap();
        let (class, code, _) = failed(scene.get("A-100").await);
        assert_eq!(
            (class, code.as_str()),
            (FailureClass::UnknownVariant, "unexpected_status")
        );
    }
    control
        .set_fault(on_get(FaultKind::ProtocolError {
            code: -32050,
            message: INSTRUCTION_TEXT.to_owned(),
        }))
        .unwrap();
    let failure = scene.get("A-100").await.unwrap_err();
    assert_eq!(failure.failure.code(), &codes::UNEXPECTED_STATUS);
    assert_eq!(failure.sent, Sent::Answered);
    // The server's own words are never part of a failure.
    assert!(!failure.failure.message().contains("Ignore"));

    control
        .set_fault(on_get(FaultKind::HttpStatus { status: 503 }))
        .unwrap();
    assert_eq!(
        failed(scene.get("A-100").await),
        code(FailureClass::Transient, &codes::UPSTREAM_UNAVAILABLE, Sent::Answered)
    );

    control.set_fault(on_get(FaultKind::ResultOffSchema)).unwrap();
    assert_eq!(
        failed(scene.get("A-100").await),
        code(FailureClass::Drift, &codes::RESPONSE_SCHEMA_MISMATCH, Sent::Answered)
    );

    // Text added to a result to steer an agent is not among the values.
    control.set_fault(on_get(FaultKind::InjectText)).unwrap();
    let read = done(scene.get("A-100").await);
    assert!(!format!("{:?}", read.values).contains(INSTRUCTION_TEXT));

    // An answer that does not come within the step's time.
    control
        .set_fault(on_get(FaultKind::Delay { ms: 5_000 }))
        .unwrap();
    let mut slow = steps(&scene.plan, "example_order_get").act;
    slow.timeout_ms = 200;
    assert_eq!(
        failed(scene.read(&slow, Values::new().with("order_id", "A-100")).await),
        code(FailureClass::Transient, &codes::TIMEOUT, Sent::Unknown)
    );
    control.clear_faults().unwrap();

    // A server that forgot its sessions: the call is refused as answered,
    // and the next run connects again.
    let (handle, mut prepared) = scene.prepared().await;
    control.expire_sessions().unwrap();
    let get = steps(&scene.plan, "example_order_get").act;
    let args = Values::new().with("order_id", "A-100");
    let gone = handle
        .act(&mut prepared, &get, &input(&args, None, None, None), Authority::Read)
        .await;
    assert_eq!(
        failed(gone),
        code(FailureClass::Transient, &codes::CONNECTION_LOST, Sent::Answered)
    );
    handle.release(prepared, true).await;
    done(scene.get("A-100").await);
}

#[tokio::test]
async fn an_https_upstream_is_refused_because_tls_is_not_built() {
    let scene = scene().await;
    let secure = surface(&scene.plan, "orders", Some("https://orders.example.test/mcp"));
    let refused = prepare(
        &scene.registry,
        &scene.plan,
        &secure,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(
        (refused.failure.class(), refused.failure.code(), refused.sent),
        (FailureClass::Input, &codes::TLS_NOT_ENABLED, Sent::NotSent)
    );
    let mode = a2m_pack_mcp::Names::new().unwrap().mode;
    let handle = scene.registry.driver(&mode).unwrap();
    assert_eq!(
        handle.health(&secure),
        DriverHealth::Unavailable(codes::TLS_NOT_ENABLED)
    );
    assert_eq!(handle.health(&scene.surface), DriverHealth::Ready);
    assert_eq!(scene.stand.control().request_count("initialize").unwrap(), 0);
}

#[tokio::test]
async fn the_revision_with_no_handshake_is_spoken_when_the_surface_states_it() {
    let scene = scene().await;
    let mut stateless = scene.surface.clone();
    stateless.surface.payload =
        PackPayload::encode(&serde_json::json!({"revision": "2026-07-28"})).unwrap();
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.plan,
        &stateless,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let get = steps(&scene.plan, "example_order_get").act;
    let args = Values::new().with("order_id", "A-102");
    let read = done(
        handle
            .act(&mut prepared, &get, &input(&args, None, None, None), Authority::Read)
            .await,
    );
    assert_eq!(read.values.get("reference"), Some(&Value::from("REF-102")));
    let control = scene.stand.control();
    assert_eq!(control.request_count("initialize").unwrap(), 0);
    assert_eq!(control.calls().unwrap()[0].connection, "stateless");
}

#[tokio::test]
async fn each_identity_has_its_own_connection_and_no_secret_reaches_a_caller() {
    let sessions = Sessions::new(vec![Arc::new(UpstreamMcp)]).await;
    let redact = sessions.redact();
    let scene = scene_with(|folder| registry_with(folder, redact)).await;
    let control = scene.stand.control();
    control.accept_token("PLANTED-token-of-ada", "ada").unwrap();
    control.accept_token("PLANTED-token-of-bob", "bob").unwrap();
    let ada = sessions.lease("ada", "PLANTED-token-of-ada").await;
    let bob = sessions.lease("bob", "PLANTED-token-of-bob").await;
    let get = steps(&scene.plan, "example_order_get").act;
    let args = Values::new().with("order_id", "A-100");

    // With no credential the server refuses, and the driver says so.
    let refused = prepare(
        &scene.registry,
        &scene.plan,
        &scene.surface,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(refused.failure.class(), FailureClass::SessionExpired);

    let mut observed = Vec::new();
    for lease in [&ada, &bob, &ada] {
        let (handle, mut prepared) = prepare(
            &scene.registry,
            &scene.plan,
            &scene.surface,
            fence_of(7),
            Posture::Live,
            Some(lease),
        )
        .await
        .unwrap();
        observed.push(done(
            handle
                .act(&mut prepared, &get, &input(&args, None, None, None), Authority::Read)
                .await,
        ));
        handle.release(prepared, false).await;
    }
    // The stand saw two sessions, each with one principal, and none shared.
    assert!(control.shared_connections().unwrap().is_empty());
    let principals: Vec<Vec<String>> = control
        .connections()
        .unwrap()
        .iter()
        .filter(|connection| !connection.principals.is_empty())
        .map(|connection| connection.principals.iter().cloned().collect())
        .collect();
    assert_eq!(principals, [vec!["ada".to_owned()], vec!["bob".to_owned()]]);
    let callers: Vec<Option<String>> = control
        .calls()
        .unwrap()
        .into_iter()
        .map(|call| call.principal)
        .collect();
    assert_eq!(
        callers,
        [Some("ada".to_owned()), Some("bob".to_owned()), Some("ada".to_owned())]
    );

    // The system's answer holds a secret: the stand is told, by a plain
    // client that is not the code under test, to store one in a note.
    let plain = reqwest::Client::builder().no_proxy().build().unwrap();
    let planted = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "orders_update", "arguments": {
            "order_id": "A-100", "field": "note", "value": "the token is PLANTED-token-of-bob"
        }}
    });
    let answer = plain
        .post(scene.stand.url())
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Authorization", "Bearer PLANTED-token-of-ada")
        .header("Content-Type", "application/json")
        .body(planted.to_string())
        .send()
        .await
        .unwrap();
    assert!(answer.status().is_success());
    assert!(
        control
            .stored_order("A-100")
            .unwrap()
            .unwrap()
            .note
            .contains("PLANTED-token-of-bob")
    );
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.plan,
        &scene.surface,
        fence_of(7),
        Posture::Live,
        Some(&ada),
    )
    .await
    .unwrap();
    let read = done(
        handle
            .act(&mut prepared, &get, &input(&args, None, None, None), Authority::Read)
            .await,
    );
    assert!(!everything(&read).contains("PLANTED"), "{}", everything(&read));
    for earlier in &observed {
        assert!(!everything(earlier).contains("PLANTED"));
    }

    // A credential the server stops accepting is `needs_login`, and the
    // failure names no token.
    control.revoke_token("PLANTED-token-of-ada").unwrap();
    let failure = handle
        .act(&mut prepared, &get, &input(&args, None, None, None), Authority::Read)
        .await
        .unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code(), failure.sent),
        (
            FailureClass::SessionExpired,
            &a2m_auth::codes::NEEDS_LOGIN,
            Sent::Answered
        )
    );
    assert!(!format!("{failure:?}").contains("PLANTED"));
    handle.release(prepared, true).await;
}
