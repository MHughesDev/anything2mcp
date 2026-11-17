//! The driver against the stdio form of the test stand: the server is a
//! local program, started through the supervisor from what the plan
//! declares. Every write is proven by asking the stand what it stored.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{Digest, FailureClass, Plan, Value, Values};
use a2m_provider::{Authority, Posture, Registry, Sent, StepOutcome};
use common::{
    Folder, compile, fence_of, input, key, prepare, record, registry, running, shown, steps, store,
    surface, token, write_target,
};
use example_mcp::faults::{Fault, FaultKind};
use example_mcp::harness::StdioStand;

const PLACEHOLDER_DIGEST: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

struct Scene {
    _folder: Folder,
    registry: Registry,
    plan: Plan,
    stand: StdioStand,
}

/// The fixture target with the surface `orders_local` pointed at the
/// stand's program, pinned by `digest`, or by the program's own digest.
/// The local mapping gains the update tool, so that a write can be proven.
fn scene(digest: Option<&str>) -> Scene {
    let folder = Folder::new("stdio");
    let registry = registry(&folder);
    let stand = StdioStand::new().unwrap();
    let program = stand.program().to_string_lossy().into_owned();
    let state = stand.state_folder().to_string_lossy().into_owned();
    let own = format!(
        "sha256:{}",
        Digest::of(&std::fs::read(stand.program()).unwrap())
    );
    let digest = digest.map_or(own, ToOwned::to_owned);
    let root = folder.path().join("target");
    write_target(&root, &|path, text| match path {
        "target.yaml" => text
            .replace("/opt/example/bin/example-mcp", &program)
            .replace(PLACEHOLDER_DIGEST, &digest)
            .replace("/var/lib/example-mcp", &state),
        "operations/order_note_update.yaml" => text.replace(
            "    tool: orders_update",
            "    surface: orders_local\n    tool: orders_update",
        ),
        "operations/order_get.yaml" => text.replace(
            "    tool: orders_get",
            "    surface: orders_local\n    tool: orders_get",
        ),
        "mappings/orders_local.yaml" => format!(
            "{text}  orders_update:\n    digest: \"sha256:4dae58ac57727e98baba35c455cb2dcffa9d03d5c3c205cbbfe05407966d0ce1\"\n    effect: write\n    read_back: orders_get\n    result: text\n"
        ),
        _ => text,
    });
    let plan = match compile(&registry, &root) {
        Ok(compiled) => compiled.plan,
        Err(problems) => panic!("the local target does not compile:\n{}", shown(&problems)),
    };
    Scene {
        _folder: folder,
        registry,
        plan,
        stand,
    }
}

#[tokio::test]
async fn a_local_server_program_is_started_by_the_supervisor_and_read_and_written() {
    let scene = scene(None);
    let control = scene.stand.control();
    let local = surface(&scene.plan, "orders_local", None);
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["A-100"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.plan,
        &local,
        fence,
        Posture::Live,
        None,
    )
    .await
    .unwrap();

    let get = steps(&scene.plan, "example_order_get").act;
    let args = Values::new().with("order_id", "A-100");
    let Ok(StepOutcome::Done(read)) = handle
        .act(
            &mut prepared,
            &get,
            &input(&args, None, None, None),
            Authority::Read,
        )
        .await
    else {
        panic!("the read was not done");
    };
    assert_eq!(read.values.get("status"), Some(&Value::from("draft")));

    let update = steps(&scene.plan, "example_order_note_update");
    let args = Values::new()
        .with("order_id", "A-100")
        .with("field", "note")
        .with("value", "Leave at the gate");
    let order = key("A-100");
    let intent = token(&ledger, fence, 0, "A-100").await;
    let written = handle
        .act(
            &mut prepared,
            &update.act,
            &input(&args, Some(record(&fence, 0)), Some(&order), None),
            Authority::Write(&intent),
        )
        .await;
    assert!(matches!(written, Ok(StepOutcome::Done(_))), "{written:?}");
    assert!(handle.release(prepared, false).await.complete);

    // The stand says what the program stored, how often, and over what.
    assert_eq!(control.write_count("A-100").unwrap(), 1);
    assert_eq!(
        control.stored_order("A-100").unwrap().unwrap().note,
        "Leave at the gate"
    );
    let connections = control.connections().unwrap();
    assert_eq!(connections.len(), 1);
    assert!(connections[0].id.starts_with("stdio-"));
}

#[tokio::test]
async fn a_program_that_exits_after_a_commit_is_possible_commit() {
    let scene = scene(None);
    let control = scene.stand.control();
    control
        .set_fault(Fault::once(FaultKind::DropAfterCommit).on_tool("orders_update"))
        .unwrap();
    let local = surface(&scene.plan, "orders_local", None);
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["A-100"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.plan,
        &local,
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
        .with("value", "Leave at the gate");
    let order = key("A-100");
    let intent = token(&ledger, fence, 0, "A-100").await;
    let lost = handle
        .act(
            &mut prepared,
            &update.act,
            &input(&args, Some(record(&fence, 0)), Some(&order), None),
            Authority::Write(&intent),
        )
        .await
        .unwrap_err();
    assert_eq!(
        (lost.failure.class(), lost.failure.code(), lost.sent),
        (
            FailureClass::PossibleCommit,
            &a2m_provider::codes::NO_CONFIRMATION,
            Sent::Unknown
        )
    );
    handle.release(prepared, true).await;
    // The program committed the write before it went away, once.
    assert_eq!(control.write_count("A-100").unwrap(), 1);
}

#[tokio::test]
async fn a_program_whose_digest_is_not_the_pinned_one_is_not_started() {
    let other = format!("sha256:{}", Digest::of(b"another program"));
    let scene = scene(Some(&other));
    let local = surface(&scene.plan, "orders_local", None);
    let refused = prepare(
        &scene.registry,
        &scene.plan,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(refused.sent, Sent::NotSent);
    // The supervisor's refusal: the program on disk is not the one the
    // plan declares.
    assert_eq!(refused.failure.class(), FailureClass::Policy);
    assert!(scene.stand.control().connections().unwrap().is_empty());
}
