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

use std::path::Path;

use a2m_core::{Digest, FailureClass, Plan, Value, Values};
use a2m_pack_mcp::codes;
use a2m_provider::{Authority, Posture, Registry, Sent, StepOutcome};
use common::{
    Folder, compile, fence_of, input, key, prepare, record, registry, running, shown, steps, store,
    surface, token, write_target,
};
use example_mcp::faults::{Drift, Fault, FaultKind};
use example_mcp::harness::StdioStand;

const PLACEHOLDER_DIGEST: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

struct Scene {
    folder: Folder,
    registry: Registry,
    plan: Plan,
    stand: StdioStand,
}

/// The fixture target with the surface `orders_local` pointed at the
/// stand's program, pinned by `digest`, or by the program's own digest,
/// and compiled. The local mapping gains the update tool, so that a write
/// can be proven, and the report tool, a second read.
fn local_plan(registry: &Registry, root: &Path, stand: &StdioStand, digest: Option<&str>) -> Plan {
    let program = stand.program().to_string_lossy().into_owned();
    let state = stand.state_folder().to_string_lossy().into_owned();
    let own = format!(
        "sha256:{}",
        Digest::of(&std::fs::read(stand.program()).unwrap())
    );
    let digest = digest.map_or(own, ToOwned::to_owned);
    let local = |text: String, tool: &str| {
        text.replace(
            &format!("    tool: {tool}"),
            &format!("    surface: orders_local\n    tool: {tool}"),
        )
    };
    write_target(root, &|path, text| match path {
        "target.yaml" => text
            .replace("/opt/example/bin/example-mcp", &program)
            .replace(PLACEHOLDER_DIGEST, &digest)
            .replace("/var/lib/example-mcp", &state),
        "operations/order_note_update.yaml" => local(text, "orders_update"),
        "operations/order_get.yaml" => local(text, "orders_get"),
        "operations/order_report.yaml" => local(text, "orders_report"),
        "mappings/orders_local.yaml" => format!(
            "{text}  orders_update:\n    digest: \"sha256:4dae58ac57727e98baba35c455cb2dcffa9d03d5c3c205cbbfe05407966d0ce1\"\n    effect: write\n    read_back: orders_get\n    result: text\n  orders_report:\n    digest: \"sha256:76c8cdcb994efe4cd6da78ea52d828ff60f4b52292f76cd91ba0105d608799d8\"\n    effect: read\n"
        ),
        _ => text,
    });
    match compile(registry, root) {
        Ok(compiled) => compiled.plan,
        Err(problems) => panic!("the local target does not compile:\n{}", shown(&problems)),
    }
}

fn scene(digest: Option<&str>) -> Scene {
    let folder = Folder::new("stdio");
    let registry = registry(&folder);
    let stand = StdioStand::new().unwrap();
    let plan = local_plan(&registry, &folder.path().join("target"), &stand, digest);
    Scene {
        folder,
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

#[tokio::test]
async fn a_plan_published_again_with_another_program_digest_does_not_use_the_running_program() {
    let scene = scene(None);
    let control = scene.stand.control();
    let local = surface(&scene.plan, "orders_local", None);
    let get = steps(&scene.plan, "example_order_get").act;
    let args = Values::new().with("order_id", "A-100");
    // The first run starts the program, and the driver keeps it running.
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.plan,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let read = handle
        .act(
            &mut prepared,
            &get,
            &input(&args, None, None, None),
            Authority::Read,
        )
        .await;
    assert!(matches!(read, Ok(StepOutcome::Done(_))), "{read:?}");
    assert!(handle.release(prepared, false).await.complete);

    // A run whose plan does not declare the program is refused, although
    // the program is running.
    let mut undeclared = local.clone();
    undeclared.surface.programs.clear();
    let refused = prepare(
        &scene.registry,
        &scene.plan,
        &undeclared,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(
        (
            refused.failure.class(),
            refused.failure.code(),
            refused.sent
        ),
        (FailureClass::Policy, &codes::NOT_GRANTED, Sent::NotSent)
    );

    // The same target, published again: it now pins another program. The
    // next run is not given the one that is running; the supervisor is
    // asked, and refuses what is on disk.
    let other = format!("sha256:{}", Digest::of(b"another program"));
    let republished = local_plan(
        &scene.registry,
        &scene.folder.path().join("republished"),
        &scene.stand,
        Some(&other),
    );
    let local = surface(&republished, "orders_local", None);
    let refused = prepare(
        &scene.registry,
        &republished,
        &local,
        fence_of(8),
        Posture::Live,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(
        (refused.failure.class(), refused.sent),
        (FailureClass::Policy, Sent::NotSent)
    );
    assert_eq!(control.connections().unwrap().len(), 1);
    assert_eq!(control.calls_to("orders_get").unwrap(), 1);
}

#[tokio::test]
async fn the_tool_list_is_read_again_on_the_servers_notice_that_it_changed() {
    let scene = scene(None);
    let control = scene.stand.control();
    let local = surface(&scene.plan, "orders_local", None);
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.plan,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let get = steps(&scene.plan, "example_order_get").act;
    let report = steps(&scene.plan, "example_order_report").act;
    let args = Values::new().with("order_id", "A-100");
    let none = Values::new();
    let first = handle
        .act(
            &mut prepared,
            &get,
            &input(&args, None, None, None),
            Authority::Read,
        )
        .await;
    assert!(matches!(first, Ok(StepOutcome::Done(_))), "{first:?}");
    // The program said in its handshake that it gives notice, so a read
    // does not read the list again by itself.
    let listed = control.request_count("tools/list").unwrap();

    // The server changes a tool and gives notice.
    control
        .set_drift(Drift::OutputSchemaChanged {
            tool: "orders_get".to_owned(),
        })
        .unwrap();
    control.announce_list_changed().unwrap();
    control.wait_for_announcements(1).await.unwrap();
    // A call of a tool that did not change still runs. Its answer left the
    // program after the notice did, so the notice has been read by now.
    let other = handle
        .act(
            &mut prepared,
            &report,
            &input(&none, None, None, None),
            Authority::Read,
        )
        .await;
    assert!(matches!(other, Ok(StepOutcome::Done(_))), "{other:?}");
    // The changed tool is not called again in this run.
    let changed = handle
        .act(
            &mut prepared,
            &get,
            &input(&args, None, None, None),
            Authority::Read,
        )
        .await
        .unwrap_err();
    assert_eq!(
        (
            changed.failure.class(),
            changed.failure.code(),
            changed.sent
        ),
        (
            FailureClass::Drift,
            &codes::UPSTREAM_TOOL_CHANGED,
            Sent::NotSent
        )
    );
    assert_eq!(control.calls_to("orders_get").unwrap(), 1);
    // The list was read again once, on the notice, and not for each call.
    let pages = control.request_count("tools/list").unwrap() - listed;
    assert_eq!(pages, listed, "the list was read again once");
    handle.release(prepared, false).await;
}

/// The local server's surface, with one more entry under `environment` or
/// `pass_environment`, compiled: the problems that come back.
fn compile_with_variable(list: &str, name: &str) -> String {
    let folder = Folder::new("environment");
    let registry = registry(&folder);
    let root = folder.path().join("target");
    write_target(&root, &|path, text| {
        match path {
        "target.yaml" if list == "environment" => text
            .replace(
                "environment: {EXAMPLE_MCP_STATE: \"/var/lib/example-mcp\"}",
                &format!("environment: {{EXAMPLE_MCP_STATE: \"/var/lib/example-mcp\", {name}: elsewhere}}"),
            ),
        "target.yaml" => text.replace(
            "environment: {EXAMPLE_MCP_STATE: \"/var/lib/example-mcp\"}",
            &format!(
                "environment: {{EXAMPLE_MCP_STATE: \"/var/lib/example-mcp\"}}\n    pass_environment: [{name}]"
            ),
        ),
        _ => text,
    }
    });
    match compile(&registry, &root) {
        Ok(_) => String::new(),
        Err(problems) => problems
            .iter()
            .map(|problem| problem.code.as_str().to_owned())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[test]
fn a_variable_of_the_engine_or_a_credential_is_never_passed_to_a_local_server() {
    for list in ["environment", "pass_environment"] {
        for name in [
            "A2M_HOME",
            "a2m_vault_passphrase",
            "EXAMPLE_MCP_TOKEN",
            "example_api_key",
            "EXAMPLE_PASSWORD",
        ] {
            let problems = compile_with_variable(list, name);
            assert!(
                problems.contains("mcp.environment_refused"),
                "`{name}` in `{list}` compiled or was refused otherwise: {problems}"
            );
        }
    }
    // A name that only has a credential word inside it is not refused.
    assert_eq!(
        compile_with_variable("environment", "EXAMPLE_TOKEN_FILE"),
        ""
    );
}

#[tokio::test]
async fn a_plan_that_hands_a_local_server_such_a_variable_starts_nothing() {
    for (list, name) in [
        ("environment", "A2M_HOME"),
        ("pass_environment", "a2m_vault_passphrase"),
        ("environment", "EXAMPLE_MCP_TOKEN"),
        ("pass_environment", "EXAMPLE_API_KEY"),
    ] {
        let scene = scene(None);
        let mut local = surface(&scene.plan, "orders_local", None);
        // A plan the compiler would not make: the surface is changed by
        // hand, as a plan altered after it was compiled would be.
        let mut fields: serde_json::Value = local.surface.payload.decode().unwrap();
        if list == "environment" {
            fields["environment"][name] = serde_json::json!("elsewhere");
        } else {
            fields[list] = serde_json::json!([name]);
        }
        local.surface.payload = a2m_core::PackPayload::encode(&fields).unwrap();
        let refused = prepare(
            &scene.registry,
            &scene.plan,
            &local,
            fence_of(7),
            Posture::Live,
            None,
        )
        .await
        .map(|_| ())
        .expect_err("the variable was refused");
        assert_eq!(refused.sent, Sent::NotSent, "{list} {name}");
        assert_eq!(refused.failure.class(), FailureClass::Input);
        assert!(
            !refused.failure.next_step().is_empty()
                && refused.failure.next_step().contains("session keeper"),
            "{:?}",
            refused.failure
        );
        // The stand: no program was started, and no connection made.
        assert!(scene.stand.control().connections().unwrap().is_empty());
    }
}
