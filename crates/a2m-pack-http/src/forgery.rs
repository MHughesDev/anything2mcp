//! Reading an anti-forgery token off the page that gives it (decision 0399).
//!
//! The page is data from outside. It is scanned, never run: tags are taken
//! apart one at a time, comments and the raw text of `script`, `style`,
//! `textarea`, and `title` are skipped so that a token written inside them
//! is not one, and the token is taken from exactly one kind of place the
//! endpoint names. A page that holds two different texts there is not
//! guessed at.

use crate::template::TokenSource;

/// The longest token that is read, in bytes.
pub(crate) const LONGEST_TOKEN: usize = 1_024;

/// What reading a page for its token found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Found {
    /// The one text the place holds.
    Token(String),
    /// Nothing is held at the place the endpoint names.
    Missing,
    /// Two different texts, or one that is not a token that can be sent.
    Unreadable,
}

/// Reads the token from a page's body and the cookies its answer set.
pub(crate) fn read(source: &TokenSource, body: &[u8], set_cookies: &[&str]) -> Found {
    let mut candidates: Vec<String> = Vec::new();
    match source {
        TokenSource::Cookie(name) => {
            for header in set_cookies {
                let first = header.split(';').next().unwrap_or("");
                if let Some((cookie, value)) = first.split_once('=')
                    && cookie.trim() == name
                {
                    candidates.push(value.trim().to_owned());
                }
            }
        }
        TokenSource::Meta(name) => {
            tags(body, |tag, attributes| {
                if tag == "meta"
                    && attribute(attributes, "name").is_some_and(|found| found == *name)
                    && let Some(content) = attribute(attributes, "content")
                {
                    candidates.push(content);
                }
            });
        }
        TokenSource::Input(name) => {
            tags(body, |tag, attributes| {
                if tag == "input"
                    && attribute(attributes, "name").is_some_and(|found| found == *name)
                    && attribute(attributes, "type")
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("hidden"))
                    && let Some(value) = attribute(attributes, "value")
                {
                    candidates.push(value);
                }
            });
        }
    }
    candidates.dedup();
    candidates.sort();
    candidates.dedup();
    match candidates.as_slice() {
        [] => Found::Missing,
        [one] if sendable(one) => Found::Token(one.clone()),
        _ => Found::Unreadable,
    }
}

/// Whether a text can be sent as a token in a header or a JSON field: some
/// visible characters with no space, in a bounded length.
fn sendable(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= LONGEST_TOKEN
        && text.bytes().all(|byte| (0x21..0x7f).contains(&byte))
}

/// The value of an attribute, by lowercase name.
fn attribute(attributes: &[(String, String)], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|(known, _)| known == name)
        .map(|(_, value)| value.clone())
}

/// Elements whose content is text, not markup.
const RAW: &[&str] = &["script", "style", "textarea", "title"];

/// Calls `each` with the lowercase name and the attributes of every start
/// tag of the page, outside comments and raw text.
fn tags(body: &[u8], mut each: impl FnMut(&str, &[(String, String)])) {
    let mut at = 0;
    while let Some(open) = find(body, at, b"<") {
        let rest = &body[open + 1..];
        if rest.starts_with(b"!--") {
            at = find(body, open + 4, b"-->").map_or(body.len(), |end| end + 3);
            continue;
        }
        let (name, mut cursor) = word(body, open + 1);
        if name.is_empty() || name.starts_with('/') {
            at = open + 1;
            continue;
        }
        let mut attributes = Vec::new();
        loop {
            cursor = skip_space(body, cursor);
            match body.get(cursor) {
                None => break,
                Some(b'>') => {
                    cursor += 1;
                    break;
                }
                Some(b'/') => cursor += 1,
                Some(_) => {
                    let (key, after) = word(body, cursor);
                    if key.is_empty() {
                        cursor += 1;
                        continue;
                    }
                    cursor = skip_space(body, after);
                    let mut value = String::new();
                    if body.get(cursor) == Some(&b'=') {
                        cursor = skip_space(body, cursor + 1);
                        match body.get(cursor) {
                            Some(&quote @ (b'"' | b'\'')) => {
                                // A quote never closed ends the scan: nothing after
                                // it can be told from the text of its value.
                                let Some(end) = find(body, cursor + 1, &[quote]) else {
                                    return;
                                };
                                value =
                                    String::from_utf8_lossy(&body[cursor + 1..end]).into_owned();
                                cursor = end + 1;
                            }
                            _ => {
                                let start = cursor;
                                while body
                                    .get(cursor)
                                    .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'>')
                                {
                                    cursor += 1;
                                }
                                value = String::from_utf8_lossy(&body[start..cursor]).into_owned();
                            }
                        }
                    }
                    attributes.push((key, value));
                }
            }
        }
        at = cursor;
        if RAW.contains(&name.as_str()) {
            let close = format!("</{name}");
            at = find_ci(body, at, close.as_bytes()).unwrap_or(body.len());
            continue;
        }
        each(&name, &attributes);
    }
}

/// A name of letters, digits, `-`, `_`, `:`, and a leading `/`, in
/// lowercase, and where it ends.
fn word(body: &[u8], from: usize) -> (String, usize) {
    let mut end = from;
    if body.get(end) == Some(&b'/') {
        end += 1;
    }
    while body
        .get(end)
        .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
    {
        end += 1;
    }
    (
        String::from_utf8_lossy(&body[from..end]).to_ascii_lowercase(),
        end,
    )
}

fn skip_space(body: &[u8], mut at: usize) -> usize {
    while body.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    at
}

fn find(body: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    body.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

fn find_ci(body: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    body.get(from..)?
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|position| position + from)
}

/// Whether `text` holds `needle` in any letter case.
pub(crate) fn mentions(text: &[u8], needle: &str) -> bool {
    !needle.is_empty()
        && text
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{Found, mentions, read};
    use crate::template::TokenSource;

    fn meta(name: &str) -> TokenSource {
        TokenSource::Meta(name.to_owned())
    }

    #[test]
    fn a_token_is_taken_from_the_one_place_the_endpoint_names() {
        let page = br#"<!doctype html><html><head><META NAME="csrf-token" content='tok-1'></head>
            <body><form><input type=hidden name=authenticity_token value=tok-1></form></body></html>"#;
        assert_eq!(
            read(&meta("csrf-token"), page, &[]),
            Found::Token("tok-1".into())
        );
        assert_eq!(
            read(&TokenSource::Input("authenticity_token".into()), page, &[]),
            Found::Token("tok-1".into())
        );
        assert_eq!(read(&meta("other"), page, &[]), Found::Missing);
        // A hidden input only: a visible one is not a token.
        let visible = br#"<input type="text" name="authenticity_token" value="x1">"#;
        assert_eq!(
            read(
                &TokenSource::Input("authenticity_token".into()),
                visible,
                &[]
            ),
            Found::Missing
        );
    }

    #[test]
    fn a_token_written_inside_a_comment_or_raw_text_is_not_one() {
        let page = br#"<!-- <meta name="csrf-token" content="in-comment"> -->
            <script>var x = '<meta name="csrf-token" content="in-script">';</script>
            <title><meta name="csrf-token" content="in-title"></title>
            <meta name="csrf-token" content="real">"#;
        assert_eq!(
            read(&meta("csrf-token"), page, &[]),
            Found::Token("real".into())
        );
        let only_hidden = br"<!-- <meta name=csrf-token content=x1> -->";
        assert_eq!(read(&meta("csrf-token"), only_hidden, &[]), Found::Missing);
    }

    #[test]
    fn two_different_texts_or_an_unsendable_one_are_not_guessed_at() {
        let two = br#"<meta name="t" content="a1"><meta name="t" content="b2">"#;
        assert_eq!(read(&meta("t"), two, &[]), Found::Unreadable);
        let same = br#"<meta name="t" content="a1"><meta name="t" content="a1">"#;
        assert_eq!(read(&meta("t"), same, &[]), Found::Token("a1".into()));
        let spaced = br#"<meta name="t" content="a b">"#;
        assert_eq!(read(&meta("t"), spaced, &[]), Found::Unreadable);
        let empty = br#"<meta name="t" content="">"#;
        assert_eq!(read(&meta("t"), empty, &[]), Found::Unreadable);
        let long = format!(r#"<meta name="t" content="{}">"#, "x".repeat(2_000));
        assert_eq!(read(&meta("t"), long.as_bytes(), &[]), Found::Unreadable);
        // An unterminated quote ends the scan, and finds nothing.
        assert_eq!(
            read(&meta("t"), b"<meta name=\"t\" content=\"abc", &[]),
            Found::Missing
        );
    }

    #[test]
    fn a_cookie_is_read_from_what_the_answer_set() {
        let cookies = ["other=1; Path=/", "xsrf=tok-9; Path=/; SameSite=Strict"];
        assert_eq!(
            read(&TokenSource::Cookie("xsrf".into()), b"", &cookies),
            Found::Token("tok-9".into())
        );
        assert_eq!(
            read(&TokenSource::Cookie("nope".into()), b"", &cookies),
            Found::Missing
        );
    }

    #[test]
    fn a_mention_is_found_in_any_letter_case() {
        assert!(mentions(b"the token was TOK-1.", "tok-1"));
        assert!(!mentions(b"nothing", "tok-1"));
        assert!(!mentions(b"anything", ""));
    }
}
