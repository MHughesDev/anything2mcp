//! A script the target keeps, run by a declared interpreter as a fixed
//! pair. What the script received is asked of the system: the script
//! writes every argument it was given to a file, and the test reads that
//! file, never what the pack says it sent.

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
    CompiledTarget, Digest, Environment, Epoch, FailureClass, NoSecrets, RunId, Value, Values,
};
use a2m_files::Diagnostics;
use a2m_pack_command::codes;
use a2m_provider::{
    Access, Authority, CheckedPaths, Deadline, Destinations, Posture, PrepareContext, ProgramRef,
    Registry, RootName, Roots, StepFailure, StepInput, SurfaceRef, current_platform,
};
use common::{Folder, registry};

const WITHIN: Duration = Duration::from_secs(60);

/// Values that hold the syntax of a shell and of the interpreters: each
/// must reach the script as the same bytes.
const VALUES: &[&str] = &[
    "$(echo example)",
    "`echo example`",
    "a; b && c | d",
    "'single' \"double\" \\back",
    "line one\nline two",
    "$HOME %PATH% ${PATH}",
    "caf\u{e9} \u{2713}",
    "*.txt ?",
];

/// One fixed pair under test.
struct Pair {
    interpreter: PathBuf,
    fixed: &'static [&'static str],
    /// The script's file name, inside `scripts/` in the target's folder.
    name: &'static str,
    text: String,
    /// Whether the argument list starts with `--`, so that a value with a
    /// leading dash may follow. Not every interpreter hands a script `--`.
    dashes: bool,
    /// The variables passed from the engine's environment.
    pass: &'static [&'static str],
    /// How long one run may take: the target's limit and the step's
    /// deadline.
    within: Duration,
}

/// A project with the target `example` in it, whose one surface is the
/// pair's interpreter, compiled.
struct Scene {
    folder: Folder,
    registry: Registry,
    compiled_target: CompiledTarget,
    out: PathBuf,
    script: PathBuf,
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn values(pair: &Pair) -> Vec<String> {
    let mut values: Vec<String> = VALUES.iter().map(|value| (*value).to_owned()).collect();
    if pair.dashes {
        values.push("-x --flag".to_owned());
    }
    values
}

/// The target's files, with `script_path` as the command's script.
fn files(
    pair: &Pair,
    script_path: &str,
    script_digest: &Digest,
    out: &Path,
) -> Vec<(String, String)> {
    let digest = Digest::of(&std::fs::read(&pair.interpreter).unwrap());
    let count = values(pair).len();
    let target = format!(
        "schema_version: 1\ntarget: example\ntitle: Example scripts\nsurfaces:\n  shell: {{kind: program, path: {}}}\nenvironments:\n  test: {{kind: test}}\ndefault_environment: test\nenvironment: {{locale: en-US, timezone: UTC}}\ndrift_risk: low\n",
        quoted(&pair.interpreter.display().to_string())
    );
    let types = "types:\n  item_name:\n    base: text\n    untrusted: true\n".to_owned();
    let fixed: Vec<String> = pair.fixed.iter().map(|arg| quoted(arg)).collect();
    let params: Vec<String> = (1..=count)
        .map(|n| format!("p{n}: {{type: item_name, pattern: '^(?s:.*)$'}}"))
        .collect();
    let mut args: Vec<String> = if pair.dashes {
        vec!["'--'".to_owned()]
    } else {
        Vec::new()
    };
    args.extend((1..=count).map(|n| format!("{{param: p{n}}}")));
    let commands = format!(
        "surface: shell\nprogram:\n  digests: {{{}: [\"sha256:{digest}\"]}}\n  interpreter: {{args: [{}]}}\n  env: {{{}set: {{EXAMPLE_OUT: {}}}}}\n  limits: {{timeout: {}s}}\n  exits: {{0: ok}}\ncommands:\n  echo_script:\n    effect: read\n    script: {{path: {}, digest: \"sha256:{script_digest}\"}}\n    params: {{{}}}\n    args: [{}]\n    output: none\n",
        current_platform(),
        fixed.join(", "),
        if pair.pass.is_empty() {
            String::new()
        } else {
            format!("pass: [{}], ", pair.pass.join(", "))
        },
        quoted(&out.display().to_string()),
        pair.within.as_secs(),
        quoted(script_path),
        params.join(", "),
        args.join(", "),
    );
    let inputs: Vec<String> = (1..=count)
        .map(|n| format!("  p{n}: {{type: item_name, description: \"A value\"}}"))
        .collect();
    let operation = format!(
        "operation: example_echo_script\ntitle: Echo through a script\neffect: read\ninputs:\n{}\noutputs: {{}}\nbindings:\n  - mode: cli\n    verb: call\n    status: verified\n    command: echo_script\ndescription: Hands values to a script the target keeps.\n",
        inputs.join("\n")
    );
    vec![
        ("target.yaml".to_owned(), target),
        ("types.yaml".to_owned(), types),
        ("commands/echo.yaml".to_owned(), commands),
        ("operations/echo_script.yaml".to_owned(), operation),
    ]
}

impl Scene {
    fn new(pair: &Pair) -> Self {
        Self::try_new(pair, None).unwrap_or_else(|problems| panic!("{problems:#?}"))
    }

    fn try_new(pair: &Pair, script_path: Option<&str>) -> Result<Self, Diagnostics> {
        let folder = Folder::new("script");
        let registry = registry(&folder, Arc::new(NoSecrets));
        let scripts = folder
            .path()
            .join("project")
            .join("targets")
            .join("example")
            .join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let script = scripts.join(pair.name);
        std::fs::write(&script, &pair.text).unwrap();
        let out = folder.path().join("received.bin");
        let digest = Digest::of(pair.text.as_bytes());
        let path = script_path.map_or_else(|| format!("scripts/{}", pair.name), str::to_owned);
        let compiled_target =
            common::compile(&files(pair, &path, &digest, &out), registry.compiler_set())?
                .compiled_target;
        Ok(Self {
            folder,
            registry,
            compiled_target,
            out,
            script,
        })
    }

    /// Runs the one operation with the pair's values, as the engine would:
    /// prepare, then each step of its binding, then release.
    async fn run(&self, pair: &Pair) -> Result<(), StepFailure> {
        let handle = self.registry.driver(&codes::mode()).unwrap();
        let surface = SurfaceRef {
            target: self.compiled_target.header.target.clone(),
            environment: Environment::new("test").unwrap(),
            surface: self.compiled_target.surfaces[0].clone(),
        };
        let supervisor = handle.supervisor(
            surface
                .surface
                .programs
                .iter()
                .map(ProgramRef::from)
                .collect(),
        );
        let mut roots = Roots::none();
        let name = RootName::new("project").unwrap();
        roots
            .add(
                name.clone(),
                &self.folder.path().join("project"),
                Access::Read,
            )
            .unwrap();
        let paths = CheckedPaths::none().with_project(roots.folder(&name, Access::Read).unwrap());
        let cx = PrepareContext {
            run: RunId::from_bytes([9; 16]),
            epoch: Epoch::new(1),
            surface: &surface,
            compiled_target: &self.compiled_target,
            posture: Posture::Live,
            deadline: Deadline::after(pair.within),
            redact: &NoSecrets,
            destinations: &Destinations::default(),
            paths: &paths,
            supervisor: &supervisor,
            confirmed: a2m_provider::ConfirmedReads::none(),
        };
        let mut prepared = handle.prepare(&cx, None).await?;
        let mut args = Values::new();
        for (n, value) in values(pair).iter().enumerate() {
            args.insert(format!("p{}", n + 1), Value::from(value.as_str()));
        }
        let operation = &self.compiled_target.operations[0];
        let mut result = Ok(());
        for step in &operation.bindings[0].act {
            let input = StepInput::new(&args, Deadline::after(pair.within));
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

    /// What the script wrote: every argument it was given, each ended by a
    /// NUL byte, as bytes.
    fn received(&self) -> Vec<Vec<u8>> {
        let bytes = std::fs::read(&self.out).unwrap();
        let mut parts: Vec<Vec<u8>> = bytes.split(|b| *b == 0).map(<[u8]>::to_vec).collect();
        assert_eq!(parts.pop(), Some(Vec::new()), "the last argument is ended");
        parts
    }
}

/// Runs the pair and asks the system what the script received.
async fn each_value_arrives_as_it_was(pair: &Pair) {
    let scene = Scene::new(pair);
    scene.run(pair).await.unwrap();
    let mut expected: Vec<Vec<u8>> = Vec::new();
    if pair.dashes {
        expected.push(b"--".to_vec());
    }
    expected.extend(values(pair).iter().map(|value| value.as_bytes().to_vec()));
    assert_eq!(scene.received(), expected);
}

#[cfg(unix)]
fn shell_pair() -> Pair {
    Pair {
        interpreter: PathBuf::from("/bin/sh"),
        fixed: &[],
        name: "echo.sh",
        text: "for a in \"$@\"; do printf '%s\\000' \"$a\"; done > \"$EXAMPLE_OUT\"\n".to_owned(),
        dashes: true,
        pass: &[],
        within: WITHIN,
    }
}

#[cfg(unix)]
fn python_pair() -> Option<Pair> {
    let interpreter = ["/usr/bin/python3", "/usr/local/bin/python3"]
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())?;
    Some(Pair {
        interpreter,
        // Isolated: no variable, user folder, or the script's own folder
        // changes what the interpreter loads.
        fixed: &["-I"],
        name: "echo.py",
        text: "import os, sys\nwith open(os.environ['EXAMPLE_OUT'], 'wb') as out:\n    for a in sys.argv[1:]:\n        out.write(os.fsencode(a) + b'\\0')\n".to_owned(),
        dashes: true,
        pass: &[],
        within: WITHIN,
    })
}

#[cfg(unix)]
#[tokio::test]
async fn the_shell_and_python_receive_each_value_as_literal_text() {
    each_value_arrives_as_it_was(&shell_pair()).await;
    match python_pair() {
        Some(pair) => each_value_arrives_as_it_was(&pair).await,
        None => println!("skipped: no python3 at /usr/bin or /usr/local/bin on this machine"),
    }
}

#[tokio::test]
async fn a_script_changed_after_its_declaration_is_refused_before_anything_starts() {
    let Some(pair) = native_pair() else {
        return;
    };
    let scene = Scene::new(&pair);
    std::fs::write(&scene.script, format!("{}echo changed\n", pair.text)).unwrap();
    let failure = scene.run(&pair).await.unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code()),
        (FailureClass::Drift, &codes::PROGRAM_DIGEST_CHANGED)
    );
    assert!(!scene.out.exists(), "nothing started");
    // Gone is refused the same way, before anything starts.
    std::fs::remove_file(&scene.script).unwrap();
    let failure = scene.run(&pair).await.unwrap_err();
    assert_eq!(failure.failure.code(), &codes::PROGRAM_MISSING);
    assert!(!scene.out.exists(), "nothing started");
}

#[cfg(unix)]
#[test]
fn a_script_path_that_leaves_the_target_does_not_compile() {
    let pair = shell_pair();
    for path in ["../other/echo.sh", "/etc/echo.sh", "scripts/../../echo.sh"] {
        let problems = Scene::try_new(&pair, Some(path)).err().unwrap();
        assert!(
            problems
                .iter()
                .any(|problem| problem.code.as_str() == "command.script_malformed"),
            "{path}: {problems:#?}"
        );
    }
}

/// The review names a fixed pair whole: the interpreter and its digest,
/// its fixed arguments, the script and its digest, with the fixed notice
/// that a digest does not make a script safe (decisions 0216 and 0252).
#[cfg(unix)]
#[test]
fn the_review_names_the_interpreter_and_the_script_it_runs() {
    let pair = python_pair().unwrap_or_else(shell_pair);
    let folder = Folder::new("script-review");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let script_digest = Digest::of(pair.text.as_bytes());
    let out = folder.path().join("received.bin");
    let path = format!("scripts/{}", pair.name);
    let compiled = common::compile(
        &files(&pair, &path, &script_digest, &out),
        registry.compiler_set(),
    )
    .unwrap();
    let binding = &compiled.review.items[0].bindings[0];
    assert_eq!(
        binding.states,
        a2m_files::BindingStates::Stated {
            runs: vec![a2m_files::RunReview {
                run: a2m_files::ProgramRun::Script {
                    interpreter: pair.interpreter.display().to_string(),
                    interpreter_digests: vec![Digest::of(
                        &std::fs::read(&pair.interpreter).unwrap()
                    )],
                    arguments: pair.fixed.iter().map(|arg| (*arg).to_owned()).collect(),
                    script: path,
                    script_digest,
                },
                notice: Some(a2m_files::SCRIPT_NOTICE),
            }],
            environment: vec![a2m_files::EnvironmentVariable {
                name: "EXAMPLE_OUT".to_owned(),
                value: a2m_files::VariableValue::Set(out.display().to_string()),
            }],
        }
    );
}

#[cfg(unix)]
#[tokio::test]
async fn an_interpreter_another_user_can_replace_is_refused() {
    use std::os::unix::fs::PermissionsExt as _;
    let place = Folder::new("open-interpreter");
    let open = place.path().join("open");
    std::fs::create_dir_all(&open).unwrap();
    let copy = open.join("sh");
    std::fs::copy(std::fs::canonicalize("/bin/sh").unwrap(), &copy).unwrap();
    std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    let pair = Pair {
        interpreter: copy,
        ..shell_pair()
    };
    let scene = Scene::new(&pair);
    let failure = scene.run(&pair).await.unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code()),
        (FailureClass::Policy, &codes::PROGRAM_PATH_UNSAFE)
    );
    assert!(!scene.out.exists(), "nothing started");
}

#[cfg(windows)]
fn powershell_pair() -> Option<Pair> {
    let root =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let interpreter = root
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if !interpreter.is_file() {
        println!(
            "skipped: no Windows PowerShell at {}",
            interpreter.display()
        );
        return None;
    }
    Some(Pair {
        interpreter,
        fixed: &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"],
        name: "echo.ps1",
        text: "$bytes = New-Object System.Collections.Generic.List[byte]\r\nforeach ($a in $args) { $bytes.AddRange([System.Text.Encoding]::UTF8.GetBytes([string]$a)); $bytes.Add(0) }\r\n[System.IO.File]::WriteAllBytes($env:EXAMPLE_OUT, $bytes.ToArray())\r\n".to_owned(),
        // PowerShell takes `--` as its own end of parameters.
        dashes: false,
        pass: &[],
        // Windows PowerShell's start ran past a minute twice on CI's
        // Windows runners, with each other interpreter here done in
        // seconds; what this test proves does not depend on the time.
        within: Duration::from_secs(180),
    })
}

#[cfg(windows)]
#[tokio::test]
async fn powershell_receives_each_value_as_literal_text() {
    if let Some(pair) = powershell_pair() {
        each_value_arrives_as_it_was(&pair).await;
    }
}

/// A program found on the search path, as the file it leads to, or why the
/// test cannot go on: on a CI machine a `required` launcher is always
/// there, so there it is a failure; elsewhere the test is skipped.
fn on_path(name: &str, required: bool) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let found = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|folder| folder.join(&file))
            .find(|path| path.is_file())
            .and_then(|path| std::fs::canonicalize(path).ok())
    });
    if found.is_none() {
        assert!(
            !required || std::env::var_os("CI").is_none(),
            "no `{file}` on the search path of this CI machine"
        );
        println!("skipped: no `{file}` on the search path of this machine");
    }
    found
}

/// The JavaScript runtime as the launcher of a fixed pair (decision 0299,
/// step two). Where its own folder is not closed to other users, as the
/// check requires, the test runs a copy of it in a folder of the test's own:
/// the runtime is one file.
fn node_pair() -> Option<(Pair, Option<Folder>)> {
    let found = on_path("node", true)?;
    let trust = a2m_provider::ProgramTrust::default();
    let role = a2m_provider::ProgramRole::Interpreter;
    let (interpreter, copy) = if a2m_provider::check_program(&found, &trust, role).is_ok() {
        (found, None)
    } else {
        let folder = Folder::new("node");
        let copy = folder
            .path()
            .join(format!("node{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(&found, &copy).unwrap();
        // The copy keeps the mode of the runner's file, which may be open
        // to a group: make it the user's own and closed to others.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (copy, Some(folder))
    };
    let pair = Pair {
        interpreter,
        fixed: &[],
        name: "echo.js",
        text: "const fs = require('fs');\nconst parts = [];\nfor (const a of process.argv.slice(2)) {\n  parts.push(Buffer.from(a, 'utf8'), Buffer.from([0]));\n}\nfs.writeFileSync(process.env.EXAMPLE_OUT, Buffer.concat(parts));\n".to_owned(),
        dashes: true,
        // Windows programs commonly expect the system's folder named.
        pass: if cfg!(windows) { &["SYSTEMROOT"] } else { &[] },
        within: WITHIN,
    };
    Some((pair, copy))
}

/// The proof of package L2 for the JavaScript runtime, on every system:
/// given the pinned file, it hands the file each value as the same bytes.
#[tokio::test]
async fn the_javascript_runtime_receives_each_value_as_literal_text() {
    if let Some((pair, _copy)) = node_pair() {
        each_value_arrives_as_it_was(&pair).await;
    }
}

/// The other half of the proof of package L2: the launcher's file, changed
/// after its declaration, is refused at start, and nothing starts.
#[tokio::test]
async fn a_launchers_file_changed_after_its_declaration_is_refused_at_start() {
    let Some((pair, _copy)) = node_pair() else {
        return;
    };
    let scene = Scene::new(&pair);
    scene.run(&pair).await.unwrap();
    std::fs::remove_file(&scene.out).unwrap();
    std::fs::write(&scene.script, format!("{}process.exit(0);\n", pair.text)).unwrap();
    let failure = scene.run(&pair).await.unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code()),
        (FailureClass::Drift, &codes::PROGRAM_DIGEST_CHANGED)
    );
    assert!(!scene.out.exists(), "nothing started");
}

/// Python on Windows, the row's third system. Python is no single file, so
/// it is run where it is installed, and only where that folder is closed
/// to other users; elsewhere the test says so and stops. It needs the
/// system's folder to start.
#[cfg(windows)]
#[tokio::test]
async fn python_on_windows_receives_each_value_as_literal_text() {
    let Some(interpreter) = on_path("python", false) else {
        return;
    };
    let trust = a2m_provider::ProgramTrust::default();
    let role = a2m_provider::ProgramRole::Interpreter;
    if let Err(why) = a2m_provider::check_program(&interpreter, &trust, role) {
        println!(
            "skipped: Python at {} is in a folder the check refuses: {why:?}",
            interpreter.display()
        );
        return;
    }
    let pair = Pair {
        interpreter,
        fixed: &["-I"],
        name: "echo.py",
        text: "import os, sys\nwith open(os.environ['EXAMPLE_OUT'], 'wb') as out:\n    for a in sys.argv[1:]:\n        out.write(a.encode('utf-8', 'surrogatepass') + b'\\0')\n".to_owned(),
        dashes: true,
        pass: &["SYSTEMROOT"],
        within: WITHIN,
    };
    each_value_arrives_as_it_was(&pair).await;
}

/// The pair every system has: the shell on Unix, PowerShell on Windows.
fn native_pair() -> Option<Pair> {
    #[cfg(unix)]
    return Some(shell_pair());
    #[cfg(windows)]
    return powershell_pair();
    #[cfg(not(any(unix, windows)))]
    return None;
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn osascript_receives_each_value_as_literal_text() {
    let interpreter = PathBuf::from("/usr/bin/osascript");
    if !interpreter.is_file() {
        println!("skipped: no /usr/bin/osascript on this machine");
        return;
    }
    // The script is UTF-8 text; it writes each argument as UTF-8, then a
    // NUL byte.
    let pair = Pair {
        interpreter,
        fixed: &[],
        name: "echo.applescript",
        text: "on run argv\n\tset f to open for access (POSIX file (system attribute \"EXAMPLE_OUT\")) with write permission\n\tset eof f to 0\n\trepeat with a in argv\n\t\twrite (contents of a) to f as \u{ab}class utf8\u{bb}\n\t\twrite (ASCII character 0) to f\n\tend repeat\n\tclose access f\nend run\n".to_owned(),
        dashes: false,
        pass: &[],
        within: WITHIN,
    };
    each_value_arrives_as_it_was(&pair).await;
}

#[cfg(unix)]
#[test]
fn the_pair_is_held_to_its_rules_where_the_files_are_compiled() {
    let pair = shell_pair();
    let folder = Folder::new("pair-rules");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let out = folder.path().join("received.bin");
    let digest = Digest::of(pair.text.as_bytes());
    let base = files(&pair, "scripts/echo.sh", &digest, &out);
    let refused = |from: &str, to: &str, code: &str| {
        let changed: Vec<(String, String)> = base
            .iter()
            .map(|(path, text)| {
                if path == "commands/echo.yaml" {
                    assert!(text.contains(from), "`{from}` is not in the file");
                    (path.clone(), text.replacen(from, to, 1))
                } else {
                    (path.clone(), text.clone())
                }
            })
            .collect();
        let problems = common::compile(&changed, registry.compiler_set())
            .err()
            .unwrap_or_else(|| panic!("`{to}` compiled"));
        assert!(
            problems.iter().any(|problem| problem.code.as_str() == code),
            "{to}: {problems:#?}"
        );
    };
    assert!(common::compile(&base, registry.compiler_set()).is_ok());
    // The interpreter is never handed a flag that runs what follows it.
    refused(
        "interpreter: {args: []}",
        "interpreter: {args: ['-c']}",
        "command.code_flag",
    );
    // A command on an interpreter names its script; one elsewhere names none.
    let line = format!("    script: {{path: 'scripts/echo.sh', digest: \"sha256:{digest}\"}}\n");
    refused(&line, "", "command.script_malformed");
    refused("  interpreter: {args: []}\n", "", "command.shell_program");
    refused(
        "digest: \"sha256:",
        "digest: \"sha1:",
        "command.digest_invalid",
    );
}

/// The target's files of a pair, with one piece of the `commands/` file
/// changed, compiled: the codes of the problems found.
#[cfg(unix)]
fn problems_with(pair: &Pair, from: &str, to: &str) -> Vec<String> {
    let folder = Folder::new("pair-changed");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let out = folder.path().join("received.bin");
    let digest = Digest::of(pair.text.as_bytes());
    let changed: Vec<(String, String)> = files(pair, "scripts/echo.sh", &digest, &out)
        .into_iter()
        .map(|(path, text)| {
            if path == "commands/echo.yaml" {
                assert!(text.contains(from), "`{from}` is not in the file");
                let text = text.replacen(from, to, 1);
                (path, text)
            } else {
                (path, text)
            }
        })
        .collect();
    match common::compile(&changed, registry.compiler_set()) {
        Ok(_) => Vec::new(),
        Err(problems) => problems
            .iter()
            .map(|problem| problem.code.as_str().to_owned())
            .collect(),
    }
}

/// The fifth safety audit's PR-1 (decision 0250): an interpreter is a pair's
/// half only in the one form the closed table gives it. A flag before the
/// shell's script, and Python without isolation, compiled before.
#[cfg(unix)]
#[test]
fn an_interpreter_in_a_form_other_than_its_own_does_not_compile() {
    let shell = shell_pair();
    for flag in ["'-x'", "'-s'", "'-i'"] {
        let problems = problems_with(
            &shell,
            "interpreter: {args: []}",
            &format!("interpreter: {{args: [{flag}]}}"),
        );
        assert!(
            problems.iter().any(|code| code == "command.shell_program"),
            "{flag}: {problems:?}"
        );
    }
    match python_pair() {
        Some(python) => {
            let problems = problems_with(
                &python,
                "interpreter: {args: ['-I']}",
                "interpreter: {args: []}",
            );
            assert!(
                problems.iter().any(|code| code == "command.shell_program"),
                "{problems:?}"
            );
            assert!(problems_with(&python, "{args: ['-I']}", "{args: ['-I']}").is_empty());
        }
        None => println!("skipped: no python3 at /usr/bin or /usr/local/bin on this machine"),
    }
}

/// The fifth safety audit's PR-2 (decision 0250): a variable that makes a
/// program load other code is refused where the files are compiled, set or
/// passed. They compiled before.
#[cfg(unix)]
#[test]
fn a_variable_that_makes_the_program_load_other_code_does_not_compile() {
    let shell = shell_pair();
    for (from, to) in [
        ("env: {set: {", "env: {set: {LD_PRELOAD: '/x.so', "),
        ("env: {set: {", "env: {set: {BASH_ENV: '/x.sh', "),
        ("env: {set: {", "env: {pass: [PYTHONPATH], set: {"),
        (
            "env: {set: {",
            "env: {pass: [DYLD_INSERT_LIBRARIES], set: {",
        ),
    ] {
        let problems = problems_with(&shell, from, to);
        assert!(
            problems
                .iter()
                .any(|code| code == "command.loader_variable"),
            "{to}: {problems:?}"
        );
    }
}

/// The fifth safety audit's PR-4 (decision 0250): the folder the run's copy
/// of the script is put in, or one above it, that another user may write is
/// refused before the interpreter starts. Before, the copy was checked by
/// its digest only, and the interpreter opened it afterwards.
#[cfg(unix)]
#[tokio::test]
async fn a_runs_folder_others_can_write_is_refused_before_the_script_starts() {
    use std::os::unix::fs::PermissionsExt as _;
    let pair = shell_pair();
    let scene = Scene::new(&pair);
    let runs = scene.folder.path().join("runs");
    std::fs::set_permissions(&runs, std::fs::Permissions::from_mode(0o777)).unwrap();
    let failure = scene.run(&pair).await.unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code()),
        (FailureClass::Policy, &codes::PROGRAM_PATH_UNSAFE)
    );
    assert!(!scene.out.exists(), "nothing started");
    // Closed again, the same run starts.
    std::fs::set_permissions(&runs, std::fs::Permissions::from_mode(0o755)).unwrap();
    scene.run(&pair).await.unwrap();
    assert!(scene.out.exists(), "the script ran");
}
