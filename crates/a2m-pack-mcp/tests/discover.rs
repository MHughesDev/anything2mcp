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
    let compiled_target = compiled_fixture(&registry);
    let stand = HttpStand::start().await.unwrap();
    let control = stand.control();
    let surface = common::surface(&compiled_target, "orders", Some(&stand.url()));
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
        &compiled_target,
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
    let write = steps(&compiled_target, "example_order_note_update").act;
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
    // The settled entries, and apart from them those left with a question.
    // No read is drafted: of the tools still marked read-only, one answers
    // a list of records and the others take one string.
    assert_eq!(written, ["mappings/orders.yaml"]);
    // The file of the entries with a question waits in the holding folder
    // and is not also in place (decision 0298).
    let held: Vec<&str> = report.held.iter().map(a2m_files::RelPath::as_str).collect();
    assert_eq!(held, ["held/mappings/orders_undecided.yaml"]);
    assert!(!root.join("mappings/orders_undecided.yaml").exists());
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
    let undecided = std::fs::read_to_string(
        root.join("held")
            .join("mappings")
            .join("orders_undecided.yaml"),
    )
    .unwrap();
    for text in [&draft, &undecided] {
        assert!(!text.contains("free_text: true") && !text.contains("free_text: false"));
    }
    // A draft with a question left in it is not a mapping yet: it is held
    // outside what the loader reads, so what is in place compiles whole and
    // nothing in place fails.
    assert!(report.kept_failing.is_empty(), "{:?}", report.kept_failing);

    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    assert_eq!(control.request_count("tools/call").unwrap(), 0);
}

/// The stand's tool list, drafted into a new target: a mapping entry for
/// each tool, the settled ones apart from those left with a question, and
/// one pass-through read for each tool the server marks read-only whose
/// entry is settled. Without the file of questions the drafts compile, and
/// the read with no input, run through the driver's handle, gives what the
/// stand stores. No write is drafted, and the stand saw no write.
#[tokio::test]
async fn the_stands_tool_list_drafts_reads_that_read_what_it_stores() {
    let folder = Folder::new("drafted-reads");
    let registry = registry(&folder);
    let compiled_target = compiled_fixture(&registry);
    let stand = HttpStand::start().await.unwrap();
    let control = stand.control();
    let surface = common::surface(&compiled_target, "orders", Some(&stand.url()));
    let (handle, prepared) = prepare(
        &registry,
        &compiled_target,
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
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;

    let root = folder.path().join("target");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(fixture_root().join("target.yaml"), root.join("target.yaml")).unwrap();
    let report = Drafter::new(registry.compiler_set(), &TargetRoot::new(&root))
        .draft(&kept.0)
        .unwrap();
    assert!(report.conflicts.is_empty() && report.refused.is_empty());
    let written: Vec<&str> = report
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .collect();
    // `orders_report` takes nothing and answers plain counts. `orders_list`
    // answers a list of records, which is not one output; `orders_get` and
    // `orders_search` take one string, which the server may run, so their
    // entries hold a question and their reads are the developer's.
    assert_eq!(
        written,
        [
            "mappings/orders.yaml",
            "operations/orders_orders_report.yaml",
        ]
    );
    let held: Vec<&str> = report.held.iter().map(a2m_files::RelPath::as_str).collect();
    assert_eq!(held, ["held/mappings/orders_undecided.yaml"]);
    assert!(!root.join("mappings/orders_undecided.yaml").exists());
    let mut questioned: Vec<&str> = report
        .open_questions
        .iter()
        .map(|open| open.file.as_str())
        .collect();
    questioned.dedup();
    assert_eq!(questioned, ["held/mappings/orders_undecided.yaml"]);
    // The loader does not read the holding folder, so the drafts compile
    // with the file of questions there.
    assert!(root.join(questioned[0]).is_file());
    // No property of the report is named as a secret's, so its description
    // says nothing was left out for its name (decision 0302).
    let read = std::fs::read_to_string(root.join("operations/orders_orders_report.yaml")).unwrap();
    assert!(read.contains("its name is provisional."), "{read}");
    assert!(
        !read.contains("Left out because") && !read.contains("never an output"),
        "{read}"
    );
    let drafted = common::compile(&registry, &root)
        .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)))
        .compiled_target;
    for operation in &drafted.operations {
        assert_eq!(operation.effect, a2m_core::EffectClass::Read);
    }
    let surface = common::surface(&drafted, "orders", Some(&stand.url()));
    let (handle, mut prepared) = prepare(
        &registry,
        &drafted,
        &surface,
        fence_of(8),
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let report = steps(&drafted, "example_orders_report").act;
    let args = Values::new();
    let outcome = handle
        .act(
            &mut prepared,
            &report,
            &input(&args, None, None, None),
            a2m_provider::Authority::Read,
        )
        .await
        .unwrap();
    handle.release(prepared, false).await;
    let a2m_provider::StepOutcome::Done(observed) = outcome else {
        panic!("the report is never absent");
    };
    let drafts = control
        .stored_orders()
        .unwrap()
        .iter()
        .filter(|order| order.status == example_mcp::store::OrderStatus::Draft)
        .count();
    assert_eq!(
        observed.values.get("draft"),
        Some(&a2m_core::Value::Int(i64::try_from(drafts).unwrap()))
    );
    assert!(control.writes().unwrap().is_empty());
    assert_eq!(control.calls_to("orders_report").unwrap(), 1);
    assert_eq!(control.request_count("tools/call").unwrap(), 1);

    // Copy test 6, finding 41: every tool the read rule drafts nothing
    // from is listed with the rule's own reason, never with none.
    let first = Drafter::new(registry.compiler_set(), &TargetRoot::new(&root))
        .draft(&kept.0)
        .unwrap();
    assert!(!first.not_drafted.is_empty());
    for left in &first.not_drafted {
        assert!(!left.reason.is_empty(), "{left:?}");
    }
    let reasons: Vec<&str> = first
        .not_drafted
        .iter()
        .map(|left| left.reason.as_str())
        .collect();
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("not mark the tool read-only")),
        "{reasons:#?}"
    );
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("`free_text: false`")),
        "{reasons:#?}"
    );

    // Decision 0365: once a person answers in the mapping that the server
    // does not run `orders_get`'s one string, for the definition listed,
    // the scout run again drafts its read, and the read reads what the
    // stand stores.
    let get = kept
        .0
        .iter()
        .map(found)
        .find(|tool| tool["tool"] == "orders_get")
        .unwrap();
    let digest = get["digest"].as_str().unwrap();
    let mapping = root.join("mappings").join("orders.yaml");
    let mut text = std::fs::read_to_string(&mapping).unwrap();
    text.push_str(&format!(
        "---\nsurface: orders\ntools:\n  orders_get:\n    digest: \"{digest}\"\n    effect: read\n    absent_error: \"There is no order\"\n    free_text: false\n"
    ));
    std::fs::write(&mapping, text).unwrap();
    let again = Drafter::new(registry.compiler_set(), &TargetRoot::new(&root))
        .draft(&kept.0)
        .unwrap();
    let written: Vec<&str> = again
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .collect();
    assert!(
        written.contains(&"operations/orders_orders_get.yaml"),
        "{written:?}"
    );
    let drafted = common::compile(&registry, &root)
        .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)))
        .compiled_target;
    let surface = common::surface(&drafted, "orders", Some(&stand.url()));
    let (handle, mut prepared) = prepare(
        &registry,
        &drafted,
        &surface,
        fence_of(9),
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let get = steps(&drafted, "example_orders_get").act;
    let args = Values::new().with("order_id", "A-100");
    let outcome = handle
        .act(
            &mut prepared,
            &get,
            &input(&args, None, None, None),
            a2m_provider::Authority::Read,
        )
        .await
        .unwrap();
    handle.release(prepared, false).await;
    let a2m_provider::StepOutcome::Done(observed) = outcome else {
        panic!("A-100 is held");
    };
    assert_eq!(
        observed.values.get("status"),
        Some(&a2m_core::Value::from("draft"))
    );
    assert_eq!(control.calls_to("orders_get").unwrap(), 1);
}

/// A tool list that names secrets (decision 0302): a tool whose input and
/// answer have properties named as a secret's, and one that needs a secret
/// as an input. The first is drafted as a read with neither, and says so;
/// the second is not drafted. No drafted file holds the invented value,
/// and the target compiles.
#[tokio::test]
async fn a_tool_property_named_as_a_secret_is_never_a_drafted_input_or_output() {
    const INVENTED: &str = "example-not-a-secret-0002";
    let folder = Folder::new("drafted-secrets");
    let registry = registry(&folder);
    let names = a2m_pack_mcp::Names::new().unwrap();
    let digest = |digit: &str| format!("sha256:{}", digit.repeat(64));
    let tool = |name: &str, digit: &str, input: serde_json::Value, output: serde_json::Value| {
        let found = serde_json::json!({
            "surface": "orders",
            "tool": name,
            "digest": digest(digit),
            "pin": null,
            "read_only": true,
            "free_text": false,
            "unchecked_keywords": [],
            "input_schema": input,
            "output_schema": output,
            "annotations": {"readOnlyHint": true},
            "untrusted": {"title": null, "description": null},
        });
        Finding::about(
            a2m_core::Subject::Address(
                a2m_core::Address::new("http://127.0.0.1:9", "/mcp").unwrap(),
            ),
            names.upstream_tool.clone(),
            found.to_string(),
            Source::LiveSystem,
            a2m_core::ProducedBy {
                provider: "mcp".to_owned(),
                version: "0".to_owned(),
                component: None,
            },
            a2m_core::Timestamp::from_unix_millis(0),
        )
        .level(EvidenceLevel::Discovered)
        .build()
        .unwrap()
    };
    let findings = vec![
        tool(
            "orders_lookup",
            "a",
            serde_json::json!({"type": "object", "required": ["order_id"], "properties": {
                "order_id": {"type": "string", "examples": ["A-100"]},
                "access_token": {"type": "string", "default": INVENTED},
            }}),
            serde_json::json!({"type": "object", "properties": {
                "reference": {"type": "string"},
                "client_secret": {"type": "string", "default": INVENTED},
                "api_key": {"type": "string"},
            }}),
        ),
        tool(
            "orders_unlock",
            "b",
            serde_json::json!({"type": "object", "required": ["password"], "properties": {
                "password": {"type": "string", "default": INVENTED},
            }}),
            serde_json::json!({"type": "object", "properties": {"ok": {"type": "boolean"}}}),
        ),
    ];
    let root = folder.path().join("target");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(fixture_root().join("target.yaml"), root.join("target.yaml")).unwrap();
    let report = Drafter::new(registry.compiler_set(), &TargetRoot::new(&root))
        .draft(&findings)
        .unwrap();
    assert!(report.conflicts.is_empty() && report.refused.is_empty());
    let written: Vec<&str> = report
        .written
        .iter()
        .map(a2m_files::RelPath::as_str)
        .collect();
    assert_eq!(
        written,
        [
            "mappings/orders.yaml",
            "operations/orders_orders_lookup.yaml"
        ]
    );
    let read = std::fs::read_to_string(root.join("operations/orders_orders_lookup.yaml")).unwrap();
    for secret in ["client_secret", "api_key", "access_token", "password"] {
        let as_key = format!("{secret}:");
        assert!(!read.contains(&as_key), "{secret} is a key of\n{read}");
    }
    assert!(
        read.contains("order_id") && read.contains("reference"),
        "{read}"
    );
    assert!(
        read.contains("`client_secret`") && read.contains("`api_key`"),
        "{read}"
    );
    // The sentence ends the description.
    assert!(
        read.contains("so that a secret is never an output"),
        "{read}"
    );
    for file in [
        "mappings/orders.yaml",
        "operations/orders_orders_lookup.yaml",
    ] {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        assert!(!text.contains(INVENTED), "{file}");
    }
    assert!(!root.join("operations/orders_orders_unlock.yaml").exists());
    common::compile(&registry, &root)
        .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)));
}
