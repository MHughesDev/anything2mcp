//! This crate's tables, through the store's transaction. Every statement
//! here names only tables `a2m-auth` owns, and the store refuses any other.
//!
//! Each function is plain and takes the transaction, so that a caller above
//! can put it in one transaction with another crate's writes (decision
//! 0080).

use std::str::FromStr;

use a2m_core::{
    Digest, Environment, Failure, FailureClass, Identity, SessionId, TargetName, Timestamp,
    WorkspaceId,
};
use a2m_ledger::{Owner, Row, SqlValue, Tx};
use a2m_vault::Sealed;

use crate::key::SessionKey;
use crate::name::{CredentialRef, ProfileRef, ProviderName};
use crate::person::PersonReason;
use crate::store::{EventKind, Saved, SessionEvent, SessionRow};

const OWNER: Owner = Owner::Auth;

/// The columns of a session, in the order [`row`] reads them.
macro_rules! columns {
    () => {
        "id, workspace_id, target, environment, identity_ref, provider, state, epoch, \
         home_runner_id, movable, profile, state_since, signed_in_at, expires_at, checked_at, \
         last_used_at, stored_attempts, wants, attempted_with, signed_in_under"
    };
}

/// A row holds something the code does not expect.
fn corrupt(what: &'static str) -> Failure {
    Failure::new(
        FailureClass::Busy,
        a2m_ledger::codes::STORE_CORRUPT,
        format!("A session's record holds {what} this engine cannot read."),
        "Run `anything2mcp doctor`. If the store was written by a newer engine, use that engine.",
    )
}

fn parsed<T: FromStr>(text: &str, what: &'static str) -> Result<T, Failure> {
    text.parse().map_err(|_| corrupt(what))
}

fn moment(value: Option<i64>) -> Option<Timestamp> {
    value.map(Timestamp::from_unix_millis)
}

fn number(value: u64) -> SqlValue {
    SqlValue::Int(i64::try_from(value).unwrap_or(i64::MAX))
}

fn row(row: &Row) -> Result<SessionRow, Failure> {
    Ok(SessionRow {
        id: parsed(row.text(0)?, "an identifier")?,
        key: SessionKey {
            workspace: parsed::<WorkspaceId>(row.text(1)?, "a workspace")?,
            target: TargetName::new(row.text(2)?).map_err(|_| corrupt("a target name"))?,
            environment: Environment::new(row.text(3)?)
                .map_err(|_| corrupt("an environment name"))?,
            identity: parsed::<Identity>(row.text(4)?, "an identity")?,
        },
        provider: ProviderName::new(row.text(5)?).map_err(|_| corrupt("a provider name"))?,
        state: parsed(row.text(6)?, "a state")?,
        epoch: u64::try_from(row.int(7)?).map_err(|_| corrupt("an epoch"))?,
        home: row
            .opt_text(8)?
            .map(|text| parsed(text, "a runner"))
            .transpose()?,
        movable: row.int(9)? != 0,
        profile: row
            .opt_text(10)?
            .map(|text| ProfileRef::new(text).map_err(|_| corrupt("a profile name")))
            .transpose()?,
        state_since: Timestamp::from_unix_millis(row.int(11)?),
        signed_in_at: moment(row.opt_int(12)?),
        expires_at: moment(row.opt_int(13)?),
        checked_at: moment(row.opt_int(14)?),
        last_used_at: moment(row.opt_int(15)?),
        stored_attempts: u32::try_from(row.int(16)?).map_err(|_| corrupt("a count"))?,
        wants: row
            .opt_text(17)?
            .map(|text| PersonReason::from_name(text).ok_or_else(|| corrupt("a reason")))
            .transpose()?,
        attempted_with: row
            .opt_text(18)?
            .map(|text| parsed(text, "a credential's mark"))
            .transpose()?,
        // Only the word `published` reads as published. Null, as in a row
        // kept before migration 0027, and any word this engine does not
        // know, read as made under drafts: fail closed (decision 0374).
        made_under_drafts: row.opt_text(19)? != Some(PUBLISHED),
    })
}

/// What `signed_in_under` holds for a session made under a published
/// version, and for one made under drafts.
const PUBLISHED: &str = "published";
const DRAFTS: &str = "drafts";

fn under(made_under_drafts: bool) -> SqlValue {
    SqlValue::from(if made_under_drafts { DRAFTS } else { PUBLISHED })
}

/// The session of a key, if it has a record.
pub(crate) fn find(tx: &mut Tx<'_>, key: &SessionKey) -> Result<Option<SessionRow>, Failure> {
    tx.tables(OWNER)
        .query_first(
            concat!(
                "SELECT ",
                columns!(),
                " FROM sessions WHERE workspace_id = ?1 AND target = ?2 AND environment = ?3
                  AND identity_ref = ?4"
            ),
            &[
                SqlValue::from(key.workspace.to_string()),
                SqlValue::from(key.target.as_str()),
                SqlValue::from(key.environment.as_str()),
                SqlValue::from(key.identity.to_string()),
            ],
        )?
        .map(|found| row(&found))
        .transpose()
}

/// One session by its identifier.
pub(crate) fn get(tx: &mut Tx<'_>, id: SessionId) -> Result<Option<SessionRow>, Failure> {
    tx.tables(OWNER)
        .query_first(
            concat!("SELECT ", columns!(), " FROM sessions WHERE id = ?1"),
            &[SqlValue::from(id.to_string())],
        )?
        .map(|found| row(&found))
        .transpose()
}

/// Every session, in the order they were made.
pub(crate) fn all(tx: &mut Tx<'_>) -> Result<Vec<SessionRow>, Failure> {
    tx.tables(OWNER)
        .query(
            concat!("SELECT ", columns!(), " FROM sessions ORDER BY id"),
            &[],
        )?
        .iter()
        .map(row)
        .collect()
}

fn insert_event(tx: &mut Tx<'_>, event: &SessionEvent) -> Result<(), Failure> {
    tx.tables(OWNER).execute(
        "INSERT INTO session_events (session_id, at, kind, state, epoch, cause, run_id, decision_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        &[
            SqlValue::from(event.session.to_string()),
            SqlValue::from(event.at),
            SqlValue::from(event.kind.as_str()),
            SqlValue::from(event.state.as_str()),
            number(event.epoch),
            SqlValue::from(event.cause.as_str()),
            SqlValue::from(event.run.map(|run| run.to_string())),
            SqlValue::from(event.decided.map(|decided| decided.to_string())),
        ],
    )?;
    Ok(())
}

/// Inserts `new` unless a session of its key exists, and returns the record
/// that is stored: the new one, or the one that was there first.
pub(crate) fn create(
    tx: &mut Tx<'_>,
    new: &SessionRow,
    event: &SessionEvent,
) -> Result<SessionRow, Failure> {
    if let Some(existing) = find(tx, &new.key)? {
        return Ok(existing);
    }
    let owner = match &new.key.identity {
        Identity::Person(person) => Some(person.to_string()),
        Identity::Account(_) => None,
    };
    tx.tables(OWNER).execute(
        "INSERT INTO sessions
            (id, workspace_id, target, environment, identity_ref, owner_principal_id, provider,
             state, epoch, home_runner_id, movable, profile, state_since, signed_in_at,
             expires_at, checked_at, last_used_at, stored_attempts, wants, attempted_with,
             signed_in_under)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19, ?20, ?21)",
        &[
            SqlValue::from(new.id.to_string()),
            SqlValue::from(new.key.workspace.to_string()),
            SqlValue::from(new.key.target.as_str()),
            SqlValue::from(new.key.environment.as_str()),
            SqlValue::from(new.key.identity.to_string()),
            SqlValue::from(owner),
            SqlValue::from(new.provider.as_str()),
            SqlValue::from(new.state.as_str()),
            number(new.epoch),
            SqlValue::from(new.home.map(|home| home.to_string())),
            SqlValue::from(new.movable),
            SqlValue::from(new.profile.as_ref().map(ProfileRef::as_str)),
            SqlValue::from(new.state_since),
            SqlValue::from(new.signed_in_at),
            SqlValue::from(new.expires_at),
            SqlValue::from(new.checked_at),
            SqlValue::from(new.last_used_at),
            SqlValue::from(new.stored_attempts),
            SqlValue::from(new.wants.map(PersonReason::as_str)),
            SqlValue::from(new.attempted_with.map(|mark| mark.to_string())),
            under(new.made_under_drafts),
        ],
    )?;
    insert_event(tx, event)?;
    Ok(new.clone())
}

/// The compare-and-set: writes `next`, its event, and the change to the
/// saved state, only if the session still has the state and the epoch
/// `seen` has. `false` when someone else changed it first; nothing is
/// written then.
pub(crate) fn advance(
    tx: &mut Tx<'_>,
    seen: &SessionRow,
    next: &SessionRow,
    event: &SessionEvent,
    saved: &Saved,
) -> Result<bool, Failure> {
    let changed = tx.tables(OWNER).execute(
        "UPDATE sessions SET provider = ?4, state = ?5, epoch = ?6, home_runner_id = ?7,
                movable = ?8, profile = ?9, state_since = ?10, signed_in_at = ?11,
                expires_at = ?12, checked_at = ?13, last_used_at = ?14, stored_attempts = ?15,
                wants = ?16, attempted_with = ?17, signed_in_under = ?18
         WHERE id = ?1 AND state = ?2 AND epoch = ?3",
        &[
            SqlValue::from(seen.id.to_string()),
            SqlValue::from(seen.state.as_str()),
            number(seen.epoch),
            SqlValue::from(next.provider.as_str()),
            SqlValue::from(next.state.as_str()),
            number(next.epoch),
            SqlValue::from(next.home.map(|home| home.to_string())),
            SqlValue::from(next.movable),
            SqlValue::from(next.profile.as_ref().map(ProfileRef::as_str)),
            SqlValue::from(next.state_since),
            SqlValue::from(next.signed_in_at),
            SqlValue::from(next.expires_at),
            SqlValue::from(next.checked_at),
            SqlValue::from(next.last_used_at),
            SqlValue::from(next.stored_attempts),
            SqlValue::from(next.wants.map(PersonReason::as_str)),
            SqlValue::from(next.attempted_with.map(|mark| mark.to_string())),
            under(next.made_under_drafts),
        ],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    let id = SqlValue::from(seen.id.to_string());
    match saved {
        Saved::Keep => {}
        Saved::Put(sealed) => {
            let now = tx.now();
            tx.tables(OWNER).execute(
                "INSERT INTO session_state (session_id, sealed, saved_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (session_id) DO UPDATE SET sealed = ?2, saved_at = ?3",
                &[id, SqlValue::from(sealed.to_bytes()), SqlValue::from(now)],
            )?;
        }
        Saved::Delete => {
            tx.tables(OWNER)
                .execute("DELETE FROM session_state WHERE session_id = ?1", &[id])?;
        }
    }
    insert_event(tx, event)?;
    Ok(true)
}

/// Notes a use, a passed check, or a restore, and appends its event, only
/// if the session's epoch is still `epoch`: the credentials the note is
/// about are still the session's. `false` when the session was renewed or
/// ended first; nothing is written then. A check that confirmed the
/// credential clears the count of unconfirmed attempts. A note changes
/// neither the state nor the epoch.
pub(crate) fn note(
    tx: &mut Tx<'_>,
    id: SessionId,
    epoch: u64,
    used_at: Option<Timestamp>,
    checked_at: Option<Timestamp>,
    confirmed: bool,
    event: &SessionEvent,
) -> Result<bool, Failure> {
    let changed = tx.tables(OWNER).execute(
        "UPDATE sessions SET last_used_at = COALESCE(?2, last_used_at),
                checked_at = COALESCE(?3, checked_at),
                stored_attempts = CASE WHEN ?4 = 1 THEN 0 ELSE stored_attempts END,
                attempted_with = CASE WHEN ?4 = 1 THEN NULL ELSE attempted_with END
         WHERE id = ?1 AND epoch = ?5",
        &[
            SqlValue::from(id.to_string()),
            SqlValue::from(used_at),
            SqlValue::from(checked_at),
            SqlValue::from(confirmed),
            number(epoch),
        ],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    insert_event(tx, event)?;
    Ok(true)
}

/// A session's saved state, sealed.
pub(crate) fn saved(tx: &mut Tx<'_>, id: SessionId) -> Result<Option<Sealed>, Failure> {
    tx.tables(OWNER)
        .query_first(
            "SELECT sealed FROM session_state WHERE session_id = ?1",
            &[SqlValue::from(id.to_string())],
        )?
        .map(|found| Sealed::from_bytes(found.bytes(0)?))
        .transpose()
}

/// A session's timeline, oldest first.
pub(crate) fn events(tx: &mut Tx<'_>, id: SessionId) -> Result<Vec<SessionEvent>, Failure> {
    tx.tables(OWNER)
        .query(
            "SELECT session_id, at, kind, state, epoch, cause, run_id, decision_id
             FROM session_events WHERE session_id = ?1 ORDER BY seq",
            &[SqlValue::from(id.to_string())],
        )?
        .iter()
        .map(|found| {
            Ok(SessionEvent {
                session: parsed(found.text(0)?, "an identifier")?,
                at: Timestamp::from_unix_millis(found.int(1)?),
                kind: EventKind::from_name(found.text(2)?)
                    .ok_or_else(|| corrupt("a kind of event"))?,
                state: parsed(found.text(3)?, "a state")?,
                epoch: u64::try_from(found.int(4)?).map_err(|_| corrupt("an epoch"))?,
                cause: found.text(5)?.to_owned(),
                run: found
                    .opt_text(6)?
                    .map(|text| parsed(text, "a run"))
                    .transpose()?,
                decided: found
                    .opt_text(7)?
                    .map(|text| parsed(text, "a decision"))
                    .transpose()?,
            })
        })
        .collect()
}

/// The newest sign-ins of a session that made it hold, newest first, at
/// most `limit` of them: each `signed_in` event, and each `renewed` event
/// of a full sign-in. Each is its cause and its time.
pub(crate) fn sign_ins(
    tx: &mut Tx<'_>,
    id: SessionId,
    limit: u32,
) -> Result<Vec<(String, Timestamp)>, Failure> {
    tx.tables(OWNER)
        .query(
            "SELECT cause, at FROM session_events
             WHERE session_id = ?1
               AND (kind = 'signed_in' OR (kind = 'renewed' AND cause = ?2))
             ORDER BY seq DESC LIMIT ?3",
            &[
                SqlValue::from(id.to_string()),
                SqlValue::from(crate::keeper::FULL_SIGN_IN_CAUSE),
                SqlValue::from(limit),
            ],
        )?
        .iter()
        .map(|found| {
            Ok((
                found.text(0)?.to_owned(),
                Timestamp::from_unix_millis(found.int(1)?),
            ))
        })
        .collect()
}

/// The mark of the credentials `references` name for one owner, as they
/// are stored now: a digest of each reference with the digest of its
/// sealed envelope, or with nothing where nothing is stored. The envelope
/// is sealed under a key the vault holds, with a random nonce, so the mark
/// changes whenever the credential is stored again and confirms no guess
/// of its value. No credential is opened.
pub(crate) fn credential_mark<'r>(
    tx: &mut Tx<'_>,
    workspace: &WorkspaceId,
    owner: &str,
    references: impl IntoIterator<Item = (&'r str, &'r CredentialRef)>,
) -> Result<Digest, Failure> {
    let mut marked = b"anything2mcp stored credential mark 1\0".to_vec();
    for (role, reference) in references {
        marked.extend_from_slice(role.as_bytes());
        marked.push(0);
        marked.extend_from_slice(reference.as_str().as_bytes());
        marked.push(0);
        match credential(tx, workspace, owner, reference)? {
            Some(sealed) => {
                marked.push(1);
                marked.extend_from_slice(Digest::of(&sealed.to_bytes()).as_bytes());
            }
            None => marked.push(0),
        }
    }
    Ok(Digest::of(&marked))
}

/// Stores a sealed credential under its reference, replacing what was there.
pub(crate) fn put_credential(
    tx: &mut Tx<'_>,
    workspace: &WorkspaceId,
    owner: &str,
    reference: &CredentialRef,
    sealed: &Sealed,
) -> Result<(), Failure> {
    let now = tx.now();
    tx.tables(OWNER).execute(
        "INSERT INTO session_credentials (workspace_id, owner, reference, sealed, stored_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (workspace_id, owner, reference) DO UPDATE SET sealed = ?4, stored_at = ?5",
        &[
            SqlValue::from(workspace.to_string()),
            SqlValue::from(owner),
            SqlValue::from(reference.as_str()),
            SqlValue::from(sealed.to_bytes()),
            SqlValue::from(now),
        ],
    )?;
    Ok(())
}

/// The sealed credential stored under a reference for one owner.
pub(crate) fn credential(
    tx: &mut Tx<'_>,
    workspace: &WorkspaceId,
    owner: &str,
    reference: &CredentialRef,
) -> Result<Option<Sealed>, Failure> {
    tx.tables(OWNER)
        .query_first(
            "SELECT sealed FROM session_credentials
             WHERE workspace_id = ?1 AND owner = ?2 AND reference = ?3",
            &[
                SqlValue::from(workspace.to_string()),
                SqlValue::from(owner),
                SqlValue::from(reference.as_str()),
            ],
        )?
        .map(|found| Sealed::from_bytes(found.bytes(0)?))
        .transpose()
}
