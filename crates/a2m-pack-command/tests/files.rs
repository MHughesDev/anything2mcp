//! The pack registers, its `example` files compile to the same bytes every
//! time, and a file that breaks a rule of section 306 is refused with a
//! file, a line, and a fix.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{Archetype, EffectClass, FailureClass, NoSecrets, SurfaceKind};
use a2m_provider::Manifest;
use common::{FILES, Folder, changed, compile, fixture, registry};

#[test]
fn a_registry_with_only_the_command_pack_builds_and_its_digest_is_stable() {
    let folder = Folder::new("registry");
    let first = registry(&folder, Arc::new(NoSecrets));
    let second = registry(&folder, Arc::new(NoSecrets));
    assert_eq!(first.digest(), second.digest());
    let mode = a2m_pack_command::codes::mode();
    assert_eq!(first.archetype(&mode), Some(Archetype::Command));
    assert!(first.driver(&mode).is_ok());
    assert!(
        first
            .surface_kind(&SurfaceKind::new("program").unwrap())
            .is_some()
    );
    let set = first.compiler_set();
    let verbs: Vec<&str> = set.verbs(&mode).into_iter().map(|verb| verb.as_str()).collect();
    assert_eq!(
        verbs,
        ["call", "create", "delete", "list", "read", "search", "update"]
    );

    // The manifest declares every code the driver can raise, each with its
    // one class, and the pack starts only what a plan declares.
    let manifest = Manifest::parse(a2m_pack_command::MANIFEST).unwrap();
    let declared: Vec<(FailureClass, &str)> = manifest
        .failure_codes
        .iter()
        .map(|code| (code.class, code.code.as_str()))
        .collect();
    assert_eq!(declared, a2m_pack_command::codes::ALL);
    assert!(manifest.effects.plan_programs);
    assert!(manifest.effects.processes.is_empty());
}

#[test]
fn the_example_files_compile_twice_to_the_same_bytes() {
    let folder = Folder::new("compile");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let set = registry.compiler_set();
    let files: Vec<(String, String)> = FILES
        .iter()
        .map(|path| ((*path).to_owned(), fixture(path)))
        .collect();
    let first = match compile(&files, set) {
        Ok(compiled) => compiled,
        Err(problems) => panic!("{problems:#?}"),
    };
    let second = compile(&files, set).unwrap();
    assert_eq!(first.encoded.bytes, second.encoded.bytes);
    assert_eq!(first.encoded.hash, second.encoded.hash);
    // From disk, through the loader, the same again.
    let loaded = a2m_files::load(&a2m_files::TargetRoot::new(common::fixtures()), set).unwrap();
    let third = a2m_files::compile(
        &loaded,
        set,
        a2m_files::CompileMode::Draft,
        a2m_core::Version::new(1, 0, 0),
    )
    .unwrap();
    assert_eq!(first.encoded.bytes, third.encoded.bytes);

    // The surface declares its program in every environment, for the
    // supervisor; the plan holds the pack's section.
    let plan = &first.plan;
    let programs: Vec<&str> = plan.surfaces[0]
        .programs
        .iter()
        .map(|program| program.path.as_str())
        .collect();
    assert_eq!(
        programs,
        [
            "/opt/example/bin/example-cli",
            "/opt/example-test/bin/example-cli"
        ]
    );
    assert_eq!(plan.packs.len(), 1);
    assert_eq!(plan.packs[0].pack, "command");

    // A write starts its program in one committing step, sees it end and
    // reads its output to confirm, and reads the record back.
    let set_quantity = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_item_set_quantity")
        .unwrap();
    assert_eq!(set_quantity.effect, EffectClass::Write);
    let binding = &set_quantity.bindings[0];
    let kinds = |steps: &[a2m_core::Step]| -> Vec<(String, bool, bool)> {
        steps
            .iter()
            .map(|step| (step.kind.to_string(), step.commits, step.lookup))
            .collect()
    };
    assert_eq!(kinds(&binding.act), [("spawn".to_owned(), true, false)]);
    assert_eq!(
        kinds(&binding.confirm),
        [
            ("expect".to_owned(), false, false),
            ("parse".to_owned(), false, false)
        ]
    );
    assert_eq!(
        kinds(&binding.read_back),
        [
            ("spawn".to_owned(), false, false),
            ("expect".to_owned(), false, true),
            ("parse".to_owned(), false, true)
        ]
    );
    let add = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_item_add")
        .unwrap();
    assert_eq!(add.bindings[0].lookup.len(), 3);
}

/// One change to the fixtures, and the code and words of its refusal.
fn refused(path: &str, from: &str, to: &str, code: &str) {
    let folder = Folder::new("refused");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let problems = match compile(&changed(path, from, to), registry.compiler_set()) {
        Ok(_) => panic!("`{to}` compiled, and `{code}` should have refused it"),
        Err(problems) => problems,
    };
    let Some(problem) = problems.find(code) else {
        panic!("`{to}` was not refused as `{code}`: {problems:#?}");
    };
    assert_eq!(problem.at.file.as_str(), path, "{problem}");
    assert!(problem.at.line > 1, "{problem}");
    assert!(!problem.fix.is_empty(), "{problem}");
}

#[test]
fn a_file_that_breaks_a_rule_does_not_compile() {
    const FILE: &str = "commands/inventory.yaml";
    // A key the schema does not know, and a command with no declared effect.
    refused(FILE, "  workdir: fresh", "  workdir: fresh\n  shell: true", "kernel.unknown_key");
    refused(FILE, "  item_list:\n    effect: read\n", "  item_list:\n", "kernel.missing_field");
    // Free text with no pattern and no closed list.
    refused(
        FILE,
        "name: {type: item_name, pattern: '^[^\\x00-\\x1f]{1,60}$'}",
        "name: {type: item_name}",
        "command.text_param_open",
    );
    // A pattern that is not anchored, and one that is not a pattern.
    refused(
        FILE,
        "pattern: '^removed (?<sku>\\S+)$'",
        "pattern: 'removed (?<sku>\\S+)'",
        "command.pattern_not_anchored",
    );
    refused(
        FILE,
        "pattern: '^removed (?<sku>\\S+)$'",
        "pattern: '^removed (?<sku>\\S+$'",
        "command.pattern_invalid",
    );
    // An argument marked as code, and a flag that runs its value.
    refused(
        FILE,
        "params: {location: {type: location_code, required: false}}",
        "params: {location: {type: location_code, required: false, code: true}}",
        "command.code_argument",
    );
    refused(
        FILE,
        "{flag: --location, param: location}]\n    output: {table",
        "{flag: -c, param: location}]\n    output: {table",
        "command.code_flag",
    );
    // A param nothing declares, and one nothing carries.
    refused(
        FILE,
        "args: [item, remove, {param: sku}, --yes]",
        "args: [item, remove, {param: code}, --yes]",
        "command.param_unknown",
    );
    refused(
        FILE,
        "args: [item, remove, {param: sku}, --yes]",
        "args: [item, remove, --yes]",
        "command.param_unused",
    );
    // A command may add an exit status and never change one.
    refused(
        FILE,
        "    args: [item, remove, {param: sku}, --yes]\n    exits: {3: absent}",
        "    args: [item, remove, {param: sku}, --yes]\n    exits: {2: ok}",
        "command.exit_changed",
    );
    // A digest that is not one; reaching outside without saying so.
    refused(
        FILE,
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "sha256:1111",
        "command.digest_invalid",
    );
    refused(
        FILE,
        "    effect: irreversible\n    hazard: sends_outside\n",
        "    effect: irreversible\n",
        "command.reaches_without_hazard",
    );
    refused(
        FILE,
        "    effect: irreversible\n    hazard: sends_outside\n",
        "    effect: write\n    hazard: sends_outside\n",
        "command.hazard_needs_irreversible",
    );
    // An assigned key the output does not hold; a session folder not built.
    refused(
        FILE,
        "assigns: {sku: output.sku}",
        "assigns: {sku: output.code}",
        "command.assigns_unknown",
    );
    refused(
        FILE,
        "EXAMPLE_CLI_HOME: '/var/lib/example/home'",
        "EXAMPLE_CLI_HOME: '{session.state_dir}'",
        "command.state_dir_not_built",
    );

    // A shell is never a program surface; a path to search for is no path.
    refused(
        "target.yaml",
        "path: '/opt/example/bin/example-cli'",
        "path: '/bin/sh'",
        "command.shell_program",
    );
    refused(
        "target.yaml",
        "path: '/opt/example/bin/example-cli'",
        "path: 'example-cli'",
        "kernel.program_invalid",
    );

    // A binding: a command nobody defines, a verb that does not fit, a
    // read-back that writes, a param with nothing to fill it.
    refused(
        "operations/item_get.yaml",
        "command: item_show",
        "command: item_peek",
        "command.unknown_command",
    );
    refused(
        "operations/item_get.yaml",
        "command: item_show",
        "command: item_remove",
        "command.effect_mismatch",
    );
    refused(
        "operations/item_set_quantity.yaml",
        "read_back: {command: item_show}",
        "read_back: {command: item_remove}",
        "command.read_back_writes",
    );
    refused(
        "operations/item_add.yaml",
        "read_back: {command: item_show, key: [sku]}",
        "read_back: {command: item_show}",
        "command.param_unbound",
    );
    refused(
        "operations/item_add.yaml",
        "lookup: {command: item_find_by_ref}",
        "lookup: {command: item_list}",
        "command.stamp_missing",
    );
}
