//! Keys and plain bytes in memory, and where random bytes come from.
//!
//! Everything here that holds a key or a plain item wipes its memory when it
//! is dropped, cannot be copied, cannot be serialized, and prints as nothing
//! but its kind and, for plain bytes, its length.

use std::fmt;

use a2m_core::Failure;
use zeroize::Zeroizing;

use crate::failure;
use crate::hex;

/// A source of random bytes for keys and nonces.
///
/// The engine uses [`OsRandom`]. A test may give a fixed source, so that what
/// it seals is the same on every run.
pub trait RandomSource: Send + Sync {
    /// Fills `out` with random bytes.
    ///
    /// # Errors
    ///
    /// `busy`/`randomness_unavailable` when no random bytes can be had. A
    /// source must never fill `out` with anything predictable instead.
    fn fill(&self, out: &mut [u8]) -> Result<(), Failure>;
}

/// The operating system's random source.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsRandom;

impl RandomSource for OsRandom {
    fn fill(&self, out: &mut [u8]) -> Result<(), Failure> {
        getrandom::fill(out).map_err(|_| failure::no_randomness())
    }
}

/// One key of the hierarchy, in the clear, in memory that is wiped on drop.
///
/// It is not `Clone`, not `Serialize`, and its `Debug` shows no byte of it.
/// The bytes are reached only through [`KeyMaterial::expose`], which a key
/// store back end needs in order to wrap the key.
pub struct KeyMaterial(Zeroizing<Vec<u8>>);

impl KeyMaterial {
    /// The number of bytes in a key.
    pub const LENGTH: usize = 32;

    /// A new key from `random`.
    ///
    /// # Errors
    ///
    /// `busy`/`randomness_unavailable` when the source fails.
    pub fn generate(random: &dyn RandomSource) -> Result<Self, Failure> {
        let mut bytes = Zeroizing::new(vec![0u8; Self::LENGTH]);
        random.fill(&mut bytes)?;
        Ok(Self(bytes))
    }

    /// A key from bytes a key store unwrapped. The caller wipes its own copy.
    ///
    /// # Errors
    ///
    /// `input`/`sealed_item_mismatch` when `bytes` is not exactly
    /// [`KeyMaterial::LENGTH`] long.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, Failure> {
        if bytes.len() != Self::LENGTH {
            return Err(failure::mismatch());
        }
        Ok(Self(Zeroizing::new(bytes.to_vec())))
    }

    /// The key's bytes, for a cipher or for a key store that wraps the key.
    ///
    /// Never pass the result to a formatter, a logger, or a file.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for KeyMaterial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeyMaterial([redacted])")
    }
}

/// A key wrapped by the key above it. Ciphertext: safe to store and to copy.
///
/// Its `Debug` shows only its length. It serializes as hexadecimal text in a
/// readable format and as bytes otherwise.
#[derive(Clone, PartialEq, Eq)]
pub struct Wrapped(Vec<u8>);

impl Wrapped {
    /// Wrapped bytes read back from where they were kept.
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The wrapped bytes, for keeping.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Wrapped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Wrapped({} bytes)", self.0.len())
    }
}

impl serde::Serialize for Wrapped {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_bytes(&self.0, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Wrapped {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_bytes(deserializer).map(Self)
    }
}

/// Writes ciphertext as hexadecimal text in a readable format, as bytes
/// otherwise.
pub(crate) fn serialize_bytes<S: serde::Serializer>(
    bytes: &[u8],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if serializer.is_human_readable() {
        serializer.serialize_str(&hex::encode(bytes))
    } else {
        serializer.serialize_bytes(bytes)
    }
}

/// Reads what [`serialize_bytes`] wrote.
pub(crate) fn deserialize_bytes<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<u8>, D::Error> {
    if deserializer.is_human_readable() {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        hex::decode(&text).ok_or_else(|| serde::de::Error::custom("not hexadecimal text"))
    } else {
        <Vec<u8> as serde::Deserialize>::deserialize(deserializer)
    }
}

/// Plain bytes that came out of a sealed item, wiped when dropped.
///
/// Not `Clone`, not `Serialize`, and its `Debug` shows only its length. The
/// caller keeps the bytes out of logs.
pub struct Opened(Zeroizing<Vec<u8>>);

impl Opened {
    pub(crate) fn new(bytes: Zeroizing<Vec<u8>>) -> Self {
        Self(bytes)
    }

    /// The plain bytes.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// How many plain bytes there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the item was empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The bytes as text, moved and not copied.
    pub(crate) fn into_text(mut self) -> Option<String> {
        let bytes = std::mem::take(&mut *self.0);
        match String::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(error) => {
                drop(Zeroizing::new(error.into_bytes()));
                None
            }
        }
    }
}

impl fmt::Debug for Opened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Opened({} bytes)", self.0.len())
    }
}
