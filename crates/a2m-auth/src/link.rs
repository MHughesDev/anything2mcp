//! Whether an address leads to this machine without leaving it.
//!
//! A loopback address is an IP address written in the address itself, in
//! `127.0.0.0/8` or `::1` (decision 0188). A name is never one, `localhost`
//! and `localhost.` included: the machine's resolver decides where a name
//! leads, and a hosts file or a resolver can send it anywhere. An IPv4
//! address written inside IPv6 (`::ffff:127.0.0.1`) and the unspecified
//! address (`0.0.0.0`) are not one either. The address is judged as the
//! HTTP client parses it, so `127.1` is `127.0.0.1` here as it is when it is
//! sent: nothing judges one spelling and sends another.
//!
//! The judgment is `a2m_core::ScopedHost::may_leave_this_machine`, the one
//! question every check of a plain link asks (decision 0240).

use a2m_core::ScopedHost;

/// Whether `url`'s host is a loopback address written as an IP address.
pub(crate) fn loopback(url: &reqwest::Url) -> bool {
    // The parser writes an IP address in its one canonical form; a name
    // never parses as one.
    !ScopedHost::of_host(url.host_str().unwrap_or_default()).may_leave_this_machine()
}

#[cfg(test)]
mod tests {
    use super::loopback;

    /// The corpus of address spellings every check of where an address is
    /// is pinned against (decision 0240).
    const SPELLINGS: &str = include_str!("../../a2m-core/tests/address-spellings.txt");

    /// Each spelling of the corpus, with the verdict of `column` now and
    /// before decision 0240.
    fn corpus(column: &str) -> Vec<(String, String, String)> {
        let mut header: Vec<&str> = Vec::new();
        let mut rows = Vec::new();
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
            let at = header.iter().position(|name| *name == column).unwrap();
            let spelling = cells[0].replace("<empty>", "").replace("<sp>", " ");
            let (now, before) = cells[at].split_once('<').unwrap_or((cells[at], cells[at]));
            rows.push((spelling, now.to_owned(), before.to_owned()));
        }
        assert!(rows.len() > 50, "the corpus was not read");
        rows
    }

    /// The verdict of `judge` for a spelling, given as `http://<spelling>/`.
    fn verdict(spelling: &str, judge: impl Fn(&reqwest::Url) -> bool) -> &'static str {
        match reqwest::Url::parse(&format!("http://{spelling}/")) {
            Ok(url) if judge(&url) => "here",
            Ok(_) => "away",
            Err(_) => "-",
        }
    }

    /// `loopback` as it was before decision 0240, kept to show that
    /// nothing it refused is allowed now.
    fn before(url: &reqwest::Url) -> bool {
        let host = url.host_str().unwrap_or_default();
        let bare = host
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
            .unwrap_or(host);
        bare.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    }

    #[test]
    fn every_spelling_of_the_shared_corpus_is_judged_as_it_is_pinned() {
        for (spelling, now, before_then) in corpus("auth") {
            assert_eq!(verdict(&spelling, loopback), now, "{spelling}");
            assert_eq!(verdict(&spelling, before), before_then, "{spelling}");
        }
    }

    #[test]
    fn no_spelling_refused_before_the_one_question_is_allowed_after_it() {
        for (spelling, _, _) in corpus("auth") {
            if verdict(&spelling, before) != "here" {
                assert_ne!(verdict(&spelling, loopback), "here", "{spelling}");
            }
        }
    }

    #[test]
    fn only_an_ip_address_in_the_loopback_range_is_loopback_and_never_a_name() {
        for (address, expected) in [
            ("http://127.0.0.1:8080/token", true),
            ("http://127.9.0.1/token", true),
            // Parsed as the client parses it: the same address.
            ("http://127.1/token", true),
            ("http://[::1]:9000/token", true),
            // Names: the resolver decides, so none is loopback, whatever it
            // resolves to today.
            ("http://localhost/token", false),
            ("http://localhost./token", false),
            ("http://localhost:8080/token", false),
            ("http://loopback.example.test/token", false),
            ("http://127.0.0.1.example.test/token", false),
            // Other addresses.
            ("http://[::ffff:127.0.0.1]/token", false),
            ("http://0.0.0.0/token", false),
            ("http://[::]/token", false),
            ("http://192.0.2.10/token", false),
        ] {
            let url = reqwest::Url::parse(address).unwrap();
            assert_eq!(loopback(&url), expected, "{address}");
        }
    }
}
