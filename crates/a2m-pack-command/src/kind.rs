//! The `commands/` file kind: its schema, the names it defines, and the
//! compile of its documents into the pack's section of the plan.
//!
//! The rules of spec section 306 that a file alone can break are checked
//! here, each under a code of the pack's own, with the place and a fix.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{Digest, FieldBase, FileKindName, Hazard, PackPayload, SurfaceKind};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind, JsonSchema,
    KindContext, Located, Place, Stage, expr_text,
};

use crate::codes::{self, FILE_KIND, FOLDER, SURFACE_KIND, lint};
use crate::model::{
    Arg, Command, Effect, ExitMeaning, JsonField, JsonKind, Limits, Matching, Output, Param,
    ParamKind, Program, SECTION_FORMAT, Section, VersionCheck,
};
use crate::pattern;

/// The file names a program surface may not have: shells, and interpreters
/// that take code as an argument. Compared without a `.exe` suffix and
/// without regard to case.
pub(crate) const SHELLS_AND_INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "zsh",
    "ksh",
    "csh",
    "tcsh",
    "fish",
    "ash",
    "busybox",
    "cmd",
    "command",
    "powershell",
    "pwsh",
    "wscript",
    "cscript",
    "python",
    "python2",
    "python3",
    "perl",
    "ruby",
    "node",
    "nodejs",
    "deno",
    "bun",
    "php",
    "lua",
    "tclsh",
    "osascript",
    "awk",
    "gawk",
    "sed",
    "env",
    "xargs",
    "nohup",
    "sudo",
    "su",
    "ssh",
    "eval",
    "exec",
];

/// Flags that hand their value to something that runs it.
const CODE_FLAGS: &[&str] = &[
    "-c",
    "-e",
    "--eval",
    "--exec",
    "--execute",
    "--command",
    "--expression",
    "--run",
    "--script",
];

/// Whether a program's path names a shell or an interpreter.
pub(crate) fn is_shell_or_interpreter(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .strip_suffix(".exe")
        .or_else(|| lower.strip_suffix(".com"))
        .unwrap_or(&lower);
    // A versioned name, such as `python3.12`, is the same program.
    let base = stem.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    SHELLS_AND_INTERPRETERS.contains(&stem)
        || SHELLS_AND_INTERPRETERS.contains(&base)
        || lower.ends_with(".bat")
        || lower.ends_with(".cmd")
        || lower.ends_with(".ps1")
        || lower.ends_with(".sh")
}

fn limits_schema() -> JsonSchema {
    JsonSchema::object()
        .optional("timeout", JsonSchema::scalar())
        .optional("stdout", JsonSchema::scalar())
        .optional("stderr", JsonSchema::scalar())
}

fn exits_schema() -> JsonSchema {
    JsonSchema::map_of(JsonSchema::words(ExitMeaning::NAMES))
}

/// The schema of one `commands/` file.
pub(crate) fn schema() -> JsonSchema {
    let version = JsonSchema::object()
        .required("args", JsonSchema::non_empty_list(JsonSchema::scalar()))
        .required(
            "expect",
            JsonSchema::object()
                .optional("exit", JsonSchema::non_empty_list(JsonSchema::int()))
                .required("stdout", JsonSchema::text()),
        )
        .required(
            "supported",
            JsonSchema::non_empty_list(JsonSchema::scalar()),
        );
    let program = JsonSchema::object()
        .required(
            "digests",
            JsonSchema::map_of(JsonSchema::non_empty_list(JsonSchema::text())),
        )
        .optional("version", version)
        .optional(
            "help",
            JsonSchema::list(JsonSchema::non_empty_list(JsonSchema::scalar())),
        )
        .optional(
            "env",
            JsonSchema::object()
                .optional("set", JsonSchema::map_of(JsonSchema::scalar()))
                .optional("pass", JsonSchema::list(JsonSchema::text())),
        )
        .optional("workdir", JsonSchema::words(&["fresh"]))
        .optional("stdin", JsonSchema::words(&["none"]))
        .optional("limits", limits_schema())
        .optional("prompts", JsonSchema::list(JsonSchema::text()))
        .optional("exits", exits_schema());
    let param = JsonSchema::object()
        .required("type", JsonSchema::field_type())
        .optional("required", JsonSchema::bool())
        .optional("list", JsonSchema::bool())
        .optional("pattern", JsonSchema::text())
        .optional("code", JsonSchema::bool());
    let arg = JsonSchema::either(vec![
        JsonSchema::scalar(),
        JsonSchema::object()
            .optional("param", JsonSchema::name())
            .optional("flag", JsonSchema::text())
            .optional("join", JsonSchema::text())
            .optional("when", JsonSchema::name())
            .optional("each", JsonSchema::name())
            .optional("stamp", JsonSchema::bool()),
    ]);
    let json_field = JsonSchema::either(vec![
        JsonSchema::words(JSON_KINDS),
        JsonSchema::object()
            .required("type", JsonSchema::words(JSON_KINDS))
            .optional("required", JsonSchema::bool()),
    ]);
    let output = JsonSchema::either(vec![
        JsonSchema::words(&["none"]),
        JsonSchema::object()
            .optional(
                "json",
                JsonSchema::object()
                    .optional("lines", JsonSchema::bool())
                    .required("fields", JsonSchema::map_of(json_field)),
            )
            .optional(
                "table",
                JsonSchema::object()
                    .optional("separator", JsonSchema::words(&["tab"]))
                    .optional("header", JsonSchema::non_empty_list(JsonSchema::text()))
                    .optional("columns", JsonSchema::non_empty_list(JsonSchema::text())),
            )
            .optional("pattern", JsonSchema::text())
            .optional(
                "match",
                JsonSchema::words(&["one_line", "every_line", "whole"]),
            ),
    ]);
    let command = JsonSchema::object()
        .required(
            "effect",
            JsonSchema::words(&["read", "write", "irreversible"]),
        )
        .optional("hazard", JsonSchema::words(Hazard::NAMES))
        .optional("params", JsonSchema::map_of(param))
        .required("args", JsonSchema::list(arg))
        .optional("exits", exits_schema())
        .required("output", output)
        .optional("assigns", JsonSchema::map_of(JsonSchema::text()))
        .optional(
            "reaches",
            JsonSchema::object().optional("network", JsonSchema::list(JsonSchema::text())),
        )
        .optional("limits", limits_schema());
    JsonSchema::object()
        .optional("surface", JsonSchema::surface())
        .required("program", program)
        .required("commands", JsonSchema::map_of(command))
}

const JSON_KINDS: &[&str] = &["text", "integer", "decimal", "boolean"];

/// The `commands/` file kind.
pub(crate) struct CommandKind {
    schema: JsonSchema,
    kinds: Vec<SurfaceKind>,
}

impl CommandKind {
    pub(crate) fn new() -> Self {
        Self {
            schema: schema(),
            kinds: vec![SURFACE_KIND],
        }
    }
}

impl FileKind for CommandKind {
    fn name(&self) -> FileKindName {
        FILE_KIND
    }

    fn surface_kinds(&self) -> &[SurfaceKind] {
        &self.kinds
    }

    fn folder(&self) -> &str {
        FOLDER
    }

    fn schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> {
        doc.get("commands")
            .map(Located::entries)
            .unwrap_or_default()
            .iter()
            .map(|entry| {
                Located::new(
                    DefinedName {
                        name: entry.key.value.clone(),
                    },
                    entry.key.at.clone(),
                )
            })
            .collect()
    }

    fn expressions(&self, _doc: &Located<Doc>) -> Vec<ExprSite> {
        Vec::new()
    }

    fn compile(
        &self,
        docs: &[Located<Doc>],
        cx: &KindContext<'_>,
    ) -> Result<CompiledSection, Diagnostics> {
        let mut found = Found::default();
        let mut section = Section::default();
        for doc in docs {
            compile_file(doc, cx, &mut section, &mut found);
        }
        if !found.problems.is_empty() {
            return Err(found.problems);
        }
        PackPayload::encode(&section)
            .map(|payload| CompiledSection {
                format: SECTION_FORMAT,
                payload,
            })
            .map_err(|failure| {
                let at = docs.first().map_or_else(
                    || Place::start_of(&a2m_files::RelPath::target_file()),
                    |doc| doc.at.clone(),
                );
                Diagnostics::from(Diagnostic::error(
                    lint("not_encodable"),
                    Stage::Lower,
                    &at,
                    failure.message().to_owned(),
                    "Keep every number in a `commands/` file whole.",
                ))
            })
    }
}

/// The problems found so far.
#[derive(Default)]
struct Found {
    problems: Diagnostics,
}

impl Found {
    fn bad(&mut self, rule: &'static str, at: &Place, message: String, fix: &str) {
        self.problems
            .push(Diagnostic::error(lint(rule), Stage::Lint, at, message, fix));
    }
}

/// A scalar as the text a program is given.
fn scalar(doc: &Located<Doc>) -> String {
    expr_text(doc).unwrap_or_default()
}

fn texts(doc: Option<&Located<Doc>>) -> Vec<String> {
    doc.map(Located::items)
        .unwrap_or_default()
        .iter()
        .map(scalar)
        .collect()
}

/// A length of time: a whole number of milliseconds, or a number with
/// `ms`, `s`, or `m` after it.
fn duration_ms(doc: &Located<Doc>) -> Option<u32> {
    let text = scalar(doc);
    let (digits, factor) = if let Some(digits) = text.strip_suffix("ms") {
        (digits, 1_u64)
    } else if let Some(digits) = text.strip_suffix('s') {
        (digits, 1_000)
    } else if let Some(digits) = text.strip_suffix('m') {
        (digits, 60_000)
    } else {
        (text.as_str(), 1)
    };
    let number: u64 = digits.trim().parse().ok()?;
    u32::try_from(number.checked_mul(factor)?)
        .ok()
        .filter(|ms| *ms > 0)
}

/// A size: a whole number of bytes, or a number with `B`, `KiB`, or `MiB`
/// after it.
fn size_bytes(doc: &Located<Doc>) -> Option<u64> {
    let text = scalar(doc);
    let (digits, factor) = if let Some(digits) = text.strip_suffix("MiB") {
        (digits, 1024 * 1024_u64)
    } else if let Some(digits) = text.strip_suffix("KiB") {
        (digits, 1024)
    } else if let Some(digits) = text.strip_suffix('B') {
        (digits, 1)
    } else {
        (text.as_str(), 1)
    };
    let number: u64 = digits.trim().parse().ok()?;
    number.checked_mul(factor).filter(|bytes| *bytes > 0)
}

fn limits(doc: Option<&Located<Doc>>, base: Limits, found: &mut Found) -> Limits {
    let mut limits = base;
    let Some(doc) = doc else {
        return limits;
    };
    if let Some(timeout) = doc.get("timeout") {
        match duration_ms(timeout) {
            Some(ms) => limits.timeout_ms = ms,
            None => found.bad(
                "limit_malformed",
                &timeout.at,
                "A time limit is a whole number of milliseconds, or a number followed by `ms`, `s`, or `m`.".to_owned(),
                "Write the limit as, for example, `10s`.",
            ),
        }
    }
    for (key, into) in [
        ("stdout", &mut limits.stdout_bytes),
        ("stderr", &mut limits.stderr_bytes),
    ] {
        if let Some(size) = doc.get(key) {
            match size_bytes(size) {
                Some(bytes) => *into = bytes,
                None => found.bad(
                    "limit_malformed",
                    &size.at,
                    "A ceiling is a whole number of bytes, or a number followed by `B`, `KiB`, or `MiB`.".to_owned(),
                    "Write the ceiling as, for example, `1MiB`.",
                ),
            }
        }
    }
    limits
}

fn exits(
    doc: Option<&Located<Doc>>,
    base: &BTreeMap<String, ExitMeaning>,
    found: &mut Found,
) -> BTreeMap<String, ExitMeaning> {
    let mut exits = base.clone();
    for entry in doc.map(Located::entries).unwrap_or_default() {
        let status = entry.key.value.parse::<i32>().ok();
        let meaning = entry.value.as_text().and_then(ExitMeaning::from_word);
        let (Some(status), Some(meaning)) = (status, meaning) else {
            found.bad(
                "exit_malformed",
                &entry.key.at,
                format!(
                    "`{}` is not an exit status with a meaning.",
                    codes::shown(&entry.key.value)
                ),
                "Write a whole number and one of the meanings, such as `3: absent`.",
            );
            continue;
        };
        let key = status.to_string();
        if base.get(&key).is_some_and(|shared| *shared != meaning) {
            found.bad(
                "exit_changed",
                &entry.key.at,
                format!(
                    "The exit status {status} has a meaning for the whole program, and the command gives it another."
                ),
                "A command may add an exit status, never change one; remove the entry or correct the program's.",
            );
            continue;
        }
        exits.insert(key, meaning);
    }
    exits
}

/// Checks a pattern and gives it back when it compiles and is anchored.
fn checked_pattern(
    doc: &Located<Doc>,
    both_ends: bool,
    found: &mut Found,
) -> Option<(String, regex::Regex)> {
    let text = doc.as_text().unwrap_or_default();
    let compiled = match pattern::compile(text) {
        Ok(compiled) => compiled,
        Err(why) => {
            found.bad(
                "pattern_invalid",
                &doc.at,
                format!("The pattern does not compile: {}.", codes::shown(&why)),
                "Correct the pattern; it is a regular expression with named groups written `(?<name>...)`.",
            );
            return None;
        }
    };
    let anchored = text.starts_with('^') && (!both_ends || pattern::ends_anchored(text));
    if !anchored {
        found.bad(
            "pattern_not_anchored",
            &doc.at,
            "The pattern is not anchored, so text around a match would pass unseen.".to_owned(),
            if both_ends {
                "Start the pattern with `^` and end it with `$`."
            } else {
                "Start the pattern with `^`."
            },
        );
        return None;
    }
    Some((text.to_owned(), compiled))
}

fn program(doc: &Located<Doc>, file: &str, surface: &str, found: &mut Found) -> Program {
    let mut digests: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in doc
        .get("digests")
        .map(Located::entries)
        .unwrap_or_default()
    {
        let mut listed = Vec::new();
        for item in entry.value.items() {
            let parsed = item
                .as_text()
                .and_then(|text| text.strip_prefix("sha256:"))
                .and_then(|hex| hex.parse::<Digest>().ok());
            match parsed {
                Some(digest) => listed.push(digest.to_string()),
                None => found.bad(
                    "digest_invalid",
                    &item.at,
                    "A digest is `sha256:` and 64 lowercase hexadecimal characters.".to_owned(),
                    "Write the SHA-256 of the program's file, as `sha256:` and its 64 characters.",
                ),
            }
        }
        digests.insert(entry.key.value.clone(), listed);
    }
    let version = doc.get("version").and_then(|version| {
        let expect = version.get("expect")?;
        let stdout = expect.get("stdout")?;
        let (text, compiled) = checked_pattern(stdout, true, found)?;
        if !compiled.capture_names().flatten().any(|name| name == "version") {
            found.bad(
                "pattern_invalid",
                &stdout.at,
                "The version pattern has no group named `version`.".to_owned(),
                "Name the part that is the version, as in `(?<version>\\d+\\.\\d+)`.",
            );
            return None;
        }
        let statuses: Vec<i32> = expect
            .get("exit")
            .map(Located::items)
            .unwrap_or_default()
            .iter()
            .filter_map(|item| item.as_int().and_then(|status| i32::try_from(status).ok()))
            .collect();
        Some(VersionCheck {
            args: texts(version.get("args")),
            exits: if statuses.is_empty() {
                vec![0]
            } else {
                statuses
            },
            stdout: text,
            supported: texts(version.get("supported")),
        })
    });
    let help: Vec<Vec<String>> = doc
        .get("help")
        .map(Located::items)
        .unwrap_or_default()
        .iter()
        .map(|form| texts(Some(form)))
        .collect();
    let env = doc.get("env");
    let mut env_set = BTreeMap::new();
    for entry in env
        .and_then(|env| env.get("set"))
        .map(Located::entries)
        .unwrap_or_default()
    {
        let name = &entry.key.value;
        let value = scalar(&entry.value);
        if name.is_empty()
            || name.contains(['=', '\0'])
            || value.contains('\0')
            || name == a2m_provider::OWNER_MARK_VARIABLE
        {
            found.bad(
                "env_malformed",
                &entry.key.at,
                "A variable has a name with no `=`, a value with no NUL character, and is not the engine's own."
                    .to_owned(),
                "Correct the variable's name or value.",
            );
            continue;
        }
        if value.contains("{session.state_dir}") {
            found.bad(
                "state_dir_not_built",
                &entry.value.at,
                "`{session.state_dir}` is not built yet: this version of the pack gives a program no session folder."
                    .to_owned(),
                "Name a folder the engine owns for the program, or leave the variable out until the pack offers the session folder.",
            );
            continue;
        }
        env_set.insert(name.clone(), value);
    }
    let env_pass = texts(env.and_then(|env| env.get("pass")));
    let mut prompts = Vec::new();
    for prompt in doc.get("prompts").map(Located::items).unwrap_or_default() {
        if let Some((text, _)) = checked_pattern(prompt, false, found) {
            prompts.push(text);
        }
    }
    Program {
        file: file.to_owned(),
        surface: surface.to_owned(),
        digests,
        version,
        help,
        env_set,
        env_pass,
        limits: limits(doc.get("limits"), Limits::default(), found),
        prompts,
        exits: exits(doc.get("exits"), &BTreeMap::new(), found),
    }
}

fn param(
    name: &str,
    doc: &Located<Doc>,
    cx: &KindContext<'_>,
    found: &mut Found,
) -> Option<Param> {
    let type_doc = doc.get("type")?;
    let field_type = cx.target().field_type(type_doc.as_text()?)?;
    let (kind, choices) = match field_type.plan.base {
        FieldBase::Text => (ParamKind::Text, Vec::new()),
        FieldBase::Choice => (
            ParamKind::Text,
            field_type
                .plan
                .codes
                .iter()
                .map(|code| code.code.clone())
                .collect(),
        ),
        FieldBase::Integer => (ParamKind::Int, Vec::new()),
        FieldBase::Decimal | FieldBase::Money | FieldBase::Measurement => {
            (ParamKind::Decimal, Vec::new())
        }
        FieldBase::Boolean => (ParamKind::Bool, Vec::new()),
        FieldBase::Date => (ParamKind::Date, Vec::new()),
        FieldBase::Datetime | FieldBase::Time => {
            found.bad(
                "param_type_unsupported",
                &type_doc.at,
                format!(
                    "The param `{name}` is a time, and this version of the pack does not write one as an argument."
                ),
                "Use a date, a number, or text with a pattern.",
            );
            return None;
        }
    };
    if doc.get("code").and_then(Located::as_bool) == Some(true) {
        found.bad(
            "code_argument",
            &doc.at,
            format!("The param `{name}` is marked as code the program would run."),
            "Remove the command: a program is never given text to run.",
        );
        return None;
    }
    let pattern = match doc.get("pattern") {
        Some(pattern) => Some(checked_pattern(pattern, true, found)?.0),
        None => None,
    };
    if kind == ParamKind::Text && pattern.is_none() && choices.is_empty() {
        found.bad(
            "text_param_open",
            &doc.at,
            format!(
                "The text param `{name}` has no anchored pattern and its field type has no closed list of values."
            ),
            "Add `pattern:` with an anchored pattern, such as `'^[A-Z]{2}-\\d{4}$'`, or use a `choice` field type.",
        );
        return None;
    }
    Some(Param {
        kind,
        required: doc.get("required").and_then(Located::as_bool) != Some(false),
        list: doc.get("list").and_then(Located::as_bool) == Some(true),
        pattern,
        choices,
    })
}

fn arg(doc: &Located<Doc>, params: &BTreeMap<String, Param>, found: &mut Found) -> Option<Arg> {
    if !matches!(doc.value, Doc::Map(_)) {
        let text = scalar(doc);
        return Some(if text == "--" {
            Arg::EndOfOptions
        } else {
            Arg::Literal { text }
        });
    }
    let text_of = |key: &str| doc.text(key).map(str::to_owned);
    let (flag, join) = (text_of("flag"), text_of("join"));
    let stamp = doc.get("stamp").and_then(Located::as_bool) == Some(true);
    let named: Vec<(&str, String)> = ["param", "when", "each"]
        .into_iter()
        .filter_map(|key| text_of(key).map(|name| (key, name)))
        .collect();
    let malformed = |found: &mut Found| {
        found.bad(
            "arg_malformed",
            &doc.at,
            "An argument entry is one of `{param}`, `{flag, param}`, `{flag, when}`, `{flag, each}`, or `{flag, stamp: true}`, with `join` only beside a value."
                .to_owned(),
            "Write the entry in one of those forms.",
        );
        None
    };
    if let Some(flag) = &flag
        && (!flag.starts_with('-') || flag == "--" || flag.contains('\0'))
    {
        found.bad(
            "arg_malformed",
            &doc.at,
            "A flag starts with `-` and is not the end-of-options marker.".to_owned(),
            "Write the flag as the program's help shows it, such as `--name`.",
        );
        return None;
    }
    let known = |name: &str, found: &mut Found| -> Option<Param> {
        let known = params.get(name).cloned();
        if known.is_none() {
            found.bad(
                "param_unknown",
                &doc.at,
                format!("The argument list names the param `{name}`, which the command does not declare."),
                "Declare the param under `params`, or correct the name.",
            );
        }
        known
    };
    match (flag, named.as_slice(), stamp) {
        (None, [("param", name)], false) if join.is_none() => {
            let declared = known(name, found)?;
            if declared.list || declared.kind == ParamKind::Bool {
                return malformed(found);
            }
            Some(Arg::Value {
                param: name.clone(),
            })
        }
        (Some(flag), [("param", name)], false) => {
            let declared = known(name, found)?;
            if declared.list || declared.kind == ParamKind::Bool {
                return malformed(found);
            }
            if CODE_FLAGS.contains(&flag.as_str()) {
                found.bad(
                    "code_flag",
                    &doc.at,
                    format!("The flag `{flag}` hands its value to something that runs it."),
                    "Remove the entry: a program is never given text to run.",
                );
                return None;
            }
            Some(Arg::Flag {
                flag,
                param: name.clone(),
                join,
            })
        }
        (Some(flag), [("when", name)], false) if join.is_none() => {
            let declared = known(name, found)?;
            if declared.kind != ParamKind::Bool || declared.list {
                return malformed(found);
            }
            Some(Arg::When {
                flag,
                param: name.clone(),
            })
        }
        (Some(flag), [("each", name)], false) => {
            let declared = known(name, found)?;
            if !declared.list || declared.kind == ParamKind::Bool {
                return malformed(found);
            }
            Some(Arg::Each {
                flag,
                param: name.clone(),
                join,
            })
        }
        (Some(flag), [], true) => Some(Arg::Stamp { flag, join }),
        _ => malformed(found),
    }
}

fn output(doc: &Located<Doc>, found: &mut Found) -> Option<Output> {
    if doc.as_text() == Some("none") {
        return Some(Output::Ignored);
    }
    let forms: Vec<&str> = ["json", "table", "pattern"]
        .into_iter()
        .filter(|key| doc.get(key).is_some())
        .collect();
    let one_form = |found: &mut Found| {
        found.bad(
            "output_malformed",
            &doc.at,
            "A command declares one output form: `json`, `table`, `pattern`, or the word `none`; `match` goes with `pattern` only."
                .to_owned(),
            "Keep one form.",
        );
        None
    };
    match forms.as_slice() {
        ["json"] if doc.get("match").is_none() => {
            let json = doc.get("json")?;
            let mut fields = BTreeMap::new();
            for entry in json.get("fields").map(Located::entries).unwrap_or_default() {
                let (word, required) = match &entry.value.value {
                    Doc::Map(_) => (
                        entry.value.text("type"),
                        entry.value.get("required").and_then(Located::as_bool) != Some(false),
                    ),
                    _ => (entry.value.as_text(), true),
                };
                let kind = match word {
                    Some("text") => JsonKind::Text,
                    Some("integer") => JsonKind::Int,
                    Some("decimal") => JsonKind::Decimal,
                    Some("boolean") => JsonKind::Bool,
                    _ => continue,
                };
                fields.insert(entry.key.value.clone(), JsonField { kind, required });
            }
            Some(Output::Json {
                lines: json.get("lines").and_then(Located::as_bool) == Some(true),
                fields,
            })
        }
        ["table"] if doc.get("match").is_none() => {
            let table = doc.get("table")?;
            let (header, columns) = match (table.get("header"), table.get("columns")) {
                (Some(header), None) => (true, texts(Some(header))),
                (None, Some(columns)) => (false, texts(Some(columns))),
                _ => {
                    found.bad(
                        "output_malformed",
                        &table.at,
                        "A table names its columns once: by `header`, when the program prints a header line, or by `columns`, when it does not."
                            .to_owned(),
                        "Keep `header` or `columns`.",
                    );
                    return None;
                }
            };
            let distinct: BTreeSet<&String> = columns.iter().collect();
            if distinct.len() != columns.len() || columns.iter().any(|name| name.contains('\t')) {
                found.bad(
                    "output_malformed",
                    &table.at,
                    "A table's columns have distinct names with no tab in them.".to_owned(),
                    "Name each column once.",
                );
                return None;
            }
            Some(Output::Table { header, columns })
        }
        ["pattern"] => {
            let (pattern, compiled) = checked_pattern(doc.get("pattern")?, true, found)?;
            if compiled.capture_names().flatten().next().is_none() {
                found.bad(
                    "pattern_invalid",
                    &doc.at,
                    "The output pattern names no group, so it gives no value.".to_owned(),
                    "Name each value, as in `(?<quantity>\\d+)`; or write `output: none`.",
                );
                return None;
            }
            Some(Output::Pattern {
                pattern,
                matching: match doc.text("match") {
                    Some("every_line") => Matching::EveryLine,
                    Some("whole") => Matching::Whole,
                    _ => Matching::OneLine,
                },
            })
        }
        _ => one_form(found),
    }
}

#[allow(clippy::too_many_lines)] // one field after another, each a few lines
fn command(
    name: &str,
    doc: &Located<Doc>,
    index: u32,
    shared: &Program,
    cx: &KindContext<'_>,
    found: &mut Found,
) -> Option<Command> {
    let before = found.problems.len();
    let effect = match doc.text("effect") {
        Some("read") => Effect::Read,
        Some("write") => Effect::Write,
        _ => Effect::Irreversible,
    };
    let hazard = doc.text("hazard").unwrap_or("none");
    let reaches = doc
        .get("reaches")
        .and_then(|reaches| reaches.get("network"))
        .is_some_and(|network| !network.items().is_empty());
    if reaches && hazard == "none" {
        found.bad(
            "reaches_without_hazard",
            &doc.at,
            format!("The command `{name}` reaches outside the target and declares no hazard."),
            "Add `hazard: sends_outside`, and `effect: irreversible`.",
        );
    }
    if hazard != "none" && effect != Effect::Irreversible {
        found.bad(
            "hazard_needs_irreversible",
            &doc.at,
            format!("The command `{name}` declares the hazard `{hazard}` and is not `irreversible`."),
            "Write `effect: irreversible`: what leaves, or what is lost beyond the record, cannot be taken back.",
        );
    }
    let mut params = BTreeMap::new();
    for entry in doc.get("params").map(Located::entries).unwrap_or_default() {
        if let Some(declared) = param(&entry.key.value, &entry.value, cx, found) {
            params.insert(entry.key.value.clone(), declared);
        }
    }
    let params_fine = found.problems.len() == before;
    let mut args = Vec::new();
    if params_fine {
        for item in doc.get("args").map(Located::items).unwrap_or_default() {
            if let Some(entry) = arg(item, &params, found) {
                args.push(entry);
            }
        }
        let used: BTreeSet<&str> = args
            .iter()
            .filter_map(|entry| match entry {
                Arg::Value { param }
                | Arg::Flag { param, .. }
                | Arg::When { param, .. }
                | Arg::Each { param, .. } => Some(param.as_str()),
                Arg::Literal { .. } | Arg::EndOfOptions | Arg::Stamp { .. } => None,
            })
            .collect();
        if let Some(unused) = params.keys().find(|name| !used.contains(name.as_str())) {
            found.bad(
                "param_unused",
                &doc.at,
                format!("The command `{name}` declares the param `{unused}` and no argument carries it."),
                "Add an argument entry for the param, or remove it.",
            );
        }
        if args
            .iter()
            .filter(|entry| matches!(entry, Arg::Stamp { .. }))
            .count()
            > 1
        {
            found.bad(
                "arg_malformed",
                &doc.at,
                format!("The command `{name}` carries the record's stamp twice."),
                "Keep one `stamp: true` entry.",
            );
        }
    }
    let exits = exits(doc.get("exits"), &shared.exits, found);
    if !exits.values().any(|meaning| *meaning == ExitMeaning::Ok) {
        found.bad(
            "no_ok_exit",
            &doc.at,
            format!("No exit status of the command `{name}` means `ok`."),
            "Declare one, usually `0: ok`, under the program's `exits`.",
        );
    }
    let output = doc.get("output").and_then(|output| self::output(output, found));
    let assigns = match doc.get("assigns").map(Located::entries) {
        None | Some([]) => None,
        Some([one]) => {
            let field = one
                .value
                .as_text()
                .and_then(|text| text.strip_prefix("output."))
                .map(str::to_owned);
            let known = field.as_ref().is_some_and(|field| {
                output
                    .as_ref()
                    .is_some_and(|output| output.fields().contains(field))
            });
            if !known {
                found.bad(
                    "assigns_unknown",
                    &one.value.at,
                    "`assigns` names a field the output form does not declare.".to_owned(),
                    "Write `output.` and the name of a declared field, such as `output.sku`.",
                );
            }
            field
        }
        Some([_, second, ..]) => {
            found.bad(
                "assigns_unknown",
                &second.key.at,
                "This version of the pack takes one assigned key from a command.".to_owned(),
                "Keep the one field that identifies the record.",
            );
            None
        }
    };
    let limits = limits(doc.get("limits"), shared.limits, found);
    if found.problems.len() != before {
        return None;
    }
    Some(Command {
        program: index,
        effect,
        params,
        args,
        exits,
        output: output?,
        assigns,
        limits,
    })
}

fn compile_file(
    doc: &Located<Doc>,
    cx: &KindContext<'_>,
    section: &mut Section,
    found: &mut Found,
) {
    let file = doc.at.file.as_str().to_owned();
    let surface = match cx.surface_of(doc) {
        Some(surface) if *surface.kind == SURFACE_KIND => surface,
        _ => {
            found.bad(
                "surface_missing",
                &doc.at,
                "The file's commands run on no surface of kind `program`.".to_owned(),
                "Declare a surface with `kind: program` and a `path` in `target.yaml`, and name it with `surface:` at the top of this file.",
            );
            return;
        }
    };
    let Some(block) = doc.get("program") else {
        return;
    };
    let shared = program(block, &file, surface.name.as_str(), found);
    // Two files that say the same of one program are one program, so that
    // its version is asked once.
    let same = section.programs.iter().position(|known| {
        Program {
            file: shared.file.clone(),
            ..known.clone()
        } == shared
    });
    let index = u32::try_from(same.unwrap_or(section.programs.len())).unwrap_or(u32::MAX);
    for entry in doc.get("commands").map(Located::entries).unwrap_or_default() {
        if let Some(compiled) = command(&entry.key.value, &entry.value, index, &shared, cx, found)
        {
            section.commands.insert(entry.key.value.clone(), compiled);
        }
    }
    if same.is_none() {
        section.programs.push(shared);
    }
}
