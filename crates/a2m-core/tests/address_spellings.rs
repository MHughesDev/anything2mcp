//! The shared corpus of address spellings (decision 0240): what
//! `ScopedHost::of_destination` reads each spelling as. The same corpus pins
//! every check of a plain link, in the crates that make one.

use a2m_core::ScopedHost;

const SPELLINGS: &str = include_str!("address-spellings.txt");

#[test]
fn every_spelling_of_the_corpus_is_read_as_its_scope_column_says() {
    let mut header: Vec<&str> = Vec::new();
    let mut read = 0;
    for line in SPELLINGS.lines() {
        if let Some(names) = line.strip_prefix("#spelling") {
            header = std::iter::once("spelling")
                .chain(names.split_whitespace())
                .collect();
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(cells.len(), header.len(), "{line}");
        let at = header.iter().position(|name| *name == "scope").unwrap();
        let spelling = cells[0].replace("<empty>", "").replace("<sp>", " ");
        let host = ScopedHost::of_destination(&spelling);
        let mut seen = host.scope().to_string();
        if host.written_as_ipv4_in_ipv6() {
            seen.push_str("+mapped");
        }
        if host.written_with_zone() {
            seen.push_str("+zone");
        }
        assert_eq!(seen, cells[at], "{spelling}");
        read += 1;
    }
    assert!(read > 50, "the corpus was not read");
}
