//! What a program's own text files say about its options: a completion
//! file of the fish shell and a manual page, each handed to the scout as a
//! file of the project (decision 0263) and read as data (decision 0284).
//!
//! The readers are in [`crate::fish`] and [`crate::manual`]. This module
//! holds what they give back, and puts what they say beside what the
//! program's help says of each listed command. Nothing here starts a
//! program, and nothing a file says is ever run.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::draft::usage_parts;

/// The name under which a fish completion file is handed to the scout.
pub const COMPLETIONS: &str = "completions";
/// The name under which a manual page is handed to the scout.
pub const MANUAL: &str = "manual";
/// The name under which a PowerShell XML help file is handed to the scout.
pub const POWERSHELL_HELP: &str = "powershell_help";
/// The name under which a PowerShell module manifest is handed to the scout.
pub const MODULE_MANIFEST: &str = "module_manifest";
/// Where the help's own statement of an option comes from.
pub const HELP: &str = "help";

/// The most options one file may state; the rest are counted unread.
pub(crate) const MOST_OPTIONS: usize = 2_000;
/// The longest description kept, in characters, after it was redacted.
pub(crate) const LONGEST_ABOUT: usize = 400;

/// Whether an option takes a value, as one source states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Takes {
    /// It takes a value.
    Value,
    /// It takes none: it is a switch.
    Nothing,
    /// The source does not say. A fish line without `-r` or `-x` says
    /// nothing either way.
    NotStated,
}

impl Takes {
    /// Reads the name a finding's text gives it.
    pub(crate) fn from_str(text: &str) -> Option<Self> {
        match text {
            "value" => Some(Self::Value),
            "nothing" => Some(Self::Nothing),
            "not_stated" => Some(Self::NotStated),
            _ => None,
        }
    }

    /// Whether two statements contradict each other.
    fn contradicts(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Value, Self::Nothing) | (Self::Nothing, Self::Value)
        )
    }
}

/// One option as one file states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileOption {
    /// Every name the line or entry gives it, as written: `--format`, `-f`.
    pub flags: Vec<String>,
    /// Whether it takes a value.
    pub takes: Takes,
    /// The value's name, where the source gives one.
    pub value: Option<String>,
    /// What the source says the option does, redacted.
    pub about: Option<String>,
}

/// Which of the program's commands a statement is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Applies {
    /// The program as a whole, before or without a command.
    Program,
    /// Every listed command whose words hold, for each group, one of its
    /// words: fish's `__fish_seen_subcommand_from`, one group per call.
    Seen(Vec<Vec<String>>),
    /// The one command whose words, joined by `-`, are this name: a manual
    /// page of its own, such as the page `example-cli-item-list`.
    Exactly(String),
}

impl Applies {
    fn fits(&self, command: &[&str]) -> bool {
        match self {
            Self::Program => false,
            Self::Seen(groups) => groups
                .iter()
                .all(|group| group.iter().any(|word| command.contains(&word.as_str()))),
            Self::Exactly(name) => command.join("-") == *name,
        }
    }

    fn words(&self) -> String {
        match self {
            Self::Program => String::new(),
            Self::Seen(groups) => groups
                .iter()
                .map(|group| group.join("|"))
                .collect::<Vec<_>>()
                .join(" "),
            Self::Exactly(name) => name.clone(),
        }
    }
}

/// One statement a file makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stated {
    pub applies: Applies,
    pub option: FileOption,
}

/// What one reader made of one file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct FileRead {
    /// The form the file was read in: `fish`, `man`, or `mdoc`.
    pub form: &'static str,
    /// What it states, in the order it states it.
    pub stated: Vec<Stated>,
    /// How many lines (fish) or option entries (manual page) were read.
    pub read: u64,
    /// How many were not, by reason.
    pub unread: BTreeMap<&'static str, u64>,
}

impl FileRead {
    pub(crate) fn not_read(&mut self, why: &'static str) {
        *self.unread.entry(why).or_default() += 1;
    }

    pub(crate) fn unread_total(&self) -> u64 {
        self.unread.values().sum()
    }

    /// Takes one statement, within the bound.
    pub(crate) fn state(&mut self, applies: Applies, option: FileOption) {
        if self.stated.len() == MOST_OPTIONS {
            self.not_read("bound");
            return;
        }
        self.read += 1;
        self.stated.push(Stated { applies, option });
    }
}

/// What one source says of one option of one command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSaid {
    /// `help`, `completions`, or `manual`.
    pub from: String,
    /// Whether it takes a value.
    pub takes: Takes,
    /// The value's name, where the source gives one.
    pub value: Option<String>,
    /// What the source says it does. The help's usage line says nothing.
    pub about: Option<String>,
}

/// One option of a command that a handed file states, with what each
/// source says of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionFound {
    /// Every name the sources give it, in the order first seen.
    pub flags: Vec<String>,
    /// What each source says, the help first when its usage line shows it.
    pub said: Vec<OptionSaid>,
    /// Whether one source says it takes a value and another that it takes
    /// none. Neither is chosen.
    pub disagree: bool,
}

impl OptionFound {
    /// Whether the program's own help shows the option.
    #[must_use]
    pub fn in_help(&self) -> bool {
        self.said.iter().any(|said| said.from == HELP)
    }

    /// Whether a source names this flag.
    #[must_use]
    pub fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|known| known == flag)
    }
}

/// One file handed to the scout, read.
pub(crate) struct Handed {
    /// `completions` or `manual`.
    pub name: &'static str,
    pub read: FileRead,
}

/// The options the handed files state of one listed command, beside what
/// its usage line shows. Empty when no file says anything of it.
pub(crate) fn options_of(
    command: &str,
    usage: Option<&str>,
    files: &[&Handed],
) -> Vec<OptionFound> {
    let words: Vec<&str> = command.split(' ').collect();
    let mut options: Vec<OptionFound> = Vec::new();
    let add = |flags: &[String], said: OptionSaid, options: &mut Vec<OptionFound>| {
        let at = options
            .iter()
            .position(|known| flags.iter().any(|flag| known.has(flag)));
        let option = match at {
            Some(at) => &mut options[at],
            None => {
                options.push(OptionFound {
                    flags: Vec::new(),
                    said: Vec::new(),
                    disagree: false,
                });
                let last = options.len() - 1;
                &mut options[last]
            }
        };
        for flag in flags {
            if !option.has(flag) {
                option.flags.push(flag.clone());
            }
        }
        option.disagree |= option
            .said
            .iter()
            .any(|known| known.takes.contradicts(said.takes));
        option.said.push(said);
    };
    for file in files {
        for stated in &file.read.stated {
            if stated.applies.fits(&words) {
                let option = &stated.option;
                add(
                    &option.flags,
                    OptionSaid {
                        from: file.name.to_owned(),
                        takes: option.takes,
                        value: option.value.clone(),
                        about: option.about.clone(),
                    },
                    &mut options,
                );
            }
        }
    }
    if options.is_empty() {
        return options;
    }
    // What the usage line shows, for the options a file states: put first,
    // and compared like any other source.
    if let Some(parts) = usage.and_then(usage_parts) {
        for part in parts {
            let Some(flag) = part.flag else { continue };
            let Some(at) = options.iter().position(|known| known.has(&flag)) else {
                continue;
            };
            let said = OptionSaid {
                from: HELP.to_owned(),
                takes: if part.place.is_some() {
                    Takes::Value
                } else {
                    Takes::Nothing
                },
                value: part.place,
                about: None,
            };
            let option = &mut options[at];
            option.disagree |= option
                .said
                .iter()
                .any(|known| known.takes.contradicts(said.takes));
            option.said.insert(0, said);
        }
    }
    options
}

/// What a file states that fits no listed command: the program's own
/// options, and the words of commands the help does not list.
pub(crate) fn beyond_commands(
    read: &FileRead,
    listed: &[String],
) -> (Vec<FileOption>, Vec<String>) {
    let mut program = Vec::new();
    let mut unlisted: Vec<String> = Vec::new();
    for stated in &read.stated {
        if stated.applies == Applies::Program {
            program.push(stated.option.clone());
            continue;
        }
        let fits_one = listed.iter().any(|command| {
            let words: Vec<&str> = command.split(' ').collect();
            stated.applies.fits(&words)
        });
        let words = stated.applies.words();
        if !fits_one && !unlisted.contains(&words) {
            unlisted.push(words);
        }
    }
    (program, unlisted)
}

/// Whether a flag is a plain option name: one or two dashes, then a letter
/// or digit, then letters, digits, `-`, `_`, or `.`.
pub(crate) fn plain_flag(flag: &str) -> bool {
    let name = flag
        .strip_prefix("--")
        .or_else(|| flag.strip_prefix('-'))
        .unwrap_or("");
    flag.len() <= 66
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// Whether a value's name is plain: letters, digits, spaces, `-`, `_`,
/// `.`, or `/`, at most 40 characters.
pub(crate) fn plain_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " -_./".contains(c))
}

/// A description, redacted whole after it was put together and then cut.
pub(crate) fn about(text: &str, redact: &dyn a2m_core::Redact) -> Option<String> {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        return None;
    }
    let clean = redact.redact(&joined);
    let cut: String = clean
        .chars()
        .filter(|c| !c.is_control())
        .take(LONGEST_ABOUT)
        .collect();
    Some(if clean.chars().count() > LONGEST_ABOUT {
        format!("{cut}…")
    } else {
        cut
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Applies, FileOption, FileRead, Handed, Stated, Takes, beyond_commands, options_of,
    };

    fn option(flags: &[&str], takes: Takes, about: &str) -> FileOption {
        FileOption {
            flags: flags.iter().map(|f| (*f).to_owned()).collect(),
            takes,
            value: None,
            about: Some(about.to_owned()),
        }
    }

    fn file(name: &'static str, stated: Vec<(Applies, FileOption)>) -> Handed {
        Handed {
            name,
            read: FileRead {
                form: "test",
                read: stated.len() as u64,
                stated: stated
                    .into_iter()
                    .map(|(applies, option)| Stated { applies, option })
                    .collect(),
                ..FileRead::default()
            },
        }
    }

    #[test]
    fn each_source_is_kept_and_a_contradiction_is_reported_not_settled() {
        let seen_set = || Applies::Seen(vec![vec!["set".to_owned()]]);
        let files = [
            file(
                "completions",
                vec![
                    (
                        seen_set(),
                        option(&["--yes"], Takes::NotStated, "Do not ask"),
                    ),
                    (
                        seen_set(),
                        option(&["-n", "--dry-run"], Takes::Nothing, "Try"),
                    ),
                    (Applies::Program, option(&["--home"], Takes::Value, "Home")),
                ],
            ),
            file(
                "manual",
                vec![(
                    Applies::Exactly("item-set".to_owned()),
                    option(&["--yes"], Takes::Value, "An answer"),
                )],
            ),
        ];
        let usage = "<sku> --quantity=<whole number> [--yes]";
        let files: Vec<&Handed> = files.iter().collect();
        let options = options_of("item set", Some(usage), &files);
        assert_eq!(options.len(), 2);
        let yes = &options[0];
        assert_eq!(yes.flags, ["--yes"]);
        let from: Vec<&str> = yes.said.iter().map(|said| said.from.as_str()).collect();
        assert_eq!(from, ["help", "completions", "manual"]);
        assert!(yes.disagree, "the help says a switch, the manual a value");
        let dry = &options[1];
        assert!(!dry.in_help() && !dry.disagree);
        // A command no file speaks of has no options at all.
        assert!(options_of("item show", Some("<sku>"), &files).is_empty());
        // Words that fit no listed command, and the program's own options.
        let listed = ["item show".to_owned()];
        let (program, unlisted) = beyond_commands(&files[0].read, &listed);
        assert_eq!(program[0].flags, ["--home"]);
        assert_eq!(program.len(), 1);
        assert_eq!(unlisted, ["set"]);
    }
}
