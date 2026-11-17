//! The envelope: what `seal` returns and the caller keeps.

use std::fmt;

use a2m_core::{Digest, Failure};

use crate::cipher::{BoundTo, NONCE_LENGTH, TAG_LENGTH};
use crate::failure;
use crate::material::{KeyMaterial, Wrapped, deserialize_bytes, serialize_bytes};
use crate::names::{Generation, KeyId, KeyRef, SealContext};

/// The four bytes every envelope starts with.
const MAGIC: &[u8; 4] = b"A2MS";
/// The envelope format this engine writes.
const FORMAT: u8 = 1;
/// The cipher this engine writes: XChaCha20-Poly1305.
const CIPHER: u8 = 1;
/// The length of a data key wrapped by an owner's key.
const DATA_KEY_LENGTH: usize = NONCE_LENGTH + KeyMaterial::LENGTH + TAG_LENGTH;
/// Everything before the ciphertext.
const HEADER_LENGTH: usize =
    MAGIC.len() + 2 + Digest::LENGTH + 4 + DATA_KEY_LENGTH + NONCE_LENGTH;

/// A sealed item: the format, the cipher, the owner's key and its generation,
/// the wrapped data key, the nonce, and the ciphertext with its tag.
///
/// It holds nothing in the clear, so it is safe to store, to copy, and to
/// back up. It needs nothing beside it but the keys above. It is kept as
/// bytes with [`Sealed::to_bytes`]; it also serializes, as hexadecimal text
/// in a readable format and as bytes otherwise.
#[derive(Clone, PartialEq, Eq)]
pub struct Sealed {
    owner_key: KeyRef,
    data_key: Wrapped,
    nonce: [u8; NONCE_LENGTH],
    ciphertext: Vec<u8>,
}

impl Sealed {
    pub(crate) fn new(
        owner_key: KeyRef,
        data_key: Wrapped,
        nonce: [u8; NONCE_LENGTH],
        ciphertext: Vec<u8>,
    ) -> Self {
        Self {
            owner_key,
            data_key,
            nonce,
            ciphertext,
        }
    }

    /// The owner's key that wrapped this item's data key, and its generation.
    /// A rotation uses it to find the items a generation still protects.
    #[must_use]
    pub fn owner_key(&self) -> KeyRef {
        self.owner_key
    }

    pub(crate) fn data_key(&self) -> &Wrapped {
        &self.data_key
    }

    pub(crate) fn nonce(&self) -> &[u8; NONCE_LENGTH] {
        &self.nonce
    }

    pub(crate) fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    /// How many bytes [`Sealed::to_bytes`] gives.
    #[must_use]
    pub fn len(&self) -> usize {
        HEADER_LENGTH + self.ciphertext.len()
    }

    /// Never true: an envelope always has its header and its tag.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The envelope as bytes, for keeping.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.len());
        bytes.extend_from_slice(MAGIC);
        bytes.push(FORMAT);
        bytes.push(CIPHER);
        bytes.extend_from_slice(self.owner_key.id.as_bytes());
        bytes.extend_from_slice(&self.owner_key.generation.get().to_le_bytes());
        bytes.extend_from_slice(self.data_key.as_bytes());
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&self.ciphertext);
        bytes
    }

    /// Reads what [`Sealed::to_bytes`] wrote. Nothing is opened yet.
    ///
    /// # Errors
    ///
    /// `input`/`sealed_format_unsupported` for a format or cipher this
    /// engine does not know; `input`/`sealed_item_mismatch` for bytes that
    /// are not an envelope.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Failure> {
        let rest = bytes
            .strip_prefix(MAGIC.as_slice())
            .ok_or_else(failure::mismatch)?;
        let ([format, cipher], rest) = rest
            .split_first_chunk::<2>()
            .ok_or_else(failure::mismatch)?;
        if *format != FORMAT || *cipher != CIPHER {
            return Err(failure::unsupported());
        }
        let (id, rest) = rest
            .split_first_chunk::<{ Digest::LENGTH }>()
            .ok_or_else(failure::mismatch)?;
        let (generation, rest) = rest
            .split_first_chunk::<4>()
            .ok_or_else(failure::mismatch)?;
        let (data_key, rest) = rest
            .split_first_chunk::<DATA_KEY_LENGTH>()
            .ok_or_else(failure::mismatch)?;
        let (nonce, ciphertext) = rest
            .split_first_chunk::<NONCE_LENGTH>()
            .ok_or_else(failure::mismatch)?;
        if ciphertext.len() < TAG_LENGTH {
            return Err(failure::mismatch());
        }
        Ok(Self {
            owner_key: KeyRef {
                id: KeyId::from_bytes(*id),
                generation: Generation::new(u32::from_le_bytes(*generation)),
            },
            data_key: Wrapped::from_bytes(data_key.to_vec()),
            nonce: *nonce,
            ciphertext: ciphertext.to_vec(),
        })
    }
}

impl fmt::Debug for Sealed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sealed")
            .field("format", &FORMAT)
            .field("owner_key", &self.owner_key)
            .field("bytes", &self.len())
            .finish()
    }
}

impl serde::Serialize for Sealed {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_bytes(&self.to_bytes(), serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Sealed {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = deserialize_bytes(deserializer)?;
        Self::from_bytes(&bytes).map_err(|_| serde::de::Error::custom("not a sealed item"))
    }
}

/// What is authenticated with an item's ciphertext and with its wrapped data
/// key: the format, the cipher, the whole context, and the owner's key. The
/// two uses have different purposes, so neither can stand in for the other.
pub(crate) fn bound_to(purpose: &str, cx: &SealContext, owner_key: KeyRef) -> BoundTo {
    cx.bind(
        BoundTo::new(purpose)
            .number("format", u32::from(FORMAT))
            .number("cipher", u32::from(CIPHER)),
    )
    .field("owner_key", owner_key.id.as_bytes())
    .number("owner_key_generation", owner_key.generation.get())
}
