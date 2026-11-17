//! Keyed digests: names that reveal nothing to someone who guesses.

use std::fmt;

use a2m_core::{Digest, DigestPurpose, KeyedDigest, WorkspaceId, keyed_digest_under};
use zeroize::Zeroizing;

use crate::material::KeyMaterial;

/// The vault's keyed digest. It holds the installation's digest key in wiped
/// memory and never releases it; each workspace's digests are made under a
/// key derived from it, so nothing can be matched across workspaces.
///
/// It is made by [`Vault::keyed_digest`](crate::Vault::keyed_digest), and
/// handed, as a `KeyedDigest`, to the ledger for blob names and to telemetry
/// for restricted values.
pub struct VaultDigest {
    key: Zeroizing<Vec<u8>>,
}

impl VaultDigest {
    pub(crate) fn new(key: &KeyMaterial) -> Self {
        Self {
            key: Zeroizing::new(key.expose().to_vec()),
        }
    }
}

impl KeyedDigest for VaultDigest {
    fn keyed_digest(
        &self,
        workspace: &WorkspaceId,
        purpose: DigestPurpose,
        bytes: &[u8],
    ) -> Digest {
        keyed_digest_under(&self.key, workspace, purpose, bytes)
    }
}

impl fmt::Debug for VaultDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VaultDigest([redacted])")
    }
}
