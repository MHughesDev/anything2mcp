//! A reader of manual pages, for the options they list.
//!
//! A manual page's source is text in one of two macro families: the older
//! `man` macros and the `mdoc` macros. This reader does the smallest true
//! thing with each: it finds the section that lists options (`OPTIONS`; in
//! an `mdoc` page that has none, the tagged lists of `DESCRIPTION`) and
//! reads each entry's flag names, whether it takes a value, and its
//! description, in the common forms. An entry in any other form is counted
//! as unread, by reason. A page compressed with gzip is inflated first,
//! within the same bound; a page compressed any other way must be given
//! uncompressed. Nothing in a page is ever run, and no file it names is
//! read (decision 0284).

use a2m_core::Redact;

use crate::metadata::{Applies, FileOption, FileRead, Takes, about, plain_flag, plain_value};

/// Why an entry is not read.
type Unread = &'static str;

/// Why a whole file cannot be read as a manual page.
pub(crate) type NotAPage = &'static str;

// ---- the bytes: plain text or gzip ----

/// The page's text: its bytes as UTF-8, or inflated first when they are
/// gzip. At most `limit` bytes of text.
pub(crate) fn text_of(bytes: &[u8], limit: usize) -> Result<String, NotAPage> {
    let other = [
        &b"BZh"[..],
        &[0xFD, b'7', b'z', b'X', b'Z', 0x00][..],
        &[0x28, 0xB5, 0x2F, 0xFD][..],
        &[0x1F, 0x9D][..],
    ];
    let plain = if bytes.starts_with(&[0x1F, 0x8B]) {
        gunzip(bytes, limit)?
    } else if other.iter().any(|magic| bytes.starts_with(magic)) {
        return Err("it is compressed in a form other than gzip; give the page uncompressed");
    } else {
        bytes.to_vec()
    };
    String::from_utf8(plain).map_err(|_| "it is not UTF-8 text")
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let low = crc & 1;
            crc >>= 1;
            if low == 1 {
                crc ^= 0xEDB8_8320;
            }
        }
    }
    !crc
}

/// One gzip member, checked by its length and its CRC.
fn gunzip(bytes: &[u8], limit: usize) -> Result<Vec<u8>, NotAPage> {
    const DAMAGED: NotAPage = "its gzip form is damaged or holds more than one member";
    let byte = |at: usize| bytes.get(at).copied().ok_or(DAMAGED);
    if byte(2)? != 8 {
        return Err(DAMAGED);
    }
    let flags = byte(3)?;
    if flags & 0xE0 != 0 {
        return Err(DAMAGED);
    }
    let mut at = 10;
    if flags & 0x04 != 0 {
        let extra = usize::from(byte(at)?) | (usize::from(byte(at + 1)?) << 8);
        at += 2 + extra;
    }
    for bit in [0x08, 0x10] {
        if flags & bit != 0 {
            while byte(at)? != 0 {
                at += 1;
            }
            at += 1;
        }
    }
    if flags & 0x02 != 0 {
        at += 2;
    }
    let end = bytes.len().checked_sub(8).ok_or(DAMAGED)?;
    let packed = bytes.get(at..end).ok_or(DAMAGED)?;
    let trailer = bytes.get(end..).ok_or(DAMAGED)?;
    let word = |from: usize| {
        u32::from_le_bytes([
            trailer[from],
            trailer[from + 1],
            trailer[from + 2],
            trailer[from + 3],
        ])
    };
    let text = miniz_oxide::inflate::decompress_to_vec_with_limit(packed, limit)
        .map_err(|_| "its gzip form is damaged, or it is larger than the discovery may read")?;
    // The trailer holds the length modulo 2^32.
    let length = u32::try_from(u64::try_from(text.len()).unwrap_or(u64::MAX) & 0xFFFF_FFFF)
        .unwrap_or(u32::MAX);
    if crc32(&text) != word(0) || length != word(4) {
        return Err(DAMAGED);
    }
    Ok(text)
}

// ---- roff text ----

/// A special character by its name, for the few that option lists use.
fn named(name: &str) -> Option<&'static str> {
    Some(match name {
        "em" => "\u{2014}",
        "en" => "\u{2013}",
        "aq" | "oq" | "cq" => "'",
        "dq" | "lq" | "rq" => "\"",
        "hy" | "mi" => "-",
        "bu" => "\u{2022}",
        "ti" => "~",
        "ha" => "^",
        "rs" => "\\",
        "lt" | "la" => "<",
        "rt" | "ra" => ">",
        _ => return None,
    })
}

/// A line of roff text as plain text: fonts dropped, the common escapes
/// written out. An escape this reader does not know is not guessed at.
fn plain(line: &str) -> Result<String, Unread> {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('f') => match chars.next() {
                Some('B' | 'I' | 'R' | 'P' | '1' | '2' | '3' | '4') => {}
                Some('(') => {
                    chars.next();
                    chars.next();
                }
                Some('[') => {
                    for c in chars.by_ref() {
                        if c == ']' {
                            break;
                        }
                    }
                }
                _ => return Err("escape"),
            },
            Some('-') => out.push('-'),
            Some('\\' | 'e') => out.push('\\'),
            Some('&' | '|' | '^' | ')' | 'c') => {}
            Some(' ' | '~' | '0') => out.push(' '),
            Some('\'') => out.push('\''),
            Some('`') => out.push('`'),
            Some('.') => out.push('.'),
            Some('"') => break,
            Some('(') => {
                let name: String = chars.by_ref().take(2).collect();
                out.push_str(named(&name).ok_or("escape")?);
            }
            Some('[') => {
                let mut name = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    name.push(c);
                }
                out.push_str(named(&name).ok_or("escape")?);
            }
            _ => return Err("escape"),
        }
    }
    Ok(out)
}

/// A request line's name and its arguments, quoted as roff quotes them.
fn request(line: &str) -> Option<(&str, Vec<String>)> {
    let rest = line.strip_prefix(['.', '\''])?.trim_start();
    let (name, rest) = rest.split_once([' ', '\t']).unwrap_or((rest, ""));
    let mut args = Vec::new();
    let mut chars = rest.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| *c == ' ' || *c == '\t') {
            chars.next();
        }
        let Some(&first) = chars.peek() else { break };
        let mut arg = String::new();
        if first == '"' {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        arg.push('"');
                    } else {
                        break;
                    }
                } else {
                    arg.push(c);
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ' ' || c == '\t' {
                    break;
                }
                arg.push(c);
                chars.next();
            }
        }
        if arg.starts_with("\\\"") {
            break;
        }
        args.push(arg);
    }
    Some((name, args))
}

fn is_comment(line: &str) -> bool {
    line.starts_with(".\\\"")
        || line.starts_with("'\\\"")
        || line.starts_with(".\\#")
        || line == "."
}

/// The text of a font request of the `man` macros: `.B`, `.I`, and the
/// alternating ones, whose arguments are joined with no space.
fn font_text(name: &str, args: &[String]) -> Option<Result<String, Unread>> {
    let joiner = match name {
        "B" | "I" | "SM" | "SB" => " ",
        "BR" | "RB" | "BI" | "IB" | "IR" | "RI" => "",
        _ => return None,
    };
    Some(
        args.iter()
            .map(|arg| plain(arg))
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join(joiner)),
    )
}

// ---- flags, as an entry's tag writes them ----

/// What a tag such as `-f, --format FORM` or `--format=<form>` states.
fn tag_options(tag: &str) -> Result<(Vec<String>, Takes, Option<String>), Unread> {
    let mut flags = Vec::new();
    let mut value: Option<String> = None;
    for alternative in tag.split(',') {
        let alternative = alternative.trim();
        if alternative.contains('[') {
            return Err("optional_value");
        }
        let (flag, rest) = match alternative.find(['=', ' ']) {
            Some(at) => (&alternative[..at], alternative[at + 1..].trim()),
            None => (alternative, ""),
        };
        if !plain_flag(flag) {
            return Err("not_read_form");
        }
        flags.push(flag.to_owned());
        if !rest.is_empty() {
            let name = rest
                .strip_prefix('<')
                .and_then(|inner| inner.strip_suffix('>'))
                .unwrap_or(rest);
            if !plain_value(name) {
                return Err("not_read_form");
            }
            value.get_or_insert_with(|| name.to_owned());
        }
    }
    let takes = if value.is_some() {
        Takes::Value
    } else {
        Takes::Nothing
    };
    Ok((flags, takes, value))
}

/// An entry being read.
struct Entry {
    head: Result<(Vec<String>, Takes, Option<String>), Unread>,
    about: Vec<String>,
    broken: Option<Unread>,
}

impl Entry {
    fn new(head: Result<(Vec<String>, Takes, Option<String>), Unread>) -> Self {
        Self {
            head,
            about: Vec::new(),
            broken: None,
        }
    }

    fn text(&mut self, text: Result<String, Unread>) {
        match text {
            Ok(text) => self.about.push(text),
            Err(why) => {
                self.broken.get_or_insert(why);
            }
        }
    }

    fn finish(self, applies: Option<&Applies>, out: &mut FileRead, redact: &dyn Redact) {
        let Some(applies) = applies else {
            out.not_read("another_command");
            return;
        };
        match (self.head, self.broken) {
            (_, Some(why)) | (Err(why), None) => out.not_read(why),
            (Ok((flags, takes, value)), None) => out.state(
                applies.clone(),
                FileOption {
                    flags,
                    takes,
                    value,
                    about: about(&self.about.join(" "), redact),
                },
            ),
        }
    }
}

/// Which commands a page's options are of, from the page's own name.
fn applies(title: Option<&str>, program: &str) -> Option<Applies> {
    let title = title?.to_ascii_lowercase();
    if title == program {
        Some(Applies::Program)
    } else {
        title
            .strip_prefix(program)
            .and_then(|rest| rest.strip_prefix('-'))
            .filter(|rest| !rest.is_empty())
            .map(|rest| Applies::Exactly(rest.to_owned()))
    }
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(|line| line.trim_end_matches('\r'))
}

// ---- the `man` macros ----

fn read_man(text: &str, program: &str, redact: &dyn Redact) -> FileRead {
    let mut out = FileRead {
        form: "man",
        ..FileRead::default()
    };
    let title = lines(text)
        .filter_map(request)
        .find(|(name, _)| *name == "TH")
        .and_then(|(_, args)| args.first().cloned());
    let applies = applies(title.as_deref(), program);
    let mut in_options = false;
    let mut expect_tag = false;
    let mut entry: Option<Entry> = None;
    let finish = |entry: &mut Option<Entry>, out: &mut FileRead| {
        if let Some(done) = entry.take() {
            done.finish(applies.as_ref(), out, redact);
        }
    };
    for line in lines(text) {
        if is_comment(line) {
            continue;
        }
        let asked = request(line);
        if let Some(("SH", args)) = &asked {
            finish(&mut entry, &mut out);
            expect_tag = false;
            in_options = args.join(" ").eq_ignore_ascii_case("OPTIONS");
            continue;
        }
        if !in_options {
            continue;
        }
        if expect_tag {
            expect_tag = false;
            let tag = match &asked {
                Some((name, args)) => font_text(name, args),
                None => Some(plain(line)),
            };
            // A tag that is not an option is an entry of prose.
            let option_like = line.contains("\\-") || line.starts_with('-');
            entry = match tag {
                Some(Ok(tag)) if tag.trim_start().starts_with('-') => {
                    Some(Entry::new(tag_options(tag.trim())))
                }
                Some(Err(why)) if option_like => Some(Entry::new(Err(why))),
                None if option_like => Some(Entry::new(Err("request"))),
                _ => None,
            };
            continue;
        }
        match asked {
            Some(("TP", _)) => {
                finish(&mut entry, &mut out);
                expect_tag = true;
            }
            Some(("IP", args)) => {
                finish(&mut entry, &mut out);
                let tag = args.first().map(|tag| plain(tag));
                entry = match tag {
                    Some(Ok(tag)) if tag.starts_with('-') => Some(Entry::new(tag_options(&tag))),
                    Some(Err(why)) => Some(Entry::new(Err(why))),
                    _ => None,
                };
            }
            Some(("SS" | "PP" | "LP" | "P" | "HP", _)) => finish(&mut entry, &mut out),
            Some(("br" | "sp" | "ns" | "ad" | "na", _)) => {}
            Some((name, args)) => {
                if let Some(current) = entry.as_mut() {
                    match font_text(name, &args) {
                        Some(text) => current.text(text),
                        None => current.text(Err("request")),
                    }
                }
            }
            None => {
                if let Some(current) = entry.as_mut() {
                    current.text(plain(line));
                }
            }
        }
    }
    finish(&mut entry, &mut out);
    out
}

// ---- the `mdoc` macros ----

/// Every macro name of `mdoc`, so that one this reader does not render is
/// recognised as a macro and not taken for a word.
const MDOC: &[&str] = &[
    "Ad", "An", "Ap", "Ar", "At", "Bd", "Bf", "Bk", "Bl", "Bo", "Bq", "Brc", "Bro", "Brq", "Bsx",
    "Bt", "Bx", "Cd", "Cm", "D1", "Db", "Dc", "Dd", "Dl", "Do", "Dq", "Dt", "Dv", "Dx", "Ec", "Ed",
    "Ef", "Ek", "El", "Em", "En", "Eo", "Er", "Es", "Ev", "Ex", "Fa", "Fc", "Fd", "Fl", "Fn", "Fo",
    "Fr", "Ft", "Fx", "Hf", "Ic", "In", "It", "Lb", "Li", "Lk", "Lp", "Ms", "Mt", "Nd", "Nm", "No",
    "Ns", "Nx", "Oc", "Oo", "Op", "Os", "Ot", "Ox", "Pa", "Pc", "Pf", "Po", "Pp", "Pq", "Qc", "Ql",
    "Qo", "Qq", "Re", "Rs", "Rv", "Sc", "Sh", "Sm", "So", "Sq", "Ss", "St", "Sx", "Sy", "Ta", "Tn",
    "Ud", "Ux", "Va", "Vt", "Xc", "Xo", "Xr",
];

/// The inline macros whose text this reader writes out.
const RENDERED: &[&str] = &[
    "Ar", "Cm", "Dv", "Em", "Er", "Ev", "Fa", "Fl", "Ic", "Li", "Nm", "No", "Ns", "Pa", "Ql", "Dq",
    "Sq", "Qq", "Pq", "Sy", "Va", "Xr", "Ap",
];

fn is_macro(word: &str) -> bool {
    MDOC.contains(&word)
}

/// The text of an `mdoc` line's arguments, its macros written out.
fn mdoc_text(first: &str, args: &[String], name: &str) -> Result<String, Unread> {
    if !RENDERED.contains(&first) {
        return Err("request");
    }
    let mut words: Vec<String> = Vec::new();
    let mut glue = false;
    let mut current = first;
    let mut taken = 0;
    let push = |word: String, words: &mut Vec<String>, glue: &mut bool| {
        let punctuation = word.len() == 1 && ".,;:)]?!".contains(word.as_str());
        match words.last_mut() {
            Some(last) if *glue || punctuation => last.push_str(&word),
            _ => words.push(word),
        }
        *glue = false;
    };
    for arg in args.iter().map(String::as_str).chain(std::iter::once("\0")) {
        let punctuation = arg.len() == 1 && ".,;:)]?!".contains(arg);
        if arg == "\0" || is_macro(arg) || punctuation {
            // The macro before this one ends; one that took no argument
            // writes what it stands for alone.
            match (current, taken) {
                ("Nm", 0) => push(name.to_owned(), &mut words, &mut glue),
                ("Ar", 0) => push("file ...".to_owned(), &mut words, &mut glue),
                ("Fl", 0) => push("-".to_owned(), &mut words, &mut glue),
                _ => {}
            }
            taken = 1;
            if arg == "\0" {
                break;
            }
            if punctuation {
                push(arg.to_owned(), &mut words, &mut glue);
                continue;
            }
            if !RENDERED.contains(&arg) {
                return Err("request");
            }
            if arg == "Ns" {
                glue = true;
            }
            current = arg;
            taken = 0;
            continue;
        }
        let text = plain(arg)?;
        let text = match current {
            "Fl" => format!("-{text}"),
            "Xr" if taken == 1 => format!("({text})"),
            _ => text,
        };
        if current == "Xr" && taken == 1 {
            glue = true;
        }
        taken += 1;
        push(text, &mut words, &mut glue);
    }
    Ok(words.join(" "))
}

/// What an item's head such as `Fl f , Fl \-format Ar form` states.
fn mdoc_head(args: &[String]) -> Result<(Vec<String>, Takes, Option<String>), Unread> {
    let mut flags = Vec::new();
    let mut value: Option<String> = None;
    let mut at = 0;
    let word = |at: usize| args.get(at).map(String::as_str);
    loop {
        if word(at) != Some("Fl") {
            return Err("not_read_form");
        }
        let name = word(at + 1)
            .filter(|name| !is_macro(name))
            .ok_or("not_read_form")?;
        let flag = format!("-{}", plain(name)?);
        if !plain_flag(&flag) {
            return Err("not_read_form");
        }
        flags.push(flag);
        at += 2;
        let named = |at: usize| -> Result<String, Unread> {
            match word(at) {
                Some(name) if !is_macro(name) => {
                    let name = plain(name)?;
                    if plain_value(&name) {
                        Ok(name)
                    } else {
                        Err("not_read_form")
                    }
                }
                _ => Ok("file".to_owned()),
            }
        };
        match (word(at), word(at + 1), word(at + 2), word(at + 3)) {
            (Some("Ar"), ..) => {
                let name = named(at + 1)?;
                at += if word(at + 1).is_some_and(|w| !is_macro(w)) {
                    2
                } else {
                    1
                };
                value.get_or_insert(name);
            }
            (Some("Ns"), Some("="), Some("Ns"), Some("Ar")) => {
                let name = named(at + 4)?;
                at += if word(at + 4).is_some_and(|w| !is_macro(w)) {
                    5
                } else {
                    4
                };
                value.get_or_insert(name);
            }
            (Some("Op"), ..) => return Err("optional_value"),
            _ => {}
        }
        match word(at) {
            None => break,
            Some(",") => at += 1,
            Some(_) => return Err("not_read_form"),
        }
    }
    let takes = if value.is_some() {
        Takes::Value
    } else {
        Takes::Nothing
    };
    Ok((flags, takes, value))
}

fn read_mdoc(text: &str, program: &str, redact: &dyn Redact) -> FileRead {
    let mut out = FileRead {
        form: "mdoc",
        ..FileRead::default()
    };
    let asked: Vec<Option<(&str, Vec<String>)>> = lines(text).map(request).collect();
    let section = |args: &[String]| args.join(" ").to_ascii_uppercase();
    let has_options = asked
        .iter()
        .flatten()
        .any(|(name, args)| *name == "Sh" && section(args) == "OPTIONS");
    let wanted = if has_options {
        "OPTIONS"
    } else {
        "DESCRIPTION"
    };
    let title = asked
        .iter()
        .flatten()
        .find(|(name, _)| *name == "Dt")
        .and_then(|(_, args)| args.first().cloned());
    let page_name = asked
        .iter()
        .flatten()
        .find(|(name, args)| *name == "Nm" && !args.is_empty())
        .and_then(|(_, args)| args.first().cloned())
        .unwrap_or_else(|| program.to_owned());
    let applies = applies(title.as_deref(), program);
    let mut in_section = false;
    // The lists open, each `true` for a tagged list.
    let mut lists: Vec<bool> = Vec::new();
    let mut entry: Option<Entry> = None;
    // The depth an entry was opened at, so that a list inside it is part
    // of it.
    let mut entry_depth = 0;
    let finish = |entry: &mut Option<Entry>, out: &mut FileRead| {
        if let Some(done) = entry.take() {
            done.finish(applies.as_ref(), out, redact);
        }
    };
    for (line, asked) in lines(text).zip(asked.iter()) {
        if is_comment(line) {
            continue;
        }
        if let Some(("Sh", args)) = asked {
            finish(&mut entry, &mut out);
            lists.clear();
            in_section = section(args) == wanted;
            continue;
        }
        if !in_section {
            continue;
        }
        match asked {
            Some(("Bl", args)) => {
                if let Some(current) = entry.as_mut() {
                    current.text(Err("request"));
                }
                lists.push(args.first().is_some_and(|kind| kind == "-tag"));
            }
            Some(("El", _)) => {
                if entry.is_some() && lists.len() == entry_depth {
                    finish(&mut entry, &mut out);
                }
                lists.pop();
            }
            Some(("It", args)) if entry.is_none() || lists.len() == entry_depth => {
                finish(&mut entry, &mut out);
                if args.first().is_some_and(|first| first == "Fl") {
                    entry_depth = lists.len();
                    let head = if lists.len() == 1 && lists[0] {
                        mdoc_head(args)
                    } else {
                        Err("list_form")
                    };
                    entry = Some(Entry::new(head));
                }
            }
            Some(("Pp" | "Lp" | "br", _)) => {}
            Some((name, args)) => {
                if let Some(current) = entry.as_mut() {
                    current.text(if is_macro(name) {
                        mdoc_text(name, args, &page_name)
                    } else {
                        Err("request")
                    });
                }
            }
            None => {
                if let Some(current) = entry.as_mut() {
                    current.text(plain(line));
                }
            }
        }
    }
    finish(&mut entry, &mut out);
    out
}

/// Reads a manual page's text. `program` is the program's file name: a
/// page named for it lists the program's own options, and a page named
/// `<program>-<words>` those of the command with those words.
///
/// # Errors
///
/// When the text is in neither macro family.
pub(crate) fn read(text: &str, program: &str, redact: &dyn Redact) -> Result<FileRead, NotAPage> {
    let names: Vec<&str> = lines(text)
        .filter_map(request)
        .map(|(name, _)| name)
        .collect();
    if names.iter().any(|name| matches!(*name, "Dd" | "Dt" | "Sh")) {
        Ok(read_mdoc(text, program, redact))
    } else if names.iter().any(|name| matches!(*name, "TH" | "SH")) {
        Ok(read_man(text, program, redact))
    } else {
        Err("it is not a manual page in the `man` or the `mdoc` macros")
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::NoSecrets;

    use super::{crc32, read, text_of};
    use crate::metadata::{Applies, Takes};

    /// One option read: its flags, whether it takes a value, the value's
    /// name, and its description.
    type Row<'a> = (Vec<&'a str>, Takes, Option<&'a str>, Option<&'a str>);

    fn unread(read: &crate::metadata::FileRead) -> Vec<(&str, u64)> {
        read.unread.iter().map(|(w, n)| (*w, *n)).collect()
    }

    #[test]
    fn the_man_macros_give_each_options_flags_value_and_description() {
        let page = concat!(
            ".\\\" A page of the imaginary program, for the tests.\n",
            ".TH EXAMPLE-CLI-ITEM-LIST 1 2026-11-18 \"example-cli 2.4.1\"\n",
            ".SH NAME\n",
            "example-cli-item-list \\- list the items\n",
            ".SH DESCRIPTION\n",
            ".TP\n",
            "\\fB\\-\\-not\\-an\\-option\\-section\\fR\n",
            "Not read: this is not the options section.\n",
            ".SH OPTIONS\n",
            "Prose before the first entry is not an option.\n",
            ".TP\n",
            "\\fB\\-l\\fR, \\fB\\-\\-location\\fR \\fIcode\\fR\n",
            "Only the items at this location,\n",
            ".I such as\n",
            "A01\\(em one of three.\n",
            ".TP\n",
            ".B \\-\\-quiet\n",
            "Print nothing but the items.\n",
            ".TP\n",
            ".BR \\-\\-format = \\fIform\\fR\n",
            "How to print them.\n",
            ".IP \"\\fB\\-\\-limit\\fR \\fIN\\fR\" 4\n",
            "At most N items, a whole number.\n",
            ".TP\n",
            "\\fB\\-\\-colour\\fR[=\\fIwhen\\fR]\n",
            "An optional value is not read.\n",
            ".TP\n",
            "\\fB\\-\\-odd\\fR\n",
            "A \\*(Tm string is not guessed at.\n",
            ".TP\n",
            "\\fB\\-\\-nested\\fR\n",
            ".RS\n",
            "A nested block is not read.\n",
            ".RE\n",
            ".TP\n",
            "ENVIRONMENT_LIKE_TAG\n",
            "A tag that is no option is prose.\n",
            ".SH SEE ALSO\n",
            "example-cli(1)\n",
        );
        let read = read(page, "example-cli", &NoSecrets).unwrap();
        assert_eq!(read.form, "man");
        let got: Vec<Row<'_>> = read
            .stated
            .iter()
            .map(|s| {
                assert_eq!(s.applies, Applies::Exactly("item-list".to_owned()));
                (
                    s.option.flags.iter().map(String::as_str).collect(),
                    s.option.takes,
                    s.option.value.as_deref(),
                    s.option.about.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (
                    vec!["-l", "--location"],
                    Takes::Value,
                    Some("code"),
                    Some("Only the items at this location, such as A01\u{2014} one of three.")
                ),
                (
                    vec!["--quiet"],
                    Takes::Nothing,
                    None,
                    Some("Print nothing but the items.")
                ),
                (
                    vec!["--format"],
                    Takes::Value,
                    Some("form"),
                    Some("How to print them.")
                ),
                (
                    vec!["--limit"],
                    Takes::Value,
                    Some("N"),
                    Some("At most N items, a whole number.")
                ),
            ]
        );
        assert_eq!(
            unread(&read),
            [("escape", 1), ("optional_value", 1), ("request", 1)]
        );
    }

    #[test]
    fn the_mdoc_macros_give_each_options_flags_value_and_description() {
        let page = concat!(
            ".Dd November 18, 2026\n",
            ".Dt EXAMPLE-CLI 1\n",
            ".Os\n",
            ".Sh NAME\n",
            ".Nm example-cli\n",
            ".Nd the imaginary inventory program\n",
            ".Sh DESCRIPTION\n",
            "The\n",
            ".Nm\n",
            "program keeps an inventory.\n",
            ".Bl -tag -width Ds\n",
            ".It Fl h , Fl \\-home Ar dir\n",
            "The folder that holds the data, as\n",
            ".Pa items.json .\n",
            ".It Fl \\-version\n",
            "Print the version of\n",
            ".Nm .\n",
            ".It Fl \\-rows Ns = Ns Ar n\n",
            "How many rows; see\n",
            ".Xr example-cli 1 .\n",
            ".It Fl o Op Ar value\n",
            "An optional value is not read.\n",
            ".It Fl \\-lists\n",
            ".Bl -bullet\n",
            ".It\n",
            "A list inside an entry is not read.\n",
            ".El\n",
            ".It Fl \\-link\n",
            "See\n",
            ".Lk https://example.invalid/ the page .\n",
            ".It Cm word\n",
            "An item that is no option is prose.\n",
            ".El\n",
            ".Bl -column\n",
            ".It Fl x\n",
            ".El\n",
            ".Sh SEE ALSO\n",
        );
        let read = read(page, "example-cli", &NoSecrets).unwrap();
        assert_eq!(read.form, "mdoc");
        let got: Vec<Row<'_>> = read
            .stated
            .iter()
            .map(|s| {
                assert_eq!(s.applies, Applies::Program);
                (
                    s.option.flags.iter().map(String::as_str).collect(),
                    s.option.takes,
                    s.option.value.as_deref(),
                    s.option.about.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (
                    vec!["-h", "--home"],
                    Takes::Value,
                    Some("dir"),
                    Some("The folder that holds the data, as items.json.")
                ),
                (
                    vec!["--version"],
                    Takes::Nothing,
                    None,
                    Some("Print the version of example-cli.")
                ),
                (
                    vec!["--rows"],
                    Takes::Value,
                    Some("n"),
                    Some("How many rows; see example-cli(1).")
                ),
            ]
        );
        assert_eq!(
            unread(&read),
            [("list_form", 1), ("optional_value", 1), ("request", 2)]
        );
    }

    #[test]
    fn a_page_of_another_program_and_a_text_that_is_no_page() {
        let other = ".TH OTHER 1\n.SH OPTIONS\n.TP\n.B \\-\\-x\nAn option of another.\n";
        let read_other = read(other, "example-cli", &NoSecrets).unwrap();
        assert!(read_other.stated.is_empty());
        assert_eq!(unread(&read_other), [("another_command", 1)]);
        assert!(read("plain words\n", "example-cli", &NoSecrets).is_err());
    }

    #[test]
    fn a_gzip_page_is_inflated_and_any_other_compression_is_refused() {
        let page = b".TH EXAMPLE-CLI 1\n.SH OPTIONS\n.TP\n.B \\-\\-yes\nDo not ask.\n";
        let packed = miniz_oxide::deflate::compress_to_vec(page, 6);
        let mut gzip = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
        gzip.extend_from_slice(&packed);
        gzip.extend_from_slice(&crc32(page).to_le_bytes());
        gzip.extend_from_slice(&u32::try_from(page.len()).unwrap().to_le_bytes());
        assert_eq!(text_of(&gzip, 1 << 20).unwrap().as_bytes(), page);
        // A damaged trailer, a bound passed, and another compression.
        let mut damaged = gzip.clone();
        let last = damaged.len() - 5;
        damaged[last] ^= 1;
        assert!(text_of(&damaged, 1 << 20).is_err());
        assert!(text_of(&gzip, 8).is_err());
        assert!(text_of(b"BZh91AY&SY", 1 << 20).is_err());
        assert!(text_of(&[0xFF, 0xFE, 0x00], 1 << 20).is_err());
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
