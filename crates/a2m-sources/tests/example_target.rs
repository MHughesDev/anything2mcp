//! The compiler and the readers, joined: the `example` target under
//! `targets/example/` compiles to a plan, and the source that plan carries
//! reads the target's fixture sheet into typed rows. The two crates share
//! the source definition's types, and this is the test that they agree.

use std::path::Path;

use a2m_core::{Decimal, Plan, Value, Version};
use a2m_files::testing::{example_root, stand_set};
use a2m_files::{CompileMode, Compiled, RelPath, TargetRoot, compile, load, load_files};
use a2m_sources::{Limits, LoadedSource, SOURCE_TEMPLATE_UNKNOWN, Sources, ValidatedRecords};

const LIMITS: Limits = Limits {
    bytes: 1024 * 1024,
    unpacked_bytes: 1024 * 1024,
    rows: 1_000,
    cells: 10_000,
    cell_bytes: 1024,
};

const VERSION: Version = Version::new(1, 0, 0);
const SOURCE: &str = "sources/order_sheet_v2.yaml";

/// A helper could not do its part: the test stops with why.
fn must<T, E: std::fmt::Display>(result: Result<T, E>, what: &str) -> T {
    result.unwrap_or_else(|error| panic!("{what}: {error}"))
}

/// Every file of the example target, as bytes with line feeds.
fn example_files() -> Vec<(RelPath, Vec<u8>)> {
    fn walk(folder: &Path, prefix: &str, out: &mut Vec<(RelPath, Vec<u8>)>) {
        let mut entries: Vec<_> = must(std::fs::read_dir(folder), "the target's folder")
            .map(|entry| must(entry, "an entry of the target's folder"))
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.path().is_dir() {
                walk(&entry.path(), &path, out);
            } else {
                let bytes = must(std::fs::read(entry.path()), "a file of the target");
                let text = must(String::from_utf8(bytes), "a file of the target");
                out.push((
                    must(RelPath::new(&path), "a path of the target"),
                    text.replace("\r\n", "\n").into_bytes(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    walk(example_root().path(), "", &mut files);
    files
}

/// Reads `sheet` through the source of the compiled target's one pipeline,
/// from the plan as the engine receives it: decoded from its bytes.
fn read(compiled: &Compiled, sheet: &[u8]) -> Result<ValidatedRecords, a2m_sources::Rejected> {
    let plan = must(
        Plan::decode(&compiled.encoded.bytes, &compiled.encoded.hash),
        "the plan's bytes",
    );
    let Some(source) = plan
        .pipelines
        .first()
        .and_then(|pipeline| pipeline.source.as_ref())
    else {
        panic!("the example target's pipeline has a source");
    };
    let definition = must(LoadedSource::load(&plan, source), "the source definition");
    assert_eq!(definition.template(), "order_sheet_v2");
    assert_eq!(definition.reader(), "csv");
    Sources::builtin().read(&definition, sheet, &LIMITS)
}

fn inches(text: &str) -> Value {
    Value::Decimal(must(Decimal::parse(text), "a decimal"))
}

#[test]
fn the_example_target_compiles_and_its_source_reads_its_fixture_sheet_into_typed_rows() {
    let set = stand_set().unwrap();
    let sources = load(&example_root(), &set).unwrap_or_else(|problems| panic!("{problems}"));
    let compiled = compile(&sources, &set, CompileMode::Publish, VERSION)
        .unwrap_or_else(|problems| panic!("{problems}"));
    let sheet = std::fs::read(
        example_root()
            .path()
            .join("fixtures")
            .join("order_sheet_enter")
            .join("order_sheet.csv"),
    )
    .unwrap();

    let records = read(&compiled, &sheet).unwrap();
    assert_eq!(records.template, "order_sheet_v2");
    assert_eq!(records.version.as_deref(), Some("2"));
    assert_eq!(records.filtered_out, 0);
    assert_eq!(records.rows.len(), 2);
    let first = &records.rows[0].fields;
    // A bare column, typed by its field type; a measurement, in the stored
    // unit; and a row field worked out by an expression of the plan.
    assert_eq!(first.get("code"), Some(&Value::from("L-101")));
    assert_eq!(first.get("length"), Some(&inches("90.5")));
    assert_eq!(first.get("label"), Some(&Value::from("L-101")));
    let second = &records.rows[1].fields;
    assert_eq!(second.get("code"), Some(&Value::from("L-102")));
    assert_eq!(second.get("length"), Some(&inches("36")));

    // A sheet of another layout is refused by the fingerprint the compiler
    // wrote, with the difference: the template's header and its place, and
    // never the input's own text.
    let rejected = read(&compiled, b"Code,Width\nL-101,\"3' 0\"\"\"\n").unwrap_err();
    assert!(
        rejected
            .problems
            .iter()
            .any(|problem| problem.code == SOURCE_TEMPLATE_UNKNOWN
                && problem.message.contains("`Length`")
                && problem.message.contains("column 2")),
        "{rejected:?}"
    );
    assert!(!format!("{rejected:?}").contains("Width"), "{rejected:?}");
}

#[test]
fn a_header_with_a_space_is_read_by_a_bare_column_and_by_a_mapping_expression() {
    let mut files = example_files();
    let file = files
        .iter_mut()
        .find(|(path, _)| path.as_str() == SOURCE)
        .unwrap();
    let mut text = String::from_utf8(file.1.clone()).unwrap();
    for (find, replace) in [
        // The compiler works the digest out from the parts.
        (
            "      digest: \"7c00941f8d452ede3c7f4908015699942a87aecdd932649718d72bf6851999c7\"\n",
            "",
        ),
        (
            "{kind: header, place: \"column 1\", value: \"Code\"}",
            "{kind: header, place: \"column 1\", value: \"Item code\"}",
        ),
        (
            "columns: [\"Code\", \"Length\"]",
            "columns: [\"Item code\", \"Length\"]",
        ),
        ("column: \"Code\"", "column: \"Item code\""),
        (
            "expression: \"upper(row.Code)\"",
            "expression: 'upper(row[\"Item code\"])'",
        ),
        (
            "rows: 'row.Code != \"\"'",
            "rows: 'row[\"Item code\"] != \"skip\"'",
        ),
    ] {
        assert!(text.contains(find), "`{find}` is not in {SOURCE}");
        text = text.replacen(find, replace, 1);
    }
    file.1 = text.into_bytes();

    let set = stand_set().unwrap();
    let sources = load_files(&TargetRoot::new("example"), &files, &set)
        .unwrap_or_else(|problems| panic!("{problems}"));
    let compiled = compile(&sources, &set, CompileMode::Publish, VERSION)
        .unwrap_or_else(|problems| panic!("{problems}"));

    let sheet = b"Item code,Length\r\nl-101,\"3' 0\"\"\"\r\nskip,\"1' 0\"\"\"\r\n";
    let records = read(&compiled, sheet).unwrap();
    assert_eq!(records.rows.len(), 1);
    assert_eq!(records.filtered_out, 1);
    let row = &records.rows[0].fields;
    assert_eq!(row.get("code"), Some(&Value::from("l-101")));
    assert_eq!(row.get("label"), Some(&Value::from("L-101")));
    assert_eq!(row.get("length"), Some(&inches("36")));
}
