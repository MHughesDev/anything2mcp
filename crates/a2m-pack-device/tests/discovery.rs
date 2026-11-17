//! Discovery on a device link: the device is asked what it is, nothing is
//! written, read, or changed, and the draft made from what it said holds
//! nothing the device did not state.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{
    Archetype, EvidenceLevel, Failure, FailureClass, Finding, Source, Subject, SurfaceKind, Values,
};
use a2m_files::{Drafter, TargetRoot};
use a2m_provider::{
    CheckedPaths, Deadline, DescribeForm, DiscoveryScope, FindingSink, Posture, ReadOnlyPort,
    SandboxPort, Sent, StepInput, SurfaceDescription,
};
use common::{Folder, Scene};
use example_device::FrameKind;

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

#[tokio::test]
async fn discovery_identifies_the_device_and_changes_nothing() {
    let scene = Scene::start().await;
    let control = scene.device.control();
    control.set_process_frozen(true);
    let before = control.inspect();

    let (handle, prepared) = scene.prepare(None, Posture::Discovery).await;
    let port = ReadOnlyPort::new(handle, prepared);
    let surface = SurfaceDescription {
        kind: SurfaceKind::new("device_link").unwrap(),
        archetype: Archetype::Device,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = scene.registry.discoverers_for(&surface).remove(0);
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(Duration::from_secs(30)),
        paths: &CheckedPaths::none(),
        redact: &a2m_core::NoSecrets,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    discoverer.discover(&port, &scope, &mut kept).await.unwrap();

    // One finding, about the device by what it reports, observed live.
    let [finding] = kept.0.as_slice() else {
        panic!("one finding, not {}", kept.0.len());
    };
    assert_eq!(finding.level(), EvidenceLevel::Discovered);
    assert_eq!(finding.source(), Source::LiveSystem);
    assert_eq!(
        finding.subject(),
        &Subject::Device {
            reported_id: "100123".to_owned(),
            model: "example-controller".to_owned(),
            firmware: "2.4.0".to_owned(),
        }
    );

    // The stand: one frame, the request to identify. No register was read,
    // nothing was written, and the device holds what it held.
    let frames = control.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(
        (frames[0].function, frames[0].kind),
        (0x2B, FrameKind::Read)
    );
    let after = control.inspect();
    assert_eq!(after.points, before.points);
    assert_eq!(after.outputs, before.outputs);
    assert_eq!(after.firmware, before.firmware);
    assert!(after.write_counts.values().all(|count| *count == 0));

    // A write sent through the port never reaches the device.
    let write = scene
        .binding("example_dosing_rate_set")
        .act
        .iter()
        .find(|step| step.commits)
        .unwrap();
    let args = Values::new().with("dosing_rate", common::decimal("3"));
    let input = StepInput::new(&args, Deadline::after(Duration::from_secs(5)));
    let refused = port.act(write, &input).await.unwrap_err();
    assert_eq!(
        (refused.failure.class(), refused.sent),
        (FailureClass::Policy, Sent::NotSent)
    );
    assert_eq!(control.frames().len(), 1);

    // What the device said about itself is kept apart from our own text.
    let described = discoverer.describe(&kept.0, DescribeForm::Full);
    assert!(!described.text.contains("example-controller"));
    assert!(
        described
            .untrusted
            .iter()
            .any(|text| text.contains("example-controller"))
    );

    // The draft: a map with the model and the firmware version the device
    // reported, and no register, range, hazard, or unit.
    let folder = Folder::new("draft");
    std::fs::copy(
        common::fixtures().join("target.yaml"),
        folder.path().join("target.yaml"),
    )
    .unwrap();
    let root = TargetRoot::new(folder.path());
    let report = Drafter::new(scene.registry.compiler_set(), &root)
        .draft(&kept.0)
        .unwrap();
    let written: Vec<&str> = report
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .collect();
    assert_eq!(written, ["registers/example_controller.yaml"]);
    assert!(report.kept_failing.is_empty(), "{:?}", report.kept_failing);
    assert!(report.open_questions.is_empty() && report.not_covered.is_empty());
    let text = std::fs::read_to_string(
        folder
            .path()
            .join("registers")
            .join("example_controller.yaml"),
    )
    .unwrap();
    assert!(
        text.contains("example-controller") && text.contains("2.4.0"),
        "{text}"
    );
    for never in ["hazard", "range", "access", "safe_state", "unit", "scale"] {
        assert!(!text.contains(never), "{never} in:\n{text}");
    }
    // The same finding gives the same bytes.
    Drafter::new(scene.registry.compiler_set(), &root)
        .draft(&kept.0)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(
            folder
                .path()
                .join("registers")
                .join("example_controller.yaml")
        )
        .unwrap(),
        text
    );
}
