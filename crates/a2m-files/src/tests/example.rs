//! The `example` target compiles, the same way every time, and each thing
//! the compiler must refuse is refused with a file, a line, a column, a
//! code, and a fix.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::codes::KERNEL_CODES;
use crate::testing::{example_root, stand_set, surface_kind};
use crate::{
    CompileMode, Compiled, CompilerSet, DRAFT_VERSION, Diagnostic, Diagnostics, JsonSchema, Level,
    LintRule, LowerContext, Lowered, Lowering, Possible, ProgramField, RelPath, ResolvedBinding,
    ResolvedView, SourceFile, Stage, StepCommits, SurfaceKindSpec, TargetRoot, compile, lint_one,
    load, load_files,
};
use a2m_core::{
    ApproveWrites, CaptureLevel, DecimalMark, DeclaredProgram, Digest, DriftRisk, EffectClass,
    FailsAs, FailureClass, FailureCode, FieldFormat, FileKindName, IdentityStrategy, Mode,
    PackPayload, Plan, ProbePlan, RecordKey, SignInMode, Stands, Step, StepArg, StepKind,
    SurfaceIndex, SurfaceKind, SurfaceName, Value, Verb, Version,
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
/// The version a test compiles under: a draft's own, or a release.
fn version_for(mode: CompileMode) -> Version {
    match mode {
        CompileMode::Draft => DRAFT_VERSION,
        CompileMode::Publish => VERSION,
    }
}

fn problems_with(
    files: &[(RelPath, Vec<u8>)],
    set: &CompilerSet,
    mode: CompileMode,
) -> Diagnostics {
    let root = TargetRoot::new("example");
    match load_files(&root, files, set) {
        Err(problems) => problems,
        Ok(sources) => match compile(&sources, set, mode, version_for(mode)) {
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
            "kernel.program_invalid",
            Case::Edit(
                "target.yaml",
                "    address: \"https://app.example.com\"\n",
                "    address: \"https://app.example.com\"\n    helper: bin/example-helper\n",
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
            "kernel.probe_invalid",
            Case::Edit(
                "auth.yaml",
                "probe: example_order_get",
                "probe: example_line_item_upsert",
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
        (
            "kernel.draft_under_release_version",
            Case::Elsewhere(
                "what_a_faulty_caller_asks_for_a_draft_under_a_release_version_is_refused",
            ),
        ),
        (
            "kernel.file_too_large",
            Case::Elsewhere("the_loader_reads_a_file_of_bounded_size_and_a_folder_once"),
        ),
        (
            "kernel.scope_invalid",
            Case::Edit(
                UPSERT,
                "description: >",
                "permissions: {destinations: [\"*\"]}\ndescription: >",
            ),
        ),
        (
            "kernel.surface_of_another_pack",
            Case::Elsewhere("what_a_faulty_lowering_returns_is_refused"),
        ),
        (
            "kernel.credential_suspected",
            Case::Edit(
                "tests/order_get.yaml",
                "expect: {status: draft,",
                "expect: {token: \"Example-Token-0001\", status: draft,",
            ),
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

    // What the target states as a whole.
    assert_eq!(plan.target.title, "Example order entry");
    assert_eq!(plan.target.locale.as_deref(), Some("en-US"));
    assert_eq!(plan.target.timezone.as_deref(), Some("UTC"));
    assert_eq!(plan.target.drift_risk, Some(DriftRisk::Low));
    assert!(!plan.target.exclusive_session);
    let default = plan.default_environment().unwrap();
    assert_eq!(default.name.as_str(), "test");
    assert_eq!(
        default.test_records,
        [RecordKey::new(vec![Value::from("SO-1001")]).unwrap()]
    );
    assert_eq!(default.approve_writes, None);

    // The sign-in, as the session keeper reads it.
    let desk = SurfaceIndex::new(0);
    let auth = plan.auth_of(desk).unwrap();
    assert_eq!(auth.surfaces, [desk]);
    assert_eq!(auth.mode, SignInMode::ManagedSession);
    assert_eq!(auth.provider.as_str(), "form_login");
    assert_eq!((auth.idle_ms, auth.absolute_ms), (Some(1_800_000), None));
    assert_eq!((auth.attempt_limit, auth.movable), (1, true));
    assert_eq!(auth.settings.len(), 1);
    assert_eq!(
        (
            auth.settings[0].name.as_str(),
            auth.settings[0].value.as_str()
        ),
        ("sign_in_path", "/login")
    );
    assert_eq!(auth.credentials[0].role, "password");
    assert_eq!(
        auth.credentials[0].reference.as_str(),
        "vault://example/service/password"
    );
    let ProbePlan::Operation { operation, inputs } = &auth.probe else {
        panic!("the probe is an operation");
    };
    assert_eq!(
        plan.operation(*operation).unwrap().name.as_str(),
        "example_order_get"
    );
    assert_eq!(inputs.get("order_id"), Some(&Value::from("SO-1001")));

    // What the tool surface shows a caller.
    let get = plan.operation(*operation).unwrap();
    assert_eq!(get.examples.len(), 1);
    assert_eq!(
        get.examples[0].get("order_id"),
        Some(&Value::from("SO-1001"))
    );
    assert!(get.decision_points.is_empty());

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
    // What a driver needs to write a value, as data beside the normalizer.
    assert_eq!(length.step.as_ref().unwrap().to_string(), "0.0625");
    assert_eq!(
        length.format,
        FieldFormat::Number {
            decimal_mark: DecimalMark::Point
        }
    );

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
    // A carriage return that stands alone is a line ending too.
    let old_style: Vec<(RelPath, Vec<u8>)> = unix
        .iter()
        .map(|(path, bytes)| {
            let text = String::from_utf8(bytes.clone()).unwrap();
            let text = if path.as_str().ends_with(".yaml") {
                text.replace('\n', "\r")
            } else {
                text
            };
            (path.clone(), text.into_bytes())
        })
        .collect();
    assert_ne!(old_style, unix);
    assert_eq!(
        compile_files(&old_style).encoded.bytes,
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
        UPSERT,
        "title: Create or update a line item",
        "titel: Create or update a line item",
    ));
    assert!(problems.has_errors());
    let problem = problems.find("kernel.unknown_key").unwrap();
    assert_eq!(problem.at.file.as_str(), UPSERT);
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
            .starts_with("operations/line_item_upsert.yaml:2:1")
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
    fn commits(&self, kind: &StepKind) -> StepCommits {
        match kind.as_str() {
            "fetch" => StepCommits::Never,
            "save" => StepCommits::Always,
            _ => StepCommits::AsDeclared,
        }
    }
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        match binding.field("fault").and_then(|fault| fault.as_text()) {
            Some("sends") => EffectClass::Irreversible,
            Some("changes") => EffectClass::Write,
            _ => EffectClass::Read,
        }
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
            surface: SurfaceName::new(match fault {
                "surface" => "nowhere",
                // A surface of the kind another pack registered.
                "foreign" => "desk",
                _ => "bench",
            })
            .unwrap(),
            act: vec![match fault {
                "step_kind" => step("other", false),
                // A kind that always saves, not marked as a save.
                "unmarked" => step("save", false),
                // A kind that never saves, marked as one.
                "marked" => step("fetch", true),
                _ => step("fetch", false),
            }],
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
        step_kinds: vec![
            StepKind::new("fetch").unwrap(),
            StepKind::new("save").unwrap(),
        ],
    };
    builder.lowering("faulty", Arc::new(lowering)).unwrap();
    builder
        .surface_kind(
            "faulty",
            SurfaceKindSpec::new(
                SurfaceKind::new("bench").unwrap(),
                "address",
                JsonSchema::object().required("address", JsonSchema::text()),
            ),
        )
        .unwrap();
    crate::testing::register_stand(&mut builder).unwrap();
    builder.build()
}

fn faulty_operation(fault: &str) -> Vec<(RelPath, Vec<u8>)> {
    let text = format!(
        "operation: example_order_probe\neffect: read\nkey: order_id\ninputs:\n  order_id: {{type: identifier}}\noutputs:\n  status: {{type: order_status}}\nbindings:\n  - {{mode: faulty, fault: {fault}}}\ndescription: Looks at one order through the faulty mode of the tests.\n"
    );
    // The faulty pack's own surface, beside the example's.
    let mut files = changed(&Case::Edit(
        "target.yaml",
        "surfaces:\n",
        "surfaces:\n  bench:\n    kind: bench\n    address: \"https://bench.example.com\"\n",
    ));
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
        // The mode is one pack's and the surface's kind another's.
        ("foreign", "kernel.surface_of_another_pack"),
        ("step_kind", "kernel.unknown_step_kind"),
        ("surface", "kernel.lowering_invalid"),
        ("commit", "kernel.lowering_invalid"),
        ("loose", "kernel.plan_not_encodable"),
        // Whether a kind of step saves is the pack's word for the kind, and
        // a step that says otherwise is refused, either way.
        ("unmarked", "kernel.lowering_invalid"),
        ("marked", "kernel.lowering_invalid"),
        // A binding that writes or sends by what it does, under `read`.
        ("changes", "kernel.effect_lower_than_derived"),
        ("sends", "kernel.effect_lower_than_derived"),
    ] {
        let problems = draft(fault);
        let problem = problems
            .find(code)
            .unwrap_or_else(|| panic!("{fault}: {problems}"));
        assert_eq!(problem.level, Level::Error);
    }
    // A step marked as a save in a read reaches no plan by another road
    // either: the effect is refused as well as the mark.
    assert!(
        draft("marked")
            .find("kernel.effect_lower_than_derived")
            .is_some()
    );
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

/// The example target with every edit made, compiled.
fn compiled_with(edits: &[Case]) -> Compiled {
    let set = stand_set().unwrap();
    let mut files = example_files();
    for edit in edits {
        let Case::Edit(path, find, replace) = edit else {
            panic!("only edits");
        };
        let file = files
            .iter_mut()
            .find(|(known, _)| known.as_str() == *path)
            .unwrap();
        let text = String::from_utf8(file.1.clone()).unwrap();
        assert!(text.contains(find), "`{find}` is not in {path}");
        file.1 = text.replacen(find, replace, 1).into_bytes();
    }
    let sources = load_files(&TargetRoot::new("example"), &files, &set)
        .unwrap_or_else(|problems| panic!("{problems}"));
    compile(&sources, &set, CompileMode::Publish, VERSION)
        .unwrap_or_else(|problems| panic!("{problems}"))
}

#[test]
fn the_plan_carries_what_the_run_time_needs_from_every_target_file() {
    let compiled = compiled_with(&[
        Case::Edit(
            "target.yaml",
            "drift_risk: low\n",
            "drift_risk: high\nexclusive_session: true\nauthorization: wrap-2026-11\n\
             modules: [order, line]\n\
             defaults: {capture: full, limits: {max_duration_ms: 600000}, timeouts: {step: 2500}}\n",
        ),
        Case::Edit(
            "target.yaml",
            "    rate: {requests: 10, per_ms: 1000}\n",
            "    rate: {requests: 10, per_ms: 1000}\n  \
             files:\n    kind: stand\n    address: \"https://files.example.com\"\n    \
             stands: {beneath: desk}\n    same_session_as: desk\n  \
             vault_db:\n    kind: stand\n    address: \"https://db.example.com\"\n    stands: alone\n",
        ),
        Case::Edit(
            "target.yaml",
            "    test_records: [\"SO-1001\"]\n",
            "    test_records: [\"SO-1001\", [\"SO-1001\", 7]]\n    approve_writes: named_approver\n",
        ),
        Case::Edit(
            "auth.yaml",
            "    attempt_limit: 1\n",
            "    attempt_limit: 0\n    movable: false\n    sign_out: order_get\n    \
             expiry_signals:\n      - {status: 401}\n      - {redirect_host: \"login.example.com\"}\n    \
             timeouts: {absolute_ms: 28800000}\n",
        ),
        Case::Edit("auth.yaml", "    timeouts: {idle_ms: 1800000}\n", ""),
        Case::Edit(
            ORDER_GET,
            "bindings:",
            "decision_points:\n  - {name: which_order, question: \"Two orders match. Which one?\"}\n\
             bindings:",
        ),
    ]);
    let plan = &compiled.plan;
    assert_eq!(plan.target.drift_risk, Some(DriftRisk::High));
    assert!(plan.target.exclusive_session);
    assert_eq!(plan.target.authorization.as_deref(), Some("wrap-2026-11"));
    assert_eq!(plan.target.modules, ["order", "line"]);
    assert_eq!(plan.target.capture, Some(CaptureLevel::Full));
    // A default limit reaches the pipeline that states none; the engine applies no default.
    assert_eq!(plan.pipelines[0].limits.max_records, Some(50));
    assert_eq!(plan.pipelines[0].limits.max_duration_ms, Some(600_000));
    // A tool knows its module.
    let module = |name: &str| {
        plan.tools
            .iter()
            .find(|tool| tool.name.as_str() == name)
            .unwrap()
            .module
            .clone()
    };
    assert_eq!(module("example_order_get").as_deref(), Some("order"));
    assert_eq!(module("example_line_item_upsert").as_deref(), Some("line"));

    assert_eq!(plan.surfaces[0].stands, None);
    assert_eq!(
        plan.surfaces[1].stands,
        Some(Stands::Beneath(SurfaceIndex::new(0)))
    );
    assert_eq!(plan.surfaces[2].stands, Some(Stands::Alone));
    // A surface that shares a sign-in is signed in to as the other is.
    assert_eq!(
        plan.auth_of(SurfaceIndex::new(1)),
        plan.auth_of(SurfaceIndex::new(0))
    );
    assert!(plan.auth_of(SurfaceIndex::new(2)).is_none());

    let test = plan.default_environment().unwrap();
    assert_eq!(test.approve_writes, Some(ApproveWrites::NamedApprover));
    assert_eq!(
        test.test_records[1],
        RecordKey::new(vec![Value::from("SO-1001"), Value::Int(7)]).unwrap()
    );

    let auth = &plan.auth[0];
    assert_eq!((auth.attempt_limit, auth.movable), (0, false));
    assert_eq!((auth.idle_ms, auth.absolute_ms), (None, Some(28_800_000)));
    assert_eq!(auth.sign_out.as_deref(), Some("order_get"));
    assert_eq!(auth.expiry_signals.len(), 2);
    assert_eq!(
        auth.expiry_signals[0]
            .decode::<serde_json::Value>()
            .unwrap(),
        serde_json::json!({"status": 401})
    );

    let get = plan
        .operations
        .iter()
        .find(|op| op.name.as_str() == "example_order_get")
        .unwrap();
    assert_eq!(get.examples.len(), 1);
    assert_eq!(
        get.examples[0].get("order_id"),
        Some(&Value::from("SO-1001"))
    );
    assert_eq!(get.decision_points[0].name, "which_order");
    assert_eq!(
        get.decision_points[0].question,
        "Two orders match. Which one?"
    );

    // All of it is in the bytes, and comes back.
    let decoded = Plan::decode(&compiled.encoded.bytes, &compiled.encoded.hash).unwrap();
    assert_eq!(&decoded, plan);
}

#[test]
fn a_probe_is_a_read_operation_with_its_inputs_or_an_entry_of_a_pack() {
    // A call the stand pack defines on the entry's surface.
    let compiled = compiled_with(&[Case::Edit(
        "auth.yaml",
        "    probe: example_order_get\n    probe_inputs: {order_id: \"SO-1001\"}\n",
        "    probe: line_item_get\n",
    )]);
    let ProbePlan::Entry(reference) = &compiled.plan.auth[0].probe else {
        panic!("the probe is a pack's entry");
    };
    assert_eq!(reference.entry, "line_item_get");
    assert_eq!(
        compiled.plan.section(reference.section).unwrap().pack,
        "stand"
    );

    let refused = |find: &'static str, replace: &'static str, code: &str, says: &str| {
        let problems = problems_of(&Case::Edit("auth.yaml", find, replace));
        let problem = problems
            .find(code)
            .unwrap_or_else(|| panic!("{code} was not reported:\n{problems}"));
        assert_eq!(problem.at.file.as_str(), "auth.yaml");
        assert!(problem.message.contains(says), "{problem}");
    };
    // A read that needs an input the entry does not give.
    refused(
        "    probe_inputs: {order_id: \"SO-1001\"}\n",
        "",
        "kernel.probe_invalid",
        "order_id",
    );
    refused(
        "probe_inputs: {order_id: \"SO-1001\"}",
        "probe_inputs: {order_id: 7}",
        "kernel.value_not_of_type",
        "order_id",
    );
    refused(
        "probe_inputs: {order_id: \"SO-1001\"}",
        "probe_inputs: {order: \"SO-1001\"}",
        "kernel.unknown_name",
        "order",
    );
    refused(
        "probe: example_order_get",
        "probe: example_nothing",
        "kernel.unknown_name",
        "example_nothing",
    );
    // A surface is signed in to once.
    refused(
        "surfaces: [desk]",
        "surfaces: [desk, desk]",
        "kernel.probe_invalid",
        "twice",
    );
}

#[test]
fn a_credential_written_in_the_auth_file_is_refused_and_never_shown() {
    let problems = problems_of(&Case::Edit(
        "auth.yaml",
        "vault://example/service/password",
        "correct-horse-battery",
    ));
    assert!(problems.find("kernel.credential_in_file").is_some());
    assert!(!problems.to_string().contains("horse"), "{problems}");
    // A reference that is not one the vault could resolve is refused the same way.
    let problems = problems_of(&Case::Edit(
        "auth.yaml",
        "vault://example/service/password",
        "vault://example/../password",
    ));
    assert!(problems.find("kernel.credential_in_file").is_some());
}

#[test]
fn what_the_new_parts_refuse() {
    let refused = |path: &'static str, find: &'static str, replace: &'static str, code: &str| {
        let problems = problems_of(&Case::Edit(path, find, replace));
        assert!(
            problems.find(code).is_some(),
            "{code} was not reported:\n{problems}"
        );
    };
    // A viewport belongs to a surface of the kind that has one, not to the target.
    refused(
        "target.yaml",
        "environment: {locale: en-US, timezone: UTC}",
        "environment: {locale: en-US, timezone: UTC, viewport: [1440, 900]}",
        "kernel.unknown_key",
    );
    refused(
        "target.yaml",
        "timezone: UTC",
        "timezone: Europe/Example",
        "kernel.unknown_value",
    );
    refused(
        "target.yaml",
        "test_records: [\"SO-1001\"]",
        "test_records: [[\"SO-1001\", [1]]]",
        "kernel.wrong_type",
    );
    refused(
        ORDER_GET,
        "  - {order_id: \"SO-1001\"}",
        "  - {order: \"SO-1001\"}",
        "kernel.unknown_name",
    );
    refused(
        ORDER_GET,
        "  - {order_id: \"SO-1001\"}",
        "  - {order_id: 7}",
        "kernel.value_not_of_type",
    );
    refused(
        ORDER_GET,
        "bindings:",
        "decision_points:\n  - {name: which, question: \"Which?\"}\n  - {name: which, question: \"Which one?\"}\nbindings:",
        "kernel.name_defined_twice",
    );
}

const DESK_ADDRESS: &str = "    address: \"https://app.example.com\"\n";

#[test]
fn a_surface_kind_that_names_a_program_puts_it_in_the_plan() {
    // The example states no program, and its plan declares none.
    assert!(compiled().plan.surfaces[0].programs.is_empty());

    let digest = Digest::of(b"example-helper");
    let stated = format!(
        "{DESK_ADDRESS}    helper: /opt/example/bin/example-helper\n    helper_digest: \"sha256:{digest}\"\n"
    );
    let stated: &'static str = stated.leak();
    let compiled = compiled_with(&[Case::Edit("target.yaml", DESK_ADDRESS, stated)]);
    assert_eq!(
        compiled.plan.surfaces[0].programs,
        vec![DeclaredProgram {
            path: "/opt/example/bin/example-helper".to_owned(),
            digest: Some(digest),
        }]
    );
    // The plan's bytes carry it, so a published version pins it.
    let decoded = Plan::decode(&compiled.encoded.bytes, &compiled.encoded.hash).unwrap();
    assert_eq!(decoded.surfaces[0].programs.len(), 1);

    // A path on another system is absolute too, and a digest is optional.
    let compiled = compiled_with(&[Case::Edit(
        "target.yaml",
        DESK_ADDRESS,
        "    address: \"https://app.example.com\"\n    helper: 'C:\\example\\example-helper.exe'\n",
    )]);
    assert_eq!(
        compiled.plan.surfaces[0].programs,
        vec![DeclaredProgram {
            path: "C:\\example\\example-helper.exe".to_owned(),
            digest: None,
        }]
    );
}

#[test]
fn a_program_named_any_other_way_is_refused_with_a_file_a_line_and_a_fix() {
    let refused = |replace: &'static str, line: u32, says: &str| {
        let problems = problems_of(&Case::Edit("target.yaml", DESK_ADDRESS, replace));
        let problem = problems
            .find("kernel.program_invalid")
            .unwrap_or_else(|| panic!("{replace}:\n{problems}"));
        assert_eq!(problem.at.file.as_str(), "target.yaml");
        assert_eq!(problem.at.line, line, "{problem}");
        assert!(problem.message.contains(says), "{problem}");
        assert!(!problem.fix.is_empty());
    };
    for relative in ["example-helper", "bin/example-helper", "./example-helper"] {
        let replace = format!("{DESK_ADDRESS}    helper: {relative}\n");
        refused(replace.leak(), 11, "absolute path");
    }
    refused(
        "    address: \"https://app.example.com\"\n    helper: /opt/example/bin/example-helper\n    helper_digest: abc\n",
        12,
        "digest",
    );
    let alone = format!(
        "{DESK_ADDRESS}    helper_digest: {}\n",
        Digest::of(b"example-helper")
    );
    refused(alone.leak(), 11, "no program");
    // A path that steps back names another file than it reads as.
    for stepping in [
        "/opt/example/../../tmp/example-helper",
        "/opt/./example-helper",
        "C:\\example\\..\\example-helper.exe",
    ] {
        let replace = format!("{DESK_ADDRESS}    helper: '{stepping}'\n");
        refused(replace.leak(), 11, "steps through");
    }
    // A program another machine serves is declared with its digest.
    let share = "\\\\server\\share\\example-helper.exe";
    let bare = format!("{DESK_ADDRESS}    helper: '{share}'\n");
    refused(bare.leak(), 11, "network share");
    let pinned = format!(
        "{DESK_ADDRESS}    helper: '{share}'\n    helper_digest: {}\n",
        Digest::of(b"example-helper")
    );
    let compiled = compiled_with(&[Case::Edit("target.yaml", DESK_ADDRESS, pinned.leak())]);
    assert_eq!(compiled.plan.surfaces[0].programs[0].path, share);
}

/// The stand pack, with its surface kind reached by starting a program: the
/// address is the program's path.
fn program_set() -> CompilerSet {
    let mut builder = CompilerSet::builder();
    for part in crate::testing::STAND_PARTS {
        if *part != "surface_kind" {
            crate::testing::register_part(&mut builder, crate::testing::PACK, part).unwrap();
        }
    }
    let fields = JsonSchema::object()
        .required("address", JsonSchema::text())
        .optional("digest", JsonSchema::text());
    builder
        .surface_kind(
            crate::testing::PACK,
            SurfaceKindSpec::new(surface_kind(), "address", fields)
                .with_program("address", Some("digest")),
        )
        .unwrap();
    builder.build()
}

#[test]
fn where_the_address_is_a_program_each_environments_address_is_declared_too() {
    let set = program_set();
    let digest = Digest::of(b"example-tool");
    let own = format!("    address: /opt/example/bin/example-tool\n    digest: {digest}\n");
    let with = |test_address: &'static str| {
        let mut files = changed(&Case::Edit("target.yaml", DESK_ADDRESS, own.clone().leak()));
        let target = files
            .iter_mut()
            .find(|(path, _)| path.as_str() == "target.yaml")
            .unwrap();
        let text = String::from_utf8(target.1.clone()).unwrap();
        assert!(text.contains("\"https://test.example.com\""));
        target.1 = text
            .replacen("\"https://test.example.com\"", test_address, 1)
            .into_bytes();
        files
    };
    let files = with("/opt/example-test/bin/example-tool");
    let sources = load_files(&TargetRoot::new("example"), &files, &set).unwrap();
    let compiled = compile(&sources, &set, CompileMode::Publish, VERSION)
        .unwrap_or_else(|problems| panic!("{problems}"));
    let surface = &compiled.plan.surfaces[0];
    assert_eq!(surface.address, "/opt/example/bin/example-tool");
    assert_eq!(
        surface.programs,
        vec![
            DeclaredProgram {
                path: "/opt/example/bin/example-tool".to_owned(),
                digest: Some(digest),
            },
            DeclaredProgram {
                path: "/opt/example-test/bin/example-tool".to_owned(),
                digest: Some(digest),
            },
        ]
    );

    // An environment cannot bring in a program by a relative path either.
    let problems = problems_with(&with("bin/example-tool"), &set, CompileMode::Publish);
    let problem = problems.find("kernel.program_invalid").unwrap();
    assert_eq!(problem.at.file.as_str(), "target.yaml");
    assert_eq!(problem.at.line, 16, "{problem}");

    // A program field is one of the kind's own fields.
    let mut builder = CompilerSet::builder();
    let failure = builder
        .surface_kind(
            crate::testing::PACK,
            SurfaceKindSpec {
                kind: surface_kind(),
                address_key: "address".to_owned(),
                fields: JsonSchema::object().required("address", JsonSchema::text()),
                programs: vec![ProgramField {
                    path_key: "address".to_owned(),
                    digest_key: Some("digest".to_owned()),
                }],
            },
        )
        .err()
        .unwrap();
    assert_eq!(failure.code().as_str(), "registration_conflict");
}

// ---- what the safety audit found, each with the test that showed it ----

const READ_CALL: &str =
    "---\ncall: line_item_touch\npath: \"/orders/{order_id}/items/{item_code}\"\neffect: read\n";

fn with_call(mut files: Vec<(RelPath, Vec<u8>)>) -> Vec<(RelPath, Vec<u8>)> {
    let calls = files
        .iter_mut()
        .find(|(path, _)| path.as_str() == "calls/orders.yaml")
        .unwrap();
    calls.1.extend_from_slice(READ_CALL.as_bytes());
    files
}

/// The route the audit gave: a call whose file says `read`, sent by the
/// verb that writes, under an operation that declares `read`. It once
/// compiled to a read tool whose step writes, with no intent recorded.
#[test]
fn a_write_never_compiles_as_a_read_whatever_a_target_file_says_of_its_call() {
    let set = stand_set().unwrap();
    let operation = |effect: &str, more: &str| {
        format!(
            "operation: example_line_item_touch\neffect: {effect}\n{more}inputs:\n  order_id: {{type: identifier, description: \"Order number shown in the application\"}}\n  item_code: {{type: identifier, description: \"Item code, such as L-101\"}}\noutputs:\n  length: {{type: length_ft_in}}\nbindings:\n  - mode: stand\n    verb: upsert\n    status: shadow\n    call: line_item_touch\n    read_back: {{call: line_item_get}}\ndescription: >\n  Touches one line item on an order, for a test of the compiler.\n  Prefer example_line_item_upsert to change one.\n"
        )
    };
    let compile_with = |text: String| {
        let mut files = with_call(example_files());
        files.push((
            RelPath::new("operations/line_item_touch.yaml").unwrap(),
            text.into_bytes(),
        ));
        problems_with(&files, &set, CompileMode::Draft)
    };
    // Declared a read: refused, by the verb and by the step that saves.
    let as_read = compile_with(operation("read", "key: [order_id, item_code]\n"));
    let refusal = as_read.find("kernel.effect_lower_than_derived").unwrap();
    assert_eq!(refusal.level, Level::Error);
    assert_eq!(refusal.at.file.as_str(), "operations/line_item_touch.yaml");
    // Declared a write with nothing to record: refused.
    let unrecorded = compile_with(operation(
        "write",
        "key: [order_id, item_code]\ncommit_points: 0\n",
    ));
    assert!(
        unrecorded.iter().any(
            |problem| problem.code.as_str() == "kernel.commit_points_differ"
                && problem.level == Level::Error
                && problem.message.contains("at least once")
        ),
        "{unrecorded}"
    );
    // Declared what it is: it compiles, and its one `send` step commits
    // although its call's file says `read`.
    let honest = compile_with(operation("write", "key: [order_id, item_code]\n"));
    assert!(!honest.has_errors(), "{honest}");
    let mut files = with_call(example_files());
    files.push((
        RelPath::new("operations/line_item_touch.yaml").unwrap(),
        operation("write", "key: [order_id, item_code]\n").into_bytes(),
    ));
    let sources = load_files(&TargetRoot::new("example"), &files, &set).unwrap();
    let plan = compile(&sources, &set, CompileMode::Draft, DRAFT_VERSION)
        .unwrap()
        .plan;
    let touch = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_line_item_touch")
        .unwrap();
    assert_eq!(touch.commit_points, 1);
    assert_eq!(
        touch.bindings[0]
            .act
            .iter()
            .filter(|step| step.commits)
            .count(),
        1
    );

    // A read states no commit point, and a call that writes is never the
    // call of a step that only reads.
    let counted = problems_of(&Case::Edit(
        ORDER_GET,
        "effect: read",
        "effect: read\ncommit_points: 1",
    ));
    let refusal = counted.find("kernel.commit_points_differ").unwrap();
    assert!(
        refusal.message.contains("a read saves nothing"),
        "{refusal}"
    );
    let fetched = problems_of(&Case::Edit(
        UPSERT,
        "read_back: {call: line_item_get}",
        "read_back: {call: line_item_put}",
    ));
    assert!(
        fetched.find("stand.fetch_of_a_write").is_some(),
        "{fetched}"
    );
}

#[test]
fn a_permission_names_one_exact_address_or_one_name() {
    let with = |permissions: &str| {
        let replace = format!("permissions: {permissions}\ndescription: >");
        problems_of(&Case::Edit(UPSERT, "description: >", replace.leak()))
    };
    for bad in [
        "*",
        "*.example.com",
        "https://*",
        "0.0.0.0",
        "0.0.0.0/0",
        "10.0.0.0/8",
        "[::]",
        "https://files.example.com/upload",
        "https://user@files.example.com",
        "files.example.com?x=1",
        "files.example.com:0",
        "files.example.com:99999",
        "files..example.com",
        "files example.com",
        "://files.example.com",
        "",
    ] {
        let problems = with(&format!("{{destinations: [\"{bad}\"]}}"));
        let problem = problems
            .find("kernel.scope_invalid")
            .unwrap_or_else(|| panic!("`{bad}` was accepted:\n{problems}"));
        assert_eq!(problem.level, Level::Error, "{bad}");
        assert_eq!(problem.at.file.as_str(), UPSERT);
    }
    for (key, bad) in [
        ("uploads", "../sheet"),
        ("downloads", "/tmp/out"),
        ("uploads", ""),
    ] {
        let problems = with(&format!("{{{key}: [\"{bad}\"]}}"));
        assert!(
            problems.find("kernel.scope_invalid").is_some(),
            "{key}: {bad}"
        );
    }
    let twice = with("{destinations: [\"files.example.com\", \"files.example.com\"]}");
    assert!(
        twice
            .find("kernel.scope_invalid")
            .unwrap()
            .message
            .contains("twice")
    );
    // What is exact is declared, and reaches the plan's manifest as written.
    let compiled = compiled_with(&[Case::Edit(
        UPSERT,
        "description: >",
        "permissions:\n  uploads: [order_sheet]\n  downloads: [receipt-out]\n  destinations: [\"files.example.com\", \"https://files.example.com:8443\", \"10.0.0.5\", \"[2001:db8::1]:443\"]\ndescription: >",
    )]);
    let manifest = &compiled
        .plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_line_item_upsert")
        .unwrap()
        .manifest;
    assert_eq!(manifest.destinations.len(), 4);
    assert_eq!(manifest.uploads, ["order_sheet"]);
    assert_eq!(manifest.downloads, ["receipt-out"]);
}

#[test]
fn an_open_question_is_refused_wherever_it_stands_even_where_any_value_fits() {
    for replace in [
        "expect: {status: {open_question: \"Which status?\"}, total: \"125.00\", note: \"Call before delivery\"}",
        "expect: {status: [draft, {open_question: \"Which status?\"}]}",
        "expect: {lines: [{status: {deep: {open_question: \"Which status?\"}}}]}",
    ] {
        let problems = problems_of(&Case::Edit(
            "tests/order_get.yaml",
            "expect: {status: draft, total: \"125.00\", note: \"Call before delivery\"}",
            replace,
        ));
        let problem = problems
            .find("kernel.open_question")
            .unwrap_or_else(|| panic!("{replace}:\n{problems}"));
        assert_eq!(problem.level, Level::Error);
        assert!(problem.message.contains("Which status?"), "{problem}");
    }
}

/// A lint rule that quotes what it finds, as a pack's rule might: every
/// text under a test's `expect`, once at the value and once at the start of
/// the document.
struct Echo;

impl LintRule for Echo {
    fn code(&self) -> a2m_core::LintCode {
        a2m_core::LintCode::new("echo.said").unwrap()
    }
    fn level(&self, _: CompileMode) -> Level {
        Level::Warning
    }
    fn summary(&self) -> &str {
        "Says what a test expects."
    }
    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let mut said = Vec::new();
        for doc in view.sources().docs_of("test") {
            for entry in doc
                .get("expect")
                .map(|expect| expect.entries())
                .unwrap_or_default()
            {
                let Some(text) = entry.value.as_text() else {
                    continue;
                };
                for at in [&entry.value.at, &doc.at] {
                    said.push(Diagnostic::error(
                        self.code(),
                        Stage::Lint,
                        at,
                        format!("The test expects `{text}`."),
                        format!("Check `{text}`."),
                    ));
                }
            }
        }
        said
    }
}

#[test]
fn a_value_the_credential_lint_knows_is_reported_and_quoted_by_no_diagnostic() {
    let mut builder = CompilerSet::builder();
    crate::testing::register_stand(&mut builder).unwrap();
    builder.lint_rule("echo", Arc::new(Echo)).unwrap();
    let set = builder.build();
    let with = |expect: &str| {
        let replace = format!("expect: {{{expect}, status: draft,");
        let files = changed(&Case::Edit(
            "tests/order_get.yaml",
            "expect: {status: draft,",
            replace.leak(),
        ));
        problems_with(&files, &set, CompileMode::Publish)
    };
    // A key that names a secret, in the forms the lint did not know: an
    // error, and the value is in no message of any rule.
    for (key, value) in [
        ("refresh_token", "example-token-for-tests-0001"),
        ("x-api-key", "example-key-for-tests-0002"),
        ("passphrase", "example words for tests 0003"),
        ("authorization", "bearer Example-Token-For-Tests-0004"),
        ("remark", "Token Example-Token-For-Tests-0005"),
    ] {
        let problems = with(&format!("{key}: \"{value}\""));
        let problem = problems
            .find("kernel.credential_in_file")
            .unwrap_or_else(|| panic!("{key}:\n{problems}"));
        assert_eq!(problem.level, Level::Error, "{key}");
        let told = format!("{problems} {}", problems.to_json());
        assert!(!told.contains(value), "{key}: {told}");
        assert!(told.contains("[not shown]"), "{key}: {told}");
    }
    // A key that often names a secret and sometimes names one's name: a
    // warning under its own code, and never quoted where it stands.
    for (key, value) in [
        ("token", "Example-Token-0006"),
        ("session_cookie", "Example=Cookie;0007"),
        ("authorization", "Example Value 0008"),
    ] {
        let problems = with(&format!("{key}: \"{value}\""));
        assert!(!problems.has_errors(), "{key}:\n{problems}");
        let warning = problems.find("kernel.credential_suspected").unwrap();
        assert_eq!(warning.level, Level::Warning);
        assert!(!problems.to_string().contains(value), "{key}: {problems}");
    }
    // A fixture's recorded file is looked through as well, by line.
    for recording in [
        "GET /orders HTTP/1.1\nauthorization: Bearer Example-Token-For-Tests-0009\n\n",
        "{\"header\": \"basic RXhhbXBsZTpUb2tlbjAwMTA=\"}",
        "-----BEGIN EXAMPLE KEY-----\nexample\n",
    ] {
        let mut files = example_files();
        files.push((
            RelPath::new("fixtures/order_get/exchange.txt").unwrap(),
            recording.as_bytes().to_vec(),
        ));
        let problems = problems_with(&files, &set, CompileMode::Publish);
        let problem = problems
            .find("kernel.credential_in_file")
            .unwrap_or_else(|| panic!("{recording}:\n{problems}"));
        assert_eq!(problem.at.file.as_str(), "fixtures/order_get/exchange.txt");
        assert!(!problems.to_string().contains("0009"), "{problems}");
    }
    let mut files = example_files();
    files.push((
        RelPath::new("fixtures/order_get/exchange.txt").unwrap(),
        b"GET /orders HTTP/1.1\naccept: application/json\nx-note: basic order data\n".to_vec(),
    ));
    assert!(!problems_with(&files, &set, CompileMode::Publish).has_errors());
    // A secret written as a number under the word itself.
    let pin = with("pin: 48210007");
    assert!(pin.find("kernel.credential_suspected").is_some(), "{pin}");
    // A name under such a key is a name: no report, and it is quoted only
    // by a diagnostic that is not at its place.
    let named = with("cookie: xsrf_cookie, token: input.order_id");
    assert!(
        named.find("kernel.credential_suspected").is_none(),
        "{named}"
    );
    let quotes = named
        .iter()
        .filter(|problem| problem.message.contains("`xsrf_cookie`"))
        .count();
    assert_eq!(quotes, 1, "{named}");
    // A number that is not under the word itself, and plain prose that
    // starts with a scheme's name, are not credentials.
    let plain =
        with("token_ttl_ms: 60000, remark: \"Basic order data\", word: \"token authentication\"");
    assert!(
        plain.find("kernel.credential_suspected").is_none(),
        "{plain}"
    );
    assert!(plain.find("kernel.credential_in_file").is_none(), "{plain}");
    // What no rule knows is still quoted: the status of the order.
    assert!(plain.to_string().contains("`draft`"), "{plain}");
}

/// A draft is never a published plan (decision 0161): compiled under a
/// release version it is refused, and under the draft version it has bytes
/// no published plan has.
#[test]
fn what_a_faulty_caller_asks_for_a_draft_under_a_release_version_is_refused() {
    let set = stand_set().unwrap();
    let sources = load_files(&TargetRoot::new("example"), &example_files(), &set).unwrap();
    let problems = compile(&sources, &set, CompileMode::Draft, VERSION).unwrap_err();
    assert!(
        problems
            .iter()
            .any(|problem| problem.code.as_str() == "kernel.draft_under_release_version"),
        "{problems}"
    );
    let draft = compile(&sources, &set, CompileMode::Draft, DRAFT_VERSION).unwrap();
    assert_ne!(draft.encoded.hash, compiled().encoded.hash);
    assert_eq!(draft.plan.header.version, DRAFT_VERSION);
}

#[test]
fn the_loader_reads_a_file_of_bounded_size_and_a_folder_once() {
    let set = stand_set().unwrap();
    // From memory: one byte over the limit of a target file.
    let mut files = example_files();
    let mut large = b"operation: example_large\n# ".to_vec();
    large.resize(crate::LARGEST_TARGET_FILE + 1, b'x');
    files.push((
        RelPath::new("operations/large.yaml").unwrap(),
        large.clone(),
    ));
    let problems = load_files(&TargetRoot::new("example"), &files, &set).unwrap_err();
    let problem = problems.find("kernel.file_too_large").unwrap();
    assert_eq!(problem.at.file.as_str(), "operations/large.yaml");
    // A fixture's recorded file may be larger than a target file.
    let mut files = example_files();
    files.push((
        RelPath::new("fixtures/order_get/recording.bin").unwrap(),
        large.clone(),
    ));
    assert!(load_files(&TargetRoot::new("example"), &files, &set).is_ok());

    // From disk: the same, without reading the whole file first.
    let folder = scratch("large");
    let root = folder.join("target");
    copy_example(&root);
    std::fs::write(root.join("operations").join("large.yaml"), &large).unwrap();
    let problems = load(&TargetRoot::new(&root), &set).unwrap_err();
    assert!(
        problems.find("kernel.file_too_large").is_some(),
        "{problems}"
    );
    std::fs::remove_file(root.join("operations").join("large.yaml")).unwrap();
    assert!(load(&TargetRoot::new(&root), &set).is_ok());

    // A link to a folder would have it read again, or without end.
    #[cfg(unix)]
    {
        for (link, to) in [
            (root.join("operations").join("again"), root.clone()),
            (root.join("loop"), root.clone()),
            (
                root.join("operations").join("more"),
                root.join("operations"),
            ),
        ] {
            std::os::unix::fs::symlink(&to, &link).unwrap();
            let problems = load(&TargetRoot::new(&root), &set).unwrap_err();
            let problem = problems.find("kernel.path_outside_target").unwrap();
            assert!(problem.message.contains("link to a folder"), "{problem}");
            // One problem for the link, however deep it would have led.
            assert_eq!(problems.len(), 1, "{problems}");
            std::fs::remove_file(&link).unwrap();
        }
        // A link to a file inside the folder is read as that file.
        std::os::unix::fs::symlink(
            root.join("operations").join("order_get.yaml"),
            root.join("operations").join("order_get_again.yaml"),
        )
        .unwrap();
        let problems = load(&TargetRoot::new(&root), &set)
            .and_then(|sources| compile(&sources, &set, CompileMode::Publish, VERSION).map(|_| ()))
            .unwrap_err();
        assert!(problems.find("kernel.path_outside_target").is_none());
        assert!(
            problems.find("kernel.name_defined_twice").is_some(),
            "{problems}"
        );
    }
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_choice_whose_code_is_another_choices_label_is_refused_when_the_type_compiles() {
    let problems = problems_of(&Case::Edit(
        "types.yaml",
        "codes: {draft: \"Draft\", released: \"Released\"}",
        "codes: {draft: \"released\", released: \"Released\"}",
    ));
    assert!(problems.has_errors(), "{problems}");
    let problem = problems
        .iter()
        .find(|problem| problem.message.contains("label of another choice"))
        .unwrap_or_else(|| panic!("{problems}"));
    assert_eq!(problem.at.file.as_str(), "types.yaml");
    // A code that is its own label is one choice.
    let own = compiled_with(&[Case::Edit(
        "types.yaml",
        "codes: {draft: \"Draft\", released: \"Released\"}",
        "codes: {draft: \"draft\", released: \"Released\"}",
    )]);
    assert!(
        own.plan
            .field_types
            .iter()
            .any(|ty| ty.name == "order_status")
    );
}

/// What a binding offers as evidence (decision 0161): an acknowledgement
/// alone only for an operation declared irreversible, which then needs no
/// read-back; and a record that cannot be found again is never read back,
/// because it has no key to be read by.
#[test]
fn an_acknowledgement_is_evidence_only_for_an_irreversible_operation() {
    let set = stand_set().unwrap();
    // A second operation beside the example's, so that no pipeline of the
    // example changes with it.
    let with = |from: &[(&str, &str)]| {
        let mut files = example_files();
        let upsert = files
            .iter()
            .find(|(path, _)| path.as_str() == UPSERT)
            .map(|(_, bytes)| String::from_utf8(bytes.clone()).unwrap())
            .unwrap();
        let mut text = upsert
            .replace("example_line_item_upsert", "example_line_item_send")
            .replace(
                "Creates or updates one line item on an order.",
                "Sends one line item of an order to the stand.",
            );
        for (old, new) in from {
            assert!(text.contains(old), "{old}");
            text = text.replace(old, new);
        }
        files.push((
            RelPath::new("operations/line_item_send.yaml").unwrap(),
            text.into_bytes(),
        ));
        files
    };
    let plan_of = |files: &[(RelPath, Vec<u8>)]| {
        let sources = load_files(&TargetRoot::new("example"), files, &set).unwrap();
        compile(&sources, &set, CompileMode::Draft, DRAFT_VERSION)
            .unwrap_or_else(|problems| panic!("{problems}"))
            .plan
    };
    let binding_of = |plan: &a2m_core::Plan| {
        plan.operations
            .iter()
            .find(|operation| operation.name.as_str() == "example_line_item_send")
            .unwrap()
            .bindings[0]
            .clone()
    };
    let read_back = "    read_back: {call: line_item_get}\n";
    let acknowledged = "    evidence: acknowledged\n";
    let irreversible = "effect: irreversible\nhazard: data_elsewhere";

    let problems = problems_with(&with(&[(read_back, acknowledged)]), &set, CompileMode::Draft);
    let problem = problems
        .find("kernel.effect_lower_than_derived")
        .unwrap_or_else(|| panic!("{problems}"));
    assert!(problem.message.contains("irreversible"), "{problem}");

    let binding = binding_of(&plan_of(&with(&[
        (read_back, acknowledged),
        ("effect: write", irreversible),
    ])));
    assert_eq!(binding.evidence, a2m_core::WriteEvidence::Acknowledged);
    assert!(binding.read_back.is_empty());

    // The response stands where the lowering prefers it.
    let binding = binding_of(&plan_of(&with(&[(
        read_back,
        "    read_back: {call: line_item_get, prefer: response}\n",
    )])));
    assert_eq!(binding.evidence, a2m_core::WriteEvidence::Response);

    // A record that cannot be found again has no key to read it back by.
    let unkeyed = with(&[
        ("key: [order_id, item_code]\n", "identity: {strategy: none}\n"),
        ("effect: write", irreversible),
    ]);
    let problems = problems_with(&unkeyed, &set, CompileMode::Draft);
    let problem = problems
        .iter()
        .find(|problem| problem.code.as_str() == "kernel.identity_invalid")
        .unwrap_or_else(|| panic!("{problems}"));
    assert!(problem.message.contains("no key"), "{problem}");
}

/// A binding's steps may read what the last read of the record gave and the
/// record's key in the system (decision 0161), and nothing else new.
#[test]
fn a_bindings_steps_may_read_the_current_record_and_its_key() {
    let set = stand_set().unwrap();
    let with_args = |args: &str| {
        let mut files = example_files();
        for (path, bytes) in &mut files {
            if path.as_str() == UPSERT {
                let text = String::from_utf8(bytes.clone()).unwrap();
                *bytes = text
                    .replace("args: {length: input.length}", args)
                    .into_bytes();
            }
        }
        files
    };
    let reads = with_args("args: {length: input.length, seen: current.length, id: record.key}");
    let problems = problems_with(&reads, &set, CompileMode::Draft);
    assert!(!problems.has_errors(), "{problems}");
    let unknown = with_args("args: {length: input.length, seen: row.length}");
    let problems = problems_with(&unknown, &set, CompileMode::Draft);
    assert!(
        problems
            .iter()
            .any(|problem| problem.code.as_str() == "kernel.expression_invalid"),
        "{problems}"
    );
}

/// A number a tool returns may reach the caller masked, as the text
/// `[redacted]`; the output schema allows that text where it declares a
/// number, so that a caller that checks the result never fails for it
/// (decision 0161). An input's schema does not.
#[test]
fn a_tools_output_schema_allows_a_masked_number() {
    let compiled = compiled();
    let tool = compiled
        .plan
        .tools
        .iter()
        .find(|tool| tool.name.as_str() == "example_line_item_upsert")
        .unwrap();
    let output = String::from_utf8(tool.output_schema.as_bytes().to_vec()).unwrap();
    assert!(output.contains("[redacted]"), "{output}");
    let input = String::from_utf8(tool.input_schema.as_bytes().to_vec()).unwrap();
    assert!(!input.contains("[redacted]"), "{input}");
}
