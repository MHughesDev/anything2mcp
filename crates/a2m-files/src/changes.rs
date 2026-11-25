//! What changed in each tool between two versions of a target, and whether
//! the change breaks a caller (decision 0439, point 2).
//!
//! The list of what is breaking is closed: the tool is gone; an input is
//! gone or renamed; an input becomes required, or a required input is
//! added with no default; an input's type changes or its words narrow; an
//! output is gone, or one that was always present may now be absent; an
//! output's type changes; the effect class rises; the hazard is added or
//! changed; the record's key changes. Everything else is compatible. The
//! comparison reads only the two compiled targets: `version.diff` gives it
//! for any two published versions, and the review summary of a publish
//! carries it against the version in force.

use std::collections::BTreeSet;

use a2m_core::{
    CompiledInput, CompiledOutput, CompiledTarget, CompiledTool, EffectClass, Hazard,
    OperationName, ToolRuns, ValueType, Version,
};
use serde::Serialize;

/// Whether a tool's change breaks a caller that worked with the earlier
/// version. The order is the order a person reads them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeClass {
    /// At least one change is on the closed list of decision 0439.
    Breaking,
    /// Something changed, and nothing on that list.
    Compatible,
    /// Nothing a caller reads changed.
    Unchanged,
}

/// One change to one tool, typed, so that a page and a client build their
/// own sentences and never print one made from a target's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    // ---- Breaking: the closed list ----
    /// The tool is not in the later version.
    ToolGone,
    /// An input is gone, or was renamed: a caller that gives it is refused.
    InputGone {
        /// The input's name in the earlier version.
        input: String,
    },
    /// An input a caller could leave out must now be given: it became
    /// required, or it is new, required, and has no default.
    InputRequired {
        /// The input.
        input: String,
    },
    /// An input's type changed.
    InputTypeChanged {
        /// The input.
        input: String,
        /// Its type in the earlier version.
        from: String,
        /// Its type in the later one.
        to: String,
    },
    /// An input's words narrowed: a value it took is now refused.
    InputWordsNarrowed {
        /// The input.
        input: String,
        /// The words it no longer takes.
        removed: Vec<String>,
    },
    /// An output is gone.
    OutputGone {
        /// The output's name in the earlier version.
        output: String,
    },
    /// An output that was always present may now be absent.
    OutputMayBeAbsent {
        /// The output.
        output: String,
    },
    /// An output's type changed.
    OutputTypeChanged {
        /// The output.
        output: String,
        /// Its type in the earlier version.
        from: String,
        /// Its type in the later one.
        to: String,
    },
    /// The effect class rose.
    EffectRose {
        /// The earlier class.
        from: EffectClass,
        /// The later one.
        to: EffectClass,
    },
    /// A hazard was added, or changed to another.
    HazardChanged {
        /// The earlier hazard, `none` where there was none.
        from: Hazard,
        /// The later one.
        to: Hazard,
    },
    /// The inputs that name the record changed.
    KeyChanged {
        /// The key's inputs in the earlier version.
        from: Vec<String>,
        /// The key's inputs in the later one.
        to: Vec<String>,
    },
    // ---- Compatible ----
    /// The tool is new in the later version.
    ToolAdded,
    /// A new input a caller may leave out.
    InputAdded {
        /// The input.
        input: String,
    },
    /// An input that had to be given may now be left out.
    InputLoosened {
        /// The input.
        input: String,
    },
    /// An input takes words it did not take before.
    InputWordsWidened {
        /// The input.
        input: String,
        /// The words it now also takes.
        added: Vec<String>,
    },
    /// A new output.
    OutputAdded {
        /// The output.
        output: String,
    },
    /// An output that could be absent is now always present.
    OutputAlwaysPresent {
        /// The output.
        output: String,
    },
    /// The words an output may hold changed.
    OutputWordsChanged {
        /// The output.
        output: String,
        /// The words it may now hold that it could not.
        added: Vec<String>,
        /// The words it can no longer hold.
        removed: Vec<String>,
    },
    /// The effect class fell.
    EffectLowered {
        /// The earlier class.
        from: EffectClass,
        /// The later one.
        to: EffectClass,
    },
    /// The hazard is gone.
    HazardRemoved {
        /// The earlier hazard.
        from: Hazard,
    },
    /// The tool's description changed. Compatible, and it still changes
    /// what a client that pins the tool's digest reads.
    DescriptionChanged,
    /// Something else a client reads changed, such as the title or an
    /// input's description, and nothing named above.
    DefinitionChanged,
}

impl Change {
    /// Whether the change is on the closed list of breaking changes.
    #[must_use]
    pub const fn breaking(&self) -> bool {
        matches!(
            self,
            Self::ToolGone
                | Self::InputGone { .. }
                | Self::InputRequired { .. }
                | Self::InputTypeChanged { .. }
                | Self::InputWordsNarrowed { .. }
                | Self::OutputGone { .. }
                | Self::OutputMayBeAbsent { .. }
                | Self::OutputTypeChanged { .. }
                | Self::EffectRose { .. }
                | Self::HazardChanged { .. }
                | Self::KeyChanged { .. }
        )
    }

    /// The change with every text from a target's files passed through
    /// `f`: names of inputs and outputs, words, and types' names.
    #[must_use]
    pub fn map_text(&self, f: &dyn Fn(&str) -> String) -> Self {
        let all = |texts: &[String]| -> Vec<String> { texts.iter().map(|text| f(text)).collect() };
        match self {
            Self::InputGone { input } => Self::InputGone { input: f(input) },
            Self::InputRequired { input } => Self::InputRequired { input: f(input) },
            Self::InputTypeChanged { input, from, to } => Self::InputTypeChanged {
                input: f(input),
                from: f(from),
                to: f(to),
            },
            Self::InputWordsNarrowed { input, removed } => Self::InputWordsNarrowed {
                input: f(input),
                removed: all(removed),
            },
            Self::OutputGone { output } => Self::OutputGone { output: f(output) },
            Self::OutputMayBeAbsent { output } => Self::OutputMayBeAbsent { output: f(output) },
            Self::OutputTypeChanged { output, from, to } => Self::OutputTypeChanged {
                output: f(output),
                from: f(from),
                to: f(to),
            },
            Self::KeyChanged { from, to } => Self::KeyChanged {
                from: all(from),
                to: all(to),
            },
            Self::InputAdded { input } => Self::InputAdded { input: f(input) },
            Self::InputLoosened { input } => Self::InputLoosened { input: f(input) },
            Self::InputWordsWidened { input, added } => Self::InputWordsWidened {
                input: f(input),
                added: all(added),
            },
            Self::OutputAdded { output } => Self::OutputAdded { output: f(output) },
            Self::OutputAlwaysPresent { output } => Self::OutputAlwaysPresent { output: f(output) },
            Self::OutputWordsChanged {
                output,
                added,
                removed,
            } => Self::OutputWordsChanged {
                output: f(output),
                added: all(added),
                removed: all(removed),
            },
            other => other.clone(),
        }
    }
}

/// What changed in one tool, the breaking changes first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolChange {
    /// The tool.
    pub tool: OperationName,
    /// Breaking when any change is.
    pub class: ChangeClass,
    /// Each change, the breaking ones first; empty when unchanged.
    pub changes: Vec<Change>,
}

impl ToolChange {
    fn of(tool: OperationName, mut changes: Vec<Change>) -> Self {
        changes.sort_by_key(|change| !change.breaking());
        let class = if changes.iter().any(Change::breaking) {
            ChangeClass::Breaking
        } else if changes.is_empty() {
            ChangeClass::Unchanged
        } else {
            ChangeClass::Compatible
        };
        Self {
            tool,
            class,
            changes,
        }
    }
}

/// Every tool of either of two versions of a target, with what changed in
/// it from the earlier to the later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VersionChanges {
    /// The earlier version.
    pub from: Version,
    /// The later version.
    pub to: Version,
    /// Each tool of either version: the breaking first, then the
    /// compatible, then the unchanged, each group by name.
    pub tools: Vec<ToolChange>,
}

impl VersionChanges {
    /// The tools whose change breaks a caller.
    pub fn breaking(&self) -> impl Iterator<Item = &ToolChange> {
        self.tools
            .iter()
            .filter(|tool| tool.class == ChangeClass::Breaking)
    }

    /// What changed in one tool, where it is in either version.
    #[must_use]
    pub fn of(&self, tool: &OperationName) -> Option<&ToolChange> {
        self.tools.iter().find(|change| &change.tool == tool)
    }

    /// The comparison with every text from a target's files passed through
    /// `f`.
    #[must_use]
    pub fn map_text(&self, f: &dyn Fn(&str) -> String) -> Self {
        Self {
            from: self.from,
            to: self.to,
            tools: self
                .tools
                .iter()
                .map(|tool| ToolChange {
                    tool: tool.tool.clone(),
                    class: tool.class,
                    changes: tool
                        .changes
                        .iter()
                        .map(|change| change.map_text(f))
                        .collect(),
                })
                .collect(),
        }
    }
}

/// What a caller of one tool depends on, read from a compiled target.
struct Contract<'a> {
    tool: &'a CompiledTool,
    effect: EffectClass,
    hazard: Hazard,
    inputs: &'a [CompiledInput],
    /// Each output with the words it may hold; none for a pipeline, whose
    /// result declares no fields.
    outputs: Vec<(&'a CompiledOutput, Vec<String>)>,
    /// The inputs that name the record; empty for a pipeline.
    key: Vec<String>,
    compiled_target: &'a CompiledTarget,
}

fn contract<'a>(
    compiled_target: &'a CompiledTarget,
    tool: &'a CompiledTool,
) -> Option<Contract<'a>> {
    let words = |field_type| {
        compiled_target
            .field_type(field_type)
            .map(|known| known.codes.iter().map(|code| code.code.clone()).collect())
            .unwrap_or_default()
    };
    Some(match tool.runs {
        ToolRuns::Operation(index) => {
            let operation = compiled_target.operation(index)?;
            Contract {
                tool,
                effect: operation.effect,
                hazard: operation.hazard,
                inputs: &operation.inputs,
                outputs: operation
                    .outputs
                    .iter()
                    .map(|output| (output, output.field_type.map(words).unwrap_or_default()))
                    .collect(),
                key: operation
                    .identity
                    .key
                    .iter()
                    .filter_map(|input| operation.inputs.get(input.get()))
                    .map(|input| input.name.clone())
                    .collect(),
                compiled_target,
            }
        }
        ToolRuns::Pipeline(index) => {
            let pipeline = compiled_target.pipeline(index)?;
            Contract {
                tool,
                effect: pipeline.effect,
                hazard: pipeline.hazard,
                inputs: &pipeline.inputs,
                outputs: Vec::new(),
                key: Vec::new(),
                compiled_target,
            }
        }
    })
}

impl Contract<'_> {
    fn words_of(&self, input: &CompiledInput) -> Vec<String> {
        self.compiled_target
            .field_type(input.field_type)
            .map(|known| known.codes.iter().map(|code| code.code.clone()).collect())
            .unwrap_or_default()
    }
}

/// A type as a person reads it: its name, and what a list holds.
fn type_text(ty: &ValueType) -> String {
    match ty {
        ValueType::List(item) => format!("list of {}", type_text(item)),
        other => other.name().to_owned(),
    }
}

/// Whether a caller must give the input.
fn must_give(input: &CompiledInput) -> bool {
    input.required && input.default.is_none()
}

fn difference(left: &[String], right: &[String]) -> Vec<String> {
    let right: BTreeSet<&String> = right.iter().collect();
    left.iter()
        .filter(|word| !right.contains(word))
        .cloned()
        .collect()
}

fn compared(before: &Contract<'_>, after: &Contract<'_>) -> Vec<Change> {
    let mut changes = Vec::new();
    // Inputs.
    for earlier in before.inputs {
        let Some(later) = after.inputs.iter().find(|later| later.name == earlier.name) else {
            changes.push(Change::InputGone {
                input: earlier.name.clone(),
            });
            continue;
        };
        if !must_give(earlier) && must_give(later) {
            changes.push(Change::InputRequired {
                input: later.name.clone(),
            });
        } else if must_give(earlier) && !must_give(later) {
            changes.push(Change::InputLoosened {
                input: later.name.clone(),
            });
        }
        if earlier.ty == later.ty {
            let (was, is) = (before.words_of(earlier), after.words_of(later));
            let removed = difference(&was, &is);
            let added = difference(&is, &was);
            if !removed.is_empty() && !was.is_empty() {
                changes.push(Change::InputWordsNarrowed {
                    input: later.name.clone(),
                    removed,
                });
            } else if !added.is_empty() && !was.is_empty() {
                changes.push(Change::InputWordsWidened {
                    input: later.name.clone(),
                    added,
                });
            } else if was.is_empty() && !is.is_empty() {
                // A free value now limited to words: a value it took may be
                // refused.
                changes.push(Change::InputWordsNarrowed {
                    input: later.name.clone(),
                    removed: Vec::new(),
                });
            }
        } else {
            changes.push(Change::InputTypeChanged {
                input: later.name.clone(),
                from: type_text(&earlier.ty),
                to: type_text(&later.ty),
            });
        }
    }
    for later in after.inputs {
        if before
            .inputs
            .iter()
            .any(|earlier| earlier.name == later.name)
        {
            continue;
        }
        changes.push(if must_give(later) {
            Change::InputRequired {
                input: later.name.clone(),
            }
        } else {
            Change::InputAdded {
                input: later.name.clone(),
            }
        });
    }
    // Outputs.
    for (earlier, was) in &before.outputs {
        let Some((later, is)) = after
            .outputs
            .iter()
            .find(|(later, _)| later.name == earlier.name)
        else {
            changes.push(Change::OutputGone {
                output: earlier.name.clone(),
            });
            continue;
        };
        if !earlier.may_be_absent && later.may_be_absent {
            changes.push(Change::OutputMayBeAbsent {
                output: later.name.clone(),
            });
        } else if earlier.may_be_absent && !later.may_be_absent {
            changes.push(Change::OutputAlwaysPresent {
                output: later.name.clone(),
            });
        }
        if earlier.ty == later.ty {
            let (added, removed) = (difference(is, was), difference(was, is));
            if !added.is_empty() || !removed.is_empty() {
                changes.push(Change::OutputWordsChanged {
                    output: later.name.clone(),
                    added,
                    removed,
                });
            }
        } else {
            changes.push(Change::OutputTypeChanged {
                output: later.name.clone(),
                from: type_text(&earlier.ty),
                to: type_text(&later.ty),
            });
        }
    }
    for (later, _) in &after.outputs {
        if !before
            .outputs
            .iter()
            .any(|(earlier, _)| earlier.name == later.name)
        {
            changes.push(Change::OutputAdded {
                output: later.name.clone(),
            });
        }
    }
    // What it does.
    if after.effect > before.effect {
        changes.push(Change::EffectRose {
            from: before.effect,
            to: after.effect,
        });
    } else if after.effect < before.effect {
        changes.push(Change::EffectLowered {
            from: before.effect,
            to: after.effect,
        });
    }
    if after.hazard != before.hazard {
        changes.push(if after.hazard == Hazard::None {
            Change::HazardRemoved {
                from: before.hazard,
            }
        } else {
            Change::HazardChanged {
                from: before.hazard,
                to: after.hazard,
            }
        });
    }
    if after.key != before.key {
        changes.push(Change::KeyChanged {
            from: before.key.clone(),
            to: after.key.clone(),
        });
    }
    if after.tool.description != before.tool.description {
        changes.push(Change::DescriptionChanged);
    }
    if changes.is_empty()
        && (after.tool.title != before.tool.title
            || after.tool.input_schema != before.tool.input_schema
            || after.tool.output_schema != before.tool.output_schema
            || after.tool.annotations != before.tool.annotations)
    {
        changes.push(Change::DefinitionChanged);
    }
    changes
}

/// What changed in each tool from `before` to `after`, by the closed list
/// of decision 0439. Every tool of either version is named once.
#[must_use]
pub fn changes_between(before: &CompiledTarget, after: &CompiledTarget) -> VersionChanges {
    let mut tools = Vec::new();
    for earlier in &before.tools {
        let changes = match after.tool(&earlier.name) {
            None => vec![Change::ToolGone],
            Some(later) => match (contract(before, earlier), contract(after, later)) {
                (Some(was), Some(is)) => compared(&was, &is),
                // A tool whose operation or pipeline cannot be found is not
                // a compiled target's; it is said as changed, never as the
                // same.
                _ => vec![Change::DefinitionChanged],
            },
        };
        tools.push(ToolChange::of(earlier.name.clone(), changes));
    }
    for later in &after.tools {
        if before.tool(&later.name).is_none() {
            tools.push(ToolChange::of(later.name.clone(), vec![Change::ToolAdded]));
        }
    }
    tools.sort_by(|a, b| (a.class, &a.tool).cmp(&(b.class, &b.tool)));
    VersionChanges {
        from: before.header.version,
        to: after.header.version,
        tools,
    }
}
