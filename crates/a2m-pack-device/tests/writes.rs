//! Writes through the driver handle, against the simulated device only.
//! The device is asked what it stored: a write is never proven by reading
//! it back through the pack alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{FailureClass, Value, Values};
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
    let input = a2m_provider::StepInput {
        args: &args,
        record: Some(record),
        key: None,
        stamp: None,
        deadline: a2m_provider::Deadline::after(std::time::Duration::from_secs(5)),
    };
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

    // Discovery: refused with a token too.
    let (fence, record, token) = write_run(&store, 1, "discovery").await;
    let (handle, mut prepared) = scene.prepare(Some(fence), Posture::Discovery).await;
    let writing = Writing {
        token: &token,
        record,
    };
    let failure = scene
        .run(&handle, &mut prepared, act, &inputs, Some(&writing))
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Policy, contract::READ_ONLY_POSTURE.as_str())
    );
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
