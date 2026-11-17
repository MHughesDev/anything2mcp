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

/// A register map as a vendor publishes it, in the layout the pack reads.
/// Two rows give no readable register: one states no data type, and one
/// an address the protocol cannot hold. A coil and a register that can
/// only be written give no read; a number with no unit gives a read that
/// waits on that question.
const PUBLISHED: &str = "# model: example-controller
# firmware: 2.4.0, 2.4.1
name,table,offset,data,words,word_order,scale,unit,access
Temperature,input,31,i16,,,0.1,°C,read
Dosing rate,holding,101,u16,,,0.01,l/min,read_write
Uptime ticks,input,32,u32,,high_first,,,read
Mystery,input,40,,,,,,read
Far away,input,65535,u32,,,,,read
Inlet valve,coil,4,bit,,,,,read_write
Reset,holding,9,u16,,,,,write
";

#[tokio::test]
async fn a_published_map_drafts_reads_that_read_the_stand_and_send_no_frame_to_draft() {
    use a2m_provider::{Access, RootName, Roots};

    let mut scene = Scene::start().await;
    let control = scene.device.control();
    let project = Folder::new("published");
    std::fs::write(project.path().join("example-map.csv"), PUBLISHED).unwrap();
    let root = RootName::new("project").unwrap();
    let mut roots = Roots::none();
    roots
        .add(root.clone(), project.path(), Access::Read)
        .unwrap();
    let mut paths = CheckedPaths::none();
    paths.insert(
        "example-map.csv",
        roots.check(&root, "example-map.csv", Access::Read).unwrap(),
    );

    // Discovery reads the file, and sends nothing on the link.
    let (handle, prepared) = scene.prepare(None, Posture::Discovery).await;
    let port = ReadOnlyPort::new(handle, prepared);
    let surface = SurfaceDescription {
        kind: SurfaceKind::new("device_link").unwrap(),
        archetype: Archetype::Device,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = scene
        .registry
        .discoverers_for(&surface)
        .into_iter()
        .find(|found| found.declared().name.as_str() == "device_register_map")
        .unwrap();
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(Duration::from_secs(30)),
        paths: &paths,
        redact: &a2m_core::NoSecrets,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    discoverer.discover(&port, &scope, &mut kept).await.unwrap();
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    assert!(control.frames().is_empty());
    let [finding] = kept.0.as_slice() else {
        panic!("one finding, not {}", kept.0.len());
    };
    // What a vendor published: inferred, never discovered.
    assert_eq!(finding.level(), EvidenceLevel::Inferred);
    assert_eq!(finding.source(), Source::PublishedDescription);

    // The rows left out are named by their line and why; the file's own
    // names are kept apart.
    let described = discoverer.describe(&kept.0, DescribeForm::Full);
    assert!(
        described
            .text
            .contains("line 7: left out, because it states no data type")
            && described.text.contains(
                "line 8: left out, because it states an address the protocol cannot hold"
            ),
        "{}",
        described.text
    );
    assert!(!described.text.contains("Mystery"), "{}", described.text);

    // The draft: a map of the readable input and holding registers, each
    // `access: read`, and one read of each.
    let folder = Folder::new("published-draft");
    std::fs::copy(
        common::fixtures().join("target.yaml"),
        folder.path().join("target.yaml"),
    )
    .unwrap();
    std::fs::copy(
        common::fixtures().join("types.yaml"),
        folder.path().join("types.yaml"),
    )
    .unwrap();
    let report = Drafter::new(
        scene.registry.compiler_set(),
        &TargetRoot::new(folder.path()),
    )
    .draft(&kept.0)
    .unwrap();
    let written: Vec<&str> = report
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .collect();
    assert_eq!(
        written,
        [
            "operations/example_map_dosing_rate_get.yaml",
            "operations/example_map_temperature_get.yaml",
            "operations/example_map_uptime_ticks_get.yaml",
            "registers/example_map.yaml",
        ]
    );
    assert!(report.refused.is_empty(), "{:?}", report.refused);
    let map =
        std::fs::read_to_string(folder.path().join("registers").join("example_map.yaml")).unwrap();
    for never in [
        "hazard",
        "range",
        "safe_state",
        "read_write",
        "coil",
        "mystery",
        "far_away",
    ] {
        assert!(!map.contains(never), "{never} in:\n{map}");
    }
    // The one question: the unit of the number whose unit the map does not
    // state, in the read of it alone.
    let questioned: Vec<&str> = report
        .open_questions
        .iter()
        .map(|question| question.file.as_str())
        .collect();
    assert_eq!(questioned, ["operations/example_map_uptime_ticks_get.yaml"]);
    assert!(report.open_questions[0].question.contains("line 6"));

    // Set aside as the scout sets it aside, the rest compiles, and a
    // drafted read returns what the stand holds, through the handle.
    std::fs::remove_file(folder.path().join(questioned[0])).unwrap();
    scene.plan = common::compile_folder(folder.path(), scene.registry.compiler_set());
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let ran = scene
        .run(
            &handle,
            &mut prepared,
            &scene.binding("example_temperature_get").act,
            &Values::new(),
            None,
        )
        .await
        .unwrap();
    let dosing = scene
        .run(
            &handle,
            &mut prepared,
            &scene.binding("example_dosing_rate_get").act,
            &Values::new(),
            None,
        )
        .await
        .unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(control.number("temperature"), Some(-35));
    assert_eq!(
        ran.values.get("temperature"),
        Some(&common::decimal("-3.5"))
    );
    let raw = control.number("dosing_rate").unwrap();
    assert_eq!(
        dosing.values.get("dosing_rate"),
        Some(&a2m_core::Value::Decimal(
            a2m_core::Decimal::from_units(raw, 2).unwrap()
        ))
    );
    // The stand saw identification and reads, and no write frame.
    let frames = control.frames();
    assert!(!frames.is_empty());
    assert!(frames.iter().all(|frame| frame.kind == FrameKind::Read));
    assert!(
        control
            .inspect()
            .write_counts
            .values()
            .all(|count| *count == 0)
    );

    // The device rule stays: a device outside the drafted map stops the read.
    let other = std::fs::read_to_string(folder.path().join("registers").join("example_map.yaml"))
        .unwrap()
        .replace("\"2.4.0\"", "\"9.9.9\"")
        .replace("\"2.4.1\"", "\"9.9.8\"");
    std::fs::write(
        folder.path().join("registers").join("example_map.yaml"),
        other,
    )
    .unwrap();
    scene.plan = common::compile_folder(folder.path(), scene.registry.compiler_set());
    let (handle, mut prepared) = scene.prepare(None, Posture::Live).await;
    let stopped = scene
        .run(
            &handle,
            &mut prepared,
            &scene.binding("example_temperature_get").act,
            &Values::new(),
            None,
        )
        .await
        .unwrap_err();
    handle.release(prepared, true).await;
    assert_eq!(stopped.failure.code().as_str(), "device_identity_mismatch");
}
