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

use a2m_core::{Digest, Environment, Epoch, FailureClass, NoSecrets, Plan, RunId, Value, Values};
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
}

/// A project with the target `example` in it, whose one surface is the
/// pair's interpreter, compiled.
struct Scene {
    folder: Folder,
    registry: Registry,
    plan: Plan,
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
        "surface: shell\nprogram:\n  digests: {{{}: [\"sha256:{digest}\"]}}\n  interpreter: {{args: [{}]}}\n  env: {{set: {{EXAMPLE_OUT: {}}}}}\n  limits: {{timeout: 60s}}\n  exits: {{0: ok}}\ncommands:\n  echo_script:\n    effect: read\n    script: {{path: {}, digest: \"sha256:{script_digest}\"}}\n    params: {{{}}}\n    args: [{}]\n    output: none\n",
        current_platform(),
        fixed.join(", "),
        quoted(&out.display().to_string()),
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
        let plan =
            common::compile(&files(pair, &path, &digest, &out), registry.compiler_set())?.plan;
        Ok(Self {
            folder,
            registry,
            plan,
            out,
            script,
        })
    }

    /// Runs the one operation with the pair's values, as the engine would:
    /// prepare, then each step of its binding, then release.
    async fn run(&self, pair: &Pair) -> Result<(), StepFailure> {
        let handle = self.registry.driver(&codes::mode()).unwrap();
        let surface = SurfaceRef {
            target: self.plan.header.target.clone(),
            environment: Environment::new("test").unwrap(),
            surface: self.plan.surfaces[0].clone(),
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
            plan: &self.plan,
            posture: Posture::Live,
            deadline: Deadline::after(WITHIN),
            redact: &NoSecrets,
            destinations: &Destinations::default(),
            paths: &paths,
            supervisor: &supervisor,
        };
        let mut prepared = handle.prepare(&cx, None).await?;
        let mut args = Values::new();
        for (n, value) in values(pair).iter().enumerate() {
            args.insert(format!("p{}", n + 1), Value::from(value.as_str()));
        }
        let operation = &self.plan.operations[0];
        let mut result = Ok(());
        for step in &operation.bindings[0].act {
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
    })
}

#[cfg(windows)]
#[tokio::test]
async fn powershell_receives_each_value_as_literal_text() {
    if let Some(pair) = powershell_pair() {
        each_value_arrives_as_it_was(&pair).await;
    }
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
