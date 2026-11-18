//! What a PowerShell help file says of a command, as a finding, and the
//! drafting rule from that finding to a `commands/` draft (decision 0290).
//!
//! A PowerShell command runs in this pack only as a fixed pair: Windows
//! PowerShell, in its one form, and a script the target keeps (section
//! 306). So a draft names that form, and leaves as open questions what the
//! help file cannot say: the script, the program's digests, the effect, and
//! the output. Each parameter of a parameter set becomes a param given by
//! name: a switch as a flag given when it is true, a parameter that takes
//! a value as its name followed by the value.

use a2m_core::{FileKindName, Finding, FindingKind};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath};
use serde::{Deserialize, Serialize};

use crate::codes::{self, FILE_KIND, FOLDER};
use crate::draft::slug;

/// The fixed arguments of Windows PowerShell's one form as a pair: the
/// provider's closed table holds the same, and a test holds the two
/// together.
pub(crate) const FORM: [&str; 5] = [
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
];

/// The parameters PowerShell itself gives a command that says it changes
/// something. They are not drafted; they word the question of the effect.
const SHOULD_PROCESS: [&str; 2] = ["WhatIf", "Confirm"];

/// One parameter of one parameter set, as the help file states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterFound {
    /// Its name, without the dash.
    pub name: String,
    /// Its .NET type, as the help writes it.
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    /// Whether it is a switch.
    pub switch: bool,
    /// Whether the help says it is required; null when it does not say.
    pub required: Option<bool>,
    /// Its position: `named`, or a number from 0.
    pub position: Option<String>,
    /// How it takes a value from the pipeline: `by_value`,
    /// `by_property_name`, or `not_stated`; empty for not at all.
    pub pipeline: Vec<String>,
    /// What the help says of it, redacted.
    pub about: Option<String>,
}

/// What a `powershell_command` finding holds: one command a PowerShell
/// help file describes. Every text in it came from the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerShellCommandFound {
    /// Its name, such as `Get-ExampleItem`.
    pub name: String,
    /// The help's synopsis.
    pub synopsis: Option<String>,
    /// Its parameter sets, each with its parameters in the help's order.
    pub sets: Vec<Vec<ParameterFound>>,
}

impl PowerShellCommandFound {
    /// Reads what a `powershell_command` finding holds.
    ///
    /// # Errors
    ///
    /// `input`/`finding_invalid` when the finding is not one of this pack's.
    pub fn read(finding: &Finding) -> Result<Self, a2m_core::Failure> {
        written::from_text(finding.found()).ok_or_else(|| {
            a2m_core::codes::row::FINDING_INVALID.failure(
                "A `powershell_command` finding does not hold what the command pack's discoverer writes.",
                "Run the discovery again with the version of the pack that is enabled.",
            )
        })
    }
}

impl From<crate::maml::PsCommand> for PowerShellCommandFound {
    fn from(command: crate::maml::PsCommand) -> Self {
        Self {
            name: command.name,
            synopsis: command.synopsis,
            sets: command
                .sets
                .into_iter()
                .map(|set| {
                    set.into_iter()
                        .map(|parameter| ParameterFound {
                            name: parameter.name,
                            type_name: parameter.type_name,
                            switch: parameter.switch,
                            required: parameter.required,
                            position: parameter.position,
                            pipeline: parameter.pipeline.into_iter().map(str::to_owned).collect(),
                            about: parameter.about,
                        })
                        .collect()
                })
                .collect(),
        }
    }
}

/// Reads a finding's text back with the crate's own JSON reader, and
/// accepts only what the discoverer itself would have written.
mod written {
    use a2m_core::PackPayload;

    use super::{ParameterFound, PowerShellCommandFound};
    use crate::json::{self, Json};

    fn field<'a>(fields: &'a [(String, Json)], name: &str) -> Option<&'a Json> {
        fields.iter().find(|(key, _)| key == name).map(|(_, v)| v)
    }

    fn text(value: &Json) -> Option<String> {
        match value {
            Json::Text(text) => Some(text.clone()),
            _ => None,
        }
    }

    fn maybe_text(value: &Json) -> Option<Option<String>> {
        match value {
            Json::Null => Some(None),
            other => text(other).map(Some),
        }
    }

    fn parameter(value: &Json) -> Option<ParameterFound> {
        let Json::Object(fields) = value else {
            return None;
        };
        Some(ParameterFound {
            name: text(field(fields, "name")?)?,
            type_name: maybe_text(field(fields, "type")?)?,
            switch: match field(fields, "switch")? {
                Json::Bool(switch) => *switch,
                _ => return None,
            },
            required: match field(fields, "required")? {
                Json::Null => None,
                Json::Bool(required) => Some(*required),
                _ => return None,
            },
            position: maybe_text(field(fields, "position")?)?,
            pipeline: match field(fields, "pipeline")? {
                Json::List(items) => items.iter().map(text).collect::<Option<Vec<_>>>()?,
                _ => return None,
            },
            about: maybe_text(field(fields, "about")?)?,
        })
    }

    pub(super) fn from_text(found: &str) -> Option<PowerShellCommandFound> {
        let Json::Object(fields) = json::parse(found).ok()? else {
            return None;
        };
        let sets = match field(&fields, "sets")? {
            Json::List(sets) => sets
                .iter()
                .map(|set| match set {
                    Json::List(items) => items.iter().map(parameter).collect::<Option<Vec<_>>>(),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?,
            _ => return None,
        };
        let read = PowerShellCommandFound {
            name: text(field(&fields, "name")?)?,
            synopsis: maybe_text(field(&fields, "synopsis")?)?,
            sets,
        };
        // Only what this crate itself would have written is accepted.
        let again = PackPayload::encode(&read).ok()?;
        (again.as_bytes() == found.as_bytes()).then_some(read)
    }
}

/// Whether a .NET type is a whole number.
fn whole(type_name: &str) -> bool {
    let last = type_name.rsplit('.').next().unwrap_or(type_name);
    [
        "Int16", "Int32", "Int64", "UInt16", "UInt32", "UInt64", "Byte", "SByte", "int", "long",
    ]
    .iter()
    .any(|known| last.eq_ignore_ascii_case(known))
}

const EFFECT: &str = "Does the quoted PowerShell command read, write, or do something that cannot be taken back? Its help does not say, and its name proves nothing.";
const EFFECT_CHANGES: &str = "Does the quoted PowerShell command read, write, or do something that cannot be taken back? Its help lists the parameter quoted second, which a PowerShell command is given when it says it changes something: that is the command's own claim, and proves nothing about what it changes.";
const SCRIPT: &str = "Which script, kept in the target's folder, runs the quoted PowerShell command? Give its path and its SHA-256. Windows PowerShell runs it with `-File`, and binds the parameters below to it by name.";
const DIGESTS: &str = "What is the SHA-256 of Windows PowerShell's program, on each platform the target runs on? A PowerShell command runs only as a fixed pair of Windows PowerShell and a script the target keeps, and a help file names neither.";
const TYPED: &str = "What type is the value of the quoted parameter of the quoted PowerShell command? The first quoted words are the command, the second the parameter, and the rest the type its help file gives and what the file says of it. A text param also needs an anchored `pattern`.";
const REQUIRED: &str = "Must the quoted parameter of the quoted PowerShell command always be given? Its help file does not say. The first quoted words are the command, the second the parameter.";
const OUTPUT: &str = "What does the quoted PowerShell command print? Its output was not seen, because the command was not run.";

/// The drafting rule from `powershell_command` findings to `commands/`
/// files.
pub(crate) struct PowerShellFromFinding;

/// The params and args of one parameter set.
fn set_of(
    found: &PowerShellCommandFound,
    set: &[ParameterFound],
    cx: &DraftContext<'_>,
) -> (Vec<(String, DraftValue)>, Vec<DraftValue>) {
    let command = found.name.as_str();
    let mut params: Vec<(String, DraftValue)> = Vec::new();
    let mut args: Vec<DraftValue> = Vec::new();
    for parameter in set {
        if SHOULD_PROCESS
            .iter()
            .any(|own| own.eq_ignore_ascii_case(&parameter.name))
        {
            continue;
        }
        let name = slug(&parameter.name);
        if params.iter().any(|(known, _)| *known == name) {
            continue;
        }
        let flag = format!("-{}", parameter.name);
        if parameter.switch {
            if parameter.required == Some(true) {
                args.push(DraftValue::Text(flag));
            } else {
                params.push((
                    name.clone(),
                    DraftValue::map(vec![
                        ("type", DraftValue::text("boolean")),
                        ("required", DraftValue::Bool(false)),
                    ]),
                ));
                args.push(DraftValue::map(vec![
                    ("flag", DraftValue::Text(flag)),
                    ("when", DraftValue::Text(name)),
                ]));
            }
            continue;
        }
        let ty = match parameter.type_name.as_deref() {
            Some(type_name) if whole(type_name) => DraftValue::text("integer"),
            type_name => cx.open_question_quoting(
                TYPED,
                &[
                    command,
                    &parameter.name,
                    type_name.unwrap_or_default(),
                    parameter.about.as_deref().unwrap_or_default(),
                ],
            ),
        };
        let mut fields = vec![("type", ty)];
        match parameter.required {
            Some(true) => {}
            Some(false) => fields.push(("required", DraftValue::Bool(false))),
            None => fields.push((
                "required",
                cx.open_question_quoting(REQUIRED, &[command, &parameter.name]),
            )),
        }
        params.push((name.clone(), DraftValue::map(fields)));
        args.push(DraftValue::map(vec![
            ("flag", DraftValue::Text(flag)),
            ("param", DraftValue::Text(name)),
        ]));
    }
    (params, args)
}

impl DraftingRule for PowerShellFromFinding {
    fn finding_kind(&self) -> FindingKind {
        codes::powershell_finding_kind()
    }

    fn file_kind(&self) -> FileKindName {
        FILE_KIND
    }

    fn draft(
        &self,
        finding: &Finding,
        cx: &DraftContext<'_>,
    ) -> Result<Vec<DraftDoc>, a2m_core::Failure> {
        let found = PowerShellCommandFound::read(finding)?;
        let name = slug(&found.name);
        let program = vec![
            ("digests", cx.open_question(DIGESTS)),
            (
                "interpreter",
                DraftValue::map(vec![(
                    "args",
                    DraftValue::List(FORM.iter().map(|arg| DraftValue::text(arg)).collect()),
                )]),
            ),
            (
                "exits",
                DraftValue::Map(vec![("0".to_owned(), DraftValue::text("ok"))]),
            ),
        ];
        let changes = found
            .sets
            .iter()
            .flatten()
            .find(|parameter| {
                SHOULD_PROCESS
                    .iter()
                    .any(|own| own.eq_ignore_ascii_case(&parameter.name))
            })
            .map(|parameter| parameter.name.clone());
        let mut commands = Vec::new();
        let several = found.sets.len() > 1;
        for (at, set) in found.sets.iter().enumerate() {
            let key = if several {
                format!("{name}_{}", at + 1)
            } else {
                name.clone()
            };
            let effect = match &changes {
                Some(own) => cx.open_question_quoting(EFFECT_CHANGES, &[&found.name, own]),
                None => cx.open_question_quoting(EFFECT, &[&found.name]),
            };
            let (params, args) = set_of(&found, set, cx);
            let mut command = vec![
                ("effect", effect),
                ("script", cx.open_question_quoting(SCRIPT, &[&found.name])),
            ];
            if !params.is_empty() {
                command.push(("params", DraftValue::Map(params)));
            }
            command.push(("args", DraftValue::List(args)));
            command.push(("output", cx.open_question_quoting(OUTPUT, &[&found.name])));
            commands.push((key, DraftValue::map(command)));
        }
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("{FOLDER}/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("program", DraftValue::map(program)),
                ("commands", DraftValue::Map(commands)),
            ]),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::{FORM, whole};

    #[test]
    fn the_form_is_the_providers_and_whole_numbers_are_known_by_type() {
        let fixed: Vec<String> = FORM.iter().map(|arg| (*arg).to_owned()).collect();
        assert_eq!(
            a2m_provider::interpreter_form(
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
                &fixed
            ),
            Ok(())
        );
        assert!(whole("System.Int32") && whole("UInt64") && whole("int"));
        assert!(!whole("System.String") && !whole("System.Int32[]") && !whole("Double"));
    }
}
