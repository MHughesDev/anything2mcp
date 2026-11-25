//! A column's heading as a person reads it (decision 0998, point 2).
//!
//! A table library that lets a person sort by a column often adds to the
//! column's heading a sentence for a screen reader, such as `Name: Activate
//! to sort`, which the browser then gives as part of the heading's name and
//! of its text. That sentence is the library's, not the heading's. The rule
//! here is closed: a heading with none of the sort words is its own words;
//! one whose words after its first colon hold a sort word and whose words
//! before it hold none is the words before it, with a doubled run of them
//! said once (the shown words and the sentence that repeats them, as
//! `Name Name: Activate to sort`); any other heading that holds a sort word
//! has no words of its own left, and is never guessed at.
//!
//! The drafter and the driver read a heading through this one function, so
//! a screen's `header` holds what both read.

/// The words that mark a sentence about sorting a column.
const SORT_WORDS: &[&str] = &["sort", "sorted", "sorting", "sortable"];

/// What is left of a heading once a table library's sort words are taken
/// off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Heading {
    /// The heading's own words; `stripped` when a sort sentence was taken
    /// off.
    Words { words: String, stripped: bool },
    /// The heading holds a sort word and no words of its own are left.
    NoWords,
}

fn has_sort_word(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|word| SORT_WORDS.contains(&word.to_lowercase().as_str()))
}

/// The heading as a person reads it, from its name or its text.
pub(crate) fn heading(text: &str) -> Heading {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !has_sort_word(&text) {
        return Heading::Words {
            words: text,
            stripped: false,
        };
    }
    let Some((before, after)) = text.split_once(':') else {
        return Heading::NoWords;
    };
    if !has_sort_word(after) || has_sort_word(before) {
        return Heading::NoWords;
    }
    let words: Vec<&str> = before.split_whitespace().collect();
    let half = words.len() / 2;
    let words = if words.len().is_multiple_of(2) && half > 0 && words[..half] == words[half..] {
        &words[..half]
    } else {
        &words[..]
    };
    if words.is_empty() {
        return Heading::NoWords;
    }
    Heading::Words {
        words: words.join(" "),
        stripped: true,
    }
}

/// The heading's words, or `None` when none are left.
pub(crate) fn words(text: &str) -> Option<String> {
    match heading(text) {
        Heading::Words { words, .. } => Some(words),
        Heading::NoWords => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Heading, heading, words};

    #[test]
    fn a_table_librarys_sort_sentence_is_taken_off_and_nothing_else_is() {
        assert_eq!(words("Name").as_deref(), Some("Name"));
        assert_eq!(words("Order date").as_deref(), Some("Order date"));
        assert_eq!(
            words("Name Name: Activate to invert sorting").as_deref(),
            Some("Name")
        );
        assert_eq!(
            words("In stock: activate to sort column ascending").as_deref(),
            Some("In stock")
        );
        assert_eq!(
            words("Part number Part number: Activate to sort").as_deref(),
            Some("Part number")
        );
        // A colon with no sort word after it is the heading's own.
        assert_eq!(words("Time: UTC").as_deref(), Some("Time: UTC"));
        assert_eq!(
            heading("Name: Activate to sort"),
            Heading::Words {
                words: "Name".to_owned(),
                stripped: true
            }
        );
    }

    #[test]
    fn a_heading_with_no_words_left_once_the_sort_words_are_off_has_none() {
        for text in [
            "Activate to sort column ascending",
            ": Activate to sort",
            "Sorted: ascending",
            "Sort by: Activate to sort",
        ] {
            assert_eq!(heading(text), Heading::NoWords, "{text}");
        }
    }
}
