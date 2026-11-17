//! Discovery over an upstream server's tool list: `mcp_tools` lists and
//! calls no tool, its describer keeps the server's words apart, and its
//! drafting rule writes a mapping entry and guesses nothing. The stand says
//! which tools were called.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use a2m_core::{
    Archetype, EvidenceLevel, Failure, FailureClass, Finding, NoSecrets, Source, Values,
};
use a2m_files::{Drafter, TargetRoot};
use a2m_provider::{
    CheckedPaths, Deadline, DescribeForm, DiscoveryScope, FindingSink, Posture, ReadOnlyPort,
    SandboxPort, Sent, SurfaceDescription,
};
use common::{Folder, compiled_fixture, fence_of, fixture_root, input, prepare, registry, steps};
use example_mcp::faults::Drift;
use example_mcp::harness::HttpStand;
use example_mcp::store::INSTRUCTION_TEXT;

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

fn found(finding: &Finding) -> serde_json::Value {
    serde_json::from_str(finding.found()).unwrap()
}

#[tokio::test]
async fn discovery_lists_the_servers_tools_and_calls_none() {
    let folder = Folder::new("discover");
    let registry = registry(&folder);
    let plan = compiled_fixture(&registry);
    let stand = HttpStand::start().await.unwrap();
    let control = stand.control();
    let surface = common::surface(&plan, "orders", Some(&stand.url()));
    // The server as it is some time after it was mapped: one tool more, one
    // whose marking was turned around, one whose description now holds
    // text aimed at an agent, and another version.
    control.set_drift(Drift::ToolAdded).unwrap();
    control
        .set_drift(Drift::AnnotationsChanged {
            tool: "orders_report".to_owned(),
        })
        .unwrap();
    control
        .set_drift(Drift::DescriptionChanged {
            tool: "orders_get".to_owned(),
        })
        .unwrap();
    control.set_server_version("2.0.0").unwrap();

    let (handle, prepared) = prepare(
        &registry,
        &plan,
        &surface,
        fence_of(7),
        Posture::Discovery,
        None,
    )
    .await
    .unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let description = SurfaceDescription {
        kind: a2m_pack_mcp::SURFACE_KIND,
        archetype: Archetype::Request,
        address: surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = registry.discoverers_for(&description).remove(0);
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(Duration::from_secs(30)),
        paths: &CheckedPaths::none(),
        redact: &NoSecrets,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    discoverer.discover(&port, &scope, &mut kept).await.unwrap();

    // The stand: its list was read, and no tool was called.
    assert!(control.request_count("tools/list").unwrap() > 0);
    assert_eq!(control.request_count("tools/call").unwrap(), 0);
    assert!(control.calls().unwrap().is_empty());
    assert!(control.writes().unwrap().is_empty());

    // One finding about the server, and one for each tool it lists, each
    // observed on the live system.
    let names = a2m_pack_mcp::Names::new().unwrap();
    for finding in &kept.0 {
        assert_eq!(finding.level(), EvidenceLevel::Discovered);
        assert_eq!(finding.source(), Source::LiveSystem);
    }
    let servers: Vec<serde_json::Value> = kept
        .0
        .iter()
        .filter(|finding| finding.kind() == &names.upstream_server)
        .map(found)
        .collect();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["version_changed"], true);
    assert_eq!(servers[0]["server"]["version"], "2.0.0");
    assert_eq!(servers[0]["mapped_server"]["version"], "1.0.0");
    let tools: BTreeMap<String, serde_json::Value> = kept
        .0
        .iter()
        .filter(|finding| finding.kind() == &names.upstream_tool)
        .map(found)
        .map(|tool| (tool["tool"].as_str().unwrap().to_owned(), tool))
        .collect();
    assert_eq!(tools.len(), 8);
    assert_eq!(kept.0.len(), 9);
    // How each stands to its pin: a changed description changes no pin, a
    // changed marking does, and what is not mapped says so.
    let pin = |tool: &str| tools[tool]["pin"].as_str().unwrap().to_owned();
    assert_eq!(pin("orders_get"), "matches");
    assert_eq!(pin("orders_update"), "matches");
    assert_eq!(pin("orders_report"), "changed");
    assert_eq!(pin("orders_delete"), "unmapped");
    assert_eq!(pin("orders_archive"), "unmapped");
    assert_eq!(tools["orders_get"]["read_only"], true);
    assert_eq!(tools["orders_archive"]["read_only"], false);
    assert_eq!(tools["orders_search"]["free_text"], true);
    assert_eq!(tools["orders_update"]["free_text"], false);
    // The server's words about a tool are kept as its claims.
    assert!(
        tools["orders_get"]["untrusted"]["description"]
            .as_str()
            .unwrap()
            .contains(INSTRUCTION_TEXT)
    );

    // Nothing that writes passes the port: a call to a tool mapped as a
    // write is refused before the driver sees it.
    let write = steps(&plan, "example_order_note_update").act;
    let args = Values::new()
        .with("order_id", "A-100")
        .with("field", "note")
        .with("value", "x");
    let refused = port
        .act(&write, &input(&args, None, None, None))
        .await
        .unwrap_err();
    assert_eq!(
        (refused.failure.class(), refused.sent),
        (FailureClass::Policy, Sent::NotSent)
    );
    assert!(control.calls().unwrap().is_empty());

    // The description: what the pack says in its own words, with the
    // server's words apart from it and marked.
    for form in [DescribeForm::Summary, DescribeForm::Full] {
        let described = discoverer.describe(&kept.0, form);
        assert!(described.text.starts_with("The server lists 8 tools"));
        assert!(!described.text.contains(INSTRUCTION_TEXT));
        assert!(!described.text.contains("Returns one order"));
        assert!(!described.text.contains("2.0.0"));
        assert!(
            described
                .untrusted
                .iter()
                .any(|text| text.contains(INSTRUCTION_TEXT))
        );
        assert_eq!(described.data.get("tool_count"), Some(&8_i64.into()));
    }

    // The draft: a mapping for the surface, written into a target that has
    // none. The digest is the one discovery reported; the effect of a tool
    // the server marks read-only is `read`, for a person to review; what
    // the server did not state is a question, never a guess.
    let root = folder.path().join("target");
    std::fs::create_dir_all(&root).unwrap();
    for file in ["target.yaml", "types.yaml"] {
        std::fs::copy(fixture_root().join(file), root.join(file)).unwrap();
    }
    let report = Drafter::new(registry.compiler_set(), &TargetRoot::new(&root))
        .draft(&kept.0)
        .unwrap();
    assert!(report.conflicts.is_empty() && report.refused.is_empty());
    let written: Vec<&str> = report
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .collect();
    assert_eq!(written, ["mappings/orders.yaml"]);
    let draft = std::fs::read_to_string(root.join("mappings").join("orders.yaml")).unwrap();
    let digest = tools["orders_list"]["digest"].as_str().unwrap();
    assert!(draft.contains(digest), "{draft}");
    assert!(!draft.contains(INSTRUCTION_TEXT));
    assert!(!draft.contains("Returns one order"));
    let questions: Vec<&str> = report
        .open_questions
        .iter()
        .map(|open| open.question.as_str())
        .collect();
    // The effect of each tool the server does not mark read-only: the four
    // that write or delete, and the one whose marking was turned around.
    let effects = questions
        .iter()
        .filter(|question| question.contains("read, write, or do something"))
        .count();
    assert_eq!(effects, 5, "{questions:#?}");
    // Whether the server runs the one string a tool takes is never
    // guessed: four tools take one string.
    let strings = questions
        .iter()
        .filter(|question| question.contains("one string"))
        .count();
    assert_eq!(strings, 4, "{questions:#?}");
    assert!(!draft.contains("free_text: true") && !draft.contains("free_text: false"));
    // A draft with a question left in it is not a mapping yet: it does not
    // compile until a person has answered.
    assert_eq!(report.kept_failing.len(), 1);

    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    assert_eq!(control.request_count("tools/call").unwrap(), 0);
}
