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

use std::net::IpAddr;

/// Whether `url`'s host is a loopback address written as an IP address.
pub(crate) fn loopback(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    let bare = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host);
    // The parser writes an IP address in its one canonical form; a name
    // never parses as one.
    bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::loopback;

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
