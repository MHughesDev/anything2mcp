//! Drafting: from one `command` finding to one draft `commands/` file.
//!
//! The draft holds what the program was seen to say: its digest, its
//! version, the command's words, and the arguments its usage line shows.
//! It holds no effect, no exit status beyond `0`, no output form, and no
//! read-back: each of those is an open question for a person.
//!
//! What a handed completion file or manual page says of an option may type
//! a parameter the usage line shows, or word the question about it. A flag
//! only a file states, and a flag the sources disagree on, is a question,
//! never a parameter (decision 0284).

use a2m_core::{FileKindName, Finding, FindingKind};
use a2m_files::{
    Doc, DraftContext, DraftDoc, DraftValue, DraftingRule, Located, RelPath, UNTRUSTED_TEXT,
    names_a_secret,
};

use crate::codes::{self, FILE_KIND, FOLDER};
use crate::discover::CommandFound;
use crate::metadata::{HELP, OptionFound};

/// Whether words name a whole number.
fn whole(words: &str) -> bool {
    let lower = words.to_ascii_lowercase();
    lower.contains("whole number") || lower.contains("integer")
}

/// What the input files, and not the help, say of an option: its value's
/// name and its description, in the order the sources were read.
fn file_words(option: &OptionFound) -> impl Iterator<Item = &str> {
    option
        .said
        .iter()
        .filter(|said| said.from != HELP)
        .flat_map(|said| said.value.iter().chain(said.about.iter()))
        .map(String::as_str)
}

/// How a value's place is typed, from the usage line and what the handed
/// files say of its flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Typed<'a> {
    /// A whole number: the place, or a file's words for it, says so.
    Integer,
    /// The sources disagree on whether the flag takes a value.
    Disagree,
    /// A question that quotes what a file says of the value.
    AskQuoting(&'a str),
    /// A question with nothing more to quote.
    Ask,
}

pub(crate) fn typed<'a>(place: &str, option: Option<&'a OptionFound>) -> Typed<'a> {
    if option.is_some_and(|option| option.disagree) {
        return Typed::Disagree;
    }
    if whole(place) || option.is_some_and(|option| file_words(option).any(whole)) {
        return Typed::Integer;
    }
    option
        .and_then(|option| {
            option
                .said
                .iter()
                .filter(|said| said.from != HELP)
                .find_map(|said| said.about.as_deref())
        })
        .map_or(Typed::Ask, Typed::AskQuoting)
}

const DISAGREE: &str = "Does the quoted flag of the quoted command take a value? The program's help and a file handed to the scout disagree, and neither is taken. The first quoted words are the command, the second the flag.";
const FILE_ONLY: &str = "Does the quoted command take the quoted flag, and in what form? A file handed to the scout states it, and the program's own help does not show it. The first quoted words are the command, the second the flag.";
const TYPED_BY_FILE: &str = "What type is the value that the usage line of the command writes in the place quoted? The first quoted words are the command, the second the place, and the third what a file handed to the scout says of it. A text param also needs an anchored `pattern`.";

/// The drafting rule from `command` findings to `commands/` files.
pub(crate) struct CommandFromFinding;

/// Whether a flag or a place of a usage line is named as a secret, as the
/// program writes it or as the parameter it becomes (decision 0302).
pub(crate) fn named_as_secret(word: &str) -> bool {
    names_a_secret(word) || names_a_secret(&slug(word))
}

/// A lowercase name made of a text's letters and digits.
pub(crate) fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_owned();
    if out.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        out
    } else {
        format!("arg_{out}")
    }
}

/// One part of a usage line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsagePart {
    /// The flag, when the part has one.
    pub flag: Option<String>,
    /// The words between `<` and `>`, when the part takes a value.
    pub place: Option<String>,
    /// Whether the flag and the value are joined by `=`.
    pub joined: bool,
    /// Whether the part stands in brackets.
    pub optional: bool,
}

/// Reads one part: `<place>`, `--flag`, `--flag <place>`, `--flag=<place>`.
fn part(text: &str, optional: bool) -> Option<UsagePart> {
    let place_of = |text: &str| {
        text.strip_prefix('<')
            .and_then(|rest| rest.strip_suffix('>'))
            .filter(|inner| !inner.is_empty() && !inner.contains(['<', '>']))
            .map(str::to_owned)
    };
    let text = text.trim();
    if let Some(place) = place_of(text) {
        return Some(UsagePart {
            flag: None,
            place: Some(place),
            joined: false,
            optional,
        });
    }
    if !text.starts_with('-') {
        return None;
    }
    let (flag, rest, joined) = match text.find(['=', ' ']) {
        None => (text, "", false),
        Some(at) => (
            &text[..at],
            text[at + 1..].trim(),
            text.as_bytes()[at] == b'=',
        ),
    };
    let fine = flag.len() > 1
        && flag != "--"
        && flag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !fine {
        return None;
    }
    let place = if rest.is_empty() {
        None
    } else {
        Some(place_of(rest)?)
    };
    Some(UsagePart {
        flag: Some(flag.to_owned()),
        place,
        joined,
        optional,
    })
}

/// The parts of a usage line, or nothing when any of it is in a form this
/// reader does not know.
pub(crate) fn usage_parts(usage: &str) -> Option<Vec<UsagePart>> {
    let mut parts = Vec::new();
    let mut rest = usage.trim();
    while !rest.is_empty() {
        let (piece, optional, after) = if let Some(inner) = rest.strip_prefix('[') {
            let end = inner.find(']')?;
            (&inner[..end], true, &inner[end + 1..])
        } else if rest.starts_with('<') {
            let end = rest.find('>')?;
            (&rest[..=end], false, &rest[end + 1..])
        } else {
            // A flag, with its value when one follows in angle brackets.
            let flag_end = rest.find(['=', ' ']).unwrap_or(rest.len());
            let after_flag = rest[flag_end..].trim_start_matches(['=', ' ']);
            let end = if after_flag.starts_with('<') {
                let close = after_flag.find('>')?;
                rest.len() - after_flag.len() + close + 1
            } else {
                flag_end
            };
            (&rest[..end], false, &rest[end..])
        };
        if piece.contains('[') || piece.contains('|') || after.starts_with("...") {
            return None;
        }
        parts.push(part(piece, optional)?);
        rest = after.trim_start();
    }
    Some(parts)
}

impl DraftingRule for CommandFromFinding {
    fn finding_kind(&self) -> FindingKind {
        codes::finding_kind()
    }

    fn file_kind(&self) -> FileKindName {
        FILE_KIND
    }

    fn draft(
        &self,
        finding: &Finding,
        cx: &DraftContext<'_>,
    ) -> Result<Vec<DraftDoc>, a2m_core::Failure> {
        let found = CommandFound::read(finding)?;
        let name = slug(&found.name);
        let listed = &found.name;
        // A command a person already declared is theirs: drafting it again
        // would name it twice, and a changed file is never overwritten.
        if declared(cx, listed).is_some() {
            cx.not_drafted(
                "A person already declared this command in a `commands/` file, so no draft of it is made.",
            );
            return Ok(Vec::new());
        }

        let mut program = vec![(
            "digests",
            DraftValue::Map(vec![(
                found.platform.clone(),
                DraftValue::List(vec![DraftValue::Text(format!("sha256:{}", found.digest))]),
            )]),
        )];
        if let Some(version) = &found.version {
            program.push((
                "version",
                DraftValue::map(vec![
                    (
                        "args",
                        DraftValue::List(
                            version.args.iter().map(|a| DraftValue::text(a)).collect(),
                        ),
                    ),
                    (
                        "expect",
                        DraftValue::map(vec![("stdout", DraftValue::text(&version.pattern))]),
                    ),
                    (
                        "supported",
                        DraftValue::List(vec![DraftValue::text(&version.version)]),
                    ),
                ]),
            ));
        }
        program.push((
            "exits",
            DraftValue::Map(vec![("0".to_owned(), DraftValue::text("ok"))]),
        ));

        // The command's words, then what its usage line shows.
        let mut args: Vec<DraftValue> = listed.split(' ').map(DraftValue::text).collect();
        let mut params: Vec<(String, DraftValue)> = Vec::new();
        let parts = found.usage.as_deref().map(usage_parts);
        // A value a program takes under a secret's name is never a param: an
        // optional one is left out of the draft, and a required one makes
        // the arguments a question (decision 0302). A command has no
        // description to say so in, and no output is drafted.
        let mut required_secret = false;
        let args = match parts {
            Some(Some(parts)) => {
                let option_of =
                    |flag: &str| found.options.iter().find(|option| option.has(flag));
                for part in parts {
                    let typed = |place: &str, flag: Option<&str>| match typed(
                        place,
                        flag.and_then(option_of),
                    ) {
                        Typed::Integer => DraftValue::text("integer"),
                        Typed::Disagree => {
                            cx.open_question_quoting(DISAGREE, &[listed, flag.unwrap_or(place)])
                        }
                        Typed::AskQuoting(about) => {
                            cx.open_question_quoting(TYPED_BY_FILE, &[listed, place, about])
                        }
                        Typed::Ask => cx.open_question_quoting("What type is the value that the usage line of the command writes in the place quoted? The first quoted words are the command, the second the place. A text param also needs an anchored `pattern`.", &[listed, place]),
                    };
                    let disagreed = |flag: &str| option_of(flag).is_some_and(|o| o.disagree);
                    let mut param = |name: String, ty: DraftValue, optional: bool| {
                        let mut fields = vec![("type", ty)];
                        if optional {
                            fields.push(("required", DraftValue::Bool(false)));
                        }
                        if !params.iter().any(|(known, _)| *known == name) {
                            params.push((name, DraftValue::map(fields)));
                        }
                    };
                    let takes_param = part.place.is_some() || part.optional;
                    let secret = takes_param
                        && (part.flag.as_deref().is_some_and(named_as_secret)
                            || part.place.as_deref().is_some_and(named_as_secret));
                    if secret {
                        required_secret |= !part.optional;
                        continue;
                    }
                    match (part.flag, part.place) {
                        (None, Some(place)) => {
                            let name = slug(&place);
                            param(name.clone(), typed(&place, None), part.optional);
                            args.push(DraftValue::map(vec![("param", DraftValue::Text(name))]));
                        }
                        (Some(flag), Some(place)) => {
                            let name = slug(&flag);
                            param(name.clone(), typed(&place, Some(&flag)), part.optional);
                            let mut entry = vec![
                                ("flag", DraftValue::Text(flag)),
                                ("param", DraftValue::Text(name)),
                            ];
                            if part.joined {
                                entry.push(("join", DraftValue::text("=")));
                            }
                            args.push(DraftValue::map(entry));
                        }
                        (Some(flag), None) if part.optional => {
                            let name = slug(&flag);
                            let ty = if disagreed(&flag) {
                                cx.open_question_quoting(DISAGREE, &[listed, &flag])
                            } else {
                                DraftValue::text("boolean")
                            };
                            param(name.clone(), ty, true);
                            args.push(DraftValue::map(vec![
                                ("flag", DraftValue::Text(flag)),
                                ("when", DraftValue::Text(name)),
                            ]));
                        }
                        (Some(flag), None) if disagreed(&flag) => {
                            args.push(cx.open_question_quoting(DISAGREE, &[listed, &flag]));
                        }
                        (Some(flag), None) => args.push(DraftValue::Text(flag)),
                        (None, None) => {}
                    }
                }
                // A flag that only a input file states is a question, never
                // a parameter: the program's own help does not show it.
                for option in found.options.iter().filter(|option| !option.in_help()) {
                    if let Some(flag) = option.flags.first() {
                        args.push(cx.open_question_quoting(FILE_ONLY, &[listed, flag]));
                    }
                }
                if required_secret {
                    cx.open_question_quoting(
                        "What are the arguments of the quoted command? Its usage line requires a value under a name that is a secret's, and a secret is never an input.",
                        &[listed],
                    )
                } else {
                    DraftValue::List(args)
                }
            }
            Some(None) => cx.open_question_quoting(
                "What are the arguments of the quoted command? Its usage line is in a form the drafter does not read.",
                &[listed],
            ),
            None => cx.open_question_quoting(
                "What are the arguments of the quoted command? Its own help was not read.",
                &[listed],
            ),
        };

        let mut command = vec![(
            "effect",
            cx.open_question_quoting(
                "Does the quoted command read, write, or do something that cannot be taken back? Its help does not say, and its name proves nothing.",
                &[listed],
            ),
        )];
        if !params.is_empty() {
            command.push(("params", DraftValue::Map(params)));
        }
        command.push(("args", args));
        command.push((
            "output",
            cx.open_question_quoting(
                "What does the quoted command print? Its output was not seen, because the command was not run.",
                &[listed],
            ),
        ));

        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("{FOLDER}/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("program", DraftValue::map(program)),
                (
                    "commands",
                    DraftValue::Map(vec![(name, DraftValue::map(command))]),
                ),
            ]),
        }])
    }
}

/// The command a person declared for a listed command, with the name they
/// gave it: the entry named as the drafter would name it, or one whose
/// argument list begins with the listed command's words, as written.
fn declared<'a>(cx: &DraftContext<'a>, listed: &str) -> Option<(&'a str, &'a Located<Doc>)> {
    let key = slug(listed);
    let words: Vec<&str> = listed.split(' ').collect();
    cx.existing()?
        .docs_of("command")
        .filter_map(|doc| doc.get("commands"))
        .flat_map(Located::entries)
        .find(|entry| {
            entry.key.value == key
                || entry.value.get("args").is_some_and(|args| {
                    let items = args.items();
                    items.len() >= words.len()
                        && items
                            .iter()
                            .zip(&words)
                            .all(|(item, word)| item.as_text() == Some(*word))
                })
        })
        .map(|entry| (entry.key.value.as_str(), &entry.value))
}

/// The drafting rule from `command` findings to read operations, over the
/// commands a person has declared `effect: read` (decision 0387). The help
/// does not say what a command does, so nothing is drafted for a command
/// whose effect nobody declared, and nothing for one declared a write; the
/// effect is the person's word, never inferred from a name.
pub(crate) struct OperationFromCommand;

/// A command's outputs as an operation holds them: the declared form's
/// fields with the type each one states, and whether the form gives one
/// record or a list of rows.
struct Shape {
    list: bool,
    outputs: Vec<(String, DraftValue)>,
}

/// The output type for a declared word of a JSON field.
fn output_type(word: &str) -> Option<&'static str> {
    match word {
        "text" => Some(UNTRUSTED_TEXT),
        "integer" => Some("integer"),
        "decimal" => Some("decimal"),
        "boolean" => Some("boolean"),
        _ => None,
    }
}

/// What a declared output form gives, or why it gives nothing to draft.
fn shape(output: &Located<Doc>) -> Result<Shape, &'static str> {
    let field = |name: &str, ty: &str, list: bool| {
        let mut fields = vec![("type", DraftValue::text(ty))];
        if list {
            fields.push(("list", DraftValue::Bool(true)));
        }
        (name.to_owned(), DraftValue::map(fields))
    };
    if let Some(json) = output.get("json") {
        let lines = json.get("lines").and_then(Located::as_bool) == Some(true);
        if json.get("at").is_some() {
            return Err(
                "The declared output follows a path into a document, and whether it holds one record or a list is not stated.",
            );
        }
        let mut outputs = Vec::new();
        for entry in json.get("fields").map(Located::entries).unwrap_or_default() {
            let word = match &entry.value.value {
                Doc::Map(_) => entry.value.text("type"),
                _ => entry.value.as_text(),
            };
            let Some(ty) = word.and_then(output_type) else {
                return Err("A declared output field has a type this rule does not read.");
            };
            outputs.push(field(&entry.key.value, ty, lines));
        }
        return Ok(Shape {
            list: lines,
            outputs,
        });
    }
    if let Some(table) = output.get("table") {
        let columns = table.get("header").or_else(|| table.get("columns"));
        let outputs = columns
            .map(Located::items)
            .unwrap_or_default()
            .iter()
            .filter_map(Located::as_text)
            .map(|name| field(name, UNTRUSTED_TEXT, true))
            .collect();
        return Ok(Shape {
            list: true,
            outputs,
        });
    }
    if let Some(pattern) = output.get("pattern").and_then(Located::as_text) {
        let Ok(compiled) = regex::Regex::new(pattern) else {
            return Err("The declared output pattern is not one this rule reads.");
        };
        let list = output.text("match") == Some("every_line");
        let outputs = compiled
            .capture_names()
            .flatten()
            .map(|name| field(name, UNTRUSTED_TEXT, list))
            .collect();
        return Ok(Shape { list, outputs });
    }
    Err("The declared output prints nothing this rule can name.")
}

impl DraftingRule for OperationFromCommand {
    fn finding_kind(&self) -> FindingKind {
        codes::finding_kind()
    }

    fn file_kind(&self) -> FileKindName {
        FileKindName::fixed("operation")
    }

    fn draft(
        &self,
        finding: &Finding,
        cx: &DraftContext<'_>,
    ) -> Result<Vec<DraftDoc>, a2m_core::Failure> {
        let found = CommandFound::read(finding)?;
        let none = |reason: &str| {
            cx.not_drafted(reason);
            Ok(Vec::new())
        };
        let Some((key, command)) = declared(cx, &found.name) else {
            return none(
                "No operation is drafted: no person has declared this command's effect, and its help does not say what it does.",
            );
        };
        match command.text("effect") {
            Some("read") => {}
            Some(_) => {
                return none(
                    "No read is drafted: the command is declared a write, which a rule never drafts.",
                );
            }
            None => {
                return none("No operation is drafted: the command's effect is not declared.");
            }
        }
        let Some(target) = cx.target() else {
            return none("No operation is drafted: the target has no name to name a tool by.");
        };
        // An operation already bound to the command is the person's.
        let bound = cx.existing().is_some_and(|sources| {
            sources.docs_of("operation").any(|operation| {
                operation.get("bindings").is_some_and(|bindings| {
                    bindings.items().iter().any(|binding| {
                        binding.text("mode") == Some("cli") && binding.text("command") == Some(key)
                    })
                })
            })
        });
        if bound {
            return none("No operation is drafted: an operation already binds this command.");
        }
        if command
            .get("args")
            .map(Located::items)
            .unwrap_or_default()
            .iter()
            .any(|arg| arg.get("stamp").is_some())
        {
            return none(
                "No operation is drafted: the command takes a record's stamp, which a read of one does not give.",
            );
        }
        let shaped = match command.get("output").map(shape) {
            Some(Ok(shaped)) if !shaped.outputs.is_empty() => shaped,
            Some(Err(reason)) => return none(reason),
            _ => return none("No operation is drafted: the command declares no output to name."),
        };
        let mut inputs: Vec<(String, DraftValue)> = Vec::new();
        for entry in command
            .get("params")
            .map(Located::entries)
            .unwrap_or_default()
        {
            let param = &entry.value;
            let name = &entry.key.value;
            let Some(ty) = param.text("type") else {
                return none("No operation is drafted: a declared param states no type.");
            };
            if named_as_secret(name) || param.get("list").is_some() {
                return none(
                    "No operation is drafted: a declared param is named as a secret or takes a list.",
                );
            }
            let mut fields = vec![("type", DraftValue::text(ty))];
            if param.get("required").and_then(Located::as_bool) == Some(false) {
                fields.push(("required", DraftValue::Bool(false)));
            }
            inputs.push((name.clone(), DraftValue::map(fields)));
        }
        let operation = format!("{}_{key}", target.as_str());
        if a2m_core::OperationName::new(&operation).is_err() {
            return none("No operation is drafted: the command's name makes no tool name.");
        }
        let verb = if shaped.list { "list" } else { "read" };
        let body = DraftValue::map(vec![
            ("operation", DraftValue::text(&operation)),
            ("effect", DraftValue::text("read")),
            ("inputs", DraftValue::Map(inputs)),
            ("outputs", DraftValue::Map(shaped.outputs)),
            (
                "bindings",
                DraftValue::List(vec![DraftValue::map(vec![
                    ("mode", DraftValue::text("cli")),
                    ("verb", DraftValue::text(verb)),
                    ("command", DraftValue::text(key)),
                ])]),
            ),
            (
                "description",
                DraftValue::text(&format!(
                    "Runs the command `{key}`, which a person declared a read, and returns what it prints as the declared output names it. Drafted by rule from the command's finding and its declaration; its name is provisional."
                )),
            ),
        ]);
        Ok(vec![DraftDoc {
            path: RelPath::new(&format!("operations/{key}.yaml"))?,
            body,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::{Typed, UsagePart, slug, typed, usage_parts};
    use crate::metadata::{OptionFound, OptionSaid, Takes};

    fn said(from: &str, takes: Takes, value: Option<&str>, about: Option<&str>) -> OptionSaid {
        OptionSaid {
            from: from.to_owned(),
            takes,
            value: value.map(str::to_owned),
            about: about.map(str::to_owned),
        }
    }

    #[test]
    fn a_files_words_type_a_place_the_help_left_open_and_a_disagreement_is_asked() {
        let rows = OptionFound {
            flags: vec!["--rows".to_owned()],
            said: vec![
                said("help", Takes::Value, Some("n"), None),
                said(
                    "manual",
                    Takes::Value,
                    Some("N"),
                    Some("At most N rows, a whole number."),
                ),
            ],
            disagree: false,
        };
        assert_eq!(typed("n", Some(&rows)), Typed::Integer);
        let form = OptionFound {
            flags: vec!["--format".to_owned()],
            said: vec![
                said("help", Takes::Value, Some("form"), None),
                said("completions", Takes::Value, None, Some("How to print it")),
            ],
            disagree: false,
        };
        assert_eq!(
            typed("form", Some(&form)),
            Typed::AskQuoting("How to print it")
        );
        // The help's own words never count as a file's.
        let help_only = OptionFound {
            flags: vec!["--x".to_owned()],
            said: vec![said("help", Takes::Value, Some("integer of sorts"), None)],
            disagree: false,
        };
        assert_eq!(typed("thing", Some(&help_only)), Typed::Ask);
        assert_eq!(typed("whole number", None), Typed::Integer);
        let split = OptionFound {
            disagree: true,
            ..rows.clone()
        };
        assert_eq!(typed("whole number", Some(&split)), Typed::Disagree);
    }

    #[test]
    fn a_usage_line_is_read_part_by_part_or_not_at_all() {
        let parts =
            usage_parts("<sku> --quantity=<whole number> [--if-version <whole number>] [--yes]")
                .unwrap();
        let part = |flag: Option<&str>, place: Option<&str>, joined, optional| UsagePart {
            flag: flag.map(str::to_owned),
            place: place.map(str::to_owned),
            joined,
            optional,
        };
        assert_eq!(
            parts,
            [
                part(None, Some("sku"), false, false),
                part(Some("--quantity"), Some("whole number"), true, false),
                part(Some("--if-version"), Some("whole number"), false, true),
                part(Some("--yes"), None, false, true),
            ]
        );
        assert_eq!(
            usage_parts("--name <name> --location <code>")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(usage_parts("").unwrap(), []);
        for unknown in [
            "[<anything>...]",
            "<a>...",
            "(--x | --y)",
            "[--a [--b]]",
            "$(reboot)",
        ] {
            assert_eq!(usage_parts(unknown), None, "{unknown}");
        }
        assert_eq!(slug("item show"), "item_show");
        assert_eq!(slug("--if-version"), "if_version");
        assert_eq!(slug("9 lives"), "arg_9_lives");
    }
}
