//! A reader of the fish shell's completion files, as data.
//!
//! A fish completion file is a script, and most of its lines are calls of
//! the `complete` builtin with literal arguments. This reader takes only
//! those: a `complete` call alone on its line, outside any block, whose
//! every argument is literal text, which names this program with `-c`, and
//! whose condition, where it has one, is a literal list of the program's
//! command words. Every other line is counted as unread, by reason, and
//! nothing in the file is ever run (decisions 0279 and 0284).

use std::collections::BTreeMap;

use crate::metadata::{Applies, FileOption, FileRead, Takes, plain_flag};

/// The longest logical line read, in bytes.
const LONGEST_LINE: usize = 4_096;
/// The most arguments one `complete` call may have.
const MOST_TOKENS: usize = 64;

/// Why a token or a line is not read.
type Unread = &'static str;

/// The logical lines of a file: a line ends at a line end outside quotes
/// that no backslash escapes. A logical line that spans more than one line
/// of the file is kept whole and marked.
fn logical_lines(text: &str) -> Vec<(String, bool)> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut spans = false;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    // A comment runs to the end of its line, whatever it holds.
    let mut comment = false;
    for c in text.chars() {
        if comment {
            if c != '\n' {
                line.push(c);
                continue;
            }
            comment = false;
        } else if !escaped
            && quote.is_none()
            && c == '#'
            && line.chars().last().is_none_or(char::is_whitespace)
        {
            comment = true;
            line.push(c);
            continue;
        }
        if escaped {
            escaped = false;
            if c == '\n' {
                spans = true;
            }
            line.push(c);
            continue;
        }
        match (quote, c) {
            (_, '\\') => escaped = true,
            (None, '\'' | '"') => quote = Some(c),
            (Some(open), close) if open == close => quote = None,
            (Some(_), '\n') => spans = true,
            (None, '\n') => {
                lines.push((std::mem::take(&mut line), spans));
                spans = false;
                continue;
            }
            _ => {}
        }
        line.push(c);
    }
    if !line.trim().is_empty() {
        lines.push((line, spans || quote.is_some()));
    }
    lines
}

/// The tokens of one logical line, when every one is literal text.
fn tokens(line: &str) -> Result<Vec<String>, Unread> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| *c == ' ' || *c == '\t') {
            chars.next();
        }
        let Some(&first) = chars.peek() else { break };
        if first == '#' {
            break;
        }
        let mut token = String::new();
        while let Some(&c) = chars.peek() {
            match c {
                ' ' | '\t' => break,
                '\'' => {
                    chars.next();
                    loop {
                        match chars.next() {
                            None => return Err("not_literal"),
                            Some('\'') => break,
                            Some('\\') => match chars.next() {
                                Some(next @ ('\'' | '\\')) => token.push(next),
                                Some(next) => {
                                    token.push('\\');
                                    token.push(next);
                                }
                                None => return Err("not_literal"),
                            },
                            Some(other) => token.push(other),
                        }
                    }
                }
                '"' => {
                    chars.next();
                    loop {
                        match chars.next() {
                            None => return Err("not_literal"),
                            Some('"') => break,
                            Some('$') => return Err("variable"),
                            Some('\\') => return Err("not_literal"),
                            Some(other) => token.push(other),
                        }
                    }
                }
                '$' => return Err("variable"),
                '(' | ')' => return Err("command_substitution"),
                ';' | '|' | '&' => return Err("more_than_one_command"),
                '{' | '}' | '*' | '?' | '<' | '>' | '\\' => return Err("not_literal"),
                '~' if token.is_empty() => return Err("not_literal"),
                other => {
                    chars.next();
                    token.push(other);
                }
            }
        }
        out.push(token);
        if out.len() > MOST_TOKENS {
            return Err("too_long");
        }
    }
    Ok(out)
}

/// The words that start the commands of a line, read loosely: for keeping
/// count of the blocks a file opens and closes, never for reading it.
fn command_words(line: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (at, c) in line.char_indices() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(open), close) if open == close => quote = None,
            (None, ';') => {
                words.extend(line[start..at].split_whitespace().next());
                start = at + 1;
            }
            _ => {}
        }
    }
    words.extend(line[start..].split_whitespace().next());
    words
}

const OPENS: &[&str] = &["function", "if", "for", "while", "switch", "begin"];

/// Whether a word is a plain command word: a letter or digit, then
/// letters, digits, `_`, `.`, `:`, or `-`.
fn plain_word(word: &str) -> bool {
    word.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
        && word.len() <= 64
}

/// A condition, when it is a literal list of command words.
fn condition(text: &str) -> Result<Applies, Unread> {
    let text = text.trim();
    if text == "__fish_use_subcommand" {
        return Ok(Applies::Program);
    }
    let mut groups = Vec::new();
    for part in text.split("; and ").flat_map(|part| part.split(" && ")) {
        let mut words = part.split_whitespace();
        if words.next() != Some("__fish_seen_subcommand_from") {
            return Err("condition");
        }
        let group: Vec<String> = words.map(str::to_owned).collect();
        if group.is_empty() || !group.iter().all(|word| plain_word(word)) {
            return Err("condition");
        }
        groups.push(group);
    }
    Ok(Applies::Seen(groups))
}

/// What one `complete` call states, read from its literal arguments.
fn complete(args: &[String], program: &str) -> Result<(Applies, FileOption), Unread> {
    let mut command = None;
    let mut applies = None;
    let mut flags = Vec::new();
    let mut takes = Takes::NotStated;
    let mut about = None;
    let mut at = 0;
    // Each option of `complete`: its letter, whether it takes a value, and
    // its long name.
    const SPEC: &[(char, bool, &str)] = &[
        ('c', true, "command"),
        ('p', true, "path"),
        ('n', true, "condition"),
        ('a', true, "arguments"),
        ('d', true, "description"),
        ('s', true, "short-option"),
        ('l', true, "long-option"),
        ('o', true, "old-option"),
        ('w', true, "wraps"),
        ('r', false, "require-parameter"),
        ('x', false, "exclusive"),
        ('f', false, "no-files"),
        ('F', false, "force-files"),
        ('k', false, "keep-order"),
        ('e', false, "erase"),
        ('C', true, "do-complete"),
        ('h', false, "help"),
    ];
    let mut take = |letter: char, value: Option<String>| -> Result<(), Unread> {
        match (letter, value) {
            ('c', Some(value)) => {
                if command.replace(value).is_some() {
                    return Err("not_read_form");
                }
            }
            ('n', Some(value)) => {
                if applies.replace(condition(&value)?).is_some() {
                    return Err("not_read_form");
                }
            }
            ('a', Some(value)) => {
                // Offered values are worked out by fish when it completes:
                // any substitution in them is a program fish would run.
                if value.contains('(') {
                    return Err("command_substitution");
                }
                if value.contains('$') {
                    return Err("variable");
                }
                if value.contains(['{', '}', '*', '?', '\\', ';', '|', '&', '<', '>']) {
                    return Err("not_literal");
                }
            }
            ('d', Some(value)) => about = Some(value),
            ('s', Some(value)) => {
                let flag = format!("-{value}");
                if value.chars().count() != 1 || !plain_flag(&flag) {
                    return Err("not_read_form");
                }
                flags.push(flag);
            }
            ('l', Some(value)) => flags.push(format!("--{value}")),
            ('o', Some(value)) => flags.push(format!("-{value}")),
            ('r' | 'x', None) => takes = Takes::Value,
            ('f' | 'F' | 'k', None) => {}
            // A path, a wrapped command, erasing, completing, and help say
            // something other than what this program's options are.
            _ => return Err("not_read_form"),
        }
        Ok(())
    };
    while at < args.len() {
        let arg = &args[at];
        at += 1;
        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (long, None),
            };
            let Some(&(letter, valued, _)) = SPEC.iter().find(|(_, _, known)| *known == name)
            else {
                return Err("not_read_form");
            };
            let value = match (valued, inline) {
                (true, Some(value)) => Some(value),
                (true, None) => {
                    let value = args.get(at).ok_or("not_read_form")?.clone();
                    at += 1;
                    Some(value)
                }
                (false, None) => None,
                (false, Some(_)) => return Err("not_read_form"),
            };
            take(letter, value)?;
        } else if let Some(cluster) = arg.strip_prefix('-').filter(|rest| !rest.is_empty()) {
            for (index, letter) in cluster.char_indices() {
                let Some(&(_, valued, _)) = SPEC.iter().find(|(known, _, _)| *known == letter)
                else {
                    return Err("not_read_form");
                };
                if valued {
                    let rest = &cluster[index + letter.len_utf8()..];
                    let value = if rest.is_empty() {
                        let value = args.get(at).ok_or("not_read_form")?.clone();
                        at += 1;
                        value
                    } else {
                        rest.to_owned()
                    };
                    take(letter, Some(value))?;
                    break;
                }
                take(letter, None)?;
            }
        } else {
            return Err("not_read_form");
        }
    }
    if command.as_deref() != Some(program) {
        return Err("another_command");
    }
    if flags.is_empty() {
        return Err("names_no_option");
    }
    if !flags.iter().all(|flag| plain_flag(flag)) {
        return Err("not_read_form");
    }
    Ok((
        applies.unwrap_or(Applies::Program),
        FileOption {
            flags,
            takes,
            value: None,
            about,
        },
    ))
}

/// Reads a fish completion file. `program` is the name `-c` must give:
/// the program's file name. Every description passes `redact`.
pub(crate) fn read(text: &str, program: &str, redact: &dyn a2m_core::Redact) -> FileRead {
    let mut out = FileRead {
        form: "fish",
        ..FileRead::default()
    };
    let mut depth: usize = 0;
    // A file written on Windows ends its lines with a carriage return too.
    let text = text.replace("\r\n", "\n");
    for (line, spans) in logical_lines(&text) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Blocks are counted from the words that start each command, so a
        // `complete` inside a function or a condition is never read.
        let inside = depth > 0;
        for word in command_words(trimmed) {
            if OPENS.contains(&word) {
                depth += 1;
            } else if word == "end" {
                depth = depth.saturating_sub(1);
            }
        }
        let first = trimmed.split_whitespace().next().unwrap_or_default();
        let why = if inside || OPENS.contains(&first) || first == "end" {
            Some("block")
        } else if spans {
            Some("continued")
        } else if line.len() > LONGEST_LINE {
            Some("too_long")
        } else {
            None
        };
        if let Some(why) = why {
            out.not_read(why);
            continue;
        }
        let read = tokens(trimmed).and_then(|tokens| match tokens.split_first() {
            Some((first, args)) if first == "complete" => complete(args, program),
            _ => Err("other_command"),
        });
        match read {
            Ok((applies, mut option)) => {
                option.about = option
                    .about
                    .as_deref()
                    .and_then(|about| crate::metadata::about(about, redact));
                out.state(applies, option);
            }
            Err(why) => out.not_read(why),
        }
    }
    out
}

/// The unread counts as a map of plain names, for a finding.
pub(crate) fn unread_counts(read: &FileRead) -> BTreeMap<String, u64> {
    read.unread
        .iter()
        .map(|(why, count)| ((*why).to_owned(), *count))
        .collect()
}

#[cfg(test)]
mod tests {
    use a2m_core::NoSecrets;

    use super::read;
    use crate::metadata::{Applies, Takes};

    #[test]
    fn plain_complete_lines_are_read_and_every_other_line_is_counted() {
        let text = concat!(
            "# a comment, and a blank line, are not lines of the program's\n",
            "\n",
            "complete -c example-cli -f\n",
            "complete -c example-cli -l home -r -d 'The folder that holds the data'\n",
            "complete -c example-cli -n '__fish_seen_subcommand_from show' -s f -l format -x -a 'text json' -d \"How to print it\"\n",
            "complete -c example-cli -n '__fish_seen_subcommand_from item; and __fish_seen_subcommand_from set' -l yes -d 'Do not ask'\n",
            "complete --command=example-cli --long-option=quiet\n",
            "complete -xc example-cli -l rows\n",
            "complete -c example-cli -l location -x -a '(example-cli item list)'\n",
            "complete -c example-cli -l ref -a \"$refs\"\n",
            "complete -c example-cli -n '__example_needs_item' -l x\n",
            "complete -c other-program -l other\n",
            "complete -c example-cli -l a; complete -c example-cli -l b\n",
            "set -l refs a b\n",
            "function __example_needs_item\n",
            "    complete -c example-cli -l hidden\n",
            "end\n",
            "complete -c example-cli \\\n",
            "    -l continued\n",
            "complete -c example-cli -w other-program\n",
            "complete -c example-cli -s ab\n",
        );
        let read = read(text, "example-cli", &NoSecrets);
        let flags: Vec<Vec<&str>> = read
            .stated
            .iter()
            .map(|s| s.option.flags.iter().map(String::as_str).collect())
            .collect();
        assert_eq!(
            flags,
            [
                vec!["--home"],
                vec!["-f", "--format"],
                vec!["--yes"],
                vec!["--quiet"],
                vec!["--rows"],
            ]
        );
        assert_eq!(read.read, 5);
        assert_eq!(read.stated[0].applies, Applies::Program);
        assert_eq!(read.stated[0].option.takes, Takes::Value);
        assert_eq!(
            read.stated[0].option.about.as_deref(),
            Some("The folder that holds the data")
        );
        assert_eq!(
            read.stated[1].applies,
            Applies::Seen(vec![vec!["show".to_owned()]])
        );
        assert_eq!(
            read.stated[2].applies,
            Applies::Seen(vec![vec!["item".to_owned()], vec!["set".to_owned()]])
        );
        assert_eq!(read.stated[2].option.takes, Takes::NotStated);
        assert_eq!(
            read.stated[4].option.takes,
            Takes::Value,
            "`-xc` is `-x -c`"
        );
        let unread: Vec<(&str, u64)> = read.unread.iter().map(|(w, n)| (*w, *n)).collect();
        assert_eq!(
            unread,
            [
                ("another_command", 1),
                ("block", 3),
                ("command_substitution", 1),
                ("condition", 1),
                ("continued", 1),
                ("more_than_one_command", 1),
                ("names_no_option", 1),
                ("not_read_form", 2),
                ("other_command", 1),
                ("variable", 1),
            ]
        );
        assert_eq!(read.unread_total(), 13);
    }

    #[test]
    fn a_description_passes_the_redactor() {
        struct Planted;
        impl a2m_core::Redact for Planted {
            fn redact(&self, text: &str) -> String {
                text.replace("secret-for-tests-1", a2m_core::REDACTED)
            }
        }
        let text = "complete -c example-cli -l token -d 'uses secret-for-tests-1 here'\n";
        let read = read(text, "example-cli", &Planted);
        let about = read.stated[0].option.about.as_deref().unwrap();
        assert!(!about.contains("secret-for-tests-1"), "{about}");
        assert!(about.contains(a2m_core::REDACTED), "{about}");
    }
}
