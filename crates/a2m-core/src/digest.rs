//! SHA-256 digests, written as 64 lowercase hexadecimal characters.

use std::fmt;
use std::str::FromStr;

use sha2::{Digest as _, Sha256};

/// A SHA-256 digest.
///
/// It names a thing by its content: a file, a program, a compiled target, a
/// finding. It is read and written as 64 lowercase hexadecimal characters and
/// nothing else, so the same content has the same name on every machine.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; Digest::LENGTH]);

impl Digest {
    /// The number of bytes in a digest.
    pub const LENGTH: usize = 32;

    /// The digest of `bytes`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = FieldHasher::new();
        hasher.raw(bytes);
        hasher.finish()
    }

    /// A digest from its bytes, as a store holds it.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    /// The digest's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

/// Text that is not a digest.
///
/// It maps to the failure class `input`, code `schema_invalid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidDigest {
    length: usize,
}

impl fmt::Display for InvalidDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a digest is 64 lowercase hexadecimal characters; this text has {} characters or is not hexadecimal",
            self.length
        )
    }
}

impl std::error::Error for InvalidDigest {}

impl FromStr for Digest {
    type Err = InvalidDigest;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let wrong = || InvalidDigest {
            length: text.chars().count(),
        };
        let bytes = text.as_bytes();
        if bytes.len() != Self::LENGTH * 2 {
            return Err(wrong());
        }
        let mut out = [0u8; Self::LENGTH];
        let (pairs, _) = bytes.as_chunks::<2>();
        for (place, [high, low]) in out.iter_mut().zip(pairs) {
            let high = nibble(*high).ok_or_else(wrong)?;
            let low = nibble(*low).ok_or_else(wrong)?;
            *place = (high << 4) | low;
        }
        Ok(Self(out))
    }
}

/// The value of one lowercase hexadecimal character.
const fn nibble(character: u8) -> Option<u8> {
    match character {
        b'0'..=b'9' => Some(character - b'0'),
        b'a'..=b'f' => Some(character - b'a' + 10),
        _ => None,
    }
}

impl serde::Serialize for Digest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Digest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Builds a digest over several named fields, so that no two different sets
/// of fields can give the same bytes to hash. Every field is written as its
/// name's length, its name, its value's length, and its value; each length
/// is eight bytes, least significant first.
///
/// This is the one way a kernel crate hashes more than one field. A crate
/// that names what its digest is for adds that as the first field.
pub struct FieldHasher(Sha256);

impl FieldHasher {
    /// A hasher that has been given nothing.
    #[must_use]
    pub fn new() -> Self {
        Self(Sha256::new())
    }

    /// Adds bytes exactly as they are.
    pub(crate) fn raw(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// Adds one named field.
    pub fn field(&mut self, name: &str, value: &[u8]) {
        self.0.update((name.len() as u64).to_le_bytes());
        self.0.update(name.as_bytes());
        self.0.update((value.len() as u64).to_le_bytes());
        self.0.update(value);
    }

    /// The digest of everything added, in the order it was added.
    #[must_use]
    pub fn finish(self) -> Digest {
        let mut out = [0u8; Digest::LENGTH];
        out.copy_from_slice(&self.0.finalize());
        Digest(out)
    }
}

impl Default for FieldHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FieldHasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FieldHasher")
    }
}

#[cfg(test)]
mod tests {
    use super::{Digest, FieldHasher};

    /// The digest of the empty input, from the SHA-256 standard.
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    /// The digest of the three bytes `abc`, from the SHA-256 standard.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// The digest of two named fields, worked out apart from this crate.
    const PINNED_FIELDS: &str = "65092bca65a0160c209fceeb989606a66b868fcba9216c9d067013c1e8562172";

    #[test]
    fn it_gives_the_standard_answers() {
        assert_eq!(Digest::of(b"").to_string(), EMPTY);
        assert_eq!(Digest::of(b"abc").to_string(), ABC);
    }

    #[test]
    fn it_reads_back_what_it_writes() {
        let digest = Digest::of(b"abc");
        assert_eq!(ABC.parse::<Digest>().unwrap(), digest);
        assert_eq!(
            serde_json::to_string(&digest).unwrap(),
            format!("\"{ABC}\"")
        );
        assert_eq!(
            serde_json::from_str::<Digest>(&format!("\"{ABC}\"")).unwrap(),
            digest
        );
        assert_eq!(format!("{digest:?}"), format!("Digest({ABC})"));
    }

    #[test]
    fn text_that_is_not_a_digest_is_refused() {
        let upper = ABC.to_uppercase();
        let short = &ABC[..63];
        let long = format!("{ABC}0");
        let not_hex = format!("{}g", &ABC[..63]);
        for bad in ["", short, long.as_str(), upper.as_str(), not_hex.as_str()] {
            assert!(bad.parse::<Digest>().is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<Digest>("\"not a digest\"").is_err());
    }

    #[test]
    fn named_fields_cannot_be_confused_with_each_other() {
        let digest = |fields: &[(&str, &str)]| {
            let mut hasher = FieldHasher::new();
            for (name, value) in fields {
                hasher.field(name, value.as_bytes());
            }
            hasher.finish()
        };
        assert_ne!(digest(&[("a", "bc")]), digest(&[("ab", "c")]));
        assert_ne!(
            digest(&[("a", "b"), ("c", "d")]),
            digest(&[("a", "bc"), ("", "d")])
        );
        assert_eq!(digest(&[("a", "b")]), digest(&[("a", "b")]));
        // The encoding is fixed: stored digests were made with it.
        assert_eq!(
            digest(&[("anything2mcp", "example"), ("a", "b")]).to_string(),
            PINNED_FIELDS
        );
    }
}
