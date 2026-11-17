//! Patterns: the one place the pack compiles a regular expression.
//!
//! The matcher takes time linear in the text, so output from a program
//! cannot make a match run away. A pattern that must cover a whole text is
//! wrapped, so that an alternative inside it cannot slip past its anchors.

use regex::{Regex, RegexBuilder};

/// The most memory a compiled pattern may take.
const SIZE_LIMIT: usize = 1 << 20;

fn build(text: &str, multi_line: bool) -> Result<Regex, String> {
    RegexBuilder::new(text)
        .size_limit(SIZE_LIMIT)
        .multi_line(multi_line)
        .build()
        .map_err(|error| error.to_string())
}

/// Compiles a pattern as written, to see that it is one.
///
/// # Errors
///
/// Why the pattern does not compile.
pub fn compile(text: &str) -> Result<Regex, String> {
    build(text, false)
}

/// Compiles a pattern that must match a whole text, from its first
/// character to its last.
///
/// # Errors
///
/// Why the pattern does not compile.
pub fn whole(text: &str) -> Result<Regex, String> {
    build(&format!(r"\A(?:{text})\z"), false)
}

/// Compiles a prompt's pattern: `^` is the start of any line.
///
/// # Errors
///
/// Why the pattern does not compile.
pub fn prompt(text: &str) -> Result<Regex, String> {
    build(text, true)
}

/// Whether a pattern ends with a `$` that is not escaped.
#[must_use]
pub fn ends_anchored(text: &str) -> bool {
    let Some(before) = text.strip_suffix('$') else {
        return false;
    };
    let slashes = before.chars().rev().take_while(|c| *c == '\\').count();
    slashes % 2 == 0
}

#[cfg(test)]
mod tests {
    use super::{ends_anchored, prompt, whole};

    #[test]
    fn a_whole_pattern_lets_no_alternative_past_its_anchors() {
        let loose = whole("^a$|b").unwrap();
        assert!(loose.is_match("b"));
        assert!(!loose.is_match("b; rm"));
        assert!(!whole(r"^\d+$").unwrap().is_match("12\n"));
    }

    #[test]
    fn an_escaped_dollar_is_not_an_anchor_and_a_prompt_starts_any_line() {
        assert!(ends_anchored(r"^a$"));
        assert!(!ends_anchored(r"^a\$"));
        assert!(ends_anchored(r"^a\\$"));
        assert!(prompt(r"^Are you sure\?").unwrap().is_match("ok\nAre you sure? [y/N] "));
    }
}
