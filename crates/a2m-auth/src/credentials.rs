//! Stored credentials: what an auth file's `vault://` references name.

use std::sync::Arc;

use a2m_core::{Failure, Identity, Secret, WorkspaceId};
use a2m_vault::{BoxFuture, ItemId, OwnerRef, SealContext, SealedKind, Vault};

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
/// keeper's rows. A credential stored for one identity cannot be opened for
/// another: the owner is part of what the vault authenticates.
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
    /// replacing what was there. The secret is registered with the redactor.
    ///
    /// # Errors
    ///
    /// The vault's failures when it cannot seal.
    pub async fn put(
        &self,
        workspace: &WorkspaceId,
        identity: &Identity,
        reference: &CredentialRef,
        secret: &Secret,
    ) -> Result<(), Failure> {
        self.vault.register(secret);
        let cx = context(workspace, identity, reference)?;
        let sealed = self.vault.seal(&cx, secret.expose().as_bytes()).await?;
        self.store
            .put_credential(*workspace, identity.to_string(), reference.clone(), sealed)
            .await
    }
}

impl Credentials for StoredCredentials {
    fn open<'a>(
        &'a self,
        session: &'a SessionKey,
        reference: &'a CredentialRef,
    ) -> BoxFuture<'a, Result<Secret, Failure>> {
        Box::pin(async move {
            let sealed = self
                .store
                .credential(
                    session.workspace,
                    session.identity.to_string(),
                    reference.clone(),
                )
                .await?
                .ok_or_else(|| failure::credential_missing(reference.as_str()))?;
            let cx = context(&session.workspace, &session.identity, reference)?;
            self.vault.open_secret(&cx, &sealed).await
        })
    }
}

fn context(
    workspace: &WorkspaceId,
    identity: &Identity,
    reference: &CredentialRef,
) -> Result<SealContext, Failure> {
    Ok(SealContext {
        workspace: *workspace,
        owner: OwnerRef::of(identity),
        kind: SealedKind::Credential,
        item: ItemId::new(reference.as_str())?,
    })
}
