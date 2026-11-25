//! The completion command, through the real program: it prints a script
//! for each shell it names, made from the command tree, with no engine and
//! no home; the bash script completes a command and a flag in a real bash;
//! and no script runs the program when it completes a word.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use a2m_actions::command_faces;
use anything2mcp::cli::COMPLETION_COMMAND;
use common::{Folder, IN_PROCESS_VARIABLE};

const SHELLS: [&str; 4] = ["bash", "zsh", "fish", "powershell"];

/// The program, run against a home that does not exist, with nothing that
/// keeps it from starting an engine.
fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anything2mcp"))
        .args(args)
        .current_dir(home.parent().unwrap())
        .env("A2M_HOME", home)
        .env_remove(IN_PROCESS_VARIABLE)
        .env_remove(anything2mcp::PASSPHRASE_VARIABLE)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn script(shell: &str) -> String {
    let folder = Folder::new("complete");
    let home = folder.path().join("absent");
    let output = run(&home, &[COMPLETION_COMMAND, shell]);
    assert_eq!(output.status.code(), Some(0), "{shell}: {output:?}");
    assert!(output.stderr.is_empty(), "{shell}: {output:?}");
    // Answered by the program itself: no engine, and no home.
    assert!(!home.exists(), "{shell}: a home was made");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn every_shell_gets_a_script_made_with_no_engine_and_no_home() {
    let registry = anything2mcp::action_registry().unwrap();
    let faces = command_faces(&registry);
    for shell in SHELLS {
        let text = script(shell);
        // The header says what is not completed.
        assert!(
            text.lines()
                .take(5)
                .any(|line| line.contains("not completed")),
            "{shell}"
        );
        // Every noun, verb, and flag of the tree is in it.
        for face in &faces {
            for word in [&face.noun, &face.verb] {
                assert!(text.contains(word.as_str()), "{shell}: {word}");
            }
            for arg in face.args.iter().filter(|arg| !arg.positional) {
                let name = if shell == "fish" {
                    format!("-l {}", arg.flag.trim_start_matches("--"))
                } else {
                    arg.flag.clone()
                };
                assert!(text.contains(&name), "{shell}: {}", arg.flag);
            }
        }
    }
    // A shell it does not know is a malformed command, and makes nothing.
    let folder = Folder::new("complete");
    let home = folder.path().join("absent");
    for wrong in [&[COMPLETION_COMMAND, "tcsh"][..], &[COMPLETION_COMMAND][..]] {
        let refused = run(&home, wrong);
        assert_eq!(refused.status.code(), Some(2), "{wrong:?}: {refused:?}");
        assert!(!home.exists());
    }
}

/// Removes the quoted strings of a line, by the shell's own rule for
/// single quotes, and gives what is left.
fn unquoted(shell: &str, line: &str) -> String {
    let mut left = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        // Outside quotes, bash and zsh take the next character as itself.
        if c == '\\' && matches!(shell, "bash" | "zsh") {
            left.push(c);
            if let Some(next) = chars.next() {
                left.push(next);
            }
            continue;
        }
        if c != '\'' {
            left.push(c);
            continue;
        }
        // Inside single quotes until the closing one.
        while let Some(inside) = chars.next() {
            match (shell, inside) {
                ("fish", '\\') => {
                    chars.next();
                }
                ("powershell", '\'') if chars.peek() == Some(&'\'') => {
                    chars.next();
                }
                (_, '\'') => break,
                _ => {}
            }
        }
        left.push_str("''");
    }
    left
}

/// The values a fish line gives to one option, such as `-n` or `-a`.
fn fish_values(line: &str, option: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = line;
    let start = format!(" {option} '");
    while let Some(at) = rest.find(&start) {
        let value: String = rest[at + start.len()..]
            .chars()
            .take_while(|c| *c != '\'')
            .collect();
        rest = &rest[at + start.len() + value.len()..];
        found.push(value);
    }
    found
}

#[test]
fn no_script_runs_the_program_when_it_completes() {
    for shell in SHELLS {
        let text = script(shell);
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            let left = unquoted(shell, line);
            // The program's names appear only where the script registers
            // itself for them, never as a command it runs.
            let registers = match shell {
                "bash" => trimmed.starts_with("complete -F "),
                "zsh" => trimmed.starts_with("compdef "),
                "fish" => trimmed.starts_with("complete -c "),
                _ => trimmed.starts_with("Register-ArgumentCompleter "),
            };
            if !registers {
                for name in ["anything2mcp", "a2m"] {
                    let named = left
                        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                        .any(|word| word == name);
                    assert!(!named, "{shell}: {line}");
                }
            }
            match shell {
                "fish" => {
                    // A substitution only of the shell's own builtins and
                    // the script's own functions.
                    for (at, _) in left.match_indices('(') {
                        let after = &left[at + 1..];
                        assert!(
                            ["commandline ", "count ", "math ", "__a2m_"]
                                .iter()
                                .any(|allowed| after.starts_with(allowed)),
                            "{shell}: {line}"
                        );
                    }
                    // A condition calls the script's own functions alone,
                    // and an offered word is only itself.
                    for condition in fish_values(line, "-n") {
                        assert!(condition.starts_with("__a2m_"), "{shell}: {line}");
                    }
                    for word in fish_values(line, "-a") {
                        assert!(!word.contains(['(', '$', ';', '|', '&']), "{shell}: {line}");
                    }
                }
                _ => {
                    for forbidden in ["$(", "`", "<(", "eval ", "Invoke-Expression", "& "] {
                        assert!(!left.contains(forbidden), "{shell}: {forbidden}: {line}");
                    }
                }
            }
        }
    }
}

/// A bash on this machine, at one of the places it is kept.
fn bash() -> Option<PathBuf> {
    if !cfg!(unix) {
        return None;
    }
    [
        "/bin/bash",
        "/usr/bin/bash",
        "/usr/local/bin/bash",
        "/opt/homebrew/bin/bash",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

/// What the bash script offers for the words given, the last being the one
/// completed.
fn offered(bash: &Path, file: &Path, words: &[&str]) -> Vec<String> {
    let quoted: Vec<String> = words.iter().map(|word| format!("'{word}'")).collect();
    let program = format!(
        "source \"$1\"; COMP_WORDS=({}); COMP_CWORD={}; _a2m_complete; printf '%s\\n' \"${{COMPREPLY[@]}}\"",
        quoted.join(" "),
        words.len() - 1
    );
    let output = Command::new(bash)
        .args(["--norc", "--noprofile", "-c", &program, "completion"])
        .arg(file)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let mut words: Vec<String> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    words.sort();
    words
}

#[test]
fn the_bash_script_completes_a_command_and_a_flag_in_bash() {
    let Some(bash) = bash() else {
        println!("skipped: no bash at /bin, /usr/bin, /usr/local/bin, or /opt/homebrew/bin");
        return;
    };
    let folder = Folder::new("complete-bash");
    let file = folder.path().join("anything2mcp.bash");
    std::fs::write(&file, script("bash")).unwrap();
    let registry = anything2mcp::action_registry().unwrap();
    let faces = command_faces(&registry);

    // The nouns that begin `to`.
    let mut nouns: Vec<String> = faces
        .iter()
        .map(|face| face.noun.clone())
        .filter(|noun| noun.starts_with("to"))
        .collect();
    nouns.sort();
    nouns.dedup();
    assert!(!nouns.is_empty());
    assert_eq!(offered(&bash, &file, &["anything2mcp", "to"]), nouns);

    // A flag of `run list`, after a global flag that takes a value.
    let face = faces.iter().find(|face| face.action == "run.list").unwrap();
    assert!(face.args.iter().any(|arg| arg.flag == "--limit"));
    assert_eq!(
        offered(
            &bash,
            &file,
            &["a2m", "--project", "here", "run", "list", "--li"]
        ),
        ["--limit"]
    );
    // The shells of the completion command itself.
    assert_eq!(
        offered(&bash, &file, &["anything2mcp", COMPLETION_COMMAND, ""]),
        ["bash", "fish", "powershell", "zsh"]
    );
}
