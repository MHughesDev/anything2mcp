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
    ActionDef, ActionRegistry, CommandArg, CommandFace, Effect, PROCESS_COMMANDS, PROGRAM,
    PersonNeed, RunsIn, Shortcut, ShortcutOption, codes, command_faces,
};
use a2m_core::{Failure, FailureClass};

use super::shown;
use serde_json::{Map, Value as Json, json};

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
pub(crate) const GLOBAL_VALUES: &[&str] = &["--project", "--idempotency-key"];

/// The command that prints a completion script for a shell. The program
/// answers it itself, as it answers `--help`: no engine is started and no
/// folder is made.
pub const COMPLETION_COMMAND: &str = "completion";

/// The width help is wrapped to when the environment does not give one.
pub(crate) const DEFAULT_WIDTH: usize = 80;
/// The narrowest width help is wrapped to; a narrower `COLUMNS` is not
/// taken.
pub(crate) const NARROWEST_WIDTH: usize = 40;

/// The width help is wrapped to: the environment's `COLUMNS` when it is a
/// whole number of at least [`NARROWEST_WIDTH`], else [`DEFAULT_WIDTH`].
pub(crate) fn width_of(environment: &BTreeMap<String, String>) -> usize {
    environment
        .get("COLUMNS")
        .and_then(|columns| columns.trim().parse::<usize>().ok())
        .filter(|columns| *columns >= NARROWEST_WIDTH)
        .unwrap_or(DEFAULT_WIDTH)
}

/// The actions the client end carries out itself. An action the registry
/// marks as the client's and that is not among these is carried out by
/// nobody in this build, and its help says that it is not built.
pub(crate) const CARRIED_BY_THE_CLIENT: &[&str] = &["engine.start", "engine.stop", "engine.logs"];

/// What the help of an action says beside its usage and summary, from its
/// definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    /// Whether a call can be answered with anything but `action_not_built`.
    pub(crate) built: bool,
    /// The phase or stage of the plan that builds it.
    pub(crate) phase: String,
    /// Its effect class, or the tool's.
    pub(crate) effect: &'static str,
    /// Who may run it, in the action catalog's words.
    pub(crate) who: &'static str,
    /// Whether a person at this machine is asked.
    pub(crate) person: &'static str,
}

impl Facts {
    pub(crate) fn of(def: &ActionDef) -> Self {
        let carried =
            def.runs_in != RunsIn::Client || CARRIED_BY_THE_CLIENT.contains(&def.name.as_str());
        Self {
            built: def.built() && carried,
            phase: def.phase.to_owned(),
            effect: match def.effect {
                Effect::Fixed(effect) => effect.as_str(),
                Effect::OfTool => "the effect of the tool it calls",
            },
            who: def.who.as_str(),
            person: match def.person {
                PersonNeed::None => "no",
                PersonNeed::Presence => "always",
                PersonNeed::PresenceWhenLoosening => "when it loosens something protected",
            },
        }
    }

    /// The first line of the help of an action that is not built.
    fn not_built(&self) -> String {
        let when = if self.phase.is_empty() {
            String::new()
        } else if self.phase.chars().all(|c| c.is_ascii_digit()) {
            format!(" (it arrives with Phase {})", self.phase)
        } else {
            format!(" (it arrives with {})", self.phase)
        };
        format!("Not built yet: a call answers `action_not_built`{when}.")
    }

    /// The line that says the effect, who may run it, and whether a person
    /// is asked.
    fn line(&self) -> String {
        format!(
            "Effect: {}. Who may run it: {}. A person at this machine is asked: {}.",
            self.effect, self.who, self.person
        )
    }
}

/// `text` wrapped at its spaces to `width` columns: the first line begins
/// with `first`, every other with `hang` spaces. A word longer than a whole
/// line is cut, so that no line is wider than `width`.
pub(crate) fn wrap(first: &str, text: &str, hang: usize, width: usize) -> String {
    fn finish(out: &mut String, line: &str) {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    let hang = hang.min(width / 2);
    let indent = " ".repeat(hang);
    let mut out = String::new();
    let mut line = first.to_owned();
    // Whether the line holds its indent and no word yet.
    let mut fresh = true;
    for word in text.split_whitespace() {
        let mut rest = word;
        while !rest.is_empty() {
            let used = line.chars().count();
            let need = rest.chars().count() + usize::from(!fresh);
            if used + need <= width {
                if !fresh {
                    line.push(' ');
                }
                line.push_str(rest);
                fresh = false;
                break;
            }
            if fresh {
                // Not even alone on a line: as much as fits, then the rest.
                let room = width.saturating_sub(used);
                let cut = rest
                    .char_indices()
                    .nth(room)
                    .map_or(rest.len(), |(at, _)| at);
                line.push_str(&rest[..cut]);
                rest = &rest[cut..];
            }
            finish(&mut out, &line);
            line.clone_from(&indent);
            fresh = true;
        }
    }
    if !fresh || out.is_empty() {
        finish(&mut out, &line);
    }
    out
}

/// One row of a two-column list, wrapped: `left` in a column `column` wide
/// after `indent`, and `right` beside it. Where the columns would leave too
/// little room, `right` goes below `left` instead.
fn row(indent: &str, left: &str, column: usize, right: &str, width: usize) -> String {
    let at = indent.chars().count() + column + 1;
    if left.chars().count() < column && at <= width / 2 {
        return wrap(&format!("{indent}{left:<column$} "), right, at, width);
    }
    let below = format!("{indent}    ");
    let mut out = wrap(indent, left, below.len(), width);
    if !right.trim().is_empty() {
        out.push_str(&wrap(&below, right, below.len(), width));
    }
    out
}

/// Each paragraph of `text`, wrapped to `width` with no indent.
fn paragraphs(text: &str, width: usize) -> String {
    text.split('\n')
        .map(|paragraph| wrap("", paragraph, 0, width))
        .collect()
}
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

/// What a command line asks for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Parsed {
    /// The version.
    Version,
    /// Help text.
    Help(String),
    /// Actions to dispatch in turn; the first that is not a success ends
    /// the sequence. One for an action command, several for a shortcut,
    /// whose every step is built, or it is not offered.
    Invoke(Globals, Vec<Step>),
    /// A process command, with the words that follow it.
    Process(Globals, &'static str, Vec<String>),
    /// A shortcut with no action beneath it.
    EmptyShortcut(Globals, &'static str),
}

/// The command tree.
#[derive(Debug, Clone)]
pub(crate) struct Tree {
    faces: Vec<CommandFace>,
    /// The shortcuts offered on this build: those whose every step is
    /// built (decision 0456).
    offered: Vec<&'static Shortcut>,
    /// What the help of each action says of it, by the action's name.
    facts: BTreeMap<String, Facts>,
    /// The width help is wrapped to.
    width: usize,
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
    /// The tree of a registry: its command faces, and the shortcuts whose
    /// every step the registry has built.
    pub(crate) fn of(registry: &ActionRegistry) -> Self {
        Self {
            faces: command_faces(registry),
            offered: a2m_actions::shortcuts_offered(registry),
            facts: registry
                .iter()
                .map(|def| (def.name.to_string(), Facts::of(def)))
                .collect(),
            width: DEFAULT_WIDTH,
        }
    }

    /// The same tree, with its help wrapped to `width` columns.
    pub(crate) fn with_width(mut self, width: usize) -> Self {
        self.width = width.max(NARROWEST_WIDTH);
        self
    }

    /// A tree with no command, for a failure met before a registry could
    /// be built.
    pub(crate) fn empty() -> Self {
        Self {
            faces: Vec::new(),
            offered: Vec::new(),
            facts: BTreeMap::new(),
            width: DEFAULT_WIDTH,
        }
    }

    /// What the help says of an action.
    pub(crate) fn facts(&self, action: &str) -> Option<&Facts> {
        self.facts.get(action)
    }

    /// Whether an action is built, as its help says.
    pub(crate) fn built(&self, action: &str) -> bool {
        self.facts(action).is_none_or(|facts| facts.built)
    }

    /// The shortcuts offered, in order, with what each does.
    pub(crate) fn shortcuts(&self) -> impl Iterator<Item = &'static Shortcut> + '_ {
        self.offered.iter().copied()
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
                return Ok(Parsed::Help(self.help_action(face)));
            }
            let (input, stdin_for) = parse_args(face, &face.args, None, &rest[2..])?;
            return Ok(Parsed::Invoke(
                globals,
                vec![Step {
                    action: face.action.clone(),
                    input,
                    stdin_for,
                }],
            ));
        }
        if let Some(shortcut) = self.shortcuts().find(|shortcut| shortcut.word == first) {
            if help {
                return Ok(Parsed::Help(self.help_shortcut(shortcut)));
            }
            if shortcut.steps.is_empty() {
                return Ok(Parsed::EmptyShortcut(globals, shortcut.word));
            }
            return Ok(Parsed::Invoke(
                globals,
                self.shortcut_steps(shortcut, &rest[1..])?,
            ));
        }
        if first == COMPLETION_COMMAND {
            if help {
                return Ok(Parsed::Help(self.help_process(COMPLETION_COMMAND)));
            }
            return Ok(Parsed::Process(
                globals,
                COMPLETION_COMMAND,
                rest[1..].to_vec(),
            ));
        }
        if let Some(word) = PROCESS_COMMANDS.iter().find(|word| *word == first) {
            if help {
                return Ok(Parsed::Help(self.help_process(word)));
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
            .chain(self.shortcuts().map(|shortcut| shortcut.word))
            .chain(process_commands().map(|word| -> &str { word }));
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
    fn shortcut_steps(&self, shortcut: &Shortcut, rest: &[String]) -> Result<Vec<Step>, Failure> {
        let single = shortcut.steps.len() == 1;
        let (given, rest) = shortcut_options(shortcut, rest)?;
        let rest = rest.as_slice();
        let mut steps = Vec::new();
        for step in shortcut.steps {
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
                            // The flag's name, never a value written into it.
                            shown(flag.split('=').next().unwrap_or_default())
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
            let mut input = input;
            for (option, value) in &given {
                if option.action != step.action {
                    continue;
                }
                if let Json::Object(map) = &mut input {
                    map.insert(
                        option.field.to_owned(),
                        match option.key {
                            Some(key) => json!({ key: value }),
                            None => Json::String(value.clone()),
                        },
                    );
                }
            }
            steps.push(Step {
                action: face.action.clone(),
                input,
                stdin_for,
            });
        }
        Ok(steps)
    }

    /// A verb as the listings show it: marked when it is not built.
    fn listed_verb(&self, face: &CommandFace) -> String {
        if self.built(&face.action) {
            face.verb.clone()
        } else {
            format!("{} (not built)", face.verb)
        }
    }

    pub(crate) fn help_top(&self) -> String {
        let width = self.width;
        let mut out = format!("{PROGRAM} {}\n\n", crate::version());
        out.push_str(&row(
            "",
            "Usage:",
            7,
            &format!("{PROGRAM} <noun> <verb> [arguments]"),
            width,
        ));
        out.push_str(&row(
            "",
            "",
            7,
            &format!("{PROGRAM} <shortcut> [argument]"),
            width,
        ));
        out.push('\n');
        out.push_str(&wrap(
            "",
            &format!(
                "Commands, by noun (`{PROGRAM} <noun> --help` lists a noun's commands; a verb marked (not built) answers `action_not_built`):"
            ),
            0,
            width,
        ));
        for noun in self.nouns() {
            let verbs: Vec<String> = self
                .of_noun(noun)
                .iter()
                .map(|face| self.listed_verb(face))
                .collect();
            out.push_str(&row("  ", noun, 12, &verbs.join(", "), width));
        }
        out.push_str("\nShortcuts:\n");
        for shortcut in self.shortcuts() {
            out.push_str(&row("  ", shortcut.word, 12, shortcut.does, width));
        }
        out.push('\n');
        out.push_str(&wrap(
            "",
            "Process commands (`<command> --help` says what each does; `runner` is not built):",
            0,
            width,
        ));
        let words: Vec<&str> = process_commands().collect();
        out.push_str(&wrap("  ", &words.join(", "), 2, width));
        out.push_str("\nGlobal flags:\n");
        for (flag, does) in GLOBAL_FLAGS {
            out.push_str(&row("  ", flag, 24, does, width));
        }
        out
    }

    pub(crate) fn help_noun(&self, noun: &str) -> String {
        let width = self.width;
        let mut out = wrap("", &format!("Commands of `{noun}`:"), 0, width);
        for face in self.of_noun(noun) {
            let usage = if self.built(&face.action) {
                face.usage.clone()
            } else {
                format!("{} (not built)", face.usage)
            };
            out.push_str(&wrap("  ", &usage, 6, width));
            out.push_str(&wrap("      ", &face.help, 6, width));
        }
        out
    }

    pub(crate) fn help_shortcut(&self, shortcut: &Shortcut) -> String {
        let width = self.width;
        let mut out = wrap(
            "",
            &format!("{PROGRAM} {} [argument]", shortcut.word),
            4,
            width,
        );
        out.push('\n');
        out.push_str(&paragraphs(shortcut.does, width));
        if shortcut.steps.is_empty() {
            out.push('\n');
            out.push_str(&wrap(
                "",
                "The command does this itself, in the folder, and starts no engine: no action of the engine is beneath it.",
                0,
                width,
            ));
            return out;
        }
        if !shortcut.options.is_empty() {
            out.push_str("\nFlags:\n");
            for option in shortcut.options {
                out.push_str(&row(
                    "  ",
                    &format!("--{} <value>", option.flag),
                    24,
                    option.does,
                    width,
                ));
            }
        }
        out.push('\n');
        out.push_str(&wrap(
            "",
            "It runs, in order, stopping at the first that does not succeed:",
            0,
            width,
        ));
        for step in shortcut.steps {
            let usage = self
                .face_of(step.action)
                .map_or_else(|| step.action.to_owned(), |face| face.usage.clone());
            out.push_str(&wrap("  ", &usage, 6, width));
        }
        out
    }

    /// The help of a process command, or of the completion command. These
    /// are the only commands written by hand.
    pub(crate) fn help_process(&self, word: &str) -> String {
        let width = self.width;
        let (usage, kind, about) = match word {
            "serve" => (
                format!("{PROGRAM} serve [--passphrase-from-stdin]"),
                "A process command: it starts a long-lived role and is not an action.",
                "Runs the local engine in the foreground: the one process that holds the store, \
                 serves commands and MCP clients over the local channel of this machine, and shows \
                 the page on which a person answers a write that waits.\nIt stops when asked (`anything2mcp engine stop`, the terminal's \
                 interrupt, or the termination or hang-up signal) and after `engine.idle_stop_minutes` with nothing to do, \
                 and its last object names what asked it to stop.\nA second engine \
                 on the same data directory is refused.\nA command that finds no engine starts this \
                 one by itself. `--passphrase-from-stdin` reads the vault's passphrase from \
                 standard input, to its end, instead of from the environment.",
            ),
            "bridge" => (
                format!("{PROGRAM} bridge [--toolset use|build]"),
                "A process command: it starts a long-lived role and is not an action.",
                "The program an MCP client starts: it copies MCP messages between its standard \
                 input and output and the local engine's MCP endpoint.\nStandard output carries \
                 protocol messages and nothing else.\nA request that finds no engine starts one; \
                 where none may be started (`engine.in_process`), it is answered with an error \
                 that says to run `anything2mcp serve`.\nThe \
                 project folder is the one it is started in, or the one `--project` names.",
            ),
            COMPLETION_COMMAND => (
                format!(
                    "{PROGRAM} {COMPLETION_COMMAND} <{}>",
                    super::complete::SHELLS.join("|")
                ),
                "A command the program answers itself: it starts no engine, makes no folder, and is not an action.",
                "Prints a script that completes this program's commands and flags in the shell named, made from the same command tree as this help.\nValues that live in the engine, such as the names of targets and tools, are not completed.\nThe script is printed as it is, whatever the output is.",
            ),
            _ => (
                format!("{PROGRAM} {word}"),
                "A process command: it starts a long-lived role and is not an action.",
                "It is not built in this build.",
            ),
        };
        let mut out = wrap("", &usage, 4, width);
        out.push('\n');
        out.push_str(&paragraphs(&format!("{kind}\n{about}"), width));
        out
    }

    pub(crate) fn help_action(&self, face: &CommandFace) -> String {
        let width = self.width;
        let mut out = String::new();
        let facts = self.facts(&face.action);
        if let Some(facts) = facts.filter(|facts| !facts.built) {
            out.push_str(&wrap("", &facts.not_built(), 0, width));
        }
        out.push_str(&wrap("", &face.usage, 4, width));
        out.push('\n');
        out.push_str(&paragraphs(&face.help, width));
        if let Some(facts) = facts {
            out.push_str(&wrap("", &facts.line(), 0, width));
        }
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
                if arg.ty == "list" && !arg.positional {
                    notes.push(LIST_FORM.replace("--files", &arg.flag));
                }
                let notes = if notes.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", notes.join("; "))
                };
                out.push_str(&row(
                    "  ",
                    &name,
                    28,
                    &format!("{}{notes}", arg.help),
                    width,
                ));
            }
        }
        if face.long_running {
            out.push('\n');
            out.push_str(&wrap(
                "",
                "It returns a `run_id` at once; follow it with `run get`.",
                0,
                width,
            ));
        }
        out
    }
}

/// The process commands, and the completion command, in the order the
/// help lists them.
pub(crate) fn process_commands() -> impl Iterator<Item = &'static str> {
    PROCESS_COMMANDS
        .iter()
        .copied()
        .chain(std::iter::once(COMPLETION_COMMAND))
}

/// The flags of a process command, with what each is for. The completion
/// scripts offer them.
pub(crate) fn process_flags(word: &str) -> &'static [(&'static str, &'static str)] {
    match word {
        "serve" => &[(
            "--passphrase-from-stdin",
            "Read the vault's passphrase from standard input, to its end.",
        )],
        "bridge" => &[("--toolset", "The built-in toolset to serve: use or build.")],
        _ => &[],
    }
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

/// The flags of a shortcut that were written, each with its value.
type GivenOptions<'a> = Vec<(&'a ShortcutOption, String)>;

/// The flags a shortcut takes beside its argument, taken out of what was
/// written: each with its value, and the words that are left. A flag the
/// shortcut does not take is left for the rest of the parse to refuse.
fn shortcut_options<'a>(
    shortcut: &'a Shortcut,
    rest: &[String],
) -> Result<(GivenOptions<'a>, Vec<String>), Failure> {
    let mut given: Vec<(&ShortcutOption, String)> = Vec::new();
    let mut left = Vec::new();
    let mut at = 0;
    let mut literal = false;
    while at < rest.len() {
        let word = &rest[at];
        at += 1;
        if literal || !word.starts_with("--") {
            left.push(word.clone());
            continue;
        }
        if word == "--" {
            literal = true;
            left.push(word.clone());
            continue;
        }
        let (name, inline) = match word.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (word.as_str(), None),
        };
        let Some(option) = shortcut
            .options
            .iter()
            .find(|option| format!("--{}", option.flag) == name)
        else {
            left.push(word.clone());
            continue;
        };
        let value = match inline {
            Some(value) => value,
            None => {
                let Some(value) = rest.get(at) else {
                    return Err(malformed(
                        format!("`--{}` takes a value, and none was given.", option.flag),
                        format!("Run `{PROGRAM} {} --help`.", shortcut.word),
                    ));
                };
                at += 1;
                value.clone()
            }
        };
        if given.iter().any(|(one, _)| one.flag == option.flag) {
            return Err(malformed(
                format!("`--{}` was given twice.", option.flag),
                format!("Run `{PROGRAM} {} --help`.", shortcut.word),
            ));
        }
        given.push((option, value));
    }
    Ok((given, left))
}

/// One argument's written value as the JSON its type asks for. A value
/// that does not fit is never quoted back: it may be a secret written in
/// the wrong place, and an error message is kept in logs.
fn value_of(arg: &CommandArg, written: &str) -> Result<Json, Failure> {
    let wrong = |what: &str| {
        malformed(
            format!(
                "`{}` takes {what}, and the value given is not one.",
                shown_name(arg),
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

/// How a list is written on the command line, as the help and a refusal
/// say it (package 5.6b).
pub(crate) const LIST_FORM: &str = "a list: repeat the flag for each item (`--files a --files b`), or give one JSON array of text (`--files '[\"a\", \"b\"]'`)";

/// The items one value of a list flag gives: a JSON array of text is its
/// items, in order, as an agent that copies a list from a result writes
/// it; any other text is one item. A comma is part of an item, since a
/// name may hold one.
fn list_items(value: Json) -> Vec<Json> {
    if let Json::String(text) = &value
        && text.trim_start().starts_with('[')
        && let Ok(Json::Array(items)) = serde_json::from_str::<Json>(text)
        && !items.is_empty()
        && items.iter().all(Json::is_string)
    {
        return items;
    }
    vec![value]
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
                // The word is not quoted: it may be a secret written in the
                // wrong place.
                return Err(malformed(
                    format!(
                        "`{} {}` takes no argument without a flag, and one was given.",
                        face.noun, face.verb,
                    ),
                    usage(),
                ));
            };
            if given_positional {
                return Err(malformed(
                    format!(
                        "`{} {}` takes one argument without a flag, and a second was given.",
                        face.noun, face.verb,
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
                items.extend(list_items(value));
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

/// A word as a shell reads it back: quoted when it holds anything but
/// plain characters.
fn quote(written: &str) -> String {
    if written.is_empty()
        || written
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || "-_./:@".contains(c)))
    {
        format!("'{}'", written.replace('\'', "'\\''"))
    } else {
        written.to_owned()
    }
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
        let quoted = quote(&written);
        if arg.positional {
            line.push_str(&format!(" {quoted}"));
        } else if let (true, Json::Array(items)) = (arg.ty == "list", value) {
            // A list is written as the flag once for each item, the form
            // the help names first.
            for item in items {
                let written = match item {
                    Json::String(text) => text.clone(),
                    other => other.to_string(),
                };
                line.push_str(&format!(" {} {}", arg.flag, quote(&written)));
            }
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

#[cfg(test)]
mod tests {
    use a2m_actions::{
        ActionDef, ActionRegistry, Effect, PersonNeed, RunsIn, SHORTCUTS, command_faces,
    };
    use serde_json::json;

    use super::{CARRIED_BY_THE_CLIENT, Parsed, Tree, process_commands, width_of};

    /// Every action of this build, packs included, with its command face.
    fn every_action() -> (ActionRegistry, Tree) {
        let registry = crate::engine::action_registry().unwrap();
        let tree = Tree::of(&registry);
        (registry, tree)
    }

    fn help(tree: &Tree, args: &[&str]) -> String {
        let mut args = words(args);
        args.push("--help".to_owned());
        match tree.parse(&args).unwrap() {
            Parsed::Help(text) => text,
            other => panic!("{args:?}: {other:?}"),
        }
    }

    /// Help as it reads before it was wrapped.
    fn flat(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Whether an action is built, from its definition alone: it has a
    /// handler, and a client's action is one the client end carries out.
    fn built(def: &ActionDef) -> bool {
        def.built()
            && (def.runs_in != RunsIn::Client || CARRIED_BY_THE_CLIENT.contains(&def.name.as_str()))
    }

    #[test]
    fn every_actions_help_says_its_effect_who_and_whether_a_person_is_asked() {
        let (registry, tree) = every_action();
        let faces = command_faces(&registry);
        assert!(faces.len() > 80, "{}", faces.len());
        for face in &faces {
            let def = registry.find(&face.action).unwrap();
            let effect = match def.effect {
                Effect::Fixed(effect) => effect.as_str(),
                Effect::OfTool => "the effect of the tool it calls",
            };
            let person = match def.person {
                PersonNeed::None => "no",
                PersonNeed::Presence => "always",
                PersonNeed::PresenceWhenLoosening => "when it loosens something protected",
            };
            let said = flat(&help(&tree, &[&face.noun, &face.verb]));
            let line = format!(
                "Effect: {effect}. Who may run it: {}. A person at this machine is asked: {person}.",
                def.who.as_str()
            );
            assert!(said.contains(&line), "{}: {said}", face.action);
        }
        // Each of the three answers is met at least once among the actions.
        let all: String = faces
            .iter()
            .map(|face| flat(&help(&tree, &[&face.noun, &face.verb])))
            .collect();
        for answer in ["asked: no.", "asked: always.", "asked: when it loosens"] {
            assert!(all.contains(answer), "{answer}");
        }
    }

    #[test]
    fn the_help_of_an_action_not_built_says_so_on_its_first_line() {
        let (registry, tree) = every_action();
        let top = flat(&tree.help_top());
        let mut not_built = 0;
        for face in command_faces(&registry) {
            let def = registry.find(&face.action).unwrap();
            let text = help(&tree, &[&face.noun, &face.verb]);
            let first = text.lines().next().unwrap();
            let noun = flat(&tree.help_noun(&face.noun));
            let marked = format!("{} (not built)", flat(&face.usage));
            if built(def) {
                assert!(
                    text.starts_with(&face.usage[..20.min(face.usage.len())]),
                    "{text}"
                );
                assert!(!first.starts_with("Not built"), "{}: {text}", face.action);
                assert!(!noun.contains(&marked), "{}: {noun}", face.action);
            } else {
                not_built += 1;
                assert!(
                    first.starts_with("Not built yet: a call answers `action_not_built`"),
                    "{}: {text}",
                    face.action
                );
                assert!(
                    first.contains(def.phase),
                    "{}: {first} / {}",
                    face.action,
                    def.phase
                );
                assert!(noun.contains(&marked), "{}: {noun}", face.action);
                assert!(
                    top.contains(&format!("{} (not built)", face.verb)),
                    "{}",
                    face.action
                );
            }
        }
        // The count the readiness section is to give.
        assert!(not_built > 10, "{not_built}");
        // An action the registry gives the client end, and the client end
        // does not carry out, is not built.
        let update = registry.find("self.update").unwrap();
        assert_eq!(update.runs_in, RunsIn::Client);
        assert!(!built(update));
    }

    #[test]
    fn no_help_line_is_wider_than_the_width() {
        let (registry, tree) = every_action();
        for width in [80, 60, 40] {
            let tree = tree.clone().with_width(width);
            let mut texts = vec![tree.help_top()];
            for noun in tree.nouns() {
                texts.push(tree.help_noun(noun));
            }
            for face in command_faces(&registry) {
                texts.push(help(&tree, &[&face.noun, &face.verb]));
            }
            for shortcut in tree.shortcuts() {
                texts.push(tree.help_shortcut(shortcut));
            }
            for word in process_commands() {
                texts.push(tree.help_process(word));
            }
            for text in &texts {
                for line in text.lines() {
                    assert!(
                        line.chars().count() <= width,
                        "{width}: {} > {width}: {line}",
                        line.chars().count()
                    );
                }
            }
        }
        // The width is the environment's `COLUMNS` when it is a number of
        // 40 or more, and 80 otherwise.
        let of = |columns: &str| {
            width_of(&std::collections::BTreeMap::from([(
                "COLUMNS".to_owned(),
                columns.to_owned(),
            )]))
        };
        assert_eq!(of("60"), 60);
        assert_eq!(of("120"), 120);
        assert_eq!(of("39"), 80);
        assert_eq!(of("wide"), 80);
        assert_eq!(width_of(&std::collections::BTreeMap::new()), 80);
    }

    fn words(text: &[&str]) -> Vec<String> {
        text.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn a_flag_for_standard_input_is_not_sent_and_names_the_field_it_fills() {
        let tree = Tree::of(&ActionRegistry::kernel_only());
        let parsed = tree
            .parse(&words(&["secret", "set", "example_name", "--stdin"]))
            .unwrap();
        let Parsed::Invoke(_, steps) = parsed else {
            panic!("{parsed:?}");
        };
        assert_eq!(steps[0].action, "secret.set");
        assert_eq!(steps[0].input, json!({"name": "example_name"}));
        assert_eq!(steps[0].stdin_for.as_deref(), Some("value"));

        // Without the flag nothing is read, and the value is sent as given.
        let parsed = tree
            .parse(&words(&["secret", "set", "example_name", "--value", "v"]))
            .unwrap();
        let Parsed::Invoke(_, steps) = parsed else {
            panic!("{parsed:?}");
        };
        assert_eq!(
            steps[0].input,
            json!({"name": "example_name", "value": "v"})
        );
        assert_eq!(steps[0].stdin_for, None);

        // Both: refused by the names of the two arguments, never by what
        // was given.
        let given = "example-value-given-as-an-argument";
        let refused = tree
            .parse(&words(&[
                "secret",
                "set",
                "example_name",
                "--value",
                given,
                "--stdin",
            ]))
            .unwrap_err();
        assert_eq!(refused.code().as_str(), "schema_invalid");
        assert!(refused.message().contains("--stdin"));
        assert!(refused.message().contains("--value"));
        assert!(!refused.message().contains(given));
        assert!(!refused.next_step().contains(given));

        // A secret written in the wrong place is not quoted back either: as
        // a second word, as the value of the flag that takes none, or as a
        // value of the wrong type.
        for line in [
            &["secret", "set", "example_name", given][..],
            &["secret", "set", "example_name", &format!("--stdin={given}")][..],
            &["run", "list", &format!("--limit={given}")][..],
        ] {
            let refused = tree.parse(&words(line)).unwrap_err();
            assert_eq!(refused.code().as_str(), "schema_invalid", "{line:?}");
            assert!(!refused.message().contains(given), "{refused:?}");
            assert!(!refused.next_step().contains(given), "{refused:?}");
        }
    }

    #[test]
    fn a_shortcut_is_a_command_only_when_every_step_is_built_and_gives_them_all() {
        let registry = ActionRegistry::kernel_only();
        let tree = Tree::of(&registry);
        for shortcut in SHORTCUTS
            .iter()
            .filter(|shortcut| !shortcut.steps.is_empty())
        {
            // The shortcut's own argument, when its steps take one.
            let takes = shortcut.steps.iter().any(|step| step.argument.is_some());
            let mut line = if takes {
                words(&[shortcut.word, "example"])
            } else {
                words(&[shortcut.word])
            };
            if !shortcut.offered(&registry) {
                // Not offered: no command, and no help or completion names it.
                let refused = tree.parse(&line).unwrap_err();
                assert_eq!(refused.code().as_str(), "schema_invalid");
                assert!(
                    refused.message().contains("There is no command"),
                    "{refused:?}"
                );
                assert!(
                    !tree
                        .shortcuts()
                        .any(|offered| offered.word == shortcut.word)
                );
                let top = tree.help_top();
                let listed = top
                    .split("Shortcuts:")
                    .nth(1)
                    .unwrap()
                    .lines()
                    .any(|row| row.split_whitespace().next() == Some(shortcut.word));
                assert!(!listed, "{}", shortcut.word);
                continue;
            }
            // A shortcut of one action takes that action's flags, and a
            // required one is given a value, as its refusal names it.
            let mut parsed = tree.parse(&line);
            while let Err(refused) = &parsed {
                let flag = refused
                    .message()
                    .split_once("needs --")
                    .map(|(_, rest)| rest.trim_end_matches('.').to_owned())
                    .unwrap_or_else(|| panic!("{refused:?}"));
                line.extend([format!("--{flag}"), "example".to_owned()]);
                assert!(line.len() < 12, "{line:?}");
                parsed = tree.parse(&line);
            }
            let parsed = parsed.unwrap();
            let Parsed::Invoke(_, steps) = parsed else {
                panic!("{parsed:?}");
            };
            // Every step, each built.
            let actions: Vec<&str> = steps.iter().map(|step| step.action.as_str()).collect();
            let all: Vec<&str> = shortcut.steps.iter().map(|step| step.action).collect();
            assert_eq!(actions, all, "{}", shortcut.word);
        }
    }
}
