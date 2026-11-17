//! Approvals: a person's decision to allow one exact diff.
//!
//! Recording one is deliberately not an action. The two functions that
//! record an approval or a decline are methods of [`ApprovalSurface`], and
//! of nothing else. The only `ApprovalSurface` there is comes out of
//! [`Policy::open`](crate::Policy::open), once; it cannot be cloned and has
//! no public constructor. Whoever wires the engine hands it to the approval
//! surface, so no command, tool, or route can hold it.

use a2m_core::{ApprovalId, Digest, EffectClass, Failure, RunId, RunState, Timestamp};
use a2m_ledger::{Durability, Tx, WriteClearance};

use crate::failure;
use crate::model::{Context, PrincipalRef, Question, Request, Resource, RunRef, word};
use crate::policy::Policy;
use crate::store;

word! {
    /// How an approval arrived: a channel the calling agent cannot operate.
    Channel {
        /// A prompt raised by the engine itself on one machine.
        LocalPrompt => "local_prompt",
        /// The approval page, opened from a link in the result.
        ApprovalPage => "approval_page",
        /// A card sent to the approver in the organization's chat system.
        ChatCard => "chat_card",
        /// The approval page, reached through URL-mode elicitation.
        UrlElicitation => "url_elicitation",
    }
}

word! {
    /// Which user-presence check the person passed, as it is stored in
    /// `approvals.presence_check`. Every word but `not_available` proves
    /// that a person was present.
    PresenceCheck {
        /// The operating system's own prompt, where it is not named below.
        OsPrompt => "os_prompt",
        /// The prompt of the Linux desktop's authorization agent.
        Polkit => "polkit",
        /// The macOS prompt for the user's password or fingerprint.
        LocalAuthentication => "local_authentication",
        /// The Windows prompt for the user's face, fingerprint, or PIN.
        WindowsHello => "windows_hello",
        /// The person signed in to the console or the approval page.
        SignedIn => "signed_in",
        /// This machine offers no such check.
        NotAvailable => "not_available",
    }
}

/// How old a proof of presence may be when the approval it came with is
/// recorded, and how far ahead of the store's clock: two minutes.
const PROOF_LIFETIME_MILLIS: i64 = 120_000;

impl PresenceCheck {
    /// Whether the check shows that a person was present.
    #[must_use]
    pub const fn proves_presence(self) -> bool {
        !matches!(self, Self::NotAvailable)
    }
}

/// A person's approval of one run's diff, as the approval surface hands it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approved {
    /// The run.
    pub run: RunId,
    /// The hash of the diff the person saw.
    pub diff_hash: Digest,
    /// The hash of the tool version the diff was made by.
    pub tool_version: Digest,
    /// The person who approved.
    pub approver: PrincipalRef,
    /// The channel the approval arrived by.
    pub channel: Channel,
    /// The user-presence check the person passed.
    pub presence: PresenceCheck,
    /// When the check was passed, by the store's clock.
    pub proved_at: Timestamp,
    /// The name of the prompt that answered, where the surface names it.
    pub prompt: Option<String>,
}

/// A refusal of one run's diff, or an approval window that ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declined {
    /// The run.
    pub run: RunId,
    /// The person who declined; nobody, when the window ended in silence.
    pub by: Option<PrincipalRef>,
    /// The channel.
    pub channel: Channel,
}

/// The one handle that can record an approval or a decline.
///
/// It comes out of [`Policy::open`](crate::Policy::open) and is handed to
/// the approval surface: the engine's own prompt on one machine, the
/// console's page on a server. It is not `Clone`.
#[derive(Debug)]
pub struct ApprovalSurface {
    policy: Policy,
}

impl ApprovalSurface {
    pub(crate) const fn new(policy: Policy) -> Self {
        Self { policy }
    }

    /// Records a person's approval in `tx`. The question "may this person
    /// approve this run?" is decided and recorded first; commit `tx` even
    /// when this refuses, or the record of the refusal is lost.
    ///
    /// # Errors
    ///
    /// `policy`/`approval_refused` when the approver is not a person, may
    /// not approve this run, or the run is not waiting at `needs_approval`;
    /// `policy`/`approval_stale` when the diff the person saw is not the
    /// run's; `input`/`not_found` when there is no such run; the store's
    /// failures.
    pub fn record_approval(
        &self,
        tx: &mut Tx<'_>,
        approved: &Approved,
    ) -> Result<ApprovalId, Failure> {
        let run = tx.ledger().run(approved.run)?;
        // The approval surface is what keeps an agent out: an agent cannot
        // operate it. Whatever it says of the approver, an agent identity is
        // refused here by the guardrail.
        let request = Request {
            principal: approved.approver.clone(),
            question: Question::Approve,
            resource: Resource::Run(RunRef {
                run: run.id,
                started_by: run.principal,
            }),
            context: Context {
                effect: run.effect,
                ..Context::plain(run.workspace, run.environment.clone())
            },
            run: Some(run.id),
            diff_hash: Some(approved.diff_hash),
        };
        let decision = self.policy.decide(tx, &request);
        if !decision.allowed() {
            return Err(decision.refusal.unwrap_or_else(failure::approval_refused));
        }
        if run.state != RunState::NeedsApproval {
            return Err(failure::approval_refused());
        }
        if run.diff_hash != Some(approved.diff_hash) {
            return Err(failure::approval_stale(
                "the diff the person saw is not the run's diff",
            ));
        }
        if run.target_version != approved.tool_version {
            return Err(failure::approval_stale(
                "the tool version the person saw is not the run's",
            ));
        }
        // The proof is checked again here, by the store's clock: it is of
        // now, and it proves presence wherever the approval did not come by
        // the engine's own prompt on a machine that has no check.
        let now = tx.now().as_unix_millis();
        let at = approved.proved_at.as_unix_millis();
        if at > now.saturating_add(PROOF_LIFETIME_MILLIS)
            || now.saturating_sub(at) > PROOF_LIFETIME_MILLIS
        {
            return Err(failure::approval_refused());
        }
        if !approved.presence.proves_presence() && approved.channel != Channel::LocalPrompt {
            return Err(failure::approval_refused());
        }
        let Some(set) = self.policy.current(tx) else {
            return Err(failure::evaluation_failed());
        };
        // Where an irreversible run needs two people, the second approval is
        // another person's.
        if set.layers.settings.two_person_irreversible && run.effect == EffectClass::Irreversible {
            let approver = approved.approver.id.to_string();
            let wanted = approved.diff_hash.to_string();
            let again = store::approvals_of(tx, self.policy.key(), &run.id.to_string())?
                .iter()
                .any(|held| {
                    held.approver == approver
                        && held.diff_hash.as_deref() == Some(wanted.as_str())
                        && usable(held, now)
                });
            if again {
                return Err(failure::approval_refused());
            }
        }
        let window = i64::from(set.layers.settings.approval_window_minutes) * 60_000;
        let id = ApprovalId::from_parts(tx.now(), tx.random()?)?;
        let expires_at =
            a2m_core::Timestamp::from_unix_millis(tx.now().as_unix_millis().saturating_add(window));
        store::insert_approval(
            tx,
            self.policy.key(),
            store::NewApproval {
                id: id.to_string(),
                workspace: run.workspace.to_string(),
                run: run.id.to_string(),
                diff_hash: approved.diff_hash.to_string(),
                tool_version: approved.tool_version.to_string(),
                approver: approved.approver.id.to_string(),
                channel: approved.channel.as_str(),
                presence: approved.presence.as_str(),
                expires_at,
            },
        )?;
        Ok(id)
    }

    /// Records a decline in `tx`, and revokes any approval of the run that
    /// was not used yet. The caller then ends the run as `expired`.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when there is no such run; the store's failures.
    pub fn record_decline(&self, tx: &mut Tx<'_>, declined: &Declined) -> Result<(), Failure> {
        let run = tx.ledger().run(declined.run)?;
        let id = ApprovalId::from_parts(tx.now(), tx.random()?)?;
        store::insert_decline(
            tx,
            self.policy.key(),
            id.to_string(),
            run.workspace.to_string(),
            run.id.to_string(),
            declined.by.as_ref().map(|by| by.id.to_string()),
            declined.channel.as_str(),
        )?;
        store::revoke_unused(tx, self.policy.key(), &run.id.to_string())?;
        Ok(())
    }

    /// Records a person's approval in a transaction of its own. The decision
    /// about the approver is kept whether the approval is recorded or
    /// refused.
    ///
    /// # Errors
    ///
    /// As [`ApprovalSurface::record_approval`].
    pub async fn approve(&self, approved: Approved) -> Result<ApprovalId, Failure> {
        let surface = Self::new(self.policy.clone());
        self.policy
            .store()
            .write(Durability::Synced, move |tx| {
                Ok(surface.record_approval(tx, &approved))
            })
            .await?
    }

    /// Records a decline in a transaction of its own.
    ///
    /// # Errors
    ///
    /// As [`ApprovalSurface::record_decline`].
    pub async fn decline(&self, declined: Declined) -> Result<(), Failure> {
        let surface = Self::new(self.policy.clone());
        self.policy
            .store()
            .write(Durability::Synced, move |tx| {
                surface.record_decline(tx, &declined)
            })
            .await
    }
}

impl Policy {
    /// Marks the run's approval used and gives the clearance the ledger
    /// checks when writes begin. Call it in the same `tx` as the ledger's
    /// `begin_writes`: the ledger refuses an approval that was not marked
    /// used in its own transaction, so an approval is used once.
    ///
    /// # Errors
    ///
    /// `policy`/`approval_missing` when the run has no approval;
    /// `policy`/`approval_stale` when its approval is for another diff, was
    /// used, was revoked, or has expired; the store's failures.
    pub fn consume(
        &self,
        tx: &mut Tx<'_>,
        run: RunId,
        diff_hash: &Digest,
    ) -> Result<WriteClearance, Failure> {
        let now = tx.now().as_unix_millis();
        let wanted = diff_hash.to_string();
        // At the point of action, whatever the caller asked before: a kill
        // switch that covers the run stops it, and a declined run is not
        // written on an approval recorded after the decline.
        let row = tx.ledger().run(run)?;
        let on = store::switches_on(tx, self.key())?;
        if switched_off(&on, &row) {
            return Err(failure::kill_switch_on());
        }
        if store::declined(tx, self.key(), &run.to_string())? {
            return Err(failure::approval_stale("the run was declined"));
        }
        let approvals = store::approvals_of(tx, self.key(), &run.to_string())?;
        let Some(newest) = approvals.first() else {
            return Err(failure::approval_missing());
        };
        let fits = |approval: &store::StoredApproval| {
            approval.diff_hash.as_deref() == Some(wanted.as_str()) && usable(approval, now)
        };
        // Where an irreversible run needs two people, it is cleared only by
        // two approvals of two of them, and both are used.
        let two = self.current(tx).is_some_and(|set| {
            set.layers.settings.two_person_irreversible
        }) && row.effect == EffectClass::Irreversible;
        if two {
            let mut people: Vec<&store::StoredApproval> = Vec::new();
            for approval in approvals.iter().filter(|approval| fits(approval)) {
                if people.iter().all(|seen| seen.approver != approval.approver) {
                    people.push(approval);
                }
            }
            if people.len() < 2 {
                return Err(failure::approval_missing());
            }
            for approval in &people[..2] {
                if !store::mark_used(tx, self.key(), &approval.id)? {
                    return Err(failure::approval_stale(
                        "it was used already, and an approval is used once",
                    ));
                }
            }
            let id: ApprovalId = people[0].id.parse()?;
            return Ok(WriteClearance::Approval(id));
        }
        let Some(found) = approvals.iter().find(|approval| fits(approval)) else {
            let why = if newest.diff_hash.as_deref() != Some(wanted.as_str()) {
                "it was given for another diff"
            } else if newest.used_at.is_some() {
                "it was used already, and an approval is used once"
            } else if newest.revoked_at.is_some() {
                "it was revoked or the run was declined"
            } else {
                "it has expired"
            };
            return Err(failure::approval_stale(why));
        };
        if !store::mark_used(tx, self.key(), &found.id)? {
            return Err(failure::approval_stale(
                "it was used already, and an approval is used once",
            ));
        }
        let id: ApprovalId = found.id.parse()?;
        Ok(WriteClearance::Approval(id))
    }

    /// Whether the run's diff was declined, by a person or by silence.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub fn declined(&self, tx: &mut Tx<'_>, run: RunId) -> Result<bool, Failure> {
        store::declined(tx, self.key(), &run.to_string())
    }
}

/// Whether an approval can still be used at `now`, whatever its diff.
fn usable(approval: &store::StoredApproval, now: i64) -> bool {
    approval.used_at.is_none()
        && approval.revoked_at.is_none()
        && approval.expires_at.is_some_and(|expires| expires > now)
}

/// Whether a kill switch that is on covers the run.
fn switched_off(on: &[(String, String)], run: &a2m_ledger::RunRow) -> bool {
    crate::policy::covers_parts(
        on,
        &run.workspace.to_string(),
        Some((run.target.as_str(), run.tool_name.as_str())),
    )
}
