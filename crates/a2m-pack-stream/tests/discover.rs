//! Discovery of the test broker and the drafts made from it. The broker's
//! control interface shows that a discovery only looked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{
    Archetype, Coverage, EvidenceLevel, Failure, Finding, LimitationCode, SurfaceKind, Values,
};
use a2m_files::{Drafter, TargetRoot};
use a2m_provider::{
    CheckedPaths, Deadline, DescribeForm, DiscoveryScope, FindingSink, Posture, ReadOnlyPort,
    SurfaceDescription,
};
use common::{PLANTED, Planted, Scene};
use example_broker::client::Publish;
use example_broker::model::Op;
use serde_json::json;

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// Runs the pack's discoverer against the scene's broker.
async fn discover(scene: &Scene) -> (Vec<Finding>, a2m_provider::DiscoveryReport, String) {
    let description = SurfaceDescription {
        kind: SurfaceKind::new("broker").unwrap(),
        archetype: Archetype::Stream,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = scene.registry.discoverers_for(&description).remove(0);
    let (handle, prepared) = scene.try_prepare(Posture::Discovery, None).await.unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(Duration::from_secs(20)),
        paths: &CheckedPaths::none(),
        redact: &Planted,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    let report = discoverer.discover(&port, &scope, &mut kept).await.unwrap();
    let summary = discoverer.describe(&kept.0, DescribeForm::Summary).text;
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    (kept.0, report, summary)
}

#[tokio::test]
async fn a_discovery_looks_at_every_channel_and_takes_nothing() {
    let scene = Scene::start().await;
    // Someone published a secret into the queue before the look.
    let mut outside = scene.broker.client("outside").await.unwrap();
    outside
        .publish(
            &Publish::new(
                "example.orders.work",
                json!({"order_id": "A-102", "task": PLANTED}),
            )
            .key("A-102"),
        )
        .await
        .unwrap();
    let before = scene.broker.truth();

    let (findings, report, summary) = discover(&scene).await;
    assert_eq!(report.coverage, Coverage::Complete);
    let of = |kind: &str| -> Vec<&Finding> {
        findings
            .iter()
            .filter(|finding| finding.kind().as_str() == kind)
            .collect()
    };
    // Four channels, each with the shape the broker reports.
    let channels = of("channel");
    assert_eq!(channels.len(), 4);
    assert!(
        channels
            .iter()
            .all(|finding| finding.level() == EvidenceLevel::Discovered)
    );
    let found: Vec<&str> = channels.iter().map(|finding| finding.found()).collect();
    assert!(found.iter().any(|text| text.contains(r#""shape":"log""#)));
    assert!(found.iter().any(|text| {
        text.contains("example.orders.notices") && text.contains(r#""look":"none""#)
    }));
    // A registered schema is a claim: inferred.
    let schemas = of("message_schema");
    assert_eq!(schemas.len(), 2);
    assert!(
        schemas
            .iter()
            .all(|finding| finding.level() == EvidenceLevel::Inferred)
    );
    // A sampled shape: names and types, never a value, and it says it is
    // a sample. A topic keeps nothing, so it has none.
    let shapes = of("message_shape");
    assert_eq!(shapes.len(), 3);
    for shape in &shapes {
        assert!(
            shape
                .limitations()
                .iter()
                .any(|limitation| limitation.code == LimitationCode::Sampled)
        );
        for value in ["A-100", "draft", "pick", "created", PLANTED] {
            assert!(!shape.found().contains(value), "{}", shape.found());
        }
    }
    assert!(
        shapes
            .iter()
            .any(|shape| shape.found().contains(r#""status":["string"]"#))
    );
    assert!(
        findings
            .iter()
            .all(|finding| !finding.found().contains(PLANTED))
    );
    assert!(summary.contains("4 channels"), "{summary}");

    // The broker: only looks were asked for; nothing was published,
    // delivered, or acknowledged; every channel holds what it held.
    let after = scene.broker.truth();
    let asked: Vec<Op> = after.requests[before.requests.len()..]
        .iter()
        .map(|request| request.op)
        .collect();
    assert!(
        asked
            .iter()
            .all(|op| matches!(op, Op::Hello | Op::Channels | Op::Browse)),
        "{asked:?}"
    );
    assert_eq!(after.received, before.received);
    assert!(after.deliveries.is_empty() && after.acknowledgements.is_empty());
    assert_eq!(after.channels, before.channels);

    // The same broker gives the same findings.
    let (again, _, _) = discover(&scene).await;
    let founds = |findings: &[Finding]| -> Vec<String> {
        findings
            .iter()
            .map(|finding| finding.found().to_owned())
            .collect()
    };
    assert_eq!(founds(&again), founds(&findings));
}

#[tokio::test]
async fn drafts_state_what_the_broker_stated_and_leave_the_judgments_open() {
    let scene = Scene::start().await;
    let (findings, _, _) = discover(&scene).await;

    // A target folder with a `target.yaml` and nothing else.
    let root = scene.folder.path().join("drafted");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(
        common::fixture_root().path().join("target.yaml"),
        root.join("target.yaml"),
    )
    .unwrap();
    let root = TargetRoot::new(root);
    let set = scene.registry.compiler_set();
    let report = Drafter::new(set, &root).draft(&findings).unwrap();
    assert!(report.not_covered.len() == 3, "{report:?}");
    assert!(
        report.conflicts.is_empty() && report.refused.is_empty(),
        "{report:?}"
    );

    let written: Vec<&str> = report.written.iter().map(|path| path.as_str()).collect();
    for expected in [
        "channels/orders_status.yaml",
        "channels/orders_events.yaml",
        "channels/orders_work.yaml",
        "channels/orders_notices.yaml",
        "schemas/orders_status.yaml",
        "schemas/orders_events.yaml",
        "operations/orders_status_get.yaml",
        "operations/orders_events_list.yaml",
        "operations/orders_work_list.yaml",
    ] {
        assert!(written.contains(&expected), "{expected} not in {written:?}");
    }
    // A topic cannot be looked at: no operation is drafted for it.
    assert!(
        !written
            .iter()
            .any(|path| path.contains("operations/orders_notices"))
    );

    let read = |path: &str| {
        let mut on_disk = root.path().to_path_buf();
        for part in path.split('/') {
            on_disk = on_disk.join(part);
        }
        std::fs::read_to_string(on_disk).unwrap()
    };
    // What the broker stated is there; what is a judgment is a question.
    let status = read("channels/orders_status.yaml");
    assert!(
        status.contains(r#"address: "example.orders.status""#),
        "{status}"
    );
    assert!(status.contains(r#"shape: "retained""#), "{status}");
    assert!(status.contains(r#"field: "order_id""#), "{status}");
    assert!(status.contains("open_question"), "{status}");
    for path in &written {
        let text = read(path);
        for never in [
            "publish",
            "take",
            "subscription:",
            "consumers",
            "ownership",
            "correlation",
        ] {
            assert!(!text.contains(never), "`{never}` in {path}:\n{text}");
        }
    }
    let asked: Vec<&str> = report
        .open_questions
        .iter()
        .map(|question| question.file.as_str())
        .collect();
    assert_eq!(
        asked
            .iter()
            .filter(|file| file.starts_with("channels/"))
            .count(),
        4
    );
    // A schema within what the pack checks is drafted whole, and compiles.
    assert!(
        !report
            .kept_failing
            .iter()
            .any(|(path, _)| path.as_str().starts_with("schemas/")),
        "{:?}",
        report.kept_failing
    );
    let schema = read("schemas/orders_status.yaml");
    assert!(schema.contains("enum:"), "{schema}");

    // Drafting again from the same findings changes nothing.
    let before: Vec<String> = written.iter().map(|path| read(path)).collect();
    Drafter::new(set, &root).draft(&findings).unwrap();
    let after: Vec<String> = written.iter().map(|path| read(path)).collect();
    assert_eq!(before, after);
}

#[tokio::test]
async fn the_drafted_reads_return_what_the_broker_holds() {
    let mut scene = Scene::start().await;
    let (findings, _, _) = discover(&scene).await;
    let root = scene.folder.path().join("drafted-reads");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(
        common::fixture_root().path().join("target.yaml"),
        root.join("target.yaml"),
    )
    .unwrap();
    let report = Drafter::new(scene.registry.compiler_set(), &TargetRoot::new(&root))
        .draft(&findings)
        .unwrap();
    // The read of a retained key states the fields the broker's schema
    // states, but the key; a look states what every look returns. No read
    // holds a question here: the broker holds a schema for the channel.
    assert!(
        report
            .open_questions
            .iter()
            .all(|question| question.file.as_str().starts_with("channels/")),
        "{:?}",
        report.open_questions
    );
    let get =
        std::fs::read_to_string(root.join("operations").join("orders_status_get.yaml")).unwrap();
    assert!(
        get.contains("status:") && get.contains("untrusted_text"),
        "{get}"
    );
    // A person answers the one judgment each channel leaves open.
    common::answer_roles(&root);
    scene.plan = match common::compile(&scene.registry, &TargetRoot::new(&root)) {
        Ok(compiled) => compiled.plan,
        Err(problems) => panic!("the drafts do not compile:\n{problems}"),
    };
    let before = scene.broker.truth();

    let (handle, mut prepared) = scene.reading().await;
    let steps = scene.binding("example_orders_status_get").act.clone();
    let observed = common::done(
        common::read(
            &handle,
            &mut prepared,
            &steps,
            &Values::new().with("key", "A-100"),
        )
        .await
        .unwrap(),
    );
    let result = common::outputs_of(&scene.plan, "example_orders_status_get", &observed);
    let held = &before.channels["example.orders.status"].retained["A-100"];
    assert_eq!(
        result.get("status").and_then(a2m_core::Value::as_text),
        held.body["status"].as_str()
    );
    // Its text is marked untrusted.
    let operation = scene
        .plan
        .operations
        .iter()
        .find(|found| found.name.as_str() == "example_orders_status_get")
        .unwrap();
    assert!(operation.outputs.iter().all(|output| {
        output
            .field_type
            .is_some_and(|index| scene.plan.field_types[index.get()].untrusted)
    }));

    for (operation, address) in [
        ("example_orders_events_list", "example.orders.events"),
        ("example_orders_work_list", "example.orders.work"),
    ] {
        let steps = scene.binding(operation).act.clone();
        let observed = common::done(
            common::read(&handle, &mut prepared, &steps, &Values::new())
                .await
                .unwrap(),
        );
        let result = common::outputs_of(&scene.plan, operation, &observed);
        let contents = &before.channels[address];
        let holds = contents.log.len() + contents.queue.len();
        assert_eq!(
            result.get("count"),
            Some(&a2m_core::Value::Int(i64::try_from(holds).unwrap())),
            "{operation}"
        );
        assert_eq!(result.get("more"), Some(&a2m_core::Value::Bool(false)));
    }
    handle.release(prepared, false).await;

    // Only looks: nothing published, delivered, or acknowledged, and every
    // channel holds what it held.
    let after = scene.broker.truth();
    assert_eq!(after.received, before.received);
    assert!(after.deliveries.is_empty() && after.acknowledgements.is_empty());
    assert_eq!(after.channels, before.channels);
}
