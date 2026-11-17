//! What a command and the local engine agree on beyond the REST interface:
//! two request headers that only the local channel carries. Both ends are
//! in this crate, so the names and the encoding are written once, here.
//!
//! Neither header is read from anything but the local channel, whose
//! other end is a process of the engine's own user.

use std::path::{Path, PathBuf};

/// The request header that carries the project folder a command was run
/// in, percent-encoded. The engine's handlers check the folder before any
/// use.
pub(crate) const PROJECT_HEADER: &str = "a2m-project";

/// The request header by which the command line says that a person is at
/// the terminal it was started at. Without it a caller is taken to be an
/// agent. It stops a mistake and is not a security boundary: a person's
/// act is proven by the presence prompt, never by this.
pub(crate) const PERSON_HEADER: &str = "a2m-person-at-terminal";

/// The one value [`PERSON_HEADER`] is read with.
pub(crate) const PERSON_PRESENT: &str = "true";

fn plain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
}

/// A folder's path as a header carries it. `None` for a path that is not
/// text: it could not be read back as the same path.
pub(crate) fn encode_path(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if plain(byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    Some(out)
}

/// The path a header carries. `None` when an escape is broken or the
/// bytes are not text.
pub(crate) fn decode_path(text: &str) -> Option<PathBuf> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{decode_path, encode_path};

    #[test]
    fn a_path_survives_the_header_and_a_broken_one_is_refused() {
        for text in [
            "/home/pat/work",
            "C:\\Users\\pat\\My Work",
            "/tmp/caf\u{e9} 100%/\u{4f8b}",
        ] {
            let sent = encode_path(Path::new(text)).unwrap();
            assert!(sent.bytes().all(|byte| byte.is_ascii_graphic()), "{sent}");
            assert_eq!(decode_path(&sent).unwrap(), Path::new(text), "{sent}");
        }
        for broken in ["%", "%4", "%zz", "%FF"] {
            assert!(decode_path(broken).is_none(), "{broken}");
        }
    }
}
