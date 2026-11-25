//! The driver against the stdio form of the test stand: the server is a local
//! program, started through the supervisor from what the compiled target
//! declares, on Linux, macOS, and Windows alike (decision 0250). Every write
//! is proven by asking the stand what it stored, and what a program was
//! given, its arguments and its environment, by asking the program.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::{Path, PathBuf};

use a2m_core::{CompiledTarget, Digest, FailureClass, Value, Values};
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
    compiled_target: CompiledTarget,
    stand: StdioStand,
}

/// A text as a YAML scalar in single quotes, in which nothing is an
/// escape: a Windows path's backslashes stay as they are.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The fixture target with the surface `orders_local` pointed at the
/// stand's program, pinned by `digest`, or by the program's own digest,
/// and compiled. The local mapping gains the update tool, so that a write
/// can be proven, and the report tool, a second read.
fn local_compiled_target(
    registry: &Registry,
    root: &Path,
    stand: &StdioStand,
    digest: Option<&str>,
) -> CompiledTarget {
    local_compiled_target_with(registry, root, stand, stand.program(), digest, &[])
}

/// As [`local_compiled_target`], with the program at `program` and these arguments.
fn local_compiled_target_with(
    registry: &Registry,
    root: &Path,
    stand: &StdioStand,
    program: &Path,
    digest: Option<&str>,
    args: &[&str],
) -> CompiledTarget {
    let state = stand.state_folder().to_string_lossy().into_owned();
    let own = format!("sha256:{}", Digest::of(&std::fs::read(program).unwrap()));
    let program = program.to_string_lossy().into_owned();
    let args: Vec<String> = args.iter().map(|arg| quoted(arg)).collect();
    let digest = digest.map_or(own, ToOwned::to_owned);
    let local = |text: String, tool: &str| {
        text.replace(
            &format!("    tool: {tool}"),
            &format!("    surface: orders_local\n    tool: {tool}"),
        )
    };
    write_target(root, &|path, text| match path {
        "target.yaml" => text
            .replace("\"/opt/example/bin/example-mcp\"", &quoted(&program))
            .replace(PLACEHOLDER_DIGEST, &digest)
            .replace(
                "\"/var/lib/example-mcp\"}",
                &format!("{}}}\n    args: [{}]", quoted(&state), args.join(", ")),
            ),
        "operations/order_note_update.yaml" => local(text, "orders_update"),
        "operations/order_get.yaml" => local(text, "orders_get"),
        "operations/order_report.yaml" => local(text, "orders_report"),
        "mappings/orders_local.yaml" => format!(
            "{text}  orders_update:\n    digest: \"sha256:4dae58ac57727e98baba35c455cb2dcffa9d03d5c3c205cbbfe05407966d0ce1\"\n    effect: write\n    read_back: orders_get\n    result: text\n  orders_report:\n    digest: \"sha256:76c8cdcb994efe4cd6da78ea52d828ff60f4b52292f76cd91ba0105d608799d8\"\n    effect: read\n"
        ),
        _ => text,
    });
    match compile(registry, root) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("the local target does not compile:\n{}", shown(&problems)),
    }
}

fn scene(digest: Option<&str>) -> Scene {
    let folder = Folder::new("stdio");
    let registry = registry(&folder);
    let stand = StdioStand::new().unwrap();
    let compiled_target =
        local_compiled_target(&registry, &folder.path().join("target"), &stand, digest);
    Scene {
        folder,
        registry,
        compiled_target,
        stand,
    }
}

#[tokio::test]
async fn a_local_server_program_is_started_by_the_supervisor_and_read_and_written() {
    let scene = scene(None);
    let control = scene.stand.control();
    let local = surface(&scene.compiled_target, "orders_local", None);
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["A-100"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence,
        Posture::Live,
        None,
    )
    .await
    .unwrap();

    let get = steps(&scene.compiled_target, "example_order_get").act;
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

    let update = steps(&scene.compiled_target, "example_order_note_update");
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
    let local = surface(&scene.compiled_target, "orders_local", None);
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["A-100"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence,
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let update = steps(&scene.compiled_target, "example_order_note_update");
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
    let local = surface(&scene.compiled_target, "orders_local", None);
    let refused = prepare(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(refused.sent, Sent::NotSent);
    // The supervisor's refusal: the program on disk is not the one the
    // compiled target declares.
    assert_eq!(refused.failure.class(), FailureClass::Policy);
    assert!(scene.stand.control().connections().unwrap().is_empty());
}

#[tokio::test]
async fn a_compiled_target_published_again_with_another_program_digest_does_not_use_the_running_program()
 {
    let scene = scene(None);
    let control = scene.stand.control();
    let local = surface(&scene.compiled_target, "orders_local", None);
    let get = steps(&scene.compiled_target, "example_order_get").act;
    let args = Values::new().with("order_id", "A-100");
    // The first run starts the program, and the driver keeps it running.
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.compiled_target,
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

    // A run whose compiled target does not declare the program is refused,
    // although the program is running.
    let mut undeclared = local.clone();
    undeclared.surface.programs.clear();
    let refused = prepare(
        &scene.registry,
        &scene.compiled_target,
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
    let republished = local_compiled_target(
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
    let local = surface(&scene.compiled_target, "orders_local", None);
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let get = steps(&scene.compiled_target, "example_order_get").act;
    let report = steps(&scene.compiled_target, "example_order_report").act;
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
            // Variables that make a program load other code (decision 0250).
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "NODE_OPTIONS",
            "PYTHONPATH",
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
async fn a_compiled_target_that_hands_a_local_server_such_a_variable_starts_nothing() {
    for (list, name) in [
        ("environment", "A2M_HOME"),
        ("pass_environment", "a2m_vault_passphrase"),
        ("environment", "EXAMPLE_MCP_TOKEN"),
        ("pass_environment", "EXAMPLE_API_KEY"),
        ("environment", "LD_PRELOAD"),
        ("pass_environment", "NODE_OPTIONS"),
    ] {
        let scene = scene(None);
        let mut local = surface(&scene.compiled_target, "orders_local", None);
        // A compiled target the compiler would not make: the surface is
        // changed by hand, as a compiled target altered after it was compiled
        // would be.
        let mut fields: serde_json::Value = local.surface.payload.decode().unwrap();
        if list == "environment" {
            fields["environment"][name] = serde_json::json!("elsewhere");
        } else {
            fields[list] = serde_json::json!([name]);
        }
        local.surface.payload = a2m_core::PackPayload::encode(&fields).unwrap();
        let refused = prepare(
            &scene.registry,
            &scene.compiled_target,
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

// ------------------------------------------- the three systems (decision 0250)

/// Long enough for a loaded machine to end a program and what it started;
/// the tests wait on the condition and not for this long.
const WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

/// The file name of the stand's program on this system, with `stem` in
/// place of its own.
fn program_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// A copy of the stand's program at `<a fresh folder>/<folder>/<name>`,
/// with the folder that holds the copy. The fresh folder is removed when
/// the returned [`Folder`] is dropped.
fn placed(stand: &StdioStand, folder: &str, name: &str) -> (Folder, PathBuf, PathBuf) {
    let fresh = Folder::new("placed");
    let holder = fresh.path().join(folder);
    std::fs::create_dir_all(&holder).unwrap();
    let program = holder.join(name);
    std::fs::copy(stand.program(), &program).unwrap();
    (fresh, holder, program)
}

/// The fixture's local surface compiled for the program at `program`,
/// started with `args`.
fn scene_with(stand: StdioStand, program: &Path, args: &[&str]) -> Scene {
    let folder = Folder::new("stdio");
    let registry = registry(&folder);
    let compiled_target = local_compiled_target_with(
        &registry,
        &folder.path().join("target"),
        &stand,
        program,
        None,
        args,
    );
    Scene {
        folder,
        registry,
        compiled_target,
        stand,
    }
}

/// Prepares the local surface and reads one order, then lets it go.
async fn read_once(scene: &Scene) -> Result<Value, a2m_provider::StepFailure> {
    let local = surface(&scene.compiled_target, "orders_local", None);
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await?;
    let get = steps(&scene.compiled_target, "example_order_get").act;
    let args = Values::new().with("order_id", "A-100");
    let read = handle
        .act(
            &mut prepared,
            &get,
            &input(&args, None, None, None),
            Authority::Read,
        )
        .await;
    assert!(handle.release(prepared, read.is_err()).await.complete);
    match read? {
        StepOutcome::Done(observed) => Ok(observed.values.get("status").cloned().unwrap()),
        other => panic!("the read gave {other:?}"),
    }
}

/// Waits until `done` holds, asking again until [`WITHIN`] has passed.
async fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let started = std::time::Instant::now();
    while !done() {
        assert!(started.elapsed() < WITHIN, "never came about: {what}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// A local server program is read and written through one harness on every
/// system: this test and the first of this file run on Linux, macOS, and
/// Windows. Here the program also writes a megabyte to its standard error
/// before it serves, which nobody reads: the program is not held up by it,
/// and the engine keeps none of it.
#[tokio::test]
async fn a_program_that_writes_a_megabyte_to_standard_error_is_not_held_up_by_it() {
    let stand = StdioStand::new().unwrap();
    let program = stand.program().to_path_buf();
    let scene = scene_with(stand, &program, &["--stderr-megabyte"]);
    assert_eq!(read_once(&scene).await.unwrap(), Value::from("draft"));
    let starts = scene.stand.control().starts().unwrap();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].args, ["--stderr-megabyte"]);
}

/// A program that never answers ends at the step's deadline, and is gone
/// afterwards, with the process it started: the system is asked whether
/// either still runs.
#[tokio::test]
async fn a_program_that_never_answers_ends_at_the_deadline_and_leaves_nothing_running() {
    let stand = StdioStand::new().unwrap();
    let program = stand.program().to_path_buf();
    let scene = scene_with(stand, &program, &["--silent", "--grandchild"]);
    let local = surface(&scene.compiled_target, "orders_local", None);
    let started = std::time::Instant::now();
    let refused = common::prepare_within(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence_of(7),
        Posture::Live,
        None,
        &a2m_provider::Destinations::new([local.surface.address.clone()]),
        std::time::Duration::from_secs(3),
    )
    .await
    .map(|_| ())
    .expect_err("a program that never answers was connected to");
    assert!(
        started.elapsed() < WITHIN,
        "the prepare outlived its deadline: {:?}",
        started.elapsed()
    );
    assert_eq!(
        refused.failure.class(),
        FailureClass::Transient,
        "{refused:?}"
    );
    let control = scene.stand.control();
    let start = control
        .starts()
        .unwrap()
        .pop()
        .expect("the program started");
    let grandchild = start
        .grandchild
        .expect("the program started its grandchild");
    wait_until("the program and its grandchild are gone", || {
        !example_mcp::harness::process_runs(start.process).unwrap()
            && !example_mcp::harness::process_runs(grandchild).unwrap()
    })
    .await;
}

/// A program ended from outside in the middle of a write call, after the
/// stand stored the write and before it answered: the write is in doubt,
/// and it is not sent again.
#[tokio::test]
async fn a_program_killed_in_the_middle_of_a_call_leaves_the_write_in_doubt_and_sends_it_once() {
    let scene = scene(None);
    let control = scene.stand.control();
    control
        .set_fault(Fault::once(FaultKind::Delay { ms: 10 * 60 * 1000 }).on_tool("orders_update"))
        .unwrap();
    let local = surface(&scene.compiled_target, "orders_local", None);
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["A-100"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence,
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let process = control.starts().unwrap()[0].process;
    let update = steps(&scene.compiled_target, "example_order_note_update");
    let args = Values::new()
        .with("order_id", "A-100")
        .with("field", "note")
        .with("value", "Leave at the gate");
    let order = key("A-100");
    let intent = token(&ledger, fence, 0, "A-100").await;
    let step_input = input(&args, Some(record(&fence, 0)), Some(&order), None);
    let (written, ()) = tokio::join!(
        handle.act(
            &mut prepared,
            &update.act,
            &step_input,
            Authority::Write(&intent)
        ),
        async {
            wait_until("the stand stored the write", || {
                control.write_count("A-100").unwrap() == 1
            })
            .await;
            example_mcp::harness::kill_process(process).unwrap();
        }
    );
    let lost = written.unwrap_err();
    assert_eq!(
        (lost.failure.class(), lost.sent),
        (FailureClass::PossibleCommit, Sent::Unknown),
        "{lost:?}"
    );
    handle.release(prepared, true).await;
    wait_until("the program is gone", || {
        !example_mcp::harness::process_runs(process).unwrap()
    })
    .await;
    assert_eq!(control.write_count("A-100").unwrap(), 1);
    assert_eq!(control.calls_to("orders_update").unwrap(), 1);
}

/// The variables the outer test sets in the inner test's process, which
/// stands for the engine's: one of the engine's own, one named for a
/// credential, and one that makes a program load other code.
const ENGINE_LIKE: &[(&str, &str)] = &[
    ("A2M_EXAMPLE_FOR_TESTS", "engine-value-for-tests"),
    ("EXAMPLE_SERVICE_TOKEN", "secret-for-tests-0001"),
    ("PYTHONPATH", "elsewhere-for-tests"),
];

/// Starts this test binary again with [`ENGINE_LIKE`] set, running only the
/// test below, which does its work only then: a test may not set a
/// variable of its own process.
#[test]
fn the_program_sees_none_of_the_engines_variables_and_no_credential() {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "the_program_started_while_such_variables_are_set",
        "--nocapture",
    ]);
    for (name, value) in ENGINE_LIKE {
        command.env(name, value);
    }
    let output = command.output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("1 passed"),
        "the inner test did not run: {text}"
    );
}

#[tokio::test]
async fn the_program_started_while_such_variables_are_set() {
    if std::env::var_os(ENGINE_LIKE[0].0).is_none() {
        return;
    }
    let scene = scene(None);
    assert_eq!(read_once(&scene).await.unwrap(), Value::from("draft"));
    let start = scene.stand.control().starts().unwrap().pop().unwrap();
    for (name, _) in ENGINE_LIKE {
        assert!(
            !start.env.iter().any(|seen| seen.eq_ignore_ascii_case(name)),
            "the program saw `{name}`: {:?}",
            start.env
        );
    }
    // What it does see: the variable its surface sets, the process mark, and
    // on Windows the system's root folder; no variable of the engine's.
    assert!(
        start.env.iter().any(|seen| seen == "EXAMPLE_MCP_STATE"),
        "{:?}",
        start.env
    );
    assert!(
        !start
            .env
            .iter()
            .any(|seen| a2m_provider::is_engine_variable(seen)),
        "{:?}",
        start.env
    );
    #[cfg(target_os = "linux")]
    {
        let mut seen = start.env.clone();
        seen.sort();
        assert_eq!(
            seen,
            [a2m_provider::PROCESS_MARK_VARIABLE, "EXAMPLE_MCP_STATE"],
            "the program's whole environment on Linux"
        );
    }
}

/// Lets every user write `path`: on Linux and macOS by its mode, on Windows
/// by an entry granting the well-known group Everyone (`S-1-1-0`) the right
/// to write, set with the system's own `icacls`, started by its path.
fn open_to_everyone(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777)).unwrap();
    }
    #[cfg(windows)]
    {
        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let made = std::process::Command::new(system.join("System32").join("icacls.exe"))
            .arg(path)
            .args(["/grant", "*S-1-1-0:(W)"])
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "icacls did not grant the right: {}",
            String::from_utf8_lossy(&made.stdout)
        );
    }
}

/// Asserts that preparing the scene's local surface is refused as the
/// provider's check of a program refuses, and that nothing started.
async fn refused_before_it_starts(scene: &Scene, why: &str) {
    let local = surface(&scene.compiled_target, "orders_local", None);
    let refused = prepare(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .map(|_| ())
    .expect_err(why);
    assert_eq!(
        (
            refused.failure.class(),
            refused.failure.code(),
            refused.sent
        ),
        (
            FailureClass::Policy,
            &codes::PROGRAM_PATH_UNSAFE,
            Sent::NotSent
        ),
        "{why}: {refused:?}"
    );
    let control = scene.stand.control();
    assert!(control.starts().unwrap().is_empty(), "{why}: it started");
    assert!(control.connections().unwrap().is_empty(), "{why}");
}

/// The fifth safety audit and decision 0250: the MCP pack holds a program
/// to the same check of who may change it as the command pack. Before, it
/// checked the digest alone, so a program another user could replace was
/// started as long as it had not been replaced yet.
#[tokio::test]
async fn a_program_another_user_could_change_is_refused_before_it_starts() {
    // The file itself.
    let stand = StdioStand::new().unwrap();
    let (_fresh, _holder, program) = placed(&stand, "closed", &program_name("example-mcp"));
    open_to_everyone(&program);
    let scene = scene_with(stand, &program, &[]);
    refused_before_it_starts(&scene, "a program everyone may write").await;
    // The folder that holds it.
    #[cfg(unix)]
    {
        let stand = StdioStand::new().unwrap();
        let (_fresh, holder, program) = placed(&stand, "open", &program_name("example-mcp"));
        open_to_everyone(&holder);
        let scene = scene_with(stand, &program, &[]);
        refused_before_it_starts(&scene, "a program in a folder everyone may write").await;
    }
}

/// A shell, an interpreter, a batch file, or a script is refused by name
/// or by its first bytes, on every system; a program the target declares
/// as itself starts from a path with a space and letters beyond ASCII, and
/// on Linux and macOS quotes, and its arguments arrive element for element.
#[tokio::test]
async fn a_script_is_refused_and_an_odd_path_starts_with_its_arguments_element_for_element() {
    // An interpreter by its name: the pack does not start a server that an
    // interpreter runs from a script, whose bytes nothing would pin.
    let stand = StdioStand::new().unwrap();
    let (_fresh, _holder, node) = placed(&stand, "bin", &program_name("node"));
    let scene = scene_with(stand, &node, &[]);
    refused_before_it_starts(&scene, "a program named as an interpreter").await;
    // A batch file on Windows, a script on Linux and macOS.
    #[cfg(windows)]
    for name in ["example-server.bat", "example-server.cmd"] {
        let stand = StdioStand::new().unwrap();
        let fresh = Folder::new("batch");
        let batch = fresh.path().join(name);
        std::fs::write(&batch, "@echo off\r\n").unwrap();
        let scene = scene_with(stand, &batch, &[]);
        refused_before_it_starts(&scene, name).await;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let stand = StdioStand::new().unwrap();
        let fresh = Folder::new("script");
        let script = fresh.path().join("example-server");
        std::fs::write(&script, "#!/bin/sh\nexec true\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let scene = scene_with(stand, &script, &[]);
        refused_before_it_starts(&scene, "a script that starts with #!").await;
    }

    // A path with a space and letters beyond ASCII, and on Linux and macOS
    // both quotes, which a Windows file name cannot hold.
    let folder = if cfg!(windows) {
        "it's a caf\u{e9} \u{2713}"
    } else {
        "it's a \"caf\u{e9}\" \u{2713}"
    };
    let args: &[&str] = &[
        "a b",
        "\"double\" and 'single'",
        "caf\u{e9} \u{2713}",
        "$(echo x) `y` %PATH% ${HOME} ; & | *",
        "back\\slash\\",
        "trailing\\\\",
        "",
        "--flag=1",
        "-x",
    ];
    let stand = StdioStand::new().unwrap();
    let (_fresh, _holder, program) = placed(&stand, folder, &program_name("example-mcp"));
    let scene = scene_with(stand, &program, args);
    assert_eq!(read_once(&scene).await.unwrap(), Value::from("draft"));
    let starts = scene.stand.control().starts().unwrap();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].args, args, "the arguments the program received");
}
