//! A reader of a PowerShell module manifest (`.psd1`), handed to the scout
//! as a file of the project and read as data (decision 0290).
//!
//! A manifest is written in PowerShell's data language. The reader never
//! runs it, never imports the module it describes, and evaluates nothing:
//! it takes a value only when it is literal text, a number, `$true`,
//! `$false`, `$null`, a list of those, or a table of them. A value written
//! any other way (a variable, a string that substitutes, a call, a script
//! block, a type) leaves its key unread, counted by reason. Of what it
//! reads it keeps the module's version and the commands the module exports
//! by name; a wildcard names no command and is counted, not kept.

use std::collections::BTreeMap;

/// Why a file is not read at all.
pub(crate) type NotManifest = &'static str;

/// The deepest tables and lists may nest.
const DEEPEST: usize = 16;
/// The most exported names kept from one manifest.
const MOST_NAMES: usize = 2_000;
/// The most tokens one manifest may hold.
const MOST_TOKENS: usize = 200_000;

/// What the reader made of one manifest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ManifestRead {
    /// The module's version, as `ModuleVersion` states it.
    pub version: Option<String>,
    /// The functions, cmdlets, and aliases it exports by name, in order.
    pub exported: Vec<String>,
    /// How many of its keys were read as data.
    pub read: u64,
    /// The keys, and the exported names, that were not, by reason.
    pub unread: BTreeMap<&'static str, u64>,
}

impl ManifestRead {
    fn not_read(&mut self, why: &'static str) {
        *self.unread.entry(why).or_default() += 1;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// `@{`
    Table,
    /// `@(`
    List,
    /// `}`
    CloseTable,
    /// `)`
    CloseList,
    Equals,
    Semicolon,
    Comma,
    Newline,
    /// A string, and whether it is literal: a single-quoted string, or a
    /// double-quoted one with nothing in it that substitutes or escapes.
    Text(String, bool),
    /// A bare word: a key, a number, or `$true`.
    Word(String),
    /// Anything else: an opening brace or parenthesis without `@`, a
    /// bracket, an operator. Never data.
    Other(char),
}

impl Token {
    fn opens(&self) -> bool {
        matches!(
            self,
            Self::Table | Self::List | Self::Other('{' | '(' | '[')
        )
    }

    fn closes(&self) -> bool {
        matches!(self, Self::CloseTable | Self::CloseList | Self::Other(']'))
    }
}

/// The quotes PowerShell takes besides the plain ones. Text holding one is
/// never taken as literal: PowerShell may end the string there.
fn odd_quote(c: char) -> bool {
    matches!(
        c,
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' | '\u{201c}' | '\u{201d}' | '\u{201e}'
    )
}

fn special(c: char) -> bool {
    c.is_whitespace() || "=;,(){}[]@'\"#".contains(c) || odd_quote(c)
}

fn tokens(text: &str) -> Result<Vec<Token>, NotManifest> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if out.len() > MOST_TOKENS {
            return Err("it holds more than the reader takes from one manifest");
        }
        let c = chars[at];
        let next = chars.get(at + 1).copied();
        match c {
            '\n' => {
                out.push(Token::Newline);
                at += 1;
            }
            c if c.is_whitespace() => at += 1,
            '#' => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
            }
            '<' if next == Some('#') => {
                let end = (at + 2..chars.len().saturating_sub(1))
                    .find(|&i| chars[i] == '#' && chars[i + 1] == '>')
                    .ok_or("a comment in it does not end")?;
                at = end + 2;
            }
            '@' if next == Some('{') => {
                out.push(Token::Table);
                at += 2;
            }
            '@' if next == Some('(') => {
                out.push(Token::List);
                at += 2;
            }
            '@' if matches!(next, Some('\'' | '"')) => {
                // A here-string: from the line after `@'` to a line that
                // starts with `'@`.
                let quote = next.unwrap_or('\'');
                let start = (at + 2..chars.len())
                    .find(|&i| chars[i] == '\n')
                    .ok_or("a here-string in it does not end")?
                    + 1;
                let end = (start..chars.len().saturating_sub(1))
                    .find(|&i| {
                        chars[i] == quote
                            && chars[i + 1] == '@'
                            && (i == start || chars[i - 1] == '\n')
                    })
                    .ok_or("a here-string in it does not end")?;
                let body: String = chars[start..end].iter().collect();
                let body = body.strip_suffix('\n').unwrap_or(&body);
                let body = body.strip_suffix('\r').unwrap_or(body).to_owned();
                let literal =
                    !body.contains(odd_quote) && (quote == '\'' || !body.contains(['$', '`']));
                out.push(Token::Text(body, literal));
                at = end + 2;
            }
            '\'' => {
                let mut body = String::new();
                at += 1;
                loop {
                    let c = *chars.get(at).ok_or("a string in it does not end")?;
                    if c == '\'' {
                        if chars.get(at + 1) == Some(&'\'') {
                            body.push('\'');
                            at += 2;
                            continue;
                        }
                        at += 1;
                        break;
                    }
                    body.push(c);
                    at += 1;
                }
                let literal = !body.contains(odd_quote);
                out.push(Token::Text(body, literal));
            }
            '"' => {
                let mut body = String::new();
                let mut literal = true;
                at += 1;
                loop {
                    let c = *chars.get(at).ok_or("a string in it does not end")?;
                    if c == '`' {
                        // An escape: the next character is taken as it is
                        // for the scan, and the string is not literal.
                        literal = false;
                        at += 2;
                        continue;
                    }
                    if c == '"' {
                        if chars.get(at + 1) == Some(&'"') {
                            body.push('"');
                            at += 2;
                            continue;
                        }
                        at += 1;
                        break;
                    }
                    if c == '$' || odd_quote(c) {
                        literal = false;
                    }
                    body.push(c);
                    at += 1;
                }
                out.push(Token::Text(body, literal));
            }
            '}' => {
                out.push(Token::CloseTable);
                at += 1;
            }
            ')' => {
                out.push(Token::CloseList);
                at += 1;
            }
            '=' => {
                out.push(Token::Equals);
                at += 1;
            }
            ';' => {
                out.push(Token::Semicolon);
                at += 1;
            }
            ',' => {
                out.push(Token::Comma);
                at += 1;
            }
            c if special(c) => {
                out.push(Token::Other(c));
                at += 1;
            }
            _ => {
                let start = at;
                while at < chars.len() && !special(chars[at]) {
                    at += 1;
                }
                out.push(Token::Word(chars[start..at].iter().collect()));
            }
        }
    }
    Ok(out)
}

/// A value read as data.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Data {
    Text(String),
    /// A list of values.
    List(Vec<Data>),
    /// A number, `$true`, `$false`, `$null`, or a table: data, and nothing
    /// the reader keeps.
    Other,
}

/// Why one key's value is not read.
type Skip = &'static str;

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn skip_lines(&mut self) {
        while matches!(self.peek(), Some(Token::Newline)) {
            self.at += 1;
        }
    }

    /// Passes over the rest of a value that is not data: to the end of its
    /// line or entry, keeping count of what opens and closes.
    fn pass_over(&mut self) -> Result<(), NotManifest> {
        let mut depth = 0_usize;
        while let Some(token) = self.peek() {
            if depth == 0 && matches!(token, Token::Newline | Token::Semicolon | Token::CloseTable)
            {
                return Ok(());
            }
            if token.opens() {
                depth += 1;
            } else if token.closes() {
                depth = depth.checked_sub(1).ok_or("its brackets do not match")?;
            }
            self.at += 1;
        }
        Ok(())
    }

    /// One value, which may be a list of values joined by commas.
    fn value(&mut self, depth: usize) -> Result<Result<Data, Skip>, NotManifest> {
        let first = match self.one(depth)? {
            Ok(data) => data,
            Err(why) => return Ok(Err(why)),
        };
        if !matches!(self.peek(), Some(Token::Comma)) {
            return Ok(Ok(first));
        }
        let mut items = vec![first];
        while matches!(self.peek(), Some(Token::Comma)) {
            self.at += 1;
            self.skip_lines();
            match self.one(depth)? {
                Ok(data) => items.push(data),
                Err(why) => return Ok(Err(why)),
            }
        }
        Ok(Ok(Data::List(items)))
    }

    fn one(&mut self, depth: usize) -> Result<Result<Data, Skip>, NotManifest> {
        if depth > DEEPEST {
            return Err("its tables and lists nest deeper than the reader takes");
        }
        let Some(token) = self.peek().cloned() else {
            return Err("it ends where a value is expected");
        };
        self.at += 1;
        Ok(match token {
            Token::Text(text, true) => Ok(Data::Text(text)),
            Token::Text(_, false) => Err("not_literal"),
            Token::Word(word) => {
                let lower = word.to_ascii_lowercase();
                let number = word.chars().next().is_some_and(|c| c.is_ascii_digit())
                    && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
                if number || matches!(lower.as_str(), "$true" | "$false" | "$null") {
                    Ok(Data::Other)
                } else {
                    Err("not_literal")
                }
            }
            Token::List => {
                let mut items = Vec::new();
                loop {
                    while matches!(self.peek(), Some(Token::Newline | Token::Comma)) {
                        self.at += 1;
                    }
                    if matches!(self.peek(), Some(Token::CloseList)) {
                        self.at += 1;
                        break;
                    }
                    match self.value(depth + 1)? {
                        Ok(Data::List(inner)) => items.extend(inner),
                        Ok(data) => items.push(data),
                        Err(why) => return Ok(Err(why)),
                    }
                }
                Ok(Data::List(items))
            }
            Token::Table => {
                let mut ignored = ManifestRead::default();
                self.entries(depth + 1, &mut ignored, false)?;
                if ignored.unread.is_empty() {
                    Ok(Data::Other)
                } else {
                    Err("not_literal")
                }
            }
            _ => Err("not_literal"),
        })
    }

    /// The entries of a table, up to its closing brace. At the top, what
    /// each key holds is kept.
    fn entries(
        &mut self,
        depth: usize,
        out: &mut ManifestRead,
        top: bool,
    ) -> Result<(), NotManifest> {
        let mut keys: Vec<String> = Vec::new();
        loop {
            while matches!(self.peek(), Some(Token::Newline | Token::Semicolon)) {
                self.at += 1;
            }
            let key = match self.peek().cloned() {
                Some(Token::CloseTable) => {
                    self.at += 1;
                    return Ok(());
                }
                Some(Token::Word(word))
                    if word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
                {
                    word
                }
                Some(Token::Text(text, true)) => text,
                None => return Err("a table in it does not end"),
                Some(_) => return Err("a key in it is not a name"),
            };
            self.at += 1;
            if self.peek() != Some(&Token::Equals) {
                return Err("a key in it has no `=`");
            }
            self.at += 1;
            self.skip_lines();
            let start = self.at;
            let value = self.value(depth)?;
            if keys.iter().any(|known| known.eq_ignore_ascii_case(&key)) {
                return Err("it names one key twice");
            }
            keys.push(key.clone());
            match value {
                Err(why) => {
                    out.not_read(why);
                    self.at = start;
                    self.pass_over()?;
                }
                Ok(data) => {
                    if !matches!(
                        self.peek(),
                        Some(Token::Newline | Token::Semicolon | Token::CloseTable) | None
                    ) {
                        out.not_read("not_literal");
                        self.at = start;
                        self.pass_over()?;
                        continue;
                    }
                    out.read += 1;
                    if top {
                        keep(&key, data, out);
                    }
                }
            }
        }
    }
}

/// Keeps what a key of the manifest says, when it is one the reader uses.
fn keep(key: &str, data: Data, out: &mut ManifestRead) {
    let lower = key.to_ascii_lowercase();
    if lower == "moduleversion" {
        if let Data::Text(text) = data
            && !text.is_empty()
            && text.len() <= 40
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
        {
            out.version = Some(text);
        }
        return;
    }
    if !matches!(
        lower.as_str(),
        "functionstoexport" | "cmdletstoexport" | "aliasestoexport"
    ) {
        return;
    }
    let names = match data {
        Data::Text(text) => vec![Data::Text(text)],
        Data::List(items) => items,
        Data::Other => Vec::new(),
    };
    for name in names {
        let Data::Text(name) = name else {
            out.not_read("not_plain_name");
            continue;
        };
        if name.contains(['*', '?', '[']) {
            out.not_read("wildcard");
        } else if !crate::maml::plain_command(&name) {
            out.not_read("not_plain_name");
        } else if out.exported.len() == MOST_NAMES {
            out.not_read("bound");
        } else if !out
            .exported
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&name))
        {
            out.exported.push(name);
        }
    }
}

/// Reads a module manifest. Its text was redacted whole before.
///
/// # Errors
///
/// Why the file is no manifest the reader takes: it is not one table, a
/// string or a comment in it does not end, its brackets do not match, or
/// it passes a bound.
pub(crate) fn read(text: &str) -> Result<ManifestRead, NotManifest> {
    let mut parser = Parser {
        tokens: tokens(text)?,
        at: 0,
    };
    parser.skip_lines();
    if parser.peek() != Some(&Token::Table) {
        return Err("it is no module manifest: it does not start with a table, `@{`");
    }
    parser.at += 1;
    let mut out = ManifestRead::default();
    parser.entries(1, &mut out, true)?;
    parser.skip_lines();
    if parser.peek().is_some() {
        return Err("it holds more than one table");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::read;

    #[test]
    fn a_manifest_is_read_as_data_and_what_is_not_data_is_counted() {
        let text = "\u{feff}<# The imaginary example module. #>\n@{\n  RootModule = 'Example.Inventory.psm1'\n  ModuleVersion = '1.4.0'\n  GUID = '00000000-0000-0000-0000-000000000000'\n  # The commands it offers.\n  FunctionsToExport = @(\n    'Get-ExampleItem',\n    'Set-ExampleItem'\n  )\n  CmdletsToExport = @()\n  AliasesToExport = 'gei', 'Get-Example*'\n  VariablesToExport = \"$env:EXAMPLE\"\n  ScriptsToProcess = (Join-Path $PSScriptRoot 'x.ps1')\n  PrivateData = @{ PSData = @{ Tags = @('example'); Notes = @'\nIt's data.\n'@ } }\n}\n";
        let read = read(text).unwrap();
        assert_eq!(read.version.as_deref(), Some("1.4.0"));
        assert_eq!(read.exported, ["Get-ExampleItem", "Set-ExampleItem", "gei"]);
        assert_eq!(read.read, 7);
        assert_eq!(read.unread.get("not_literal"), Some(&2));
        assert_eq!(read.unread.get("wildcard"), Some(&1));
        for refused in [
            "Get-ExampleItem",
            "@{ A = 'x' ",
            "@{ A = 'x }",
            "@{ A = 1; A = 2 }",
            "@{ A = 1 } @{ B = 2 }",
            "@{ = 1 }",
        ] {
            assert!(super::read(refused).is_err(), "{refused}");
        }
    }
}
