//! The scheduler's tables, inside one transaction of the store.
//!
//! Every function here is plain, blocking code that runs on the store's own
//! thread with a [`Tx`]. Time is the transaction's moment. The run rows are
//! the ledger's: they are reached through [`Tx::ledger`] and no other way.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::time::Duration;

use a2m_core::{
    Epoch, Failure, FailureClass, FailureCode, JobId, Mode, PrincipalId, RunId, RunState, RunnerId,
    TargetName, Timestamp, WorkspaceId,
};
use a2m_ledger::{Fence, Opened, Owner, Row, SqlValue, Tx};
use serde::{Deserialize, Serialize};

use crate::budget::{self, Take};
use crate::failure;
use crate::types::{
    Admitted, CancelOutcome, Directive, Enqueue, HealthState, HoldScope, JobEnd, JobKind, JobLease,
    JobRequirements, JobRow, JobState, LeaseRef, OsName, PROTOCOL_VERSION, Priority, Renewal,
    ResourceKey, RunnerOffer, RunnerTerms, SchedSettings, Slots, Submission,
};

/// The most keys a job may name as exclusive, and the most modes.
const MOST_KEYS: usize = 16;

// ---- small conversions ---------------------------------------------------

fn ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

pub(crate) fn plus(moment: Timestamp, duration: Duration) -> Timestamp {
    Timestamp::from_unix_millis(moment.as_unix_millis().saturating_add(ms(duration)))
}

/// The time from `from` to `to`, or none when `to` is not later.
pub(crate) fn between(from: Timestamp, to: Timestamp) -> Duration {
    let millis = to.as_unix_millis().saturating_sub(from.as_unix_millis());
    Duration::from_millis(u64::try_from(millis).unwrap_or(0))
}

fn stored<T: FromStr>(text: &str) -> Result<T, Failure> {
    text.parse()
        .map_err(|_| failure::corrupt("a value that does not have the shape of its column"))
}

fn stored_opt<T: FromStr>(text: Option<&str>) -> Result<Option<T>, Failure> {
    text.map(stored).transpose()
}

fn epoch_value(epoch: Epoch) -> Result<SqlValue, Failure> {
    i64::try_from(epoch.get())
        .map(SqlValue::Int)
        .map_err(|_| failure::corrupt("a lease number too large to keep"))
}

fn count(row: Option<Row>) -> Result<i64, Failure> {
    row.ok_or_else(|| failure::corrupt("a count that gave no row"))?
        .int(0)
}

// ---- rows ----------------------------------------------------------------

/// A job, as the scheduler reads it.
#[derive(Debug, Clone)]
pub(crate) struct Job {
    pub(crate) id: JobId,
    pub(crate) run: Option<RunId>,
    pub(crate) workspace: WorkspaceId,
    pub(crate) principal: PrincipalId,
    pub(crate) requirements: JobRequirements,
    pub(crate) priority: Priority,
    pub(crate) available_at: Timestamp,
    pub(crate) queue_until: Timestamp,
    pub(crate) state: JobState,
    pub(crate) epoch: Epoch,
    pub(crate) runner: Option<RunnerId>,
    pub(crate) lease_until: Option<Timestamp>,
    pub(crate) attempts: u32,
    pub(crate) cancel: bool,
    pub(crate) end_state: Option<RunState>,
    pub(crate) end: Option<(FailureClass, FailureCode)>,
    pub(crate) created_at: Timestamp,
}

macro_rules! select_jobs {
    ($tail:literal) => {
        concat!(
            "SELECT j.id, j.run_id, j.workspace_id, j.principal_id, j.requirements, j.priority,
                    j.available_at, j.queue_until, j.state, j.epoch, j.runner_id, j.lease_until,
                    j.attempts, j.directive, j.end_state, j.end_class, j.end_code, j.created_at
             FROM jobs j ",
            $tail
        )
    };
}

fn job_of(row: &Row) -> Result<Job, Failure> {
    let requirements: JobRequirements = serde_json::from_str(row.text(4)?)
        .map_err(|_| failure::corrupt("the requirements of a job"))?;
    let end = match (row.opt_text(15)?, row.opt_text(16)?) {
        (Some(class), Some(code)) => Some((stored(class)?, stored(code)?)),
        _ => None,
    };
    Ok(Job {
        id: stored(row.text(0)?)?,
        run: stored_opt(row.opt_text(1)?)?,
        workspace: stored(row.text(2)?)?,
        principal: stored(row.text(3)?)?,
        requirements,
        priority: Priority::from_stored(row.int(5)?)?,
        available_at: Timestamp::from_unix_millis(row.int(6)?),
        queue_until: Timestamp::from_unix_millis(row.int(7)?),
        state: JobState::parse(row.text(8)?)?,
        epoch: Epoch::new(
            u64::try_from(row.int(9)?)
                .map_err(|_| failure::corrupt("a lease number below zero"))?,
        ),
        runner: stored_opt(row.opt_text(10)?)?,
        lease_until: row.opt_int(11)?.map(Timestamp::from_unix_millis),
        attempts: u32::try_from(row.int(12)?)
            .map_err(|_| failure::corrupt("a count of attempts out of range"))?,
        cancel: row.text(13)? == "cancel",
        end_state: stored_opt(row.opt_text(14)?)?,
        end,
        created_at: Timestamp::from_unix_millis(row.int(17)?),
    })
}

fn find_job(tx: &mut Tx<'_>, job: JobId) -> Result<Job, Failure> {
    let row = tx
        .tables(Owner::Sched)
        .query_first(select_jobs!("WHERE j.id = ?1"), &[job.to_string().into()])?
        .ok_or_else(|| failure::not_found("job"))?;
    job_of(&row)
}

/// The job, for a caller that wants to see it.
pub(crate) fn job_row(tx: &mut Tx<'_>, job: JobId) -> Result<JobRow, Failure> {
    let job = find_job(tx, job)?;
    Ok(JobRow {
        id: job.id,
        run: job.run,
        kind: job.requirements.kind,
        state: job.state,
        priority: job.priority,
        epoch: job.epoch,
        runner: job.runner,
        attempts: job.attempts,
        available_at: job.available_at,
        lease_until: job.lease_until,
        cancel_requested: job.cancel,
        end_state: job.end_state,
        end: job.end,
    })
}

/// The unfinished job of a run, when it has one.
fn live_job(tx: &mut Tx<'_>, run: RunId) -> Result<Option<Job>, Failure> {
    tx.tables(Owner::Sched)
        .query_first(
            select_jobs!("WHERE j.run_id = ?1 AND j.state IN ('pending', 'leased')"),
            &[run.to_string().into()],
        )?
        .as_ref()
        .map(job_of)
        .transpose()
}

/// The identifier of the unfinished job of a run, when it has one.
pub(crate) fn unfinished_job(tx: &mut Tx<'_>, run: RunId) -> Result<Option<JobId>, Failure> {
    Ok(live_job(tx, run)?.map(|job| job.id))
}

/// How many jobs are waiting or leased. Each count has an index of its own.
pub(crate) fn unfinished_jobs(tx: &mut Tx<'_>) -> Result<u64, Failure> {
    let mut jobs = 0_u64;
    for sql in [
        "SELECT COUNT(*) FROM jobs WHERE state = 'pending'",
        "SELECT COUNT(*) FROM jobs WHERE state = 'leased'",
    ] {
        let found = tx
            .tables(Owner::Sched)
            .query_first(sql, &[])?
            .map(|row| row.int(0))
            .transpose()?
            .unwrap_or(0);
        jobs = jobs.saturating_add(u64::try_from(found).unwrap_or(0));
    }
    Ok(jobs)
}

/// What a runner offers, as the store keeps it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Capabilities {
    targets: Vec<TargetName>,
    modes: Vec<Mode>,
    os: Option<OsName>,
}

#[derive(Debug, Clone)]
struct Runner {
    workspace: WorkspaceId,
    owner: Option<PrincipalId>,
    capabilities: Capabilities,
    state: String,
}

fn runner_of(row: &Row) -> Result<Runner, Failure> {
    Ok(Runner {
        workspace: stored(row.text(0)?)?,
        owner: stored_opt(row.opt_text(1)?)?,
        capabilities: serde_json::from_str(row.text(2)?)
            .map_err(|_| failure::corrupt("the capabilities of a runner"))?,
        state: row.text(3)?.to_owned(),
    })
}

fn find_runner(tx: &mut Tx<'_>, runner: RunnerId) -> Result<Option<Runner>, Failure> {
    tx.tables(Owner::Sched)
        .query_first(
            "SELECT workspace_id, owner_principal_id, capabilities, state FROM runners WHERE id = ?1",
            &[runner.to_string().into()],
        )?
        .as_ref()
        .map(runner_of)
        .transpose()
}

/// What a runner lacks for a job, from the least to the most specific.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Missing {
    Workspace,
    Target,
    Mode,
    Os,
    Person,
}

impl Runner {
    /// Whether this runner could ever serve the job, slots and sessions
    /// aside. Modes are compared by name and nothing else.
    fn could_serve(
        &self,
        workspace: WorkspaceId,
        requirements: &JobRequirements,
    ) -> Result<(), Missing> {
        let offers = &self.capabilities;
        if self.workspace != workspace || matches!(self.state.as_str(), "revoked") {
            return Err(Missing::Workspace);
        }
        if !offers.targets.is_empty() && !offers.targets.contains(&requirements.target) {
            return Err(Missing::Target);
        }
        if !requirements.modes.is_empty()
            && !requirements
                .modes
                .iter()
                .any(|mode| offers.modes.contains(mode))
        {
            return Err(Missing::Mode);
        }
        if requirements.os.is_some() && requirements.os != offers.os {
            return Err(Missing::Os);
        }
        if self.owner.is_some() && self.owner != requirements.acts_as {
            return Err(Missing::Person);
        }
        Ok(())
    }

    /// The slot the job would take on this runner now: `Some(None)` for a
    /// job that names no mode.
    fn slot(&self, requirements: &JobRequirements, free: &Slots) -> Option<Option<Mode>> {
        if free.total == 0 {
            return None;
        }
        if requirements.modes.is_empty() {
            return Some(None);
        }
        requirements
            .modes
            .iter()
            .find(|mode| {
                self.capabilities.modes.contains(mode)
                    && free.per_mode.get(*mode).copied().unwrap_or(0) > 0
            })
            .map(|mode| Some(mode.clone()))
    }
}

/// Says what no enrolled runner offers, or nothing when one could serve.
fn missing_runner(
    tx: &mut Tx<'_>,
    workspace: WorkspaceId,
    requirements: &JobRequirements,
) -> Result<Option<String>, Failure> {
    let rows = tx.tables(Owner::Sched).query(
        "SELECT workspace_id, owner_principal_id, capabilities, state FROM runners ORDER BY id",
        &[],
    )?;
    let mut closest = Missing::Workspace;
    for row in &rows {
        match runner_of(row)?.could_serve(workspace, requirements) {
            Ok(()) => return Ok(None),
            Err(missing) => closest = closest.max(missing),
        }
    }
    Ok(Some(match closest {
        Missing::Workspace => "no runner is enrolled in this workspace".to_owned(),
        Missing::Target => format!("no runner serves the target `{}`", requirements.target),
        Missing::Mode => format!(
            "no runner offers one of the modes the job can use ({})",
            requirements
                .modes
                .iter()
                .map(Mode::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Missing::Os => format!(
            "no runner reports the operating system `{}`",
            requirements.os.as_ref().map_or("", OsName::as_str)
        ),
        Missing::Person => {
            "the device of the person the job acts as is not enrolled, and no other runner fits"
                .to_owned()
        }
    }))
}

// ---- holds on scopes -----------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Hold {
    pub(crate) scope: HoldScope,
    held_at: Timestamp,
}

pub(crate) fn scope_holds(tx: &mut Tx<'_>) -> Result<Vec<Hold>, Failure> {
    let rows = tx.tables(Owner::Sched).query(
        "SELECT scope, workspace_id, name, held_at FROM scheduler_holds ORDER BY scope, workspace_id, name",
        &[],
    )?;
    rows.iter()
        .map(|row| {
            let scope = match row.text(0)? {
                "everything" => HoldScope::Everything,
                "workspace" => HoldScope::Workspace(stored(row.text(1)?)?),
                "target" => HoldScope::Target(stored(row.text(1)?)?, stored(row.text(2)?)?),
                "caller" => HoldScope::Caller(stored(row.text(1)?)?, stored(row.text(2)?)?),
                _ => {
                    return Err(failure::corrupt(
                        "a hold on a scope that is not in the list",
                    ));
                }
            };
            Ok(Hold {
                scope,
                held_at: Timestamp::from_unix_millis(row.int(3)?),
            })
        })
        .collect()
}

fn covers(
    scope: &HoldScope,
    workspace: WorkspaceId,
    target: &TargetName,
    caller: PrincipalId,
) -> bool {
    match scope {
        HoldScope::Everything => true,
        HoldScope::Workspace(held) => *held == workspace,
        HoldScope::Target(held, name) => *held == workspace && name == target,
        HoldScope::Caller(held, principal) => *held == workspace && *principal == caller,
    }
}

fn hold_on<'h>(
    holds: &'h [Hold],
    workspace: WorkspaceId,
    target: &TargetName,
    caller: PrincipalId,
) -> Option<&'h Hold> {
    holds
        .iter()
        .find(|hold| covers(&hold.scope, workspace, target, caller))
}

/// Puts a scope on hold. A scope that is on hold already keeps its moment.
pub(crate) fn hold(tx: &mut Tx<'_>, scope: &HoldScope) -> Result<(), Failure> {
    let (word, workspace, name) = scope.stored();
    let now = tx.now();
    tx.tables(Owner::Sched).execute(
        "INSERT INTO scheduler_holds (scope, workspace_id, name, held_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (scope, workspace_id, name) DO NOTHING",
        &[word.into(), workspace.into(), name.into(), now.into()],
    )?;
    Ok(())
}

/// Releases a hold. The jobs it covered are given back the time they could
/// not be handed out, so that the hold does not use up their queue limit.
pub(crate) fn release_hold(tx: &mut Tx<'_>, scope: &HoldScope) -> Result<bool, Failure> {
    let holds = scope_holds(tx)?;
    let Some(held) = holds.iter().find(|hold| hold.scope == *scope) else {
        return Ok(false);
    };
    let (word, workspace, name) = scope.stored();
    tx.tables(Owner::Sched).execute(
        "DELETE FROM scheduler_holds WHERE scope = ?1 AND workspace_id = ?2 AND name = ?3",
        &[word.into(), workspace.into(), name.into()],
    )?;
    let lost = ms(between(held.held_at, tx.now()));
    let pending = tx.tables(Owner::Sched).query(
        select_jobs!("WHERE j.state = 'pending' ORDER BY j.seq"),
        &[],
    )?;
    for row in &pending {
        let job = job_of(row)?;
        if covers(
            scope,
            job.workspace,
            &job.requirements.target,
            job.principal,
        ) {
            tx.tables(Owner::Sched).execute(
                "UPDATE jobs SET queue_until = queue_until + ?2 WHERE id = ?1",
                &[job.id.to_string().into(), lost.into()],
            )?;
        }
    }
    Ok(true)
}

// ---- exclusive resources -------------------------------------------------

/// Every exclusive resource that is held, and the job that holds it.
fn resource_holds(tx: &mut Tx<'_>) -> Result<BTreeMap<ResourceKey, String>, Failure> {
    let rows = tx
        .tables(Owner::Sched)
        .query("SELECT resource_key, job_id FROM resource_holds", &[])?;
    rows.iter()
        .map(|row| {
            Ok((
                ResourceKey::from_stored(row.text(0)?),
                row.text(1)?.to_owned(),
            ))
        })
        .collect()
}

fn take_holds(tx: &mut Tx<'_>, job: JobId, keys: &[ResourceKey]) -> Result<(), Failure> {
    let now = tx.now();
    for key in keys {
        tx.tables(Owner::Sched).execute(
            "INSERT INTO resource_holds (resource_key, job_id, acquired_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (resource_key) DO NOTHING",
            &[key.as_str().into(), job.to_string().into(), now.into()],
        )?;
        let holder = tx.tables(Owner::Sched).query_first(
            "SELECT job_id FROM resource_holds WHERE resource_key = ?1",
            &[key.as_str().into()],
        )?;
        let ours = match &holder {
            Some(row) => row.text(0)? == job.to_string(),
            None => false,
        };
        if !ours {
            // The primary key refused: one holder for one key, whatever the
            // code above believed.
            return Err(failure::resource_in_use(Duration::from_secs(1)));
        }
    }
    Ok(())
}

fn drop_holds(tx: &mut Tx<'_>, job: JobId) -> Result<bool, Failure> {
    let dropped = tx.tables(Owner::Sched).execute(
        "DELETE FROM resource_holds WHERE job_id = ?1",
        &[job.to_string().into()],
    )?;
    Ok(dropped > 0)
}

/// What asking for exclusive resources during a run gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Acquired {
    /// The job holds every key now.
    Taken,
    /// A key is held by another job, or wanted by one that waits already.
    Blocked { lease_until: Timestamp },
}

/// Takes `keys` for the job, all or none. The ordering rule is checked
/// first: a job may ask only for keys that sort after every key it holds.
pub(crate) fn acquire(
    tx: &mut Tx<'_>,
    lease: &LeaseRef,
    keys: &[ResourceKey],
    waited_for: &BTreeSet<ResourceKey>,
) -> Result<Acquired, Failure> {
    let job = held(tx, lease, true)?;
    let holds = resource_holds(tx)?;
    let me = job.id.to_string();
    let highest = holds
        .iter()
        .filter(|(_, holder)| **holder == me)
        .map(|(key, _)| key)
        .max();
    let wanted: Vec<ResourceKey> = keys
        .iter()
        .filter(|key| holds.get(*key) != Some(&me))
        .cloned()
        .collect();
    if let Some(highest) = highest
        && wanted.iter().any(|key| key < highest)
    {
        return Err(failure::resource_order());
    }
    let blocked = wanted
        .iter()
        .any(|key| holds.contains_key(key) || waited_for.contains(key));
    if blocked {
        return Ok(Acquired::Blocked {
            lease_until: job.lease_until.unwrap_or_else(|| tx.now()),
        });
    }
    take_holds(tx, job.id, &wanted)?;
    Ok(Acquired::Taken)
}

/// Gives freed resources to those who wait, in the order they came. A
/// waiter behind another that wants the same key is not served first.
pub(crate) fn grant(
    tx: &mut Tx<'_>,
    waiters: &[(u64, JobId, Vec<ResourceKey>)],
) -> Result<Vec<u64>, Failure> {
    let mut holds = resource_holds(tx)?;
    let mut spoken_for = BTreeSet::new();
    let mut granted = Vec::new();
    for (id, job, keys) in waiters {
        let me = job.to_string();
        let free = keys.iter().all(|key| {
            holds.get(key).is_none_or(|holder| *holder == me) && !spoken_for.contains(key)
        });
        if find_job(tx, *job)?.state != JobState::Leased {
            // Its job has ended: it is given nothing and keeps no place.
            continue;
        }
        if free {
            take_holds(tx, *job, keys)?;
            for key in keys {
                holds.insert(key.clone(), me.clone());
            }
            granted.push(*id);
        } else {
            spoken_for.extend(keys.iter().cloned());
        }
    }
    Ok(granted)
}

/// Gives back some of the resources a job took during its run.
pub(crate) fn release(
    tx: &mut Tx<'_>,
    lease: &LeaseRef,
    keys: &[ResourceKey],
) -> Result<(), Failure> {
    let job = held(tx, lease, false)?;
    for key in keys {
        tx.tables(Owner::Sched).execute(
            "DELETE FROM resource_holds WHERE resource_key = ?1 AND job_id = ?2",
            &[key.as_str().into(), job.id.to_string().into()],
        )?;
    }
    Ok(())
}

// ---- admission -----------------------------------------------------------

struct NewJob {
    run: Option<RunId>,
    workspace: WorkspaceId,
    principal: PrincipalId,
    requirements: JobRequirements,
    priority: Priority,
    available_at: Timestamp,
    queue_until: Timestamp,
}

fn insert_job(tx: &mut Tx<'_>, new: &NewJob) -> Result<JobId, Failure> {
    let now = tx.now();
    let id = JobId::from_parts(now, tx.random()?)?;
    let seq = count(
        tx.tables(Owner::Sched)
            .query_first("SELECT COALESCE(MAX(seq), 0) + 1 FROM jobs", &[])?,
    )?;
    let requirements = serde_json::to_string(&new.requirements)
        .map_err(|_| failure::invalid("requirements that cannot be written down"))?;
    let session = new.requirements.session.as_ref();
    tx.tables(Owner::Sched).execute(
        "INSERT INTO jobs
            (id, seq, run_id, workspace_id, principal_id, kind, target, environment, requirements,
             session_key, home_runner_id, writes, priority, available_at, queue_until, state,
             created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'pending', ?16)",
        &[
            id.to_string().into(),
            seq.into(),
            new.run.map(|run| run.to_string()).into(),
            new.workspace.to_string().into(),
            new.principal.to_string().into(),
            new.requirements.kind.as_str().into(),
            new.requirements.target.as_str().into(),
            new.requirements.environment.as_str().into(),
            requirements.into(),
            session.map(|need| need.session.to_string()).into(),
            session
                .and_then(|need| need.home)
                .map(|home| home.to_string())
                .into(),
            new.requirements.writes.into(),
            new.priority.stored().into(),
            new.available_at.into(),
            new.queue_until.into(),
            now.into(),
        ],
    )?;
    Ok(id)
}

fn check_requirements(requirements: &JobRequirements) -> Result<(), Failure> {
    if requirements.exclusive.len() > MOST_KEYS || requirements.modes.len() > MOST_KEYS {
        return Err(failure::invalid(
            "a job names at most 16 modes and 16 exclusive resources",
        ));
    }
    if requirements.session.is_some() {
        let limit = if requirements.writes {
            requirements.session_limits.writes
        } else {
            requirements.session_limits.reads
        };
        if limit == 0 {
            return Err(failure::invalid(
                "a job on a session whose limit for that kind of work is zero",
            ));
        }
    }
    Ok(())
}

fn check_health(health: &HealthState, writes: bool) -> Result<(), Failure> {
    match health {
        HealthState::Ready => Ok(()),
        HealthState::Degraded { cause } if writes => Err(failure::engine_degraded(false, cause)),
        HealthState::Degraded { .. } => Ok(()),
        HealthState::Stopped { cause } => Err(failure::engine_degraded(true, cause)),
    }
}

/// The checks every job passes before it is queued, in the order of the
/// crate's design, and then the job row.
fn place(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
    new: &NewJob,
    queue_wait: Duration,
) -> Result<JobId, Failure> {
    let now = tx.now();
    let workspace = new.workspace.to_string();
    let waiting = count(tx.tables(Owner::Sched).query_first(
        "SELECT COUNT(*) FROM jobs
         WHERE state = 'pending' AND workspace_id = ?1 AND target = ?2 AND environment = ?3",
        &[
            workspace.clone().into(),
            new.requirements.target.as_str().into(),
            new.requirements.environment.as_str().into(),
        ],
    )?)?;
    if waiting >= i64::from(settings.queue_length) {
        // The queue's recent rate: how many of its jobs ended in the last
        // minute. One place frees up in a minute divided by that many.
        let minute = Duration::from_secs(60);
        let since = Timestamp::from_unix_millis(now.as_unix_millis().saturating_sub(ms(minute)));
        let ended = count(tx.tables(Owner::Sched).query_first(
            "SELECT COUNT(*) FROM jobs
             WHERE state = 'done' AND workspace_id = ?1 AND target = ?2 AND environment = ?3
               AND finished_at > ?4",
            &[
                workspace.into(),
                new.requirements.target.as_str().into(),
                new.requirements.environment.as_str().into(),
                since.into(),
            ],
        )?)?;
        let retry = match u32::try_from(ended) {
            Ok(ended) if ended > 0 => minute / ended,
            _ => settings.lease_length,
        };
        return Err(failure::queue_full(settings.queue_length, retry));
    }
    let of_caller = count(tx.tables(Owner::Sched).query_first(
        "SELECT COUNT(*) FROM jobs
         WHERE state = 'pending' AND workspace_id = ?1 AND principal_id = ?2",
        &[workspace.into(), new.principal.to_string().into()],
    )?)?;
    if of_caller >= i64::from(settings.caller_queue_cap) {
        return Err(failure::caller_queue_full(settings.caller_queue_cap));
    }
    if let Some(missing) = missing_runner(tx, new.workspace, &new.requirements)? {
        return Err(failure::runner_unavailable(&missing));
    }
    let new = NewJob {
        queue_until: plus(new.available_at, queue_wait),
        requirements: new.requirements.clone(),
        ..*new
    };
    insert_job(tx, &new)
}

/// Admits a run with its first job, or returns the unfinished run the same
/// call already made, or refuses. One transaction: a refusal leaves nothing.
pub(crate) fn admit(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
    submission: Submission,
) -> Result<Admitted, Failure> {
    let Submission {
        run,
        job,
        priority,
        health,
        queue_wait,
        budgets,
    } = submission;
    check_requirements(&job)?;
    if job.target != run.target || job.environment != run.environment {
        return Err(failure::invalid(
            "a job for another target or environment than its run's",
        ));
    }
    check_health(&health, job.writes)?;
    let holds = scope_holds(tx)?;
    if let Some(held) = hold_on(&holds, run.workspace, &run.target, run.principal) {
        return Err(failure::on_hold(held.scope.word()));
    }
    let (workspace, principal) = (run.workspace, run.principal);
    let opened = match tx.ledger().open_run(run)? {
        Opened::Existing(run) => return Ok(Admitted::Existing { run }),
        Opened::New(fence) => fence,
    };
    let now = tx.now();
    let new = NewJob {
        run: Some(opened.run),
        workspace,
        principal,
        requirements: job,
        priority,
        available_at: now,
        queue_until: now,
    };
    let job = place(
        tx,
        settings,
        &new,
        queue_wait.unwrap_or(settings.queue_wait),
    )?;
    if !budgets.is_empty()
        && let Take::Wait { until } = budget::take(tx, &settings.default_rate, &budgets, 1, true)?
    {
        return Err(failure::rate_limited(between(now, until)));
    }
    Ok(Admitted::New {
        run: opened.run,
        job,
    })
}

/// Queues a further job for a run that exists.
pub(crate) fn enqueue(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
    enqueue: Enqueue,
) -> Result<JobId, Failure> {
    check_requirements(&enqueue.job)?;
    check_health(&enqueue.health, enqueue.job.writes)?;
    let run = tx.ledger().run(enqueue.run)?;
    if run.state.is_final() {
        return Err(failure::invalid("a job for a run that has ended"));
    }
    if enqueue.job.target != run.target || enqueue.job.environment != run.environment {
        return Err(failure::invalid(
            "a job for another target or environment than its run's",
        ));
    }
    if live_job(tx, run.id)?.is_some() {
        return Err(failure::invalid(
            "a second job for a run that already has one waiting or leased",
        ));
    }
    let holds = scope_holds(tx)?;
    if let Some(held) = hold_on(&holds, run.workspace, &run.target, run.principal) {
        return Err(failure::on_hold(held.scope.word()));
    }
    let now = tx.now();
    let new = NewJob {
        run: Some(run.id),
        workspace: run.workspace,
        principal: run.principal,
        requirements: enqueue.job,
        priority: enqueue.priority,
        available_at: now,
        queue_until: now,
    };
    place(
        tx,
        settings,
        &new,
        enqueue.queue_wait.unwrap_or(settings.queue_wait),
    )
}

// ---- runners -------------------------------------------------------------

/// Records what a runner offers and tells it the terms.
pub(crate) fn hello(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
    offer: &RunnerOffer,
) -> Result<RunnerTerms, Failure> {
    let previous = PROTOCOL_VERSION.saturating_sub(1).max(1);
    if offer.protocol != PROTOCOL_VERSION && offer.protocol != previous {
        return Err(failure::protocol_unsupported(offer.protocol));
    }
    if offer.modes.len() > 64 || offer.targets.len() > 1024 {
        return Err(failure::invalid(
            "a runner that offers too many modes or targets",
        ));
    }
    let capabilities = serde_json::to_string(&Capabilities {
        targets: offer.targets.clone(),
        modes: offer.modes.clone(),
        os: offer.os.clone(),
    })
    .map_err(|_| failure::invalid("capabilities that cannot be written down"))?;
    let now = tx.now();
    tx.tables(Owner::Sched).execute(
        "INSERT INTO runners
            (id, workspace_id, owner_principal_id, capabilities, protocol_version, state, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'online', ?6)
         ON CONFLICT (id) DO UPDATE
         SET workspace_id = ?2, owner_principal_id = ?3, capabilities = ?4, protocol_version = ?5,
             state = 'online', last_seen_at = ?6
         WHERE state IS NOT 'revoked'",
        &[
            offer.runner.to_string().into(),
            offer.workspace.to_string().into(),
            offer.owner.map(|owner| owner.to_string()).into(),
            capabilities.into(),
            offer.protocol.into(),
            now.into(),
        ],
    )?;
    Ok(RunnerTerms {
        heartbeat: settings.heartbeat_interval,
        lease: settings.lease_length,
        protocol: PROTOCOL_VERSION,
    })
}

/// Moves a runner to `state`: `draining` or `offline`.
pub(crate) fn set_runner_state(
    tx: &mut Tx<'_>,
    runner: RunnerId,
    state: &'static str,
) -> Result<(), Failure> {
    let now = tx.now();
    let changed = tx.tables(Owner::Sched).execute(
        "UPDATE runners SET state = ?2, last_seen_at = ?3 WHERE id = ?1 AND state IS NOT 'revoked'",
        &[runner.to_string().into(), state.into(), now.into()],
    )?;
    if changed == 0 {
        return Err(failure::runner_not_enrolled());
    }
    Ok(())
}

fn seen(tx: &mut Tx<'_>, runner: RunnerId) -> Result<(), Failure> {
    let now = tx.now();
    tx.tables(Owner::Sched).execute(
        "UPDATE runners SET last_seen_at = ?2 WHERE id = ?1",
        &[runner.to_string().into(), now.into()],
    )?;
    Ok(())
}

// ---- claiming ------------------------------------------------------------

/// A lease that was just granted, and how long its job had waited.
#[derive(Debug, Clone)]
pub(crate) struct Leased {
    pub(crate) lease: JobLease,
    pub(crate) waited: Duration,
}

/// Why a waiting job cannot be handed out now, runners aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocked {
    Session,
    Resource,
}

fn blocked(
    tx: &mut Tx<'_>,
    job: &Job,
    holds: &BTreeMap<ResourceKey, String>,
    spoken_for: &BTreeSet<ResourceKey>,
) -> Result<Option<Blocked>, Failure> {
    let me = job.id.to_string();
    let resource_busy =
        job.requirements.exclusive.iter().any(|key| {
            holds.get(key).is_some_and(|holder| *holder != me) || spoken_for.contains(key)
        });
    if resource_busy {
        return Ok(Some(Blocked::Resource));
    }
    if let Some(need) = &job.requirements.session {
        let limit = if job.requirements.writes {
            job.requirements.session_limits.writes
        } else {
            job.requirements.session_limits.reads
        };
        let at_work = count(tx.tables(Owner::Sched).query_first(
            "SELECT COUNT(*) FROM jobs WHERE state = 'leased' AND session_key = ?1 AND writes = ?2",
            &[
                need.session.to_string().into(),
                job.requirements.writes.into(),
            ],
        )?)?;
        if at_work >= i64::from(limit) {
            return Ok(Some(Blocked::Session));
        }
    }
    Ok(None)
}

/// Whether the runner may take a job that needs a live session: it is the
/// session's home, or the session has no home, or the home is gone and the
/// session may move.
fn session_fits(tx: &mut Tx<'_>, job: &Job, runner: RunnerId) -> Result<bool, Failure> {
    let Some(need) = &job.requirements.session else {
        return Ok(true);
    };
    match need.home {
        None => Ok(true),
        Some(home) if home == runner => Ok(true),
        Some(home) => {
            let gone = find_runner(tx, home)?
                .is_none_or(|home| matches!(home.state.as_str(), "offline" | "revoked"));
            Ok(gone && need.movable)
        }
    }
}

/// Hands the runner the jobs it can serve and has slots for, in the order of
/// the queue: priority, then the caller served longest ago, then arrival.
///
/// `waited_for` holds the exclusive resources that jobs already running
/// wait for; a waiting job does not take those ahead of them.
pub(crate) fn claim(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
    runner_id: RunnerId,
    mut free: Slots,
    waited_for: &BTreeSet<ResourceKey>,
) -> Result<Vec<Leased>, Failure> {
    let runner = find_runner(tx, runner_id)?.ok_or_else(failure::runner_not_enrolled)?;
    if runner.state == "revoked" {
        return Err(failure::runner_not_enrolled());
    }
    seen(tx, runner_id)?;
    let mut leased = Vec::new();
    if runner.state != "online" {
        // A runner that drains takes no new work.
        return Ok(leased);
    }
    let scope_holds = scope_holds(tx)?;
    while free.total > 0 {
        let now = tx.now();
        let rows = tx.tables(Owner::Sched).query(
            select_jobs!(
                "LEFT JOIN caller_turns c
                    ON c.workspace_id = j.workspace_id AND c.principal_id = j.principal_id
                 WHERE j.state = 'pending' AND j.available_at <= ?1
                 ORDER BY j.priority, COALESCE(c.served, 0), j.seq"
            ),
            &[now.into()],
        )?;
        let holds = resource_holds(tx)?;
        let mut spoken_for = waited_for.clone();
        let mut chosen = None;
        for row in &rows {
            let job = job_of(row)?;
            let target = &job.requirements.target;
            if hold_on(&scope_holds, job.workspace, target, job.principal).is_some() {
                continue;
            }
            let waits = blocked(tx, &job, &holds, &spoken_for)?.is_some();
            let slot = match runner.could_serve(job.workspace, &job.requirements) {
                Ok(()) if !waits && session_fits(tx, &job, runner_id)? => {
                    runner.slot(&job.requirements, &free)
                }
                _ => None,
            };
            match slot {
                Some(mode) => {
                    chosen = Some((job, mode));
                    break;
                }
                // A job that stays behind keeps its place for the resources
                // it needs: a later job does not take them first.
                None => spoken_for.extend(job.requirements.exclusive.iter().cloned()),
            }
        }
        let Some((job, mode)) = chosen else {
            break;
        };
        if let Some(lease) = lease(tx, settings, &job, runner_id, mode.clone())? {
            free.total -= 1;
            if let Some(mode) = mode
                && let Some(slots) = free.per_mode.get_mut(&mode)
            {
                *slots = slots.saturating_sub(1);
            }
            leased.push(lease);
        }
    }
    Ok(leased)
}

/// Grants the lease: the run's lease number grows in the ledger, and the job
/// carries the new number, in this one transaction. A job whose run has
/// ended meanwhile is ended instead, and nothing is granted.
fn lease(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
    job: &Job,
    runner: RunnerId,
    mode: Option<Mode>,
) -> Result<Option<Leased>, Failure> {
    let now = tx.now();
    let epoch = match job.run {
        Some(run) => {
            let row = match tx.ledger().run(run) {
                Ok(row) => row,
                // Retention removed the run: its job has nothing to work on,
                // and must not stand in the way of the jobs behind it.
                Err(gone) if gone.code() == &failure::NOT_FOUND => {
                    end_pending(tx, job, None, Some((gone.class(), gone.code().clone())))?;
                    return Ok(None);
                }
                Err(other) => return Err(other),
            };
            if row.state.is_final() {
                end_pending(tx, job, Some(row.state), None)?;
                return Ok(None);
            }
            tx.ledger().new_epoch(run)?.epoch
        }
        None => job
            .epoch
            .next()
            .ok_or_else(|| failure::corrupt("a job whose lease numbers have run out"))?,
    };
    let changed = tx.tables(Owner::Sched).execute(
        "UPDATE jobs
         SET state = 'leased', epoch = ?2, runner_id = ?3, lease_mode = ?4, lease_until = ?5,
             heartbeat_at = ?6, attempts = attempts + 1
         WHERE id = ?1 AND state = 'pending'",
        &[
            job.id.to_string().into(),
            epoch_value(epoch)?,
            runner.to_string().into(),
            mode.as_ref().map(Mode::as_str).into(),
            plus(now, settings.lease_length).into(),
            now.into(),
        ],
    )?;
    if changed != 1 {
        // Another runner holds it: one job, one lease.
        return Err(failure::lease_superseded("the job is no longer waiting"));
    }
    take_holds(tx, job.id, &job.requirements.exclusive)?;
    let turn = count(
        tx.tables(Owner::Sched)
            .query_first("SELECT COALESCE(MAX(served), 0) + 1 FROM caller_turns", &[])?,
    )?;
    tx.tables(Owner::Sched).execute(
        "INSERT INTO caller_turns (workspace_id, principal_id, served) VALUES (?1, ?2, ?3)
         ON CONFLICT (workspace_id, principal_id) DO UPDATE SET served = ?3",
        &[
            job.workspace.to_string().into(),
            job.principal.to_string().into(),
            turn.into(),
        ],
    )?;
    Ok(Some(Leased {
        lease: JobLease {
            job: job.id,
            run: job.run,
            epoch,
            length: settings.lease_length,
            mode,
            attempt: job.attempts.saturating_add(1),
            requirements: job.requirements.clone(),
        },
        waited: between(job.created_at, now),
    }))
}

// ---- leases --------------------------------------------------------------

/// The job, when the lease is its current one. With `in_time`, a lease whose
/// time has run out on the scheduler's count is refused too.
fn held(tx: &mut Tx<'_>, lease: &LeaseRef, in_time: bool) -> Result<Job, Failure> {
    let job = find_job(tx, lease.job)?;
    if job.state != JobState::Leased {
        return Err(failure::lease_superseded("the job is no longer leased"));
    }
    if job.epoch != lease.epoch {
        return Err(failure::lease_superseded(
            "the call carries another lease number than the job's current one",
        ));
    }
    if in_time && job.lease_until.is_none_or(|until| tx.now() >= until) {
        return Err(failure::lease_superseded(
            "it ran out before this call arrived",
        ));
    }
    Ok(job)
}

/// The moment the lease runs out, for a call that waits inside it.
pub(crate) fn lease_until(tx: &mut Tx<'_>, lease: &LeaseRef) -> Result<Timestamp, Failure> {
    let job = held(tx, lease, true)?;
    job.lease_until
        .ok_or_else(|| failure::corrupt("a leased job with no end of lease"))
}

/// Renews the lease and says what the runner should do.
pub(crate) fn heartbeat(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
    lease: &LeaseRef,
) -> Result<Renewal, Failure> {
    let job = held(tx, lease, true)?;
    let now = tx.now();
    tx.tables(Owner::Sched).execute(
        "UPDATE jobs SET lease_until = ?2, heartbeat_at = ?3 WHERE id = ?1",
        &[
            job.id.to_string().into(),
            plus(now, settings.lease_length).into(),
            now.into(),
        ],
    )?;
    let mut draining = false;
    if let Some(runner) = job.runner {
        seen(tx, runner)?;
        draining = find_runner(tx, runner)?.is_some_and(|runner| runner.state == "draining");
    }
    let holds = scope_holds(tx)?;
    let on_hold = hold_on(
        &holds,
        job.workspace,
        &job.requirements.target,
        job.principal,
    )
    .is_some();
    let directive = if job.cancel {
        Directive::Cancel
    } else if on_hold {
        Directive::Pause
    } else if draining {
        Directive::Drain
    } else {
        Directive::Continue
    };
    Ok(Renewal {
        remaining: settings.lease_length,
        directive,
    })
}

/// What ending a job changed, for the scheduler's waiters.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Ended {
    /// Exclusive resources were freed.
    pub(crate) freed: bool,
    /// A reconcile job was queued.
    pub(crate) reconcile: Option<JobId>,
}

fn end_job(
    tx: &mut Tx<'_>,
    job: &Job,
    from: &'static str,
    to: JobState,
    end_state: Option<RunState>,
    end: Option<(FailureClass, FailureCode)>,
) -> Result<(), Failure> {
    let now = tx.now();
    let changed = tx.tables(Owner::Sched).execute(
        "UPDATE jobs
         SET state = ?3, end_state = ?4, end_class = ?5, end_code = ?6, finished_at = ?7
         WHERE id = ?1 AND state = ?2",
        &[
            job.id.to_string().into(),
            from.into(),
            to.as_str().into(),
            end_state.map(RunState::as_str).into(),
            end.as_ref().map(|(class, _)| class.as_str()).into(),
            end.as_ref().map(|(_, code)| code.as_str()).into(),
            now.into(),
        ],
    )?;
    if changed != 1 {
        return Err(failure::lease_superseded("the job changed under this call"));
    }
    Ok(())
}

/// Ends a job nobody holds, and frees what it held for later.
fn end_pending(
    tx: &mut Tx<'_>,
    job: &Job,
    end_state: Option<RunState>,
    end: Option<(FailureClass, FailureCode)>,
) -> Result<bool, Failure> {
    end_job(tx, job, "pending", JobState::Dead, end_state, end)?;
    drop_holds(tx, job.id)
}

/// Queues the job that reads back the records in doubt of `after`'s run, to
/// be handed out from `available_at`. What `after` held alone passes to it,
/// so nobody else takes those resources in between.
fn queue_reconcile(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
    after: &Job,
    available_at: Timestamp,
) -> Result<JobId, Failure> {
    let mut requirements = after.requirements.clone();
    requirements.kind = JobKind::Reconcile;
    let id = insert_job(
        tx,
        &NewJob {
            run: after.run,
            workspace: after.workspace,
            principal: after.principal,
            requirements,
            priority: after.priority,
            available_at,
            queue_until: plus(available_at, settings.queue_wait),
        },
    )?;
    tx.tables(Owner::Sched).execute(
        "UPDATE resource_holds SET job_id = ?2 WHERE job_id = ?1",
        &[after.id.to_string().into(), id.to_string().into()],
    )?;
    Ok(id)
}

/// Records how a job ended. The run's state is the ledger's: it is read
/// here and copied, never written.
pub(crate) fn finish(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
    lease: &LeaseRef,
    end: &JobEnd,
) -> Result<Ended, Failure> {
    // A result is taken until the lease is declared lost: until then nobody
    // else has been given the job.
    let job = held(tx, lease, false)?;
    let run_state = match job.run {
        Some(run) => Some(tx.ledger().run(run)?.state),
        None => None,
    };
    let mut ended = Ended::default();
    match end {
        JobEnd::Done | JobEnd::Rests => {
            end_job(tx, &job, "leased", JobState::Done, run_state, None)?;
        }
        JobEnd::Stopped {
            class,
            code,
            in_doubt,
        } => {
            end_job(
                tx,
                &job,
                "leased",
                JobState::Done,
                run_state,
                Some((*class, code.clone())),
            )?;
            if *in_doubt && run_state.is_some_and(|state| !state.is_final()) {
                let now = tx.now();
                ended.reconcile = Some(queue_reconcile(tx, settings, &job, now)?);
            }
        }
        JobEnd::LeaseEnding => {
            let now = tx.now();
            tx.tables(Owner::Sched).execute(
                "UPDATE jobs
                 SET state = 'pending', runner_id = NULL, lease_mode = NULL, lease_until = NULL,
                     available_at = ?2
                 WHERE id = ?1 AND state = 'leased'",
                &[job.id.to_string().into(), now.into()],
            )?;
        }
    }
    ended.freed = drop_holds(tx, job.id)?;
    if let Some(runner) = job.runner {
        seen(tx, runner)?;
    }
    Ok(ended)
}

/// The run states a run can be stopped from.
fn can_stop(state: RunState) -> bool {
    matches!(
        state,
        RunState::Queued
            | RunState::Validating
            | RunState::Running
            | RunState::NeedsInput
            | RunState::NeedsLogin
    )
}

/// Cancels a run: at once when no runner holds it, and through the next
/// heartbeat when one does.
pub(crate) fn cancel(tx: &mut Tx<'_>, run: RunId) -> Result<(CancelOutcome, bool), Failure> {
    let row = tx.ledger().run(run)?;
    if row.state.is_final() {
        return Ok((CancelOutcome::AlreadyFinal(row.state), false));
    }
    let mut freed = false;
    if let Some(job) = live_job(tx, run)? {
        if job.state == JobState::Leased {
            tx.tables(Owner::Sched).execute(
                "UPDATE jobs SET directive = 'cancel' WHERE id = ?1",
                &[job.id.to_string().into()],
            )?;
            return Ok((CancelOutcome::Requested, false));
        }
        freed = end_pending(tx, &job, Some(RunState::Cancelled), None)?;
    }
    // Nobody holds a lease on the run, so its current number is the
    // scheduler's to use.
    let fence = Fence {
        run,
        epoch: row.epoch,
    };
    tx.ledger().set_state(&fence, RunState::Cancelled, None)?;
    Ok((CancelOutcome::Cancelled, freed))
}

// ---- time ----------------------------------------------------------------

/// What a sweep changed.
#[derive(Debug, Clone, Default)]
pub(crate) struct Swept {
    pub(crate) lost: Vec<JobId>,
    pub(crate) reconcile: Vec<JobId>,
    pub(crate) expired: Vec<JobId>,
    pub(crate) freed: bool,
}

/// Declares lost every lease that ended without a result, and ends every
/// job no runner took within its queue limit.
pub(crate) fn sweep(tx: &mut Tx<'_>, settings: &SchedSettings) -> Result<Swept, Failure> {
    let now = tx.now();
    let mut swept = Swept::default();

    // Lost leases (spec section 223, "When a lease is lost"), each in this
    // one transaction: the job is lost; the run is fenced by a new lease
    // number that nobody holds, which sorts its locks, and stops as
    // `possible_commit`, `runner_lost`; a reconcile job waits out the
    // quarantine.
    let lost_before =
        Timestamp::from_unix_millis(now.as_unix_millis().saturating_sub(ms(settings.lost_after)));
    let rows = tx.tables(Owner::Sched).query(
        select_jobs!("WHERE j.state = 'leased' AND j.lease_until <= ?1 ORDER BY j.seq"),
        &[lost_before.into()],
    )?;
    for row in &rows {
        let job = job_of(row)?;
        let why = failure::runner_lost();
        end_job(
            tx,
            &job,
            "leased",
            JobState::Lost,
            None,
            Some((why.class(), why.code().clone())),
        )?;
        swept.lost.push(job.id);
        let mut reconcile = false;
        if let Some(run) = job.run {
            let state = tx.ledger().run(run)?.state;
            if state.is_final() {
                tx.ledger().quarantine_locks(run)?;
            } else {
                let fence = tx.ledger().new_epoch(run)?;
                if can_stop(state) {
                    tx.ledger()
                        .set_state(&fence, RunState::Stopped, Some(&why))?;
                }
                reconcile = job.requirements.writes || !tx.ledger().in_doubt(run)?.is_empty();
            }
        }
        if reconcile {
            let quarantine = settings.quarantine.max(job.requirements.max_step);
            let id = queue_reconcile(tx, settings, &job, plus(now, quarantine))?;
            swept.reconcile.push(id);
        } else {
            swept.freed |= drop_holds(tx, job.id)?;
        }
    }

    // Queue limits. A job on hold is not counted out while it is held.
    let scope_holds = scope_holds(tx)?;
    let rows = tx.tables(Owner::Sched).query(
        select_jobs!("WHERE j.state = 'pending' AND j.queue_until <= ?1 ORDER BY j.seq"),
        &[now.into()],
    )?;
    let holds = resource_holds(tx)?;
    for row in &rows {
        let job = job_of(row)?;
        if hold_on(
            &scope_holds,
            job.workspace,
            &job.requirements.target,
            job.principal,
        )
        .is_some()
        {
            continue;
        }
        let why = match blocked(tx, &job, &holds, &BTreeSet::new())? {
            Some(Blocked::Session) => failure::session_in_use(),
            Some(Blocked::Resource) => failure::resource_in_use(settings.lease_length),
            None => {
                let missing =
                    missing_runner(tx, job.workspace, &job.requirements)?.unwrap_or_else(|| {
                        "no runner that can serve it was free within its queue limit".to_owned()
                    });
                failure::runner_unavailable(&missing)
            }
        };
        swept.freed |= end_pending(tx, &job, None, Some((why.class(), why.code().clone())))?;
        swept.expired.push(job.id);
        if let Some(run) = job.run {
            let row = tx.ledger().run(run)?;
            if can_stop(row.state) {
                let fence = Fence {
                    run,
                    epoch: row.epoch,
                };
                tx.ledger()
                    .set_state(&fence, RunState::Stopped, Some(&why))?;
            }
        }
    }
    Ok(swept)
}

/// The earliest moment at which a sweep has something to do, or a waiting
/// job becomes ready to hand out.
pub(crate) fn next_due(
    tx: &mut Tx<'_>,
    settings: &SchedSettings,
) -> Result<Option<Timestamp>, Failure> {
    let now = tx.now();
    let mut due: Option<Timestamp> = None;
    let mut sooner = |moment: Timestamp| {
        due = Some(due.map_or(moment, |known| known.min(moment)));
    };
    if let Some(row) = tx.tables(Owner::Sched).query_first(
        "SELECT MIN(lease_until) FROM jobs WHERE state = 'leased'",
        &[],
    )? && let Some(until) = row.opt_int(0)?
    {
        sooner(plus(
            Timestamp::from_unix_millis(until),
            settings.lost_after,
        ));
    }
    let scope_holds = scope_holds(tx)?;
    let rows = tx.tables(Owner::Sched).query(
        select_jobs!("WHERE j.state = 'pending' ORDER BY j.seq"),
        &[],
    )?;
    for row in &rows {
        let job = job_of(row)?;
        if hold_on(
            &scope_holds,
            job.workspace,
            &job.requirements.target,
            job.principal,
        )
        .is_some()
        {
            continue;
        }
        sooner(job.queue_until);
        if job.available_at > now {
            sooner(job.available_at);
        }
    }
    Ok(due)
}
