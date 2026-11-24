//! What every face is generated from: for each face of each action, its
//! complete description as data.
//!
//! The product crate builds its command tree from [`command_faces`];
//! `a2m-mcp` answers a tool list from [`mcp_tools`]; `a2m-api` builds its
//! router and its OpenAPI document from [`rest_routes`]; the SDK generator
//! reads [`sdk_methods`]. None of them writes a face by hand, and none
//! branches on what a response holds: [`McpResultFace::of`] is the one
//! rendering of a response for the MCP face.

use a2m_core::EffectClass;
use serde_json::{Value as Json, json};

use crate::def::{ActionDef, BuiltInToolset, Effect, FieldType, RunsIn};
use crate::dispatch::Response;
use crate::registry::ActionRegistry;

/// The program's name, as the first word of every command.
pub const PROGRAM: &str = "anything2mcp";
/// What the name of every built-in MCP tool starts with.
pub const TOOL_PREFIX: &str = "a2m_";
/// What the path of every REST route starts with.
pub const REST_PREFIX: &str = "/api/v1";

/// One argument of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandArg {
    /// The input field it fills.
    pub field: String,
    /// The flag, such as `--target`. Empty for the positional argument.
    pub flag: String,
    /// Whether it is given without a flag.
    pub positional: bool,
    /// Whether it must be given.
    pub required: bool,
    /// Its type, in a word: `text`, `number`, `flag`, `word`, `list`,
    /// `map`, or `value`. A `flag` takes no value; a `list` may be repeated;
    /// a `map` and a `value` take JSON.
    pub ty: &'static str,
    /// The words allowed, for a `word`.
    pub choices: Vec<String>,
    /// What it is.
    pub help: String,
    /// For a `flag`: the field that standard input fills when the flag is
    /// given. The command then reads standard input to its end, removes one
    /// line ending from the end, sends the text as that field, and does not
    /// send the flag. `None` for every other argument.
    pub stdin_for: Option<String>,
}

/// The command face of one action: `anything2mcp <noun> <verb>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandFace {
    /// The action.
    pub action: String,
    /// The first word after the program's name.
    pub noun: String,
    /// The second.
    pub verb: String,
    /// The usage line.
    pub usage: String,
    /// The help text.
    pub help: String,
    /// The arguments, the positional one first.
    pub args: Vec<CommandArg>,
    /// Whether the client end carries the action out itself.
    pub client: bool,
    /// Whether the action returns a `run_id` at once.
    pub long_running: bool,
}

/// The command face of one action.
#[must_use]
pub fn command_faces_of(def: &ActionDef) -> CommandFace {
    let mut args: Vec<CommandArg> = def
        .input
        .fields
        .iter()
        // The command line carries an idempotency key in its own flag for
        // every command, and the dispatcher takes the key from beside the
        // input as well as from inside it. A second flag of the same name
        // would be taken for the first.
        .filter(|field| field.name != crate::dispatch::KEY_FIELD)
        .map(|field| CommandArg {
            field: field.name.clone(),
            flag: if field.positional {
                String::new()
            } else {
                format!("--{}", field.name.replace('_', "-"))
            },
            positional: field.positional,
            required: field.required,
            ty: field.ty.as_str(),
            choices: match &field.ty {
                FieldType::Word(words) => words.iter().map(|word| (*word).to_owned()).collect(),
                _ => Vec::new(),
            },
            help: field.help.clone(),
            stdin_for: field.stdin_for.clone(),
        })
        .collect();
    args.sort_by_key(|arg| !arg.positional);
    let mut usage = format!("{PROGRAM} {} {}", def.name.noun(), def.name.verb());
    for arg in &args {
        let shown = if arg.positional {
            format!("<{}>", arg.field)
        } else if arg.ty == "flag" {
            arg.flag.clone()
        } else {
            format!("{} <{}>", arg.flag, arg.ty)
        };
        if arg.required {
            usage.push_str(&format!(" {shown}"));
        } else {
            usage.push_str(&format!(" [{shown}]"));
        }
    }
    CommandFace {
        action: def.name.to_string(),
        noun: def.name.noun().to_owned(),
        verb: def.name.verb().to_owned(),
        usage,
        help: def.summary.clone(),
        args,
        client: def.runs_in == RunsIn::Client,
        long_running: def.long_running,
    }
}

/// The command face of every action that has one, in catalog order.
#[must_use]
pub fn command_faces(registry: &ActionRegistry) -> Vec<CommandFace> {
    registry
        .iter()
        .filter(|def| def.faces.command)
        .map(command_faces_of)
        .collect()
}

/// The annotations of an MCP tool, set from the action's effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolAnnotationsFace {
    /// The tool changes nothing.
    pub read_only: bool,
    /// The tool may do what cannot be undone.
    pub destructive: bool,
    /// Calling it again with the same input changes nothing more.
    pub idempotent: bool,
    /// The tool reaches a system outside the engine.
    pub open_world: bool,
}

/// The MCP face of one action: the tool `a2m_<noun>_<verb>`.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolFace {
    /// The tool's name.
    pub name: String,
    /// The action it is.
    pub action: String,
    /// A title for people.
    pub title: String,
    /// The description: the action's summary, whose first 240 characters
    /// are enough to choose the tool, then how a result arrives.
    pub description: String,
    /// The input schema, as a JSON Schema document.
    pub input_schema: Json,
    /// The output schema, as a JSON Schema document. Published; not sent in
    /// a tool list unless the client asks.
    pub output_schema: Json,
    /// The annotations.
    pub annotations: ToolAnnotationsFace,
}

/// How every built-in tool's result arrives, said once in each description.
const RESULT_NOTE: &str = "A success carries structured content that fits the output schema, and the same JSON as text. A failure carries one JSON object as text: class, code, message, fix, and retry_safe.";

/// The name of the MCP tool of an action.
#[must_use]
pub fn tool_name(def: &ActionDef) -> String {
    format!("{TOOL_PREFIX}{}_{}", def.name.noun(), def.name.verb())
}

/// The MCP face of one action.
#[must_use]
pub fn mcp_tool(def: &ActionDef) -> McpToolFace {
    let effect = def.fixed_effect();
    McpToolFace {
        name: tool_name(def),
        action: def.name.to_string(),
        title: format!("{} {}", def.name.noun(), def.name.verb()),
        description: format!("{} {RESULT_NOTE}", def.summary),
        input_schema: def.input.schema.to_json(),
        output_schema: def.output.to_json(),
        annotations: ToolAnnotationsFace {
            read_only: effect == Some(EffectClass::Read),
            destructive: effect == Some(EffectClass::Irreversible) || def.effect == Effect::OfTool,
            idempotent: effect == Some(EffectClass::Read),
            open_world: def.effect == Effect::OfTool || def.shape == crate::def::Shape::Observe,
        },
    }
}

/// The tools a built-in toolset lists up front, in catalog order. Every
/// other action is reached through `a2m_action_run`.
#[must_use]
pub fn mcp_tools(registry: &ActionRegistry, toolset: BuiltInToolset) -> Vec<McpToolFace> {
    registry
        .toolset(toolset)
        .into_iter()
        .filter(|def| def.faces.mcp)
        .map(mcp_tool)
        .collect()
}

impl ActionRegistry {
    /// The action whose MCP tool has this name. A tool name that is not an
    /// action's is a target's tool, which `tool.call` runs.
    #[must_use]
    pub fn by_tool_name(&self, name: &str) -> Option<&ActionDef> {
        name.strip_prefix(TOOL_PREFIX)?;
        self.iter()
            .find(|def| def.faces.mcp && tool_name(def) == name)
    }
}

/// A response as the MCP face returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct McpResultFace {
    /// Whether the result is marked as an error.
    pub is_error: bool,
    /// The structured content: the result of a success, which fits the
    /// tool's output schema. None for a failure.
    pub structured: Option<Json>,
    /// The one text item: the same JSON as the structured content, or, for
    /// a failure, its class, code, message, fix, and whether a retry is
    /// safe, as one compact JSON object.
    pub text: String,
}

impl McpResultFace {
    /// The rendering of a response. It reads `ok`, `result`, and `error`,
    /// and branches on nothing else.
    #[must_use]
    pub fn of(response: &Response) -> Self {
        Self::rendered(response, true)
    }

    /// The same rendering for a caller on `toolset`: where the toolset
    /// does not list the tool of the following action, `next` names
    /// `a2m_action_run` with the action as its argument, so that a caller
    /// is never pointed at a tool its list does not hold.
    #[must_use]
    pub fn of_in(response: &Response, registry: &ActionRegistry, toolset: BuiltInToolset) -> Self {
        let listed = response.next.as_ref().is_none_or(|next| {
            registry
                .toolset(toolset)
                .iter()
                .any(|def| def.name.as_str() == next.action)
        });
        Self::rendered(response, listed)
    }

    fn rendered(response: &Response, next_is_listed: bool) -> Self {
        match (&response.error, &response.result) {
            (Some(error), result) => {
                // A run that failed names itself, and what the system said
                // is shown in a field of its own, apart from the sentence
                // (decision 0176): the result is dropped, so both are
                // carried into the error's object.
                let mut shown = error.to_json();
                if let Some(result) = result {
                    if let Some(run) = result.get("run_id") {
                        shown["run_id"] = run.clone();
                    }
                    if let Some(said) = result
                        .get("failure")
                        .and_then(|failure| failure.get("said"))
                    {
                        shown["said"] = said.clone();
                    }
                }
                Self {
                    is_error: true,
                    structured: None,
                    text: shown.to_string(),
                }
            }
            (None, Some(result)) => {
                // The most useful following call travels inside the result
                // (spec section 221): every output schema admits `next`.
                let mut result = result.clone();
                if let (Some(next), Json::Object(map)) = (&response.next, &mut result)
                    && !map.contains_key("next")
                {
                    map.insert(
                        "next".to_owned(),
                        if next_is_listed {
                            next.to_result_json()
                        } else {
                            next.through_action_run()
                        },
                    );
                }
                Self {
                    is_error: false,
                    text: result.to_string(),
                    structured: Some(result),
                }
            }
            (None, None) => Self {
                is_error: false,
                structured: Some(json!({})),
                text: "{}".to_owned(),
            },
        }
    }
}

/// One REST route of an action.
#[derive(Debug, Clone, PartialEq)]
pub struct RestRouteFace {
    /// `GET` or `POST`.
    pub method: &'static str,
    /// The full path. `{field}` stands for an input field taken from the
    /// path.
    pub path: String,
    /// The action it dispatches.
    pub action: String,
    /// Whether this is a resource-shaped alias beside the action's own
    /// route.
    pub alias: bool,
    /// The input fields taken from the path. The rest come from the query
    /// of a `GET`, or the JSON body of a `POST`.
    pub path_fields: Vec<String>,
    /// One line for the API description.
    pub summary: String,
    /// The input schema, as a JSON Schema document.
    pub input_schema: Json,
    /// The output schema, as a JSON Schema document.
    pub output_schema: Json,
}

/// Every REST route, in catalog order: the action's own route, then its
/// alias when it declares one.
#[must_use]
pub fn rest_routes(registry: &ActionRegistry) -> Vec<RestRouteFace> {
    let mut routes = Vec::new();
    for def in registry.iter().filter(|def| def.faces.rest) {
        let face = |method, path: String, alias, path_fields| RestRouteFace {
            method,
            path,
            action: def.name.to_string(),
            alias,
            path_fields,
            summary: def.summary.clone(),
            input_schema: def.input.schema.to_json(),
            output_schema: def.output.to_json(),
        };
        routes.push(face(
            "POST",
            format!("{REST_PREFIX}/actions/{}", def.name),
            false,
            Vec::new(),
        ));
        if let Some(alias) = def.rest_alias {
            let fields = alias
                .path
                .split('/')
                .filter_map(|part| part.strip_prefix('{')?.strip_suffix('}'))
                .map(str::to_owned)
                .collect();
            routes.push(face(
                alias.method,
                format!("{REST_PREFIX}{}", alias.path),
                true,
                fields,
            ));
        }
    }
    routes
}

/// The SDK face of one action: `client.<noun>.<verb>()`.
#[derive(Debug, Clone, PartialEq)]
pub struct SdkMethodFace {
    /// The action.
    pub action: String,
    /// The name space on the client: the noun.
    pub namespace: String,
    /// The method: the verb.
    pub method: String,
    /// The documentation line.
    pub doc: String,
    /// The parameters, as the command face has them.
    pub params: Vec<CommandArg>,
    /// The input schema, as a JSON Schema document.
    pub input_schema: Json,
    /// The output schema, as a JSON Schema document.
    pub output_schema: Json,
}

/// The SDK method of every action that has one, in catalog order.
#[must_use]
pub fn sdk_methods(registry: &ActionRegistry) -> Vec<SdkMethodFace> {
    registry
        .iter()
        .filter(|def| def.faces.sdk)
        .map(|def| SdkMethodFace {
            action: def.name.to_string(),
            namespace: def.name.noun().to_owned(),
            method: def.name.verb().to_owned(),
            doc: def.summary.clone(),
            params: command_faces_of(def).args,
            input_schema: def.input.schema.to_json(),
            output_schema: def.output.to_json(),
        })
        .collect()
}

/// One step of a shortcut: an action, and the input field the shortcut's
/// own argument fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutStep {
    /// The action.
    pub action: &'static str,
    /// The field of the action that takes the shortcut's argument.
    pub argument: Option<&'static str>,
}

/// A flag a shortcut takes beside its one argument: the field of one of its
/// steps that the flag's value fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutOption {
    /// The flag, without its dashes.
    pub flag: &'static str,
    /// The step's action that takes the value.
    pub action: &'static str,
    /// The field of that action.
    pub field: &'static str,
    /// When given, the field is a map and the value is put in it under
    /// this key, as `spec.import` hands its file under `description`.
    pub key: Option<&'static str>,
    /// What the flag takes, in a sentence.
    pub does: &'static str,
}

/// A single-word command for people: a fixed list of actions, as data. It
/// adds no behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    /// The word.
    pub word: &'static str,
    /// What it does, in a sentence.
    pub does: &'static str,
    /// The actions, in order. Empty for `init`, which has no action beneath
    /// it yet.
    pub steps: &'static [ShortcutStep],
    /// The flags it takes beside its one argument, each filling one field
    /// of one step. Without them, a shortcut of several actions takes no
    /// flag.
    pub options: &'static [ShortcutOption],
}

/// What a shortcut does on this engine: the steps to carry out, and where it
/// stops when a step is not built yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutRun {
    /// The steps to carry out, in order: the shortcut's steps up to the
    /// first whose action is not built.
    pub steps: Vec<ShortcutStep>,
    /// The first action that is not built. The shortcut stops before it and
    /// carries out no step after it.
    pub stops_before: Option<&'static str>,
    /// What to print when the shortcut stops early: what was done, what it
    /// stopped before, and what was left undone. `None` when every step is
    /// built.
    pub note: Option<String>,
}

impl Shortcut {
    /// What this shortcut does with the actions `registry` holds. A
    /// shortcut never begins a step that is not built: it stops before it,
    /// whatever was done already stays done, and the note says so.
    #[must_use]
    pub fn on(&self, registry: &ActionRegistry) -> ShortcutRun {
        let built = |step: &&ShortcutStep| registry.find(step.action).is_some_and(ActionDef::built);
        let steps: Vec<ShortcutStep> = self.steps.iter().take_while(built).copied().collect();
        let rest = &self.steps[steps.len()..];
        let Some(first) = rest.first() else {
            return ShortcutRun {
                steps,
                stops_before: None,
                note: None,
            };
        };
        let names = |steps: &[ShortcutStep]| {
            steps
                .iter()
                .map(|step| format!("`{}`", step.action))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let note = if steps.is_empty() {
            format!(
                "`{}` does nothing yet: its first step, `{}`, is not built. Call `action.search` to see which actions are built.",
                self.word, first.action
            )
        } else {
            format!(
                "`{}` stopped before `{}`, which is not built yet. Done: {}. Not done: {}.",
                self.word,
                first.action,
                names(&steps),
                names(rest)
            )
        };
        ShortcutRun {
            steps,
            stops_before: Some(first.action),
            note: Some(note),
        }
    }
}

const fn step(action: &'static str, argument: Option<&'static str>) -> ShortcutStep {
    ShortcutStep { action, argument }
}

/// Every shortcut, in the order of the action catalog.
pub const SHORTCUTS: &[Shortcut] = &[
    Shortcut {
        word: "init",
        does: "Create a project and the guidance file; choose a profile.",
        steps: &[],
        options: &[],
    },
    Shortcut {
        word: "add",
        does: "Register a target, scout it, and show the report, which says which read to try next and asks for a sign-in only where a read waits for one.",
        steps: &[
            step("target.add", Some("name")),
            step("scout.run", Some("target")),
            step("scout.report", Some("target")),
        ],
        options: &[
            ShortcutOption {
                flag: "address",
                action: "target.add",
                field: "address",
                key: None,
                does: "Where the system is reached; given with `--kind`.",
            },
            ShortcutOption {
                flag: "kind",
                action: "target.add",
                field: "kind",
                key: None,
                does: "The kind of the surface at `--address`.",
            },
            ShortcutOption {
                flag: "description",
                action: "scout.run",
                field: "files",
                key: Some("description"),
                does: "A description the system publishes about itself, as a file of the project's `sources` folder, such as `sources/example-openapi.json`; the scout drafts from it as `spec.import` does.",
            },
        ],
    },
    Shortcut {
        word: "learn",
        does: "Record a session and infer endpoints from it.",
        steps: &[
            step("capture.start", Some("target")),
            step("capture.stop", Some("target")),
            step("endpoints.infer", Some("target")),
        ],
        options: &[],
    },
    Shortcut {
        word: "lint",
        does: "Lint and compile a target's drafts.",
        steps: &[step("draft.lint", Some("target"))],
        options: &[],
    },
    Shortcut {
        word: "try",
        does: "Run a draft binding in the sandbox.",
        steps: &[step("draft.try", Some("target"))],
        options: &[],
    },
    Shortcut {
        word: "test",
        does: "Run the publish gates.",
        steps: &[step("test.run", Some("target"))],
        options: &[],
    },
    Shortcut {
        word: "publish",
        does: "Submit a version for review, then wait for a person.",
        steps: &[step("publish.request", Some("target"))],
        options: &[],
    },
    Shortcut {
        word: "status",
        does: "The target's build state and the next action.",
        steps: &[step("target.status", Some("target"))],
        options: &[],
    },
    Shortcut {
        word: "call",
        does: "Call an operation.",
        steps: &[step("tool.call", Some("tool"))],
        options: &[],
    },
    Shortcut {
        word: "run",
        does: "Run a pipeline.",
        steps: &[step("tool.call", Some("tool"))],
        options: &[],
    },
    Shortcut {
        word: "diff",
        does: "Compare two versions.",
        steps: &[step("version.diff", Some("target"))],
        options: &[],
    },
    Shortcut {
        word: "bundle",
        does: "Pack a version into a signed file.",
        steps: &[step("version.bundle", Some("target"))],
        options: &[],
    },
    Shortcut {
        word: "rollback",
        does: "Make an earlier version current.",
        steps: &[step("version.rollback", Some("target"))],
        options: &[],
    },
    Shortcut {
        word: "migrate",
        does: "Recompile stored sources for a newer engine.",
        steps: &[step("version.migrate", Some("target"))],
        options: &[],
    },
    Shortcut {
        word: "connect",
        does: "Write the configuration for an MCP client.",
        steps: &[step("client.connect", Some("client"))],
        options: &[],
    },
    Shortcut {
        word: "doctor",
        does: "Check this machine and print fixes.",
        steps: &[step("doctor.run", None)],
        options: &[],
    },
    Shortcut {
        word: "docs",
        does: "A built-in reference page.",
        steps: &[step("docs.get", Some("topic"))],
        options: &[],
    },
];

/// The process commands: they start a long-lived role and are not actions.
pub const PROCESS_COMMANDS: &[&str] = &["serve", "runner", "bridge"];

/// The line a command prints for what the system itself said, apart from
/// the failure's sentence and its next step, marked as the system's words.
/// Every control character is shown as its escape, so that nothing the
/// system said can move the cursor, ring, or rewrite the line before it,
/// and a line break stays visible. The text, already redacted, is cut to
/// [`a2m_core::SAID_LONGEST`] characters, never inside a redaction mark.
#[must_use]
pub fn said_line(said: &str) -> String {
    format!(
        "The system said: \"{}\"",
        a2m_core::said_quoted(said, a2m_core::SAID_LONGEST)
    )
}
