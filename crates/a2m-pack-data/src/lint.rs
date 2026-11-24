//! The pack's lint rules. Each sees the resolved target and changes
//! nothing.

use a2m_core::{Hazard, LintCode};
use a2m_files::{CompileMode, Diagnostic, Doc, Level, LintRule, Located, ResolvedView, Stage};

use crate::names::{STATEMENT, lint};

/// Every statement the target declares, with its body.
fn statements<'a>(
    view: &ResolvedView<'a>,
) -> Vec<(&'a str, &'a Located<Doc>, &'a Located<String>)> {
    crate::names::surface_kinds()
        .iter()
        .flat_map(|kind| view.entries(&STATEMENT, kind))
        .flat_map(|doc| {
            doc.get("statements")
                .map(Located::entries)
                .unwrap_or_default()
        })
        .map(|entry| (entry.key.value.as_str(), &entry.value, &entry.key))
        .collect()
}

/// The statements an operation's bindings name, with the place of each.
fn bound(operation: &Located<Doc>) -> Vec<&Located<Doc>> {
    operation
        .get("bindings")
        .map(Located::items)
        .unwrap_or_default()
        .iter()
        .filter(|binding| binding.get("statement").is_some())
        .flat_map(|binding| [binding.get("statement"), binding.get("read_back")])
        .flatten()
        .collect()
}

/// An operation bound to a write statement that declares a hazard declares
/// that hazard too.
pub(crate) struct HazardNotDeclared;

impl LintRule for HazardNotDeclared {
    fn code(&self) -> LintCode {
        lint::HAZARD_NOT_DECLARED
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "An operation bound to a write statement that declares a hazard declares that hazard too."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let declared = statements(view);
        let mut found = Vec::new();
        for operation in view.operations() {
            if operation.hazard() != Hazard::None {
                continue;
            }
            for named in bound(operation.doc()) {
                let hazardous = declared.iter().any(|(name, body, _)| {
                    Some(*name) == named.as_text() && body.text("hazard") == Some("data_elsewhere")
                });
                if hazardous {
                    found.push(Diagnostic::error(
                        self.code(),
                        Stage::Lint,
                        &named.at,
                        format!(
                            "The statement `{}` declares the hazard `data_elsewhere`, and the operation `{}` declares none.",
                            named.as_text().unwrap_or_default(),
                            operation.name()
                        ),
                        "Declare `hazard: data_elsewhere` and `effect: irreversible` on the operation.",
                    ));
                }
            }
        }
        found
    }
}

/// Every declared statement is named by a binding.
pub(crate) struct StatementUnused;

impl LintRule for StatementUnused {
    fn code(&self) -> LintCode {
        lint::STATEMENT_UNUSED
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Warning
    }

    fn summary(&self) -> &str {
        "Every declared statement is named by a binding."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let used: Vec<&str> = view
            .operations()
            .iter()
            .flat_map(|operation| bound(operation.doc()))
            .filter_map(|named| named.as_text())
            .collect();
        statements(view)
            .into_iter()
            .filter(|(name, _, _)| !used.contains(name))
            .map(|(name, _, key)| {
                Diagnostic::error(
                    self.code(),
                    Stage::Lint,
                    &key.at,
                    format!("No binding names the statement `{name}`."),
                    "Bind an operation to the statement, or remove it: every statement in a compiled target is one a tool may run.",
                )
                .as_warning()
            })
            .collect()
    }
}
