//! The lowering of every verb of the mode `mcp` into `call_tool` steps, and
//! the pack's lint rules. Both read the mapping documents, and one function
//! finds a binding's tool for both, so they cannot disagree.

use a2m_core::{
    EffectClass, FailsAs, FailureClass, FileKindName, IdentityStrategy, LintCode, Mode,
    PackPayload, PackRef, Step, StepArg, StepKind, SurfaceKind, ValueType, Verb, WriteEvidence,
};
use a2m_files::{
    BindingStatement, CompileMode, Diagnostic, Diagnostics, Doc, EnvironmentVariable, JsonSchema,
    Level, LintRule, Located, LowerContext, Lowered, Lowering, Place, Possible, ProgramRun,
    ResolvedBinding, ResolvedView, Sends, Stage, Statement, StepChanges, StepCommits, TargetView,
    VariableValue,
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

/// The refusal of a step whose tool takes the record's key from the key the
/// engine gives the step, when the operation has no key input to give it.
/// A record with no key input is known to the engine only by its place in
/// the run, and that place is not a key the server knows: it must never be
/// sent as an argument. An argument the step states itself is not filled
/// from the key.
fn place_key_problem(
    at: &Place,
    tool: &str,
    entry: &ToolEntry,
    given: &[StepArg],
    key_inputs: usize,
) -> Option<Diagnostic> {
    let unfilled = entry.key_args.iter().enumerate().find(|(at, argument)| {
        *at >= key_inputs && !given.iter().any(|arg| &arg.name == *argument)
    })?;
    Some(Diagnostic::error(
        names::BINDING_INCOMPLETE,
        Stage::Lower,
        at,
        format!(
            "The mapping of the tool `{}` takes the argument `{}` from the record's key, and the operation has only {key_inputs} key input(s). A record with no key input has only its place in the run, which is never sent to a server.",
            codes::shown(tool),
            unfilled.1
        ),
        "Give the operation the key inputs the tool needs, or state the argument in the binding's `args`.",
    ))
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
    let mut certain = call.entry.key.clone();
    if call.entry.result == crate::mapping::RESULT_TEXT {
        certain.push("text".to_owned());
    }
    let payload = PackPayload::encode(&CallPayload {
        surface: call.surface.to_owned(),
        tool: call.tool.to_owned(),
        certain,
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

    /// A `call_tool` sends a read and a write alike, and which it is is the
    /// `effect` the mapping gives the tool it names: the lowering marks it
    /// `commits` exactly when the tool is mapped as a write and the step is
    /// the binding's own act, and the driver refuses a tool mapped as a
    /// write from any step not so marked. No kind always saves.
    fn commits(&self, kind: &StepKind) -> StepCommits {
        if *kind == names::CALL_TOOL {
            StepCommits::AsDeclared
        } else {
            StepCommits::Never
        }
    }

    /// A `call_tool` changes the system exactly when the mapping declares
    /// its tool a write, and the driver refuses such a tool from a step not
    /// marked as a save.
    fn changes(&self, kind: &StepKind) -> StepChanges {
        if *kind == names::CALL_TOOL {
            StepChanges::AsDeclared
        } else {
            StepChanges::Never
        }
    }

    /// `create`, `update`, and `upsert` write. `delete` and `submit` cannot
    /// be taken back. The rest claim nothing more than a read: what a tool
    /// does is the mapping's effect, which the lowering holds the operation
    /// to.
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        match binding.verb.as_str() {
            "create" | "update" | "upsert" => EffectClass::Write,
            "delete" | "submit" => EffectClass::Irreversible,
            _ => EffectClass::Read,
        }
    }

    /// A call to a tool mapped `irreversible`, in an operation with no
    /// identity to find again and a binding that names no `read_back`, has
    /// nothing to read the result back through: the tool's own answer is
    /// the only evidence there can be. (A tool mapped as a write that names
    /// no read tool is refused by the lint `mcp.write_no_readback`, so an
    /// operation declared `irreversible` here is calling a tool mapped so.)
    /// Every other write keeps the read-back, and a binding that names a
    /// `read_back` always keeps it. An operation with an identity and no
    /// read-back is left to the compiler's refusal.
    fn evidence(&self, binding: &ResolvedBinding<'_>, lowered: &Lowered) -> WriteEvidence {
        let operation = binding.operation;
        if operation.effect() == EffectClass::Irreversible
            && operation.strategy() == IdentityStrategy::None
            && binding.field("read_back").is_none()
            && lowered.read_back.is_empty()
            && lowered.act.iter().any(|step| step.commits)
        {
            WriteEvidence::Acknowledged
        } else {
            WriteEvidence::ReadBack
        }
    }

    /// Every output but those the calls of the act and the read-back are
    /// certain to give, as their payloads carry them (decision 0315).
    fn absent_outputs(&self, binding: &ResolvedBinding<'_>, lowered: &Lowered) -> Vec<String> {
        let certain: Vec<String> = lowered
            .act
            .iter()
            .chain(&lowered.read_back)
            .filter_map(|step| step.payload.decode::<CallPayload>().ok())
            .flat_map(|payload| payload.certain)
            .collect();
        binding
            .operation
            .outputs()
            .iter()
            .map(|output| output.name.clone())
            .filter(|name| !certain.contains(name))
            .collect()
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

    /// A binding on a remote server starts nothing and sends only to its
    /// surface's address: the client follows no redirect and uses no
    /// proxy. A binding on a local server starts the surface's program,
    /// by its path and digest, with the variables the surface sets and
    /// passes; where that program sends is not stated, since what a
    /// program reaches is its own (decision 0252). A value the surface
    /// sets is plain text in the target's file: a variable named for a
    /// credential is refused there (`mcp.environment_refused`).
    fn statement(
        &self,
        _binding: &ResolvedBinding<'_>,
        lowered: &Lowered,
        cx: &LowerContext<'_>,
    ) -> Statement {
        let Some(surface) = cx.target().surface(lowered.surface.as_str()) else {
            return Statement::NotStated;
        };
        let Some(program) = surface.doc.get("program") else {
            if surface.address == crate::mapping::LOCAL_ADDRESS {
                return Statement::NotStated;
            }
            return Statement::Stated(BindingStatement {
                sends: Sends::Only {
                    surface: true,
                    addresses: Vec::new(),
                },
                ..BindingStatement::default()
            });
        };
        let Some(path) = program.as_text() else {
            return Statement::NotStated;
        };
        let digests = surface
            .doc
            .text("digest")
            .and_then(parse_digest)
            .into_iter()
            .collect();
        let mut environment: Vec<EnvironmentVariable> = surface
            .doc
            .get("environment")
            .map(Located::entries)
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| {
                Some(EnvironmentVariable {
                    name: entry.key.value.clone(),
                    value: VariableValue::Set(entry.value.as_text()?.to_owned()),
                })
            })
            .collect();
        environment.extend(
            surface
                .doc
                .get("pass_environment")
                .map(Located::items)
                .unwrap_or_default()
                .iter()
                .filter_map(|item| {
                    Some(EnvironmentVariable {
                        name: item.as_text()?.to_owned(),
                        value: VariableValue::Passed,
                    })
                }),
        );
        // A launcher and the entry file it runs are the fixed pair the
        // review shows for a script: the launcher, its fixed arguments, and
        // the file with its digest (decision 0399).
        let run = match (
            surface.doc.text(crate::mapping::ENTRY),
            surface
                .doc
                .text(crate::mapping::ENTRY_DIGEST)
                .and_then(parse_digest),
        ) {
            (Some(entry), Some(script_digest)) => ProgramRun::Script {
                interpreter: path.to_owned(),
                interpreter_digests: digests,
                arguments: surface
                    .doc
                    .get(crate::mapping::LAUNCHER_ARGS)
                    .map(Located::items)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|item| item.as_text().map(str::to_owned))
                    .collect(),
                script: entry.to_owned(),
                script_digest,
            },
            (Some(_), None) => return Statement::NotStated,
            (None, _) => ProgramRun::Program {
                path: path.to_owned(),
                digests,
            },
        };
        Statement::Stated(BindingStatement {
            runs: vec![run],
            environment,
            sends: Sends::NotStated,
        })
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
        let key_inputs = operation.key().len();
        let mut act_args = args_of(binding.field("args"), cx, &mut problems);
        // A read belongs to no commit point, so the engine gives its step
        // no record key: the arguments its mapping names under `key_args`
        // are filled here, from the operation's key inputs in order, where
        // the binding states none of its own. So what lint and the offline
        // test see is what the live call sends (field test 6, finding 38).
        if !entry.writes() {
            for (argument, name) in entry.key_args.iter().zip(operation.key()) {
                if act_args.iter().any(|arg| &arg.name == argument) {
                    continue;
                }
                match cx.expr_at(&format!("input.{name}"), &tool_doc.at, None) {
                    Ok(value) => act_args.push(StepArg {
                        name: argument.clone(),
                        value,
                    }),
                    Err(problem) => problems.push(problem),
                }
            }
        }
        problems.extend(place_key_problem(
            &tool_doc.at,
            tool,
            &entry,
            &act_args,
            key_inputs,
        ));
        let act = step(
            Call {
                surface: &surface_name,
                tool,
                entry: &entry,
                args: act_args,
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
                    // A read-back that runs after the write is given the key
                    // the server assigned, when the tool that wrote reports
                    // one under `key`: then no key input is needed.
                    if entry.key.is_empty() {
                        problems.extend(place_key_problem(
                            &tool_doc.at,
                            read_tool,
                            &read_entry,
                            &args,
                            key_inputs,
                        ));
                    }
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
                    let lookup_args = args_of(lookup_doc.get("args"), cx, &mut problems);
                    problems.extend(place_key_problem(
                        &lookup_tool.at,
                        name,
                        &lookup_entry,
                        &lookup_args,
                        key_inputs,
                    ));
                    lookup.extend(step(
                        Call {
                            surface: &surface_name,
                            tool: name,
                            entry: &lookup_entry,
                            args: lookup_args,
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
    EnvironmentRefused,
    UnencryptedCredentials,
    PackageRunner,
    LauncherForm,
}

impl Rule {
    pub(crate) const ALL: [Self; 10] = [
        Self::ToolUnmapped,
        Self::ToolUnpinned,
        Self::ProgramUnpinned,
        Self::WriteNoReadback,
        Self::DiscoveryBound,
        Self::FreeText,
        Self::EnvironmentRefused,
        Self::UnencryptedCredentials,
        Self::PackageRunner,
        Self::LauncherForm,
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
            Self::EnvironmentRefused => names::ENVIRONMENT_REFUSED,
            Self::UnencryptedCredentials => names::UNENCRYPTED_CREDENTIALS,
            Self::PackageRunner => names::PACKAGE_RUNNER,
            Self::LauncherForm => names::LAUNCHER_FORM,
        }
    }

    fn level(&self, _mode: CompileMode) -> Level {
        match self {
            Self::FreeText | Self::UnencryptedCredentials => Level::Warning,
            _ => Level::Error,
        }
    }

    fn summary(&self) -> &str {
        match self {
            Self::ToolUnmapped => "A binding names a tool that its surface's mapping names.",
            Self::ToolUnpinned => "A mapped tool states the digest of its definition.",
            Self::ProgramUnpinned => {
                "A local server program states its digest, and so does the entry file a launcher runs."
            }
            Self::WriteNoReadback => {
                "A tool mapped as a write names a mapped read tool whose result has fields, or is mapped as irreversible."
            }
            Self::DiscoveryBound => {
                "No binding uses a tool of a mapping whose purpose is discovery."
            }
            Self::FreeText => {
                "A mapped tool that a reviewer found runs free text is never called, and no binding uses it."
            }
            Self::EnvironmentRefused => {
                "A local server program is handed no variable of the engine's, none that makes it load other code, and none named for a credential."
            }
            Self::UnencryptedCredentials => {
                "A surface that sends its credentials over an unencrypted link says so, and is reported on every lint."
            }
            Self::PackageRunner => {
                "A local server program is not a package runner, which fetches the code it runs when it starts."
            }
            Self::LauncherForm => {
                "A surface that names an entry file names a launcher of the provider's table as its program, in that launcher's one form."
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
                .chain(
                    target
                        .surfaces()
                        .into_iter()
                        .filter(|surface| *surface.kind == names::SURFACE_KIND)
                        .filter(|surface| {
                            surface.doc.get(crate::mapping::ENTRY).is_some()
                                && surface
                                    .doc
                                    .text(crate::mapping::ENTRY_DIGEST)
                                    .and_then(parse_digest)
                                    .is_none()
                        })
                        .map(|surface| {
                            problem(
                                &surface.doc.at,
                                format!(
                                    "The surface `{}` names an entry file for its launcher and states no SHA-256 digest for it.",
                                    surface.name
                                ),
                                "Add `entry_digest:` with the SHA-256 digest of the installed entry file that was reviewed.",
                            )
                        }),
                )
                .collect(),
            Self::PackageRunner => target
                .surfaces()
                .into_iter()
                .filter(|surface| *surface.kind == names::SURFACE_KIND)
                .filter_map(|surface| {
                    let program = surface.doc.get("program")?;
                    crate::mapping::is_package_runner(program.as_text()?).then(|| {
                        problem(
                            &program.at,
                            format!(
                                "The surface `{}` starts its server through a package runner, which fetches the code it runs when it starts; nothing the product starts fetches code then.",
                                surface.name
                            ),
                            "Install the server's package, then name the launcher of the provider's table as `program` and the installed entry file as `entry`, each with its digest.",
                        )
                    })
                })
                .collect(),
            Self::LauncherForm => target
                .surfaces()
                .into_iter()
                .filter(|surface| *surface.kind == names::SURFACE_KIND)
                .filter_map(|surface| {
                    let launcher_args: Vec<String> = surface
                        .doc
                        .get(crate::mapping::LAUNCHER_ARGS)
                        .map(Located::items)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|item| item.as_text().map(str::to_owned))
                        .collect();
                    let entry = surface.doc.get(crate::mapping::ENTRY);
                    let program = surface.doc.get("program");
                    let why = match (entry, program.and_then(|program| program.as_text())) {
                        (None, _) if surface.doc.get(crate::mapping::LAUNCHER_ARGS).is_some() => {
                            "gives a launcher's arguments and names no entry file"
                        }
                        (None, _) => return None,
                        (Some(_), None) => "names an entry file and no program to run it",
                        (Some(_), Some(path)) => {
                            match a2m_provider::interpreter_form(path, &launcher_args) {
                                Ok(()) => return None,
                                Err(a2m_provider::InterpreterRefused::NotItsForm(_)) => {
                                    "gives its launcher arguments that are not that launcher's one form"
                                }
                                Err(_) => {
                                    "names an entry file, and its program is not a launcher of the provider's table"
                                }
                            }
                        }
                    };
                    Some(problem(
                        &surface.doc.at,
                        format!("The surface `{}` {why}.", surface.name),
                        "Name a launcher of the table as `program`, with the fixed arguments of its form as `launcher_args`, and the server's installed entry file as `entry`; or name a program that is itself the server and no entry file.",
                    ))
                })
                .collect(),
            Self::UnencryptedCredentials => target
                .surfaces()
                .into_iter()
                .filter(|surface| *surface.kind == names::SURFACE_KIND)
                .filter_map(|surface| {
                    let field = surface.doc.get(crate::mapping::UNENCRYPTED_CREDENTIALS)?;
                    (field.as_text() == Some(crate::mapping::ACCEPTED)).then(|| {
                        problem(
                            &field.at,
                            format!(
                                "The surface `{}` accepts that its credentials cross an unencrypted link to another machine, where anyone on the path can read them.",
                                surface.name
                            ),
                            "Reach the server over `https` and remove `unencrypted_credentials`, once it offers it.",
                        )
                        .as_warning()
                    })
                })
                .collect(),
            Self::EnvironmentRefused => target
                .surfaces()
                .into_iter()
                .filter(|surface| *surface.kind == names::SURFACE_KIND)
                .flat_map(|surface| {
                    let set = surface
                        .doc
                        .get("environment")
                        .map(Located::entries)
                        .unwrap_or_default()
                        .iter()
                        .map(|entry| (entry.key.value.clone(), entry.key.at.clone()));
                    let passed = surface
                        .doc
                        .get("pass_environment")
                        .map(Located::items)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|item| Some((item.as_text()?.to_owned(), item.at.clone())));
                    set.chain(passed).collect::<Vec<_>>()
                })
                .filter_map(|(name, at)| {
                    let why = crate::mapping::refused_variable(&name)?;
                    Some(problem(
                        &at,
                        format!(
                            "The variable `{}` {why}.",
                            codes::shown(&name)
                        ),
                        "Remove it. A credential for a local server program goes through the session keeper, never through its environment.",
                    ))
                })
                .collect(),
            Self::WriteNoReadback => tool_entries(mappings)
                .into_iter()
                .filter(|(surface, _, entry, _)| {
                    // A read tool whose result is its text gives a read-back
                    // no field to compare (field test 6, finding 39).
                    entry.text("effect") == Some("write")
                        && !entry
                            .text("read_back")
                            .and_then(|read| find_tool(mappings, surface, read))
                            .is_some_and(|found| {
                                found.entry.text("effect") == Some("read")
                                    && found.entry.text("result")
                                        != Some(crate::mapping::RESULT_TEXT)
                            })
                })
                .map(|(_, tool, _, at)| {
                    problem(
                        at,
                        format!(
                            "The tool `{}` is mapped as a write and names no mapped read tool that reads back what it wrote with fields: a read tool whose result is `text` gives none.",
                            codes::shown(tool)
                        ),
                        "Add `read_back:` with a tool of the same mapping whose effect is `read` and whose result has fields (`structured`, or `json_text` for a tool that answers with JSON as text), or map the tool as `irreversible`.",
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
