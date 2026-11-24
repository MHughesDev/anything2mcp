//! An input of the built-in field type `url` accepts only the destinations
//! its operation declares (spec section 502: no input may change where a
//! request goes). The engine holds every such input to the operation's
//! `permissions.destinations` before anything is prepared or sent, and
//! refuses any other address as `policy`, `surface_not_declared`, naming
//! the input and never its value.
//!
//! An address is read strictly, and anything this reading does not take is
//! refused, never guessed at:
//!
//! - It is `scheme://host[:port]` and then nothing, or a path, a query, or a
//!   fragment, each starting with `/`, `?`, or `#`. A scheme-relative
//!   address (`//host`), and one with no `://`, is refused.
//! - It holds only printable ASCII, with no space and no `\`.
//! - Its host part holds no `@` (a user part) and no `%` (an encoded host).
//!   The host is a name of letters, digits, dots, and hyphens, or an
//!   address literal between brackets; anything else is refused.
//! - It matches a declared destination that names its scheme, when the
//!   scheme is the destination's, the host is the destination's, compared
//!   without regard to case and with nothing else made equal (an address
//!   literal matches only the same literal, as written; a number that some
//!   programs read as an address is a name here, and matches only itself),
//!   and the port is the destination's as written: both name none, or both
//!   name the same number. No port is taken to be a scheme's usual one, so
//!   an address that names the usual port of a destination that names none
//!   is refused. A destination that names no scheme is matched by no input:
//!   which schemes it would allow is not written down, and the compiler
//!   refuses an operation whose `url` input has no destination with one.
//!
//! Where the request goes after that, a redirect included, is held by the
//! destinations the driver is prepared with, which are the surfaces and the
//! operation's declared destinations and never an input, and by each pack's
//! own refusal of a redirect that leaves them.

use a2m_core::{CompiledInput, CompiledTarget, Failure, Value, Values};

use crate::codes;

/// The name of the built-in field type of an address.
pub(crate) const URL_TYPE: &str = "url";

/// An address, read: its scheme, its host, and the port it names, if any.
#[derive(Debug, PartialEq, Eq)]
struct Address {
    scheme: String,
    host: String,
    port: Option<u16>,
}

/// Reads `host[:port]`, or `[literal][:port]`.
fn host_and_port(authority: &str) -> Option<(String, Option<u16>)> {
    if authority.is_empty() || authority.contains(['@', '%']) {
        return None;
    }
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let (literal, after) = inner.split_once(']')?;
        if literal.is_empty()
            || !literal
                .chars()
                .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.'))
        {
            return None;
        }
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':')?),
        };
        (format!("[{}]", literal.to_ascii_lowercase()), port)
    } else {
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        if host.is_empty()
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        {
            return None;
        }
        (host.to_ascii_lowercase(), port)
    };
    let port = match port {
        None => None,
        Some(digits) => {
            if digits.is_empty() || digits.len() > 5 || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            Some(digits.parse::<u16>().ok()?)
        }
    };
    Some((host, port))
}

fn scheme_of(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then(|| text.to_ascii_lowercase())
}

/// Reads `scheme://` and then the host and port in `rest`, which ends
/// where `host_and_port` is to stop reading.
fn read_address(text: &str, whole: bool) -> Option<Address> {
    let (scheme, rest) = text.split_once("://")?;
    let scheme = scheme_of(scheme)?;
    let end = if whole {
        rest.len()
    } else {
        rest.find(['/', '?', '#']).unwrap_or(rest.len())
    };
    let (host, port) = host_and_port(&rest[..end])?;
    Some(Address { scheme, host, port })
}

/// An address an input gives.
fn read_input(text: &str) -> Option<Address> {
    if text.is_empty()
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'\\')
    {
        return None;
    }
    read_address(text, false)
}

/// A declared destination that names its scheme; nothing follows the port
/// (the compiler holds a destination to that form). One with no scheme is
/// not read, and so matches nothing.
fn read_destination(text: &str) -> Option<Address> {
    read_address(text, true)
}

/// Whether the address `text` is one of `destinations`.
pub(crate) fn is_declared(destinations: &[String], text: &str) -> bool {
    let Some(given) = read_input(text) else {
        return false;
    };
    destinations
        .iter()
        .filter_map(|destination| read_destination(destination))
        .any(|destination| destination == given)
}

/// Holds every input of the type `url` among `declared` to `destinations`:
/// each text it holds is one of them. `whose` names the tool or the step,
/// for the message.
///
/// # Errors
///
/// `surface_not_declared`, naming each input that gives another address.
pub(crate) fn guard_url_inputs(
    compiled_target: &CompiledTarget,
    declared: &[CompiledInput],
    inputs: &Values,
    destinations: &[String],
    whose: &str,
) -> Result<(), Failure> {
    let mut refused = Vec::new();
    for input in declared {
        let is_url = compiled_target
            .field_types
            .get(input.field_type.get())
            .is_some_and(|field_type| field_type.name == URL_TYPE);
        if !is_url {
            continue;
        }
        let Some(value) = inputs.get(&input.name) else {
            continue;
        };
        let texts: Vec<&Value> = match value {
            Value::List(items) => items.iter().collect(),
            Value::Null => Vec::new(),
            other => vec![other],
        };
        let fits = texts.iter().all(|value| match value {
            Value::Text(text) => is_declared(destinations, text),
            _ => false,
        });
        if !fits {
            refused.push(format!("`{}`", input.name));
        }
    }
    if refused.is_empty() {
        return Ok(());
    }
    Err(codes::fail(
        &codes::row::SURFACE_NOT_DECLARED,
        format!(
            "The input {} of {whose} gives an address that is not one of the destinations its operation declares; nothing was sent.",
            refused.join(", ")
        ),
        "Give an address the operation declares in `permissions.destinations`, or declare the address there and publish again.",
    ))
}

#[cfg(test)]
mod tests {
    use super::is_declared;

    fn destinations() -> Vec<String> {
        vec![
            "https://files.example.com".to_owned(),
            "https://upload.example.com:8443".to_owned(),
            "http://[fd00::1]:8080".to_owned(),
            "https://192.0.2.10".to_owned(),
            // Names no scheme: no input matches it.
            "plain.example.com".to_owned(),
        ]
    }

    #[test]
    fn a_declared_destination_is_accepted() {
        let declared_ones = destinations();
        for address in [
            "https://files.example.com",
            "https://files.example.com/",
            "https://files.example.com/reports/1.pdf?part=2#top",
            "https://FILES.Example.com/a",
            "HTTPS://files.example.com/a",
            "https://upload.example.com:8443/in",
            "http://[fd00::1]:8080/",
            "https://192.0.2.10/x",
            // A redirect the address asks for is not followed here: where
            // the request goes after it is the driver's, prepared with the
            // declared destinations alone.
            "https://files.example.com/go?to=https://elsewhere.example.net/",
        ] {
            assert!(is_declared(&declared_ones, address), "{address}");
        }
    }

    #[test]
    fn every_other_address_is_refused() {
        let declared_ones = destinations();
        for address in [
            // Another host, and hosts that only look alike.
            "https://elsewhere.example.net/",
            "https://files.example.com.example.net/",
            "https://evilfiles.example.com/",
            "https://files.example.com./",
            "https://sub.files.example.com/",
            // Another scheme, or none.
            "ftp://files.example.com/",
            "file:///etc/passwd",
            "file://files.example.com/etc/passwd",
            "ws://files.example.com/",
            "http://upload.example.com:8443/in",
            "//files.example.com/a",
            "files.example.com/a",
            "javascript:alert(1)",
            "data:text/plain,files.example.com",
            // A destination that names no scheme.
            "https://plain.example.com/",
            "http://plain.example.com/",
            // Another scheme of a declared host.
            "http://files.example.com/a",
            // Another port, the usual one of the scheme included.
            "https://files.example.com:443/a",
            "https://files.example.com:8443/",
            "https://files.example.com:80/",
            "https://upload.example.com/in",
            "https://files.example.com:/",
            "https://files.example.com:+443/",
            "https://files.example.com:65536/",
            // A user part, and the forms that hide another host.
            "https://user@files.example.com/",
            "https://user:secret@files.example.com/",
            "https://files.example.com@elsewhere.example.net/",
            "https://files.example.com:443@elsewhere.example.net/",
            "https://files.example.com\\@elsewhere.example.net/",
            "https://elsewhere.example.net\\files.example.com",
            "https://elsewhere.example.net#@files.example.com",
            "https://elsewhere.example.net?@files.example.com",
            // An encoded host, and characters that are not ASCII.
            "https://files%2eexample.com/",
            "https://%66iles.example.com/",
            "https://files.example.com%40elsewhere.example.net/",
            "https://files.ex\u{0430}mple.com/",
            "https://files.example.com\u{3002}/",
            " https://files.example.com/",
            "https://files.example.com/ a",
            "https://files.example.com/\n",
            // IP literals and the numbers some programs read as addresses.
            "http://127.0.0.1/",
            "http://localhost/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://2130706433/",
            "http://0x7f000001/",
            "http://0177.0.0.1/",
            "http://[fd00::2]:8080/",
            "http://[fd00::1]/",
            "https://192.0.2.11/",
            "https://192.0.2.010/",
            "http://169.254.169.254/latest/meta-data/",
            // Nothing.
            "",
            "https://",
            "https:///a",
        ] {
            assert!(!is_declared(&declared_ones, address), "{address:?}");
        }
        // Nothing is declared: nothing is accepted.
        assert!(!is_declared(&[], "https://files.example.com/"));
    }
}
