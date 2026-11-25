//! Reads through the driver handle, against the simulated device: the
//! device is identified before anything is read, only read frames are
//! sent, every failure has its one class, and nothing is guessed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Digest, DigestPurpose, FailureClass, KeyedDigest, OperationName, RunId, Sensitivity, Value,
    Values, WorkspaceId,
};
use a2m_logs::{
    CaptureLevel, ChoiceReason, PassStart, Phase, StepRecord, TraceBuilder, TraceIdentity,
    TraceOutcome, TracedValue,
};
use a2m_provider::{Destinations, Posture, Sent, StepFailure};
use common::{Planted, Ran, Scene, decimal};
use example_device::{Answer, Corruption, Fault, FaultKind, FrameKind, Scope, Version};

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

/// Carries out the `act` list of a read operation on a link of its own.
async fn read(scene: &Scene, operation: &str) -> Result<Ran, StepFailure> {
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let result = scene
        .run(
            &handle,
            &mut prepared,
            &scene.binding(operation).act,
            &Values::new(),
            None,
        )
        .await;
    assert!(handle.release(prepared, result.is_err()).await.complete);
    scene.links_closed().await;
    result
}

#[tokio::test]
async fn a_read_identifies_the_device_and_sends_only_read_frames() {
    let scene = Scene::start().await;
    let control = scene.device.control();

    let ran = read(&scene, "example_pump_speed_get").await.unwrap();
    assert_eq!(ran.values.get("speed"), Some(&decimal("0")));
    assert_eq!(ran.values.get("running"), Some(&Value::Bool(false)));

    // The stand: identification first, then one frame per register, each a
    // read, each answered, and nothing was written.
    let frames = control.frames();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].function, 0x2B);
    assert!(frames.iter().all(|frame| frame.kind == FrameKind::Read));
    assert!(frames.iter().all(|frame| frame.answer == Answer::Normal));
    assert!(
        control
            .inspect()
            .write_counts
            .values()
            .all(|count| *count == 0)
    );

    // Signed, scaled, two-word, and text values, by the map's layout,
    // against what the device holds.
    let status = read(&scene, "example_status_get").await.unwrap();
    assert_eq!(control.number("temperature"), Some(-35));
    assert_eq!(status.values.get("temperature"), Some(&decimal("-3.5")));
    assert_eq!(control.number("batch_target"), Some(5000));
    assert_eq!(status.values.get("batch"), Some(&Value::Int(5000)));
    assert_eq!(
        status.values.get("model"),
        Some(&Value::Text("example-controller".to_owned()))
    );
    assert!(matches!(status.values.get("uptime"), Some(Value::Int(ticks)) if *ticks > 0));
}

struct NoKey;

impl KeyedDigest for NoKey {
    fn keyed_digest(
        &self,
        workspace: &WorkspaceId,
        purpose: DigestPurpose,
        bytes: &[u8],
    ) -> Digest {
        a2m_core::keyed_digest_under(b"a test key", workspace, purpose, bytes)
    }
}

/// The trace of one run of a read, as the engine would record it: how each
/// step was done is what the driver's `acting` capture says, and the frames
/// it sent are measured.
fn signature(scene: &Scene, run: u8, ran: &Ran) -> Digest {
    let handle = scene.handle();
    let operation = OperationName::new("example_pump_speed_get").unwrap();
    let mut trace = TraceBuilder::new(
        TraceIdentity {
            workspace: WorkspaceId::from_bytes([1; 16]),
            run: RunId::from_bytes([run; 16]),
            attempt: 1,
            operation: operation.clone(),
            compiled_target: Digest::of(b"plan"),
            engine: common::VERSION,
            registry: scene.registry.digest(),
        },
        CaptureLevel::Standard,
        Arc::new(a2m_core::NoSecrets),
        Arc::new(NoKey),
    );
    let mut values = ran.values.iter();
    for (position, kind) in ran.kinds.iter().enumerate() {
        // How the step was done is what the driver said: its `acting`
        // capture, one for each step.
        let acting = Some(ran.acting[position].clone());
        let read = (kind == "expect")
            .then(|| values.next())
            .flatten()
            .map(|(field, value)| {
                TracedValue::read(field, Sensitivity::Internal, value.clone(), value.clone())
            });
        trace.record(StepRecord {
            pass: 0,
            phase: Phase::Act,
            step: u32::try_from(position).unwrap(),
            kind: a2m_core::StepKind::new(kind).unwrap(),
            record: None,
            begins: (position == 0).then(|| PassStart {
                operation: operation.clone(),
                mode: handle.mode().clone(),
                reason: ChoiceReason::FirstInOrder,
                driver: handle.choice().digest,
                queue: Duration::from_millis(u64::from(run)),
                prepare: Duration::from_millis(u64::from(run)),
            }),
            took: Duration::from_millis(u64::from(run)),
            locating: None,
            acting,
            values: read.into_iter().collect(),
            outcome: TraceOutcome::Done,
            captures: Vec::new(),
        });
    }
    trace.finish().unwrap().signature
}

#[tokio::test]
async fn five_runs_of_one_read_give_one_trace_signature() {
    let scene = Scene::start().await;
    // The device's own time is held still, so that the same read finds the
    // same device.
    scene.device.control().set_process_frozen(true);
    let mut signatures = Vec::new();
    let mut first: Option<Ran> = None;
    for run in 1..=5_u8 {
        let ran = read(&scene, "example_pump_speed_get").await.unwrap();
        signatures.push(signature(&scene, run, &ran));
        // The frames sent are the same bytes in every run.
        match &first {
            None => first = Some(ran),
            Some(first) => assert_eq!(first, &ran),
        }
    }
    assert!(signatures.windows(2).all(|pair| pair[0] == pair[1]));
    // Every step said how it was done, and what it said is not a value.
    let first = first.unwrap();
    assert_eq!(first.acting.len(), first.kinds.len());
    let means: Vec<_> = first.acting.iter().map(|step| step.means.clone()).collect();
    assert!(means.iter().all(|step| step[0].name == "step"));
}

#[tokio::test]
async fn a_firmware_version_outside_the_map_stops_a_read() {
    let scene = Scene::start().await;
    let control = scene.device.control();
    control.set_firmware_version(Version::new(2, 5, 0));

    let failure = read(&scene, "example_pump_speed_get").await.unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::UnknownVariant, "device_identity_mismatch")
    );
    assert_eq!(failure.sent, Sent::Answered);
    // The device was asked what it is, and nothing else.
    let frames = control.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].function, 0x2B);

    // A version on the map's list reads.
    control.set_firmware_version(Version::new(2, 4, 1));
    assert!(read(&scene, "example_pump_speed_get").await.is_ok());
}

#[tokio::test]
async fn a_step_that_reads_without_identifying_first_sends_nothing() {
    let scene = Scene::start().await;
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let act = &scene.binding("example_pump_speed_get").act;
    // The list without its `open` step.
    let failure = scene
        .run(&handle, &mut prepared, &act[1..], &Values::new(), None)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::UnknownVariant, "device_identity_mismatch")
    );
    assert_eq!(failure.sent, Sent::NotSent);
    assert!(scene.device.control().frames().is_empty());
    handle.release(prepared, true).await;
}

#[tokio::test]
async fn a_link_that_is_not_declared_is_never_opened() {
    let scene = Scene::start().await;
    let elsewhere = Destinations::new(["192.0.2.7:502".to_owned()]);
    let failure = scene
        .try_prepare(None, Posture::Live, &elsewhere)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Policy, "surface_not_declared")
    );
    assert_eq!(failure.sent, Sent::NotSent);
    let seen = scene.device.control().inspect();
    assert_eq!((seen.open_links, seen.frames), (0, 0));
}

#[tokio::test]
async fn what_the_device_does_to_a_read_is_named() {
    let scene = Scene::start().await;
    let control = scene.device.control();
    let registers = Scope::Function(0x04);

    // An address the device does not have: its own refusal.
    let failure = read(&scene, "example_absent_point_get").await.unwrap_err();
    assert_eq!(
        (class_and_code(&failure), failure.sent),
        ((FailureClass::Rejected, "device_refused"), Sent::Answered)
    );

    // Busy: named, and passed on. Nothing here sends the read again.
    control.clear_frames();
    control.set_fault(Fault::busy().on(registers));
    let failure = read(&scene, "example_pump_speed_get").await.unwrap_err();
    assert_eq!(
        (class_and_code(&failure), failure.sent),
        ((FailureClass::Transient, "device_busy"), Sent::Answered)
    );
    assert_eq!(control.frames().len(), 2);

    // A reply for another request.
    control
        .set_fault(Fault::once(FaultKind::CorruptReply(Corruption::TransactionId)).on(registers));
    let failure = read(&scene, "example_pump_speed_get").await.unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Transient, "frame_corrupt")
    );

    // A reply cut off.
    control.set_fault(Fault::once(FaultKind::CorruptReply(Corruption::Truncated)).on(registers));
    let failure = read(&scene, "example_pump_speed_get").await.unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Transient, "frame_corrupt")
    );

    // The link dropped before the reply.
    control.set_fault(Fault::once(FaultKind::DropLink { applied: false }).on(registers));
    let failure = read(&scene, "example_pump_speed_get").await.unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Transient, "connection_lost")
    );

    // No reply at all, within the time the map allows.
    control.set_fault(Fault::once(FaultKind::NoReply { applied: false }).on(registers));
    let failure = read(&scene, "example_pump_speed_get").await.unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Transient, "timeout")
    );

    // A device that is down accepts the link and closes it.
    control.power_off();
    let failure = read(&scene, "example_pump_speed_get").await.unwrap_err();
    assert_eq!(
        class_and_code(&failure),
        (FailureClass::Transient, "connection_lost")
    );
    control.power_on();
    assert!(read(&scene, "example_pump_speed_get").await.is_ok());
}

#[tokio::test]
async fn a_secret_in_what_the_device_says_reaches_no_value_and_no_failure() {
    // The device's model text stands for a secret the vault knows.
    let scene = Scene::start_with(Arc::new(Planted("example-controller"))).await;
    let ran = read(&scene, "example_status_get").await.unwrap();
    assert_eq!(
        ran.values.get("model"),
        Some(&Value::Text(a2m_core::REDACTED.to_owned()))
    );
    let everything = format!("{ran:?}");
    assert!(!everything.contains("example-controller"), "{everything}");
    for capture in &ran.captures {
        let text = String::from_utf8(capture.bytes.clone()).unwrap();
        // A capture holds the frame sent and the size of the reply, never
        // the reply's content.
        assert!(!text.contains(&hex(b"example-controller")), "{text}");
    }
    // A failure names the map and never quotes the device.
    scene
        .device
        .control()
        .set_firmware_version(Version::new(9, 9, 9));
    let failure = read(&scene, "example_status_get").await.unwrap_err();
    let text = format!(
        "{} {}",
        failure.failure.message(),
        failure.failure.next_step()
    );
    assert!(
        !text.contains("example-controller") && !text.contains("9.9.9"),
        "{text}"
    );
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[tokio::test]
async fn a_driver_in_the_discovery_posture_sends_nothing_but_identification() {
    let scene = Scene::start().await;
    let control = scene.device.control();

    // A read's steps, handed to the driver under the discovery posture:
    // the device is identified, and the register read is refused by the
    // driver itself.
    let (handle, mut prepared) = scene.prepare(None, Posture::Discovery).await;
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
        (class_and_code(&failure), failure.sent),
        (
            (FailureClass::Policy, "discovery_identifies_only"),
            Sent::NotSent
        )
    );
    handle.release(prepared, true).await;
    scene.links_closed().await;

    // The stand: one frame, the identification.
    let frames = control.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].function, 0x2B);
}

#[tokio::test]
async fn a_run_stops_at_its_frame_ceiling() {
    let scene = Scene::start().await;
    let control = scene.device.control();

    // One link, one run: the identification, then two frames for each
    // pass over a read's steps. The default ceiling is one hundred frames.
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let act = &scene.binding("example_pump_speed_get").act;
    let mut stopped = None;
    for _ in 0..60 {
        if let Err(failure) = scene
            .run(&handle, &mut prepared, act, &Values::new(), None)
            .await
        {
            stopped = Some(failure);
            break;
        }
    }
    let failure = stopped.expect("a run sent frames without a count");
    assert_eq!(
        (class_and_code(&failure), failure.sent),
        (
            (FailureClass::Policy, "frame_budget_exceeded"),
            Sent::NotSent
        )
    );
    handle.release(prepared, true).await;
    scene.links_closed().await;

    // The stand: one hundred frames, and not one more.
    assert_eq!(control.frames().len(), 100);
}
