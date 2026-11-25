//! The commands of a launcher, a version manager, and a container runner
//! that run no code but the program's own (decision 0299, step one),
//! against the command-line test stand. The stand's program is put in place
//! under the name of a program of the provider's closed list; what it was
//! started with is asked of the stand's own log, never of the pack.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    CompiledTarget, Digest, Environment, Epoch, FailureClass, NoSecrets, RunId, Values,
};
use a2m_files::Diagnostics;
use a2m_pack_command::codes;
use a2m_provider::{
    Authority, CheckedPaths, Deadline, Destinations, Posture, PrepareContext, ProgramRef, Registry,
    StepFailure, StepInput, SurfaceRef, current_platform,
};
use common::{Folder, registry};
use example_cli::harness::Stand;

const WITHIN: Duration = Duration::from_secs(60);

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The stand's program, put in the stand's folder as `name`.
fn placed(stand: &Stand, name: &str) -> PathBuf {
    let at = stand
        .root()
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(stand.program(), &at).unwrap();
    at
}

/// The target's files: one surface, the program at `program`, and one
/// command, `runner_call`, whose `program` block and command are the texts
/// given.
fn files(program: &Path, block: &str, command: &str) -> Vec<(String, String)> {
    let digest = Digest::of(&std::fs::read(program).unwrap());
    let target = format!(
        "schema_version: 1\ntarget: example\ntitle: Example runner\nsurfaces:\n  runner: {{kind: program, path: {}}}\nenvironments:\n  test: {{kind: test}}\ndefault_environment: test\nenvironment: {{locale: en-US, timezone: UTC}}\ndrift_risk: low\n",
        quoted(&program.display().to_string())
    );
    let types = "types:\n  item_name:\n    base: text\n    untrusted: true\n".to_owned();
    let commands = format!(
        "surface: runner\nprogram:\n  digests: {{{}: [\"sha256:{digest}\"]}}\n  limits: {{timeout: 60s}}\n  exits: {{0: ok}}\n{block}commands:\n  runner_call:\n{command}    output: none\n",
        current_platform(),
    );
    let inputs = if command.contains("{param: name}") {
        "inputs:\n  name: {type: item_name, description: \"A name\"}\n"
    } else {
        "inputs: {}\n"
    };
    let operation = format!(
        "operation: example_runner_call\ntitle: Ask the runner\neffect: read\n{inputs}outputs: {{}}\nbindings:\n  - mode: cli\n    verb: call\n    status: verified\n    command: runner_call\ndescription: Asks the program something of itself.\n"
    );
    vec![
        ("target.yaml".to_owned(), target),
        ("types.yaml".to_owned(), types),
        ("commands/runner.yaml".to_owned(), commands),
        ("operations/runner_call.yaml".to_owned(), operation),
    ]
}

/// The command `args` as a `read` command of the file.
fn reading(args: &str) -> String {
    format!("    effect: read\n    args: {args}\n")
}

fn compile(files: &[(String, String)], registry: &Registry) -> Result<CompiledTarget, Diagnostics> {
    common::compile(files, registry.compiler_set()).map(|compiled| compiled.compiled_target)
}

fn codes_of(problems: &Diagnostics) -> Vec<String> {
    problems
        .iter()
        .map(|problem| problem.code.as_str().to_owned())
        .collect()
}

/// Runs the one operation, as the engine would: prepare, each step of its
/// binding, then release.
async fn run(registry: &Registry, compiled_target: &CompiledTarget) -> Result<(), StepFailure> {
    let handle = registry.driver(&codes::mode()).unwrap();
    let surface = SurfaceRef {
        target: compiled_target.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface: compiled_target.surfaces[0].clone(),
    };
    let supervisor = handle.supervisor(
        surface
            .surface
            .programs
            .iter()
            .map(ProgramRef::from)
            .collect(),
    );
    let paths = CheckedPaths::none();
    let cx = PrepareContext {
        run: RunId::from_bytes([7; 16]),
        epoch: Epoch::new(1),
        surface: &surface,
        compiled_target,
        posture: Posture::Live,
        deadline: Deadline::after(WITHIN),
        redact: &NoSecrets,
        destinations: &Destinations::default(),
        paths: &paths,
        supervisor: &supervisor,
        confirmed: a2m_provider::ConfirmedReads::none(),
    };
    let mut prepared = handle.prepare(&cx, None).await?;
    let args = Values::new();
    let mut result = Ok(());
    for step in &compiled_target.operations[0].bindings[0].act {
        let input = StepInput::new(&args, Deadline::after(WITHIN));
        if let Err(failure) = handle
            .act(&mut prepared, step, &input, Authority::Read)
            .await
        {
            result = Err(failure);
            break;
        }
    }
    assert!(handle.release(prepared, result.is_err()).await.complete);
    result
}

/// What the stand was started with, each start's arguments in order.
fn started(stand: &Stand) -> Vec<Vec<String>> {
    stand
        .control()
        .invocations()
        .unwrap()
        .into_iter()
        .map(|invocation| invocation.args)
        .collect()
}

/// The proof of package L1: a listed command of a program of each kind
/// runs against the stand, as the program, and the stand saw exactly the
/// listed elements.
#[tokio::test(flavor = "multi_thread")]
async fn a_listed_command_of_a_launcher_a_version_manager_and_a_container_runner_runs() {
    for name in ["node", "uv", "docker"] {
        let folder = Folder::new("own-code");
        let registry = registry(&folder, Arc::new(NoSecrets));
        let stand = Stand::start().unwrap();
        let program = placed(&stand, name);
        let compiled_target = compile(&files(&program, "", &reading("['--version']")), &registry)
            .unwrap_or_else(|problems| panic!("{name}: {problems:#?}"));
        run(&registry, &compiled_target)
            .await
            .unwrap_or_else(|failure| panic!("{name}: {failure:?}"));
        assert_eq!(
            started(&stand),
            vec![vec!["--version".to_owned()]],
            "{name}"
        );
    }
}

/// The other half of the proof: a command not on the list, and every other
/// way the files could hand such a program more than a listed command, does
/// not compile, by the rule that refuses a program by its name.
#[test]
fn a_command_not_listed_is_refused_by_name_where_the_files_are_compiled() {
    let folder = Folder::new("own-code-files");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let stand = Stand::start().unwrap();
    let docker = placed(&stand, "docker");
    let listed = "['--context', 'default', 'container', 'inspect', '--', {param: name}]";
    let with_name = |args: &str| {
        format!(
            "    effect: read\n    params: {{name: {{type: item_name, pattern: '^[a-z]{{1,20}}$'}}}}\n    args: {args}\n"
        )
    };
    assert!(compile(&files(&docker, "", &reading("['--version']")), &registry).is_ok());
    if let Err(problems) = compile(&files(&docker, "", &with_name(listed)), &registry) {
        panic!("{problems:#?}");
    }
    for (block, command) in [
        // A command the list does not hold, or holds in another form.
        (String::new(), reading("[run, example]")),
        (String::new(), reading("['--help']")),
        (
            String::new(),
            reading("[ps, '--all', '--no-trunc', '--format', json]"),
        ),
        (String::new(), reading("['--version', '-H', example]")),
        (
            String::new(),
            with_name("['--context', 'default', 'container', 'inspect', {param: name}]"),
        ),
        // A listed command declared as a write.
        (
            String::new(),
            "    effect: write\n    args: ['--version']\n".to_owned(),
        ),
        // Variables, a help form, and a version check outside the list.
        (
            "  env: {set: {EXAMPLE_HOST: example}}\n".to_owned(),
            reading("['--version']"),
        ),
        ("  help: [['--help']]\n".to_owned(), reading("['--version']")),
        (
            "  version:\n    args: [version]\n    expect: {exit: [0], stdout: '^(?<version>.+)$'}\n    supported: ['2.4.1']\n"
                .to_owned(),
            reading("['--version']"),
        ),
    ] {
        let problems = compile(&files(&docker, &block, &command), &registry)
            .err()
            .unwrap_or_else(|| panic!("compiled: {block}{command}"));
        assert!(
            codes_of(&problems)
                .iter()
                .any(|code| code == "command.shell_program"),
            "{block}{command}: {problems:#?}"
        );
    }
    // A program not on the list is refused by its name whatever it is
    // given, as before.
    let make = placed(&stand, "make");
    let problems = compile(&files(&make, "", &reading("['--version']")), &registry)
        .err()
        .unwrap();
    assert!(
        codes_of(&problems)
            .iter()
            .any(|code| code == "command.shell_program"),
        "{problems:#?}"
    );
}

/// The driver does not take the compiled target's word: a compiled target
/// changed after it was compiled, to give the program a command not on the
/// list, starts nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_compiled_target_that_gives_such_a_program_a_command_not_listed_starts_nothing() {
    let folder = Folder::new("own-code-plan");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let stand = Stand::start().unwrap();
    let docker = placed(&stand, "docker");
    let mut compiled_target =
        compile(&files(&docker, "", &reading("['--version']")), &registry).unwrap();
    let pack = compiled_target
        .packs
        .iter_mut()
        .find(|section| section.pack == "command")
        .unwrap();
    let mut section: serde_json::Value = pack.payload.decode().unwrap();
    section["commands"]["runner_call"]["args"] = serde_json::json!([
        {"is": "literal", "text": "run"},
        {"is": "literal", "text": "example"},
    ]);
    pack.payload = a2m_core::PackPayload::encode(&section).unwrap();
    let failure = run(&registry, &compiled_target).await.unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code()),
        (FailureClass::Policy, &codes::PROGRAM_PATH_UNSAFE)
    );
    assert!(started(&stand).is_empty(), "nothing started");
}
