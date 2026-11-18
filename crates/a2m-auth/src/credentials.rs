//! Stored credentials: what an auth file's `vault://` references name.
//!
//! A credential belongs to the target it was stored for (decision 0248). A
//! reference's first part is that target's name, `vault://<target>/...`; an
//! auth entry that names a reference of another target is refused by name
//! before anything is opened; and the vault seals the credential with the
//! target in its context, so it does not open under another target even
//! where a check above were missed.

use std::sync::Arc;

use a2m_core::{Failure, Identity, Secret, TargetName, WorkspaceId};
use a2m_vault::{BoxFuture, OwnerRef, SealContext, Vault};

use crate::failure;
use crate::key::SessionKey;
use crate::name::CredentialRef;
use crate::store::SessionStore;

/// Where the keeper's worker gets a stored credential from, by reference.
///
/// [`StoredCredentials`] keeps them sealed in the keeper's own rows. A
/// runner that fetches a service account's credentials from an outside
/// secrets manager implements this over that manager.
///
/// An implementation registers the secret with the redactor before it
/// returns it, and never writes it anywhere.
pub trait Credentials: Send + Sync {
    /// The credential stored under `reference` for the identity of
    /// `session`.
    ///
    /// # Errors
    ///
    /// `input`/`credential_missing` when nothing is stored there; the
    /// vault's failures when it cannot be opened.
    fn open<'a>(
        &'a self,
        session: &'a SessionKey,
        reference: &'a CredentialRef,
    ) -> BoxFuture<'a, Result<Secret, Failure>>;
}

/// Credentials sealed in the vault under their owner's key and kept in the
/// keeper's rows. A credential stored for one identity, or for one target,
/// cannot be opened for another: the owner and the target are part of what
/// the vault authenticates.
#[derive(Debug, Clone)]
pub struct StoredCredentials {
    store: SessionStore,
    vault: Arc<Vault>,
}

impl StoredCredentials {
    /// Stored credentials over the keeper's rows and the vault.
    #[must_use]
    pub fn new(store: SessionStore, vault: Arc<Vault>) -> Self {
        Self { store, vault }
    }

    /// Stores `secret` under `reference` for one identity of one workspace,
    /// replacing what was there, sealed for the target the reference
    /// belongs to: its first part. The secret is registered with the
    /// redactor.
    ///
    /// # Errors
    ///
    /// `input`/`credential_of_another_target` when the reference's first
    /// part is not a target's name; the vault's failures when it cannot
    /// seal.
    pub async fn put(
        &self,
        workspace: &WorkspaceId,
        identity: &Identity,
        reference: &CredentialRef,
        secret: &Secret,
    ) -> Result<(), Failure> {
        self.vault.register(secret);
        let target = target_of(reference)?;
        let cx = context(workspace, identity, &target, reference)?;
        let sealed = self.vault.seal(&cx, secret.expose().as_bytes()).await?;
        self.store
            .put_credential(*workspace, identity.to_string(), reference.clone(), sealed)
            .await
    }

    /// Whether something is stored under `reference` for one identity of
    /// one workspace. Nothing is opened: what is stored stays sealed, and
    /// no secret is read to answer.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub async fn holds(
        &self,
        workspace: &WorkspaceId,
        identity: &Identity,
        reference: &CredentialRef,
    ) -> Result<bool, Failure> {
        Ok(self
            .store
            .credential(*workspace, identity.to_string(), reference.clone())
            .await?
            .is_some())
    }
}

impl Credentials for StoredCredentials {
    fn open<'a>(
        &'a self,
        session: &'a SessionKey,
        reference: &'a CredentialRef,
    ) -> BoxFuture<'a, Result<Secret, Failure>> {
        Box::pin(async move {
            belongs_to(reference, &session.target)?;
            let sealed = self
                .store
                .credential(
                    session.workspace,
                    session.identity.to_string(),
                    reference.clone(),
                )
                .await?
                .ok_or_else(|| failure::credential_missing(reference.as_str()))?;
            let cx = context(
                &session.workspace,
                &session.identity,
                &session.target,
                reference,
            )?;
            self.vault
                .open_secret(&cx, &sealed)
                .await
                .map_err(|failure| {
                    // Sealed for another target, or before the target was
                    // part of the context: the person stores it again.
                    if failure.code() == &a2m_vault::codes::SEALED_ITEM_MISMATCH {
                        failure::credential_unreadable(reference.as_str())
                    } else {
                        failure
                    }
                })
        })
    }
}

/// The target a reference belongs to: its first part.
fn target_of(reference: &CredentialRef) -> Result<TargetName, Failure> {
    reference
        .as_str()
        .strip_prefix(CredentialRef::SCHEME)
        .and_then(|path| path.split('/').next())
        .and_then(|first| TargetName::new(first).ok())
        .ok_or_else(|| failure::reference_names_no_target(reference.as_str()))
}

/// Refuses a reference that does not belong to `target`.
///
/// # Errors
///
/// `input`/`credential_of_another_target`.
pub(crate) fn belongs_to(reference: &CredentialRef, target: &TargetName) -> Result<(), Failure> {
    match target_of(reference) {
        Ok(own) if &own == target => Ok(()),
        _ => Err(failure::credential_of_another_target(
            reference.as_str(),
            target.as_str(),
        )),
    }
}

/// What a stored credential is sealed as: its workspace, its owner, the
/// target it is for, and its reference.
pub(crate) fn context(
    workspace: &WorkspaceId,
    identity: &Identity,
    target: &TargetName,
    reference: &CredentialRef,
) -> Result<SealContext, Failure> {
    SealContext::credential(
        *workspace,
        OwnerRef::of(identity),
        target,
        reference.as_str(),
    )
}
