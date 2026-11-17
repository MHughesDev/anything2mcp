//! A session's credentials, as a provider makes them and the keeper's worker
//! holds them.

use std::collections::BTreeMap;
use std::fmt;

use a2m_core::{Failure, FailureClass, REDACTED, Secret, Timestamp};
use zeroize::Zeroizing;

use crate::name::{ProfileRef, ProviderName};

/// The four bytes saved material starts with, and its format number.
const MAGIC: &[u8; 4] = b"A2MM";
const FORMAT: u8 = 1;

/// A session's credentials: named secret parts, named plain parts, the
/// moment the credentials themselves run out, and the engine-owned profile
/// when the live state is a directory.
///
/// A provider makes it in `sign_in` and `refresh`, and reads it in `inject`.
/// It is held in memory on the session's home runner and nowhere else. It
/// cannot be cloned or serialized, its `Debug` shows names only, and the one
/// way to bytes is the keeper's worker sealing it in the vault.
pub struct Material {
    provider: ProviderName,
    secrets: BTreeMap<String, Secret>,
    plain: BTreeMap<String, String>,
    expires_at: Option<Timestamp>,
    profile: Option<ProfileRef>,
}

impl Material {
    /// Empty material of one provider.
    #[must_use]
    pub fn new(provider: ProviderName) -> Self {
        Self {
            provider,
            secrets: BTreeMap::new(),
            plain: BTreeMap::new(),
            expires_at: None,
            profile: None,
        }
    }

    /// The material with one more secret part, such as `access_token`.
    #[must_use]
    pub fn with_secret(mut self, name: &str, value: Secret) -> Self {
        self.secrets.insert(name.to_owned(), value);
        self
    }

    /// The material with one more plain part, such as `user`. Never a
    /// credential: a plain part may be shown.
    #[must_use]
    pub fn with_plain(mut self, name: &str, value: &str) -> Self {
        self.plain.insert(name.to_owned(), value.to_owned());
        self
    }

    /// The material with the moment its credentials run out.
    #[must_use]
    pub fn expiring_at(mut self, at: Option<Timestamp>) -> Self {
        self.expires_at = at;
        self
    }

    /// The material with the engine-owned profile that holds the live state.
    #[must_use]
    pub fn with_profile(mut self, profile: ProfileRef) -> Self {
        self.profile = Some(profile);
        self
    }

    /// The provider that made it.
    #[must_use]
    pub fn provider(&self) -> &ProviderName {
        &self.provider
    }

    /// One secret part.
    #[must_use]
    pub fn secret(&self, name: &str) -> Option<&Secret> {
        self.secrets.get(name)
    }

    /// One plain part.
    #[must_use]
    pub fn plain(&self, name: &str) -> Option<&str> {
        self.plain.get(name).map(String::as_str)
    }

    /// When the credentials themselves run out, if they say.
    #[must_use]
    pub fn expires_at(&self) -> Option<Timestamp> {
        self.expires_at
    }

    /// The engine-owned profile, when the live state is a directory.
    #[must_use]
    pub fn profile(&self) -> Option<&ProfileRef> {
        self.profile.as_ref()
    }

    /// Every secret part, for registering with the redactor.
    pub(crate) fn secrets(&self) -> impl Iterator<Item = &Secret> {
        self.secrets.values()
    }

    /// The material as bytes, for sealing and nothing else. The buffer is
    /// wiped when dropped.
    pub(crate) fn to_sealable(&self) -> Zeroizing<Vec<u8>> {
        // Sized once, so that growing never leaves a copy behind unwiped.
        let parts = |name: &String, length: usize| 8 + name.len() + length;
        let size = 32
            + self.provider.as_str().len()
            + self
                .profile
                .as_ref()
                .map_or(0, |profile| profile.as_str().len())
            + self
                .secrets
                .iter()
                .map(|(name, value)| parts(name, value.expose().len()))
                .sum::<usize>()
            + self
                .plain
                .iter()
                .map(|(name, value)| parts(name, value.len()))
                .sum::<usize>();
        let mut out = Zeroizing::new(Vec::with_capacity(size));
        out.extend_from_slice(MAGIC);
        out.push(FORMAT);
        put(&mut out, self.provider.as_str().as_bytes());
        match self.expires_at {
            Some(at) => {
                out.push(1);
                out.extend_from_slice(&at.as_unix_millis().to_le_bytes());
            }
            None => out.push(0),
        }
        match &self.profile {
            Some(profile) => {
                out.push(1);
                put(&mut out, profile.as_str().as_bytes());
            }
            None => out.push(0),
        }
        put_count(&mut out, self.secrets.len());
        for (name, value) in &self.secrets {
            put(&mut out, name.as_bytes());
            put(&mut out, value.expose().as_bytes());
        }
        put_count(&mut out, self.plain.len());
        for (name, value) in &self.plain {
            put(&mut out, name.as_bytes());
            put(&mut out, value.as_bytes());
        }
        out
    }

    /// Reads what [`Material::to_sealable`] wrote.
    ///
    /// # Errors
    ///
    /// `input`/`sealed_item_mismatch` when the bytes are not saved material
    /// of a format this engine reads.
    pub(crate) fn from_sealable(bytes: &[u8]) -> Result<Self, Failure> {
        Reader(bytes).material().ok_or_else(unreadable)
    }
}

impl fmt::Debug for Material {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Material")
            .field("provider", &self.provider.as_str())
            .field("parts", &self.secrets.keys().collect::<Vec<_>>())
            .field("values", &REDACTED)
            .finish()
    }
}

fn unreadable() -> Failure {
    Failure::new(
        FailureClass::Input,
        a2m_vault::codes::SEALED_ITEM_MISMATCH,
        "The saved session state is not in a form this engine reads.",
        "Sign in again; the saved state is made anew.",
    )
}

fn put_count(out: &mut Vec<u8>, count: usize) {
    out.extend_from_slice(&u32::try_from(count).unwrap_or(u32::MAX).to_le_bytes());
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    put_count(out, bytes.len());
    out.extend_from_slice(bytes);
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Option<&[u8]> {
        let (head, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        Some(head)
    }

    fn byte(&mut self) -> Option<u8> {
        self.take(1).map(|bytes| bytes[0])
    }

    fn count(&mut self) -> Option<usize> {
        let bytes: [u8; 4] = self.take(4)?.try_into().ok()?;
        usize::try_from(u32::from_le_bytes(bytes)).ok()
    }

    fn text(&mut self) -> Option<String> {
        let length = self.count()?;
        String::from_utf8(self.take(length)?.to_vec()).ok()
    }

    fn material(mut self) -> Option<Material> {
        if self.take(MAGIC.len())? != MAGIC || self.byte()? != FORMAT {
            return None;
        }
        let mut material = Material::new(ProviderName::new(&self.text()?).ok()?);
        if self.byte()? == 1 {
            let bytes: [u8; 8] = self.take(8)?.try_into().ok()?;
            material.expires_at = Some(Timestamp::from_unix_millis(i64::from_le_bytes(bytes)));
        }
        if self.byte()? == 1 {
            material.profile = Some(ProfileRef::new(&self.text()?).ok()?);
        }
        for _ in 0..self.count()? {
            let name = self.text()?;
            material.secrets.insert(name, Secret::new(self.text()?));
        }
        for _ in 0..self.count()? {
            let name = self.text()?;
            material.plain.insert(name, self.text()?);
        }
        self.0.is_empty().then_some(material)
    }
}

#[cfg(test)]
mod tests {
    use a2m_core::{Secret, Timestamp};

    use super::Material;
    use crate::name::{ProfileRef, ProviderName};

    fn sample() -> Material {
        Material::new(ProviderName::new("oauth_authorization_code").unwrap())
            .with_secret("access_token", Secret::new("at-PLANTED-0001"))
            .with_secret("refresh_token", Secret::new("rt-PLANTED-0002"))
            .with_plain("token_type", "Bearer")
            .expiring_at(Some(Timestamp::from_unix_millis(1_700_000_000_000)))
            .with_profile(ProfileRef::new("s_abc").unwrap())
    }

    #[test]
    fn material_never_prints_a_secret() {
        let shown = format!("{:?} {:#?}", sample(), sample());
        assert!(!shown.contains("PLANTED"), "{shown}");
        assert!(shown.contains("[redacted]"), "{shown}");
        assert!(shown.contains("access_token"), "{shown}");
    }

    #[test]
    fn material_survives_being_saved_and_read_back() {
        let bytes = sample().to_sealable();
        let back = Material::from_sealable(&bytes).unwrap();
        assert_eq!(back.provider().as_str(), "oauth_authorization_code");
        assert_eq!(
            back.secret("access_token").unwrap().expose(),
            "at-PLANTED-0001"
        );
        assert_eq!(
            back.secret("refresh_token").unwrap().expose(),
            "rt-PLANTED-0002"
        );
        assert_eq!(back.plain("token_type"), Some("Bearer"));
        assert_eq!(
            back.expires_at(),
            Some(Timestamp::from_unix_millis(1_700_000_000_000))
        );
        assert_eq!(back.profile().unwrap().as_str(), "s_abc");
    }

    #[test]
    fn damaged_saved_material_is_refused_and_never_half_read() {
        let bytes = sample().to_sealable();
        for cut in [0, 3, 5, bytes.len() / 2, bytes.len() - 1] {
            assert!(Material::from_sealable(&bytes[..cut]).is_err(), "{cut}");
        }
        let mut longer = bytes.to_vec();
        longer.push(0);
        assert!(Material::from_sealable(&longer).is_err());
    }
}
