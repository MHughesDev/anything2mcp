//! The pack's lint rules that look across files. What one binding or one
//! channel file gets wrong is reported where it is lowered or compiled.

use std::collections::BTreeSet;

use a2m_core::LintCode;
use a2m_files::{CompileMode, Diagnostic, Level, LintRule, Located, ResolvedView, Stage};

use crate::channel::{code, problem};
use crate::names::{BROKER, CHANNEL, MESSAGE_SCHEMA, PACK};

/// A channel names a message schema that `schemas/` defines.
pub(crate) struct UnknownSchema;

impl LintRule for UnknownSchema {
    fn code(&self) -> LintCode {
        code("unknown_schema")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "A channel names a message schema that a file under `schemas/` defines."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let defined: BTreeSet<&str> = view
            .entries(&MESSAGE_SCHEMA, &BROKER)
            .iter()
            .filter_map(|doc| doc.text("schema"))
            .collect();
        view.entries(&CHANNEL, &BROKER)
            .iter()
            .filter_map(|doc| doc.get("schema"))
            .filter(|schema| !schema.as_text().is_some_and(|name| defined.contains(name)))
            .map(|schema| {
                problem(
                    "unknown_schema",
                    Stage::Lint,
                    &schema.at,
                    "No file under `schemas/` defines a message schema of this name.",
                    "Name a schema that `schemas/` defines, or add its file.",
                )
            })
            .collect()
    }
}

/// A channel that is published to says who reads it.
pub(crate) struct ConsumersUnknown;

impl LintRule for ConsumersUnknown {
    fn code(&self) -> LintCode {
        code("consumers_unknown")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Warning
    }

    fn summary(&self) -> &str {
        "A channel that is published to says who reads it."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let published: BTreeSet<&str> = view
            .operations()
            .iter()
            .flat_map(|operation| {
                operation
                    .doc()
                    .get("bindings")
                    .map(Located::items)
                    .unwrap_or_default()
            })
            .filter(|binding| {
                binding.text("mode") == Some(PACK) && binding.text("verb") == Some("publish")
            })
            .filter_map(|binding| binding.text("channel"))
            .collect();
        view.entries(&CHANNEL, &BROKER)
            .iter()
            .filter(|doc| {
                doc.text("channel")
                    .is_some_and(|name| published.contains(name))
                    && doc.get("consumers").is_none()
            })
            .map(|doc| {
                problem(
                    "consumers_unknown",
                    Stage::Lint,
                    &doc.at,
                    "The channel is published to and does not say who reads it, so every publish is treated as leaving the target.",
                    "Add `consumers: outside`, or `consumers: none` when nothing outside the target reads the channel.",
                )
            })
            .collect()
    }
}
