//! Published versions and their plans, stored by hash.
//!
//! A version is the bytes of its plan and nothing else. They are written
//! once, by publishing, and the hash is checked on every load.

use a2m_core::{
    Digest, EncodedPlan, Failure, PrincipalId, TargetName, Timestamp, Version, WorkspaceId,
};

use crate::codec::{moment, parse};
use crate::failure;
use crate::tx::Tx;

/// A version to store. Made only by publishing (`version.publish`).
#[derive(Debug, Clone)]
pub struct PublishedVersion {
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The target.
    pub target: TargetName,
    /// The version's number.
    pub version: Version,
    /// The plan's bytes and their hash, from `Plan::encode`.
    pub plan: EncodedPlan,
    /// Who published it.
    pub published_by: PrincipalId,
    /// The evidence of the publish gates it passed, as canonical JSON: what
    /// each gate found, and which the documents name that did not run
    /// (spec section 403, decision 0180).
    pub gate_evidence: Option<String>,
}

/// A stored version, without its plan's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    /// The hash that names the plan.
    pub plan_hash: Digest,
    /// The version's number.
    pub version: Version,
    /// Who published it.
    pub published_by: PrincipalId,
    /// When.
    pub published_at: Timestamp,
    /// Whether it is the version that runs now.
    pub current: bool,
}

/// Published versions, inside one transaction.
pub struct Versions<'t, 'a> {
    tx: &'t mut Tx<'a>,
}

impl<'a> Tx<'a> {
    /// Published versions and their plans.
    pub fn versions(&mut self) -> Versions<'_, 'a> {
        Versions { tx: self }
    }
}

impl Versions<'_, '_> {
    /// Stores a published version. Storing the same plan again changes
    /// nothing.
    ///
    /// # Errors
    ///
    /// `input`/`plan_hash_mismatch` when the hash given is not the digest of
    /// the bytes given.
    pub fn put(&mut self, version: &PublishedVersion) -> Result<(), Failure> {
        if Digest::of(&version.plan.bytes) != version.plan.hash {
            return Err(a2m_core::codes::row::PLAN_HASH_MISMATCH.failure(
                "The plan's hash is not the digest of its bytes.",
                "Compile the target again and publish what the compiler gives.",
            ));
        }
        self.tx.exec(
            "INSERT INTO target_versions
                (workspace_id, target, plan_hash, version, plan, published_by, published_at,
                 gate_evidence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (workspace_id, target, plan_hash) DO NOTHING",
            &[
                version.workspace.to_string().into(),
                version.target.as_str().into(),
                version.plan.hash.to_string().into(),
                version.version.to_string().into(),
                version.plan.bytes.clone().into(),
                version.published_by.to_string().into(),
                self.tx.now().into(),
                version.gate_evidence.clone().into(),
            ],
        )?;
        Ok(())
    }

    /// The evidence of the publish gates the version with this hash passed,
    /// as it was stored with it; `None` for a version stored without.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when no such version is stored.
    pub fn gate_evidence(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        hash: &Digest,
    ) -> Result<Option<String>, Failure> {
        let row = self
            .tx
            .row(
                "SELECT gate_evidence FROM target_versions
                 WHERE workspace_id = ?1 AND target = ?2 AND plan_hash = ?3",
                &[
                    workspace.to_string().into(),
                    target.as_str().into(),
                    hash.to_string().into(),
                ],
            )?
            .ok_or_else(|| failure::not_found("published version"))?;
        Ok(row.opt_text(0)?.map(str::to_owned))
    }

    /// The plan with this hash. The bytes are hashed again before they are
    /// returned.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when no such version is stored;
    /// `busy`/`store_corrupt` when the stored bytes no longer have the hash
    /// that names them.
    pub fn load(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        hash: &Digest,
    ) -> Result<EncodedPlan, Failure> {
        let row = self
            .tx
            .row(
                "SELECT plan FROM target_versions
                 WHERE workspace_id = ?1 AND target = ?2 AND plan_hash = ?3",
                &[
                    workspace.to_string().into(),
                    target.as_str().into(),
                    hash.to_string().into(),
                ],
            )?
            .ok_or_else(|| failure::not_found("published version"))?;
        let bytes = row.bytes(0)?.to_vec();
        if Digest::of(&bytes) != *hash {
            return Err(failure::corrupt("a stored plan's hash"));
        }
        Ok(EncodedPlan { bytes, hash: *hash })
    }

    /// Makes the version with this hash the one that runs now.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when no such version is stored.
    pub fn make_current(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        hash: &Digest,
    ) -> Result<(), Failure> {
        let place = [workspace.to_string().into(), target.as_str().into()];
        self.tx.exec(
            "UPDATE target_versions SET is_current = 0
             WHERE workspace_id = ?1 AND target = ?2 AND is_current = 1",
            &place,
        )?;
        let made = self.tx.exec(
            "UPDATE target_versions SET is_current = 1
             WHERE workspace_id = ?1 AND target = ?2 AND plan_hash = ?3",
            &[
                workspace.to_string().into(),
                target.as_str().into(),
                hash.to_string().into(),
            ],
        )?;
        if made == 1 {
            Ok(())
        } else {
            Err(failure::not_found("published version"))
        }
    }

    /// The versions of `target`, oldest first.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn list(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
    ) -> Result<Vec<VersionRow>, Failure> {
        self.tx
            .rows(
                "SELECT plan_hash, version, published_by, published_at, is_current
                 FROM target_versions WHERE workspace_id = ?1 AND target = ?2
                 ORDER BY published_at, plan_hash",
                &[workspace.to_string().into(), target.as_str().into()],
            )?
            .iter()
            .map(|row| {
                Ok(VersionRow {
                    plan_hash: parse(row.text(0)?)?,
                    version: parse(row.text(1)?)?,
                    published_by: parse(row.text(2)?)?,
                    published_at: moment(row.int(3)?),
                    current: row.int(4)? == 1,
                })
            })
            .collect()
    }

    /// The version that runs now, or `None`.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn current(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
    ) -> Result<Option<Digest>, Failure> {
        self.tx
            .row(
                "SELECT plan_hash FROM target_versions
                 WHERE workspace_id = ?1 AND target = ?2 AND is_current = 1",
                &[workspace.to_string().into(), target.as_str().into()],
            )?
            .map(|row| parse(row.text(0)?))
            .transpose()
    }
}
