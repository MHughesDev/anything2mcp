//! What of an address, a query, or a JSON answer is kept as structure,
//! and what is a value that is never kept (decision 0195).
//!
//! A page's own words name the parts of an address and the fields of an
//! answer; a record's key, a token, or an address of mail can stand in the
//! same places. What cannot be told apart by its form is not kept as text:
//! a segment of a path becomes the placeholder [`PLACEHOLDER`], a query's
//! entry is counted and not named, and an object's names are withheld
//! whole.

/// What a segment of a path that is not a plain word is kept as.
pub const PLACEHOLDER: &str = "{value}";

/// The longest segment of a path kept as text, in characters.
pub const LONGEST_SEGMENT: usize = 24;
/// The longest part of a segment between its separators kept as text.
pub const LONGEST_SEGMENT_PART: usize = 16;
/// The longest name of a query's entry, or of a field of an answer, kept.
pub const LONGEST_NAME: usize = 48;
/// The longest part of such a name between its separators.
pub const LONGEST_NAME_PART: usize = 32;
/// The most names one object of an answer may have and still be read as
/// structure. An object with more is read as a dictionary, whose names are
/// values.
pub const MOST_NAMES: usize = 64;

/// Whether `text` has the form of a word, within the bounds given: at
/// least one character; every character an ASCII letter, a digit, `-`,
/// `.`, `_`, or `~`; not dots only; and every run between those four
/// separators at most `longest_part` characters and letters only, digits
/// only, or letters followed by digits (`v2`, `A100`). `A-100`, `orders`,
/// and `total_length_in` are words; `3f9a1c`, `x7Kq9`, `2fa`, and anything
/// longer than `longest` are not.
fn is_word(text: &str, longest: usize, longest_part: usize) -> bool {
    let separator = |c: char| matches!(c, '-' | '.' | '_' | '~');
    if text.is_empty()
        || text.chars().count() > longest
        || text.chars().all(|c| c == '.')
        || !text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || separator(c))
    {
        return false;
    }
    text.split(separator).all(|part| {
        if part.len() > longest_part {
            return false;
        }
        let letters = part.chars().take_while(char::is_ascii_alphabetic).count();
        part[letters..].chars().all(|c| c.is_ascii_digit())
    })
}

/// Whether a segment of a path is a plain word, kept as text: at most
/// [`LONGEST_SEGMENT`] characters, parts of at most
/// [`LONGEST_SEGMENT_PART`], in the form of a word.
#[must_use]
pub fn is_plain_segment_word(segment: &str) -> bool {
    is_word(segment, LONGEST_SEGMENT, LONGEST_SEGMENT_PART)
}

/// Whether the name of a query's entry, or of a field of a JSON answer, is
/// a plain name, kept as structure: at most [`LONGEST_NAME`] characters,
/// parts of at most [`LONGEST_NAME_PART`], in the form of a word.
#[must_use]
pub fn is_plain_name(name: &str) -> bool {
    is_word(name, LONGEST_NAME, LONGEST_NAME_PART)
}

/// A path, by structure: every segment that is not a plain word is
/// [`PLACEHOLDER`]. An empty segment, as the path `/` has, is kept.
#[must_use]
pub fn path_by_structure(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.is_empty() || is_plain_segment_word(segment) {
                segment
            } else {
                PLACEHOLDER
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The names of a query's entries, in order, each once, and whether an
/// entry was left out: one with no `=`, whose whole text is a value, or one
/// whose name is not a plain name.
#[must_use]
pub fn query_names(query: &str) -> (Vec<String>, bool) {
    let mut names: Vec<String> = Vec::new();
    let mut left_out = false;
    for entry in query.split('&').filter(|entry| !entry.is_empty()) {
        match entry.split_once('=') {
            Some((name, _)) if is_plain_name(name) => {
                if !names.iter().any(|known| known == name) {
                    names.push(name.to_owned());
                }
            }
            _ => left_out = true,
        }
    }
    names.sort();
    (names, left_out)
}

/// Whether the names of one object of a JSON answer are structure: it has
/// at most [`MOST_NAMES`] of them, and every one is a plain name. Otherwise
/// the object is read as a dictionary, whose names are values, and none of
/// them is kept.
#[must_use]
pub fn names_are_structure<'a>(mut names: impl ExactSizeIterator<Item = &'a String>) -> bool {
    names.len() <= MOST_NAMES && names.all(|name| is_plain_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_that_is_not_a_plain_word_is_kept_as_a_placeholder() {
        for kept in [
            "orders",
            "A-100",
            "L-101",
            "v2",
            "total_length_in",
            "api",
            "x86",
        ] {
            assert!(is_plain_segment_word(kept), "{kept}");
        }
        for value in [
            "3f9a1c0b",
            "x7Kq9",
            "2fa",
            "eyJhbGciOiJIUzI1NiJ9",
            "abcdefghijklmnopqrstuvwxyz",
            "A%20100",
            "a@b",
            "..",
            "",
            "{order_id}",
        ] {
            assert!(!is_plain_segment_word(value), "{value}");
        }
        assert_eq!(
            path_by_structure("/reset/9b2f0c4e7a1d/check"),
            "/reset/{value}/check"
        );
        assert_eq!(path_by_structure("/orders/A-100"), "/orders/A-100");
        assert_eq!(path_by_structure("/"), "/");
    }

    #[test]
    fn a_bare_query_entry_is_never_kept_and_is_counted() {
        assert_eq!(
            query_names("view=summary&lang=en&view=x"),
            (vec!["lang".to_owned(), "view".to_owned()], false)
        );
        assert_eq!(
            query_names("example-token-for-tests-1a2b3c"),
            (Vec::new(), true)
        );
        assert_eq!(
            query_names("page=2&q7Zx9kP2=1"),
            (vec!["page".to_owned()], true)
        );
    }

    #[test]
    fn the_names_of_a_dictionary_are_not_structure() {
        let plain = ["order_id".to_owned(), "lines".to_owned()];
        assert!(names_are_structure(plain.iter()));
        let keyed = ["someone@example.com".to_owned(), "lines".to_owned()];
        assert!(!names_are_structure(keyed.iter()));
        let many: Vec<String> = (0..=MOST_NAMES).map(|at| format!("n{at}")).collect();
        assert!(!names_are_structure(many.iter()));
    }
}
