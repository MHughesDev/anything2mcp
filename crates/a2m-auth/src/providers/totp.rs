//! One-time codes (RFC 6238) for a dedicated account, where IT allows a
//! stored one-time-code secret.
//!
//! This is not a provider of sessions. It computes the code for a moment
//! from a stored secret; the provider that signs the account in enters it
//! where the system asks, through
//! [`SignInContext::one_time_code`](crate::SignInContext::one_time_code).

use a2m_core::{Failure, Secret, Timestamp};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

use crate::encode::base32_decode;
use crate::failure;
use crate::key::AuthEntry;

/// The credential role that holds the one-time-code secret, in Base32.
pub const ROLE: &str = "totp_secret";

/// The hash a code is computed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// SHA-1, which the standard uses by default and most systems expect.
    Sha1,
    /// SHA-256.
    Sha256,
    /// SHA-512.
    Sha512,
}

/// How a system's one-time codes are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// How many digits a code has: 6 to 8.
    pub digits: u32,
    /// How many seconds a code lasts.
    pub period: u32,
    /// The hash.
    pub algorithm: Algorithm,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            digits: 6,
            period: 30,
            algorithm: Algorithm::Sha1,
        }
    }
}

impl Settings {
    /// The settings an auth file entry states: `totp_digits`, `totp_period`,
    /// and `totp_algorithm`, each with the standard's default.
    ///
    /// # Errors
    ///
    /// `input`/`auth_settings_invalid` when one is out of range.
    pub fn from_entry(entry: &AuthEntry) -> Result<Self, Failure> {
        let wrong = |what: &str| failure::settings_invalid(entry.provider.as_str(), what);
        let mut settings = Self::default();
        if let Some(digits) = entry.optional("totp_digits") {
            settings.digits = digits
                .parse()
                .ok()
                .filter(|digits| (6..=8).contains(digits))
                .ok_or_else(|| wrong("the setting `totp_digits` is not a number from 6 to 8"))?;
        }
        if let Some(period) = entry.optional("totp_period") {
            settings.period = period
                .parse()
                .ok()
                .filter(|period| (1..=3600).contains(period))
                .ok_or_else(|| wrong("the setting `totp_period` is not a number of seconds"))?;
        }
        settings.algorithm = match entry.optional("totp_algorithm") {
            None | Some("sha1") => Algorithm::Sha1,
            Some("sha256") => Algorithm::Sha256,
            Some("sha512") => Algorithm::Sha512,
            Some(_) => {
                return Err(wrong(
                    "the setting `totp_algorithm` is not `sha1`, `sha256`, or `sha512`",
                ));
            }
        };
        Ok(settings)
    }
}

/// HMAC (RFC 2104) over a hash with the given block size.
fn hmac<H: Digest>(block_size: usize, key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut block = vec![0u8; block_size];
    if key.len() > block_size {
        let digest = H::digest(key);
        block[..digest.len()].copy_from_slice(&digest);
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |with: u8| block.iter().map(|byte| byte ^ with).collect::<Vec<u8>>();
    let mut inner = H::new();
    inner.update(pad(0x36));
    inner.update(message);
    let mut outer = H::new();
    outer.update(pad(0x5c));
    outer.update(inner.finalize());
    outer.finalize().to_vec()
}

/// The one-time code for the moment `at`, from a secret written in Base32.
/// The caller registers the code with the redactor before it goes anywhere.
///
/// # Errors
///
/// A fixed phrase, never the secret, when the secret is not Base32, the
/// moment is before 1970, or the settings are out of range.
pub fn code(secret: &Secret, at: Timestamp, settings: &Settings) -> Result<Secret, &'static str> {
    if !(6..=8).contains(&settings.digits) || settings.period == 0 {
        return Err("the one-time-code settings are out of range");
    }
    let key = base32_decode(secret.expose())
        .ok_or("the stored one-time-code secret is not written in Base32")?;
    let seconds = u64::try_from(at.as_unix_millis() / 1000)
        .map_err(|_| "the clock shows a moment before 1970")?;
    let counter = (seconds / u64::from(settings.period)).to_be_bytes();
    let mac = match settings.algorithm {
        Algorithm::Sha1 => hmac::<Sha1>(64, &key, &counter),
        Algorithm::Sha256 => hmac::<Sha256>(64, &key, &counter),
        Algorithm::Sha512 => hmac::<Sha512>(128, &key, &counter),
    };
    // Dynamic truncation (RFC 4226, section 5.3).
    let offset = usize::from(mac.last().copied().unwrap_or(0) & 0x0f);
    let word = mac
        .get(offset..offset + 4)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .ok_or("the one-time code could not be computed")?;
    let number = (u32::from_be_bytes(word) & 0x7fff_ffff) % 10u32.pow(settings.digits);
    let width = settings.digits as usize;
    Ok(Secret::new(format!("{number:0width$}")))
}

#[cfg(test)]
mod tests {
    use a2m_core::{Secret, Timestamp};

    use super::{Algorithm, Settings, code};

    /// The Base32 of the ASCII keys RFC 6238 uses in its appendix B.
    const SHA1_KEY: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    const SHA256_KEY: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZA====";
    const SHA512_KEY: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNA=";

    fn at(seconds: i64, key: &str, algorithm: Algorithm) -> String {
        let settings = Settings {
            digits: 8,
            period: 30,
            algorithm,
        };
        code(
            &Secret::new(key),
            Timestamp::from_unix_millis(seconds * 1000),
            &settings,
        )
        .unwrap()
        .expose()
        .to_owned()
    }

    #[test]
    fn codes_match_the_standards_test_vectors() {
        assert_eq!(at(59, SHA1_KEY, Algorithm::Sha1), "94287082");
        assert_eq!(at(59, SHA256_KEY, Algorithm::Sha256), "46119246");
        assert_eq!(at(59, SHA512_KEY, Algorithm::Sha512), "90693936");
        assert_eq!(at(1_111_111_109, SHA1_KEY, Algorithm::Sha1), "07081804");
        assert_eq!(at(1_111_111_109, SHA256_KEY, Algorithm::Sha256), "68084774");
        assert_eq!(at(1_111_111_109, SHA512_KEY, Algorithm::Sha512), "25091201");
        assert_eq!(at(20_000_000_000, SHA1_KEY, Algorithm::Sha1), "65353130");
    }

    #[test]
    fn a_code_has_the_digits_asked_for_and_a_bad_secret_is_named_without_being_shown() {
        let six = code(
            &Secret::new(SHA1_KEY),
            Timestamp::from_unix_millis(59_000),
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(six.expose(), "287082");
        let bad = code(
            &Secret::new("not base32 !PLANTED!"),
            Timestamp::from_unix_millis(0),
            &Settings::default(),
        )
        .unwrap_err();
        assert!(!bad.contains("PLANTED"));
        let early = code(
            &Secret::new(SHA1_KEY),
            Timestamp::from_unix_millis(-5_000),
            &Settings::default(),
        );
        assert!(early.is_err());
    }
}
