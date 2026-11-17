//! The `example` target compiles, the same way every time, and each thing
//! the compiler must refuse is refused with a file, a line, a column, a
//! code, and a fix.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::codes::KERNEL_CODES;
use crate::testing::{example_root, stand_set, surface_kind};
use crate::{
    CompileMode, Compiled, CompilerSet, Diagnostics, JsonSchema, Level, LowerContext, Lowered,
    Lowering, Possible, RelPath, ResolvedBinding, SourceFile, TargetRoot, compile, lint_one, load,
    load_files,
};
use a2m_core::{
    EffectClass, FailsAs, FailureClass, FailureCode, FileKindName, IdentityStrategy, Mode,
    PackPayload, Plan, Step, StepArg, StepKind, SurfaceKind, SurfaceName, Verb, Version,
};

const VERSION: Version = Version::new(1, 0, 0);

/// Every file of the example target, as bytes, with line feeds.
fn example_files() -> Vec<(RelPath, Vec<u8>)> {
    fn walk(folder: &Path, prefix: &str, out: &mut Vec<(RelPath, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap())
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
                let text = String::from_utf8(std::fs::read(entry.path()).unwrap()).unwrap();
                out.push((
                    RelPath::new(&path).unwrap(),
                    text.replace("\r\n", "\n").into_bytes(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    walk(example_root().path(), "", &mut files);
    files
}

enum Case {
    /// Replace `find` with `replace` in one file.
    Edit(&'static str, &'static str, &'static str),
    /// Add a file.
    Add(&'static str, &'static str),
    /// Remove a file.
    Remove(&'static str),
    /// Proven by the named test in this file, which needs more than an edit.
    Elsewhere(&'static str),
}

fn changed(case: &Case) -> Vec<(RelPath, Vec<u8>)> {
    let mut files = example_files();
    match case {
        Case::Edit(path, find, replace) => {
            let file = files
                .iter_mut()
                .find(|(known, _)| known.as_str() == *path)
                .unwrap();
            let text = String::from_utf8(file.1.clone()).unwrap();
            assert!(text.contains(find), "`{find}` is not in {path}");
            file.1 = text.replacen(find, replace, 1).into_bytes();
        }
        Case::Add(path, content) => {
            files.push((RelPath::new(path).unwrap(), content.as_bytes().to_vec()));
        }
        Case::Remove(path) => files.retain(|(known, _)| known.as_str() != *path),
        Case::Elsewhere(_) => {}
    }
    files
}

/// Everything the loader and the compiler say about `files`.
fn problems_with(
    files: &[(RelPath, Vec<u8>)],
    set: &CompilerSet,
    mode: CompileMode,
) -> Diagnostics {
    let root = TargetRoot::new("example");
    match load_files(&root, files, set) {
        Err(problems) => problems,
        Ok(sources) => match compile(&sources, set, mode, VERSION) {
            Ok(compiled) => compiled.warnings,
            Err(problems) => problems,
        },
    }
}

fn problems_of(case: &Case) -> Diagnostics {
    problems_with(&changed(case), &stand_set().unwrap(), CompileMode::Publish)
}

fn compiled() -> Compiled {
    let set = stand_set().unwrap();
    let sources = load(&example_root(), &set).unwrap_or_else(|problems| panic!("{problems}"));
    compile(&sources, &set, CompileMode::Publish, VERSION)
        .unwrap_or_else(|problems| panic!("{problems}"))
}

const ORDER_GET: &str = "operations/order_get.yaml";
const UPSERT: &str = "operations/line_item_upsert.yaml";
const SHEET: &str = "pipelines/order_sheet_enter.yaml";

/// One way to break the example target for every kernel lint code.
fn cases() -> Vec<(&'static str, Case)> {
    vec![
        (
            "kernel.yaml_malformed",
            Case::Edit(ORDER_GET, "effect: read", "effect: [read"),
        ),
        (
            "kernel.duplicate_key",
            Case::Edit(ORDER_GET, "effect: read\n", "effect: read\neffect: read\n"),
        ),
        (
            "kernel.yaml_feature_refused",
            Case::Edit(ORDER_GET, "effect: read", "effect: &a read"),
        ),
        (
            "kernel.path_outside_target",
            Case::Elsewhere("the_loader_never_reads_outside_the_target_folder"),
        ),
        (
            "kernel.unknown_file",
            Case::Add("notes/todo.yaml", "a: 1\n"),
        ),
        ("kernel.file_missing", Case::Remove("target.yaml")),
        (
            "kernel.unknown_key",
            Case::Edit(ORDER_GET, "title: Get an order", "titel: Get an order"),
        ),
        (
            "kernel.missing_field",
            Case::Edit(ORDER_GET, "effect: read\n", ""),
        ),
        (
            "kernel.wrong_type",
            Case::Edit(ORDER_GET, "effect: read", "effect: [read]"),
        ),
        (
            "kernel.unknown_value",
            Case::Edit(ORDER_GET, "effect: read", "effect: peek"),
        ),
        (
            "kernel.name_malformed",
            Case::Edit(
                ORDER_GET,
                "  order_id: {type: identifier,",
                "  Order-Id: {type: identifier,",
            ),
        ),
        (
            "kernel.schema_version_unsupported",
            Case::Edit("target.yaml", "schema_version: 1", "schema_version: 2"),
        ),
        (
            "kernel.open_question",
            Case::Edit(
                "types.yaml",
                "currency: USD",
                "currency: {open_question: \"Which currency?\"}",
            ),
        ),
        (
            "kernel.unknown_surface_kind",
            Case::Edit("target.yaml", "kind: stand", "kind: stend"),
        ),
        (
            "kernel.unknown_control",
            Case::Edit(
                "calls/orders.yaml",
                "effect: write",
                "effect: write\ncontrol: dial",
            ),
        ),
        (
            "kernel.unknown_name",
            Case::Edit(
                "tests/order_get.yaml",
                "test: example_order_get",
                "test: example_order_got",
            ),
        ),
        (
            "kernel.name_defined_twice",
            Case::Edit(
                "calls/orders.yaml",
                "call: line_item_get",
                "call: order_get",
            ),
        ),
        (
            "kernel.unknown_field_type",
            Case::Edit(ORDER_GET, "type: order_status", "type: order_state"),
        ),
        (
            "kernel.field_type_incomplete",
            Case::Edit("types.yaml", "    currency: USD\n", ""),
        ),
        (
            "kernel.value_not_of_type",
            Case::Edit(
                "tests/order_get.yaml",
                "order_id: \"SO-1001\"",
                "order_id: 1001",
            ),
        ),
        (
            "kernel.expression_invalid",
            Case::Edit(UPSERT, "input.length", "inputs.length"),
        ),
        (
            "kernel.expression_type_error",
            Case::Edit(SHEET, "result.status == 'draft'", "result.status"),
        ),
        (
            "kernel.unknown_mode",
            Case::Edit(ORDER_GET, "mode: stand", "mode: stnad"),
        ),
        (
            "kernel.unknown_verb",
            Case::Edit(UPSERT, "verb: upsert", "verb: remove"),
        ),
        (
            "kernel.unknown_step_kind",
            Case::Elsewhere("what_a_faulty_lowering_returns_is_refused"),
        ),
        (
            "kernel.read_kind_not_registered",
            Case::Elsewhere("what_a_faulty_lowering_returns_is_refused"),
        ),
        (
            "kernel.lowering_invalid",
            Case::Elsewhere("what_a_faulty_lowering_returns_is_refused"),
        ),
        (
            "kernel.write_without_identity",
            Case::Edit(UPSERT, "key: [order_id, item_code]\n", ""),
        ),
        (
            "kernel.identity_not_possible",
            Case::Edit(
                UPSERT,
                "key: [order_id, item_code]",
                "identity: {strategy: stamped}",
            ),
        ),
        (
            "kernel.identity_invalid",
            Case::Edit(
                UPSERT,
                "key: [order_id, item_code]",
                "key: [order_id, item]",
            ),
        ),
        (
            "kernel.write_without_read_back",
            Case::Edit(UPSERT, "    read_back: {call: line_item_get}\n", ""),
        ),
        (
            "kernel.commit_points_differ",
            Case::Edit(UPSERT, "commit_points: 1", "commit_points: 2"),
        ),
        (
            "kernel.effect_lower_than_derived",
            Case::Edit(SHEET, "effect: write", "effect: read"),
        ),
        (
            "kernel.step_cycle",
            Case::Edit(
                SHEET,
                "with: {order_id: input.order_id}",
                "with: {order_id: steps.totals.result.note}",
            ),
        ),
        (
            "kernel.surface_not_declared",
            Case::Edit(
                "calls/orders.yaml",
                "call: order_get\n",
                "call: order_get\nsurface: counter\n",
            ),
        ),
        (
            "kernel.for_each_without_checkpoint",
            Case::Edit(SHEET, ",\n     checkpoint: per_record", ""),
        ),
        (
            "kernel.write_pipeline_without_limit",
            Case::Edit(SHEET, "limits: {max_records: 50}\n", ""),
        ),
        (
            "kernel.credential_in_file",
            Case::Edit(
                "auth.yaml",
                "vault://example/service/password",
                "correct-horse-battery",
            ),
        ),
        (
            "kernel.operation_without_test",
            Case::Remove("tests/order_get.yaml"),
        ),
        (
            "kernel.fixture_not_scrubbed",
            Case::Edit(
                "fixtures/order_get/fixture.yaml",
                "scrubbed: true",
                "scrubbed: false",
            ),
        ),
        (
            "kernel.tool_name_scheme",
            Case::Edit(
                ORDER_GET,
                "operation: example_order_get",
                "operation: sample_order_get",
            ),
        ),
        (
            "kernel.tool_names_too_alike",
            Case::Add(
                "operations/order_get_v2.yaml",
                "operation: example_order_get_v2\neffect: read\nkey: order_id\ninputs:\n  order_id: {type: identifier}\noutputs:\n  status: {type: order_status}\nbindings:\n  - {mode: stand, call: order_get}\ndescription: Reads the status of one order, in the second form.\n",
            ),
        ),
        (
            "kernel.description_weak",
            Case::Edit(
                UPSERT,
                "description: >\n  Creates or updates one line item on an order.\n  Prefer example_order_sheet_enter for whole sheets.\n",
                "description: Saves.\n",
            ),
        ),
        (
            "kernel.output_not_marked",
            Case::Edit(ORDER_GET, "note: {type: note}", "note: {type: text}"),
        ),
        (
            "kernel.tool_indistinguishable",
            Case::Add(
                "operations/order_peek.yaml",
                "operation: example_order_peek\neffect: read\nkey: order_id\ninputs:\n  order_id: {type: identifier, description: \"Order number shown in the application\"}\noutputs:\n  status: {type: order_status}\nbindings:\n  - {mode: stand, call: order_get}\ndescription: >\n  Reads the status, the total, and the note of one order.\n  Prefer example_order_sheet_enter to enter a whole sheet.\n",
            ),
        ),
        (
            "kernel.plan_not_encodable",
            Case::Elsewhere("what_a_faulty_lowering_returns_is_refused"),
        ),
    ]
}

#[test]
fn the_example_target_compiles_clean_and_holds_every_kernel_file_kind() {
    let compiled = compiled();
    assert!(compiled.warnings.is_empty(), "{}", compiled.warnings);
    let plan = &compiled.plan;
    assert_eq!(plan.header.target.as_str(), "example");
    assert_eq!(plan.header.version, VERSION);
    assert_eq!(plan.surfaces.len(), 1);
    assert_eq!(plan.surfaces[0].address, "https://app.example.com");
    assert_eq!(
        plan.environments
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["test", "production"]
    );
    assert_eq!(plan.operations.len(), 2);
    assert_eq!(plan.pipelines.len(), 1);
    assert_eq!(plan.tools.len(), 3);
    assert_eq!(plan.tests.len(), 3);
    assert_eq!(plan.packs.len(), 1);
    assert_eq!(plan.packs[0].pack, "stand");

    let upsert = plan
        .operations
        .iter()
        .find(|op| op.name.as_str() == "example_line_item_upsert")
        .unwrap();
    assert_eq!(upsert.effect, EffectClass::Write);
    assert_eq!(upsert.identity.strategy, IdentityStrategy::Composite);
    assert_eq!(upsert.identity.key.len(), 2);
    assert_eq!(upsert.commit_points, 1);
    assert_eq!(upsert.bindings[0].read_back.len(), 1);
    assert!(upsert.bindings[0].read_back[0].lookup);
    assert_eq!(upsert.manifest.surfaces.len(), 1);

    let money = plan
        .field_types
        .iter()
        .find(|ty| ty.name == "amount")
        .unwrap();
    assert_eq!(money.places, Some(2));
    assert_eq!(money.currency.as_deref(), Some("USD"));
    let length = plan
        .field_types
        .iter()
        .find(|ty| ty.name == "length_ft_in")
        .unwrap();
    assert_eq!(length.unit.as_deref(), Some("in"));
    assert!(length.normalizer.is_some());

    let pipeline = &plan.pipelines[0];
    assert_eq!(pipeline.effect, EffectClass::Write);
    assert_eq!(
        pipeline
            .inputs
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>(),
        ["order_id", "source"]
    );
    // Bare columns are data in the source's payload; only an expression is in the plan's mapping.
    let source = pipeline.source.as_ref().unwrap();
    assert_eq!(source.mapping.len(), 1);
    assert_eq!(source.mapping[0].name, "label");
    let spec: crate::SourceSpec = source.payload.decode().unwrap();
    assert_eq!(spec.columns, ["Code", "Length"]);
    assert_eq!(spec.fields.len(), 3);
    assert_eq!(spec.fields[0].column.as_deref(), Some("Code"));
    assert_eq!(spec.fields[2].column, None);
    assert_eq!(spec.key, ["code"]);
    assert_eq!(spec.locate, crate::Locate::Document);
    assert!(spec.versions[0].fingerprint.is_sound());
    assert_eq!(spec.options.delimiter, None);
    assert_eq!(pipeline.limits.max_records, Some(50));

    let tool = plan
        .tools
        .iter()
        .find(|tool| tool.name.as_str() == "example_order_get")
        .unwrap();
    assert!(tool.annotations.read_only);
    // The review lists the riskiest tool first.
    assert_eq!(compiled.review.items[0].effect, EffectClass::Write);
    assert_eq!(
        compiled.review.items.last().unwrap().effect,
        EffectClass::Read
    );
}

#[test]
fn compiling_twice_gives_the_same_bytes_and_the_plan_decodes_to_an_equal_plan() {
    let first = compiled();
    let second = compiled();
    assert_eq!(first.encoded.bytes, second.encoded.bytes);
    assert_eq!(first.encoded.hash, second.encoded.hash);
    let decoded = Plan::decode(&first.encoded.bytes, &first.encoded.hash).unwrap();
    assert_eq!(decoded, first.plan);
    assert_eq!(decoded.encode().unwrap().hash, first.encoded.hash);
    // No path of this machine reaches the plan.
    let text = String::from_utf8(first.encoded.bytes).unwrap();
    assert!(!text.contains("targets"), "a path reached the plan");
}

#[test]
fn files_with_other_line_endings_and_from_memory_compile_to_the_same_plan() {
    let set = stand_set().unwrap();
    let from_disk = compiled();
    let compile_files = |files: &[(RelPath, Vec<u8>)]| {
        let sources = load_files(&TargetRoot::new("anywhere"), files, &set).unwrap();
        compile(&sources, &set, CompileMode::Publish, VERSION).unwrap()
    };
    let unix = example_files();
    assert_eq!(compile_files(&unix).encoded.hash, from_disk.encoded.hash);
    let windows: Vec<(RelPath, Vec<u8>)> = unix
        .iter()
        .map(|(path, bytes)| {
            let text = String::from_utf8(bytes.clone()).unwrap();
            // Recorded fixture files are digested byte for byte, so only
            // the YAML is given other line endings.
            let text = if path.as_str().ends_with(".yaml") {
                text.replace('\n', "\r\n")
            } else {
                text
            };
            (path.clone(), text.into_bytes())
        })
        .collect();
    assert_ne!(windows, unix);
    assert_eq!(
        compile_files(&windows).encoded.bytes,
        from_disk.encoded.bytes
    );
    // Another version is another plan.
    let sources = load_files(&TargetRoot::new("anywhere"), &unix, &set).unwrap();
    let other = compile(&sources, &set, CompileMode::Publish, Version::new(1, 0, 1)).unwrap();
    assert_ne!(other.encoded.hash, from_disk.encoded.hash);
}

#[test]
fn a_broken_file_fails_with_a_file_a_line_a_column_a_code_and_a_fix() {
    let problems = problems_of(&Case::Edit(
        ORDER_GET,
        "title: Get an order",
        "titel: Get an order",
    ));
    assert!(problems.has_errors());
    let problem = problems.find("kernel.unknown_key").unwrap();
    assert_eq!(problem.at.file.as_str(), ORDER_GET);
    assert_eq!((problem.at.line, problem.at.column), (2, 1));
    assert_eq!(problem.level, Level::Error);
    assert_eq!(problem.fix, "Write `title`.");
    assert!(problem.message.contains("titel"));
    // For an agent, as JSON; for an action, as one failure.
    let json = problems.to_json();
    assert_eq!(json[0]["code"], "kernel.unknown_key");
    assert_eq!(json[0]["at"]["line"], 2);
    let failure = a2m_core::Failure::from(problems);
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code().as_str(), "lint_failed");
    assert!(
        failure
            .message()
            .starts_with("operations/order_get.yaml:2:1")
    );
}

#[test]
fn an_expression_with_a_type_error_is_reported_at_its_place_in_the_file() {
    let problems = problems_of(&Case::Edit(
        SHEET,
        "result.status == 'draft'",
        "result.status + 1",
    ));
    let problem = problems
        .iter()
        .find(|problem| problem.code.as_str().starts_with("kernel.expression_"))
        .unwrap_or_else(|| panic!("{problems}"));
    assert_eq!(problem.at.file.as_str(), SHEET);
    assert_eq!(problem.at.line, 13);
    assert!(problem.message.contains("result.status + 1"), "{problem}");
    assert!(!problem.fix.is_empty());
}

#[test]
fn every_kernel_lint_code_has_a_case_that_fails_with_it_and_the_example_passes() {
    let cases = cases();
    let covered: BTreeSet<&str> = cases.iter().map(|(code, _)| *code).collect();
    for (code, _) in KERNEL_CODES {
        assert!(covered.contains(code), "{code} has no failing case");
    }
    assert_eq!(
        covered.len(),
        KERNEL_CODES.len(),
        "a case names a code the kernel does not have"
    );
    for (code, case) in &cases {
        if let Case::Elsewhere(test) = case {
            assert!(
                test.starts_with("the_loader") || test.starts_with("what_a_faulty"),
                "{code}"
            );
            continue;
        }
        let problems = problems_of(case);
        let found = problems
            .find(code)
            .unwrap_or_else(|| panic!("{code} was not reported:\n{problems}"));
        assert!(!found.fix.is_empty() && !found.message.is_empty(), "{code}");
        assert!(found.at.line >= 1 && found.at.column >= 1, "{code}");
    }
    // The pack's own registered rule, both ways.
    let set = stand_set().unwrap();
    assert!(
        set.lint_codes()
            .iter()
            .any(|(code, _)| code == "stand.path_not_rooted")
    );
    let unrooted = changed(&Case::Edit(
        "calls/orders.yaml",
        "path: \"/orders/{order_id}\"",
        "path: \"orders/{order_id}\"",
    ));
    let published = problems_with(&unrooted, &set, CompileMode::Publish);
    assert_eq!(
        published.find("stand.path_not_rooted").unwrap().level,
        Level::Error
    );
    let drafted = problems_with(&unrooted, &set, CompileMode::Draft);
    assert_eq!(
        drafted.find("stand.path_not_rooted").unwrap().level,
        Level::Warning
    );
}

#[test]
fn test_rules_are_warnings_for_a_draft_and_errors_for_a_publish() {
    let files = changed(&Case::Remove("tests/order_get.yaml"));
    let set = stand_set().unwrap();
    let draft = problems_with(&files, &set, CompileMode::Draft);
    assert!(!draft.has_errors(), "{draft}");
    assert_eq!(
        draft.find("kernel.operation_without_test").unwrap().level,
        Level::Warning
    );
    let publish = problems_with(&files, &set, CompileMode::Publish);
    assert!(publish.has_errors());
}

#[test]
fn without_the_pack_its_surface_kind_file_kind_and_mode_do_not_compile() {
    let files = example_files();
    let problems = problems_with(&files, &CompilerSet::kernel_only(), CompileMode::Publish);
    // The folder nobody registered stops the load.
    assert_eq!(
        problems
            .find("kernel.unknown_file")
            .unwrap()
            .at
            .file
            .as_str(),
        "calls/orders.yaml"
    );
    let without_calls: Vec<_> = files
        .into_iter()
        .filter(|(path, _)| path.folder() != Some("calls"))
        .collect();
    let problems = problems_with(
        &without_calls,
        &CompilerSet::kernel_only(),
        CompileMode::Publish,
    );
    assert!(
        problems.find("kernel.unknown_surface_kind").is_some(),
        "{problems}"
    );
    let mode = problems.find("kernel.unknown_mode").unwrap();
    assert_eq!((mode.at.file.as_str(), mode.at.line), (UPSERT, 13));
}

#[test]
fn a_source_definition_that_the_readers_could_not_load_is_refused_when_it_is_compiled() {
    const SOURCE: &str = "sources/order_sheet_v2.yaml";
    // An expression over `row`, whose cells have no type until a file is
    // read, must end in a type of its own.
    let problems = problems_of(&Case::Edit(SOURCE, "upper(row.Code)", "row.Code"));
    let problem = problems.find("kernel.expression_type_error").unwrap();
    assert_eq!((problem.at.file.as_str(), problem.at.line), (SOURCE, 16));
    // A digest that is not the digest of its parts, with the right one as the fix.
    let problems = problems_of(&Case::Edit(SOURCE, "value: \"Length\"", "value: \"Width\""));
    let problem = problems.find("kernel.unknown_value").unwrap();
    assert_eq!(problem.at.line, 6);
    assert!(problem.fix.contains("digest: \""), "{problem}");
    for (find, replace, code) in [
        (
            "column: \"Code\"",
            "column: \"Item\"",
            "kernel.unknown_name",
        ),
        ("key: [code]", "key: [item]", "kernel.unknown_name"),
        (
            "columns: [\"Code\", \"Length\"]",
            "columns: [\"Code\", \"Code\"]",
            "kernel.name_defined_twice",
        ),
        (
            "column: \"Length\"}",
            "column: \"Length\", expression: \"1\"}",
            "kernel.missing_field",
        ),
        (
            "hidden: reject",
            "hidden: reject\noptions: {delimiter: \"ab\"}",
            "kernel.unknown_value",
        ),
        (
            "type: length_ft_in",
            "type: length_in_ft",
            "kernel.unknown_field_type",
        ),
    ] {
        let problems = problems_of(&Case::Edit(SOURCE, find, replace));
        assert!(problems.find(code).is_some(), "{replace}: {problems}");
    }
    // A source read from a file knows at least one template version.
    let text = String::from_utf8(
        example_files()
            .into_iter()
            .find(|(path, _)| path.as_str() == SOURCE)
            .unwrap()
            .1,
    )
    .unwrap();
    let without: String = text
        .lines()
        .filter(|line| {
            !(1..=9).contains(&text.lines().position(|l| l == *line).unwrap_or(0))
                || line.starts_with("template")
                || line.starts_with("reader")
        })
        .map(|line| format!("{line}\n"))
        .collect();
    assert!(!without.contains("versions"));
    let mut files = example_files();
    files
        .iter_mut()
        .find(|(path, _)| path.as_str() == SOURCE)
        .unwrap()
        .1 = without.into_bytes();
    let problems = problems_with(&files, &stand_set().unwrap(), CompileMode::Publish);
    assert_eq!(
        problems
            .find("kernel.missing_field")
            .unwrap()
            .at
            .file
            .as_str(),
        SOURCE
    );
}

#[test]
fn commit_points_equal_the_steps_that_commit_in_every_write_binding() {
    let plan = compiled().plan;
    for operation in &plan.operations {
        for binding in &operation.bindings {
            let commits = binding.act.iter().filter(|step| step.commits).count();
            let elsewhere = binding
                .confirm
                .iter()
                .chain(&binding.read_back)
                .chain(&binding.lookup)
                .filter(|step| step.commits)
                .count();
            assert_eq!(elsewhere, 0);
            if operation.effect == EffectClass::Read {
                assert_eq!(commits, 0, "{}", operation.name);
            } else {
                assert_eq!(
                    commits, operation.commit_points as usize,
                    "{}",
                    operation.name
                );
            }
        }
    }
}

#[test]
fn one_edited_file_is_linted_against_the_rest() {
    let set = stand_set().unwrap();
    let sources = load(&example_root(), &set).unwrap();
    let path = RelPath::new(ORDER_GET).unwrap();
    let text = String::from_utf8(
        example_files()
            .into_iter()
            .find(|(known, _)| *known == path)
            .unwrap()
            .1,
    )
    .unwrap();
    let edited = SourceFile::read(
        path.clone(),
        text.replace("type: amount", "type: amout").as_bytes(),
        &set,
    )
    .unwrap();
    let problems = lint_one(&sources, edited, &set);
    let problem = problems.find("kernel.unknown_field_type").unwrap();
    assert_eq!(problem.fix, "Write `amount`.");
    let unchanged = SourceFile::read(path, text.as_bytes(), &set).unwrap();
    // A draft: the example has no warnings either.
    assert!(lint_one(&sources, unchanged, &set).is_empty());
}

#[test]
fn the_loader_never_reads_outside_the_target_folder() {
    for bad in [
        "../secrets.yaml",
        "/etc/passwd",
        "operations/../../x.yaml",
        "C:\\x.yaml",
    ] {
        assert_eq!(
            RelPath::new(bad).unwrap_err().code().as_str(),
            "path_not_allowed"
        );
    }
    // A missing folder is a diagnostic, not a panic.
    let nowhere = TargetRoot::new(std::env::temp_dir().join("a2m-files-no-such-target"));
    let problems = load(&nowhere, &stand_set().unwrap()).unwrap_err();
    assert!(problems.find("kernel.file_missing").is_some());
    #[cfg(unix)]
    {
        let scratch = scratch("link");
        std::fs::write(scratch.join("outside.yaml"), "operation: example_outside\n").unwrap();
        let root = scratch.join("target");
        copy_example(&root);
        std::os::unix::fs::symlink(
            scratch.join("outside.yaml"),
            root.join("operations").join("outside.yaml"),
        )
        .unwrap();
        let problems = load(&TargetRoot::new(&root), &stand_set().unwrap()).unwrap_err();
        let problem = problems.find("kernel.path_outside_target").unwrap();
        assert_eq!(problem.at.file.as_str(), "operations/outside.yaml");
        let failure = a2m_core::Failure::from(problems);
        assert_eq!(failure.code().as_str(), "path_not_allowed");
        std::fs::remove_dir_all(&scratch).unwrap();
    }
}

#[test]
fn a_copy_of_the_target_in_another_folder_compiles_to_the_same_plan() {
    let folder = scratch("copy");
    let root = folder.join("somewhere").join("else");
    copy_example(&root);
    // A name that starts with a dot is passed over.
    std::fs::write(root.join(".notes"), "not a target file").unwrap();
    let set = stand_set().unwrap();
    let sources = load(&TargetRoot::new(&root), &set).unwrap();
    let copy = compile(&sources, &set, CompileMode::Publish, VERSION).unwrap();
    assert_eq!(copy.encoded.bytes, compiled().encoded.bytes);
    std::fs::remove_dir_all(&folder).unwrap();
}

fn scratch(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!("a2m-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn copy_example(root: &Path) {
    for (path, bytes) in example_files() {
        let on_disk = path.under(root);
        std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        std::fs::write(on_disk, bytes).unwrap();
    }
}

/// A lowering that returns what a correct pack never would, by what its
/// binding asks for.
struct Faulty {
    schema: JsonSchema,
    verbs: Vec<Verb>,
    reads: Vec<(FileKindName, SurfaceKind)>,
    step_kinds: Vec<StepKind>,
}

impl Lowering for Faulty {
    fn mode(&self) -> Mode {
        Mode::new("faulty").unwrap()
    }
    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }
    fn default_verb(&self) -> Option<Verb> {
        self.verbs.first().cloned()
    }
    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &self.reads
    }
    fn step_kinds(&self) -> &[StepKind] {
        &self.step_kinds
    }
    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }
    fn identity(&self, _: &ResolvedBinding<'_>, _: IdentityStrategy) -> Possible {
        Possible::Yes
    }
    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let fault = binding
            .field("fault")
            .and_then(|fault| fault.as_text())
            .unwrap_or_default();
        // Another pack's kind is never handed over unless it was declared.
        assert!(
            cx.entries(&FileKindName::new("call").unwrap(), &surface_kind())
                .is_none()
        );
        assert!(
            cx.section(&FileKindName::new("call").unwrap(), &surface_kind())
                .is_none()
        );
        let good = cx.expr_at("input.order_id", &binding.doc.at, None).unwrap();
        let step = |kind: &str, commits: bool| Step {
            kind: StepKind::new(kind).unwrap(),
            refers_to: None,
            args: vec![StepArg {
                name: "order_id".to_owned(),
                value: if fault == "loose" {
                    a2m_core::ExprIndex::new(9_999)
                } else {
                    good
                },
            }],
            timeout_ms: 1_000,
            fails_as: FailsAs {
                class: FailureClass::Drift,
                code: FailureCode::fixed("reply_changed"),
            },
            commits,
            lookup: false,
            idempotent: false,
            when: None,
            payload: PackPayload::none(),
        };
        Ok(Lowered {
            surface: SurfaceName::new(if fault == "surface" {
                "nowhere"
            } else {
                "desk"
            })
            .unwrap(),
            act: vec![step(
                if fault == "step_kind" {
                    "other"
                } else {
                    "fetch"
                },
                false,
            )],
            confirm: vec![],
            read_back: if fault == "commit" {
                vec![step("fetch", true)]
            } else {
                vec![]
            },
            lookup: vec![],
            settle_ms: 0,
            prefer_response: false,
        })
    }
}

fn faulty_set(reads_a_kind_nobody_owns: bool) -> CompilerSet {
    // A pack with one mode, and beside it the stand pack.
    let mut builder = CompilerSet::builder();
    let lowering = Faulty {
        schema: JsonSchema::object().optional("fault", JsonSchema::name()),
        verbs: vec![Verb::new("read").unwrap()],
        reads: if reads_a_kind_nobody_owns {
            vec![(FileKindName::new("ghost").unwrap(), surface_kind())]
        } else {
            vec![]
        },
        step_kinds: vec![StepKind::new("fetch").unwrap()],
    };
    builder.lowering("faulty", Arc::new(lowering)).unwrap();
    crate::testing::register_stand(&mut builder).unwrap();
    builder.build()
}

fn faulty_operation(fault: &str) -> Vec<(RelPath, Vec<u8>)> {
    let text = format!(
        "operation: example_order_probe\neffect: read\nkey: order_id\ninputs:\n  order_id: {{type: identifier}}\noutputs:\n  status: {{type: order_status}}\nbindings:\n  - {{mode: faulty, fault: {fault}}}\ndescription: Looks at one order through the faulty mode of the tests.\n"
    );
    let mut files = example_files();
    files.push((
        RelPath::new("operations/order_probe.yaml").unwrap(),
        text.into_bytes(),
    ));
    files
}

#[test]
fn what_a_faulty_lowering_returns_is_refused() {
    let set = faulty_set(false);
    let draft = |fault: &str| problems_with(&faulty_operation(fault), &set, CompileMode::Draft);
    assert!(!draft("none").has_errors(), "{}", draft("none"));
    for (fault, code) in [
        ("step_kind", "kernel.unknown_step_kind"),
        ("surface", "kernel.lowering_invalid"),
        ("commit", "kernel.lowering_invalid"),
        ("loose", "kernel.plan_not_encodable"),
    ] {
        let problems = draft(fault);
        let problem = problems
            .find(code)
            .unwrap_or_else(|| panic!("{fault}: {problems}"));
        assert_eq!(problem.level, Level::Error);
    }
    // A mode that reads a kind no enabled pack owns compiles nothing.
    let problems = problems_with(
        &faulty_operation("none"),
        &faulty_set(true),
        CompileMode::Draft,
    );
    let problem = problems.find("kernel.read_kind_not_registered").unwrap();
    assert_eq!(problem.at.file.as_str(), "operations/order_probe.yaml");
    // And the example, which does not use the mode, still compiles.
    assert!(!problems_with(&example_files(), &faulty_set(true), CompileMode::Publish).has_errors());
}

#[test]
fn a_second_registration_for_the_same_key_is_refused() {
    let mut builder = CompilerSet::builder();
    crate::testing::register_stand(&mut builder).unwrap();
    let failure = crate::testing::register_stand(&mut builder).unwrap_err();
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code().as_str(), "registration_conflict");
    // Each kind of registration on its own, under another pack's name.
    for part in crate::testing::STAND_PARTS {
        let mut builder = CompilerSet::builder();
        crate::testing::register_stand(&mut builder).unwrap();
        let failure = crate::testing::register_part(&mut builder, "other", part).unwrap_err();
        assert_eq!(failure.code().as_str(), "registration_conflict", "{part}");
    }
    // The kernel's own names are not a pack's to register.
    let mut builder = CompilerSet::builder();
    let failure = crate::testing::register_part(&mut builder, "kernel", "control").unwrap_err();
    assert_eq!(failure.code().as_str(), "registration_conflict");
}

#[test]
fn every_schema_is_published() {
    let schemas = stand_set().unwrap().schemas();
    for name in [
        "kernel.target",
        "kernel.auth",
        "kernel.types",
        "kernel.operation",
        "kernel.pipeline",
        "kernel.source",
        "kernel.test",
        "kernel.fixture",
        "kernel.surface",
        "kernel.binding",
        "stand.call.stand",
        "stand.binding.stand.read",
        "stand.surface.stand",
    ] {
        assert_eq!(schemas[name]["type"], "object", "{name}");
    }
    assert_eq!(
        crate::COMPILER_VERSION.to_string(),
        env!("CARGO_PKG_VERSION")
    );
}
