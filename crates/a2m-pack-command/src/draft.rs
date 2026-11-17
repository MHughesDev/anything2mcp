//! Drafting: from one `command` finding to one draft `commands/` file.
//!
//! The draft holds what the program was seen to say: its digest, its
//! version, the command's words, and the arguments its usage line shows.
//! It holds no effect, no exit status beyond `0`, no output form, and no
//! read-back: each of those is an open question for a person.

use a2m_core::{FileKindName, Finding, FindingKind};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath};

use crate::codes::{self, FILE_KIND, FOLDER};
use crate::discover::CommandFound;

/// The drafting rule from `command` findings to `commands/` files.
pub(crate) struct CommandFromFinding;

/// A lowercase name made of a text's letters and digits.
fn slug(text: &str) -> String {
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
        let args = match parts {
            Some(Some(parts)) => {
                for part in parts {
                    let typed = |place: &str| {
                        let lower = place.to_ascii_lowercase();
                        if lower.contains("whole number") || lower.contains("integer") {
                            DraftValue::text("integer")
                        } else {
                            cx.open_question_quoting("What type is the value that the usage line of the command writes in the place quoted? The first quoted words are the command, the second the place. A text param also needs an anchored `pattern`.", &[listed, place])
                        }
                    };
                    let mut param = |name: String, ty: DraftValue, optional: bool| {
                        let mut fields = vec![("type", ty)];
                        if optional {
                            fields.push(("required", DraftValue::Bool(false)));
                        }
                        if !params.iter().any(|(known, _)| *known == name) {
                            params.push((name, DraftValue::map(fields)));
                        }
                    };
                    match (part.flag, part.place) {
                        (None, Some(place)) => {
                            let name = slug(&place);
                            param(name.clone(), typed(&place), part.optional);
                            args.push(DraftValue::map(vec![("param", DraftValue::Text(name))]));
                        }
                        (Some(flag), Some(place)) => {
                            let name = slug(&flag);
                            param(name.clone(), typed(&place), part.optional);
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
                            param(name.clone(), DraftValue::text("boolean"), true);
                            args.push(DraftValue::map(vec![
                                ("flag", DraftValue::Text(flag)),
                                ("when", DraftValue::Text(name)),
                            ]));
                        }
                        (Some(flag), None) => args.push(DraftValue::Text(flag)),
                        (None, None) => {}
                    }
                }
                DraftValue::List(args)
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

#[cfg(test)]
mod tests {
    use super::{UsagePart, slug, usage_parts};

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
