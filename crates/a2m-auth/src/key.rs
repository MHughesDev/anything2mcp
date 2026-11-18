//! Which session, and what its auth file entry says about it.

use std::collections::BTreeMap;
use std::time::Duration;

use a2m_core::{Environment, Failure, Identity, RunnerId, SessionId, TargetName, WorkspaceId};

use crate::failure;
use crate::name::{CredentialRef, ProviderName};

/// What a session is keyed by: one identity on one target in one
/// environment, in one workspace (spec section 215).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionKey {
    /// The workspace, the boundary nothing crosses.
    pub workspace: WorkspaceId,
    /// The target.
    pub target: TargetName,
    /// Which copy of the system.
    pub environment: Environment,
    /// Who the system sees. Policy chose it; the keeper is told.
    pub identity: Identity,
}

/// One session as the keeper's work sees it: which it is, and at which epoch.
/// It names; it holds no credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    /// The session.
    pub session: SessionId,
    /// What it is keyed by.
    pub key: SessionKey,
    /// The epoch of the credentials now held, before the work.
    pub epoch: u64,
    /// The runner that holds the live state. `None` for the local engine.
    pub home: Option<RunnerId>,
}

/// One entry of a target's `auth.yaml`, as the keeper uses it. The compiler
/// checks the file; `a2m-actions` hands the entry in with each request.
///
/// Settings are names and plain values: an endpoint, a client identifier, a
/// header's name. Credentials are references only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthEntry {
    /// The auth provider.
    pub provider: ProviderName,
    /// The provider's own settings, by name. Each provider documents its
    /// names; one it does not know is refused.
    pub settings: BTreeMap<String, String>,
    /// Stored credentials by the role the provider gives them, such as
    /// `password` or `client_secret`.
    pub credentials: BTreeMap<String, CredentialRef>,
    /// The system's declared idle timeout, when known.
    pub idle: Option<Duration>,
    /// The identity provider's declared maximum age, when known.
    pub absolute: Option<Duration>,
    /// Automatic sign-in attempts with a stored credential; 1 by default.
    pub attempt_limit: u32,
    /// Whether the session's state may be saved and restored.
    pub movable: bool,
    /// What the plan holds about the entry's sign-in: the surfaces it
    /// covers, for a provider that reaches a surface itself (decision
    /// 0245); every address the target declares, where a sign-in may send;
    /// and whether the plan is a published version's (decision 0248).
    pub surfaces: crate::surfaces::EntrySurfaces,
}

impl AuthEntry {
    /// An entry for `provider` with the defaults of the file format: no
    /// settings, no credentials, no declared timeouts, one automatic
    /// attempt, and state that may be saved.
    #[must_use]
    pub fn new(provider: ProviderName) -> Self {
        Self {
            provider,
            settings: BTreeMap::new(),
            credentials: BTreeMap::new(),
            idle: None,
            absolute: None,
            attempt_limit: 1,
            movable: true,
            surfaces: crate::surfaces::EntrySurfaces::default(),
        }
    }

    /// The entry with one more setting.
    #[must_use]
    pub fn setting(mut self, name: &str, value: &str) -> Self {
        self.settings.insert(name.to_owned(), value.to_owned());
        self
    }

    /// The entry marked as taken from a published version's plan, as
    /// [`EntrySurfaces::published`](crate::EntrySurfaces::published) marks
    /// its surfaces. Only the caller that loaded the plan from the version
    /// store says so (decision 0248).
    #[must_use]
    pub fn published(mut self) -> Self {
        self.surfaces.from_published = true;
        self
    }

    /// The entry with one more credential reference.
    #[must_use]
    pub fn credential(mut self, role: &str, reference: CredentialRef) -> Self {
        self.credentials.insert(role.to_owned(), reference);
        self
    }

    /// A setting the provider cannot work without.
    ///
    /// # Errors
    ///
    /// `input`/`auth_settings_invalid` when it is absent or empty.
    pub fn required(&self, name: &str) -> Result<&str, Failure> {
        match self.settings.get(name) {
            Some(value) if !value.is_empty() => Ok(value),
            _ => Err(failure::settings_invalid(
                self.provider.as_str(),
                &format!("the setting `{name}` is missing"),
            )),
        }
    }

    /// A setting the provider can work without.
    #[must_use]
    pub fn optional(&self, name: &str) -> Option<&str> {
        self.settings
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    /// Refuses an entry that holds a setting or a credential role the
    /// provider does not know. Unknown means stop.
    ///
    /// # Errors
    ///
    /// `input`/`auth_settings_invalid`, naming the first unknown name.
    pub fn only(&self, settings: &[&str], roles: &[&str]) -> Result<(), Failure> {
        if let Some(name) = self
            .settings
            .keys()
            .find(|name| !settings.contains(&name.as_str()))
        {
            return Err(failure::settings_invalid(
                self.provider.as_str(),
                &format!("the setting `{name}` is not one this provider knows"),
            ));
        }
        if let Some(role) = self
            .credentials
            .keys()
            .find(|role| !roles.contains(&role.as_str()))
        {
            return Err(failure::settings_invalid(
                self.provider.as_str(),
                &format!("the credential role `{role}` is not one this provider knows"),
            ));
        }
        Ok(())
    }
}
