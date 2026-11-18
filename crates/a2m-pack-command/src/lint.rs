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
        surfaces
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
            })
            .collect()
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
