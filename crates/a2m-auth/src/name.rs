//! The names this crate checks: a provider, a credential reference, and the
//! engine-owned place a session's live state is kept.

use a2m_core::{Failure, FailureClass, FailureCode};

fn bad(what: &'static str, rule: &'static str) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::SCHEMA_INVALID,
        format!("This is not a well-formed {what}: {rule}."),
        format!("Write the {what} as the rule says."),
    )
}

fn is_word(text: &str, longest: usize) -> bool {
    let mut bytes = text.bytes();
    let starts = bytes.next().is_some_and(|first| first.is_ascii_lowercase());
    starts
        && text.len() <= longest
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The name of an auth provider, such as `api_key`, and a reference to a
/// stored credential, `vault://` and one to four parts. Both are `a2m-core`'s,
/// because the compiled target carries them (decision 0121); this crate names
/// them as it always has.
pub use a2m_core::{AuthProviderName as ProviderName, CredentialRef};

/// The engine-owned place where a session's live state is kept on its home
/// runner, when the state is a directory: a name, never a path. The provider
/// that owns the session resolves it under the directory the engine gives
/// it, so no caller's input can choose a file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileRef(String);

impl ProfileRef {
    /// The longest a profile's name may be.
    pub const LONGEST: usize = 64;

    /// Checks `name` and keeps it.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` when the name is not lowercase letters,
    /// digits, and underscores starting with a letter, or is too long.
    pub fn new(name: &str) -> Result<Self, Failure> {
        if !is_word(name, Self::LONGEST) {
            return Err(bad(
                "profile name",
                "lowercase letters, digits, and underscores, starting with a letter, at most 64",
            ));
        }
        Ok(Self(name.to_owned()))
    }

    /// The name, which is one path component.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! as_text {
    ($name:ident) => {
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = <String as serde::Deserialize>::deserialize(deserializer)?;
                Self::new(&text).map_err(|failure| serde::de::Error::custom(failure.message()))
            }
        }
    };
}

as_text!(ProfileRef);

#[cfg(test)]
mod tests {
    use super::{CredentialRef, ProfileRef, ProviderName};

    #[test]
    fn names_are_checked() {
        assert!(ProviderName::new("api_key").is_ok());
        for bad in ["", "Api", "api-key", "9key", &"a".repeat(33)] {
            assert!(ProviderName::new(bad).is_err(), "{bad}");
        }
        assert!(ProfileRef::new("s_0123abcd").is_ok());
        for bad in ["", "..", "a/b", "A"] {
            assert!(ProfileRef::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_credential_reference_is_a_vault_name_and_nothing_else() {
        assert!(CredentialRef::new("vault://example/service/password").is_ok());
        for bad in [
            "example/service/password",
            "vault://",
            "vault://a//b",
            "vault://a/../b",
            "vault://a/b/c/d/e",
            "vault://A",
            "file:///etc/passwd",
        ] {
            assert!(CredentialRef::new(bad).is_err(), "{bad}");
        }
    }
}
