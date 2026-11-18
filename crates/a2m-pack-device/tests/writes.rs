//! Writes through the driver handle, against the simulated device only.
//! The device is asked what it stored: a write is never proven by reading
//! it back through the pack alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{Digest, FailureClass, NoSecrets, PackPayload, Step, Value, Values};
use a2m_ledger::Store;
use a2m_provider::{Posture, Sent, StepFailure, codes as contract};
use common::{Ran, Scene, Writing, decimal, store, write_run};
use example_device::{Answer, Corruption, Fault, FaultKind, FrameKind, Scope};

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

/// How far a write got: what its three lists of steps gave, or the list
/// that failed and how.
struct Written {
    act: Result<Ran, StepFailure>,
    confirm: Option<Result<Ran, StepFailure>>,
    read_back: Option<Result<Ran, StepFailure>>,
}

impl Written {
    fn failure(&self) -> &StepFailure {
        self.act
            .as_ref()
            .err()
            .or_else(|| self.confirm.as_ref().and_then(|ran| ran.as_ref().err()))
            .or_else(|| self.read_back.as_ref().and_then(|ran| ran.as_ref().err()))
            .expect("the write failed")
    }

    fn read_back(&self) -> &Ran {
        self.read_back.as_ref().unwrap().as_ref().unwrap()
    }
}

/// Carries a write operation out as the engine does: act, confirm, read
/// back, each only when the one before it passed. Nothing is sent twice.
async fn write(
    scene: &Scene,
    store: &Store,
    caller: u8,
    operation: &str,
    inputs: &Values,
) -> Written {
    // Each run writes a record of its own: the ledger keeps the lock of a
    // run that did not finish, as it should.
    let (fence, record, token) = write_run(store, caller, &format!("{operation} {caller}")).await;
    let (handle, mut prepared) = scene.prepare(Some(fence), Posture::Live).await;
    let writing = Writing {
        token: &token,
        record,
    };
    let binding = scene.binding(operation);
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
    Written {
        act,
        confirm,
        read_back,
    }
}

fn rate(text: &str) -> Values {
    Values::new().with("rate", decimal(text))
}

fn write_frames(scene: &Scene) -> usize {
    scene
        .device
        .control()
        .frames()
        .iter()
        .filter(|frame| frame.kind == FrameKind::Write)
        .count()
}

#[tokio::test]
async fn a_write_lands_once_and_a_value_the_device_rounds_still_verifies() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();

    // 2.52 is raw 252; the device keeps the nearest multiple of 5.
    let written = write(&scene, &store, 1, "example_dosing_rate_set", &rate("2.52")).await;
    assert!(written.act.is_ok() && matches!(written.confirm, Some(Ok(_))));

    // The stand: exactly one write, of the register named, and what it
    // stored.
    assert_eq!(control.write_count("dosing_rate"), 1);
    assert_eq!(write_frames(&scene), 1);
    assert_eq!(control.number("dosing_rate"), Some(250));

    // The value before the write was kept for reconcile, before the frame.
    let act = written.act.as_ref().unwrap();
    assert_eq!(
        act.for_reconcile.get("before_dosing_rate"),
        Some(&decimal("2.5"))
    );

    // The acknowledgement echoed 2.52; the register read back is 2.5. They
    // are the same through the field type's normalizer.
    let read = written.read_back().values.get("rate").unwrap().clone();
    assert_eq!(read, decimal("2.5"));
    assert_eq!(
        scene.normalized("dosing_rate_lpm", &read),
        scene.normalized("dosing_rate_lpm", &decimal("2.52"))
    );

    // A two-word value, low word first, lands as the map lays it out.
    let target = Values::new().with("target", 123_456_i64);
    let written = write(&scene, &store, 2, "example_batch_target_set", &target).await;
    assert_eq!(control.number("batch_target"), Some(123_456));
    assert_eq!(
        written.read_back().values.get("target"),
        Some(&Value::Int(123_456))
    );
    assert_eq!(control.write_count("batch_target"), 1);
}

#[tokio::test]
async fn a_value_the_device_changes_is_a_different_value_on_read_back() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();

    // The device clamps the limit to 80 and still echoes what was sent.
    let limit = Values::new().with("limit", 90_i64);
    let written = write(&scene, &store, 1, "example_heater_limit_set", &limit).await;
    assert!(
        matches!(written.confirm, Some(Ok(_))),
        "the echo acknowledged the write"
    );
    assert_eq!(control.number("heater_power_limit"), Some(80));
    assert_eq!(control.write_count("heater_power_limit"), 1);

    // An acknowledgement is not a read-back: the register differs, and
    // through the field type's normalizer it still differs.
    let read = written.read_back().values.get("limit").unwrap().clone();
    assert_eq!(read, Value::Int(80));
    assert_ne!(
        scene.normalized("integer", &read),
        scene.normalized("integer", &Value::Int(90))
    );
}

#[tokio::test]
async fn a_value_outside_its_range_sends_no_frame() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();

    for (caller, refused) in [(1, "100.01"), (2, "-0.01"), (3, "2.555")] {
        let written = write(
            &scene,
            &store,
            caller,
            "example_dosing_rate_set",
            &rate(refused),
        )
        .await;
        let failure = written.failure();
        assert_eq!(
            (class_and_code(failure), failure.sent),
            ((FailureClass::Input, "value_out_of_range"), Sent::NotSent),
            "{refused}"
        );
        assert!(failure.failure.message().contains("No frame was sent"));
        // The stated value never reaches a message.
        assert!(!failure.failure.message().contains(refused));
    }
    // A value of another kind is refused the same way.
    let wrong = Values::new().with("limit", "90");
    let (fence, record, token) = write_run(&store, 4, "wrong kind").await;
    let (handle, mut prepared) = scene.prepare(Some(fence), Posture::Live).await;
    let steps = &scene.binding("example_heater_limit_set").act;
    let args = Values::new().with("heater_power_limit", "90");
    let input = a2m_provider::StepInput::new(
        &args,
        a2m_provider::Deadline::after(std::time::Duration::from_secs(5)),
    )
    .for_record(record, None, None);
    scene
        .run(&handle, &mut prepared, &steps[..4], &wrong, None)
        .await
        .unwrap();
    let failure = handle
        .act(
            &mut prepared,
            &steps[4],
            &input,
            a2m_provider::Authority::Write(&token),
        )
        .await
        .unwrap_err();
    assert_eq!(
        (class_and_code(&failure), failure.sent),
        ((FailureClass::Input, "schema_invalid"), Sent::NotSent)
    );
    handle.release(prepared, true).await;

    // The stand: reads only. No write frame was ever received, and nothing
    // was stored.
    assert_eq!(write_frames(&scene), 0);
    assert!(
        control
            .inspect()
            .write_counts
            .values()
            .all(|count| *count == 0)
    );
    assert_eq!(control.number("dosing_rate"), Some(250));
}

#[tokio::test]
async fn a_link_lost_after_a_write_frame_is_a_possible_commit_and_nothing_is_sent_again() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();

    // The device applies the write and drops the link before the reply.
    control.set_fault(Fault::once(FaultKind::DropLink { applied: true }).on(Scope::Writes));
    let written = write(&scene, &store, 1, "example_dosing_rate_set", &rate("3.00")).await;
    assert!(written.act.is_ok(), "the frame was sent");
    let failure = written.failure();
    assert_eq!(
        (class_and_code(failure), failure.sent),
        (
            (FailureClass::PossibleCommit, "no_confirmation"),
            Sent::Unknown
        )
    );
    // The stand: it landed, once, and no second write frame came.
    assert_eq!(control.number("dosing_rate"), Some(300));
    assert_eq!(control.write_count("dosing_rate"), 1);
    assert_eq!(write_frames(&scene), 1);
    assert!(written.read_back.is_none());

    // The same when the device stays silent and stored nothing: the pack
    // cannot tell the two apart, and does not guess.
    control.set_fault(Fault::once(FaultKind::NoReply { applied: false }).on(Scope::Writes));
    let written = write(&scene, &store, 2, "example_dosing_rate_set", &rate("4.00")).await;
    assert_eq!(
        class_and_code(written.failure()),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert_eq!(control.number("dosing_rate"), Some(300));
    assert_eq!(write_frames(&scene), 2);

    // An echo of another value is no acknowledgement.
    control.set_fault(Fault::once(FaultKind::CorruptReply(Corruption::LastByte)).on(Scope::Writes));
    let written = write(&scene, &store, 3, "example_dosing_rate_set", &rate("5.00")).await;
    assert_eq!(
        (class_and_code(written.failure()), written.failure().sent),
        (
            (FailureClass::PossibleCommit, "no_confirmation"),
            Sent::Unknown
        )
    );
    assert_eq!(write_frames(&scene), 3);

    // Reconcile is a new link: the device is identified again, and the
    // register is read. It reads what the stand holds.
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let ran = scene
        .run(
            &handle,
            &mut prepared,
            &scene.binding("example_dosing_rate_set").read_back,
            &Values::new(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(ran.kinds[0], "open");
    assert_eq!(ran.values.get("rate"), Some(&decimal("5")));
    assert_eq!(control.number("dosing_rate"), Some(500));
    handle.release(prepared, false).await;
}

/// The `frame_check` evidence a run of read-back steps kept, if any.
fn frame_check(ran: &Ran) -> Option<serde_json::Value> {
    ran.captures
        .iter()
        .find(|capture| capture.name == "frame_check")
        .map(|capture| serde_json::from_slice(&capture.bytes).unwrap())
}

#[tokio::test]
async fn a_resumed_write_is_read_against_the_frame_that_was_recorded() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    let read_back = &scene.binding("example_dosing_rate_set").read_back;

    // The device applies the write and drops the link before the reply. The
    // frame was recorded by the step before the one that sent it.
    control.set_fault(Fault::once(FaultKind::DropLink { applied: true }).on(Scope::Writes));
    let written = write(&scene, &store, 1, "example_dosing_rate_set", &rate("3.00")).await;
    assert_eq!(
        class_and_code(written.failure()),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    let recorded = written.act.as_ref().unwrap().for_reconcile.clone();
    assert!(recorded.get("frame_dosing_rate").is_some());

    // On a resume the register is read on a new link, and the record has no
    // key: the register is found by its address in the step's own
    // arguments. What the device holds is the frame's value.
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let ran = scene
        .run_recorded(
            &handle,
            &mut prepared,
            read_back,
            &Values::new(),
            None,
            &recorded,
        )
        .await
        .unwrap();
    assert_eq!(ran.values.get("rate"), Some(&decimal("3")));
    assert_eq!(
        frame_check(&ran).unwrap()["device_vs_frame"],
        "matches",
        "the device holds what the recorded frame carried"
    );
    // The step said that it compared, and what it found is measured.
    let compared = ran
        .acting
        .iter()
        .find(|step| {
            step.means
                .iter()
                .any(|detail| detail.name == "compared_with")
        })
        .unwrap();
    assert!(
        compared
            .measured
            .iter()
            .any(|detail| detail.name == "frame_match" && detail.value == "matches")
    );
    // Nothing was written again: the same one frame, and the device's value.
    assert_eq!(write_frames(&scene), 1);
    assert_eq!(control.number("dosing_rate"), Some(300));

    // The same read, with no frame recorded, compares nothing.
    let plain = scene
        .run(&handle, &mut prepared, read_back, &Values::new(), None)
        .await
        .unwrap();
    assert!(frame_check(&plain).is_none());
    handle.release(prepared, false).await;

    // A write the device never applied: the frame carried 4.00 and the
    // device still holds 3.00. The read says so, and does not decide.
    control.set_fault(Fault::once(FaultKind::NoReply { applied: false }).on(Scope::Writes));
    let written = write(&scene, &store, 2, "example_dosing_rate_set", &rate("4.00")).await;
    assert_eq!(
        class_and_code(written.failure()),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    let recorded = written.act.as_ref().unwrap().for_reconcile.clone();
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let ran = scene
        .run_recorded(
            &handle,
            &mut prepared,
            read_back,
            &Values::new(),
            None,
            &recorded,
        )
        .await
        .unwrap();
    assert_eq!(ran.values.get("rate"), Some(&decimal("3")));
    assert_eq!(frame_check(&ran).unwrap()["device_vs_frame"], "differs");

    // A recorded frame that is no write of this register is read as
    // nothing, and said so.
    let garbled = Values::new().with("frame_dosing_rate", "00zz");
    let ran = scene
        .run_recorded(
            &handle,
            &mut prepared,
            read_back,
            &Values::new(),
            None,
            &garbled,
        )
        .await
        .unwrap();
    assert_eq!(frame_check(&ran).unwrap()["device_vs_frame"], "unreadable");
    handle.release(prepared, false).await;
    assert_eq!(write_frames(&scene), 2);
}

#[tokio::test]
async fn a_refusal_and_a_busy_device_are_named_and_a_write_is_never_sent_twice() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();

    // The device refuses the value with its own exception.
    control.set_fault(Fault::once(FaultKind::Exception { code: 0x03 }).on(Scope::Writes));
    let written = write(&scene, &store, 1, "example_dosing_rate_set", &rate("3.00")).await;
    assert_eq!(
        (class_and_code(written.failure()), written.failure().sent),
        ((FailureClass::Rejected, "device_refused"), Sent::Answered)
    );

    // Busy on a write: named, answered, and not sent again.
    control.set_fault(Fault::busy().on(Scope::Writes));
    let written = write(&scene, &store, 2, "example_dosing_rate_set", &rate("3.00")).await;
    assert_eq!(
        (class_and_code(written.failure()), written.failure().sent),
        ((FailureClass::Transient, "device_busy"), Sent::Answered)
    );

    // A failure that does not say whether the write was applied.
    control.set_fault(Fault::once(FaultKind::Exception { code: 0x04 }).on(Scope::Writes));
    let written = write(&scene, &store, 3, "example_dosing_rate_set", &rate("3.00")).await;
    assert_eq!(
        class_and_code(written.failure()),
        (FailureClass::PossibleCommit, "no_confirmation")
    );

    // The stand: three write frames, one for each run, each answered with
    // an exception, and nothing stored.
    let writes: Vec<_> = control
        .frames()
        .into_iter()
        .filter(|frame| frame.kind == FrameKind::Write)
        .collect();
    assert_eq!(writes.len(), 3);
    assert!(
        writes
            .iter()
            .all(|frame| matches!(frame.answer, Answer::Exception(_)))
    );
    assert_eq!(control.write_count("dosing_rate"), 0);
    assert_eq!(control.number("dosing_rate"), Some(250));
}

#[tokio::test]
async fn a_write_frame_needs_the_ledgers_token_and_a_posture_that_may_write() {
    let scene = Scene::start().await;
    let store = store().await;
    let inputs = rate("3.00");
    let act = &scene.binding("example_dosing_rate_set").act;

    // No token: the handle refuses the committing step before the driver.
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let failure = scene
        .run(&handle, &mut prepared, act, &inputs, None)
        .await
        .unwrap_err();
    assert_eq!(
        (class_and_code(&failure), failure.sent),
        (
            (FailureClass::Policy, contract::INTENT_MISSING.as_str()),
            Sent::NotSent
        )
    );
    handle.release(prepared, true).await;
    scene.links_closed().await;

    // Discovery: refused with a token too. The driver refuses to build
    // the frame, and the handle refuses the step that would send it.
    let (fence, record, token) = write_run(&store, 1, "discovery").await;
    let (handle, mut prepared) = scene.prepare(Some(fence), Posture::Discovery).await;
    let writing = Writing {
        token: &token,
        record,
    };
    let (build, send) = (&act[act.len() - 2], &act[act.len() - 1]);
    scene
        .run(&handle, &mut prepared, &act[..1], &inputs, Some(&writing))
        .await
        .unwrap();
    for step in [build, send] {
        let failure = scene
            .run(
                &handle,
                &mut prepared,
                std::slice::from_ref(step),
                &inputs,
                Some(&writing),
            )
            .await
            .unwrap_err();
        assert_eq!(
            (class_and_code(&failure), failure.sent),
            (
                (FailureClass::Policy, contract::READ_ONLY_POSTURE.as_str()),
                Sent::NotSent
            ),
            "{}",
            step.kind
        );
    }
    handle.release(prepared, true).await;

    assert_eq!(write_frames(&scene), 0);
    assert_eq!(scene.device.control().write_count("dosing_rate"), 0);
}

#[tokio::test]
async fn a_command_is_sent_once_and_has_nothing_to_read_back() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();

    let written = write(&scene, &store, 1, "example_fault_reset", &Values::new()).await;
    assert!(written.act.is_ok() && matches!(written.confirm, Some(Ok(_))));
    assert_eq!(written.read_back().kinds, Vec::<String>::new());
    assert_eq!(control.write_count("fault_reset"), 1);
    let frames = control.frames();
    // Identification, then the one command. Nothing was read on the way.
    assert_eq!(frames.len(), 2);
    assert_eq!(
        (frames[1].function, frames[1].kind),
        (0x05, FrameKind::Write)
    );
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|at| u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).unwrap())
        .collect()
}

#[tokio::test]
async fn a_device_with_another_serial_is_refused_and_nothing_is_written_where_none_is_declared() {
    let mut scene = Scene::start().await;
    let store = store().await;

    // Another unit of the same model and firmware answers at the address:
    // the surface declares a serial the device does not report.
    scene.declare_serial(Some("999999"));
    let written = write(&scene, &store, 1, "example_dosing_rate_set", &rate("3.00")).await;
    let failure = written.failure();
    assert_eq!(
        (class_and_code(failure), failure.sent),
        (
            (FailureClass::UnknownVariant, "device_identity_mismatch"),
            Sent::Answered
        )
    );
    // Neither serial reaches the message.
    assert!(!failure.failure.message().contains("999999"));
    assert!(!failure.failure.message().contains("100123"));
    // A read stops the same way.
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
    // The stand: two identifications, and nothing else.
    let frames = scene.device.control().frames();
    assert_eq!(frames.len(), 2);
    assert!(frames.iter().all(|frame| frame.function == 0x2B));

    // No serial declared for the environment: a read goes on by model and
    // firmware, and a write is refused before its frame is built.
    scene.declare_serial(None);
    scene.device.control().clear_frames();
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    scene
        .run(
            &handle,
            &mut prepared,
            &scene.binding("example_pump_speed_get").act,
            &Values::new(),
            None,
        )
        .await
        .unwrap();
    handle.release(prepared, false).await;
    for (caller, operation, inputs) in [
        (2, "example_dosing_rate_set", rate("3.00")),
        (3, "example_fault_reset", Values::new()),
    ] {
        let written = write(&scene, &store, caller, operation, &inputs).await;
        let failure = written.failure();
        assert_eq!(
            (class_and_code(failure), failure.sent),
            (
                (FailureClass::Policy, "device_serial_not_declared"),
                Sent::NotSent
            ),
            "{operation}"
        );
    }
    assert_eq!(write_frames(&scene), 0);
    assert_eq!(scene.device.control().write_count("dosing_rate"), 0);
    assert_eq!(scene.device.control().write_count("fault_reset"), 0);
    assert_eq!(scene.device.control().number("dosing_rate"), Some(250));
}

#[tokio::test]
async fn the_bytes_of_a_write_are_recorded_before_the_frame_is_sent() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    let binding = scene.binding("example_dosing_rate_set");
    let (before, commit) = binding.act.split_at(binding.act.len() - 1);
    assert!(commit[0].commits && before.iter().all(|step| !step.commits));

    let (fence, record, token) = write_run(&store, 1, "recorded first").await;
    let (handle, mut prepared) = scene.prepare(Some(fence), Posture::Live).await;
    let writing = Writing {
        token: &token,
        record,
    };
    let inputs = rate("3.00");
    let ran = scene
        .run(&handle, &mut prepared, before, &inputs, Some(&writing))
        .await
        .unwrap();
    // A step before the one that commits gave the frame for reconcile: the
    // engine stores what is given so before the committing step is sent.
    let Some(Value::Text(recorded)) = ran.for_reconcile.get("frame_dosing_rate") else {
        panic!(
            "no frame was recorded before the write: {:?}",
            ran.for_reconcile
        );
    };
    // It is kept as evidence too, with its digest.
    let capture = ran
        .captures
        .iter()
        .find(|capture| capture.name == "frame")
        .unwrap();
    let evidence = String::from_utf8(capture.bytes.clone()).unwrap();
    assert!(evidence.contains(recorded.as_str()), "{evidence}");
    assert!(
        evidence.contains(&Digest::of(&unhex(recorded)).to_string()),
        "{evidence}"
    );
    // The stand: no byte of a write has arrived yet.
    assert_eq!(write_frames(&scene), 0);

    // The link dies as the write arrives: no reply, and no later step.
    control.set_fault(Fault::once(FaultKind::DropLink { applied: true }).on(Scope::Writes));
    let sent = scene
        .run(&handle, &mut prepared, commit, &inputs, Some(&writing))
        .await
        .unwrap();
    // The handle refuses reconcile values from a committing step.
    assert!(sent.for_reconcile.is_empty());
    let failure = scene
        .run(
            &handle,
            &mut prepared,
            &binding.confirm,
            &inputs,
            Some(&writing),
        )
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    handle.release(prepared, true).await;
    scene.links_closed().await;
    // The stand: the one write frame it received is, byte for byte, the
    // frame that was recorded before it was sent.
    let writes: Vec<_> = control
        .frames()
        .into_iter()
        .filter(|frame| frame.kind == FrameKind::Write)
        .collect();
    assert_eq!(writes.len(), 1);
    assert_eq!(hex(&writes[0].raw), *recorded);

    // A value that changed after its frame was built and recorded is not
    // sent: the bytes on the link are the bytes in the record, or none.
    let (fence, record, token) = write_run(&store, 2, "changed since").await;
    let (handle, mut prepared) = scene.prepare(Some(fence), Posture::Live).await;
    let writing = Writing {
        token: &token,
        record,
    };
    scene
        .run(&handle, &mut prepared, before, &inputs, Some(&writing))
        .await
        .unwrap();
    let failure = scene
        .run(
            &handle,
            &mut prepared,
            commit,
            &rate("4.00"),
            Some(&writing),
        )
        .await
        .unwrap_err();
    assert_eq!(
        (class_and_code(&failure), failure.sent),
        ((FailureClass::Input, "payload_unreadable"), Sent::NotSent)
    );
    handle.release(prepared, true).await;
    scene.links_closed().await;

    // A write frame that no step built is not sent either.
    let (fence, record, token) = write_run(&store, 3, "never built").await;
    let (handle, mut prepared) = scene.prepare(Some(fence), Posture::Live).await;
    let writing = Writing {
        token: &token,
        record,
    };
    scene
        .run(
            &handle,
            &mut prepared,
            &before[..1],
            &inputs,
            Some(&writing),
        )
        .await
        .unwrap();
    let failure = scene
        .run(&handle, &mut prepared, commit, &inputs, Some(&writing))
        .await
        .unwrap_err();
    assert_eq!(
        (class_and_code(&failure), failure.sent),
        ((FailureClass::Input, "payload_unreadable"), Sent::NotSent)
    );
    handle.release(prepared, true).await;
    assert_eq!(write_frames(&scene), 1);
    assert_eq!(control.write_count("dosing_rate"), 1);
}

/// The steps of a write, with every step that builds or sends the write
/// frame pointed at another register: a plan the compiler did not make.
fn pointed_at(steps: &[Step], register: &str) -> Vec<Step> {
    steps
        .iter()
        .map(|step| {
            let mut step = step.clone();
            let mut payload: serde_json::Value = step.payload.decode().unwrap();
            for word in ["build", "write"] {
                if let Some(details) = payload.get_mut(word) {
                    details["register"] = register.into();
                }
            }
            step.payload = PackPayload::encode(&payload).unwrap();
            step
        })
        .collect()
}

#[tokio::test]
async fn a_hand_built_step_that_writes_a_physical_register_is_refused_by_the_driver() {
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    let before = control.number("pump_speed_setpoint");

    // The compiler refuses a binding that writes a `physical` register.
    // These steps were not made by it: an update and a command, each
    // pointed at a register that moves something.
    for (caller, operation, register, inputs) in [
        (
            1,
            "example_dosing_rate_set",
            "pump_speed_setpoint",
            rate("3.00"),
        ),
        (2, "example_fault_reset", "inlet_valve", Values::new()),
    ] {
        let act = pointed_at(&scene.binding(operation).act, register);
        let (fence, record, token) = write_run(&store, caller, register).await;
        let (handle, mut prepared) = scene.prepare(Some(fence), Posture::Live).await;
        let writing = Writing {
            token: &token,
            record,
        };
        let failure = scene
            .run(&handle, &mut prepared, &act, &inputs, Some(&writing))
            .await
            .unwrap_err();
        assert_eq!(
            (class_and_code(&failure), failure.sent),
            (
                (FailureClass::UnknownVariant, "physical_not_built"),
                Sent::NotSent
            ),
            "{register}"
        );
        handle.release(prepared, true).await;
        scene.links_closed().await;
    }

    // The stand: no write frame arrived, and nothing moved.
    assert_eq!(write_frames(&scene), 0);
    assert_eq!(control.write_count("pump_speed_setpoint"), 0);
    assert_eq!(control.write_count("inlet_valve"), 0);
    assert_eq!(control.number("pump_speed_setpoint"), before);
}

#[tokio::test]
async fn a_write_is_not_started_when_the_frames_left_would_not_cover_its_read_back() {
    // Identification, the value before, the write, the read-back: four.
    for (ceiling, lands) in [(4_i64, true), (3, false)] {
        let settings = Values::new().with("max_frames_per_run", ceiling);
        let scene = Scene::start_set(Arc::new(NoSecrets), Some(settings)).await;
        let store = store().await;
        let control = scene.device.control();
        let written = write(&scene, &store, 1, "example_dosing_rate_set", &rate("3.00")).await;
        if lands {
            assert_eq!(written.read_back().values.get("rate"), Some(&decimal("3")));
            assert_eq!(control.frames().len(), 4);
            assert_eq!(control.write_count("dosing_rate"), 1);
        } else {
            let failure = written.failure();
            assert!(written.act.is_err(), "the write was started");
            assert_eq!(
                (class_and_code(failure), failure.sent),
                (
                    (FailureClass::Policy, "frame_budget_exceeded"),
                    Sent::NotSent
                )
            );
            // The stand: two reads, no write, and the value it held.
            assert_eq!(control.frames().len(), 2);
            assert_eq!(write_frames(&scene), 0);
            assert_eq!(control.number("dosing_rate"), Some(250));
        }
    }
}

#[tokio::test]
async fn a_write_frame_not_marked_as_the_commit_point_is_refused_where_test_records_may_be_written()
{
    // The posture the engine gives the one context that writes a declared
    // test record. The driver lifts its guard there for the step marked
    // `commits` alone.
    const TEST_RECORDS: Posture = Posture::Sandbox { test_records: true };
    let scene = Scene::start().await;
    let store = store().await;
    let control = scene.device.control();
    let before = control.number("dosing_rate");
    let inputs = rate("3.00");
    let act = &scene.binding("example_dosing_rate_set").act;

    // A hand-built step: the binding's own write frame, with the mark
    // taken off. The handle passes it on, since a `write_frame` does not
    // always save; the driver refuses it before anything is sent.
    let mut unmarked = act.to_vec();
    let send = unmarked.last_mut().unwrap();
    assert!(send.commits);
    send.commits = false;
    let (fence, record, token) = write_run(&store, 1, "unmarked").await;
    let (handle, mut prepared) = scene.prepare(Some(fence), TEST_RECORDS).await;
    let writing = Writing {
        token: &token,
        record,
    };
    let failure = scene
        .run(&handle, &mut prepared, &unmarked, &inputs, Some(&writing))
        .await
        .unwrap_err();
    assert_eq!(
        (class_and_code(&failure), failure.sent),
        (
            (FailureClass::Policy, contract::READ_ONLY_POSTURE.as_str()),
            Sent::NotSent
        )
    );
    handle.release(prepared, true).await;
    scene.links_closed().await;

    // The stand: no write frame arrived, and the register holds what it held.
    assert_eq!(write_frames(&scene), 0);
    assert_eq!(control.write_count("dosing_rate"), 0);
    assert_eq!(control.number("dosing_rate"), before);

    // The same steps, with the commit point marked, write once.
    let (fence, record, token) = write_run(&store, 2, "marked").await;
    let (handle, mut prepared) = scene.prepare(Some(fence), TEST_RECORDS).await;
    let writing = Writing {
        token: &token,
        record,
    };
    let confirm = &scene.binding("example_dosing_rate_set").confirm;
    for steps in [act, confirm] {
        scene
            .run(&handle, &mut prepared, steps, &inputs, Some(&writing))
            .await
            .unwrap();
    }
    handle.release(prepared, false).await;
    assert_eq!(write_frames(&scene), 1);
    assert_eq!(control.write_count("dosing_rate"), 1);
}
