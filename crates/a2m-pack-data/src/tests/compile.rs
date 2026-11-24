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
pub(crate) fn errors_after(file: &[&str], from: &str, to: &str) -> Diagnostics {
    errors_after_all(&[(file, from, to)])
}

/// The same, after several replacements.
pub(crate) fn errors_after_all(edits: &[(&[&str], &str, &str)]) -> Diagnostics {
    let folder = Folder::new("compile");
    let registry = registry_of(&crate::pack(), &folder, Arc::new(NoSecrets));
    let root = folder.path().join("example");
    copy(&fixtures(), &root);
    for (file, from, to) in edits {
        let path = file.iter().fold(root.clone(), |path, part| path.join(part));
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        assert!(text.contains(from), "the fixture no longer holds `{from}`");
        std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
    }
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
        .compiled_target
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
    // The read of a record by its key is the pack's own text too: every
    // field's column and the version, where the full key is the one bound.
    let by_key = &section.statements["line_item_by_key"];
    assert_eq!(
        by_key.text,
        "SELECT \"order_id\", \"item_code\", \"description\", \"quantity\", \"length_in\", \"unit_price\", \"row_version\" FROM \"order_lines\" WHERE \"order_id\" = ? AND \"item_code\" = ?"
    );
    assert!(by_key.by_key && by_key.access == Access::ReadOnly);
    assert_eq!(by_key.returns, Returns::OneOrNone);
    assert!(!section.statements["shipment_by_key"].by_key);

    // A read is three steps and commits nothing; a write has one commit
    // point, its last step, and reads back in a transaction of its own.
    let kinds = |steps: &[a2m_core::Step]| -> Vec<String> {
        steps.iter().map(|step| step.kind.to_string()).collect()
    };
    let binding = |name: &str| {
        &first
            .compiled_target
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

/// The hand-written read of a customer by its key, in place of the one the
/// pack writes; `predicate` is what follows `FROM customers`.
fn hand_written_customer_read(predicate: &str, params: &str) -> Diagnostics {
    errors_after(
        &["statements", "orders.yaml"],
        "  customer_by_key:\n    effect: read\n    form: table\n    record: customer\n",
        &format!(
            "  customer_by_key:\n    effect: read\n    record: customer\n    text: SELECT customer_id, name, credit_limit_cents, notes FROM customers {predicate}\n    params: {params}\n    returns: one_or_none\n"
        ),
    )
}

#[test]
fn a_write_is_read_back_only_by_the_read_the_pack_wrote_for_its_record() {
    // A read that picks some row, with no key at all.
    let problems = hand_written_customer_read("ORDER BY customer_id DESC LIMIT 1", "[]");
    let problem = problems
        .find("data.read_back_invalid")
        .expect("a read-back that is not by key");
    assert_eq!(
        problem.at.file.as_str(),
        "operations/customer_credit_update.yaml"
    );
    assert!(problem.fix.contains("form: table"), "{problem}");
    // A hand-written read with the key's own parameter: its text is not
    // the pack's, so what it selects is not known, and it is refused too.
    let problems = hand_written_customer_read(
        "WHERE customer_id = ?",
        "[{name: customer_id, type: identifier}]",
    );
    assert!(
        problems.find("data.read_back_invalid").is_some(),
        "{problems:#?}"
    );
    // The pack's own read of another record.
    let problems = errors_after(
        &["operations", "customer_credit_update.yaml"],
        "read_back: customer_by_key",
        "read_back: line_item_by_key",
    );
    assert!(
        problems.find("data.read_back_invalid").is_some(),
        "{problems:#?}"
    );
    // A record with no key has no read by key.
    let problems = errors_after(
        &["statements", "orders.yaml"],
        "    table: customers\n    key: [customer_id]\n",
        "    table: customers\n    key: []\n",
    );
    assert!(
        problems.find("data.statement_invalid").is_some(),
        "{problems:#?}"
    );
}

#[test]
fn a_hand_written_read_is_one_plain_select() {
    let refused = |text: &str, params: &str| {
        let problems = errors_after(
            &["statements", "orders.yaml"],
            "      SELECT shipment_id, order_id, carrier, external_ref\n      FROM shipments WHERE shipment_id = ?\n    params: [{name: shipment_id, type: integer}]\n",
            &format!("      {text}\n    params: {params}\n"),
        );
        let problem = problems
            .find("data.statement_invalid")
            .unwrap_or_else(|| panic!("`{text}` must not compile: {problems:#?}"));
        assert_eq!(problem.at.file.as_str(), "statements/orders.yaml");
        assert!(problem.at.line > 1 && !problem.fix.is_empty(), "{problem}");
    };
    let one = "[{name: shipment_id, type: integer}]";
    refused("ATTACH ? AS other", one);
    refused("DETACH other", "[]");
    refused("PRAGMA query_only = 0", "[]");
    refused("VACUUM INTO ?", one);
    refused("CREATE TABLE made_here (x)", "[]");
    refused("COMMIT", "[]");
    refused(
        "SELECT shipment_id FROM shipments WHERE shipment_id = ?; DELETE FROM shipments",
        one,
    );
    refused("SELECT load_extension(?) AS carrier", one);
    refused("SELECT file AS carrier FROM pragma_database_list", "[]");
    refused(
        "DELETE FROM shipments WHERE shipment_id = ? RETURNING shipment_id",
        one,
    );
}

#[test]
fn a_field_named_as_the_guards_parameter_is_refused() {
    // The guard's value is the step argument `version`. A field of that
    // name in the same statement would be bound to the same argument, and
    // the guard would compare the new value of the field, not the version
    // the caller saw.
    let problems = errors_after_all(&[
        (
            &["statements", "orders.yaml"],
            "      length: {column: length_in, type: length_inches}\n",
            "      version: {column: length_in, type: length_inches}\n",
        ),
        (
            &["statements", "orders.yaml"],
            "    set: [length, unit_price]\n",
            "    set: [version, unit_price]\n",
        ),
    ]);
    let problem = problems
        .find("data.statement_invalid")
        .expect("a field named as the guard's parameter");
    assert!(problem.message.contains("`version`"), "{problem}");
    assert!(!problem.fix.is_empty());
}

#[test]
fn a_write_never_compiles_as_a_read_and_a_delete_never_as_a_plain_write() {
    // An update declared as a read: refused by the kernel, from what the
    // pack says the verb does, whatever the operation's file says.
    let problems = errors_after(
        &["operations", "customer_credit_update.yaml"],
        "effect: write",
        "effect: read",
    );
    let problem = problems
        .find("kernel.effect_lower_than_derived")
        .expect("a write declared as a read");
    assert_eq!(
        problem.at.file.as_str(),
        "operations/customer_credit_update.yaml"
    );
    // A delete cannot be undone, so `write` is too little for it.
    let problems = errors_after(
        &["operations", "line_item_delete.yaml"],
        "effect: irreversible",
        "effect: write",
    );
    let problem = problems
        .find("kernel.effect_lower_than_derived")
        .expect("a delete declared as a plain write");
    assert!(problem.fix.contains("irreversible"), "{problem}");
}

/// Compiles a copy of the fixtures after one replacement, and gives the
/// compiled target.
fn compiled_target_after(file: &[&str], from: &str, to: &str) -> a2m_core::CompiledTarget {
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
    match compile_folder(&registry, &root) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("it must compile: {problems:#?}"),
    }
}

fn evidence_of(
    compiled_target: &a2m_core::CompiledTarget,
    operation: &str,
) -> a2m_core::WriteEvidence {
    compiled_target
        .operations
        .iter()
        .find(|found| found.name.as_str() == operation)
        .unwrap()
        .bindings[0]
        .evidence
}

#[test]
fn an_append_with_no_key_states_the_stores_count_as_its_evidence_and_every_other_write_is_read_back()
 {
    // The assigned-key insert keeps the read-back it names.
    let compiled_target = compiled_target_after(
        &["operations", "shipment_create.yaml"],
        "    verb: create\n",
        "    verb: create\n",
    );
    assert_eq!(
        evidence_of(&compiled_target, "example_shipment_create"),
        a2m_core::WriteEvidence::ReadBack
    );
    assert_eq!(
        evidence_of(&compiled_target, "example_line_item_update"),
        a2m_core::WriteEvidence::ReadBack
    );
    // The same insert with no read-back: `irreversible`, identity `none`.
    let compiled_target = compiled_target_after(
        &["operations", "shipment_create.yaml"],
        "    read_back: shipment_read_back\n",
        "",
    );
    assert_eq!(
        evidence_of(&compiled_target, "example_shipment_create"),
        a2m_core::WriteEvidence::Acknowledged
    );
}

#[test]
fn a_keyed_write_with_no_read_back_is_refused_by_the_pack_and_the_fix_names_the_read() {
    let problems = errors_after(
        &["operations", "line_item_update.yaml"],
        "    read_back: line_item_by_key\n",
        "",
    );
    assert!(
        problems
            .iter()
            .any(|problem| { problem.fix.contains("read the pack writes by key") }),
        "{problems:#?}"
    );
}

/// What the review says of every tool of the fixtures after one
/// replacement: where each sends, and whether it is stated at all.
fn reach_after(file: &[&str], from: &str, to: &str) -> Vec<a2m_files::Reach> {
    let folder = Folder::new("states");
    let registry = registry_of(&crate::pack(), &folder, Arc::new(NoSecrets));
    let root = folder.path().join("example");
    copy(&fixtures(), &root);
    let path = file.iter().fold(root.clone(), |path, part| path.join(part));
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert!(text.contains(from), "the fixture no longer holds `{from}`");
    std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
    let compiled = match compile_folder(&registry, &root) {
        Ok(compiled) => compiled,
        Err(problems) => panic!("it must compile: {problems:#?}"),
    };
    assert!(!compiled.review.items.is_empty());
    for item in &compiled.review.items {
        for binding in &item.bindings {
            // Nothing is started and no variable is set, whichever the store.
            if let a2m_files::BindingStates::Stated { runs, environment } = &binding.states {
                assert!(runs.is_empty() && environment.is_empty());
            }
        }
    }
    compiled
        .review
        .items
        .into_iter()
        .map(|item| item.reach)
        .collect()
}

#[test]
fn a_store_in_a_local_file_states_that_it_sends_to_no_address() {
    for reach in reach_after(&["target.yaml"], "stands: alone", "stands: alone") {
        let a2m_files::Reach::Stated { groups, .. } = reach else {
            panic!("a local file's tool does not state where it sends");
        };
        assert!(groups.is_empty(), "{groups:?}");
    }
}

#[test]
fn a_store_in_a_file_of_another_machine_is_not_stated() {
    let reaches = reach_after(
        &["target.yaml"],
        "file: example.db",
        "file: //other/share/example.db",
    );
    assert!(
        reaches
            .iter()
            .all(|reach| matches!(reach, a2m_files::Reach::Partly { .. })),
        "{reaches:?}"
    );
}
