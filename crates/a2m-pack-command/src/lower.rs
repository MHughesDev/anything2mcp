//! From a `cli` binding to steps: `spawn`, `expect`, `parse` for the
//! command the binding names, and the same three for its read-back and its
//! lookup.

use std::collections::BTreeSet;

use a2m_core::{
    CodeEntry, Digest, EffectClass, FailsAs, FileKindName, IdentityStrategy, Mode, PackPayload,
    PackRef, SectionIndex, Step, StepArg, StepKind, SurfaceKind, SurfaceName, Verb, WriteEvidence,
};
use a2m_files::{
    BindingStatement, Diagnostic, Diagnostics, Doc, EnvironmentVariable, JsonSchema, Located,
    LowerContext, Lowered, Lowering, Place, Possible, ProgramRun, ResolvedBinding, Sends, Stage,
    Statement, StepChanges, StepCommits, VariableValue,
};

use crate::codes::{self, EXPECT, FILE_KIND, PARSE, SPAWN, SURFACE_KIND, lint};
use crate::model::{Arg, Command, Effect, Section, StepPayload};

/// The verbs this version of the pack lowers.
pub(crate) const VERBS: &[&str] = &[
    "read", "list", "search", "create", "update", "delete", "call",
];

/// The lowering of every verb of the `cli` mode.
pub(crate) struct CliLowering {
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
}

impl CliLowering {
    pub(crate) fn new() -> Self {
        let other = || {
            JsonSchema::object()
                .required("command", JsonSchema::name())
                .optional("args", JsonSchema::map_of(JsonSchema::expression()))
                .optional("key", JsonSchema::non_empty_list(JsonSchema::name()))
        };
        Self {
            schema: JsonSchema::object()
                .required("command", JsonSchema::name())
                .optional("args", JsonSchema::map_of(JsonSchema::expression()))
                .optional("read_back", other())
                .optional("lookup", other()),
            verbs: VERBS
                .iter()
                .filter_map(|verb| Verb::new(verb).ok())
                .collect(),
            step_kinds: vec![SPAWN, EXPECT, PARSE],
        }
    }
}

fn problem(rule: &'static str, at: &Place, message: String, fix: &str) -> Diagnostic {
    Diagnostic::error(lint(rule), Stage::Lower, at, message, fix)
}

/// The pack's section, decoded, with its place in the plan.
fn section(cx: &LowerContext<'_>) -> Option<(SectionIndex, Section)> {
    let (index, found) = cx.section(&FILE_KIND, &SURFACE_KIND)?;
    found.payload.decode::<Section>().ok().map(|s| (index, s))
}

/// What one run of a command is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// The binding's own command.
    Act,
    /// The read-back of a write.
    ReadBack,
    /// The search for a record by its stamp or by what was written.
    Lookup,
}

struct Run<'a> {
    name: &'a Located<Doc>,
    args: Option<&'a Located<Doc>>,
    key: Option<&'a Located<Doc>>,
    role: Role,
}

/// The three steps of one run of a command.
fn steps(
    run: &Run<'_>,
    index: SectionIndex,
    section: &Section,
    cx: &LowerContext<'_>,
    problems: &mut Diagnostics,
) -> Option<(Vec<Step>, String)> {
    let name = run.name.as_text()?;
    let Some(command) = section.commands.get(name) else {
        problems.push(problem(
            "unknown_command",
            &run.name.at,
            format!("No file under `commands/` defines a command named `{name}`."),
            "Name a command that a `commands/` file defines.",
        ));
        return None;
    };
    if run.role != Role::Act && command.effect != Effect::Read {
        problems.push(problem(
            "read_back_writes",
            &run.name.at,
            format!("The command `{name}` is used to read a record back or look it up, and it is not declared `read`."),
            "Name a command whose `effect` is `read`.",
        ));
        return None;
    }
    let key_params: Vec<String> = run
        .key
        .map(Located::items)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| item.as_text().map(str::to_owned))
        .collect();
    let mut step_args = Vec::new();
    let mut fine = true;
    for (param, declared) in &command.params {
        if key_params.contains(param) {
            continue;
        }
        let given = run.args.and_then(|args| args.get(param));
        let value = match given {
            Some(given) => cx.expr(given, None).map(Some),
            None if cx.operation().input(param).is_some() => cx
                .expr_at(&format!("input.{param}"), &run.name.at, None)
                .map(Some),
            None if declared.required => Err(problem(
                "param_unbound",
                &run.name.at,
                format!(
                    "The param `{param}` of the command `{name}` matches no input, and has no entry under `args` or `key`."
                ),
                "Add an input of that name, an entry under `args`, or the param to `key`.",
            )),
            None => Ok(None),
        };
        match value {
            Ok(Some(value)) => step_args.push(StepArg {
                name: param.clone(),
                value,
            }),
            Ok(None) => {}
            Err(found) => {
                problems.push(found);
                fine = false;
            }
        }
    }
    let known: BTreeSet<&str> = command.params.keys().map(String::as_str).collect();
    let extra = run
        .args
        .map(Located::entries)
        .unwrap_or_default()
        .iter()
        .map(|entry| &entry.key)
        .find(|key| !known.contains(key.value.as_str()));
    if let Some(extra) = extra {
        problems.push(problem(
            "param_unknown",
            &extra.at,
            format!(
                "The binding gives a value for `{}`, which the command `{name}` does not declare.",
                extra.value
            ),
            "Remove the entry, or correct the name.",
        ));
        fine = false;
    }
    if let Some(unknown) = key_params
        .iter()
        .find(|param| !known.contains(param.as_str()))
    {
        problems.push(problem(
            "param_unknown",
            &run.name.at,
            format!("`key` names `{unknown}`, which the command `{name}` does not declare."),
            "Name params of the command, in the order of the record key's parts.",
        ));
        fine = false;
    }
    if !fine {
        return None;
    }
    let writes = command.effect != Effect::Read;
    let payload = PackPayload::encode(&StepPayload::Command {
        key_params,
        one: run.role == Role::Lookup,
    })
    .ok()?;
    let step = |kind: StepKind, row: &CodeEntry, args: Vec<StepArg>, commits, lookup| Step {
        kind,
        refers_to: Some(PackRef {
            section: index,
            entry: name.to_owned(),
        }),
        args,
        timeout_ms: command.limits.timeout_ms,
        fails_as: FailsAs::from(row),
        commits,
        lookup,
        idempotent: false,
        when: None,
        payload: payload.clone(),
    };
    let spawn_fails = if writes {
        &a2m_provider::codes::row::NO_CONFIRMATION
    } else {
        &a2m_provider::codes::row::TIMEOUT
    };
    let surface = section
        .programs
        .get(command.program as usize)
        .map(|program| program.surface.clone())?;
    Some((
        vec![
            step(SPAWN, spawn_fails, step_args, writes, false),
            step(
                EXPECT,
                &codes::row::UNEXPECTED_EXIT_STATUS,
                Vec::new(),
                false,
                !writes,
            ),
            step(
                PARSE,
                &codes::row::OUTPUT_UNPARSED,
                Vec::new(),
                false,
                !writes,
            ),
        ],
        surface,
    ))
}

fn has_stamp(command: &Command) -> bool {
    command
        .args
        .iter()
        .any(|entry| matches!(entry, Arg::Stamp { .. }))
}

impl Lowering for CliLowering {
    fn mode(&self) -> Mode {
        codes::mode()
    }

    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &[]
    }

    fn step_kinds(&self) -> &[StepKind] {
        &self.step_kinds
    }

    /// A `spawn` starts a command that reads and a command that writes
    /// alike, and which it is is the `effect` its `commands/` file declares:
    /// the lowering marks it `commits` exactly for a command that writes,
    /// and the driver refuses a `spawn` whose mark and command disagree.
    /// `expect` and `parse` read how the last start ended and what it
    /// printed, and never save.
    fn commits(&self, kind: &StepKind) -> StepCommits {
        if *kind == SPAWN {
            StepCommits::AsDeclared
        } else {
            StepCommits::Never
        }
    }

    /// A `spawn` changes the system exactly when its command is declared a
    /// write, and the driver refuses a `spawn` whose mark and command
    /// disagree. `expect` and `parse` read how the start ended.
    fn changes(&self, kind: &StepKind) -> StepChanges {
        if *kind == SPAWN {
            StepChanges::AsDeclared
        } else {
            StepChanges::Never
        }
    }

    /// `create` and `update` write; `delete` cannot be taken back and is
    /// `irreversible`. A read verb, and `call`, claim nothing more than a
    /// read: what a `call` does is its command's declared effect, which the
    /// lowering marks and holds the operation to.
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        match binding.verb.as_str() {
            "create" | "update" => EffectClass::Write,
            "delete" => EffectClass::Irreversible,
            _ => EffectClass::Read,
        }
    }

    /// An operation declared `irreversible`, with no identity to find again
    /// and a binding that names no `read_back` command, has nothing to read
    /// the result back through: the program's own end is the only evidence
    /// there can be. Every other write keeps what the default says, and a
    /// binding that names a `read_back` always keeps it. An operation with
    /// an identity and no read-back is left to the compiler's refusal.
    fn evidence(&self, binding: &ResolvedBinding<'_>, lowered: &Lowered) -> WriteEvidence {
        let operation = binding.operation;
        if operation.effect() == EffectClass::Irreversible
            && operation.strategy() == IdentityStrategy::None
            && binding.field("read_back").is_none()
            && lowered.read_back.is_empty()
            && lowered.act.first().is_some_and(|step| step.commits)
        {
            WriteEvidence::Acknowledged
        } else {
            WriteEvidence::ReadBack
        }
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        let named = |field: Option<&Located<Doc>>| {
            field
                .and_then(|field| field.text("command"))
                .map(str::to_owned)
        };
        let lookup = named(binding.field("lookup"));
        match strategy {
            IdentityStrategy::Natural | IdentityStrategy::Composite | IdentityStrategy::None => {
                Possible::Yes
            }
            IdentityStrategy::Find if lookup.is_some() => Possible::Yes,
            IdentityStrategy::Find => Possible::No(
                "the binding names no `lookup` command that lists by what the write set."
                    .to_owned(),
            ),
            // Whether both commands carry the stamp is checked when the
            // binding is lowered, where the commands can be read.
            IdentityStrategy::Stamped if lookup.is_some() => Possible::Yes,
            IdentityStrategy::Stamped => Possible::No(
                "the binding names no `lookup` command that finds a record by its stamp."
                    .to_owned(),
            ),
        }
    }

    /// What the binding starts: for each command it runs (its own, its
    /// read-back's, its lookup's), the program by the address of its
    /// surface and every digest its file pins, or the fixed pair of the
    /// interpreter and the script the target keeps; and each variable its
    /// program is given, set or passed. A value set in a `commands/` file
    /// is plain text the file states: a credential is refused there
    /// (`command.secret_variable`, `command.secret_literal`), and no
    /// credential reaches a program yet, so no variable is a secret. Where
    /// it sends is not stated: what a program reaches is the program's
    /// own (decision 0252).
    fn statement(
        &self,
        binding: &ResolvedBinding<'_>,
        _lowered: &Lowered,
        cx: &LowerContext<'_>,
    ) -> Statement {
        let Some((_, section)) = section(cx) else {
            return Statement::NotStated;
        };
        let commands: Vec<&Command> = [
            binding.field("command"),
            binding
                .field("read_back")
                .and_then(|other| other.get("command")),
            binding
                .field("lookup")
                .and_then(|other| other.get("command")),
        ]
        .into_iter()
        .flatten()
        .filter_map(|name| section.commands.get(name.as_text()?))
        .collect();
        if commands.is_empty() {
            return Statement::NotStated;
        }
        let mut stated = BindingStatement::default();
        let mut programs: Vec<u32> = Vec::new();
        for command in commands {
            let Some(program) = usize::try_from(command.program)
                .ok()
                .and_then(|at| section.programs.get(at))
            else {
                return Statement::NotStated;
            };
            let Some(surface) = cx.target().surface(&program.surface) else {
                return Statement::NotStated;
            };
            let digests: Vec<Digest> = program
                .digests
                .values()
                .flatten()
                .filter_map(|hex| hex.parse::<Digest>().ok())
                .collect();
            let run = match (&program.interpreter, &command.script) {
                (Some(arguments), Some(script)) => {
                    let Ok(script_digest) = script.digest.parse::<Digest>() else {
                        return Statement::NotStated;
                    };
                    ProgramRun::Script {
                        interpreter: surface.address.to_owned(),
                        interpreter_digests: digests,
                        arguments: arguments.clone(),
                        script: script.path.clone(),
                        script_digest,
                    }
                }
                (None, None) => ProgramRun::Program {
                    path: surface.address.to_owned(),
                    digests,
                },
                // An interpreter with no script, or a script with no
                // interpreter, does not compile; nothing is guessed.
                _ => return Statement::NotStated,
            };
            if !stated.runs.contains(&run) {
                stated.runs.push(run);
            }
            if programs.contains(&command.program) {
                continue;
            }
            programs.push(command.program);
            for (name, value) in &program.env_set {
                stated.environment.push(EnvironmentVariable {
                    name: name.clone(),
                    value: VariableValue::Set(value.clone()),
                });
            }
            for name in &program.env_pass {
                stated.environment.push(EnvironmentVariable {
                    name: name.clone(),
                    value: VariableValue::Passed,
                });
            }
        }
        stated.sends = Sends::NotStated;
        Statement::Stated(stated)
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let mut problems = Diagnostics::new();
        let Some((index, section)) = section(cx) else {
            return Err(Diagnostics::from(problem(
                "unknown_command",
                &binding.doc.at,
                "The target has no `commands/` file that compiled, so the binding's command cannot be found.".to_owned(),
                "Add a file under `commands/` that defines the command, and correct what it reports.",
            )));
        };
        let Some(name) = binding.field("command") else {
            return Err(problems);
        };
        let run = |field: &'static str, role| {
            binding.field(field).and_then(|other| {
                Some(Run {
                    name: other.get("command")?,
                    args: other.get("args"),
                    key: other.get("key"),
                    role,
                })
            })
        };
        let act = steps(
            &Run {
                name,
                args: binding.field("args"),
                key: None,
                role: Role::Act,
            },
            index,
            &section,
            cx,
            &mut problems,
        );
        let read_back = run("read_back", Role::ReadBack)
            .and_then(|run| steps(&run, index, &section, cx, &mut problems));
        let lookup = run("lookup", Role::Lookup)
            .and_then(|run| steps(&run, index, &section, cx, &mut problems));

        let command = name.as_text().and_then(|name| section.commands.get(name));
        if let Some(command) = command {
            let verb = binding.verb.as_str();
            let reads = matches!(verb, "read" | "list" | "search");
            let writes = matches!(verb, "create" | "update" | "delete");
            if (reads && command.effect != Effect::Read)
                || (writes && command.effect == Effect::Read)
            {
                problems.push(problem(
                    "effect_mismatch",
                    &name.at,
                    format!(
                        "The verb `{verb}` does not fit the command, whose declared effect is another."
                    ),
                    "Use `read`, `list`, or `search` for a command declared `read`, and `create`, `update`, or `delete` for one that writes; `call` takes either.",
                ));
            }
            if command.effect == Effect::Irreversible
                && cx.operation().effect() != EffectClass::Irreversible
            {
                problems.push(problem(
                    "effect_mismatch",
                    &name.at,
                    "The command is declared `irreversible` and the operation is not.".to_owned(),
                    "Write `effect: irreversible` in the operation.",
                ));
            }
            if cx.operation().strategy() == IdentityStrategy::Stamped {
                let lookup_command = binding
                    .field("lookup")
                    .and_then(|lookup| lookup.text("command"))
                    .and_then(|name| section.commands.get(name));
                if !has_stamp(command) || !lookup_command.is_some_and(has_stamp) {
                    problems.push(problem(
                        "stamp_missing",
                        &name.at,
                        "The operation's identity is `stamped`, and the command or its lookup has no `{flag: ..., stamp: true}` entry.".to_owned(),
                        "Add the entry to both commands, or choose another identity strategy.",
                    ));
                }
            }
        }
        let (Some((act, surface)), true) = (act, problems.is_empty()) else {
            if problems.is_empty() {
                problems.push(problem(
                    "unknown_command",
                    &binding.doc.at,
                    "The binding's command cannot be lowered.".to_owned(),
                    "Name a command that a `commands/` file defines.",
                ));
            }
            return Err(problems);
        };
        let Ok(surface) = SurfaceName::new(&surface) else {
            return Err(problems);
        };
        // The work is the start of the program; seeing how it ended and
        // reading what it printed confirm it. A read's values come from
        // its `parse` step, so a read keeps all three together.
        let writes = act.first().is_some_and(|step| step.commits);
        let (act, confirm) = if writes {
            let mut act = act;
            let confirm = act.split_off(1);
            (act, confirm)
        } else {
            (act, Vec::new())
        };
        Ok(Lowered {
            surface,
            act,
            confirm,
            read_back: read_back.map(|(steps, _)| steps).unwrap_or_default(),
            lookup: lookup.map(|(steps, _)| steps).unwrap_or_default(),
            settle_ms: 0,
            prefer_response: false,
        })
    }
}
