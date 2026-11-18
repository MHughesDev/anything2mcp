//! Every list a reader returns is bounded, and a reading says what a bound
//! cut.

use serde::Serialize;

/// The most interfaces kept.
pub const MOST_INTERFACES: usize = 256;
/// The most addresses kept on one interface.
pub const MOST_ADDRESSES: usize = 64;
/// The most routes kept.
pub const MOST_ROUTES: usize = 4096;
/// The most neighbour entries kept.
pub const MOST_NEIGHBOURS: usize = 4096;
/// The most listening sockets kept.
pub const MOST_LISTENERS: usize = 4096;
/// The most resolver settings kept, and the most name servers and search
/// domains in each.
pub const MOST_RESOLVER_ENTRIES: usize = 32;
/// The most proxy settings kept.
pub const MOST_PROXY_SETTINGS: usize = 32;
/// The longest text kept from one value the system gives, in bytes.
pub const LONGEST_TEXT: usize = 1024;
/// The most bytes read from one table the system gives, as a file or a
/// dump. A table larger than this is read up to it, and the reading says so.
pub const MOST_TABLE_BYTES: usize = 16 * 1024 * 1024;
/// The most processes walked to find the owners of listening sockets.
pub const MOST_PROCESSES: usize = 65_536;

/// One list a bound cut: how many entries were kept, of how many seen.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Cut {
    /// The list's name, as it appears in the output.
    pub list: &'static str,
    /// The entries kept.
    pub kept: usize,
    /// The entries seen. Where the table itself was cut, more may exist.
    pub seen: usize,
}

/// Keeps the first `most` entries of an already ordered list, and records
/// the cut when there is one.
pub(crate) fn keep<T>(list: &mut Vec<T>, most: usize, name: &'static str, cuts: &mut Vec<Cut>) {
    if list.len() > most {
        cuts.push(Cut {
            list: name,
            kept: most,
            seen: list.len(),
        });
        list.truncate(most);
    }
}

/// Cuts a text the system gave to [`LONGEST_TEXT`] bytes, on a character
/// boundary.
pub(crate) fn text(value: &str) -> String {
    if value.len() <= LONGEST_TEXT {
        return value.to_owned();
    }
    let mut end = LONGEST_TEXT;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_list_is_cut_and_the_cut_is_named() {
        let mut list: Vec<u32> = (0..10).collect();
        let mut cuts = Vec::new();
        keep(&mut list, 4, "routes", &mut cuts);
        assert_eq!(list, vec![0, 1, 2, 3]);
        assert_eq!(
            cuts,
            vec![Cut {
                list: "routes",
                kept: 4,
                seen: 10
            }]
        );
        let mut short = vec![1];
        keep(&mut short, 4, "routes", &mut cuts);
        assert_eq!(cuts.len(), 1);
    }

    #[test]
    fn a_long_text_is_cut_on_a_character_boundary() {
        let long = "é".repeat(LONGEST_TEXT);
        let cut = text(&long);
        assert!(cut.len() <= LONGEST_TEXT);
        assert!(cut.chars().all(|c| c == 'é'));
    }
}
