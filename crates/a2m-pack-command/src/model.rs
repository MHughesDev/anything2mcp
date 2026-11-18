//! What the `commands/` files compile to: the pack's own section of the
//! plan, and the payload of each step. The file kind encodes these, the
//! driver decodes them, and the kernel carries them without reading them.
//!
//! Nothing here is a float, and every map is ordered, so the same files
//! give the same bytes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The version of the section's shape.
pub const SECTION_FORMAT: u16 = 1;

/// Everything the `commands/` files of one target say.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    /// One entry for each file: the program it describes.
    pub programs: Vec<Program>,
    /// Every command of every file, by name. A name is unique in a target.
    pub commands: BTreeMap<String, Command>,
}

/// The `program` block of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    /// The file the block was written in, for messages.
    pub file: String,
    /// The surface of kind `program` it runs on.
    pub surface: String,
    /// The known builds: for each platform, digests as 64 hexadecimal
    /// characters.
    pub digests: BTreeMap<String, Vec<String>>,
    /// The version check, when the file declares one.
    pub version: Option<VersionCheck>,
    /// The argument lists discovery may run. `{command}` stands for the
    /// words of a command's name.
    pub help: Vec<Vec<String>>,
    /// The variables the program is given, by name.
    pub env_set: BTreeMap<String, String>,
    /// The names copied from the engine's environment when they are set.
    pub env_pass: Vec<String>,
    /// The limits of every start, unless a command states its own.
    pub limits: Limits,
    /// Patterns that mean the program is asking a person something.
    pub prompts: Vec<String>,
    /// What each exit status means, shared by every command.
    pub exits: BTreeMap<String, ExitMeaning>,
    /// When the program is the interpreter of fixed pairs: the arguments it
    /// is always given before a script's path. Every command of such a
    /// file names its script, and no other command runs on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<Vec<String>>,
}

/// A script the target keeps, run by its file's interpreter as a fixed
/// pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    /// Its path inside the target's folder, names joined by `/`.
    pub path: String,
    /// The SHA-256 of its bytes, as 64 hexadecimal characters.
    pub digest: String,
}

/// The command that reports the program's version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionCheck {
    /// Its arguments.
    pub args: Vec<String>,
    /// The exit statuses that are fine.
    pub exits: Vec<i32>,
    /// The anchored pattern standard output must match, with a group named
    /// `version`.
    pub stdout: String,
    /// The versions the file was written for.
    pub supported: Vec<String>,
}

/// The limits of one start of a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// How long it may run, in milliseconds.
    pub timeout_ms: u32,
    /// The ceiling of standard output, in bytes.
    pub stdout_bytes: u64,
    /// The ceiling of standard error, in bytes.
    pub stderr_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout_ms: 10_000,
            stdout_bytes: 1024 * 1024,
            stderr_bytes: 64 * 1024,
        }
    }
}

/// What an exit status means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitMeaning {
    /// The command did its work.
    Ok,
    /// The record it names does not exist. Nothing changed.
    Absent,
    /// The program refused the arguments as badly formed. Nothing changed.
    Input,
    /// The program refused the values by a rule of its own. Nothing changed.
    Rejected,
    /// The record is not as the caller last saw it. Nothing changed.
    Stale,
    /// Another start holds what this one needs. Nothing changed.
    Busy,
    /// Try again later. Nothing changed.
    Transient,
    /// The program is not signed in. Nothing changed.
    SessionExpired,
}

impl ExitMeaning {
    /// The words a file may write.
    pub const NAMES: &'static [&'static str] = &[
        "ok",
        "absent",
        "input",
        "rejected",
        "stale",
        "busy",
        "transient",
        "session_expired",
    ];

    /// The meaning a word names.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "ok" => Self::Ok,
            "absent" => Self::Absent,
            "input" => Self::Input,
            "rejected" => Self::Rejected,
            "stale" => Self::Stale,
            "busy" => Self::Busy,
            "transient" => Self::Transient,
            "session_expired" => Self::SessionExpired,
            _ => return None,
        })
    }
}

/// Whether a command reads, writes, or cannot be taken back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// It changes nothing.
    Read,
    /// It creates, changes, or removes a record.
    Write,
    /// It cannot be taken back.
    Irreversible,
}

/// One command of a program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// Which entry of [`Section::programs`] it belongs to.
    pub program: u32,
    /// Its declared effect.
    pub effect: Effect,
    /// Its params, by name.
    pub params: BTreeMap<String, Param>,
    /// Its argument list, element by element.
    pub args: Vec<Arg>,
    /// What each exit status means: the program's, and what the command adds.
    pub exits: BTreeMap<String, ExitMeaning>,
    /// The form of its standard output.
    pub output: Output,
    /// The output field that holds the key the program assigned.
    pub assigns: Option<String>,
    /// Its limits.
    pub limits: Limits,
    /// The script its program, an interpreter, is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,
}

/// What kind of value a param takes, which decides how it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    /// Text: needs a pattern or a closed list.
    Text,
    /// A whole number.
    Int,
    /// An exact number.
    Decimal,
    /// True or false.
    Bool,
    /// A calendar day, written `YYYY-MM-DD`.
    Date,
}

/// One param of a command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    /// The kind of its value, from its field type.
    pub kind: ParamKind,
    /// Whether a value must be given.
    pub required: bool,
    /// Whether it takes a list of values, for an `each` element.
    pub list: bool,
    /// The anchored pattern a text value must match.
    pub pattern: Option<String>,
    /// The closed list a text value must be in, from a choice field type.
    pub choices: Vec<String>,
}

/// One entry of a command's argument list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "snake_case", deny_unknown_fields)]
pub enum Arg {
    /// One element, as written.
    Literal {
        /// The element.
        text: String,
    },
    /// The end-of-options marker, `--`.
    EndOfOptions,
    /// One element: the param's value.
    Value {
        /// The param.
        param: String,
    },
    /// The flag and the param's value: two elements, or one when joined.
    /// None when the param is optional and absent.
    Flag {
        /// The flag, as written.
        flag: String,
        /// The param.
        param: String,
        /// What joins the flag and the value into one element.
        join: Option<String>,
    },
    /// The flag alone, when the boolean param is true.
    When {
        /// The flag.
        flag: String,
        /// The boolean param.
        param: String,
    },
    /// The flag and a value, for each item of a list param.
    Each {
        /// The flag.
        flag: String,
        /// The list param.
        param: String,
        /// What joins the flag and each value into one element.
        join: Option<String>,
    },
    /// The flag and the record's stamp.
    Stamp {
        /// The flag.
        flag: String,
        /// What joins the flag and the stamp into one element.
        join: Option<String>,
    },
}

/// The kind of one field of a JSON document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonKind {
    /// Text.
    Text,
    /// A whole number.
    Int,
    /// An exact number.
    Decimal,
    /// True or false.
    Bool,
}

/// One declared field of a JSON document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonField {
    /// The kind of its value.
    pub kind: JsonKind,
    /// Whether the field must be there and hold a value.
    pub required: bool,
}

/// How a pattern is laid over standard output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Matching {
    /// The output is one line, and it matches.
    OneLine,
    /// Every line matches; each is a row.
    EveryLine,
    /// The whole output matches.
    Whole,
}

/// The declared form of a command's standard output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case", deny_unknown_fields)]
pub enum Output {
    /// Nothing is read from the output.
    Ignored,
    /// One JSON document, or one for each line.
    Json {
        /// Whether each line is a document, and so a row.
        lines: bool,
        /// The fields of a document. Any other field is drift.
        fields: BTreeMap<String, JsonField>,
    },
    /// Rows of columns split by a tab.
    Table {
        /// The header line, when the program prints one. It must match.
        header: bool,
        /// The columns, in order.
        columns: Vec<String>,
    },
    /// An anchored pattern with named groups.
    Pattern {
        /// The pattern.
        pattern: String,
        /// How it is laid over the output.
        matching: Matching,
    },
}

impl Output {
    /// The names of the fields a record or a row holds.
    #[must_use]
    pub fn fields(&self) -> Vec<String> {
        match self {
            Self::Ignored => Vec::new(),
            Self::Json { fields, .. } => fields.keys().cloned().collect(),
            Self::Table { columns, .. } => columns.clone(),
            Self::Pattern { pattern, .. } => regex::Regex::new(pattern)
                .map(|compiled| {
                    compiled
                        .capture_names()
                        .flatten()
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// Which declared form of a program a discovery step runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case", deny_unknown_fields)]
pub enum Probe {
    /// The version command.
    Version,
    /// One of the help forms.
    Help {
        /// Which entry of the program's `help` list.
        index: u32,
        /// The command whose words stand for `{command}`.
        command: Option<String>,
    },
}

/// The pack's own details of one step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepPayload {
    /// A step of one run of a command. The command is the step's reference.
    Command {
        /// The params that take the parts of the record's key, in order.
        key_params: Vec<String>,
        /// Whether the rows of the output must be exactly one record: none
        /// is `absent`, more is ambiguous.
        one: bool,
    },
    /// A declared help or version form, which discovery may run.
    Probe {
        /// Which entry of [`Section::programs`].
        program: u32,
        /// Which form.
        probe: Probe,
    },
}
