//! The command tree, built from the action registry's command faces, and
//! the parser over it.
//!
//! Nothing here knows an action. A command is `<noun> <verb>` with the
//! arguments its face lists; a shortcut is one word and the fixed list of
//! actions its data names; the process commands and the global flags are
//! the only words written by hand.

use std::collections::BTreeMap;
use std::path::PathBuf;

use a2m_actions::{
    ActionRegistry, CommandArg, CommandFace, PROCESS_COMMANDS, PROGRAM, SHORTCUTS, Shortcut,
    ShortcutRun, ShortcutStep, codes, command_faces,
};
use a2m_core::{Failure, FailureClass};

use super::shown;
use serde_json::{Map, Value as Json};

/// The global flags: their form, and what each is for. They are the same
/// for every command and are never an action's argument.
pub(crate) const GLOBAL_FLAGS: &[(&str, &str)] = &[
    (
        "--json",
        "Print the result as one JSON object. Always so when the output is not a terminal.",
    ),
    (
        "--project <folder>",
        "The project folder. Otherwise the nearest folder upward that holds `anything2mcp.toml`.",
    ),
    (
        "--in-process",
        "Run the engine inside this command, with no local engine. Refused while one runs.",
    ),
    (
        "--idempotency-key <key>",
        "A key that makes a repeated call the same call.",
    ),
    ("--help, -h", "Show help for the command."),
    ("--version, -V", "Show the version. Only as the first word."),
];

/// The names of the global flags that take a value.
const GLOBAL_VALUES: &[&str] = &["--project", "--idempotency-key"];
/// The names of the global flags that take none.
const GLOBAL_SWITCHES: &[&str] = &["--json", "--in-process", "--help", "-h"];

/// The flags of a command that are not its action's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Globals {
    pub(crate) json: bool,
    pub(crate) project: Option<PathBuf>,
    pub(crate) in_process: bool,
    pub(crate) idempotency_key: Option<String>,
}

/// One action to dispatch, with its input.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Step {
    pub(crate) action: String,
    pub(crate) input: Json,
    /// The input field that standard input fills before the action is
    /// sent, when the command gave the flag whose face says so. The flag
    /// itself is not part of the input.
    pub(crate) stdin_for: Option<String>,
}

/// Where a shortcut stops on this build: before the first of its actions
/// that is not built. It never begins that step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stop {
    /// The action it stops before.
    pub(crate) before: &'static str,
    /// What to say: what was done, and what was not.
    pub(crate) note: String,
}

/// What a command line asks for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Parsed {
    /// The version.
    Version,
    /// Help text.
    Help(String),
    /// Actions to dispatch in turn; the first that is not a success ends
    /// the sequence. One for an action command, several for a shortcut. A
    /// shortcut that cannot do all of its steps on this build says where
    /// it stops.
    Invoke(Globals, Vec<Step>, Option<Stop>),
    /// A process command, with the words that follow it.
    Process(Globals, &'static str, Vec<String>),
    /// A shortcut with no action beneath it.
    EmptyShortcut(Globals, &'static str),
}

/// The command tree.
#[derive(Debug, Clone)]
pub(crate) struct Tree {
    faces: Vec<CommandFace>,
    /// What each shortcut does on this build, by its word.
    runs: BTreeMap<&'static str, ShortcutRun>,
}

fn malformed(message: impl Into<String>, fix: impl Into<String>) -> Failure {
    Failure::new(FailureClass::Input, codes::SCHEMA_INVALID, message, fix)
}

/// The words nearest to `wrong`, by shared prefix and then by edits.
fn nearest<'a>(wrong: &str, known: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    fn distance(a: &str, b: &str) -> usize {
        let b: Vec<char> = b.chars().collect();
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.chars().enumerate() {
            let mut last = row[0];
            row[0] = i + 1;
            for (j, cb) in b.iter().enumerate() {
                let old = row[j + 1];
                row[j + 1] = (last + usize::from(ca != *cb)).min(row[j] + 1).min(old + 1);
                last = old;
            }
        }
        row[b.len()]
    }
    let mut scored: Vec<(usize, &str)> = known
        .map(|word| (distance(wrong, word), word))
        .filter(|(score, word)| *score <= 2 || word.starts_with(wrong))
        .collect();
    scored.sort();
    scored.dedup();
    scored.into_iter().take(3).map(|(_, word)| word).collect()
}

impl Tree {
    /// The tree of a registry: its command faces, and what each shortcut
    /// does with the actions the registry holds.
    pub(crate) fn of(registry: &ActionRegistry) -> Self {
        Self {
            faces: command_faces(registry),
            runs: SHORTCUTS
                .iter()
                .map(|shortcut| (shortcut.word, shortcut.on(registry)))
                .collect(),
        }
    }

    /// A tree with no command, for a failure met before a registry could
    /// be built.
    pub(crate) fn empty() -> Self {
        Self {
            faces: Vec::new(),
            runs: BTreeMap::new(),
        }
    }

    /// Every command face, in catalog order.
    pub(crate) fn faces(&self) -> &[CommandFace] {
        &self.faces
    }

    /// The nouns, in catalog order, each once.
    pub(crate) fn nouns(&self) -> Vec<&str> {
        let mut nouns: Vec<&str> = Vec::new();
        for face in &self.faces {
            if !nouns.contains(&face.noun.as_str()) {
                nouns.push(&face.noun);
            }
        }
        nouns
    }

    fn of_noun(&self, noun: &str) -> Vec<&CommandFace> {
        self.faces.iter().filter(|face| face.noun == noun).collect()
    }

    fn face(&self, noun: &str, verb: &str) -> Option<&CommandFace> {
        self.faces
            .iter()
            .find(|face| face.noun == noun && face.verb == verb)
    }

    /// The command face of an action, by the action's name.
    pub(crate) fn face_of(&self, action: &str) -> Option<&CommandFace> {
        self.faces.iter().find(|face| face.action == action)
    }

    /// Parses the arguments that follow the program's name.
    pub(crate) fn parse(&self, args: &[String]) -> Result<Parsed, Failure> {
        if matches!(args.first().map(String::as_str), Some("--version" | "-V")) && args.len() == 1 {
            return Ok(Parsed::Version);
        }
        let (globals, help, rest) = split_globals(args)?;
        let Some(first) = rest.first() else {
            return Ok(Parsed::Help(self.help_top()));
        };
        let second = rest.get(1).map(String::as_str);

        // A word with one of its verbs is the action. The bare word, or
        // the word with anything else, is the shortcut or the process
        // command of that name when there is one.
        if let Some(face) = second.and_then(|verb| self.face(first, verb)) {
            if help {
                return Ok(Parsed::Help(help_action(face)));
            }
            let (input, stdin_for) = parse_args(face, &face.args, None, &rest[2..])?;
            return Ok(Parsed::Invoke(
                globals,
                vec![Step {
                    action: face.action.clone(),
                    input,
                    stdin_for,
                }],
                None,
            ));
        }
        if let Some(shortcut) = SHORTCUTS.iter().find(|shortcut| shortcut.word == first) {
            if help {
                return Ok(Parsed::Help(self.help_shortcut(shortcut)));
            }
            if shortcut.steps.is_empty() {
                return Ok(Parsed::EmptyShortcut(globals, shortcut.word));
            }
            // The steps this build can carry out: those before the first
            // action that is not built.
            let (steps, stop) = match self.runs.get(shortcut.word) {
                Some(run) => (
                    run.steps.as_slice(),
                    run.stops_before.map(|before| Stop {
                        before,
                        note: run.note.clone().unwrap_or_default(),
                    }),
                ),
                None => (shortcut.steps, None),
            };
            return Ok(Parsed::Invoke(
                globals,
                self.shortcut_steps(shortcut, steps, &rest[1..])?,
                stop,
            ));
        }
        if let Some(word) = PROCESS_COMMANDS.iter().find(|word| *word == first) {
            if help {
                return Ok(Parsed::Help(help_process(word)));
            }
            return Ok(Parsed::Process(globals, word, rest[1..].to_vec()));
        }
        let verbs = self.of_noun(first);
        if !verbs.is_empty() {
            if help || second.is_none() {
                return Ok(Parsed::Help(self.help_noun(first)));
            }
            let known: Vec<&str> = verbs.iter().map(|face| face.verb.as_str()).collect();
            return Err(malformed(
                format!(
                    "`{first}` has no command `{}`.",
                    shown(second.unwrap_or_default())
                ),
                format!("Use one of: {}.", known.join(", ")),
            ));
        }
        let words = self
            .nouns()
            .into_iter()
            .chain(SHORTCUTS.iter().map(|shortcut| shortcut.word))
            .chain(PROCESS_COMMANDS.iter().copied());
        let close = nearest(first, words);
        Err(malformed(
            format!("There is no command `{}`.", shown(first)),
            if close.is_empty() {
                format!("Run `{PROGRAM} --help` for the commands.")
            } else {
                format!(
                    "Did you mean: {}? Run `{PROGRAM} --help` for the commands.",
                    close.join(", ")
                )
            },
        ))
    }

    /// The actions of a shortcut, each with the shortcut's own argument in
    /// the field its step names. A shortcut of one action also takes that
    /// action's flags.
    fn shortcut_steps(
        &self,
        shortcut: &Shortcut,
        run: &[ShortcutStep],
        rest: &[String],
    ) -> Result<Vec<Step>, Failure> {
        let single = shortcut.steps.len() == 1;
        let mut steps = Vec::new();
        for step in run {
            let Some(face) = self.face_of(step.action) else {
                return Err(malformed(
                    format!(
                        "The shortcut `{}` names the action `{}`, which this build does not have.",
                        shortcut.word, step.action
                    ),
                    format!("Run `{PROGRAM} --help` for the commands."),
                ));
            };
            let (input, stdin_for) = if single {
                parse_args(face, &face.args, step.argument, rest)?
            } else {
                // Several actions share one argument and nothing else.
                let (positional, flag) = tokens(rest);
                if let Some(flag) = flag {
                    return Err(malformed(
                        format!(
                            "The shortcut `{}` takes one argument and no flag, and `{}` was given.",
                            shortcut.word,
                            shown(&flag)
                        ),
                        format!("Run `{PROGRAM} {} --help`.", shortcut.word),
                    ));
                }
                let mut input = Map::new();
                match (step.argument, positional.as_slice()) {
                    (Some(field), [value]) => {
                        input.insert(field.to_owned(), Json::String(value.clone()));
                    }
                    (_, []) => {}
                    _ => {
                        return Err(malformed(
                            format!("The shortcut `{}` takes one argument.", shortcut.word),
                            format!("Run `{PROGRAM} {} --help`.", shortcut.word),
                        ));
                    }
                }
                (Json::Object(input), None)
            };
            steps.push(Step {
                action: face.action.clone(),
                input,
                stdin_for,
            });
        }
        Ok(steps)
    }

    fn help_top(&self) -> String {
        let mut out = format!(
            "{PROGRAM} {}\n\nUsage: {PROGRAM} <noun> <verb> [arguments]\n       {PROGRAM} <shortcut> [argument]\n\nCommands, by noun (`{PROGRAM} <noun> --help` lists a noun's commands):\n",
            crate::version()
        );
        for noun in self.nouns() {
            let verbs: Vec<&str> = self
                .of_noun(noun)
                .iter()
                .map(|face| face.verb.as_str())
                .collect();
            out.push_str(&format!("  {noun:<12} {}\n", verbs.join(", ")));
        }
        out.push_str("\nShortcuts:\n");
        for shortcut in SHORTCUTS {
            out.push_str(&format!("  {:<12} {}\n", shortcut.word, shortcut.does));
        }
        out.push_str(
            "\nProcess commands (`<command> --help` says what each does; `runner` is not built):\n",
        );
        out.push_str(&format!("  {}\n", PROCESS_COMMANDS.join(", ")));
        out.push_str("\nGlobal flags:\n");
        for (flag, does) in GLOBAL_FLAGS {
            out.push_str(&format!("  {flag:<24} {does}\n"));
        }
        out
    }

    fn help_noun(&self, noun: &str) -> String {
        let mut out = format!("Commands of `{noun}`:\n");
        for face in self.of_noun(noun) {
            out.push_str(&format!("  {}\n      {}\n", face.usage, face.help));
        }
        out
    }

    fn help_shortcut(&self, shortcut: &Shortcut) -> String {
        let mut out = format!(
            "{PROGRAM} {} [argument]\n\n{}\n",
            shortcut.word, shortcut.does
        );
        if shortcut.steps.is_empty() {
            out.push_str("\nNo action is beneath it yet.\n");
            return out;
        }
        out.push_str("\nIt runs, in order, stopping at the first that does not succeed:\n");
        let built = self
            .runs
            .get(shortcut.word)
            .map_or(shortcut.steps.len(), |run| run.steps.len());
        for (at, step) in shortcut.steps.iter().enumerate() {
            let usage = self
                .face_of(step.action)
                .map_or_else(|| step.action.to_owned(), |face| face.usage.clone());
            out.push_str(&format!("  {usage}\n"));
            if at == built {
                out.push_str("      Not built yet: the shortcut stops before this step.\n");
            }
        }
        out
    }
}

/// The help of a process command. These three are the only commands
/// written by hand.
fn help_process(word: &str) -> String {
    let (usage, about) = match word {
        "serve" => (
            format!("{PROGRAM} serve"),
            "Runs the local engine in the foreground: the one process that holds the store, \
             serves commands and MCP clients over the local channel of this machine, and shows \
             the page on which a person answers a write that waits.\nIt stops when asked (`anything2mcp engine stop`, or the terminal's \
             interrupt) and after `engine.idle_stop_minutes` with nothing to do.\nA second engine \
             on the same data directory is refused.",
        ),
        "bridge" => (
            format!("{PROGRAM} bridge [--toolset use|build]"),
            "The program an MCP client starts: it copies MCP messages between its standard \
             input and output and the local engine's MCP endpoint.\nStandard output carries \
             protocol messages and nothing else.\nIt starts no engine: until one runs \
             (`anything2mcp serve`), every request is answered with an error that says so.\nThe \
             project folder is the one it is started in, or the one `--project` names.",
        ),
        _ => (
            format!("{PROGRAM} {word}"),
            "It is not built in this build.",
        ),
    };
    format!(
        "{usage}\n\nA process command: it starts a long-lived role and is not an action.\n{about}\n"
    )
}

fn help_action(face: &CommandFace) -> String {
    let mut out = format!("{}\n\n{}\n", face.usage, face.help);
    if !face.args.is_empty() {
        out.push_str("\nArguments:\n");
        for arg in &face.args {
            let name = if arg.positional {
                format!("<{}>", arg.field)
            } else if arg.ty == "flag" {
                arg.flag.clone()
            } else {
                format!("{} <{}>", arg.flag, arg.ty)
            };
            let mut notes = Vec::new();
            if arg.required {
                notes.push("required".to_owned());
            }
            if !arg.choices.is_empty() {
                notes.push(format!("one of: {}", arg.choices.join(", ")));
            }
            let notes = if notes.is_empty() {
                String::new()
            } else {
                format!(" ({})", notes.join("; "))
            };
            out.push_str(&format!("  {name:<28} {}{notes}\n", arg.help));
        }
    }
    if face.long_running {
        out.push_str("\nIt returns a `run_id` at once; follow it with `run get`.\n");
    }
    out
}

/// Takes the global flags out of the arguments, wherever they stand before
/// a `--`. What is left is the command's own words, in order.
fn split_globals(args: &[String]) -> Result<(Globals, bool, Vec<String>), Failure> {
    let mut globals = Globals::default();
    let mut help = false;
    let mut rest = Vec::new();
    let mut at = 0;
    while at < args.len() {
        let arg = &args[at];
        at += 1;
        if arg == "--" {
            rest.push(arg.clone());
            rest.extend(args[at..].iter().cloned());
            break;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_owned())),
            _ => (arg.as_str(), None),
        };
        if GLOBAL_SWITCHES.contains(&name) && inline.is_none() {
            match name {
                "--json" => globals.json = true,
                "--in-process" => globals.in_process = true,
                _ => help = true,
            }
        } else if GLOBAL_VALUES.contains(&name) {
            let value = match inline {
                Some(value) => value,
                None => {
                    let value = args.get(at).cloned().ok_or_else(|| {
                        malformed(
                            format!("`{name}` needs a value."),
                            format!("Write `{name} <value>`."),
                        )
                    })?;
                    at += 1;
                    value
                }
            };
            if name == "--project" {
                globals.project = Some(PathBuf::from(value));
            } else {
                globals.idempotency_key = Some(value);
            }
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((globals, help, rest))
}

/// Splits a shortcut's arguments into the words given without a flag and
/// the first flag among them, if there is one.
fn tokens(rest: &[String]) -> (Vec<String>, Option<String>) {
    let mut positional = Vec::new();
    let mut flag = None;
    let mut literal = false;
    for arg in rest {
        if literal {
            positional.push(arg.clone());
        } else if arg == "--" {
            literal = true;
        } else if arg.starts_with("--") {
            flag.get_or_insert_with(|| arg.clone());
        } else {
            positional.push(arg.clone());
        }
    }
    (positional, flag)
}

/// One argument's written value as the JSON its type asks for.
fn value_of(arg: &CommandArg, written: &str) -> Result<Json, Failure> {
    let wrong = |what: &str| {
        malformed(
            format!(
                "`{}` takes {what}, and `{}` was given.",
                shown_name(arg),
                shown(written)
            ),
            format!("Write {what}."),
        )
    };
    match arg.ty {
        "number" => written
            .parse::<i64>()
            .map(Json::from)
            .map_err(|_| wrong("a whole number")),
        "flag" => match written {
            "true" => Ok(Json::Bool(true)),
            "false" => Ok(Json::Bool(false)),
            _ => Err(wrong("`true` or `false`")),
        },
        "map" => match serde_json::from_str::<Json>(written) {
            Ok(Json::Object(map)) => Ok(Json::Object(map)),
            _ => Err(wrong("a JSON object, such as '{\"name\": \"value\"}'")),
        },
        // A value of any shape: JSON when it reads as JSON, text otherwise.
        "value" => Ok(serde_json::from_str::<Json>(written)
            .unwrap_or_else(|_| Json::String(written.to_owned()))),
        _ => Ok(Json::String(written.to_owned())),
    }
}

fn shown_name(arg: &CommandArg) -> String {
    if arg.positional {
        format!("<{}>", arg.field)
    } else {
        arg.flag.clone()
    }
}

/// Parses a command's own arguments against its face into the action's
/// input. `shortcut_field` is the field a shortcut's bare argument fills,
/// when it is not the face's own positional field.
///
/// A flag whose face names a field for standard input is not put into the
/// input: the field is returned beside it, for the caller to fill from
/// standard input.
fn parse_args(
    face: &CommandFace,
    args: &[CommandArg],
    shortcut_field: Option<&str>,
    rest: &[String],
) -> Result<(Json, Option<String>), Failure> {
    let usage = || format!("Usage: {}", face.usage);
    let by_flag: BTreeMap<&str, &CommandArg> = args
        .iter()
        .filter(|arg| !arg.positional)
        .map(|arg| (arg.flag.as_str(), arg))
        .collect();
    let positional = args
        .iter()
        .find(|arg| arg.positional)
        .or_else(|| shortcut_field.and_then(|field| args.iter().find(|arg| arg.field == field)));
    let mut input = Map::new();
    let mut from_stdin: Option<&CommandArg> = None;
    let mut given_positional = false;
    let mut literal = false;
    let mut at = 0;
    while at < rest.len() {
        let word = &rest[at];
        at += 1;
        if !literal && word == "--" {
            literal = true;
            continue;
        }
        if literal || !word.starts_with("--") {
            let Some(arg) = positional else {
                return Err(malformed(
                    format!(
                        "`{} {}` takes no argument without a flag, and `{}` was given.",
                        face.noun,
                        face.verb,
                        shown(word)
                    ),
                    usage(),
                ));
            };
            if given_positional {
                return Err(malformed(
                    format!(
                        "`{} {}` takes one argument without a flag, and a second, `{}`, was given.",
                        face.noun,
                        face.verb,
                        shown(word)
                    ),
                    usage(),
                ));
            }
            given_positional = true;
            input.insert(arg.field.clone(), value_of(arg, word)?);
            continue;
        }
        let (name, inline) = match word.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (word.as_str(), None),
        };
        let Some(arg) = by_flag.get(name).copied() else {
            let close = nearest(name, by_flag.keys().copied());
            return Err(malformed(
                format!(
                    "`{} {}` has no flag `{}`.",
                    face.noun,
                    face.verb,
                    shown(name)
                ),
                if close.is_empty() {
                    usage()
                } else {
                    format!("Did you mean: {}? {}", close.join(", "), usage())
                },
            ));
        };
        let value = match (arg.ty, inline) {
            ("flag", None) => Json::Bool(true),
            (_, Some(written)) => value_of(arg, written)?,
            (_, None) => {
                let written = rest
                    .get(at)
                    .ok_or_else(|| malformed(format!("`{name}` needs a value."), usage()))?;
                at += 1;
                value_of(arg, written)?
            }
        };
        if arg.stdin_for.is_some() {
            // The flag says where the value comes from, and is not sent.
            if from_stdin.is_some() {
                return Err(malformed(format!("`{name}` was given twice."), usage()));
            }
            if value == Json::Bool(true) {
                from_stdin = Some(arg);
            }
            continue;
        }
        if arg.ty == "list" {
            let list = input
                .entry(arg.field.clone())
                .or_insert_with(|| Json::Array(Vec::new()));
            if let Json::Array(items) = list {
                items.push(value);
            }
        } else if input.insert(arg.field.clone(), value).is_some() {
            return Err(malformed(format!("`{name}` was given twice."), usage()));
        }
    }
    let stdin_for = from_stdin.and_then(|arg| arg.stdin_for.clone());
    if let (Some(flag), Some(field)) = (from_stdin, &stdin_for)
        && input.contains_key(field)
    {
        // Never the value itself: only the names of the two arguments.
        let other = args
            .iter()
            .find(|arg| arg.field == *field)
            .map_or_else(|| field.clone(), shown_name);
        return Err(malformed(
            format!(
                "`{}` reads `{field}` from standard input, and `{other}` was given as well.",
                flag.flag
            ),
            format!("Give `{}` or `{other}`, not both.", flag.flag),
        ));
    }
    if let Some(missing) = args.iter().find(|arg| {
        arg.required && !input.contains_key(&arg.field) && stdin_for.as_ref() != Some(&arg.field)
    }) {
        return Err(malformed(
            format!(
                "`{} {}` needs {}.",
                face.noun,
                face.verb,
                shown_name(missing)
            ),
            usage(),
        ));
    }
    Ok((Json::Object(input), stdin_for))
}

/// The command line that asks for `action` with `input`, for the `next`
/// line of a result. `None` when the action has no command face.
pub(crate) fn command_line(tree: &Tree, action: &str, input: &Json) -> Option<String> {
    let face = tree.face_of(action)?;
    let mut line = format!("{PROGRAM} {} {}", face.noun, face.verb);
    let fields = input.as_object();
    for arg in &face.args {
        let Some(value) = fields.and_then(|fields| fields.get(&arg.field)) else {
            continue;
        };
        let written = match value {
            Json::String(text) => text.clone(),
            other => other.to_string(),
        };
        let quoted = if written.is_empty()
            || written
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || "-_./:@".contains(c)))
        {
            format!("'{}'", written.replace('\'', "'\\''"))
        } else {
            written
        };
        if arg.positional {
            line.push_str(&format!(" {quoted}"));
        } else if arg.ty == "flag" {
            if value == &Json::Bool(true) {
                line.push_str(&format!(" {}", arg.flag));
            }
        } else {
            line.push_str(&format!(" {} {quoted}", arg.flag));
        }
    }
    Some(line)
}
