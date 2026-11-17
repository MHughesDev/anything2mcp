//! The pack's lint rules that look across files. What one binding or one
//! channel file gets wrong is reported where it is lowered or compiled.

use std::collections::BTreeSet;

use a2m_core::{LintCode, ValueType};
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

/// Whether a value of this type is, or holds, text.
fn holds_text(ty: &ValueType) -> bool {
    match ty {
        ValueType::Text => true,
        ValueType::List(of) => holds_text(of),
        ValueType::Record(fields) => fields.iter().any(|field| holds_text(&field.ty)),
        _ => false,
    }
}

/// Text a broker returns reaches a caller only as untrusted text.
///
/// Every value of the mode is a broker's: a message another producer
/// wrote, or the broker's own words about one. So every output of an
/// operation bound through the mode that is or holds text must be of a
/// field type marked untrusted. One kind of text is not free: a field the
/// channel's schema holds to an `enum`, because the driver refuses a
/// message whose field holds anything else.
pub(crate) struct UntrustedUnmarked;

impl LintRule for UntrustedUnmarked {
    fn code(&self) -> LintCode {
        code("untrusted_unmarked")
    }

    fn level(&self, _mode: CompileMode) -> Level {
        Level::Error
    }

    fn summary(&self) -> &str {
        "An output that holds text a broker returns is of a field type marked untrusted, or a field the channel's schema holds to an `enum`."
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let channels = view.entries(&CHANNEL, &BROKER);
        let schemas = view.entries(&MESSAGE_SCHEMA, &BROKER);
        // Whether the schema of a channel holds a field to listed texts.
        let listed = |channel: &str, field: &str| {
            channels
                .iter()
                .find(|doc| doc.text("channel") == Some(channel))
                .and_then(|doc| doc.text("schema"))
                .and_then(|name| schemas.iter().find(|doc| doc.text("schema") == Some(name)))
                .and_then(|doc| doc.get("properties"))
                .and_then(|properties| properties.get(field))
                .and_then(|rule| rule.get("enum"))
                .is_some_and(|texts| !texts.items().is_empty())
        };
        let mut found = Vec::new();
        for operation in view.operations() {
            let bound: Vec<&str> = operation
                .doc()
                .get("bindings")
                .map(Located::items)
                .unwrap_or_default()
                .iter()
                .filter(|binding| binding.text("mode") == Some(PACK))
                .map(|binding| binding.text("channel").unwrap_or_default())
                .collect();
            if bound.is_empty() {
                continue;
            }
            for output in operation
                .doc()
                .get("outputs")
                .map(Located::entries)
                .unwrap_or_default()
            {
                let name = output.key.value.as_str();
                let text = operation
                    .outputs()
                    .iter()
                    .find(|plan| plan.name == name)
                    .is_some_and(|plan| holds_text(&plan.ty));
                let marked = output
                    .value
                    .text("type")
                    .and_then(|field_type| view.target().field_type(field_type))
                    .is_some_and(|field_type| field_type.plan.untrusted);
                if text && !marked && !bound.iter().all(|channel| listed(channel, name)) {
                    found.push(problem(
                        "untrusted_unmarked",
                        Stage::Lint,
                        &output.key.at,
                        format!(
                            "The output `{name}` is text, and text a broker returns is untrusted: its field type is not marked untrusted, and the channel's schema does not hold the field to an `enum`."
                        ),
                        "Give the output a field type with `untrusted: true`, or list the texts the field may hold under `enum` in the channel's schema.",
                    ));
                }
            }
        }
        found
    }
}
