//! The one cipher: XChaCha20-Poly1305, from a vetted library (decision 0074).
//!
//! Nothing here is cryptography of our own. This module only decides what is
//! authenticated beside the ciphertext, and turns every failure to open into
//! the one mismatch failure.

use a2m_core::Failure;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::failure;
use crate::material::{KeyMaterial, RandomSource, Wrapped};

/// The number of bytes in a nonce. Long enough that random nonces under one
/// key do not meet in practice.
pub(crate) const NONCE_LENGTH: usize = 24;
/// The number of bytes the cipher adds to authenticate a ciphertext.
pub(crate) const TAG_LENGTH: usize = 16;

/// A fresh nonce from `random`.
pub(crate) fn fresh_nonce(random: &dyn RandomSource) -> Result<[u8; NONCE_LENGTH], Failure> {
    let mut nonce = [0u8; NONCE_LENGTH];
    random.fill(&mut nonce)?;
    Ok(nonce)
}

fn cipher(key: &KeyMaterial) -> Result<XChaCha20Poly1305, Failure> {
    XChaCha20Poly1305::new_from_slice(key.expose()).map_err(|_| failure::mismatch())
}

/// Encrypts `plain` under `key`, authenticating `bound_to` with it.
pub(crate) fn encrypt(
    key: &KeyMaterial,
    nonce: &[u8; NONCE_LENGTH],
    bound_to: &[u8],
    plain: &[u8],
) -> Result<Vec<u8>, Failure> {
    cipher(key)?
        .encrypt(
            &XNonce::from(*nonce),
            Payload {
                msg: plain,
                aad: bound_to,
            },
        )
        .map_err(|_| failure::mismatch())
}

/// Opens what [`encrypt`] made. A wrong key, a changed byte, or different
/// bound data all give the same failure and no bytes.
pub(crate) fn decrypt(
    key: &KeyMaterial,
    nonce: &[u8; NONCE_LENGTH],
    bound_to: &[u8],
    sealed: &[u8],
) -> Result<Zeroizing<Vec<u8>>, Failure> {
    cipher(key)?
        .decrypt(
            &XNonce::from(*nonce),
            Payload {
                msg: sealed,
                aad: bound_to,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| failure::mismatch())
}

/// Wraps `key` under `under`: a fresh nonce, then the ciphertext.
pub(crate) fn wrap(
    under: &KeyMaterial,
    key: &KeyMaterial,
    bound_to: &[u8],
    random: &dyn RandomSource,
) -> Result<Wrapped, Failure> {
    let nonce = fresh_nonce(random)?;
    let mut bytes = nonce.to_vec();
    bytes.extend_from_slice(&encrypt(under, &nonce, bound_to, key.expose())?);
    Ok(Wrapped::from_bytes(bytes))
}

/// Unwraps what [`wrap`] made.
pub(crate) fn unwrap(
    under: &KeyMaterial,
    wrapped: &[u8],
    bound_to: &[u8],
) -> Result<KeyMaterial, Failure> {
    let (nonce, sealed) = wrapped
        .split_first_chunk::<NONCE_LENGTH>()
        .ok_or_else(failure::mismatch)?;
    let plain = decrypt(under, nonce, bound_to, sealed)?;
    KeyMaterial::from_slice(&plain)
}

/// Bytes to authenticate beside a ciphertext: a purpose and named fields,
/// each written with its length so that no two sets of fields give the same
/// bytes.
pub(crate) struct BoundTo(Vec<u8>);

impl BoundTo {
    pub(crate) fn new(purpose: &str) -> Self {
        let mut bound = Self(Vec::with_capacity(160));
        bound.push(purpose.as_bytes());
        bound
    }

    pub(crate) fn field(mut self, name: &str, value: &[u8]) -> Self {
        self.push(name.as_bytes());
        self.push(value);
        self
    }

    pub(crate) fn number(self, name: &str, value: u32) -> Self {
        self.field(name, &value.to_le_bytes())
    }

    fn push(&mut self, bytes: &[u8]) {
        self.0
            .extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        self.0.extend_from_slice(bytes);
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
