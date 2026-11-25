//! What the scout learns from a service the application keeps on the
//! session bus (decision 0313), against a service of the stand's own on a
//! session bus of the test's own. The stand records every member it is
//! asked and every program the bus starts: the tests ask the stand, and
//! never the pack, whether anything was called or started. Linux only: the
//! bus is Linux's.

#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{
    AccessKind, EvidenceLevel, Finding, NoSecrets, Redact, Source, SurfaceKind, Values,
};
use a2m_provider::{
    Access, CheckedPaths, Deadline, DiscoveryReport, DiscoveryScope, FindingSink, Posture,
    ReadOnlyPort, RootName, Roots, SurfaceDescription,
};
use common::{Scene, every_file};
use example_desk::{ADMIN_PASSWORD, Calls, IDLE_NAME, NUMBER, Service};

const WITHIN: Duration = Duration::from_secs(20);

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), a2m_core::Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// A redactor that knows one planted secret.
struct Planted(&'static str);

impl Redact for Planted {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, a2m_core::REDACTED)
    }
}

/// The pack's discoverer of services, run against the scene's session,
/// handed `files`: each a name and the bytes of a file put in an `inputs`
/// folder of the test's own.
async fn discover(
    scene: &Scene,
    files: &[(&str, &str, &[u8])],
    redact: &dyn Redact,
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
        .find(|found| found.declared().name.as_str() == "desktop_service")
        .expect("the pack registers a discoverer of services");
    let sources = scene.folder.path().join("inputs");
    std::fs::create_dir_all(&sources).unwrap();
    let root = RootName::new("inputs").unwrap();
    let mut roots = Roots::none();
    roots.add(root.clone(), &sources, Access::Read).unwrap();
    let mut paths = CheckedPaths::none();
    for (name, file, bytes) in files {
        std::fs::write(sources.join(file), bytes).unwrap();
        paths.insert(*name, roots.check(&root, file, Access::Read).unwrap());
    }
    let (handle, prepared) = scene.prepare(Posture::Discovery).await.unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(WITHIN),
        paths: &paths,
        redact,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    let said = discoverer.discover(&port, &scope, &mut kept).await;
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    (said, kept.0)
}

#[tokio::test]
async fn a_running_service_is_read_by_introspection_and_no_member_of_it_is_called() {
    let Some(scene) = Scene::start("service").await else {
        return;
    };
    let service = Service::start(&scene.session).await.unwrap();
    let (said, findings) = discover(&scene, &[], &NoSecrets).await;
    said.unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    let live = &findings[0];
    assert_eq!(live.kind().as_str(), "dbus_service");
    assert_eq!(live.level(), EvidenceLevel::Discovered);
    assert_eq!(live.source(), Source::LiveSystem);
    let found = live.found();
    for stated in [
        r#""running":true"#,
        r#""path":"/org/example/Desk""#,
        r#""path":"/org/example/Desk/Orders/A100""#,
        r#""name":"org.example.Desk.Orders""#,
        r#""name":"Count""#,
        r#""name":"Changed""#,
        // The access each property's document states, carried.
        r#""access":"read","name":"Total","type":"u""#,
        r#""access":"readwrite","name":"Label","type":"s""#,
        r#""access":"read","name":"Number","type":"s""#,
    ] {
        assert!(found.contains(stated), "{stated} not in {found}");
    }
    // The bus's own interfaces are counted, not listed.
    assert!(
        !found.contains("org.freedesktop.DBus.Properties"),
        "{found}"
    );
    // Names and types, never a value.
    for value in [ADMIN_PASSWORD, NUMBER, "Open orders"] {
        assert!(!found.contains(value), "{value} in {found}");
    }
    // The stand was asked no method, no property was read or set: what was
    // asked was the introspection the bus library answers.
    assert_eq!(service.calls(), Calls::default());
    assert!(!scene.desk().truth().acted());
}

#[tokio::test]
async fn a_service_that_is_not_running_is_reported_and_never_started() {
    let Some(scene) = Scene::with_service("idle", IDLE_NAME).await else {
        return;
    };
    let idle = scene.session.idle_service();
    assert_eq!(idle.name, IDLE_NAME);
    let (said, findings) = discover(&scene, &[], &NoSecrets).await;
    let report = said.unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    let found = findings[0].found();
    assert!(found.contains(r#""running":false"#), "{found}");
    assert!(found.contains(r#""objects":[]"#), "{found}");
    assert_eq!(report.coverage, a2m_core::Coverage::Partial);
    assert_eq!(findings[0].coverage(), a2m_core::Coverage::Partial);
    // The bus was told not to start it: nothing started.
    assert!(!idle.started(), "the program of the idle service ran");

    // The control: the same question with no such flag does start it, so
    // the check above could have failed.
    let bus = zbus::connection::Builder::address(scene.session.address())
        .build()
        .await
        .unwrap();
    // Not waited for to its end: the bus answers only after its activation
    // timeout, since the stand's program owns no name.
    let asked = bus.call_method(
        Some(IDLE_NAME),
        "/",
        Some("org.freedesktop.DBus.Introspectable"),
        "Introspect",
        &(),
    );
    let _ = tokio::time::timeout(Duration::from_millis(250), asked).await;
    let deadline = tokio::time::Instant::now() + WITHIN;
    while !idle.started() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(idle.started(), "the stand's idle service cannot be started");
}

#[tokio::test]
async fn input_files_are_read_as_text_and_start_nothing() {
    let Some(scene) = Scene::with_service("files", IDLE_NAME).await else {
        return;
    };
    let idle = scene.session.idle_service();
    let secret = "secret-for-tests-service-argument";
    let service_file = format!(
        "[D-BUS Service]\nName={IDLE_NAME}\nExec={} --token={secret}\n",
        idle.service_file.display()
    );
    let interface = br#"<node>
  <interface name="org.example.Desk.Orders">
    <property name="Total" type="u" access="read"/>
    <property name="Label" type="s" access="readwrite"/>
    <method name="Count"><arg type="u" direction="out"/></method>
  </interface>
</node>"#;
    let (said, findings) = discover(
        &scene,
        &[
            ("service_file", "idle.service", service_file.as_bytes()),
            ("interface_file", "orders.xml", interface),
        ],
        &Planted(secret),
    )
    .await;
    said.unwrap();
    // The live question (not running), and one finding for each file.
    assert_eq!(findings.len(), 3, "{findings:?}");
    let files: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.source() == Source::Artifact)
        .collect();
    assert_eq!(files.len(), 2);
    for file in &files {
        assert_eq!(file.level(), EvidenceLevel::Inferred);
    }
    let all: String = findings.iter().map(Finding::found).collect();
    assert!(
        all.contains(&format!(r#""service":"{IDLE_NAME}""#)),
        "{all}"
    );
    assert!(all.contains(r#""starts":"#), "{all}");
    assert!(
        all.contains(r#""access":"read","name":"Total","type":"u""#),
        "{all}"
    );
    assert!(
        all.contains(r#""access":"readwrite","name":"Label","type":"s""#),
        "{all}"
    );
    // The program's argument is kept by nothing, and what was planted is
    // in no finding.
    assert!(!all.contains(secret), "{all}");
    assert!(!all.contains("--token"), "{all}");
    // Reading a file starts nothing it names.
    assert!(!idle.started());

    // A file that is not one of the kinds is a named stop, with nothing
    // asked of the bus.
    let (said, findings) = discover(
        &scene,
        &[(
            "interface_file",
            "orders.xml",
            b"<!DOCTYPE x [<!ENTITY a 'b'>]><node/>",
        )],
        &NoSecrets,
    )
    .await;
    let failure = said.unwrap_err();
    assert!(
        failure.message().contains("`interface_file`"),
        "{}",
        failure.message()
    );
    assert!(findings.is_empty());
    assert!(!idle.started());
}

#[tokio::test]
async fn a_property_stated_read_only_is_drafted_as_a_read_and_never_as_a_write() {
    let Some(mut scene) = Scene::start("drafted-property").await else {
        return;
    };
    let service = Service::start(&scene.session).await.unwrap();
    let (said, findings) = discover(&scene, &[], &NoSecrets).await;
    said.unwrap();
    let (root, report) = scene.draft(&findings);
    let written: Vec<&str> = report.written.iter().map(|path| path.as_str()).collect();
    // The two properties that state `read`, of a form the step gives as
    // text. Not `Label` (it states `readwrite`), not `AdminPassword` (named as a
    // secret), not `Level` (not text), no method, no signal.
    assert_eq!(
        written,
        [
            "operations/order_number_get.yaml",
            "operations/orders_total_get.yaml"
        ],
        "{report:?}"
    );
    assert!(
        report.refused.is_empty() && report.conflicts.is_empty() && report.held.is_empty(),
        "{report:?}"
    );
    for (path, text) in every_file(root.path()) {
        if !path.to_string_lossy().contains("operations") {
            continue;
        }
        for never in [
            "Label",
            "Count",
            "Remove",
            "AdminPassword",
            "Level",
            ADMIN_PASSWORD,
            "update",
            "upsert",
            "effect: write",
            "action",
            "commits",
        ] {
            assert!(
                !text.contains(never),
                "`{never}` in {}:\n{text}",
                path.display()
            );
        }
        assert!(text.contains(r#"verb: "read""#), "{text}");
        assert!(text.contains(r#"effect: "read""#), "{text}");
    }

    // The drafts compile, and run against the stand: the values the
    // service holds, read.
    scene.compiled_target = match common::compile_at(&scene.registry, root.path()) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("the drafts do not compile:\n{}", common::shown(&problems)),
    };
    scene.surface = common::surface_of(&scene.compiled_target, &scene.program);
    let total = scene
        .read("example_orders_total_get", WITHIN)
        .await
        .unwrap();
    assert_eq!(
        total.get("total").and_then(a2m_core::Value::as_text),
        Some("3")
    );
    let number = scene
        .read("example_order_number_get", WITHIN)
        .await
        .unwrap();
    assert_eq!(
        number.get("number").and_then(a2m_core::Value::as_text),
        Some(NUMBER)
    );
    // Asked of the stand: two reads of properties, and nothing was called
    // or set.
    let calls = service.calls();
    assert_eq!(calls.gets, ["Total", "Number"], "{calls:?}");
    assert!(
        calls.methods.is_empty() && calls.sets.is_empty(),
        "{calls:?}"
    );
}

#[tokio::test]
async fn a_property_read_by_hand_runs_and_a_property_of_another_form_is_a_named_stop() {
    let Some(scene) = Scene::start("property").await else {
        return;
    };
    let service = Service::start(&scene.session).await.unwrap();
    let values = scene
        .read("example_orders_total_get", WITHIN)
        .await
        .unwrap();
    assert_eq!(
        values.get("total").and_then(a2m_core::Value::as_text),
        Some("3")
    );
    assert_eq!(service.calls().gets, ["Total"]);
    assert!(service.calls().methods.is_empty());
    // A list is not a form the step gives as text: a named stop.
    let stopped = scene
        .read("example_orders_level_get", WITHIN)
        .await
        .unwrap_err();
    assert_eq!(stopped.failure.code().as_str(), "property_not_text");
    assert!(service.calls().methods.is_empty() && service.calls().sets.is_empty());
}

#[tokio::test]
async fn a_read_of_a_service_that_is_not_running_is_a_named_stop_and_starts_nothing() {
    let Some(scene) = Scene::with_service("property-idle", IDLE_NAME).await else {
        return;
    };
    let idle = scene.session.idle_service();
    let stopped = scene
        .read("example_orders_total_get", WITHIN)
        .await
        .unwrap_err();
    assert_eq!(stopped.failure.code().as_str(), "service_not_running");
    assert!(!idle.started());
}
