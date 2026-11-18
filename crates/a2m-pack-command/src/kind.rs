//! The `commands/` file kind: its schema, the names it defines, and the
//! compile of its documents into the pack's section of the plan.
//!
//! The rules of spec section 306 that a file alone can break are checked
//! here, each under a code of the pack's own, with the place and a fix.

use std::collections::{BTreeMap, BTreeSet};

use a2m_core::{Digest, FieldBase, FileKindName, Hazard, PackPayload, SurfaceKind};
use a2m_files::{
    CompiledSection, DefinedName, Diagnostic, Diagnostics, Doc, ExprSite, FileKind, JsonSchema,
    KindContext, Located, Place, RelPath, Stage, expr_text,
};

use crate::codes::{self, FILE_KIND, FOLDER, SURFACE_KIND, lint};
use crate::model::{
    Arg, Command, Effect, ExitMeaning, JsonField, JsonKind, Limits, Matching, Output, Param,
    ParamKind, Program, SECTION_FORMAT, Script, Section, VersionCheck,
};
use crate::pattern;

/// The file names a program surface may not have: shells, interpreters
/// that take code as an argument, and programs whose work is to start the
/// program their arguments name. Compared without a `.exe` suffix and
/// without regard to case.
///
/// The list is a net for a mistake, and it is not complete: many programs
/// can be told to run another through some option of their own. What
/// holds for those is the review of the target's files, where a command's
/// arguments are shown.
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
    // More interpreters.
    "pythonw",
    "ipython",
    "rscript",
    "r",
    "java",
    "javaw",
    "jshell",
    "jrunscript",
    "irb",
    "wish",
    "expect",
    "mshta",
    // Programs that start the program their arguments name.
    "nice",
    "ionice",
    "timeout",
    "stdbuf",
    "chroot",
    "doas",
    "pkexec",
    "runuser",
    "setsid",
    "setpriv",
    "unshare",
    "nsenter",
    "taskset",
    "chrt",
    "flock",
    "time",
    "watch",
    "script",
    "strace",
    "ltrace",
    "find",
    "parallel",
    "systemd-run",
    "start-stop-daemon",
    "wsl",
    "rundll32",
    "forfiles",
];

/// One-letter flags that hand their value to something that runs it,
/// matched as written: `-C` is a folder to many programs.
const CODE_FLAG_LETTERS: &[&str] = &["c", "e"];

/// Longer names of such flags, matched without regard to case and with one
/// dash or two: `--exec` and `-exec` are the same flag to different
/// programs.
const CODE_FLAG_WORDS: &[&str] = &[
    "eval",
    "exec",
    "execdir",
    "execute",
    "command",
    "cmd",
    "expression",
    "expr",
    "run",
    "script",
    "ok",
    "okdir",
    "rsh",
    "to-command",
    "use-compress-program",
];

/// Whether a flag hands its value to something that runs it. The flag is
/// read as a program would read it: without its dashes, or the `/` of an
/// option on Windows, and up to a `=` or `:` written into it.
///
/// A cluster of one-letter flags, such as `-xc`, is not recognised: what a
/// cluster means is the program's own.
pub(crate) fn runs_its_value(flag: &str) -> bool {
    let name = flag
        .strip_prefix('/')
        .unwrap_or_else(|| flag.trim_start_matches('-'));
    let name = name.split(['=', ':']).next().unwrap_or(name);
    CODE_FLAG_LETTERS.contains(&name)
        || CODE_FLAG_WORDS.contains(&name.to_ascii_lowercase().as_str())
}

/// Words that name a credential when they end a variable's or a flag's
/// name, as in `EXAMPLE_TOOL_TOKEN` and `--password`.
const CREDENTIAL_WORDS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "apikey",
    "credential",
    "credentials",
];

/// Words that name a credential when `key` follows them, as in `API_KEY`.
const CREDENTIAL_KEYS: &[&str] = &[
    "api",
    "access",
    "private",
    "secret",
    "signing",
    "encryption",
];

/// Whether a variable's or a flag's name says that it holds a credential.
/// Only the end of the name counts, so that `TOKEN_FILE` is not one.
pub(crate) fn names_a_credential(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    match words.as_slice() {
        [.., last] if CREDENTIAL_WORDS.contains(last) => true,
        [.., before, "key"] => CREDENTIAL_KEYS.contains(before),
        _ => false,
    }
}

/// Whether text written in a file looks like a credential: an
/// authorization scheme and its value, or a long run of letters and digits
/// mixed, as a key is. A net, not a proof: a key written with other
/// characters passes, and a review of the files is what holds then.
pub(crate) fn looks_like_credential(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if ["bearer ", "basic "]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return true;
    }
    text.len() >= 32
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'='))
        && text.bytes().any(|b| b.is_ascii_digit())
        && text.bytes().any(|b| b.is_ascii_alphabetic())
}

/// The part of an element that a flag written into it carries: what
/// follows the first `=` of an element that begins with `-`.
fn joined_value(element: &str) -> Option<(&str, &str)> {
    element
        .starts_with('-')
        .then(|| element.split_once('='))
        .flatten()
}

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
        .optional("exits", exits_schema())
        .optional(
            "interpreter",
            JsonSchema::object().required("args", JsonSchema::list(JsonSchema::scalar())),
        );
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
        .optional("limits", limits_schema())
        .optional(
            "script",
            JsonSchema::object()
                .required("path", JsonSchema::text())
                .required("digest", JsonSchema::text()),
        );
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

    /// A file describes a write when any of its commands is declared
    /// anything but `read`, or is missing its effect. An effect the drafter
    /// left as an open question is not a write: it states nothing, and the
    /// file does not compile until a person writes the effect in its place.
    /// A command is a write only by a person's word.
    fn writes(&self, doc: &Located<Doc>) -> bool {
        let Some(commands) = doc.get("commands") else {
            return false;
        };
        commands.entries().iter().any(|entry| {
            let effect = entry.value.get("effect");
            let open = effect.is_some_and(|effect| {
                matches!(effect.entries(), [only] if only.key.value == a2m_files::OPEN_QUESTION)
            });
            !open && effect.and_then(Located::as_text) != Some("read")
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
    for entry in doc.get("digests").map(Located::entries).unwrap_or_default() {
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
        if !compiled
            .capture_names()
            .flatten()
            .any(|name| name == "version")
        {
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
        .map(|form| {
            let elements = texts(Some(form));
            // The words of `{command}` are what the program's own help
            // printed: they are a value, and no flag may run them.
            let runs = elements.windows(2).any(|pair| {
                pair[1] == "{command}"
                    && pair[0].starts_with(['-', '/'])
                    && runs_its_value(&pair[0])
            });
            if runs {
                found.bad(
                    "code_flag",
                    &form.at,
                    "A help form puts the words of a command after a flag that hands its value to something that runs it."
                        .to_owned(),
                    "Remove the form: a program is never given text to run.",
                );
            }
            elements
        })
        .collect();
    let fixed = doc
        .get("version")
        .and_then(|version| version.get("args"))
        .into_iter()
        .chain(doc.get("help").map(Located::items).unwrap_or_default());
    for list in fixed {
        if texts(Some(list)).iter().any(|text| text.contains('\0')) {
            found.bad(
                "arg_malformed",
                &list.at,
                "An element of an argument list holds a NUL character.".to_owned(),
                "Remove the character: no program can be given it.",
            );
        }
    }
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
        if a2m_provider::is_engine_variable(name) {
            found.bad(
                "engine_variable",
                &entry.key.at,
                format!(
                    "The variable `{}` belongs to the engine, and the engine's own variables are never handed to a program.",
                    codes::shown(name)
                ),
                "Remove it. A program is given only variables of its own.",
            );
            continue;
        }
        if names_a_credential(name) {
            found.bad(
                "secret_variable",
                &entry.key.at,
                format!(
                    "The variable `{}` is named for a credential, and its value would sit in the target's files and in the plan.",
                    codes::shown(name)
                ),
                "Remove it. A credential is kept in the vault and reaches a program only through the session, never through a target's files.",
            );
            continue;
        }
        if looks_like_credential(&value) {
            found.bad(
                "secret_literal",
                &entry.value.at,
                format!(
                    "The value of the variable `{}` looks like a credential written into a target file.",
                    codes::shown(name)
                ),
                "Remove it. A credential is kept in the vault, never in a target's files.",
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
    for name in env
        .and_then(|env| env.get("pass"))
        .map(Located::items)
        .unwrap_or_default()
    {
        if a2m_provider::is_engine_variable(&scalar(name)) {
            found.bad(
                "engine_variable",
                &name.at,
                format!(
                    "The variable `{}` belongs to the engine, and the engine's own variables are never handed to a program.",
                    codes::shown(&scalar(name))
                ),
                "Remove it. A program is given only variables of its own.",
            );
            continue;
        }
        if names_a_credential(&scalar(name)) {
            found.bad(
                "secret_variable",
                &name.at,
                format!(
                    "The variable `{}` is named for a credential, and copying it from the engine's own environment would hand the program a secret the redactor was never told of.",
                    codes::shown(&scalar(name))
                ),
                "Remove it. A credential reaches a program only through the session, which registers it with the redactor first.",
            );
        }
    }
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
        interpreter: interpreter(doc, found),
    }
}

/// The fixed arguments of a program declared as the interpreter of fixed
/// pairs. They come before the script's path, and none of them may be a
/// flag that hands what follows it to something that runs it: the script's
/// path follows, and the typed values after it.
fn interpreter(doc: &Located<Doc>, found: &mut Found) -> Option<Vec<String>> {
    let block = doc.get("interpreter")?;
    let args = block.get("args");
    let fixed = texts(args);
    if fixed.iter().any(|text| text.contains('\0')) {
        found.bad(
            "arg_malformed",
            &block.at,
            "An element of an argument list holds a NUL character.".to_owned(),
            "Remove the character: no program can be given it.",
        );
    }
    if let Some(flag) = fixed
        .iter()
        .find(|text| text.starts_with(['-', '/']) && runs_its_value(text))
    {
        found.bad(
            "code_flag",
            &block.at,
            format!(
                "The interpreter is given `{}`, which hands what follows it to something that runs it.",
                codes::shown(flag)
            ),
            "Give the interpreter the arguments that make it read a script file, such as `-File` or none at all.",
        );
    }
    if !doc.get("help").map(Located::items).unwrap_or_default().is_empty() {
        found.bad(
            "script_malformed",
            &block.at,
            "An interpreter has no help forms: discovery never runs one.".to_owned(),
            "Remove `help` from a file that declares an `interpreter`.",
        );
    }
    Some(fixed)
}

/// The script a command of an interpreter's file names: a path inside the
/// target's folder and the digest of its bytes.
fn script(name: &str, doc: &Located<Doc>, shared: &Program, found: &mut Found) -> Option<Script> {
    let block = doc.get("script");
    match (block, shared.interpreter.is_some()) {
        (None, false) => None,
        (Some(block), false) => {
            found.bad(
                "script_malformed",
                &block.at,
                format!("The command `{name}` names a script, and its file declares no `interpreter`."),
                "Declare the interpreter under `program.interpreter`, with the arguments that make it read a script file.",
            );
            None
        }
        (None, true) => {
            found.bad(
                "script_malformed",
                &doc.at,
                format!(
                    "The command `{name}` names no script, and its program is an interpreter: an interpreter is given a script the target keeps, or nothing."
                ),
                "Add `script: {path: ..., digest: ...}` to the command.",
            );
            None
        }
        (Some(block), true) => {
            let path = block
                .text("path")
                .filter(|path| RelPath::new(path).is_ok())
                .map(str::to_owned);
            if path.is_none() {
                found.bad(
                    "script_malformed",
                    &block.at,
                    format!("The script of the command `{name}` is not named by a path inside the target's folder."),
                    "Name the script by its path inside the target's folder, names joined by `/`, with no `..`, no drive, and no leading `/`.",
                );
            }
            let digest = block
                .text("digest")
                .and_then(|text| text.strip_prefix("sha256:"))
                .and_then(|hex| hex.parse::<Digest>().ok())
                .map(|digest| digest.to_string());
            if digest.is_none() {
                found.bad(
                    "digest_invalid",
                    &block.at,
                    "A digest is `sha256:` and 64 lowercase hexadecimal characters.".to_owned(),
                    "Write the SHA-256 of the script's file, as `sha256:` and its 64 characters.",
                );
            }
            Some(Script {
                path: path?,
                digest: digest?,
            })
        }
    }
}

fn param(name: &str, doc: &Located<Doc>, cx: &KindContext<'_>, found: &mut Found) -> Option<Param> {
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

/// Refuses a flag that is named for a credential and carries a value.
fn secret_argument(flag: &str, doc: &Located<Doc>, found: &mut Found) {
    found.bad(
        "secret_argument",
        &doc.at,
        format!(
            "The flag `{}` is named for a credential, and a credential never travels as an argument: other programs on the machine can read a process's arguments.",
            codes::shown(flag)
        ),
        "Remove the entry. A program that takes its credential only as an argument cannot be given one.",
    );
}

fn arg(doc: &Located<Doc>, params: &BTreeMap<String, Param>, found: &mut Found) -> Option<Arg> {
    if !matches!(doc.value, Doc::Map(_)) {
        let text = scalar(doc);
        if text.contains('\0') {
            found.bad(
                "arg_malformed",
                &doc.at,
                "An element of the argument list holds a NUL character.".to_owned(),
                "Remove the character: no program can be given it.",
            );
            return None;
        }
        // A credential written as an element, alone or joined to its flag.
        let (flag, value) = joined_value(&text).unwrap_or(("", &text));
        if !value.is_empty() && names_a_credential(flag) {
            secret_argument(flag, doc, found);
            return None;
        }
        if looks_like_credential(value) {
            found.bad(
                "secret_literal",
                &doc.at,
                "An element of the argument list looks like a credential written into a target file."
                    .to_owned(),
                "Remove it. A credential is kept in the vault, never in a target's files, and never travels as an argument.",
            );
            return None;
        }
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
    if join.as_ref().is_some_and(|join| join.contains('\0')) {
        return malformed(found);
    }
    // Whatever form carries it, a value never follows a flag that runs its
    // value, and never a flag named for a credential.
    let carries_value = stamp || named.iter().any(|(key, _)| *key != "when");
    if let Some(flag) = flag.as_ref().filter(|_| carries_value) {
        if runs_its_value(flag) {
            found.bad(
                "code_flag",
                &doc.at,
                format!(
                    "The flag `{}` hands its value to something that runs it.",
                    codes::shown(flag)
                ),
                "Remove the entry: a program is never given text to run.",
            );
            return None;
        }
        let name = flag.split('=').next().unwrap_or(flag);
        if names_a_credential(name) {
            secret_argument(name, doc, found);
            return None;
        }
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
        // A flag written as an element of its own, and the value after it.
        for pair in args.windows(2) {
            let [Arg::Literal { text }, after] = pair else {
                continue;
            };
            let value_follows = matches!(after, Arg::Value { .. });
            if value_follows && text.starts_with(['-', '/']) && runs_its_value(text) {
                found.bad(
                    "code_flag",
                    &doc.at,
                    format!(
                        "The command `{name}` puts a value after `{}`, which hands what follows it to something that runs it.",
                        codes::shown(text)
                    ),
                    "Remove the value: a program is never given text to run.",
                );
            }
            let follows = value_follows || matches!(after, Arg::Literal { .. });
            if follows && text.starts_with('-') && names_a_credential(text) {
                secret_argument(text, doc, found);
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
                format!(
                    "The command `{name}` declares the param `{unused}` and no argument carries it."
                ),
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
    let output = doc
        .get("output")
        .and_then(|output| self::output(output, found));
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
    let script = script(name, doc, shared, found);
    if found.problems.len() != before {
        return None;
    }
    Some(Command {
        script,
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
    for entry in doc
        .get("commands")
        .map(Located::entries)
        .unwrap_or_default()
    {
        if let Some(compiled) = command(&entry.key.value, &entry.value, index, &shared, cx, found) {
            section.commands.insert(entry.key.value.clone(), compiled);
        }
    }
    if same.is_none() {
        section.programs.push(shared);
    }
}

#[cfg(test)]
mod tests {
    use a2m_files::{FileKind, RelPath, parse_yaml};

    use super::{
        CommandKind, is_shell_or_interpreter, looks_like_credential, names_a_credential,
        runs_its_value,
    };

    #[test]
    fn a_file_describes_a_write_when_a_command_is_declared_anything_but_read() {
        let kind = CommandKind::new();
        let file = RelPath::new("commands/one.yaml").unwrap();
        let writes = |effects: &[&str]| {
            let mut text = String::from("program: {path: /opt/example/bin/tool}\ncommands:\n");
            for (n, effect) in effects.iter().enumerate() {
                text.push_str(&format!("  c{n}:\n    effect: {effect}\n    args: []\n"));
            }
            let docs = parse_yaml(&file, &text).unwrap();
            kind.writes(&docs[0])
        };
        assert!(!writes(&["read", "read"]));
        assert!(writes(&["read", "write"]));
        assert!(writes(&["irreversible"]));
        assert!(writes(&["\"maybe\""]));
        // What the drafter leaves for a person states no effect.
        assert!(!writes(&["{open_question: \"Does it write?\"}"]));
    }

    /// A rule of this pack that drafts a command declared `write`.
    struct DraftsAWrite;

    impl a2m_files::DraftingRule for DraftsAWrite {
        fn finding_kind(&self) -> a2m_core::FindingKind {
            crate::codes::finding_kind()
        }
        fn file_kind(&self) -> a2m_core::FileKindName {
            crate::codes::FILE_KIND
        }
        fn draft(
            &self,
            _finding: &a2m_core::Finding,
            _cx: &a2m_files::DraftContext<'_>,
        ) -> Result<Vec<a2m_files::DraftDoc>, a2m_core::Failure> {
            use a2m_files::DraftValue;
            let command = DraftValue::map(vec![
                ("effect", DraftValue::text("write")),
                ("args", DraftValue::List(Vec::new())),
            ]);
            Ok(vec![a2m_files::DraftDoc {
                path: RelPath::new("commands/remove.yaml")?,
                body: DraftValue::map(vec![
                    (
                        "program",
                        DraftValue::map(vec![("path", DraftValue::text("/opt/example/bin/tool"))]),
                    ),
                    ("commands", DraftValue::map(vec![("remove", command)])),
                ]),
            }])
        }
    }

    #[test]
    fn a_drafting_rule_that_drafts_a_write_is_refused_and_nothing_is_written() {
        use std::sync::Arc;

        let mut builder = a2m_files::CompilerSet::builder();
        builder
            .file_kind(crate::codes::PACK, Arc::new(CommandKind::new()))
            .unwrap();
        builder
            .drafting_rule(crate::codes::PACK, Arc::new(DraftsAWrite))
            .unwrap();
        let set = builder.build();
        let folder =
            std::env::temp_dir().join(format!("a2m-pack-command-draft-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let finding = a2m_core::Finding::about(
            a2m_core::Subject::File {
                digest: a2m_core::Digest::of(b"tool"),
                format: "program".to_owned(),
                part: None,
            },
            crate::codes::finding_kind(),
            "{}",
            a2m_core::Source::LiveSystem,
            a2m_core::ProducedBy {
                provider: "command".to_owned(),
                version: "0.0.1".to_owned(),
                component: None,
            },
            a2m_core::Timestamp::from_unix_millis(1_000),
        )
        .build()
        .unwrap();
        let root = a2m_files::TargetRoot::new(&folder);
        let report = a2m_files::Drafter::new(&set, &root)
            .draft(&[finding])
            .unwrap();
        assert!(report.written.is_empty(), "{report:?}");
        assert_eq!(report.refused.len(), 1, "{report:?}");
        assert!(!folder.join("commands").join("remove.yaml").exists());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn shells_interpreters_and_programs_that_start_another_are_known_by_name() {
        for refused in [
            "/bin/sh",
            "/usr/bin/python3.12",
            "/usr/bin/Rscript",
            "/usr/bin/java",
            "/usr/bin/timeout",
            "/usr/bin/nice",
            "/usr/bin/find",
            "/usr/sbin/chroot",
            "/usr/bin/pkexec",
            r"C:\Windows\System32\rundll32.exe",
            r"C:\Windows\System32\WSL.EXE",
            r"C:\tools\run.bat",
        ] {
            assert!(is_shell_or_interpreter(refused), "{refused}");
        }
        for allowed in [
            "/opt/example/bin/example-tool",
            "/usr/bin/finder",
            "/opt/timeouts",
        ] {
            assert!(!is_shell_or_interpreter(allowed), "{allowed}");
        }
    }

    #[test]
    fn a_flag_that_runs_its_value_is_known_in_each_spelling() {
        for runs in [
            "-c",
            "-e",
            "--eval",
            "--Eval",
            "-exec",
            "--exec=",
            "--command:",
            "/c",
            "-okdir",
            "--rsh",
        ] {
            assert!(runs_its_value(runs), "{runs}");
        }
        for plain in ["-C", "-E", "--name", "--executor", "--run-id", "--", "-xc"] {
            assert!(!runs_its_value(plain), "{plain}");
        }
    }

    #[test]
    fn a_credential_is_known_by_the_end_of_its_name_or_by_its_look() {
        for named in [
            "EXAMPLE_TOOL_TOKEN",
            "--password",
            "--api-key",
            "ApiKey",
            "AWS_SECRET_ACCESS_KEY",
            "-passphrase",
        ] {
            assert!(names_a_credential(named), "{named}");
        }
        for plain in [
            "TOKEN_FILE",
            "--key",
            "--name",
            "TZ",
            "--password-stdin",
            "KEYBOARD",
        ] {
            assert!(!names_a_credential(plain), "{plain}");
        }
        for key in [
            "Bearer example-token-0001",
            "example0key1example2key3example4key5",
        ] {
            assert!(looks_like_credential(key), "{key}");
        }
        for plain in [
            "/var/lib/example/home/2024/exports/data",
            "123e4567-e89b-12d3-a456-426614174000",
            "iso8601-extended",
            "C.UTF-8",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(!looks_like_credential(plain), "{plain}");
        }
    }
}
