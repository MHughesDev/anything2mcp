//! Free text on its way into evidence: markup and hidden characters removed,
//! and the length limited (spec section 217, "Keeping secrets out", rule 4).
//!
//! Text that came from a system may be shown to an agent later, when a trace
//! is read or a repair bundle is built. Characters a person cannot see, and
//! tags a viewer might interpret, are how such text carries instructions
//! nobody meant to pass on, so they are taken out before the text is kept.

/// The most characters of one piece of free text a trace keeps.
pub const FREE_TEXT_LIMIT: usize = 512;

/// The most characters of a short label a trace keeps: a field name, a
/// locator, a detail's name.
pub const LABEL_LIMIT: usize = 128;

/// What is appended to text that was cut.
const CUT_MARK: char = '…';

/// Returns `text` without markup and without hidden characters.
///
/// A tag is `<` followed by a letter, `/`, `!`, or `?`, up to the next `>`;
/// a `<` followed by anything else is ordinary text and is kept. A tag that
/// never closes is removed to the end of the text. Hidden characters are
/// control characters other than tab and line feed, zero-width and
/// direction-changing characters, the soft hyphen, the byte-order mark, and
/// the tag characters above U+E0000.
#[must_use]
pub fn clean_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '<'
            && characters
                .peek()
                .is_some_and(|next| next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?'))
        {
            for inside in characters.by_ref() {
                if inside == '>' {
                    break;
                }
            }
            continue;
        }
        if !is_hidden(character) {
            out.push(character);
        }
    }
    out
}

/// Returns at most `limit` characters of `text`, with a mark when it was cut.
#[must_use]
pub fn limit_text(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        None => text.to_owned(),
        Some((end, _)) => {
            let mut cut = text[..end].to_owned();
            cut.push(CUT_MARK);
            cut
        }
    }
}

/// Whether a person reading the text would not see this character.
const fn is_hidden(character: char) -> bool {
    matches!(
        character,
        '\u{0000}'..='\u{0008}'
            | '\u{000B}'..='\u{001F}'
            | '\u{007F}'..='\u{009F}'
            | '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

#[cfg(test)]
mod tests {
    use super::{clean_text, limit_text};

    #[test]
    fn markup_is_removed_and_ordinary_angle_brackets_are_kept() {
        assert_eq!(clean_text("<b>Total</b> due"), "Total due");
        assert_eq!(clean_text("a < b and c > d"), "a < b and c > d");
        assert_eq!(clean_text("3 <4"), "3 <4");
        assert_eq!(clean_text("note<!-- hidden -->d"), "noted");
        assert_eq!(clean_text("cut <script never closes"), "cut ");
    }

    #[test]
    fn hidden_characters_are_removed() {
        assert_eq!(clean_text("pa\u{200B}id\u{202E}"), "paid");
        assert_eq!(
            clean_text("\u{FEFF}line one\nline\ttwo\u{0007}"),
            "line one\nline\ttwo"
        );
        assert_eq!(clean_text("tag\u{E0041}ged"), "tagged");
    }

    #[test]
    fn text_is_cut_on_a_character_boundary_and_marked() {
        assert_eq!(limit_text("naïve", 10), "naïve");
        assert_eq!(limit_text("naïve", 3), "naï…");
        assert_eq!(limit_text("", 0), "");
    }
}
