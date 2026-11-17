//! Small text encodings the providers need: Base64, Base32, percent-encoding,
//! and lowercase hexadecimal. Each is a standard encoding, written here so
//! that no secret passes through a crate that might keep a copy.

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64_with(bytes: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextet = |shift: u32| char::from(alphabet[((n >> shift) & 63) as usize]);
        out.push(sextet(18));
        out.push(sextet(12));
        if chunk.len() > 1 {
            out.push(sextet(6));
        } else if pad {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(sextet(0));
        } else if pad {
            out.push('=');
        }
    }
    out
}

/// Base64 with padding (RFC 4648, section 4).
pub(crate) fn base64(bytes: &[u8]) -> String {
    base64_with(bytes, BASE64, true)
}

/// Base64 for URLs, without padding (RFC 4648, section 5).
pub(crate) fn base64_url(bytes: &[u8]) -> String {
    base64_with(bytes, BASE64_URL, false)
}

/// Decodes Base32 (RFC 4648, section 6). Spaces, hyphens, padding, and case
/// are ignored, as one-time-code secrets are written in many ways. `None`
/// when a character is not in the alphabet or nothing is left.
pub(crate) fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u64 = 0;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a',
            b'2'..=b'7' => byte - b'2' + 26,
            b' ' | b'-' | b'=' => continue,
            _ => return None,
        };
        buffer = (buffer << 5) | u64::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Percent-encodes one component of a query or a form body (RFC 3986:
/// everything but the unreserved characters).
pub(crate) fn percent(text: &str, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
}

/// Name and value pairs as a query string or a form body.
pub(crate) fn form(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (name, value) in pairs {
        if !out.is_empty() {
            out.push('&');
        }
        percent(name, &mut out);
        out.push('=');
        percent(value, &mut out);
    }
    out
}

/// Bytes as lowercase hexadecimal.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 15)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base32_decode, base64, base64_url, form, hex};

    #[test]
    fn base64_matches_the_standards_examples() {
        // RFC 4648, section 10.
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
        assert_eq!(base64_url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn base32_decodes_the_standards_examples_however_they_are_written() {
        assert_eq!(base32_decode("MZXW6YTBOI======").unwrap(), b"foobar");
        assert_eq!(base32_decode("mzxw 6ytb-oi").unwrap(), b"foobar");
        assert!(base32_decode("MZXW1").is_none());
        assert!(base32_decode("").is_none());
    }

    #[test]
    fn a_form_escapes_everything_but_unreserved_characters() {
        assert_eq!(
            form(&[("scope", "read write"), ("a&b", "c=d/é")]),
            "scope=read%20write&a%26b=c%3Dd%2F%C3%A9"
        );
        assert_eq!(hex(&[0, 0xab, 0xff]), "00abff");
    }
}
