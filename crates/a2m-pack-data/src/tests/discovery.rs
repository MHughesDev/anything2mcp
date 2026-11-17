//! Discovery: the catalog as findings, with nothing written, and the draft
//! a table finding gives.

use std::time::Duration;

use a2m_core::{Archetype, EvidenceLevel, Failure, Finding, Source, Values};
use a2m_files::{Drafter, TargetRoot};
use a2m_provider::{
    CheckedPaths, Deadline, DescribeForm, DiscoveryScope, FindingSink, Posture, ReadOnlyPort,
    SurfaceDescription,
};
use example_db::schema::Table;

use super::{Folder, Scene, compile_folder};
use crate::discover::TableFound;
use crate::names::DATABASE;

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// Runs the pack's discoverer against the stand, through a port that can
/// only read.
async fn discover(scene: &Scene) -> (Vec<Finding>, a2m_provider::DiscovererHandle) {
    let description = SurfaceDescription {
        kind: DATABASE,
        archetype: Archetype::Request,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = scene.registry.discoverers_for(&description).remove(0);
    let handle = scene.handle();
    let prepared = scene
        .try_prepare(
            &handle,
            Posture::Discovery,
            a2m_ledger::Fence {
                run: a2m_core::RunId::from_bytes([7; 16]),
                epoch: a2m_core::Epoch::new(1),
            },
        )
        .await
        .unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
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
    let report = discoverer.discover(&port, &scope, &mut kept).await.unwrap();
    assert_eq!(report.coverage, a2m_core::Coverage::Complete);
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    (kept.0, discoverer)
}

fn table<'a>(tables: &'a [TableFound], name: &str) -> &'a TableFound {
    tables.iter().find(|table| table.table == name).unwrap()
}

#[tokio::test]
async fn the_catalog_is_found_and_nothing_is_written() {
    let scene = Scene::new();
    // The stand's own logging triggers are taken out of the catalog, so
    // that what is found is the system's own.
    scene.db.set_write_log(false).unwrap();
    let before = scene.db.data_version().unwrap();
    let rows_before = scene.db.stored_rows(Table::OrderLines).unwrap();
    let (findings, discoverer) = discover(&scene).await;
    assert_eq!(scene.db.data_version().unwrap(), before);
    assert_eq!(
        scene.db.stored_rows(Table::OrderLines).unwrap(),
        rows_before
    );

    assert!(findings.iter().all(|finding| {
        finding.level() == EvidenceLevel::Discovered && finding.source() == Source::LiveSystem
    }));
    let tables: Vec<TableFound> = findings
        .iter()
        .map(|finding| serde_json::from_str(finding.found()).unwrap())
        .collect();

    // What the stand's own control interface says of the catalog.
    let lines = table(&tables, "order_lines");
    let stand = scene.db.columns(Table::OrderLines).unwrap();
    assert_eq!(lines.columns.len(), stand.len());
    for (found, stored) in lines.columns.iter().zip(&stand) {
        assert_eq!(
            (
                &found.name,
                &found.declared_type,
                found.not_null,
                found.key_position
            ),
            (
                &stored.name,
                &stored.declared_type,
                stored.not_null,
                stored.key_position
            )
        );
    }
    let stand_triggers = scene.db.triggers().unwrap();
    let found_triggers: usize = tables.iter().map(|table| table.triggers.len()).sum();
    assert_eq!(found_triggers, stand_triggers.len());

    // The hazards: a trigger that writes another table, and cascading keys.
    let orders = table(&tables, "orders");
    assert_eq!(orders.triggers, ["orders_record_status_change"]);
    assert_eq!(orders.cascades_to, ["order_lines", "order_notes"]);
    assert!(orders.reaches_elsewhere() && lines.reaches_elsewhere());
    let customers = table(&tables, "customers");
    assert!(!customers.reaches_elsewhere());
    assert_eq!(customers.unique, [["email"]]);
    // A table with no key, and a view.
    let history = table(&tables, "order_status_history");
    assert!(
        history
            .columns
            .iter()
            .all(|column| column.key_position == 0)
    );
    assert!(table(&tables, "order_summaries").view);
    // No row was read: no value of the seed is in any finding.
    let all = findings.iter().map(Finding::found).collect::<String>();
    assert!(
        !all.contains("Left rail") && !all.contains("Ignore all earlier"),
        "{all}"
    );

    let summary = discoverer.describe(&findings, DescribeForm::Summary);
    assert!(summary.untrusted.is_empty());
    assert!(!summary.text.contains("order_lines"), "{}", summary.text);
    let full = discoverer.describe(&findings, DescribeForm::Full);
    assert!(full.untrusted.iter().any(|name| name == "order_lines"));
}

/// The stand's whole catalog, drafted into a new target: for each keyed
/// table a statements file and a read by key and a list over it, which
/// compile in draft mode beside the two drafts that hold a question, and
/// which read, through the driver's handle, what the stand stores. The
/// read by key has no example, since discovery reads no row; the list
/// needs no input. No write is drafted and nothing is written.
#[tokio::test]
async fn the_stands_catalog_drafts_reads_that_read_what_it_stores() {
    let mut scene = Scene::new();
    scene.db.set_write_log(false).unwrap();
    let before = scene.db.data_version().unwrap();
    let (findings, _) = discover(&scene).await;
    let folder = Folder::new("drafted-reads");
    let root = folder.path().join("example");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(
        super::fixtures().join("target.yaml"),
        root.join("target.yaml"),
    )
    .unwrap();
    let set = scene.registry.compiler_set();
    let report = Drafter::new(set, &TargetRoot::new(&root))
        .draft(&findings)
        .unwrap();
    assert!(report.refused.is_empty(), "{report:?}");
    let operations: Vec<&str> = report
        .written
        .iter()
        .map(|path| path.as_str())
        .filter(|path| path.starts_with("operations/"))
        .collect();
    for table in [
        "customers",
        "orders",
        "order_lines",
        "shipments",
        "order_notes",
    ] {
        for verb in ["get", "list"] {
            let path = format!("operations/{table}_{verb}.yaml");
            assert!(operations.contains(&path.as_str()), "{operations:?}");
        }
    }
    // The table with no key and the view hold a question, and have no read.
    // While they are in the folder no statements file compiles, since the
    // kind's section is not made; without them, every draft does.
    let mut questioned: Vec<&str> = report
        .open_questions
        .iter()
        .map(|question| question.file.as_str())
        .collect();
    questioned.dedup();
    assert_eq!(
        questioned,
        [
            "statements/order_status_history.yaml",
            "statements/order_summaries.yaml"
        ]
    );
    assert!(
        !operations
            .iter()
            .any(|path| path.contains("history") || path.contains("summaries"))
    );
    for path in &questioned {
        std::fs::remove_file(root.join(path)).unwrap();
    }
    let plan = compile_folder(&scene.registry, &root).unwrap().plan;
    let lines_get = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_order_lines_get")
        .unwrap();
    assert!(lines_get.examples.is_empty());
    assert_eq!(lines_get.effect, a2m_core::EffectClass::Read);
    let notes = lines_get
        .outputs
        .iter()
        .find(|output| output.name == "description")
        .unwrap();
    assert!(plan.field_types[notes.field_type.unwrap().get()].untrusted);
    for operation in &plan.operations {
        assert_eq!(operation.effect, a2m_core::EffectClass::Read);
    }
    scene.plan = plan;
    let binding = |name: &str| {
        scene
            .plan
            .operations
            .iter()
            .find(|operation| operation.name.as_str() == name)
            .unwrap()
            .bindings[0]
            .act
            .clone()
    };
    let (get, list) = (
        binding("example_order_lines_get"),
        binding("example_customers_list"),
    );
    let mut run = scene.prepare(a2m_provider::Posture::Live).await;
    let key = Values::new()
        .with("order_id", "A-100")
        .with("item_code", "L-101");
    let row = super::values(run.steps(&get, &key, None).await.unwrap());
    let stored = scene.db.stored_rows(Table::OrderLines).unwrap();
    let left = stored
        .iter()
        .find(|row| row.get("item_code") == Some(&example_db::rows::Value::from("L-101")))
        .unwrap();
    let Some(example_db::rows::Value::Text(description)) = left.get("description") else {
        panic!("{left:?}");
    };
    assert_eq!(
        row.get("description").and_then(a2m_core::Value::as_text),
        Some(description.as_str())
    );
    let all = super::values(run.steps(&list, &Values::new(), None).await.unwrap());
    let ids: Vec<&str> = all
        .get("rows")
        .and_then(a2m_core::Value::as_list)
        .unwrap()
        .iter()
        .map(|row| {
            row.as_record()
                .and_then(|row| row.get("customer_id"))
                .and_then(a2m_core::Value::as_text)
                .unwrap()
        })
        .collect();
    assert_eq!(ids, ["C-001", "C-002", "C-003"]);
    assert_eq!(all.get("more"), Some(&a2m_core::Value::Bool(false)));
    run.release().await;
    assert_eq!(scene.db.data_version().unwrap(), before);
}

#[tokio::test]
async fn a_table_with_a_key_drafts_a_read_by_key_and_one_without_drafts_a_question() {
    let scene = Scene::new();
    scene.db.set_write_log(false).unwrap();
    let (findings, _) = discover(&scene).await;
    let of = |name: &str| -> Finding {
        findings
            .iter()
            .find(|finding| {
                serde_json::from_str::<TableFound>(finding.found())
                    .unwrap()
                    .table
                    == name
            })
            .unwrap()
            .clone()
    };
    let folder = Folder::new("draft");
    let root = folder.path().join("example");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(
        super::fixtures().join("target.yaml"),
        root.join("target.yaml"),
    )
    .unwrap();
    let set = scene.registry.compiler_set();
    let target = TargetRoot::new(&root);
    let report = Drafter::new(set, &target)
        .draft(&[of("order_lines"), of("order_status_history")])
        .unwrap();
    let written: Vec<&str> = report.written.iter().map(|path| path.as_str()).collect();
    assert_eq!(
        written,
        [
            "operations/order_lines_get.yaml",
            "operations/order_lines_list.yaml",
            "statements/order_lines.yaml",
            "statements/order_status_history.yaml"
        ]
    );

    let lines = std::fs::read_to_string(root.join("statements").join("order_lines.yaml")).unwrap();
    assert!(lines.contains("order_lines_by_key"), "{lines}");
    // The read by key is one the pack writes when the file is compiled: the
    // draft holds no statement text for it, and the read can read a write
    // back. The list's text names the catalog's own columns, quoted, in
    // the order of the key, and nothing from an input.
    assert!(lines.contains("form: \"table\""), "{lines}");
    assert_eq!(lines.matches("text:").count(), 1, "{lines}");
    assert!(
        lines.contains(r#"FROM \"order_lines\" ORDER BY \"order_id\", \"item_code\""#),
        "{lines}"
    );
    assert!(lines.contains("max_rows: 1000"), "{lines}");
    // No write, and no unit: a length is text until a person says more.
    assert!(
        !lines.contains("write") && !lines.contains("length_ft_in"),
        "{lines}"
    );
    assert!(
        lines.contains("length_in:\n        column: \"length_in\"\n        type: \"text\""),
        "{lines}"
    );
    // The table with no key holds a question where its key would be, and
    // no statement.
    let history =
        std::fs::read_to_string(root.join("statements").join("order_status_history.yaml")).unwrap();
    assert!(!history.contains("by_key"), "{history}");
    assert!(
        report
            .open_questions
            .iter()
            .any(
                |question| question.file.as_str() == "statements/order_status_history.yaml"
                    && question.question.contains("identify one row")
            ),
        "{:#?}",
        report.open_questions
    );
    // The same findings give the same bytes.
    let again = Folder::new("draft");
    let second = again.path().join("example");
    std::fs::create_dir_all(&second).unwrap();
    std::fs::copy(
        super::fixtures().join("target.yaml"),
        second.join("target.yaml"),
    )
    .unwrap();
    Drafter::new(set, &TargetRoot::new(&second))
        .draft(&[of("order_lines"), of("order_status_history")])
        .unwrap();
    assert_eq!(
        lines,
        std::fs::read_to_string(second.join("statements").join("order_lines.yaml")).unwrap()
    );

    // The keyed draft alone compiles with the pack's registrations, its
    // two statements each bound by a drafted read.
    std::fs::remove_file(root.join("statements").join("order_status_history.yaml")).unwrap();
    let compiled = compile_folder(&scene.registry, &root).unwrap();
    assert!(compiled.warnings.find("data.statement_unused").is_none());
}

/// A rule that drafts what the pack's own rule never does: a statement
/// that writes.
struct DraftsAWrite(a2m_core::FindingKind);

impl a2m_files::DraftingRule for DraftsAWrite {
    fn finding_kind(&self) -> a2m_core::FindingKind {
        self.0.clone()
    }

    fn file_kind(&self) -> a2m_core::FileKindName {
        crate::names::STATEMENT
    }

    fn draft(
        &self,
        _finding: &Finding,
        _cx: &a2m_files::DraftContext<'_>,
    ) -> Result<Vec<a2m_files::DraftDoc>, Failure> {
        use a2m_files::DraftValue;
        let statement = |effect: &str| {
            DraftValue::map(vec![
                ("effect", DraftValue::text(effect)),
                ("form", DraftValue::text("table")),
                ("record", DraftValue::text("order_lines")),
            ])
        };
        let doc = |file: &str, effect: &str| -> Result<a2m_files::DraftDoc, Failure> {
            Ok(a2m_files::DraftDoc {
                path: a2m_files::RelPath::new(&format!("statements/{file}.yaml"))?,
                body: DraftValue::map(vec![
                    ("surface", DraftValue::text("orders")),
                    (
                        "statements",
                        DraftValue::map(vec![
                            ("order_lines_by_key", statement("read")),
                            ("order_lines_change", statement(effect)),
                        ]),
                    ),
                ]),
            })
        };
        Ok(vec![
            doc("tries_a_write", "write")?,
            doc("only_reads", "read")?,
        ])
    }
}

#[tokio::test]
async fn a_drafting_rule_that_drafts_a_statement_that_writes_is_refused() {
    let scene = Scene::new();
    scene.db.set_write_log(false).unwrap();
    let (findings, _) = discover(&scene).await;
    let kind = findings[0].kind().clone();
    let mut builder = a2m_files::CompilerSet::builder();
    builder
        .file_kind(
            crate::names::PACK,
            std::sync::Arc::new(crate::kind::StatementKind::new(1000)),
        )
        .unwrap()
        .drafting_rule(crate::names::PACK, std::sync::Arc::new(DraftsAWrite(kind)))
        .unwrap();
    let set = builder.build();
    let folder = Folder::new("draft-write");
    let root = folder.path().join("example");
    std::fs::create_dir_all(&root).unwrap();
    let report = Drafter::new(&set, &TargetRoot::new(&root))
        .draft(&findings[..1])
        .unwrap();
    // The file with a statement that writes is not written; the one that
    // only reads is.
    let refused: Vec<&str> = report
        .refused
        .iter()
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(refused, ["statements/tries_a_write.yaml"]);
    let written: Vec<&str> = report.written.iter().map(|path| path.as_str()).collect();
    assert_eq!(written, ["statements/only_reads.yaml"]);
    assert!(!root.join("statements").join("tries_a_write.yaml").exists());
}
