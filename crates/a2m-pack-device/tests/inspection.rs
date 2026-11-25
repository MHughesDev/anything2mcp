//! Firmware inspection through the optional provider: an image given as a
//! file is read and described, what it holds is inferred, nothing is sent
//! to a device, nothing is decrypted, and the provider is off unless it is
//! added.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{
    AccessKind, Digest, EvidenceLevel, Failure, FailureClass, Finding, Source, Subject,
    SurfaceKind, Values,
};
use a2m_provider::{
    Access, CheckedPaths, Deadline, DescribeForm, DiscoveryScope, FindingSink, Manifest, Posture,
    ReadOnlyPort, RootName, Roots, SurfaceDescription,
};
use common::{Folder, Scene, registry, registry_inspecting};
use example_device::Version;
use example_device::firmware::{MODEL_CODE, build_image};

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

fn surface(scene: &Scene) -> SurfaceDescription {
    SurfaceDescription {
        kind: SurfaceKind::new("device_link").unwrap(),
        access_kind: AccessKind::Device,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    }
}

#[test]
fn the_provider_is_off_unless_it_is_added_and_declares_what_it_raises() {
    let surface = SurfaceDescription {
        kind: SurfaceKind::new("device_link").unwrap(),
        access_kind: AccessKind::Device,
        address: "127.0.0.1:502".to_owned(),
        facts: Values::new(),
    };
    let folder = Folder::new("inspection-off");
    let off = registry(&folder);
    let names = |registry: &a2m_provider::Registry| -> Vec<String> {
        registry
            .discoverers_for(&surface)
            .iter()
            .map(|found| found.declared().name.as_str().to_owned())
            .collect()
    };
    assert!(!names(&off).contains(&"firmware_image".to_owned()));
    assert_eq!(off.listing().len(), 1);

    let on = registry_inspecting(&folder);
    assert!(names(&on).contains(&"firmware_image".to_owned()));
    assert_eq!(on.listing().len(), 2);
    let declared = on
        .discoverers_for(&surface)
        .into_iter()
        .find(|found| found.declared().name.as_str() == "firmware_image")
        .unwrap();
    // At most inferred, from a file, with no frame and no network.
    assert_eq!(declared.declared().max_level, EvidenceLevel::Inferred);
    assert_eq!(
        declared.declared().files,
        [a2m_pack_device::FIRMWARE_IMAGE_FILE]
    );
    let manifest = Manifest::parse(a2m_pack_device::inspect::MANIFEST).unwrap();
    assert!(!manifest.effects.devices);
    let mut own = a2m_pack_device::inspect::codes::ALL.to_vec();
    let mut listed: Vec<(FailureClass, &str)> = manifest
        .failure_codes
        .iter()
        .map(|code| (code.class, code.code.as_str()))
        .collect();
    own.sort_by_key(|(_, code)| *code);
    listed.sort_by_key(|(_, code)| *code);
    assert_eq!(own, listed);
}

/// Hands one file to the discoverer and discovers.
async fn inspect(
    scene: &Scene,
    registry: &a2m_provider::Registry,
    file: &str,
    under: &str,
    project: &Folder,
) -> (
    Result<a2m_provider::DiscoveryReport, Failure>,
    Vec<Finding>,
    String,
) {
    let surface = surface(scene);
    let discoverer = registry
        .discoverers_for(&surface)
        .into_iter()
        .find(|found| found.declared().name.as_str() == "firmware_image")
        .unwrap();
    let root = RootName::new("sources").unwrap();
    let mut roots = Roots::none();
    roots
        .add(root.clone(), project.path(), Access::Read)
        .unwrap();
    let mut paths = CheckedPaths::none();
    paths.insert(under, roots.check(&root, file, Access::Read).unwrap());
    let (handle, prepared) = scene.prepare(None, Posture::Discovery).await;
    let port = ReadOnlyPort::new(handle, prepared);
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
    let report = discoverer.discover(&port, &scope, &mut kept).await;
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    let described = discoverer.describe(&kept.0, DescribeForm::Full);
    (
        report,
        kept.0,
        format!("{}\n{}", described.text, described.untrusted.join("\n")),
    )
}

#[tokio::test]
async fn an_image_given_as_a_file_is_read_and_described_as_inferred_and_nothing_is_sent() {
    let scene = Scene::start().await;
    let control = scene.device.control();
    let folder = Folder::new("inspection-registry");
    let registry = registry_inspecting(&folder);
    let project = Folder::new("inspection-project");
    let payload = b"mode=normal\n# a note\nrate_limit = 10\napi_token=hunter2\nnot a setting\n";
    let bytes = build_image(MODEL_CODE, Version::new(2, 5, 0), payload);
    std::fs::write(project.path().join("image.bin"), &bytes).unwrap();

    let (report, found, described) = inspect(
        &scene,
        &registry,
        "image.bin",
        a2m_pack_device::FIRMWARE_IMAGE_FILE,
        &project,
    )
    .await;
    report.unwrap();
    let [finding] = found.as_slice() else {
        panic!("one finding, not {}", found.len());
    };
    // Inferred, never discovered; from a file; about the image by digest.
    assert_eq!(finding.level(), EvidenceLevel::Inferred);
    assert_eq!(finding.source(), Source::Artifact);
    assert_eq!(finding.kind().as_str(), "firmware_image");
    match finding.subject() {
        Subject::File { digest, .. } => assert_eq!(*digest, Digest::of(&bytes)),
        other => panic!("not the image: {other:?}"),
    }
    // Regions and settings; the secret-named setting is listed with no
    // value, and the value is nowhere.
    let inspected: serde_json::Value = serde_json::from_str(finding.found()).unwrap();
    assert_eq!(inspected["version"], "2.5.0");
    assert_eq!(inspected["checksum_holds"], true);
    let regions: Vec<&str> = inspected["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|region| region["name"].as_str().unwrap())
        .collect();
    assert_eq!(regions, ["head", "payload", "tail"]);
    let settings = inspected["settings"].as_array().unwrap();
    assert_eq!(settings.len(), 3);
    assert_eq!(settings[2]["key"], "api_token");
    assert!(
        settings[2].get("value").is_none_or(|v| v.is_null()),
        "{settings:?}"
    );
    assert!(!finding.found().contains("hunter2"));
    assert!(!described.contains("hunter2"));
    // The image's own words are kept apart in the description.
    assert!(described.contains("line 1: mode=normal"), "{described}");
    assert!(described.contains("1 withheld"), "{described}");

    // The image is read and never changed, and the device was not asked
    // anything: no frame, not even identification.
    assert_eq!(
        std::fs::read(project.path().join("image.bin")).unwrap(),
        bytes
    );
    assert!(control.frames().is_empty());
    assert_eq!(control.firmware().update_count, 1);
}

#[tokio::test]
async fn an_image_of_another_form_or_under_another_name_is_not_read_and_a_protected_one_stops() {
    let scene = Scene::start().await;
    let folder = Folder::new("inspection-registry");
    let registry = registry_inspecting(&folder);
    let project = Folder::new("inspection-other");
    std::fs::write(project.path().join("plain.txt"), "not an image of any form").unwrap();
    let random: Vec<u8> = (0..2048_u32)
        .map(|n| u8::try_from(n.wrapping_mul(2_654_435_761) >> 13 & 0xFF).unwrap())
        .collect();
    std::fs::write(project.path().join("sealed.bin"), &random).unwrap();
    let good = build_image(MODEL_CODE, Version::new(2, 5, 0), b"mode=normal");
    std::fs::write(project.path().join("image.bin"), &good).unwrap();

    // A form this does not read is not made into a finding.
    let (report, found, _) = inspect(
        &scene,
        &registry,
        "plain.txt",
        a2m_pack_device::FIRMWARE_IMAGE_FILE,
        &project,
    )
    .await;
    assert_eq!((report.unwrap().dropped, found.len()), (1, 0));

    // Under another name, nothing is read.
    let (report, found, _) = inspect(&scene, &registry, "image.bin", "other_name", &project).await;
    report.unwrap();
    assert!(found.is_empty());

    // An image that looks encrypted is a named stop. Nothing is decrypted
    // or looked for.
    let (report, found, _) = inspect(
        &scene,
        &registry,
        "sealed.bin",
        a2m_pack_device::FIRMWARE_IMAGE_FILE,
        &project,
    )
    .await;
    let failure = report.unwrap_err();
    assert_eq!(
        (failure.class(), failure.code().as_str()),
        (FailureClass::EdgeBlocked, "image_protected")
    );
    assert!(found.is_empty());
    assert_eq!(
        std::fs::read(project.path().join("sealed.bin")).unwrap(),
        random
    );
}

#[tokio::test]
async fn a_damaged_image_is_read_and_says_its_checksum_does_not_hold() {
    let scene = Scene::start().await;
    let folder = Folder::new("inspection-registry");
    let registry = registry_inspecting(&folder);
    let project = Folder::new("inspection-damaged");
    let mut bytes = build_image(MODEL_CODE, Version::new(2, 5, 0), b"mode=normal");
    bytes[16] ^= 1;
    std::fs::write(project.path().join("image.bin"), &bytes).unwrap();
    let (report, found, described) = inspect(
        &scene,
        &registry,
        "image.bin",
        a2m_pack_device::FIRMWARE_IMAGE_FILE,
        &project,
    )
    .await;
    report.unwrap();
    assert_eq!(found.len(), 1);
    assert!(described.contains("checksum does not hold"), "{described}");
}
