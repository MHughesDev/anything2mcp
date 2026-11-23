//! The origins a tab may be on, and what the origin of an address is.
//!
//! An origin is a scheme, a host, and a port, written as the browser writes
//! it: `https://app.example.com`, or `http://127.0.0.1:8080` where the port
//! is not the scheme's own. A tab is opened for a set of them, and refuses
//! to go to, look at, or act on a page of any other (decision 0143).
//!
//! The engine has no parser of addresses beyond this module, on purpose:
//! an address is compared with what the browser itself reports, as text. A
//! host written in a form the browser would rewrite (an address in
//! hexadecimal, a name outside ASCII) therefore matches nothing, and the
//! tab refuses: a difference between two readings of one address can only
//! close the tab's reach, never widen it.

use std::collections::BTreeSet;

use a2m_core::ScopedHost;
use a2m_provider::{MachineState, NetworkPlace, Placed};

use crate::browsers::Address;

/// What stands for a page that has no origin of its own, such as a page
/// given as a `data:` address: its scheme, with the colon.
const DATA_PAGES: &str = "data:";

/// The origin of an `http` or `https` address, or of a `blob:` address
/// made by a page of one, as the browser writes it: the host in lower
/// case, and the port only when it is not the scheme's own. `None` for
/// anything else.
pub(crate) fn origin_of(address: &str) -> Option<String> {
    let address = address.strip_prefix("blob:").unwrap_or(address);
    let (scheme, rest) = address.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let own_port: u16 = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    // A name and a password before the host are not part of an origin.
    let host_and_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, after)| after);
    // The last colon is the port's, unless it is inside the brackets of
    // an IPv6 address.
    let (host, port) = match host_and_port.rfind(':') {
        Some(at) if !host_and_port[at..].contains(']') => {
            (&host_and_port[..at], Some(&host_and_port[at + 1..]))
        }
        _ => (host_and_port, None),
    };
    if host.is_empty() {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let port = match port {
        None | Some("") => None,
        Some(port) => Some(port.parse::<u16>().ok()?),
    };
    Some(match port {
        Some(port) if port != own_port => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    })
}

/// What a tab compares a page or a request by: its origin, or, for a page
/// with no origin of its own, its scheme.
fn key_of(address: &str) -> Option<String> {
    if address.starts_with(DATA_PAGES) {
        return Some(DATA_PAGES.to_owned());
    }
    origin_of(address)
}

/// The path of an `http` or `https` address: what follows the host, up to
/// the query. `/` when there is none.
pub(crate) fn path_of(address: &str) -> &str {
    let rest = address.split_once("://").map_or("", |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    match rest.find('/') {
        Some(at) => &rest[at..],
        None => "/",
    }
}

/// The origins a tab may be on. A tab is given them when it is opened,
/// from the destinations its plan or its discovery declared, and they do
/// not change while it lives. An empty set lets the tab go nowhere.
///
/// Beside them, the origins its page may send requests to although the tab
/// is never on them (`and_requests_to`, decision 0292): a request of the
/// page, its frames, or its workers to an origin in neither set is refused
/// in the browser.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Origins {
    allowed: BTreeSet<String>,
    requests_only: BTreeSet<String>,
}

impl Origins {
    /// The origins of these addresses.
    #[must_use]
    pub fn of<'a>(addresses: impl IntoIterator<Item = &'a Address>) -> Self {
        Self {
            allowed: addresses.into_iter().map(Address::origin).collect(),
            requests_only: BTreeSet::new(),
        }
    }

    /// These origins and those of `more`, both sets.
    #[must_use]
    pub fn and(mut self, more: &Self) -> Self {
        self.allowed.extend(more.allowed.iter().cloned());
        self.requests_only
            .extend(more.requests_only.iter().cloned());
        self
    }

    /// The same, and the origins of these addresses as places the tab's
    /// page may send requests to: an image, a script, a style, a font, a
    /// frame, a `fetch`. The tab is never on them: `goto`, the tab's own
    /// document, and every look and act are held to the tab's own origins
    /// alone (decision 0292).
    #[must_use]
    pub fn and_requests_to<'a>(mut self, addresses: impl IntoIterator<Item = &'a Address>) -> Self {
        self.requests_only
            .extend(addresses.into_iter().map(Address::origin));
        self
    }

    /// Whether the address is on one of the origins.
    #[must_use]
    pub fn allows(&self, address: &Address) -> bool {
        self.allowed.contains(&address.origin())
    }

    /// The network place of each origin a request may go to (the tab's own
    /// and those its page may send requests to), in order, each with the
    /// reason. A place is derived from the origin's host as written and from
    /// what the machine says of itself, never taken from an input, and a name
    /// is never resolved: an origin on a name is `unknown`
    /// (decisions 0223 and 0143). The guard itself still compares origins,
    /// which tell apart what one place does not: two origins can be on one
    /// place, and a tab refuses the other (package N2b).
    #[must_use]
    pub fn places(&self, state: &MachineState) -> Vec<(&str, Placed)> {
        self.reach()
            .into_iter()
            .map(|origin| {
                (
                    origin,
                    NetworkPlace::of(&ScopedHost::of_destination(origin), state),
                )
            })
            .collect()
    }

    /// The origins, in order, each as the browser writes it.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.allowed.iter().map(String::as_str)
    }

    /// The origins the tab's page may send requests to and the tab is
    /// never on, in order.
    pub fn requests_only(&self) -> impl Iterator<Item = &str> {
        self.requests_only.iter().map(String::as_str)
    }

    /// Whether an address the browser reported is on one of the origins.
    pub(crate) fn allows_reported(&self, address: &str) -> bool {
        key_of(address).is_some_and(|key| self.allowed.contains(&key))
    }

    /// Whether a request the browser reported may go to its address: on
    /// one of the tab's origins, or on one its page may send requests to.
    pub(crate) fn reaches_reported(&self, address: &str) -> bool {
        key_of(address)
            .is_some_and(|key| self.allowed.contains(&key) || self.requests_only.contains(&key))
    }

    /// Every origin a request may go to, each as the browser writes it.
    pub(crate) fn reach(&self) -> Vec<&str> {
        self.allowed
            .union(&self.requests_only)
            .map(String::as_str)
            .collect()
    }

    /// The same origins, and the pages a test gives as `data:` addresses.
    /// For this crate's tests only: the product has no way to let a tab
    /// onto a page that came from no declared origin.
    #[cfg(test)]
    pub(crate) fn with_data_pages(mut self) -> Self {
        self.allowed.insert(DATA_PAGES.to_owned());
        self
    }
}

impl Address {
    /// The address's origin, as the browser writes it.
    #[must_use]
    pub fn origin(&self) -> String {
        // An address has been checked to be `http` or `https` with a host;
        // one this module still cannot read has an origin nothing matches.
        origin_of(self.as_str()).unwrap_or_default()
    }
}

/// An address the browser reported, as far as it may be shown: its origin,
/// or its scheme when it has no origin.
pub(crate) fn shown_origin(address: &str) -> String {
    origin_of(address).unwrap_or_else(|| {
        address
            .split_once(':')
            .map_or_else(String::new, |(scheme, _)| format!("{scheme}:"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_origin_is_written_as_the_browser_writes_it() {
        for (address, origin) in [
            (
                "https://app.example.com/orders?x=1#top",
                "https://app.example.com",
            ),
            ("https://APP.Example.com:443/", "https://app.example.com"),
            ("http://app.example.com:80", "http://app.example.com"),
            ("http://127.0.0.1:8080/a", "http://127.0.0.1:8080"),
            (
                "https://app.example.com:8443",
                "https://app.example.com:8443",
            ),
            ("http://[::1]:8080/a", "http://[::1]:8080"),
            ("http://[::1]/a", "http://[::1]"),
            (
                "https://someone:word@app.example.com/",
                "https://app.example.com",
            ),
            (
                "blob:https://app.example.com/1b2c",
                "https://app.example.com",
            ),
        ] {
            assert_eq!(origin_of(address).as_deref(), Some(origin), "{address}");
        }
        for none in [
            "about:blank",
            "data:text/html,x",
            "chrome-error://chromewebdata/",
            "https://",
            "https://app.example.com:port/",
            "https://app.example.com:99999/",
            "ws://app.example.com/",
            "app.example.com",
        ] {
            assert_eq!(origin_of(none), None, "{none}");
        }
        assert_eq!(path_of("https://app.example.com"), "/");
        assert_eq!(path_of("https://app.example.com/a/b?c=/d#e"), "/a/b");
        assert_eq!(shown_origin("data:text/html,secret"), "data:");
        assert_eq!(
            shown_origin("https://app.example.com/x?token=1"),
            "https://app.example.com"
        );
    }

    #[test]
    fn a_tab_s_origins_hold_exactly_the_origins_of_the_addresses_given() {
        let address = |text: &str| Address::new(text).unwrap();
        let origins = Origins::of([
            &address("https://app.example.com/orders"),
            &address("https://api.example.com:8443/v1"),
        ]);
        assert_eq!(
            origins.iter().collect::<Vec<_>>(),
            ["https://api.example.com:8443", "https://app.example.com"]
        );
        assert!(origins.allows(&address("https://APP.example.com/anything?at=all")));
        assert!(origins.allows_reported("https://api.example.com:8443/v2"));
        for outside in [
            "http://app.example.com/orders",
            "https://app.example.com:8443/",
            "https://app.example.com.other.example/",
            "https://other.example/https://app.example.com/",
            "https://api.example.com/v1",
        ] {
            assert!(!origins.allows(&address(outside)), "{outside}");
            assert!(!origins.allows_reported(outside), "{outside}");
        }
        // A page with no origin is on none of them, and nothing is by default.
        assert!(!origins.allows_reported("data:text/html,x"));
        assert!(!origins.allows_reported("about:blank"));
        assert!(!Origins::default().allows(&address("https://app.example.com/")));
        assert!(
            origins
                .clone()
                .with_data_pages()
                .allows_reported("data:text/html,x")
        );
        // A place the page may send requests to is not a place the tab is.
        let wider = origins.and_requests_to([&address("https://assets.example.com/fonts")]);
        assert!(wider.reaches_reported("https://assets.example.com/a.woff2"));
        assert!(!wider.allows_reported("https://assets.example.com/a.woff2"));
        assert!(!wider.allows(&address("https://assets.example.com/")));
        assert!(wider.reaches_reported("https://app.example.com/orders"));
        assert!(!wider.reaches_reported("https://other.example/x"));
        assert_eq!(
            wider.iter().collect::<Vec<_>>(),
            ["https://api.example.com:8443", "https://app.example.com"]
        );
        assert_eq!(wider.reach().len(), 3);
    }
}
