//! The firmware update, against the simulated device only: the device's own
//! sequence, with one commit point, approved as irreversible, never sent
//! twice, and read back after the device restarts. The device is asked what
//! it holds: an update is never proven by reading it back through the pack
//! alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{Digest, FailureClass, NoSecrets, PackPayload, Plan, Value, Values};
use a2m_files::{CompileMode, RelPath};
use a2m_ledger::Store;
use a2m_provider::{Posture, Registry, Sent, StepFailure, SurfaceRef};
use common::{
    Folder, Ran, Scene, Writing, compile_files, edited, files, registry, store, write_run,
};
use example_device::firmware::{MODEL_CODE, build_image, image_checksum};
use example_device::{Fault, FaultKind, FrameKind, Request, Scope, UpdateState, Version};

const OPERATION: &str = "example_firmware_update";
const MAP: &str = "registers/controller.yaml";
const OPERATION_FILE: &str = "operations/firmware_update.yaml";

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// An image the device's imaginary maker issued for this model, long enough
/// for three blocks.
fn image(version: Version) -> Vec<u8> {
    build_image(MODEL_CODE, version, &[7_u8; 500])
}

fn inputs(image: &[u8], version: &str) -> Values {
    Values::new()
        .with("image", hex(image))
        .with("version", version)
}

/// How far an update got: what its three lists of steps gave, or the list
/// that failed and how.
struct Updated {
    act: Result<Ran, StepFailure>,
    confirm: Option<Result<Ran, StepFailure>>,
    read_back: Option<Result<Ran, StepFailure>>,
}

impl Updated {
    fn failure(&self) -> &StepFailure {
        self.act
            .as_ref()
            .err()
            .or_else(|| self.confirm.as_ref().and_then(|ran| ran.as_ref().err()))
            .or_else(|| self.read_back.as_ref().and_then(|ran| ran.as_ref().err()))
            .expect("the update failed")
    }

    fn read_back(&self) -> &Ran {
        self.read_back.as_ref().unwrap().as_ref().unwrap()
    }
}

/// Carries an update out as the engine does: act, confirm, read back, each
/// only when the one before it passed. Nothing is sent twice.
async fn update(scene: &Scene, store: &Store, caller: u8, inputs: &Values) -> Updated {
    let (fence, record, token) = write_run(store, caller, &format!("update {caller}")).await;
    let (handle, mut prepared) = scene.prepare(Some(fence), Posture::Live).await;
    let writing = Writing {
        token: &token,
        record,
    };
    let binding = scene.binding(OPERATION);
    let act = scene
        .run(&handle, &mut prepared, &binding.act, inputs, Some(&writing))
        .await;
    let confirm = match &act {
        Ok(_) => Some(
            scene
                .run(
                    &handle,
                    &mut prepared,
                    &binding.confirm,
                    inputs,
                    Some(&writing),
                )
                .await,
        ),
        Err(_) => None,
    };
    let read_back = match &confirm {
        Some(Ok(_)) => Some(
            scene
                .run(
                    &handle,
                    &mut prepared,
                    &binding.read_back,
                    inputs,
                    Some(&writing),
                )
                .await,
        ),
        _ => None,
    };
    let poisoned = act.is_err() || !matches!(confirm, Some(Ok(_)));
    assert!(handle.release(prepared, poisoned).await.complete);
    scene.links_closed().await;
    Updated {
        act,
        confirm,
        read_back,
    }
}

/// The write frames the device received, as the addresses and what they
/// carried.
fn writes(scene: &Scene) -> Vec<Request> {
    scene
        .device
        .control()
        .frames()
        .into_iter()
        .filter(|frame| frame.kind == FrameKind::Write)
        .map(|frame| frame.request)
        .collect()
}

/// The plan of the example target with one edit.
fn edited_plan(registry: &Registry, path: &str, find: &str, replace: &str) -> Plan {
    compile_files(
        &edited(path, find, replace),
        registry.compiler_set(),
        CompileMode::Draft,
    )
    .unwrap_or_else(|problems| panic!("{problems}"))
    .plan
}

/// Declares another serial for the device of the environment `test`, or
/// none.
fn declare_serial(surface: &mut SurfaceRef, serial: Option<&str>) {
    let mut fields = serde_json::json!({"protocol": "modbus_tcp", "unit": 1});
    if let Some(serial) = serial {
        fields["serials"] = serde_json::json!({"test": serial});
    }
    surface.surface.payload = PackPayload::encode(&fields).unwrap();
}

#[tokio::test]
async fn an_update_goes_through_the_devices_own_sequence_and_is_read_back_after_the_restart() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    let bytes = image(Version::new(2, 4, 1));
    let checksum_before = control.firmware().checksum;

    let updated = update(&scene, &store, 1, &inputs(&bytes, "2.4.1")).await;
    assert!(updated.act.is_ok() && matches!(updated.confirm, Some(Ok(_))));

    // What was recorded before the step that commits: the image's digest,
    // what the device ran before, and nothing about the image's bytes.
    let act = updated.act.as_ref().unwrap();
    assert_eq!(
        act.for_reconcile.get("firmware_image_digest"),
        Some(&Value::Text(Digest::of(&bytes).to_string()))
    );
    assert_eq!(
        act.for_reconcile.get("before_firmware_version"),
        Some(&Value::Text("2.4.0".to_owned()))
    );
    assert_eq!(
        act.for_reconcile.get("before_firmware_checksum"),
        Some(&Value::Int(i64::from(checksum_before)))
    );
    let recorded: serde_json::Value = serde_json::from_slice(
        &act.captures
            .iter()
            .find(|capture| capture.name == "firmware")
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert_eq!(recorded["blocks"], 3);
    assert_eq!(recorded["signature_checked"], false);
    assert!(
        recorded["recovery"]
            .as_str()
            .unwrap()
            .contains("slot it ran before")
    );
    assert_eq!(recorded["cannot_undo"].as_array().unwrap().len(), 2);

    // The read-back: the device identified itself again, and its own
    // checksum is the one the image carries.
    let read_back = updated.read_back();
    assert_eq!(
        read_back.values.get("version"),
        Some(&Value::Text("2.4.1".to_owned()))
    );
    let checksum = image_checksum(&bytes).unwrap();
    assert_eq!(
        read_back.values.get("checksum"),
        Some(&Value::Text(format!("{checksum:08x}")))
    );

    // The stand: the device runs the image, committed once, from the
    // other slot, after one restart.
    let firmware = control.firmware();
    assert_eq!(firmware.version, Version::new(2, 4, 1));
    assert_eq!(firmware.checksum, checksum);
    assert_eq!(firmware.update_count, 2);
    assert_eq!(firmware.update_state, UpdateState::Idle);
    assert_eq!(control.inspect().restarts, 1);

    // The writes are the device's own sequence and no others: the length,
    // update mode, three blocks in order, verify, and one commit.
    let sent = writes(&scene);
    let command = |value: u16| Request::WriteSingleRegister {
        address: 900,
        value,
    };
    assert_eq!(sent.len(), 7, "{sent:?}");
    assert_eq!(
        sent[0],
        Request::WriteSingleRegister {
            address: 901,
            value: u16::try_from(bytes.len()).unwrap()
        }
    );
    assert_eq!(sent[1], command(1));
    for (number, request) in sent[2..5].iter().enumerate() {
        match request {
            Request::WriteMultipleRegisters { address, values } => {
                assert_eq!((*address, usize::from(values[0])), (1000, number));
            }
            other => panic!("not a block: {other:?}"),
        }
    }
    assert_eq!(sent[5], command(2));
    assert_eq!(sent[6], command(3));
    assert_eq!(
        control.write_count("update_command"),
        3,
        "enter, verify, commit"
    );
}

#[tokio::test]
async fn a_version_the_map_does_not_list_is_read_back_and_then_stops_every_run() {
    let scene = Scene::start().await;
    let store = store().await;
    let bytes = image(Version::new(2, 5, 0));

    let updated = update(&scene, &store, 1, &inputs(&bytes, "2.5.0")).await;
    assert_eq!(
        updated.read_back().values.get("version"),
        Some(&Value::Text("2.5.0".to_owned()))
    );

    // The map was proven against 2.4.0 and 2.4.1: a device that reports
    // another version stops the next run, reads included.
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let failure = scene
        .run(
            &handle,
            &mut prepared,
            &scene.binding("example_pump_speed_get").act,
            &Values::new(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::UnknownVariant, "device_identity_mismatch")
    );
    handle.release(prepared, true).await;
}

#[tokio::test]
async fn an_image_that_will_not_be_sent_sends_no_write_frame() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    let good = image(Version::new(2, 4, 1));
    let mut damaged = good.clone();
    damaged[20] ^= 1;
    let too_large = build_image(MODEL_CODE, Version::new(2, 4, 1), &vec![0_u8; 4096]);

    for (caller, inputs) in [
        // Not hexadecimal text.
        (
            1,
            Values::new().with("image", "zz").with("version", "2.4.1"),
        ),
        // Damaged: its checksum does not hold.
        (2, inputs(&damaged, "2.4.1")),
        // Not the version it carries.
        (3, inputs(&good, "2.4.0")),
        // Larger than the device takes.
        (4, inputs(&too_large, "2.4.1")),
        // Not an image at all.
        (
            5,
            inputs(b"not an image at all, but long enough to try", "2.4.1"),
        ),
    ] {
        let updated = update(&scene, &store, caller, &inputs).await;
        let failure = updated.failure();
        assert_eq!(
            (class_and_code(failure), failure.sent),
            ((FailureClass::Input, "image_not_accepted"), Sent::NotSent),
            "{caller}"
        );
    }
    assert!(writes(&scene).is_empty());
    assert_eq!(control.firmware().update_count, 1);
}

#[tokio::test]
async fn an_image_off_the_operations_list_of_digests_is_not_sent() {
    let mut scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    let bytes = image(Version::new(2, 4, 1));
    let listed = Digest::of(&bytes).to_string();
    let other = "0".repeat(64);
    scene.plan = edited_plan(
        &scene.registry,
        OPERATION_FILE,
        "      signature_checked: false\n",
        &format!("      signature_checked: false\n      accepts: [\"{other}\"]\n"),
    );
    let updated = update(&scene, &store, 1, &inputs(&bytes, "2.4.1")).await;
    assert_eq!(
        class_and_code(updated.failure()),
        (FailureClass::Input, "image_not_accepted")
    );
    assert!(writes(&scene).is_empty());

    // The same image, once its digest is on the list.
    scene.plan = edited_plan(
        &scene.registry,
        OPERATION_FILE,
        "      signature_checked: false\n",
        &format!("      signature_checked: false\n      accepts: [\"{listed}\"]\n"),
    );
    let updated = update(&scene, &store, 2, &inputs(&bytes, "2.4.1")).await;
    assert!(updated.read_back.is_some_and(|ran| ran.is_ok()));
    assert_eq!(control.firmware().update_count, 2);
}

#[tokio::test]
async fn an_update_is_not_started_while_a_precondition_does_not_hold() {
    let mut scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    // The operation says the pump must run at 10 before an update.
    scene.plan = edited_plan(
        &scene.registry,
        OPERATION_FILE,
        "pump_speed_setpoint: \"0\"",
        "pump_speed_setpoint: \"10\"",
    );
    let bytes = image(Version::new(2, 4, 1));
    let updated = update(&scene, &store, 1, &inputs(&bytes, "2.4.1")).await;
    let failure = updated.failure();
    assert_eq!(
        (class_and_code(failure), failure.sent),
        (
            (FailureClass::Policy, "update_precondition_unmet"),
            Sent::NotSent
        )
    );
    assert!(writes(&scene).is_empty());
    assert_eq!(control.firmware().update_count, 1);
}

#[tokio::test]
async fn a_device_that_refuses_the_image_is_told_to_leave_update_mode_and_the_run_ends_refused() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();

    // Refused at verify: well formed, and the device will not take it.
    control.set_refuse_image(true);
    let bytes = image(Version::new(2, 4, 1));
    let updated = update(&scene, &store, 1, &inputs(&bytes, "2.4.1")).await;
    let failure = updated.failure();
    assert_eq!(
        (class_and_code(failure), failure.sent),
        ((FailureClass::Rejected, "image_refused"), Sent::Answered)
    );
    assert!(updated.confirm.is_none());
    assert!(failure.failure.message().contains("update error 11"));
    // The stand: still the old image, out of update mode, and what cannot be
    // undone happened: the other slot was given up. No commit was sent.
    let firmware = control.firmware();
    assert_eq!(firmware.version, Version::new(2, 4, 0));
    assert_eq!(firmware.update_count, 1);
    assert_eq!(firmware.update_state, UpdateState::Idle);
    assert!(!firmware.slots[1 - firmware.active_slot].valid);
    assert!(!writes(&scene).contains(&Request::WriteSingleRegister {
        address: 900,
        value: 3
    }));
    assert_eq!(control.inspect().restarts, 0);

    // An image for another model passes the pack's checks and the device
    // refuses it at verify: the pack does not know the model code.
    control.reset();
    let other = build_image(0x0001, Version::new(2, 4, 1), &[1_u8; 100]);
    let updated = update(&scene, &store, 2, &inputs(&other, "2.4.1")).await;
    assert_eq!(
        class_and_code(updated.failure()),
        (FailureClass::Rejected, "image_refused")
    );
    assert!(
        updated
            .failure()
            .failure
            .message()
            .contains("update error 10")
    );
    assert_eq!(control.firmware().version, Version::new(2, 4, 0));
}

#[tokio::test]
async fn a_device_that_refuses_to_enter_update_mode_loses_nothing() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    // Update mode is refused with exception 04 at the enter command.
    control.set_fault(Fault {
        kind: FaultKind::Exception { code: 4 },
        scope: Scope::Writes,
        skip: 1,
        times: Some(1),
    });
    let bytes = image(Version::new(2, 4, 1));
    let updated = update(&scene, &store, 1, &inputs(&bytes, "2.4.1")).await;
    let failure = updated.failure();
    assert_eq!(
        (class_and_code(failure), failure.sent),
        ((FailureClass::Rejected, "device_refused"), Sent::Answered)
    );
    // The length was written, and nothing else: the other slot is intact.
    assert_eq!(writes(&scene).len(), 2);
    assert!(control.firmware().slots.iter().all(|slot| slot.valid));
}

#[tokio::test]
async fn a_link_lost_at_the_commit_is_a_possible_commit_and_nothing_is_sent_again() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    let bytes = image(Version::new(2, 4, 1));

    // The device carries the commit out and closes the link with no reply.
    // Six writes pass first: the length, enter, three blocks, verify.
    control.set_fault(Fault {
        kind: FaultKind::DropLink { applied: true },
        scope: Scope::Writes,
        skip: 6,
        times: Some(1),
    });
    let updated = update(&scene, &store, 1, &inputs(&bytes, "2.4.1")).await;
    assert!(updated.act.is_ok(), "the commit frame was sent");
    let failure = updated.failure();
    assert_eq!(
        (class_and_code(failure), failure.sent),
        (
            (FailureClass::PossibleCommit, "no_confirmation"),
            Sent::Unknown
        )
    );
    assert!(updated.read_back.is_none());
    // The stand: it landed, once, and no write frame followed the commit.
    assert_eq!(writes(&scene).len(), 7);
    assert_eq!(control.firmware().version, Version::new(2, 4, 1));
    assert_eq!(control.firmware().update_count, 2);

    // A frame lost while a block was sent is the same, and nothing is
    // retried: the device holds a partial image, and is not touched again.
    control.reset();
    control.set_fault(Fault {
        kind: FaultKind::DropLink { applied: false },
        scope: Scope::Writes,
        skip: 3,
        times: Some(1),
    });
    let updated = update(&scene, &store, 2, &inputs(&bytes, "2.4.1")).await;
    assert_eq!(
        class_and_code(updated.failure()),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert_eq!(writes(&scene).len(), 4);
    assert_eq!(control.firmware().update_count, 1);
}

#[tokio::test]
async fn a_device_that_does_not_come_back_is_a_possible_commit_and_nothing_is_retried() {
    let mut scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    // A short restart window, so that the test does not wait the ten
    // seconds the map declares.
    scene.plan = edited_plan(
        &scene.registry,
        MAP,
        "restart_within: 10s",
        "restart_within: 300ms",
    );
    control.set_stay_down_after_restart(true);
    let bytes = image(Version::new(2, 4, 1));

    let updated = update(&scene, &store, 1, &inputs(&bytes, "2.4.1")).await;
    assert!(updated.act.is_ok() && matches!(updated.confirm, Some(Ok(_))));
    let failure = updated.failure();
    assert_eq!(
        (class_and_code(failure), failure.sent),
        (
            (FailureClass::PossibleCommit, "device_not_returned"),
            Sent::Unknown
        )
    );
    assert!(failure.failure.message().contains("restart window"));
    // The stand: committed once, the device is down, and no write frame
    // followed the commit.
    assert_eq!(writes(&scene).len(), 7);
    assert!(!control.inspect().powered);
    assert_eq!(control.firmware().update_count, 2);
}

#[tokio::test]
async fn an_update_needs_the_serial_the_frames_it_may_send_and_a_run_that_may_write() {
    let mut scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    let bytes = image(Version::new(2, 4, 1));

    // No serial declared for the environment: nothing is written.
    declare_serial(&mut scene.surface, None);
    let updated = update(&scene, &store, 1, &inputs(&bytes, "2.4.1")).await;
    assert_eq!(
        class_and_code(updated.failure()),
        (FailureClass::Policy, "device_serial_not_declared")
    );
    declare_serial(&mut scene.surface, Some("100123"));
    assert!(writes(&scene).is_empty());

    // A run whose frames would not cover the update and its read-back does
    // not start it.
    let settings = Values::new().with("max_frames_per_run", 14_i64);
    let mut small = Scene::start_set(Arc::new(NoSecrets), Some(settings)).await;
    declare_serial(&mut small.surface, Some("100123"));
    let small_control = small.device.control();
    let updated = update(&small, &store, 2, &inputs(&bytes, "2.4.1")).await;
    let failure = updated.failure();
    assert_eq!(
        (class_and_code(failure), failure.sent),
        (
            (FailureClass::Policy, "frame_budget_exceeded"),
            Sent::NotSent
        )
    );
    assert!(
        small_control
            .frames()
            .iter()
            .all(|frame| frame.kind == FrameKind::Read)
    );

    // Where nothing may be written the update is refused, and no frame is
    // sent at all by its write steps.
    let (fence, record, token) = write_run(&store, 3, "read only").await;
    let (handle, mut prepared) = scene
        .prepare(
            Some(fence),
            Posture::Sandbox {
                test_records: false,
            },
        )
        .await;
    let writing = Writing {
        token: &token,
        record,
    };
    let failure = scene
        .run(
            &handle,
            &mut prepared,
            &scene.binding(OPERATION).act,
            &inputs(&bytes, "2.4.1"),
            Some(&writing),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.failure.class(), FailureClass::Policy);
    handle.release(prepared, true).await;
    assert!(writes(&scene).is_empty());
    assert_eq!(control.firmware().update_count, 1);
}

fn problems(files: &[(RelPath, Vec<u8>)]) -> a2m_files::Diagnostics {
    let folder = Folder::new("firmware-compile");
    let registry = registry(&folder);
    match compile_files(files, registry.compiler_set(), CompileMode::Draft) {
        Ok(compiled) => compiled.warnings,
        Err(problems) => problems,
    }
}

fn refused(files: &[(RelPath, Vec<u8>)], code: &str, file: &str) {
    let problems = problems(files);
    let found = problems
        .iter()
        .find(|found| found.code.as_str() == code && found.at.file.as_str() == file)
        .unwrap_or_else(|| panic!("no `{code}` in {file}:\n{problems}"));
    assert_eq!(found.level, a2m_files::Level::Error, "{found}");
    assert!(found.at.line >= 1 && found.at.column >= 1, "{found}");
    assert!(!found.fix.is_empty(), "{found}");
}

#[test]
fn an_update_without_a_declared_recovery_route_does_not_compile() {
    // The roadmap's proof: an update without a declared recovery path
    // cannot be published.
    let recovery =
        "      recovery: \"A person at the device starts it from the slot it ran before.\"\n";
    refused(
        &edited(OPERATION_FILE, recovery, ""),
        "device.firmware_without_recovery",
        OPERATION_FILE,
    );
    refused(
        &edited(OPERATION_FILE, recovery, "      recovery: \"   \"\n"),
        "device.firmware_without_recovery",
        OPERATION_FILE,
    );
    // What cannot be undone, and whether the device checks the maker's
    // signature, are declared too.
    refused(
        &edited(OPERATION_FILE, "      signature_checked: false\n", ""),
        "kernel.missing_field",
        OPERATION_FILE,
    );
}

#[test]
fn an_update_is_irreversible_with_the_hazard_device_function_and_needs_a_mechanism() {
    refused(
        &edited(
            OPERATION_FILE,
            "hazard: device_function\n",
            "hazard: none\n",
        ),
        "device.firmware_not_irreversible",
        OPERATION_FILE,
    );
    refused(
        &edited(OPERATION_FILE, "effect: irreversible\n", "effect: write\n"),
        "device.firmware_not_irreversible",
        OPERATION_FILE,
    );
    // A map that declares no update mechanism has none to carry.
    let mut files = files();
    let map = files
        .iter_mut()
        .find(|(path, _)| path.as_str() == MAP)
        .unwrap();
    let text = String::from_utf8(map.1.clone()).unwrap();
    let cut = text.find("firmware_update:").unwrap();
    map.1 = text.as_bytes()[..cut].to_vec();
    refused(
        &files,
        "device.firmware_update_not_declared",
        OPERATION_FILE,
    );
    // A digest on the list is the whole digest.
    refused(
        &edited(
            OPERATION_FILE,
            "      signature_checked: false\n",
            "      signature_checked: false\n      accepts: [\"abc\"]\n",
        ),
        "device.firmware_accepts_malformed",
        OPERATION_FILE,
    );
    // The update's registers are registers of the map that can be read
    // without effect.
    refused(
        &edited(
            MAP,
            "  state: update_state\n",
            "  state: no_such_register\n",
        ),
        "device.firmware_update_invalid",
        MAP,
    );
}

#[test]
fn the_update_plan_is_a_firmware_operation_with_one_commit_point() {
    let folder = Folder::new("firmware-plan");
    let registry = registry(&folder);
    let plan = common::compiled(registry.compiler_set()).plan;
    let operation = plan
        .operations
        .iter()
        .find(|found| found.name.as_str() == OPERATION)
        .unwrap();
    let binding = &operation.bindings[0];
    assert_eq!(operation.effect, a2m_core::EffectClass::Irreversible);
    let commits: Vec<_> = binding
        .act
        .iter()
        .chain(&binding.confirm)
        .chain(&binding.read_back)
        .filter(|step| step.commits)
        .collect();
    assert_eq!(commits.len(), 1);
    assert!(commits.iter().all(|step| !step.idempotent));
}
