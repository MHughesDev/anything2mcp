//! What the scout learns of a Windows program from what it publishes about
//! itself (decision 0318): a COM type library a person hands it, read as
//! bytes on every system and never loaded by the system, and the definition
//! of a management class, asked of the system's management service on
//! Windows and reported as an unsupported system everywhere else. A
//! property get of a type library and a property of a class definition
//! that is not marked writable are drafted as reads that compile; this
//! version of the pack does not carry them out, which a test holds as a
//! named stop. The test of the management service itself runs on Windows
//! only, where CI runs it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{
    AccessKind, EvidenceLevel, Finding, NoSecrets, Source, SurfaceKind, Timestamp, Values,
};
use a2m_provider::{
    Access, CheckedPaths, Deadline, DiscoveryReport, DiscoveryScope, FindingSink, Posture,
    ReadOnlyPort, RootName, Roots, SurfaceDescription,
};
use common::{Folder, Scene, every_file};

const WITHIN: Duration = Duration::from_secs(20);

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), a2m_core::Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// A discoverer of the pack, run against the scene's session, handed the
/// type library `bytes` (when there are any) as `library.tlb` of a
/// `inputs` folder of the test's own.
async fn discover(
    scene: &Scene,
    discoverer: &str,
    library: Option<&[u8]>,
) -> (Result<DiscoveryReport, a2m_core::Failure>, Vec<Finding>) {
    let description = SurfaceDescription {
        kind: SurfaceKind::new("desktop").unwrap(),
        access_kind: AccessKind::Interface,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = scene
        .registry
        .discoverers_for(&description)
        .into_iter()
        .find(|found| found.declared().name.as_str() == discoverer)
        .expect("the pack registers the discoverer");
    let sources = Folder::new("inputs");
    let root = RootName::new("inputs").unwrap();
    let mut roots = Roots::none();
    roots
        .add(root.clone(), sources.path(), Access::Read)
        .unwrap();
    let mut paths = CheckedPaths::none();
    if let Some(bytes) = library {
        std::fs::write(sources.path().join("library.tlb"), bytes).unwrap();
        paths.insert(
            "type_library",
            roots.check(&root, "library.tlb", Access::Read).unwrap(),
        );
    }
    let (handle, prepared) = scene.prepare(Posture::Discovery).await.unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(WITHIN),
        paths: &paths,
        redact: &NoSecrets,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    let said = discoverer.discover(&port, &scope, &mut kept).await;
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    (said, kept.0)
}

#[tokio::test]
async fn a_type_library_is_read_from_its_bytes_and_each_member_says_how_it_is_invoked() {
    let Some(scene) = Scene::start("typelib").await else {
        return;
    };
    let bytes = common::typelib::example_typelib();
    let (said, findings) = discover(&scene, "desktop_com", Some(&bytes)).await;
    said.unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    let library = &findings[0];
    assert_eq!(library.kind().as_str(), "com_type_library");
    assert_eq!(library.level(), EvidenceLevel::Inferred);
    assert_eq!(library.source(), Source::Artifact);
    let found = library.found();
    for stated in [
        r#""name":"ExampleDeskLib""#,
        r#""version":"1.0""#,
        r#""kind":"dispatch""#,
        r#""name":"Orders""#,
        // Each member, with how the library declares it to be invoked.
        r#""invoke":"property_get","memid":1,"name":"Total""#,
        r#""invoke":"property_get","memid":2,"name":"Label""#,
        r#""invoke":"property_put","memid":2,"name":"Label""#,
        r#""invoke":"function","memid":3,"name":"Remove""#,
        r#""returns":{"type":"Long","vt":3}"#,
        r#""returns":{"type":"String","vt":8}"#,
    ] {
        assert!(found.contains(stated), "{stated} not in {found}");
    }
    // Nothing was asked of the application.
    assert!(!scene.desk().truth().acted());
}

#[tokio::test]
async fn a_type_library_of_an_unknown_format_is_a_named_stop() {
    let Some(scene) = Scene::start("typelib-unknown").await else {
        return;
    };
    for (bytes, why) in [
        (b"SLTG\x01\x02\x03\x04".to_vec(), "SLTG"),
        (b"MZ\x90\x00\x03\x00\x00\x00".to_vec(), "program file"),
        (b"\x7fELF\x02\x01\x01".to_vec(), "MSFT form"),
        (b"just some text".to_vec(), "MSFT form"),
    ] {
        let (said, findings) = discover(&scene, "desktop_com", Some(&bytes)).await;
        let failure = said.unwrap_err();
        assert_eq!(
            failure.code().as_str(),
            "type_library_format_unknown",
            "{}",
            failure.message()
        );
        assert!(failure.message().contains(why), "{}", failure.message());
        assert!(findings.is_empty());
    }
    // The MSFT form that does not hold together is another stop.
    let whole = common::typelib::example_typelib();
    let (said, findings) = discover(&scene, "desktop_com", Some(&whole[..100])).await;
    assert_eq!(said.unwrap_err().code().as_str(), "finding_invalid");
    assert!(findings.is_empty());
}

/// A target folder whose surface states the management class, with the
/// drafts of `findings` written into it.
fn drafted(scene: &Scene, findings: &[Finding], class: Option<&str>) -> (Folder, Vec<String>) {
    let root = Folder::new("drafted");
    let mut target = std::fs::read_to_string(common::fixture().join("target.yaml"))
        .unwrap()
        .replace(
            "/opt/example/bin/example-desk",
            &scene.program.replace('\\', "\\\\"),
        );
    if let Some(class) = class {
        target = target.replace(
            "    service: org.example.Desk\n",
            &format!("    service: org.example.Desk\n    management_class: {class}\n"),
        );
    }
    std::fs::write(root.path().join("target.yaml"), target).unwrap();
    let report = a2m_files::Drafter::new(
        scene.registry.compiler_set(),
        &a2m_files::TargetRoot::new(root.path()),
    )
    .draft(findings)
    .unwrap();
    assert!(
        report.refused.is_empty() && report.conflicts.is_empty() && report.held.is_empty(),
        "{report:?}"
    );
    assert!(report.kept_failing.is_empty(), "{report:?}");
    let written = report
        .written
        .iter()
        .map(|path| path.as_str().to_owned())
        .collect();
    (root, written)
}

#[tokio::test]
async fn a_property_get_in_a_type_library_is_drafted_as_a_read_and_a_function_is_not() {
    let Some(mut scene) = Scene::start("typelib-draft").await else {
        return;
    };
    let bytes = common::typelib::example_typelib();
    let (said, findings) = discover(&scene, "desktop_com", Some(&bytes)).await;
    said.unwrap();
    let (root, written) = drafted(&scene, &findings, None);
    assert_eq!(
        written,
        [
            "operations/orders_label_get.yaml",
            "operations/orders_total_get.yaml"
        ]
    );
    for (path, text) in every_file(root.path()) {
        if !path.to_string_lossy().contains("operations") {
            continue;
        }
        assert!(text.contains(r#"verb: "read""#), "{text}");
        assert!(text.contains(r#"effect: "read""#), "{text}");
        assert!(text.contains("com:"), "{text}");
        for never in [
            "Remove",
            "update",
            "upsert",
            "action",
            "commits",
            "property_put",
        ] {
            assert!(!text.contains(never), "`{never}` in {text}");
        }
    }
    // They compile, and they are reads. This version carries out no such
    // step, which is a named stop with nothing sent.
    scene.compiled_target = match common::compile_at(&scene.registry, root.path()) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("the drafts do not compile:\n{}", common::shown(&problems)),
    };
    scene.surface = common::surface_of(&scene.compiled_target, &scene.program);
    let binding = scene.binding("example_orders_total_get");
    let kinds: Vec<&str> = binding.act.iter().map(|step| step.kind.as_str()).collect();
    assert_eq!(kinds, ["com_get"]);
    assert!(binding.act.iter().all(|step| !step.commits));
    let stopped = scene
        .read("example_orders_total_get", WITHIN)
        .await
        .unwrap_err();
    assert_eq!(stopped.failure.code().as_str(), "step_not_built");
}

/// A finding of a management class's definition, as the discoverer writes
/// it on Windows, made by hand so that the drafting is proven on every
/// system.
fn class_finding(scene: &Scene) -> Finding {
    let found = serde_json::json!({
        "format": "1",
        "from": "management_schema",
        "program": scene.program,
        "namespace": "ROOT\\EXAMPLE",
        "class": "Example_Counter",
        "singleton": true,
        "properties": [
            {"name": "Total", "type": "uint32", "array": false, "access": "read"},
            {"name": "Label", "type": "string", "array": false, "access": "readwrite"},
            {"name": "Names", "type": "string", "array": true, "access": "read"},
            {"name": "AdminPassword", "type": "string", "array": false, "access": "read"},
        ],
        "methods": ["Reset"],
    });
    Finding::about(
        a2m_core::Subject::Address(
            a2m_core::Address::new("desktop://desk", "/management/class").unwrap(),
        ),
        a2m_core::FindingKind::new("management_class").unwrap(),
        found.to_string(),
        Source::LiveSystem,
        a2m_core::ProducedBy {
            provider: "desktop".to_owned(),
            version: "0.0.1".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(1_800_000_000_000),
    )
    .level(EvidenceLevel::Discovered)
    .coverage(a2m_core::Coverage::Complete)
    .build()
    .unwrap()
}

#[tokio::test]
async fn a_property_marked_read_only_in_a_schema_is_drafted_as_a_read_and_never_as_a_write() {
    let Some(mut scene) = Scene::start("management-draft").await else {
        return;
    };
    let finding = class_finding(&scene);
    // A surface that does not state the class gets no draft.
    let (_, none) = drafted(&scene, std::slice::from_ref(&finding), None);
    assert!(none.is_empty(), "{none:?}");
    let (root, written) = drafted(
        &scene,
        std::slice::from_ref(&finding),
        Some("Example_Counter"),
    );
    // `Total` is not marked writable and is a number; `Label` is writable;
    // `Names` is an array; `AdminPassword` is named as a secret.
    assert_eq!(written, ["operations/example_counter_total_get.yaml"]);
    for (path, text) in every_file(root.path()) {
        if !path.to_string_lossy().contains("operations") {
            continue;
        }
        assert!(text.contains(r#"verb: "read""#), "{text}");
        assert!(text.contains("management:"), "{text}");
        for never in [
            "Label",
            "Names",
            "Reset",
            "AdminPassword",
            "update",
            "action",
        ] {
            if never == "AdminPassword" {
                // Its name is said once, as what was left out, in the
                // snake form the compiler judges.
                assert!(!text.contains(never), "`{never}` in {text}");
                assert!(text.contains("admin_password"), "{text}");
            } else {
                assert!(!text.contains(never), "`{never}` in {text}");
            }
        }
    }
    scene.compiled_target = match common::compile_at(&scene.registry, root.path()) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("the drafts do not compile:\n{}", common::shown(&problems)),
    };
    scene.surface = common::surface_of(&scene.compiled_target, &scene.program);
    let binding = scene.binding("example_example_counter_total_get");
    let kinds: Vec<&str> = binding.act.iter().map(|step| step.kind.as_str()).collect();
    assert_eq!(kinds, ["management_get"]);
    let stopped = scene
        .read("example_example_counter_total_get", WITHIN)
        .await
        .unwrap_err();
    assert_eq!(stopped.failure.code().as_str(), "step_not_built");
}

/// On every system but Windows the management service is not asked: the
/// discoverer reports an unsupported system, a named stop, and stores
/// nothing.
#[cfg(not(windows))]
#[tokio::test]
async fn a_management_class_is_reported_as_an_unsupported_system_off_windows() {
    let Some(scene) = Scene::start("management-off").await else {
        return;
    };
    // No class stated: nothing is asked and nothing is stored.
    let (said, findings) = discover(&scene, "desktop_management", None).await;
    said.unwrap();
    assert!(findings.is_empty());
}

#[cfg(not(windows))]
#[tokio::test]
async fn a_management_class_a_surface_names_stops_by_name_off_windows() {
    let Some(scene) = Scene::with_management_class("management-named", "Example_Counter").await
    else {
        return;
    };
    let (said, findings) = discover(&scene, "desktop_management", None).await;
    let failure = said.unwrap_err();
    assert_eq!(failure.code().as_str(), "system_not_built");
    assert!(
        failure.message().contains("only on Windows"),
        "{}",
        failure.message()
    );
    assert!(findings.is_empty());
}

/// On Windows, a namespace of another machine is refused by the driver
/// itself, before the management service is asked anything: the review
/// withdraws the binding's statement, and the driver fails closed as well.
#[cfg(windows)]
#[tokio::test]
async fn a_management_namespace_of_another_machine_is_refused_by_the_driver() {
    let Some(scene) = Scene::with_surface_lines(
        "management-remote",
        "    management_class: Example_Counter\n    management_namespace: '//other/root'\n",
    )
    .await
    else {
        return;
    };
    let (said, findings) = discover(&scene, "desktop_management", None).await;
    let failure = said.unwrap_err();
    assert_eq!(
        (failure.class(), failure.code().as_str()),
        (a2m_core::FailureClass::Policy, "remote_namespace_refused")
    );
    assert!(findings.is_empty());
}
