//! The fixtures compile, twice, to the same bytes; and what does not
//! compile is refused with a file, a line, and a fix.

use std::path::Path;
use std::sync::Arc;

use a2m_core::NoSecrets;
use a2m_files::{Diagnostics, Level};

use super::{Folder, compile_folder, fixtures, registry_of};
use crate::model::{Access, Returns, Section};
use crate::names::{DATABASE, STATEMENT};

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Compiles a copy of the fixtures in which `file` had `from` replaced by
/// `to`, and gives the errors.
fn errors_after(file: &[&str], from: &str, to: &str) -> Diagnostics {
    let folder = Folder::new("compile");
    let registry = registry_of(&crate::pack(), &folder, Arc::new(NoSecrets));
    let root = folder.path().join("example");
    copy(&fixtures(), &root);
    let path = file.iter().fold(root.clone(), |path, part| path.join(part));
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert!(text.contains(from), "the fixture no longer holds `{from}`");
    std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
    let Err(problems) = compile_folder(&registry, &root) else {
        panic!("it must not compile");
    };
    problems
        .iter()
        .filter(|problem| problem.level == Level::Error)
        .cloned()
        .collect()
}

#[test]
fn the_fixtures_compile_twice_to_the_same_bytes() {
    let folder = Folder::new("compile");
    let registry = registry_of(&crate::pack(), &folder, Arc::new(NoSecrets));
    let first = compile_folder(&registry, &fixtures()).unwrap();
    let second = compile_folder(&registry, &fixtures()).unwrap();
    assert_eq!(first.encoded.bytes, second.encoded.bytes);
    assert_eq!(first.encoded.hash, second.encoded.hash);
    assert!(
        first.warnings.find("data.statement_unused").is_none(),
        "{:#?}",
        first.warnings
    );

    // The pack's section: every statement is a constant, and a table-form
    // write carries the full key and its guard.
    let section: Section = first
        .plan
        .section_of(&STATEMENT, &DATABASE)
        .unwrap()
        .payload
        .decode()
        .unwrap();
    let update = &section.statements["line_item_update"];
    assert_eq!(
        update.text,
        "UPDATE \"order_lines\" SET \"length_in\" = ?, \"unit_price\" = ? WHERE \"order_id\" = ? AND \"item_code\" = ? AND \"row_version\" = ?"
    );
    assert_eq!(update.access, Access::ReadWrite);
    assert_eq!(
        update
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        ["length", "unit_price", "order_id", "item_code", "version"]
    );
    assert_eq!(
        section.statements["line_item_delete"].text,
        "DELETE FROM \"order_lines\" WHERE \"order_id\" = ? AND \"item_code\" = ?"
    );
    let insert = &section.statements["shipment_insert"];
    assert_eq!(
        insert.text,
        "INSERT INTO \"shipments\" (\"order_id\", \"carrier\", \"external_ref\") VALUES (?, ?, ?) RETURNING \"shipment_id\""
    );
    assert_eq!(insert.returns, Returns::AssignedKey);

    // A read is three steps and commits nothing; a write has one commit
    // point, its last step, and reads back in a transaction of its own.
    let kinds = |steps: &[a2m_core::Step]| -> Vec<String> {
        steps.iter().map(|step| step.kind.to_string()).collect()
    };
    let binding = |name: &str| {
        &first
            .plan
            .operations
            .iter()
            .find(|operation| operation.name.as_str() == name)
            .unwrap()
            .bindings[0]
    };
    let read = binding("example_line_item_get");
    assert_eq!(kinds(&read.act), ["begin", "execute", "fetch"]);
    assert!(read.act.iter().all(|step| !step.commits) && read.act[2].lookup);
    let write = binding("example_line_item_update");
    assert_eq!(
        kinds(&write.act),
        ["begin", "execute", "expect_rows", "commit"]
    );
    assert_eq!(
        write
            .act
            .iter()
            .map(|step| step.commits)
            .collect::<Vec<_>>(),
        [false, false, false, true]
    );
    assert_eq!(kinds(&write.read_back), ["begin", "execute", "fetch"]);
    assert_eq!(
        kinds(&binding("example_shipment_create").act),
        ["begin", "execute", "expect_rows", "fetch", "commit"]
    );
}

#[test]
fn an_unknown_key_is_refused_with_a_file_a_line_and_a_fix() {
    let problems = errors_after(
        &["statements", "orders.yaml"],
        "    max_rows: 500\n",
        "    max_rows: 500\n    order_by: item_code\n",
    );
    let problem = problems.find("kernel.unknown_key").expect("an unknown key");
    assert_eq!(problem.at.file.as_str(), "statements/orders.yaml");
    assert!(problem.at.line > 1 && problem.at.column > 1, "{problem}");
    assert!(!problem.fix.is_empty());
}

#[test]
fn what_the_pack_does_not_accept_is_refused_under_its_own_codes() {
    // A statement that names no declared record.
    let problems = errors_after(
        &["statements", "orders.yaml"],
        "    record: shipment\n    text:",
        "    record: parcel\n    text:",
    );
    let problem = problems
        .find("data.unknown_record")
        .expect("an unknown record");
    assert!(problem.at.line > 1 && !problem.fix.is_empty(), "{problem}");

    // A list with no limit.
    let problems = errors_after(&["statements", "orders.yaml"], "    max_rows: 500\n", "");
    assert!(
        problems.find("data.limit_invalid").is_some(),
        "{problems:#?}"
    );

    // A binding to a statement nobody declared.
    let problems = errors_after(
        &["operations", "customer_get.yaml"],
        "statement: customer_by_key",
        "statement: customer_by_name",
    );
    let problem = problems
        .find("data.unknown_statement")
        .expect("an unknown statement");
    assert_eq!(problem.at.file.as_str(), "operations/customer_get.yaml");
    assert!(problem.fix.contains("customer_by_key"), "{problem}");

    // A verb that does not fit the statement.
    let problems = errors_after(
        &["operations", "line_item_update.yaml"],
        "verb: update",
        "verb: delete",
    );
    assert!(
        problems.find("data.verb_mismatch").is_some(),
        "{problems:#?}"
    );

    // A hazard the statement declares and the operation does not.
    let problems = errors_after(
        &["statements", "orders.yaml"],
        "    set: [credit_limit_cents]\n    guard: none\n",
        "    set: [credit_limit_cents]\n    guard: none\n    hazard: data_elsewhere\n",
    );
    assert!(
        problems.find("data.hazard_not_declared").is_some(),
        "{problems:#?}"
    );

    // A hand-written write, which is not built.
    let problems = errors_after(
        &["statements", "orders.yaml"],
        "    form: table\n    record: customer\n",
        "    form: statement\n    record: customer\n",
    );
    assert!(
        problems.find("data.statement_invalid").is_some(),
        "{problems:#?}"
    );
}

#[test]
fn a_write_compiles_only_where_the_store_stands_alone() {
    // Not declared: every write binding is refused, and no read is.
    let problems = errors_after(&["target.yaml"], "    stands: alone\n", "");
    assert!(!problems.is_empty());
    for problem in problems.iter() {
        assert_eq!(
            problem.code.as_str(),
            "data.write_needs_stands_alone",
            "{problem}"
        );
        assert!(
            problem.at.file.as_str().starts_with("operations/"),
            "{problem}"
        );
    }
    let files: Vec<&str> = problems
        .iter()
        .map(|problem| problem.at.file.as_str())
        .collect();
    assert!(files.contains(&"operations/line_item_update.yaml"));
    assert!(!files.contains(&"operations/line_item_get.yaml"));
}

#[test]
fn d4_probe() {
    let problems = errors_after(
        &["statements", "orders.yaml"],
        "      FROM customers WHERE customer_id = ?\n    params: [{name: customer_id, type: identifier}]\n",
        "      FROM customers ORDER BY customer_id DESC LIMIT 1\n    params: []\n",
    );
    assert!(problems.find("data.read_back_invalid").is_some(), "{problems:#?}");
}
