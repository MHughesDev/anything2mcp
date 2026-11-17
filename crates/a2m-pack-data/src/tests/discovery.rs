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
            "statements/order_lines.yaml",
            "statements/order_status_history.yaml"
        ]
    );

    let lines = std::fs::read_to_string(root.join("statements").join("order_lines.yaml")).unwrap();
    assert!(lines.contains("order_lines_by_key"), "{lines}");
    // The read by key is one the pack writes when the file is compiled: the
    // draft holds no statement text, and the read can read a write back.
    assert!(lines.contains("form: \"table\""), "{lines}");
    assert!(
        !lines.contains("SELECT") && !lines.contains("text:"),
        "{lines}"
    );
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

    // The keyed draft alone compiles with the pack's registrations: its
    // read is not bound yet, which is a warning and no error.
    std::fs::remove_file(root.join("statements").join("order_status_history.yaml")).unwrap();
    let compiled = compile_folder(&scene.registry, &root);
    match compiled {
        Ok(compiled) => assert!(compiled.warnings.find("data.statement_unused").is_some()),
        Err(problems) => {
            let (errors, _) = problems.split();
            assert!(errors.is_empty(), "{errors:#?}");
        }
    }
}
