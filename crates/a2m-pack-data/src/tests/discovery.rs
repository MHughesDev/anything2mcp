//! Discovery: the catalog as findings, with nothing written, and the draft
//! a table finding gives.

use std::time::Duration;

use a2m_core::{AccessKind, EvidenceLevel, Failure, Finding, Source, Values};
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
        access_kind: AccessKind::Request,
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
    // Both wait in the holding folder, and are not also in place; the loader
    // does not read that folder, so every other draft compiles with them
    // there (decision 0298).
    let mut questioned: Vec<&str> = report
        .open_questions
        .iter()
        .map(|question| question.file.as_str())
        .collect();
    questioned.dedup();
    assert_eq!(
        questioned,
        [
            "held/statements/order_status_history.yaml",
            "held/statements/order_summaries.yaml"
        ]
    );
    let held: Vec<&str> = report.held.iter().map(|path| path.as_str()).collect();
    assert_eq!(held, questioned);
    assert!(
        !report
            .written
            .iter()
            .any(|path| path.as_str().contains("history") || path.as_str().contains("summaries"))
    );
    for path in &questioned {
        assert!(root.join(path).is_file(), "{path}");
        let in_place = path.strip_prefix("held/").unwrap();
        assert!(!root.join(in_place).exists(), "{in_place}");
    }
    let compiled_target = compile_folder(&scene.registry, &root)
        .unwrap()
        .compiled_target;
    let lines_get = compiled_target
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
    assert!(compiled_target.field_types[notes.field_type.unwrap().get()].untrusted);
    for operation in &compiled_target.operations {
        assert_eq!(operation.effect, a2m_core::EffectClass::Read);
    }
    scene.compiled_target = compiled_target;
    let binding = |name: &str| {
        scene
            .compiled_target
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

/// The reads drafted from the stand's catalog, without the two drafts that
/// hold a question, each list's `max_rows` set to the number given, and
/// compiled.
async fn drafted_compiled_target(
    scene: &Scene,
    max_rows: u32,
) -> (Folder, a2m_core::CompiledTarget) {
    let (findings, _) = discover(scene).await;
    let folder = Folder::new("drafted-list");
    let root = folder.path().join("example");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(
        super::fixtures().join("target.yaml"),
        root.join("target.yaml"),
    )
    .unwrap();
    let report = Drafter::new(scene.registry.compiler_set(), &TargetRoot::new(&root))
        .draft(&findings)
        .unwrap();
    for question in &report.open_questions {
        let _ = std::fs::remove_file(root.join(question.file.as_str()));
    }
    let statements = root.join("statements").join("order_lines.yaml");
    let text = std::fs::read_to_string(&statements).unwrap();
    assert!(text.contains("max_rows: 1000"), "{text}");
    std::fs::write(
        &statements,
        text.replace("max_rows: 1000", &format!("max_rows: {max_rows}")),
    )
    .unwrap();
    let compiled_target = compile_folder(&scene.registry, &root)
        .unwrap()
        .compiled_target;
    (folder, compiled_target)
}

/// The stand's value as the engine's.
fn stored(value: Option<&example_db::rows::Value>) -> a2m_core::Value {
    match value {
        Some(example_db::rows::Value::Text(text)) => a2m_core::Value::from(text.as_str()),
        Some(example_db::rows::Value::Integer(number)) => a2m_core::Value::Int(*number),
        Some(example_db::rows::Value::Null) | None => a2m_core::Value::Null,
        Some(other) => panic!("the seed holds {other:?}"),
    }
}

#[tokio::test]
async fn the_drafted_list_returns_the_stands_rows_column_by_column_through_the_compiled_targets_outputs()
 {
    let mut scene = Scene::new();
    scene.db.set_write_log(false).unwrap();
    let before = scene.db.data_version().unwrap();
    let rows = scene.db.stored_rows(Table::OrderLines).unwrap();
    let count = u32::try_from(rows.len()).unwrap();
    assert!(count > 1);
    // At the limit nothing is left; one below it, one row is.
    for (max_rows, more) in [(count, false), (count - 1, true)] {
        let (_folder, compiled_target) = drafted_compiled_target(&scene, max_rows).await;
        scene.compiled_target = compiled_target;
        let operation = scene
            .compiled_target
            .operations
            .iter()
            .find(|operation| operation.name.as_str() == "example_order_lines_list")
            .unwrap();
        let steps = operation.bindings[0].act.clone();
        let mut run = scene.prepare(a2m_provider::Posture::Live).await;
        let observed = super::values(run.steps(&steps, &Values::new(), None).await.unwrap());
        run.release().await;
        let result = super::outputs_of(
            &scene.compiled_target,
            "example_order_lines_list",
            &observed,
        );
        // Every column of the record, and `more`: nothing else.
        assert_eq!(
            result.iter().count(),
            scene.db.columns(Table::OrderLines).unwrap().len() + 1
        );
        assert_eq!(result.get("more"), Some(&a2m_core::Value::Bool(more)));
        let read = rows.iter().take(usize::try_from(max_rows).unwrap());
        for column in [
            "order_id",
            "item_code",
            "quantity",
            "unit_price",
            "length_in",
        ] {
            let list = result
                .get(column)
                .and_then(a2m_core::Value::as_list)
                .unwrap();
            let expected: Vec<a2m_core::Value> =
                read.clone().map(|row| stored(row.get(column))).collect();
            assert_eq!(list, expected.as_slice(), "{column}");
        }
        // An exact decimal the store holds as text is that text.
        assert!(
            result
                .get("unit_price")
                .and_then(a2m_core::Value::as_list)
                .unwrap()
                .iter()
                .all(|price| price.as_text().is_some_and(|text| text.contains('.')))
        );
        // Free text the store holds is marked untrusted.
        let description = operation
            .outputs
            .iter()
            .find(|output| output.name == "description")
            .unwrap();
        assert!(scene.compiled_target.field_types[description.field_type.unwrap().get()].untrusted);
    }
    assert_eq!(scene.db.data_version().unwrap(), before);
}

/// Decision 0305: a column that takes a null is drafted `nullable`, and its
/// outputs, one record's and a list's, may be absent; a column that refuses
/// one, or is part of the key, is not.
#[tokio::test]
async fn a_column_that_takes_a_null_is_drafted_nullable_and_its_outputs_may_be_absent() {
    let mut scene = Scene::new();
    scene.db.set_write_log(false).unwrap();
    let (folder, compiled_target) = drafted_compiled_target(&scene, 1000).await;
    let customers = std::fs::read_to_string(
        folder
            .path()
            .join("example")
            .join("statements")
            .join("customers.yaml"),
    )
    .unwrap();
    assert!(
        customers.contains(
            "notes:\n        column: \"notes\"\n        type: \"text\"\n        nullable: true"
        ),
        "{customers}"
    );
    assert!(
        customers.contains("name:\n        column: \"name\"\n        type: \"text\"\n      "),
        "{customers}"
    );
    let flags = |operation: &str| -> Vec<(String, bool)> {
        compiled_target
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap()
            .outputs
            .iter()
            .filter(|output| ["customer_id", "name", "notes"].contains(&output.name.as_str()))
            .map(|output| (output.name.clone(), output.may_be_absent))
            .collect()
    };
    let expected = [
        ("customer_id".to_owned(), false),
        ("name".to_owned(), false),
        ("notes".to_owned(), true),
    ];
    // The read by key takes the key as its input, and returns the rest. A
    // row that is not there is answered with `found: false`, so every one
    // of its outputs may be absent (decision 0348).
    assert_eq!(
        flags("example_customers_get"),
        [("name".to_owned(), true), ("notes".to_owned(), true)]
    );
    assert_eq!(flags("example_customers_list"), expected);

    // The list of every customer gives `null` for the one with no notes,
    // and its outputs are what the compiled target allows.
    scene.compiled_target = compiled_target;
    let steps = scene
        .compiled_target
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_customers_list")
        .unwrap()
        .bindings[0]
        .act
        .clone();
    let mut run = scene.prepare(a2m_provider::Posture::Live).await;
    let observed = super::values(run.steps(&steps, &Values::new(), None).await.unwrap());
    run.release().await;
    let result = super::outputs_of(&scene.compiled_target, "example_customers_list", &observed);
    let notes = result
        .get("notes")
        .and_then(a2m_core::Value::as_list)
        .unwrap();
    assert!(notes.contains(&a2m_core::Value::Null), "{notes:?}");
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
            "statements/order_lines.yaml"
        ]
    );
    // The table with no key holds a question, so its draft waits in the
    // holding folder, and is not also in place (decision 0298).
    let held: Vec<&str> = report.held.iter().map(|path| path.as_str()).collect();
    assert_eq!(held, ["held/statements/order_status_history.yaml"]);
    assert!(!root.join("statements/order_status_history.yaml").exists());

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
    let history = std::fs::read_to_string(
        root.join("held")
            .join("statements")
            .join("order_status_history.yaml"),
    )
    .unwrap();
    assert!(!history.contains("by_key"), "{history}");
    assert!(
        report
            .open_questions
            .iter()
            .any(
                |question| question.file.as_str() == "held/statements/order_status_history.yaml"
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

    // The keyed draft compiles with the pack's registrations, its two
    // statements each bound by a drafted read, while the held draft stays
    // where it is: the loader does not read the holding folder.
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
    // The target has the surface the drafted documents name, of the kind the
    // pack registered `statement` files for, so that a draft is not refused
    // for living on none (decision 0203).
    std::fs::write(
        root.join("target.yaml"),
        "schema_version: 1\ntarget: example\nsurfaces:\n  orders:\n    kind: database\n    store: sqlite\n    file: example.db\n    stands: alone\n",
    )
    .unwrap();
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
    assert!(
        report.refused[0].1.contains("drafts nothing that writes"),
        "refused for another reason: {}",
        report.refused[0].1
    );
    let written: Vec<&str> = report.written.iter().map(|path| path.as_str()).collect();
    assert_eq!(written, ["statements/only_reads.yaml"]);
    assert!(!root.join("statements").join("tries_a_write.yaml").exists());
}

/// Decision 0302: a column named as a secret is no field of the drafted
/// record and no output of a drafted read, whether its name is a secret's as
/// the store writes it or as the field it becomes; the description of each
/// read says so. The value is invented, and no draft holds it.
#[tokio::test]
async fn a_column_named_as_a_secret_is_never_drafted_as_a_field_or_an_output() {
    const VALUE: &str = "example-not-a-secret-0001";
    let scene = Scene::new();
    scene.db.set_write_log(false).unwrap();
    scene
        .db
        .someone_else(
            "CREATE TABLE account_logins (login_id text PRIMARY KEY, label text NOT NULL, \
             \"Client-Secret\" text NOT NULL, passwordHint text)",
            &[],
        )
        .unwrap();
    scene
        .db
        .someone_else(
            &format!(
                "INSERT INTO account_logins VALUES ('login-1', 'first', '{VALUE}', '{VALUE}')"
            ),
            &[],
        )
        .unwrap();
    let (findings, _) = discover(&scene).await;
    let finding = findings
        .into_iter()
        .find(|finding| {
            serde_json::from_str::<TableFound>(finding.found())
                .unwrap()
                .table
                == "account_logins"
        })
        .unwrap();
    let folder = Folder::new("draft-secret-columns");
    let root = folder.path().join("example");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(
        super::fixtures().join("target.yaml"),
        root.join("target.yaml"),
    )
    .unwrap();
    let report = Drafter::new(scene.registry.compiler_set(), &TargetRoot::new(&root))
        .draft(std::slice::from_ref(&finding))
        .unwrap();
    assert!(report.refused.is_empty(), "{report:?}");
    assert_eq!(report.written.len(), 3, "{report:?}");
    for path in &report.written {
        let text = std::fs::read_to_string(root.join(path.as_str())).unwrap();
        assert!(!text.contains(VALUE), "{path}: {text}");
        // No field, output, or input is named as a secret.
        for line in text.lines() {
            let key = line.trim().split(':').next().unwrap_or("");
            assert!(
                !(a2m_files::names_a_secret(key) && line.trim().ends_with(':')),
                "{path}: {line}"
            );
        }
        // The statement text selects only the columns that are drafted.
        // Only a description names them, to say they were left out.
        for line in text
            .lines()
            .filter(|line| !line.starts_with("description:"))
        {
            let line = line.to_ascii_lowercase();
            assert!(
                !line.contains("client-secret") && !line.contains("passwordhint"),
                "{path}: {line}"
            );
        }
    }
    let list = std::fs::read_to_string(root.join("operations/account_logins_list.yaml")).unwrap();
    assert!(
        list.contains("Left out because their names are a secret's, so that a secret is never an output: `client_secret`, `passwordhint`."),
        "{list}"
    );
    let get = std::fs::read_to_string(root.join("operations/account_logins_get.yaml")).unwrap();
    assert!(
        get.contains("label:") && get.contains("Left out because"),
        "{get}"
    );
    // The target compiles.
    compile_folder(&scene.registry, &root).unwrap();
}

#[tokio::test]
async fn a_drafted_read_by_key_answers_a_row_that_is_not_there_with_found_false() {
    let mut scene = Scene::new();
    scene.db.set_write_log(false).unwrap();
    let (_folder, compiled_target) = drafted_compiled_target(&scene, 1000).await;
    scene.compiled_target = compiled_target;
    let operation = scene
        .compiled_target
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_customers_get")
        .unwrap();
    assert!(
        operation
            .outputs
            .iter()
            .any(|output| output.name == "found")
    );
    let steps = operation.bindings[0].act.clone();
    // The store holds the first customer and not the second.
    let stored = scene.db.stored_rows(Table::Customers).unwrap();
    let holds = |id: &str| {
        stored
            .iter()
            .any(|row| row.get("customer_id") == Some(&example_db::rows::Value::from(id)))
    };
    assert!(holds("C-001") && !holds("C-999"));
    let mut run = scene.prepare(a2m_provider::Posture::Live).await;
    let present = super::values(
        run.steps(&steps, &super::inputs(&[("customer_id", "C-001")]), None)
            .await
            .unwrap(),
    );
    let missing = super::values(
        run.steps(&steps, &super::inputs(&[("customer_id", "C-999")]), None)
            .await
            .unwrap(),
    );
    run.release().await;
    let got = super::outputs_of(&scene.compiled_target, "example_customers_get", &present);
    assert_eq!(got.get("found"), Some(&a2m_core::Value::Bool(true)));
    assert!(got.get("name").and_then(a2m_core::Value::as_text).is_some());
    // A missing row is an answer: `found: false`, and every other output
    // absent, as `null`.
    let none = super::outputs_of(&scene.compiled_target, "example_customers_get", &missing);
    assert_eq!(none.get("found"), Some(&a2m_core::Value::Bool(false)));
    assert_eq!(none.get("name"), Some(&a2m_core::Value::Null));
    assert_eq!(none.get("notes"), Some(&a2m_core::Value::Null));
}
