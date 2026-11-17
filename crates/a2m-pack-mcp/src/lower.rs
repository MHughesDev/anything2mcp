//! The lowering of every verb of the mode `mcp` into `call_tool` steps, and
//! the pack's lint rules. Both read the mapping documents, and one function
//! finds a binding's tool for both, so they cannot disagree.

use a2m_core::{
    EffectClass, FailsAs, FailureClass, FileKindName, IdentityStrategy, LintCode, Mode,
    PackPayload, PackRef, Step, StepArg, StepKind, SurfaceKind, ValueType, Verb,
};
use a2m_files::{
    CompileMode, Diagnostic, Diagnostics, Doc, JsonSchema, Level, LintRule, Located, LowerContext,
    Lowered, Lowering, Place, Possible, ResolvedBinding, ResolvedView, Stage, TargetView,
};

use crate::codes;
use crate::json::parse_digest;
use crate::mapping::{CallPayload, ToolEntry, tool_entry};
use crate::names::{self, READ_VERBS};

/// The timeout of a step whose binding and target state none.
const DEFAULT_TIMEOUT_MS: u32 = 30_000;

fn binding_schema() -> JsonSchema {
    let args = || JsonSchema::map_of(JsonSchema::expression());
    JsonSchema::object()
        .required("tool", JsonSchema::text())
        .optional("surface", JsonSchema::surface())
        .optional("args", args())
        .optional("timeout_ms", JsonSchema::int_between(1, 3_600_000))
        .optional("settle_ms", JsonSchema::int_between(0, 600_000))
        .optional("read_back", JsonSchema::object().optional("args", args()))
        .optional(
            "lookup",
            JsonSchema::object()
                .required("tool", JsonSchema::text())
                .optional("args", args()),
        )
}

/// The name of the surface a binding acts on: the one it states, or the
/// target's first surface of the kind `mcp`.
fn surface_of(binding: &Located<Doc>, target: &TargetView<'_>) -> Option<String> {
    match binding.text("surface") {
        Some(named) => Some(named.to_owned()),
        None => target
            .surfaces()
            .into_iter()
            .find(|surface| *surface.kind == names::SURFACE_KIND)
            .map(|surface| surface.name.as_str().to_owned()),
    }
}

/// A tool of a surface's mapping, as the mapping documents state it.
struct Found<'a> {
    entry: &'a Located<Doc>,
    discovery: bool,
}

/// Looks a tool up in the mapping documents of one surface.
fn find_tool<'a>(mappings: &'a [Located<Doc>], surface: &str, tool: &str) -> Option<Found<'a>> {
    let of_surface = || {
        mappings
            .iter()
            .filter(move |doc| doc.text("surface") == Some(surface))
    };
    let discovery = of_surface().any(|doc| doc.text("purpose") == Some("discovery"));
    of_surface()
        .filter_map(|doc| doc.get("tools"))
        .flat_map(|tools| tools.entries())
        .find(|entry| entry.key.value == tool)
        .map(|entry| Found {
            entry: &entry.value,
            discovery,
        })
}

fn unmapped(at: &Place, tool: &str, surface: &str) -> Diagnostic {
    Diagnostic::error(
        names::TOOL_UNMAPPED,
        Stage::Lint,
        at,
        format!(
            "The binding names the tool `{}`, which the mapping of the surface `{surface}` does not name.",
            codes::shown(tool)
        ),
        "Map the tool under `tools` in the surface's file in `mappings/`, with its digest and its effect, or name a tool that is mapped.",
    )
}

fn discovery_bound(at: &Place, tool: &str, surface: &str) -> Diagnostic {
    Diagnostic::error(
        names::DISCOVERY_BOUND,
        Stage::Lint,
        at,
        format!(
            "The binding uses the tool `{}` of the surface `{surface}`, whose mapping has the purpose `discovery`.",
            codes::shown(tool)
        ),
        "Bind operations only to a surface whose mapping has the purpose `tools`.",
    )
}

/// What is wrong with the tool one binding names, if anything: it is not
/// mapped, or its mapping is for discovery only.
fn tool_problem(
    mappings: &[Located<Doc>],
    surface: Option<&str>,
    tool: &Located<Doc>,
) -> Option<Diagnostic> {
    let name = tool.as_text()?;
    let surface = surface.unwrap_or("(none)");
    match find_tool(mappings, surface, name) {
        None => Some(unmapped(&tool.at, name, surface)),
        Some(found) if found.discovery => Some(discovery_bound(&tool.at, name, surface)),
        Some(_) => None,
    }
}

/// The refusal of a binding that uses a tool whose mapping entry says the
/// server runs the text it is given. Such a tool is never called, so no
/// binding may be lowered onto it.
fn runs_free_text(at: &Place, tool: &str) -> Diagnostic {
    Diagnostic::error(
        names::FREE_TEXT,
        Stage::Lower,
        at,
        format!(
            "The binding uses the tool `{}`, whose mapping entry says `free_text: true`: the server runs the text it is given. Such a tool is never called.",
            codes::shown(tool)
        ),
        "Bind the operation to a tool that takes typed arguments. If a person finds that the server does not run the tool's one string, state `free_text: false` in its mapping entry.",
    )
}

/// Whether a value of this type is or holds text. One of a field type's
/// stored codes is not: the kernel admits only the codes.
fn holds_text(ty: &ValueType) -> bool {
    match ty {
        ValueType::Text => true,
        ValueType::List(of) => holds_text(of),
        ValueType::Record(fields) => fields.iter().any(|field| holds_text(&field.ty)),
        _ => false,
    }
}

/// The lowering of the mode `mcp`: every verb becomes one `call_tool` step,
/// with the mapped read tool as its read-back.
pub(crate) struct McpLowering {
    mode: Mode,
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
}

impl McpLowering {
    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            schema: binding_schema(),
            verbs: names::VERBS.to_vec(),
            step_kinds: vec![names::CALL_TOOL],
        }
    }
}

/// What one step is built from.
struct Call<'a> {
    surface: &'a str,
    tool: &'a str,
    entry: &'a ToolEntry,
    args: Vec<StepArg>,
    timeout_ms: u32,
    /// Whether the step is the binding's own act, which commits when the
    /// tool is mapped as a write.
    acts: bool,
    /// Whether the step looks for one record, which may be absent.
    one_record: bool,
}

fn step(call: Call<'_>, cx: &LowerContext<'_>) -> Option<Step> {
    let section = cx.section_index(&names::MAPPING_KIND, &names::SURFACE_KIND)?;
    let payload = PackPayload::encode(&CallPayload {
        surface: call.surface.to_owned(),
        tool: call.tool.to_owned(),
    })
    .ok()?;
    Some(Step {
        kind: names::CALL_TOOL,
        refers_to: Some(PackRef {
            section,
            entry: format!("{}/{}", call.surface, call.tool),
        }),
        args: call.args,
        timeout_ms: call.timeout_ms,
        fails_as: FailsAs {
            class: FailureClass::Drift,
            code: codes::UPSTREAM_TOOL_CHANGED,
        },
        commits: call.acts && call.entry.writes(),
        lookup: call.one_record && !call.entry.writes() && call.entry.can_be_absent(),
        idempotent: false,
        when: None,
        payload,
    })
}

fn args_of(
    given: Option<&Located<Doc>>,
    cx: &LowerContext<'_>,
    problems: &mut Diagnostics,
) -> Vec<StepArg> {
    let mut args = Vec::new();
    for arg in given.map(Located::entries).unwrap_or_default() {
        match cx.expr(&arg.value, None) {
            Ok(value) => args.push(StepArg {
                name: arg.key.value.clone(),
                value,
            }),
            Err(problem) => problems.push(problem),
        }
    }
    args
}

/// The arguments of a read by key: each input that makes the operation's
/// record key, under its own name.
fn key_args(at: &Place, cx: &LowerContext<'_>, problems: &mut Diagnostics) -> Vec<StepArg> {
    let mut args = Vec::new();
    for name in cx.operation().key() {
        match cx.expr_at(&format!("input.{name}"), at, None) {
            Ok(value) => args.push(StepArg {
                name: name.clone(),
                value,
            }),
            Err(problem) => problems.push(problem),
        }
    }
    args
}

fn effect_of(entry: &ToolEntry) -> EffectClass {
    match entry.effect.as_str() {
        "read" => EffectClass::Read,
        "write" => EffectClass::Write,
        _ => EffectClass::Irreversible,
    }
}

impl Lowering for McpLowering {
    fn mode(&self) -> Mode {
        self.mode.clone()
    }

    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    fn default_verb(&self) -> Option<Verb> {
        self.verbs.first().cloned()
    }

    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &[]
    }

    fn step_kinds(&self) -> &[StepKind] {
        &self.step_kinds
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    /// `natural` and `composite` read the record back by its key through
    /// the mapped read tool; `none` needs nothing. `stamped` and `find` need
    /// a `lookup` tool in the binding: a list or search tool that finds the
    /// record.
    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::Natural | IdentityStrategy::Composite | IdentityStrategy::None => {
                Possible::Yes
            }
            IdentityStrategy::Stamped | IdentityStrategy::Find => {
                if binding.field("lookup").is_some() {
                    Possible::Yes
                } else {
                    Possible::No(
                        "the binding names no `lookup` tool that finds the record.".to_owned(),
                    )
                }
            }
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let mut problems = Diagnostics::new();
        let incomplete = |at: &Place, message: &str, fix: &str| {
            Diagnostics::from(Diagnostic::error(
                names::BINDING_INCOMPLETE,
                Stage::Lower,
                at,
                message,
                fix,
            ))
        };
        let target = cx.target();
        let mappings = cx
            .entries(&names::MAPPING_KIND, &names::SURFACE_KIND)
            .unwrap_or_default();
        let (Some(tool_doc), Some(surface_name)) =
            (binding.field("tool"), surface_of(binding.doc, &target))
        else {
            return Err(incomplete(
                &binding.doc.at,
                "The binding names no tool, or the target has no surface of the kind `mcp`.",
                "Add `tool:` with the name of a mapped tool, and a surface of the kind `mcp` to `target.yaml`.",
            ));
        };
        let Some(surface) = target.surface(&surface_name) else {
            return Err(incomplete(
                &binding.doc.at,
                "The binding's surface is not a surface of the target.",
                "Name a surface of the kind `mcp` that `target.yaml` declares.",
            ));
        };
        if let Some(problem) = tool_problem(mappings, Some(&surface_name), tool_doc) {
            return Err(Diagnostics::from(problem));
        }
        let tool = tool_doc.as_text().unwrap_or_default();
        let Some(found) = find_tool(mappings, &surface_name, tool) else {
            return Err(Diagnostics::from(unmapped(
                &tool_doc.at,
                tool,
                &surface_name,
            )));
        };
        let entry = tool_entry(found.entry);
        let operation = cx.operation();

        // The effect is the developer's judgment in the mapping; an
        // operation may not state less than the tool it calls.
        let reading = READ_VERBS.contains(&binding.verb.as_str());
        if effect_of(&entry) > operation.effect() || (reading && entry.writes()) {
            problems.push(Diagnostic::error(
                names::EFFECT_UNDERSTATED,
                Stage::Lower,
                &tool_doc.at,
                format!(
                    "The tool `{}` is mapped as `{}`, which is more than the operation's effect `{}` or the verb `{}` allows.",
                    codes::shown(tool),
                    entry.effect,
                    operation.effect(),
                    binding.verb
                ),
                "State the operation's effect as the mapping judges the tool, and use a verb that writes for a tool that writes.",
            ));
        }
        if entry.free_text == Some(true) {
            problems.push(runs_free_text(&tool_doc.at, tool));
        }
        // Every value of the mode comes from the upstream server, whichever
        // tool of the binding fills it: the act, the read-back, or the
        // lookup. So every output that is or holds text must arrive under a
        // field type that marks it untrusted.
        for output in operation
            .doc()
            .get("outputs")
            .map(Located::entries)
            .unwrap_or_default()
        {
            let text = operation
                .outputs()
                .iter()
                .find(|plan| plan.name == output.key.value)
                .is_some_and(|plan| holds_text(&plan.ty));
            let marked = output
                .value
                .text("type")
                .and_then(|name| target.field_type(name))
                .is_some_and(|field_type| field_type.plan.untrusted);
            if text && !marked {
                problems.push(Diagnostic::error(
                    names::UNTRUSTED_UNMARKED,
                    Stage::Lower,
                    &output.key.at,
                    format!(
                        "The output `{}` is text, and text an upstream server returns is untrusted: its field type is not marked untrusted.",
                        output.key.value
                    ),
                    "Give the output a field type with `untrusted: true`, or a type that is not text, such as a choice with its codes.",
                ));
            }
        }

        let timeout_ms = binding
            .field("timeout_ms")
            .and_then(|timeout| timeout.as_int())
            .and_then(|timeout| u32::try_from(timeout).ok())
            .or_else(|| target.default_timeout_ms("step"))
            .unwrap_or(DEFAULT_TIMEOUT_MS);
        let act = step(
            Call {
                surface: &surface_name,
                tool,
                entry: &entry,
                args: args_of(binding.field("args"), cx, &mut problems),
                timeout_ms,
                acts: true,
                one_record: binding.verb.as_str() == "read",
            },
            cx,
        );

        // The read-back: the mapped read tool, called with the record's
        // key unless the binding states other arguments.
        let mut read_back = Vec::new();
        if entry.writes()
            && let Some(read_tool) = entry.read_back.as_deref()
        {
            match find_tool(mappings, &surface_name, read_tool).map(|found| tool_entry(found.entry))
            {
                Some(read_entry) if !read_entry.writes() => {
                    if read_entry.free_text == Some(true) {
                        problems.push(runs_free_text(&tool_doc.at, read_tool));
                    }
                    // The record's key reaches the read tool through the
                    // arguments its mapping names under `key_args`, or
                    // else through the operation's key inputs by name.
                    let args = match binding.field("read_back").and_then(|read| read.get("args")) {
                        Some(given) => args_of(Some(given), cx, &mut problems),
                        None if read_entry.key_args.is_empty() => {
                            key_args(&tool_doc.at, cx, &mut problems)
                        }
                        None => Vec::new(),
                    };
                    read_back.extend(step(
                        Call {
                            surface: &surface_name,
                            tool: read_tool,
                            entry: &read_entry,
                            args,
                            timeout_ms,
                            acts: false,
                            one_record: true,
                        },
                        cx,
                    ));
                }
                // The lint rule `mcp.write_no_readback` reports this at the
                // mapping; the binding is lowered without a read-back.
                _ => {}
            }
        }

        // The lookup: a list or search tool that finds the record by its
        // stamp or by what the binding states.
        let mut lookup = Vec::new();
        if let Some(lookup_doc) = binding.field("lookup")
            && let Some(lookup_tool) = lookup_doc.get("tool")
        {
            if let Some(problem) = tool_problem(mappings, Some(&surface_name), lookup_tool) {
                problems.push(problem);
            } else if let Some(name) = lookup_tool.as_text()
                && let Some(found) = find_tool(mappings, &surface_name, name)
            {
                let lookup_entry = tool_entry(found.entry);
                if lookup_entry.free_text == Some(true) {
                    problems.push(runs_free_text(&lookup_tool.at, name));
                }
                if lookup_entry.writes() || !lookup_entry.can_be_absent() {
                    problems.push(Diagnostic::error(
                        names::BINDING_INCOMPLETE,
                        Stage::Lower,
                        &lookup_tool.at,
                        format!(
                            "The lookup tool `{}` is not mapped as a read that says how an absent record shows.",
                            codes::shown(name)
                        ),
                        "Map the lookup tool with `effect: read` and with `records` or `absent_error`.",
                    ));
                } else {
                    lookup.extend(step(
                        Call {
                            surface: &surface_name,
                            tool: name,
                            entry: &lookup_entry,
                            args: args_of(lookup_doc.get("args"), cx, &mut problems),
                            timeout_ms,
                            acts: false,
                            one_record: true,
                        },
                        cx,
                    ));
                }
            }
        }
        if operation.strategy() == IdentityStrategy::Stamped && entry.stamp.is_none() {
            problems.push(Diagnostic::error(
                names::BINDING_INCOMPLETE,
                Stage::Lower,
                &tool_doc.at,
                format!(
                    "The operation's identity is `stamped`, and the mapping of the tool `{}` names no argument that carries the stamp.",
                    codes::shown(tool)
                ),
                "Add `stamp:` to the tool's mapping entry, with the argument that takes the stamp.",
            ));
        }

        let (Some(act), true) = (act, problems.is_empty()) else {
            if problems.is_empty() {
                return Err(incomplete(
                    &binding.doc.at,
                    "The binding's tool cannot be lowered.",
                    "Check that the surface has a file in `mappings/`.",
                ));
            }
            return Err(problems);
        };
        Ok(Lowered {
            surface: surface.name.clone(),
            act: vec![act],
            confirm: Vec::new(),
            read_back,
            lookup,
            settle_ms: binding
                .field("settle_ms")
                .and_then(|settle| settle.as_int())
                .and_then(|settle| u32::try_from(settle).ok())
                .unwrap_or(0),
            prefer_response: false,
        })
    }
}

/// The pack's lint rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
    ToolUnmapped,
    ToolUnpinned,
    ProgramUnpinned,
    WriteNoReadback,
    DiscoveryBound,
    FreeText,
}

impl Rule {
    pub(crate) const ALL: [Self; 6] = [
        Self::ToolUnmapped,
        Self::ToolUnpinned,
        Self::ProgramUnpinned,
        Self::WriteNoReadback,
        Self::DiscoveryBound,
        Self::FreeText,
    ];
}

/// Every tool entry of every mapping document, with its surface.
fn tool_entries(mappings: &[Located<Doc>]) -> Vec<(&str, &str, &Located<Doc>, &Place)> {
    mappings
        .iter()
        .flat_map(|doc| {
            let surface = doc.text("surface").unwrap_or_default();
            doc.get("tools")
                .map(Located::entries)
                .unwrap_or_default()
                .iter()
                .map(move |entry| {
                    (
                        surface,
                        entry.key.value.as_str(),
                        &entry.value,
                        &entry.key.at,
                    )
                })
        })
        .collect()
}

impl LintRule for Rule {
    fn code(&self) -> LintCode {
        match self {
            Self::ToolUnmapped => names::TOOL_UNMAPPED,
            Self::ToolUnpinned => names::TOOL_UNPINNED,
            Self::ProgramUnpinned => names::PROGRAM_UNPINNED,
            Self::WriteNoReadback => names::WRITE_NO_READBACK,
            Self::DiscoveryBound => names::DISCOVERY_BOUND,
            Self::FreeText => names::FREE_TEXT,
        }
    }

    fn level(&self, _mode: CompileMode) -> Level {
        match self {
            Self::FreeText => Level::Warning,
            _ => Level::Error,
        }
    }

    fn summary(&self) -> &str {
        match self {
            Self::ToolUnmapped => "A binding names a tool that its surface's mapping names.",
            Self::ToolUnpinned => "A mapped tool states the digest of its definition.",
            Self::ProgramUnpinned => "A local server program states its digest.",
            Self::WriteNoReadback => {
                "A tool mapped as a write names a mapped read tool, or is mapped as irreversible."
            }
            Self::DiscoveryBound => {
                "No binding uses a tool of a mapping whose purpose is discovery."
            }
            Self::FreeText => {
                "A mapped tool that a reviewer found runs free text is never called, and no binding uses it."
            }
        }
    }

    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic> {
        let mappings = view.entries(&names::MAPPING_KIND, &names::SURFACE_KIND);
        let target = view.target();
        let code = self.code();
        let problem = |at: &Place, message: String, fix: &str| {
            Diagnostic::error(code.clone(), Stage::Lint, at, message, fix)
        };
        match self {
            Self::ToolUnmapped | Self::DiscoveryBound => view
                .operations()
                .iter()
                .flat_map(|operation| {
                    operation
                        .doc()
                        .get("bindings")
                        .map(Located::items)
                        .unwrap_or_default()
                })
                .filter(|binding| binding.text("mode") == Some("mcp"))
                .flat_map(|binding| {
                    let surface = surface_of(binding, &target);
                    [
                        binding.get("tool"),
                        binding.get("lookup").and_then(|lookup| lookup.get("tool")),
                    ]
                    .into_iter()
                    .flatten()
                    .filter_map(move |tool| tool_problem(mappings, surface.as_deref(), tool))
                    .collect::<Vec<_>>()
                })
                .filter(|found| found.code == code)
                .collect(),
            Self::ToolUnpinned => tool_entries(mappings)
                .into_iter()
                .filter(|(_, _, entry, _)| {
                    entry.text("digest").and_then(parse_digest).is_none()
                })
                .map(|(_, tool, _, at)| {
                    problem(
                        at,
                        format!(
                            "The mapped tool `{}` states no digest of its definition, or one that is not a SHA-256 digest.",
                            codes::shown(tool)
                        ),
                        "Run discovery on the surface and write the digest it reports under `digest:`.",
                    )
                })
                .collect(),
            Self::ProgramUnpinned => target
                .surfaces()
                .into_iter()
                .filter(|surface| *surface.kind == names::SURFACE_KIND)
                .filter(|surface| {
                    surface.doc.get("program").is_some()
                        && surface.doc.text("digest").and_then(parse_digest).is_none()
                })
                .map(|surface| {
                    problem(
                        &surface.doc.at,
                        format!(
                            "The surface `{}` names a local server program and states no SHA-256 digest for it.",
                            surface.name
                        ),
                        "Add `digest:` with the SHA-256 digest of the program that was reviewed.",
                    )
                })
                .collect(),
            Self::WriteNoReadback => tool_entries(mappings)
                .into_iter()
                .filter(|(surface, _, entry, _)| {
                    entry.text("effect") == Some("write")
                        && !entry
                            .text("read_back")
                            .and_then(|read| find_tool(mappings, surface, read))
                            .is_some_and(|found| found.entry.text("effect") == Some("read"))
                })
                .map(|(_, tool, _, at)| {
                    problem(
                        at,
                        format!(
                            "The tool `{}` is mapped as a write and names no mapped read tool that reads back what it wrote.",
                            codes::shown(tool)
                        ),
                        "Add `read_back:` with a tool of the same mapping whose effect is `read`, or map the tool as `irreversible`.",
                    )
                })
                .collect(),
            Self::FreeText => tool_entries(mappings)
                .into_iter()
                .filter(|(_, _, entry, _)| {
                    entry.get("free_text").and_then(|flag| flag.as_bool()) == Some(true)
                })
                .map(|(_, tool, _, at)| {
                    problem(
                        at,
                        format!(
                            "The mapping says of the tool `{}` that the server runs the text it is given, as a command, a query, or a prompt. The tool is never called, and no binding may use it.",
                            codes::shown(tool)
                        ),
                        "Take the tool out of the mapping, or leave it as the record that it was reviewed and refused. State `free_text: false` only if a person finds that the server does not run the tool's one string.",
                    )
                })
                .collect(),
        }
    }
}
