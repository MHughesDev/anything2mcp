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
/// The most open descriptors read, across all processes, to find the owners
/// of listening sockets. With [`MOST_PROCESSES`] it bounds the walk's time.
pub const MOST_DESCRIPTORS: usize = 1 << 20;

/// One list a bound cut: how many entries were kept, of how many seen.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Cut {
    /// The list's name, as it appears in the output.
    pub list: &'static str,
    /// The entries kept.
    pub kept: usize,
    /// The entries seen.
    pub seen: usize,
    /// Whether the system's table itself was read only up to
    /// [`MOST_TABLE_BYTES`], so that more entries than were seen may exist.
    pub table_cut: bool,
}

/// The cut of a table the system gave that was read only up to
/// [`MOST_TABLE_BYTES`]; `seen` entries were in the part read.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn table_cut(list: &'static str, seen: usize) -> Cut {
    Cut {
        list,
        kept: seen,
        seen,
        table_cut: true,
    }
}

/// Keeps the first `most` entries of an already ordered list, and records
/// the cut when there is one, in the entry for that list if there is one.
pub(crate) fn keep<T>(list: &mut Vec<T>, most: usize, name: &'static str, cuts: &mut Vec<Cut>) {
    if list.len() > most {
        let seen = list.len();
        list.truncate(most);
        match cuts.iter_mut().find(|c| c.list == name) {
            Some(cut) => {
                cut.kept = most;
                cut.seen = cut.seen.max(seen);
            }
            None => cuts.push(Cut {
                list: name,
                kept: most,
                seen,
                table_cut: false,
            }),
        }
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
                seen: 10,
                table_cut: false
            }]
        );
        let mut short = vec![1];
        keep(&mut short, 4, "routes", &mut cuts);
        assert_eq!(cuts.len(), 1);
        // A table the system gave in part, then cut again by the bound.
        let mut cuts = vec![table_cut("neighbours", 9)];
        let mut list: Vec<u32> = (0..9).collect();
        keep(&mut list, 3, "neighbours", &mut cuts);
        assert_eq!(cuts.len(), 1);
        assert_eq!(
            (cuts[0].kept, cuts[0].seen, cuts[0].table_cut),
            (3, 9, true)
        );
    }

    #[test]
    fn a_long_text_is_cut_on_a_character_boundary() {
        let long = "é".repeat(LONGEST_TEXT);
        let cut = text(&long);
        assert!(cut.len() <= LONGEST_TEXT);
        assert!(cut.chars().all(|c| c == 'é'));
    }
}
