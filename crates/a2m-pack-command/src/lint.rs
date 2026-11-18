//! The pack's lint rules that look across files. The rules a `commands/`
//! file can break on its own are checked where the file is compiled.

use a2m_core::LintCode;
use a2m_files::{CompileMode, Diagnostic, Level, LintRule, Located, ResolvedView, Stage};

use crate::codes::{FILE_KIND, SURFACE_KIND, lint};
use crate::kind::is_shell_or_interpreter;

/// A program surface is never a shell or an interpreter.
pub(crate) struct ShellProgram;

impl LintRule for ShellProgram {
    fn code(&self) -> LintCode {
        lint("shell_program")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "A program surface is never a shell or an interpreter."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let surfaces: Vec<_> = view
            .target()
            .surfaces()
            .into_iter()
            .filter(|surface| *surface.kind == SURFACE_KIND)
            .collect();
        let first = surfaces.first().map(|surface| surface.name.as_str());
        let docs = view.entries(&FILE_KIND, &SURFACE_KIND);
        // An interpreter is allowed only as the half of fixed pairs: every
        // `commands/` file on its surface declares it so, and each of their
        // commands names its script, which the file kind holds them to.
        let pairs_only = |surface: &str| {
            let on: Vec<_> = docs
                .iter()
                .filter(|doc| doc.text("surface").or(first) == Some(surface))
                .collect();
            !on.is_empty()
                && on.iter().all(|doc| {
                    doc.get("program")
                        .and_then(|program| program.get("interpreter"))
                        .is_some()
                })
        };
        // The interpreter of a pair is started only in the one form the
        // provider's closed table gives it, in which it reads a script file
        // and takes what follows as data (decision 0250).
        let mut found: Vec<Diagnostic> = Vec::new();
        for surface in &surfaces {
            for doc in docs
                .iter()
                .filter(|doc| doc.text("surface").or(first) == Some(surface.name.as_str()))
            {
                let Some(interpreter) = doc
                    .get("program")
                    .and_then(|program| program.get("interpreter"))
                else {
                    continue;
                };
                let fixed: Vec<String> = interpreter
                    .get("args")
                    .map(Located::items)
                    .unwrap_or_default()
                    .iter()
                    .map(|item| item.as_text().unwrap_or_default().to_owned())
                    .collect();
                let why = match a2m_provider::interpreter_form(surface.address, &fixed) {
                    Ok(()) => continue,
                    Err(a2m_provider::InterpreterRefused::ReadsItsLineAsCode) => {
                        "reads its whole command line as code, so a value given to its script would be read as a command too".to_owned()
                    }
                    Err(a2m_provider::InterpreterRefused::NotKnown) => {
                        "is not an interpreter the pack knows the form of: no test has proved that it takes what follows its script as data".to_owned()
                    }
                    Err(a2m_provider::InterpreterRefused::NotItsForm(form)) => format!(
                        "is given fixed arguments other than its one form, which is `[{}]`",
                        form.join(", ")
                    ),
                };
                found.push(Diagnostic::error(
                    self.code(),
                    Stage::Lint,
                    &interpreter.at,
                    format!(
                        "The surface `{}` names an interpreter that {why}.",
                        surface.name
                    ),
                    "Name an interpreter the pack starts as a pair (the POSIX shell, Python, Windows PowerShell, or AppleScript), with the fixed arguments of its form, or name the program itself.",
                ));
            }
        }
        found.extend(surfaces
            .iter()
            .filter(|surface| is_shell_or_interpreter(surface.address))
            .filter(|surface| !pairs_only(surface.name.as_str()))
            .map(|surface| {
                Diagnostic::error(
                    self.code(),
                    Stage::Lint,
                    &surface.doc.at,
                    format!(
                        "The surface `{}` names a shell, an interpreter, or a script, which reads its arguments as code.",
                        surface.name
                    ),
                    "Name the program itself, by its absolute path; a shell is never started. An interpreter runs only a script the target keeps: declare `program.interpreter` in every `commands/` file on the surface.",
                )
            }));
        found
    }
}

/// A write command declares the exit statuses that mean nothing changed.
pub(crate) struct WriteExitDeclared;

impl LintRule for WriteExitDeclared {
    fn code(&self) -> LintCode {
        lint("write_exit_declared")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Warning
    }

    fn summary(&self) -> &str {
        "A write command declares the exit statuses that mean nothing changed."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let mut found = Vec::new();
        for doc in view.entries(&FILE_KIND, &SURFACE_KIND) {
            let declares = |exits: Option<&Located<a2m_files::Doc>>| {
                exits
                    .map(Located::entries)
                    .unwrap_or_default()
                    .iter()
                    .any(|entry| entry.value.as_text() != Some("ok"))
            };
            let shared = declares(doc.get("program").and_then(|program| program.get("exits")));
            for entry in doc
                .get("commands")
                .map(Located::entries)
                .unwrap_or_default()
            {
                let writes = entry.value.text("effect") != Some("read");
                if writes && !shared && !declares(entry.value.get("exits")) {
                    found.push(Diagnostic::error(
                        self.code(),
                        Stage::Lint,
                        &entry.key.at,
                        format!(
                            "The command `{}` writes, and no exit status says that nothing changed: every failure of it will be a possible commit.",
                            entry.key.value
                        ),
                        "Declare the statuses the program uses to refuse, such as `2: input` and `4: rejected`, under `exits`.",
                    ));
                }
            }
        }
        found
    }
}
