//! The policy engine over a real store: recorded decisions, approvals joined
//! with the ledger's rule 3, kill switches, limits, published versions, and a
//! restart. What was stored is asked of the database through a connection of
//! the test's own, never only through the policy engine's read path.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_core::{
    Digest, EffectClass, Environment, Failure, FailureClass, Hazard, OperationName, PrincipalId,
    RunId, RunState, TargetName, Timestamp, WorkspaceId,
};
use a2m_ledger::{
    DATABASE_FILE_NAME, Durability, Fence, NewRun, Opened, RetentionClass, Store, StoreConfig,
    ToolKind, WriteClearance,
};
use a2m_logs::ManualClock;
use a2m_policy::{
    APPROVAL_MISSING, APPROVAL_REFUSED, APPROVAL_STALE, ActionName, ActionRef, Answer,
    ApprovalSurface, ApproveWrites, Approved, BuiltIn, Channel, Context, Decision, Declined, Grant,
    IdentityKind, KILL_SWITCH_ON, KillScope, KillSwitch, LIMIT_EXCEEDED, LayerChange, LimitCheck,
    ModuleName, Outcome, POLICY_EVALUATION_FAILED, POLICY_TESTS_FAILED, PRESENCE_REQUIRED, Policy,
    PolicyHealth, PolicyLayers, PolicyTest, PreApproval, PresenceCheck, PrincipalKind,
    PrincipalRef, PublishProof, PublishReview, Question, Quota, Request, Resource, Role,
    RoleBinding, RoleGrant, RuleKind, SECOND_ADMIN_REQUIRED, Scope, Subject, ToolRef, Toolset,
    ToolsetName,
};
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};

const START: i64 = 1_800_000_000_000;

/// Random bytes that are the same on every run: a counter, hashed.
struct Counted(AtomicU64);

impl RandomSource for Counted {
    fn fill(&self, out: &mut [u8]) -> Result<(), Failure> {
        for chunk in out.chunks_mut(Digest::LENGTH) {
            let number = self.0.fetch_add(1, Ordering::SeqCst);
            let block = Digest::of(&number.to_le_bytes());
            chunk.copy_from_slice(&block.as_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}

fn counted() -> Arc<Counted> {
    counted_from(0)
}

/// A source that starts elsewhere, as a process started later does: it must
/// not give the identifiers an earlier process gave.
fn counted_from(start: u64) -> Arc<Counted> {
    Arc::new(Counted(AtomicU64::new(start)))
}

/// A folder under the system's temporary folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-policy-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// A connection of the test's own to the store's database.
    fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.0.join(DATABASE_FILE_NAME)).unwrap()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Stand {
    folder: Folder,
    store: Store,
    clock: Arc<ManualClock>,
    policy: Policy,
    surface: ApprovalSurface,
    keys: Arc<MemoryKeyStore>,
}

async fn open_store(
    folder: &Folder,
    keys: &Arc<MemoryKeyStore>,
    clock: &Arc<ManualClock>,
    random: Arc<Counted>,
) -> Store {
    let config = StoreConfig::in_dir(folder.path())
        .with_clock(Arc::clone(clock) as Arc<dyn a2m_logs::Clock>)
        .with_random(random);
    let store = Store::open(config).await.unwrap();
    let vault = Arc::new(Vault::with_random(
        Arc::clone(keys) as Arc<dyn a2m_vault::KeyStore>,
        Arc::new(store.key_records()),
        counted(),
    ));
    store.attach(vault).await.unwrap();
    store
}

async fn stand(name: &str, built_in: BuiltIn) -> Stand {
    let folder = Folder::new(name);
    let keys = Arc::new(MemoryKeyStore::with_random(counted()));
    let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
    let store = open_store(&folder, &keys, &clock, counted()).await;
    let (policy, surface) = Policy::open(&store, built_in).await.unwrap();
    Stand {
        folder,
        store,
        clock,
        policy,
        surface,
        keys,
    }
}

async fn local(name: &str) -> Stand {
    stand(name, BuiltIn::local(person(1), workspace())).await
}

fn workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([1; 16])
}

fn person(number: u8) -> PrincipalId {
    PrincipalId::from_bytes([number; 16])
}

fn environment() -> Environment {
    Environment::new("test").unwrap()
}

fn tool_in(target: &str, name: &str) -> Resource {
    Resource::Tool(ToolRef {
        target: TargetName::new(target).unwrap(),
        module: ModuleName::new("orders").unwrap(),
        tool: OperationName::new(name).unwrap(),
    })
}

fn tool() -> Resource {
    tool_in("example", "example_order_update")
}

fn ask(by: u8, question: Question, resource: Resource, effect: EffectClass) -> Request {
    Request {
        principal: PrincipalRef::person(person(by)),
        question,
        resource,
        context: Context {
            effect,
            records: Some(2),
            ..Context::plain(workspace(), environment())
        },
        run: None,
        diff_hash: None,
    }
}

fn admin_action(by: u8) -> Request {
    ask(
        by,
        Question::PerformAction,
        Resource::Action(ActionRef {
            name: ActionName::new("killswitch.set").unwrap(),
            needs: Role::Admin,
        }),
        EffectClass::Read,
    )
}

fn diff() -> Digest {
    Digest::of(b"the diff a person saw")
}

/// Opens a write run by `caller` and brings it to `needs_approval` with a
/// stored diff hash, as the engine does after its preview.
async fn waiting_run(store: &Store, caller: u8, input: &str) -> Fence {
    waiting_run_of(store, caller, input, EffectClass::Write).await
}

async fn waiting_run_of(store: &Store, caller: u8, input: &str, effect: EffectClass) -> Fence {
    let new = NewRun {
        workspace: workspace(),
        principal: person(caller),
        client_label: "test".to_owned(),
        tool_name: OperationName::new("example_order_update").unwrap(),
        tool_kind: ToolKind::Operation,
        target: TargetName::new("example").unwrap(),
        target_version: Digest::of(b"plan"),
        environment: environment(),
        identity: None,
        effect,
        input_hash: Digest::of(input.as_bytes()),
        input: None,
        retention: RetentionClass::Long,
    };
    store
        .write(Durability::Synced, move |tx| {
            let Opened::New(fence) = tx.ledger().open_run(new)? else {
                panic!("the run exists already")
            };
            tx.ledger().set_state(&fence, RunState::Validating, None)?;
            tx.ledger().store_diff(&fence, diff(), None)?;
            tx.ledger()
                .set_state(&fence, RunState::NeedsApproval, None)?;
            Ok(fence)
        })
        .await
        .unwrap()
}

fn approved(run: RunId, by: PrincipalRef, hash: Digest) -> Approved {
    Approved {
        run,
        diff_hash: hash,
        tool_version: Digest::of(b"plan"),
        approver: by,
        channel: Channel::LocalPrompt,
        presence: PresenceCheck::OsPrompt,
        proved_at: Timestamp::from_unix_millis(START),
        prompt: Some("example_prompt".to_owned()),
    }
}

/// Consumes the run's approval and begins its writes, in one transaction.
async fn begin(stand: &Stand, fence: Fence, hash: Digest) -> Result<(), Failure> {
    let policy = stand.policy.clone();
    stand
        .store
        .write(Durability::Synced, move |tx| {
            let clearance = policy.consume(tx, fence.run, &hash)?;
            tx.ledger().begin_writes(&fence, clearance)
        })
        .await
}

async fn run_state(store: &Store, run: RunId) -> RunState {
    store
        .read(move |tx| Ok(tx.ledger().run(run)?.state))
        .await
        .unwrap()
}

async fn health(store: &Store, policy: &Policy) -> PolicyHealth {
    let policy = policy.clone();
    store.read(move |tx| Ok(policy.health(tx))).await.unwrap()
}

fn count(raw: &rusqlite::Connection, sql: &str) -> i64 {
    raw.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn code(result: Result<impl std::fmt::Debug, Failure>) -> a2m_core::FailureCode {
    let failure = result.unwrap_err();
    assert_eq!(failure.class(), FailureClass::Policy, "{failure}");
    failure.code().clone()
}

#[tokio::test]
async fn every_decision_is_recorded_and_a_write_waits_for_a_person() {
    let stand = local("decide").await;

    let call = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, tool(), EffectClass::Write))
        .await;
    assert_eq!(call.outcome(), Outcome::Allow);

    let write = stand
        .policy
        .decide_alone(ask(
            1,
            Question::WriteWithoutPerson,
            tool(),
            EffectClass::Write,
        ))
        .await;
    assert_eq!(write.outcome(), Outcome::NeedsApproval);
    assert_eq!(write.clearance(), None);

    let stranger = stand
        .policy
        .decide_alone(ask(2, Question::UseTool, tool(), EffectClass::Read))
        .await;
    let Outcome::Refused(refusal) = stranger.outcome() else {
        panic!("a stranger was not refused")
    };
    assert_eq!(refusal.code(), &a2m_policy::NOT_GRANTED);

    // The database holds all three, allow and deny alike, with the version.
    let raw = stand.folder.raw();
    let PolicyHealth::Ok { version } = health(&stand.store, &stand.policy).await else {
        panic!("no valid set")
    };
    let rows: Vec<(String, String, i64, String, Option<String>)> = raw
        .prepare("SELECT id, question, allowed, policy_version, refusal_code FROM decisions ORDER BY rowid")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let version = version.to_string();
    assert_eq!(
        rows,
        [
            (
                call.id.to_string(),
                "use_tool".to_owned(),
                1,
                version.clone(),
                None
            ),
            (
                write.id.to_string(),
                "write_without_person".to_owned(),
                0,
                version.clone(),
                None
            ),
            (
                stranger.id.to_string(),
                "use_tool".to_owned(),
                0,
                version,
                Some("not_granted".to_owned())
            ),
        ]
    );

    // Asking what the answer would be records nothing.
    let policy = stand.policy.clone();
    let would = stand
        .store
        .read(
            move |tx| Ok(policy.would(tx, &ask(1, Question::UseTool, tool(), EffectClass::Write))),
        )
        .await
        .unwrap();
    assert_eq!(would, Answer::Allow);
    assert_eq!(count(&raw, "SELECT COUNT(*) FROM decisions"), 3);
}

#[tokio::test]
async fn only_the_decision_about_a_read_is_recorded_without_a_sync_and_it_is_recorded() {
    let read = ask(1, Question::UseTool, tool(), EffectClass::Read);
    let write = ask(1, Question::UseTool, tool(), EffectClass::Write);
    let bound = Request {
        diff_hash: Some(diff()),
        ..read.clone()
    };
    // A relaxed commit is asked for and given only for a read bound to no
    // diff; a decision a write rests on is synced whatever is asked.
    assert_eq!(
        Policy::durability_for(&read, Durability::Relaxed),
        Durability::Relaxed
    );
    assert_eq!(
        Policy::durability_for(&read, Durability::Synced),
        Durability::Synced
    );
    assert_eq!(
        Policy::durability_for(&write, Durability::Relaxed),
        Durability::Synced
    );
    assert_eq!(
        Policy::durability_for(&bound, Durability::Relaxed),
        Durability::Synced
    );
    // An administrator's action is synced even where its effect is
    // labelled a read: no caller can ask it relaxed.
    assert_eq!(
        Policy::durability_for(&admin_action(1), Durability::Relaxed),
        Durability::Synced
    );

    // Relaxed or not, the decision commits before its answer is given.
    let stand = local("decide-relaxed").await;
    let decided = stand
        .policy
        .decide_alone_as(read, Durability::Relaxed)
        .await;
    assert_eq!(decided.outcome(), Outcome::Allow);
    let raw = stand.folder.raw();
    assert_eq!(count(&raw, "SELECT COUNT(*) FROM decisions"), 1);
}

#[tokio::test]
async fn an_approval_is_bound_to_its_diff_and_used_once() {
    let stand = local("approval").await;
    let fence = waiting_run(&stand.store, 1, "a").await;

    // No approval yet: the write phase cannot begin.
    assert_eq!(code(begin(&stand, fence, diff()).await), APPROVAL_MISSING);

    // An approval of another diff than the run's is not recorded.
    let other = Digest::of(b"another diff");
    let wrong = stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), other))
        .await;
    assert_eq!(code(wrong), APPROVAL_STALE);

    let id = stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), diff()))
        .await
        .unwrap();

    // The database holds it: bound to the run, the diff, the person, and an expiry.
    let raw = stand.folder.raw();
    let row: (String, String, String, String, i64, Option<i64>) = raw
        .query_row(
            "SELECT run_id, diff_hash, approver_id, channel, expires_at, used_at FROM approvals WHERE id = ?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            fence.run.to_string(),
            diff().to_string(),
            person(1).to_string(),
            "local_prompt".to_owned(),
            START + 15 * 60_000,
            None
        )
    );

    // It cannot be used for another diff.
    assert_eq!(code(begin(&stand, fence, other).await), APPROVAL_STALE);
    assert_eq!(
        run_state(&stand.store, fence.run).await,
        RunState::NeedsApproval
    );

    // Used in the transaction that begins the writes: the run is running.
    begin(&stand, fence, diff()).await.unwrap();
    assert_eq!(run_state(&stand.store, fence.run).await, RunState::Running);
    assert_eq!(
        count(
            &raw,
            "SELECT COUNT(*) FROM approvals WHERE used_at IS NOT NULL"
        ),
        1
    );

    // Used once: a second use is refused.
    let policy = stand.policy.clone();
    let again = stand
        .store
        .write(Durability::Synced, move |tx| {
            policy.consume(tx, fence.run, &diff())
        })
        .await;
    assert_eq!(code(again), APPROVAL_STALE);
}

#[tokio::test]
async fn an_approval_consumed_in_an_earlier_transaction_does_not_begin_writes() {
    let stand = local("apart").await;
    let fence = waiting_run(&stand.store, 1, "a").await;
    stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), diff()))
        .await
        .unwrap();
    let policy = stand.policy.clone();
    let clearance = stand
        .store
        .write(Durability::Synced, move |tx| {
            policy.consume(tx, fence.run, &diff())
        })
        .await
        .unwrap();
    let later = stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.ledger().begin_writes(&fence, clearance)
        })
        .await;
    assert_eq!(code(later), APPROVAL_MISSING);
    assert_eq!(
        run_state(&stand.store, fence.run).await,
        RunState::NeedsApproval
    );
}

#[tokio::test]
async fn an_expired_or_declined_approval_is_refused() {
    let stand = local("expiry").await;
    let fence = waiting_run(&stand.store, 1, "a").await;
    stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), diff()))
        .await
        .unwrap();
    // The approval window of the local set is 15 minutes.
    stand.clock.advance(Duration::from_secs(15 * 60));
    assert_eq!(code(begin(&stand, fence, diff()).await), APPROVAL_STALE);

    // A fresh approval, then a decline: the decline revokes it.
    let now = Timestamp::from_unix_millis(START + 15 * 60 * 1000);
    let fresh = stand
        .surface
        .approve(Approved {
            proved_at: now,
            ..approved(fence.run, PrincipalRef::person(person(1)), diff())
        })
        .await
        .unwrap();
    stand
        .surface
        .decline(Declined {
            run: fence.run,
            by: None,
            channel: Channel::LocalPrompt,
        })
        .await
        .unwrap();
    assert_eq!(code(begin(&stand, fence, diff()).await), APPROVAL_STALE);
    // An approval recorded after the decline is not used either: `consume`
    // asks whether the run was declined at the point of action (decision
    // 0145).
    stand
        .surface
        .approve(Approved {
            proved_at: now,
            ..approved(fence.run, PrincipalRef::person(person(1)), diff())
        })
        .await
        .unwrap();
    assert_eq!(code(begin(&stand, fence, diff()).await), APPROVAL_STALE);
    let raw = stand.folder.raw();
    let revoked: Option<i64> = raw
        .query_row(
            "SELECT revoked_at FROM approvals WHERE id = ?1",
            [fresh.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(revoked.is_some());
    assert_eq!(
        count(
            &raw,
            "SELECT COUNT(*) FROM approval_declines WHERE declined_by IS NULL"
        ),
        1,
        "silence is a decline by nobody"
    );
    let policy = stand.policy.clone();
    assert!(
        stand
            .store
            .read(move |tx| policy.declined(tx, fence.run))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn only_a_person_who_may_approve_this_run_can() {
    let stand = local("approver").await;
    let fence = waiting_run(&stand.store, 1, "a").await;
    let raw = stand.folder.raw();

    // An agent identity, a program, and a person without the role.
    let agent = PrincipalRef::agent(person(1));
    assert_eq!(
        code(
            stand
                .surface
                .approve(approved(fence.run, agent, diff()))
                .await
        ),
        APPROVAL_REFUSED
    );
    let stranger = PrincipalRef::person(person(2));
    assert_eq!(
        code(
            stand
                .surface
                .approve(approved(fence.run, stranger, diff()))
                .await
        ),
        APPROVAL_REFUSED
    );
    assert_eq!(count(&raw, "SELECT COUNT(*) FROM approvals"), 0);
    // Each refusal is a recorded decision.
    assert_eq!(
        count(
            &raw,
            "SELECT COUNT(*) FROM decisions WHERE question = 'approve' AND allowed = 0"
        ),
        2
    );

    // A run that is not waiting takes no approval.
    stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Cancelled, None)
        })
        .await
        .unwrap();
    let late = stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), diff()))
        .await;
    assert_eq!(code(late), APPROVAL_REFUSED);
    assert_eq!(count(&raw, "SELECT COUNT(*) FROM approvals"), 0);
}

fn server_layers() -> PolicyLayers {
    let mut layers = PolicyLayers {
        roles: [1, 2]
            .into_iter()
            .map(|number| RoleBinding {
                who: Subject::Principal(person(number)),
                role: Role::Approver,
            })
            .collect(),
        grants: vec![Grant {
            who: Subject::Principal(person(1)),
            scope: Scope::Target(TargetName::new("example").unwrap()),
            max_effect: EffectClass::Write,
            identity_mode: IdentityKind::PerUserSession,
        }],
        ..PolicyLayers::default()
    };
    layers.settings.approve_writes = ApproveWrites::NamedApprover;
    layers
}

async fn publish(
    stand: &Stand,
    layers: PolicyLayers,
    proof: PublishProof,
) -> Result<Digest, Failure> {
    let policy = stand.policy.clone();
    stand
        .store
        .write(Durability::Synced, move |tx| {
            let candidate = policy.compile(&layers)?;
            policy.publish(tx, &candidate, &PrincipalRef::person(person(9)), proof)
        })
        .await
}

#[tokio::test]
async fn where_an_approver_is_named_the_caller_cannot_approve_their_own_run() {
    let stand = stand("named", BuiltIn::server()).await;
    // A server's first set denies everything.
    let before = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, tool(), EffectClass::Read))
        .await;
    assert_eq!(before.answer, Answer::Deny);

    let version = publish(&stand, server_layers(), PublishProof::default())
        .await
        .unwrap();
    let after = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, tool(), EffectClass::Read))
        .await;
    assert_eq!(after.answer, Answer::Allow);
    assert_eq!(
        after.version,
        Some(version),
        "the new version is in force at once"
    );

    let fence = waiting_run(&stand.store, 1, "a").await;
    let own = stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), diff()))
        .await;
    assert_eq!(code(own), APPROVAL_REFUSED);
    stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(2)), diff()))
        .await
        .unwrap();
    begin(&stand, fence, diff()).await.unwrap();
    assert_eq!(run_state(&stand.store, fence.run).await, RunState::Running);
}

#[tokio::test]
async fn a_pre_approval_needs_a_person_to_publish_and_then_clears_one_run_and_diff() {
    let stand = local("pre").await;
    let mut layers = BuiltIn::local(person(1), workspace()).layers().clone();
    layers.pre_approvals.push(PreApproval {
        name: "small_test_updates".to_owned(),
        who: Subject::Principal(person(1)),
        scope: Scope::Pattern("example_order_*".to_owned()),
        max_records: 5,
        condition: Some("environment == \"test\"".to_owned()),
    });

    // A rule that lets writes through is a loosening: only a person, present.
    let unproved = publish(&stand, layers.clone(), PublishProof::default()).await;
    assert_eq!(code(unproved), PRESENCE_REQUIRED);
    let unavailable = PublishProof {
        presence: Some(PresenceCheck::NotAvailable),
    };
    assert_eq!(
        code(publish(&stand, layers.clone(), unavailable).await),
        PRESENCE_REQUIRED
    );
    let present = PublishProof {
        presence: Some(PresenceCheck::OsPrompt),
    };
    let version = publish(&stand, layers, present).await.unwrap();
    assert_eq!(
        health(&stand.store, &stand.policy).await,
        PolicyHealth::Ok { version }
    );

    let fence = waiting_run(&stand.store, 1, "a").await;
    let mut request = ask(1, Question::WriteWithoutPerson, tool(), EffectClass::Write);
    request.run = Some(fence.run);
    request.diff_hash = Some(diff());

    // A hazard still needs a person, whatever the rule says.
    let mut hazardous = request.clone();
    hazardous.context.hazard = Hazard::SendsOutside;
    let refused = stand.policy.decide_alone(hazardous).await;
    assert_eq!(refused.outcome(), Outcome::NeedsApproval);
    assert_eq!(refused.clearance(), None);

    // A decision made in an earlier transaction does not begin writes: a
    // kill switch or a newer set may have come between (decision 0145).
    let earlier: Decision = stand.policy.decide_alone(request.clone()).await;
    assert_eq!(earlier.outcome(), Outcome::Allow);
    let stale = earlier.clearance().unwrap();
    let refused = stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.ledger().begin_writes(&fence, stale)
        })
        .await;
    assert_eq!(code(refused), APPROVAL_MISSING);

    // Decided and begun in one transaction, it clears the run.
    let policy = stand.policy.clone();
    let clearance = stand
        .store
        .write(Durability::Synced, move |tx| {
            let decision = policy.decide(tx, &request);
            assert_eq!(decision.outcome(), Outcome::Allow);
            let clearance = decision.clearance().unwrap();
            assert_eq!(clearance, WriteClearance::PreApproved(decision.id));
            tx.ledger().begin_writes(&fence, clearance)?;
            Ok(clearance)
        })
        .await
        .unwrap();
    assert_eq!(run_state(&stand.store, fence.run).await, RunState::Running);

    // The same decision does not clear another run.
    let other = waiting_run(&stand.store, 1, "b").await;
    let reused = stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.ledger().begin_writes(&other, clearance)
        })
        .await;
    assert_eq!(code(reused), APPROVAL_MISSING);
}

#[tokio::test]
async fn a_version_that_fails_its_test_table_is_not_published() {
    let stand = local("tests").await;
    let PolicyHealth::Ok { version: before } = health(&stand.store, &stand.policy).await else {
        panic!("no valid set")
    };
    let mut layers = BuiltIn::local(person(1), workspace()).layers().clone();
    layers.settings.max_records_per_run = 10;
    layers.tests.push(PolicyTest {
        name: "a stranger writes".to_owned(),
        request: ask(2, Question::UseTool, tool(), EffectClass::Write),
        expect: Answer::Allow,
    });
    let failure = publish(&stand, layers.clone(), PublishProof::default())
        .await
        .unwrap_err();
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code(), &POLICY_TESTS_FAILED);
    assert!(failure.message().contains("a stranger writes"));
    let raw = stand.folder.raw();
    assert_eq!(count(&raw, "SELECT COUNT(*) FROM policy_versions"), 1);

    // With the test corrected, the tighter set is published with no proof.
    layers.tests.last_mut().unwrap().expect = Answer::Deny;
    let version = publish(&stand, layers, PublishProof::default())
        .await
        .unwrap();
    assert_ne!(version, before);
    assert_eq!(count(&raw, "SELECT COUNT(*) FROM policy_versions"), 2);
    // A published version never changes.
    assert!(
        raw.execute("UPDATE policy_versions SET layers = '{}'", [])
            .is_err()
    );
}

fn switch(scope: KillScope, on: bool, presence: Option<PresenceCheck>) -> KillSwitch {
    KillSwitch {
        scope,
        on,
        by: person(1),
        presence,
    }
}

async fn set_switch(stand: &Stand, switch: KillSwitch) -> Result<u64, Failure> {
    let policy = stand.policy.clone();
    stand
        .store
        .write(Durability::Synced, move |tx| {
            policy.set_kill_switch(tx, &switch)
        })
        .await
}

#[tokio::test]
async fn a_kill_switch_refuses_everything_it_covers() {
    let stand = local("kill").await;
    let target = KillScope::Target(TargetName::new("example").unwrap());
    let epoch = set_switch(&stand, switch(target.clone(), true, None))
        .await
        .unwrap();
    assert_eq!(epoch, 1);

    let on_target = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, tool(), EffectClass::Read))
        .await;
    let Outcome::Refused(refusal) = on_target.outcome() else {
        panic!("a covered call was not refused")
    };
    assert_eq!(refusal.code(), &KILL_SWITCH_ON);
    // A write that would wait for a person is refused too, not left waiting.
    let write = stand
        .policy
        .decide_alone(ask(
            1,
            Question::WriteWithoutPerson,
            tool(),
            EffectClass::Write,
        ))
        .await;
    assert!(matches!(write.outcome(), Outcome::Refused(_)));

    let elsewhere = tool_in("example_two", "example_two_order_get");
    let other = stand
        .policy
        .decide_alone(ask(
            1,
            Question::UseTool,
            elsewhere.clone(),
            EffectClass::Read,
        ))
        .await;
    assert_eq!(other.answer, Answer::Allow);

    // One operation, then everything.
    let operation = KillScope::Operation(OperationName::new("example_two_order_get").unwrap());
    set_switch(&stand, switch(operation, true, None))
        .await
        .unwrap();
    let stopped = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, elsewhere, EffectClass::Read))
        .await;
    assert_eq!(stopped.answer, Answer::Deny);
    let third = tool_in("example_three", "example_three_order_get");
    let free = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, third.clone(), EffectClass::Read))
        .await;
    assert_eq!(free.answer, Answer::Allow);
    set_switch(&stand, switch(KillScope::Everything, true, None))
        .await
        .unwrap();
    let all = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, third.clone(), EffectClass::Read))
        .await;
    assert_eq!(all.answer, Answer::Deny);

    // Administering stays possible, or no switch could be released.
    let admin = stand.policy.decide_alone(admin_action(1)).await;
    assert_eq!(admin.answer, Answer::Allow);

    // On one machine a release needs a person, present.
    let unproved = set_switch(&stand, switch(KillScope::Everything, false, None)).await;
    assert_eq!(code(unproved), PRESENCE_REQUIRED);
    let epoch = set_switch(
        &stand,
        switch(KillScope::Everything, false, Some(PresenceCheck::OsPrompt)),
    )
    .await
    .unwrap();
    assert_eq!(epoch, 4);
    let policy = stand.policy.clone();
    assert_eq!(
        stand
            .store
            .read(move |tx| policy.kill_epoch(tx))
            .await
            .unwrap(),
        4
    );
    let released = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, third, EffectClass::Read))
        .await;
    assert_eq!(released.answer, Answer::Allow);

    // The database holds the switches as set.
    let raw = stand.folder.raw();
    assert_eq!(
        count(&raw, "SELECT COUNT(*) FROM kill_switches WHERE is_on = 1"),
        2
    );
}

async fn limits(stand: &Stand, records: Option<u32>, count_call: bool) -> Result<(), Failure> {
    let policy = stand.policy.clone();
    let check = LimitCheck {
        principal: PrincipalRef::person(person(1)),
        workspace: workspace(),
        records,
        count_call,
    };
    stand
        .store
        .write(Durability::Synced, move |tx| {
            policy.check_limits(tx, &check)
        })
        .await
}

#[tokio::test]
async fn records_per_run_and_quotas_are_limits() {
    let stand = local("limits").await;
    limits(&stand, Some(50), false).await.unwrap();
    assert_eq!(code(limits(&stand, Some(51), false).await), LIMIT_EXCEEDED);

    // A quota of two calls an hour; adding one loosens nothing.
    let mut layers = BuiltIn::local(person(1), workspace()).layers().clone();
    layers.quotas.push(Quota {
        who: Subject::Anyone,
        calls: 2,
        period_minutes: 60,
    });
    publish(&stand, layers, PublishProof::default())
        .await
        .unwrap();
    limits(&stand, None, true).await.unwrap();
    limits(&stand, None, true).await.unwrap();
    assert_eq!(code(limits(&stand, None, true).await), LIMIT_EXCEEDED);
    // The refused call was not counted: its transaction was taken back.
    let raw = stand.folder.raw();
    assert_eq!(count(&raw, "SELECT calls FROM quota_counters"), 2);
    // The next period starts a new count.
    stand.clock.advance(Duration::from_secs(60 * 60));
    limits(&stand, None, true).await.unwrap();
    assert_eq!(count(&raw, "SELECT calls FROM quota_counters"), 1);
}

#[tokio::test]
async fn decisions_versions_and_switches_survive_a_restart_and_can_be_explained() {
    let Stand {
        folder,
        store,
        clock,
        policy,
        surface,
        keys,
    } = local("restart").await;
    let mut layers = BuiltIn::local(person(1), workspace()).layers().clone();
    layers.settings.max_records_per_run = 10;
    let candidate = policy.compile(&layers).unwrap();
    let publisher = policy.clone();
    let version = store
        .write(Durability::Synced, move |tx| {
            publisher.publish(
                tx,
                &candidate,
                &PrincipalRef::person(person(1)),
                PublishProof::default(),
            )
        })
        .await
        .unwrap();
    let denied = policy
        .decide_alone(ask(2, Question::UseTool, tool(), EffectClass::Read))
        .await;
    let waits = policy
        .decide_alone(ask(
            1,
            Question::WriteWithoutPerson,
            tool(),
            EffectClass::Write,
        ))
        .await;
    let stopper = policy.clone();
    let target = KillScope::Target(TargetName::new("example").unwrap());
    store
        .write(Durability::Synced, move |tx| {
            stopper.set_kill_switch(tx, &switch(target, true, None))
        })
        .await
        .unwrap();
    drop((policy, surface, store));

    let store = open_store(&folder, &keys, &clock, counted_from(1_000_000)).await;
    let (policy, _surface) = Policy::open(&store, BuiltIn::local(person(1), workspace()))
        .await
        .unwrap();
    // The published version, not the built-in one, is in force again.
    assert_eq!(health(&store, &policy).await, PolicyHealth::Ok { version });
    let raw = folder.raw();
    assert_eq!(count(&raw, "SELECT COUNT(*) FROM decisions"), 2);
    assert_eq!(count(&raw, "SELECT COUNT(*) FROM policy_versions"), 2);

    let explainer = policy.clone();
    let (why_denied, why_waits) = store
        .read(move |tx| {
            Ok((
                explainer.explain(tx, denied.id, workspace())?,
                explainer.explain(tx, waits.id, workspace())?,
            ))
        })
        .await
        .unwrap();
    assert_eq!(why_denied.answer, Answer::Deny);
    assert_eq!(why_denied.refusal_code.as_deref(), Some("not_granted"));
    assert_eq!(why_denied.rules[0].id, "default.deny");
    assert!(
        why_denied.summary.contains("no rule permitted this"),
        "{}",
        why_denied.summary
    );
    assert!(why_denied.summary.contains(&version.to_string()));
    assert_eq!(why_denied.resource, "tool:example_order_update");
    assert!(
        why_waits.summary.contains("waits for a person's approval"),
        "{}",
        why_waits.summary
    );

    // The kill switch is still on.
    let stopped = policy
        .decide_alone(ask(1, Question::UseTool, tool(), EffectClass::Read))
        .await;
    assert_eq!(stopped.refusal.unwrap().code(), &KILL_SWITCH_ON);
    let explainer = policy.clone();
    let why = store
        .read(move |tx| explainer.explain(tx, stopped.id, workspace()))
        .await
        .unwrap();
    assert!(why.summary.contains("kill switch"), "{}", why.summary);

    // An identifier that names no decision is an input failure.
    let unknown = a2m_core::DecisionId::from_bytes([5; 16]);
    let missing = store
        .read(move |tx| policy.explain(tx, unknown, workspace()))
        .await
        .unwrap_err();
    assert_eq!(missing.class(), FailureClass::Input);
}

#[tokio::test]
async fn a_new_version_that_fails_to_load_leaves_the_previous_one_in_force() {
    // Section 224: a new policy version that fails to load is not used; the
    // previous version stays in force and goes on deciding.
    let stand = local("previous").await;
    let PolicyHealth::Ok { version: before } = health(&stand.store, &stand.policy).await else {
        panic!("no valid set")
    };
    let raw = stand.folder.raw();
    let versions = count(&raw, "SELECT COUNT(*) FROM policy_versions");

    // A rule whose condition does not compile, and a rule whose test fails:
    // neither set is stored.
    let mut broken = BuiltIn::local(person(1), workspace()).layers().clone();
    broken.pre_approvals.push(PreApproval {
        name: "does_not_compile".to_owned(),
        who: Subject::Principal(person(1)),
        scope: Scope::Pattern("example_order_*".to_owned()),
        max_records: 5,
        condition: Some("environment == (".to_owned()),
    });
    let present = PublishProof {
        presence: Some(PresenceCheck::OsPrompt),
    };
    let refused = publish(&stand, broken, present).await.unwrap_err();
    assert_eq!(refused.class(), FailureClass::Input, "{refused:?}");
    let mut failing = BuiltIn::local(person(1), workspace()).layers().clone();
    failing.grants.clear();
    failing.tests.push(PolicyTest {
        name: "the person reads".to_owned(),
        request: ask(1, Question::UseTool, tool(), EffectClass::Read),
        expect: Answer::Allow,
    });
    let refused = publish(&stand, failing, PublishProof::default())
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &POLICY_TESTS_FAILED);
    assert_eq!(
        count(&raw, "SELECT COUNT(*) FROM policy_versions"),
        versions
    );

    // The previous version decides, as before: the person reads, a stranger
    // does not.
    assert_eq!(
        health(&stand.store, &stand.policy).await,
        PolicyHealth::Ok { version: before }
    );
    let read = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, tool(), EffectClass::Read))
        .await;
    assert_eq!(read.answer, Answer::Allow);
    assert_eq!(read.version, Some(before));
    let stranger = stand
        .policy
        .decide_alone(ask(2, Question::UseTool, tool(), EffectClass::Read))
        .await;
    assert_eq!(stranger.answer, Answer::Deny);
    assert_eq!(stranger.version, Some(before));
}

#[tokio::test]
async fn with_no_valid_set_every_decision_is_a_recorded_denial() {
    let stand = local("invalid").await;
    // Another program writes a newest version that cannot be read.
    let raw = stand.folder.raw();
    raw.execute(
        "INSERT INTO policy_versions (seq, version, format, guardrails, layers, tests, published_at)
         VALUES (99, 'not-a-version', 1, '', '{', '[]', 0)",
        [],
    )
    .unwrap();
    for request in [
        ask(1, Question::UseTool, tool(), EffectClass::Read),
        admin_action(1),
    ] {
        let decision = stand.policy.decide_alone(request).await;
        assert_eq!(decision.answer, Answer::Deny);
        assert_eq!(decision.version, None);
        assert_eq!(decision.refusal.unwrap().code(), &POLICY_EVALUATION_FAILED);
    }
    assert_eq!(
        health(&stand.store, &stand.policy).await,
        PolicyHealth::NoValidSet
    );
    assert_eq!(
        count(
            &raw,
            "SELECT COUNT(*) FROM decisions WHERE allowed = 0 AND policy_version IS NULL"
        ),
        2
    );
    assert_eq!(
        code(limits(&stand, Some(1), false).await),
        POLICY_EVALUATION_FAILED
    );
}

/// Compiles only while `ApprovalSurface` is not `Clone`: were it `Clone`, the
/// two blanket implementations would both apply and the call would be
/// ambiguous. With no public constructor, the handle `Policy::open` returns
/// is the only one there is.
#[test]
fn the_approval_handle_cannot_be_copied() {
    trait NotClone<Marker> {
        fn holds() {}
    }
    impl<T> NotClone<()> for T {}
    impl<T: Clone> NotClone<u8> for T {}
    <ApprovalSurface as NotClone<_>>::holds();
}

// ---- decision 0145: what the audit asked of the policy engine ----

#[tokio::test]
async fn the_policy_engine_opens_once_per_store() {
    let stand = local("once").await;
    // A second opening would hand out a second approval handle and could
    // put another built-in set in force: it is refused.
    let again = Policy::open(&stand.store, BuiltIn::local(person(5), workspace())).await;
    let Err(refused) = again else {
        panic!("a second policy engine opened on one store")
    };
    assert_eq!(refused.code().as_str(), "table_not_owned");
    // The first one still decides.
    let decision = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, tool(), EffectClass::Read))
        .await;
    assert_eq!(decision.answer, Answer::Allow);
}

#[tokio::test]
async fn consume_refuses_under_a_kill_switch_whatever_was_decided_before() {
    let stand = local("consume-switch").await;
    let fence = waiting_run(&stand.store, 1, "a").await;
    stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), diff()))
        .await
        .unwrap();
    let policy = stand.policy.clone();
    stand
        .store
        .write(Durability::Synced, move |tx| {
            policy.set_kill_switch(
                tx,
                &KillSwitch {
                    scope: KillScope::Target(TargetName::new("example").unwrap()),
                    on: true,
                    by: person(1),
                    presence: None,
                },
            )
        })
        .await
        .unwrap();
    assert_eq!(code(begin(&stand, fence, diff()).await), KILL_SWITCH_ON);
    assert_eq!(
        run_state(&stand.store, fence.run).await,
        RunState::NeedsApproval
    );
}

#[tokio::test]
async fn an_approval_is_refused_for_another_tool_version_or_a_proof_not_of_now() {
    let stand = local("approval-checks").await;
    let fence = waiting_run(&stand.store, 1, "a").await;
    let by = || PrincipalRef::person(person(1));
    let other_version = stand
        .surface
        .approve(Approved {
            tool_version: Digest::of(b"another plan"),
            ..approved(fence.run, by(), diff())
        })
        .await;
    assert_eq!(code(other_version), APPROVAL_STALE);
    let old_proof = stand
        .surface
        .approve(Approved {
            proved_at: Timestamp::from_unix_millis(START - 121_000),
            ..approved(fence.run, by(), diff())
        })
        .await;
    assert_eq!(code(old_proof), APPROVAL_REFUSED);
    let ahead = stand
        .surface
        .approve(Approved {
            proved_at: Timestamp::from_unix_millis(START + 121_000),
            ..approved(fence.run, by(), diff())
        })
        .await;
    assert_eq!(code(ahead), APPROVAL_REFUSED);
    // Off the engine's own prompt, a check that proves nothing is refused;
    // each of the operating systems' prompts proves presence.
    let unproved = stand
        .surface
        .approve(Approved {
            channel: Channel::ApprovalPage,
            presence: PresenceCheck::NotAvailable,
            ..approved(fence.run, by(), diff())
        })
        .await;
    assert_eq!(code(unproved), APPROVAL_REFUSED);
    for check in [
        PresenceCheck::Polkit,
        PresenceCheck::LocalAuthentication,
        PresenceCheck::WindowsHello,
        PresenceCheck::CopyTestHarness,
    ] {
        assert!(check.proves_presence(), "{check}");
    }
    stand
        .surface
        .approve(Approved {
            presence: PresenceCheck::Polkit,
            ..approved(fence.run, by(), diff())
        })
        .await
        .unwrap();
    begin(&stand, fence, diff()).await.unwrap();
    let raw = stand.folder.raw();
    let stored: String = raw
        .query_row("SELECT presence_check FROM approvals", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored, "polkit");
}

#[tokio::test]
async fn an_irreversible_run_needs_two_people_where_the_setting_asks() {
    let mut settings = BuiltIn::server().layers().settings.clone();
    settings.two_person_irreversible = true;
    let stand = stand("two", BuiltIn::server().with_settings(settings)).await;
    // The set published later does not ask for two people, and an
    // administrator who signed in publishes it; the installation's setting
    // still holds under it.
    publish(
        &stand,
        server_layers(),
        PublishProof {
            presence: Some(PresenceCheck::SignedIn),
        },
    )
    .await
    .unwrap();
    let fence = waiting_run_of(&stand.store, 3, "a", EffectClass::Irreversible).await;
    stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), diff()))
        .await
        .unwrap();
    assert_eq!(code(begin(&stand, fence, diff()).await), APPROVAL_MISSING);
    let twice = stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), diff()))
        .await;
    assert_eq!(code(twice), APPROVAL_REFUSED);
    stand
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(2)), diff()))
        .await
        .unwrap();
    begin(&stand, fence, diff()).await.unwrap();
    assert_eq!(run_state(&stand.store, fence.run).await, RunState::Running);
    let raw = stand.folder.raw();
    assert_eq!(
        count(
            &raw,
            "SELECT COUNT(*) FROM approvals WHERE used_at IS NOT NULL"
        ),
        2
    );

    // Without the setting, one approval clears a run of the same kind.
    let plain = stand_named_two_off().await;
    let fence = waiting_run_of(&plain.store, 3, "b", EffectClass::Irreversible).await;
    plain
        .surface
        .approve(approved(fence.run, PrincipalRef::person(person(1)), diff()))
        .await
        .unwrap();
    begin(&plain, fence, diff()).await.unwrap();
}

async fn stand_named_two_off() -> Stand {
    let stand = stand("two-off", BuiltIn::server()).await;
    publish(&stand, server_layers(), PublishProof::default())
        .await
        .unwrap();
    stand
}

#[tokio::test]
async fn the_installations_settings_stay_in_force_under_a_published_set() {
    let mut settings = BuiltIn::local(person(1), workspace())
        .layers()
        .settings
        .clone();
    settings.max_records_per_run = 10;
    let stand = stand(
        "floor",
        BuiltIn::local(person(1), workspace()).with_settings(settings),
    )
    .await;
    let check = |records| LimitCheck {
        principal: PrincipalRef::person(person(1)),
        workspace: workspace(),
        records: Some(records),
        count_call: false,
    };
    let limits = |records| {
        let policy = stand.policy.clone();
        let store = stand.store.clone();
        async move {
            store
                .write(Durability::Synced, move |tx| {
                    policy.check_limits(tx, &check(records))
                })
                .await
        }
    };
    assert_eq!(code(limits(11).await), LIMIT_EXCEEDED);
    // A set published with a looser limit, by a person present, does not
    // lift the installation's.
    let mut layers = BuiltIn::local(person(1), workspace()).layers().clone();
    layers.settings.max_records_per_run = 40;
    publish(
        &stand,
        layers,
        PublishProof {
            presence: Some(PresenceCheck::OsPrompt),
        },
    )
    .await
    .unwrap();
    assert_eq!(code(limits(11).await), LIMIT_EXCEEDED);
    limits(10).await.unwrap();
}

#[tokio::test]
async fn a_decision_is_explained_only_in_its_own_workspace() {
    let stand = local("explain-ws").await;
    let decided = stand
        .policy
        .decide_alone(ask(1, Question::UseTool, tool(), EffectClass::Read))
        .await;
    let policy = stand.policy.clone();
    let id = decided.id;
    let other = stand
        .store
        .read(move |tx| policy.explain(tx, id, WorkspaceId::from_bytes([8; 16])))
        .await
        .unwrap_err();
    assert_eq!(other.code(), &a2m_policy::DECISION_NOT_FOUND);
    let policy = stand.policy.clone();
    stand
        .store
        .read(move |tx| policy.explain(tx, id, workspace()))
        .await
        .unwrap();
}

fn example_scope() -> Scope {
    Scope::Target(TargetName::new("example").unwrap())
}

fn write_grant(who: u8) -> Grant {
    Grant {
        who: Subject::Principal(person(who)),
        scope: example_scope(),
        max_effect: EffectClass::Write,
        identity_mode: IdentityKind::PerUserSession,
    }
}

fn through(toolset: &str, mut request: Request) -> Request {
    request.context.toolset = Some(ToolsetName::new(toolset).unwrap());
    request
}

fn reporting_layers() -> PolicyLayers {
    let mut layers = server_layers();
    layers.grants.push(write_grant(2));
    layers.toolsets = vec![
        Toolset {
            name: ToolsetName::new("reporting").unwrap(),
            who: vec![Subject::Principal(person(1)), Subject::Principal(person(3))],
            tools: vec![Scope::Pattern("example_order_get*".to_owned())],
            max_effect: EffectClass::Read,
            identity_mode: IdentityKind::PerUserSession,
            allow_pre_approval: false,
        },
        Toolset {
            name: ToolsetName::new("everyone").unwrap(),
            who: vec![Subject::Anyone],
            tools: vec![Scope::Pattern("*".to_owned())],
            max_effect: EffectClass::Write,
            identity_mode: IdentityKind::PerUserSession,
            allow_pre_approval: true,
        },
    ];
    layers
}

async fn decide(stand: &Stand, request: Request) -> Decision {
    stand.policy.decide_alone(request).await
}

async fn explained(stand: &Stand, id: a2m_core::DecisionId) -> a2m_policy::Explanation {
    let policy = stand.policy.clone();
    stand
        .store
        .read(move |tx| policy.explain(tx, id, workspace()))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_toolset_only_narrows_what_the_grants_allow() {
    let stand = stand("toolset", BuiltIn::server()).await;
    publish(&stand, reporting_layers(), PublishProof::default())
        .await
        .unwrap();
    let get = || tool_in("example", "example_order_get");
    let use_tool = |by: u8, resource: Resource, effect: EffectClass| {
        ask(by, Question::UseTool, resource, effect)
    };

    // Inside the toolset: selected, connectable, and granted.
    let inside = decide(
        &stand,
        through("reporting", use_tool(1, get(), EffectClass::Read)),
    )
    .await;
    assert_eq!(inside.answer, Answer::Allow);
    // The same call with no toolset is judged by the grants alone.
    let bare = decide(&stand, use_tool(1, tool(), EffectClass::Write)).await;
    assert_eq!(bare.answer, Answer::Allow);
    // A tool the toolset does not select, though a grant covers it.
    let outside = decide(
        &stand,
        through("reporting", use_tool(1, tool(), EffectClass::Write)),
    )
    .await;
    assert_eq!(outside.answer, Answer::Deny);
    let says = explained(&stand, outside.id).await;
    let ids: Vec<&str> = says.rules.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"toolset.reporting.not_selected"), "{ids:?}");
    assert!(ids.contains(&"toolset.reporting.effect_ceiling"), "{ids:?}");
    assert!(says.rules.iter().all(|r| r.kind == RuleKind::Toolset));
    // Not someone the toolset lists, though a grant covers them.
    let stranger = decide(
        &stand,
        through("reporting", use_tool(2, get(), EffectClass::Read)),
    )
    .await;
    assert_eq!(stranger.answer, Answer::Deny);
    assert_eq!(
        explained(&stand, stranger.id).await.rules[0].id,
        "toolset.reporting.not_connectable"
    );
    // A toolset lists a caller no grant covers: still denied. It cannot widen.
    let ungranted = decide(
        &stand,
        through("reporting", use_tool(3, get(), EffectClass::Read)),
    )
    .await;
    assert_eq!(ungranted.answer, Answer::Deny);
    assert_eq!(
        explained(&stand, ungranted.id).await.rules[0].id,
        "default.deny"
    );
    // The identity kind of the toolset.
    let mut act = through(
        "reporting",
        ask(
            1,
            Question::ActAs,
            Resource::Identity(a2m_core::Identity::Person(person(1))),
            EffectClass::Read,
        ),
    );
    act.context.identity_mode = Some(IdentityKind::PerUserToken);
    assert_eq!(decide(&stand, act.clone()).await.answer, Answer::Deny);
    act.context.identity_mode = Some(IdentityKind::PerUserSession);
    assert_eq!(decide(&stand, act).await.answer, Answer::Allow);
    // Once a set defines toolsets, one it does not define is unknown.
    let unknown = decide(
        &stand,
        through("nowhere", use_tool(1, get(), EffectClass::Read)),
    )
    .await;
    assert_eq!(unknown.answer, Answer::Deny);
    assert_eq!(
        explained(&stand, unknown.id).await.rules[0].id,
        "toolset.unknown"
    );
}

#[tokio::test]
async fn a_toolset_can_forbid_a_write_without_a_person() {
    let stand = stand("toolset-pre", BuiltIn::server()).await;
    let mut layers = reporting_layers();
    layers.settings.pre_approval = true;
    layers.settings.writes_outside_tests = true;
    layers.pre_approvals.push(PreApproval {
        name: "small_writes".to_owned(),
        who: Subject::Anyone,
        scope: Scope::Pattern("example_*".to_owned()),
        max_records: 5,
        condition: Some("tool != \"marker_condition_text\"".to_owned()),
    });
    publish(
        &stand,
        layers,
        PublishProof {
            presence: Some(PresenceCheck::SignedIn),
        },
    )
    .await
    .unwrap();
    let fence = waiting_run(&stand.store, 1, "a").await;
    let write = |toolset: &str, resource: Resource, effect: EffectClass| {
        let mut request = through(
            toolset,
            ask(1, Question::WriteWithoutPerson, resource, effect),
        );
        request.run = Some(fence.run);
        request.diff_hash = Some(diff());
        request
    };
    // Where the toolset allows a rule to clear a write, the rule does.
    let cleared = decide(&stand, write("everyone", tool(), EffectClass::Write)).await;
    assert_eq!(cleared.outcome(), Outcome::Allow);
    let says = explained(&stand, cleared.id).await;
    assert_eq!(says.rules[0].kind, RuleKind::PreApproval);
    assert!(says.summary.contains("small_writes"), "{}", says.summary);
    assert!(
        !says.summary.contains("marker_condition_text"),
        "an explanation never prints a rule's condition: {}",
        says.summary
    );
    // Where it does not, the run waits for a person: not a refusal.
    let get = tool_in("example", "example_order_get");
    let waits = decide(&stand, write("reporting", get, EffectClass::Read)).await;
    assert_eq!(waits.outcome(), Outcome::NeedsApproval);
    assert_eq!(
        explained(&stand, waits.id).await.rules[0].id,
        "toolset.reporting.no_pre_approval"
    );
}

#[tokio::test]
async fn a_role_is_a_bundle_of_grants() {
    let stand = stand("rolegrants", BuiltIn::server()).await;
    let mut layers = PolicyLayers::default();
    layers.roles.push(RoleBinding {
        who: Subject::Principal(person(2)),
        role: Role::Operator,
    });
    layers.role_grants = vec![
        RoleGrant {
            role: Role::Operator,
            scope: example_scope(),
            max_effect: EffectClass::Write,
            identity_mode: IdentityKind::PerUserSession,
        },
        RoleGrant {
            role: Role::Viewer,
            scope: example_scope(),
            max_effect: EffectClass::Read,
            identity_mode: IdentityKind::PerUserSession,
        },
    ];
    publish(&stand, layers, PublishProof::default())
        .await
        .unwrap();
    let held = decide(
        &stand,
        ask(2, Question::UseTool, tool(), EffectClass::Write),
    )
    .await;
    assert_eq!(held.answer, Answer::Allow);
    let says = explained(&stand, held.id).await;
    assert_eq!(says.rules[0].id, "role_grant.0");
    assert_eq!(says.rules[0].kind, RuleKind::RoleGrant);
    assert!(
        says.rules[0].says.contains("operator"),
        "{}",
        says.rules[0].says
    );
    // Not holding the role is holding nothing.
    let without = decide(&stand, ask(3, Question::UseTool, tool(), EffectClass::Read)).await;
    assert_eq!(without.answer, Answer::Deny);
    // The role's own ceiling.
    let high = decide(
        &stand,
        ask(2, Question::UseTool, tool(), EffectClass::Irreversible),
    )
    .await;
    assert_eq!(
        high.refusal.as_ref().unwrap().code(),
        &a2m_policy::EFFECT_TOO_HIGH
    );
}

#[tokio::test]
async fn a_set_that_asks_for_a_second_administrator_cannot_be_published_by_one() {
    let stand = stand("second", BuiltIn::server()).await;
    let mut layers = server_layers();
    layers.roles.push(RoleBinding {
        who: Subject::Principal(person(5)),
        role: Role::Admin,
    });
    layers.roles.push(RoleBinding {
        who: Subject::Principal(person(6)),
        role: Role::Publisher,
    });
    layers.second_admin_publish = true;
    // The set that turns the need on needs no second one: nothing asked yet.
    publish(&stand, layers.clone(), PublishProof::default())
        .await
        .unwrap();

    let publisher = PrincipalRef::person(person(9));
    let attempt = |review: Option<PublishReview>, next: PolicyLayers, proof: PublishProof| {
        let policy = stand.policy.clone();
        let store = stand.store.clone();
        let publisher = publisher.clone();
        async move {
            store
                .write(Durability::Synced, move |tx| {
                    let candidate = policy.compile(&next)?;
                    policy.publish_reviewed(tx, &candidate, &publisher, proof, review.as_ref())
                })
                .await
        }
    };
    let mut next = layers.clone();
    next.grants.push(write_grant(7));
    let review = |who: PrincipalRef, drafted_by: Option<PrincipalId>| {
        Some(PublishReview {
            reviewer: who,
            drafted_by,
        })
    };
    let none = PublishProof::default();
    // No reviewer; the publisher as reviewer; a person with no role; the author.
    assert_eq!(
        code(attempt(None, next.clone(), none).await),
        SECOND_ADMIN_REQUIRED
    );
    for (who, drafted) in [
        (PrincipalRef::person(person(9)), None),
        (PrincipalRef::person(person(7)), None),
        (PrincipalRef::person(person(5)), Some(person(5))),
    ] {
        assert_eq!(
            code(attempt(review(who, drafted), next.clone(), none).await),
            SECOND_ADMIN_REQUIRED
        );
    }
    // An agent reviewer, even one that holds the role.
    let mut agent = PrincipalRef::person(person(5));
    agent.kind = PrincipalKind::Agent;
    assert_eq!(
        code(attempt(review(agent, None), next.clone(), none).await),
        SECOND_ADMIN_REQUIRED
    );
    // Nothing was published by the refusals.
    let before = decide(
        &stand,
        ask(7, Question::UseTool, tool(), EffectClass::Write),
    )
    .await;
    assert_eq!(before.answer, Answer::Deny);
    // An administrator, then a publisher, reviews it.
    attempt(
        review(PrincipalRef::person(person(5)), Some(person(9))),
        next.clone(),
        none,
    )
    .await
    .unwrap();
    let after = decide(
        &stand,
        ask(7, Question::UseTool, tool(), EffectClass::Write),
    )
    .await;
    assert_eq!(after.answer, Answer::Allow);
    let mut again = next.clone();
    again.grants.push(write_grant(8));
    attempt(review(PrincipalRef::person(person(6)), None), again, none)
        .await
        .unwrap();
    // A set cannot switch the need off for itself: it needs the reviewer, and
    // then, because it loosens a protected setting, a person present.
    let mut off = next;
    off.second_admin_publish = false;
    assert_eq!(
        code(attempt(None, off.clone(), none).await),
        SECOND_ADMIN_REQUIRED
    );
    assert_eq!(
        code(attempt(review(PrincipalRef::person(person(5)), None), off, none).await),
        PRESENCE_REQUIRED
    );
}

#[tokio::test]
async fn one_grant_changes_through_a_new_version_with_its_own_tests() {
    let stand = stand("change", BuiltIn::server()).await;
    publish(&stand, server_layers(), PublishProof::default())
        .await
        .unwrap();
    let by = PrincipalRef::person(person(9));
    let change = |change: LayerChange, tests: Vec<PolicyTest>| {
        let policy = stand.policy.clone();
        let store = stand.store.clone();
        let by = by.clone();
        async move {
            store
                .write(Durability::Synced, move |tx| {
                    policy.publish_change(tx, change, tests, &by, PublishProof::default(), None)
                })
                .await
        }
    };
    let can = |who: u8| {
        let request = ask(who, Question::UseTool, tool(), EffectClass::Write);
        let policy = stand.policy.clone();
        let store = stand.store.clone();
        async move {
            store
                .read(move |tx| Ok(policy.would(tx, &request)))
                .await
                .unwrap()
        }
    };
    assert_eq!(can(2).await, Answer::Deny);
    let own_test = PolicyTest {
        name: "the new person writes".to_owned(),
        request: ask(2, Question::UseTool, tool(), EffectClass::Write),
        expect: Answer::Allow,
    };
    // A test table the new version fails stops it, and nothing changes.
    let wrong = PolicyTest {
        expect: Answer::Deny,
        ..own_test.clone()
    };
    let failed = change(LayerChange::AddGrant(write_grant(2)), vec![wrong])
        .await
        .unwrap_err();
    assert_eq!(failed.code(), &POLICY_TESTS_FAILED);
    assert_eq!(can(2).await, Answer::Deny);

    let version = change(LayerChange::AddGrant(write_grant(2)), vec![own_test])
        .await
        .unwrap();
    assert_eq!(can(2).await, Answer::Allow);
    assert_eq!(
        can(1).await,
        Answer::Allow,
        "the other grants are untouched"
    );
    // Adding it again makes no second grant, so the layers are the same.
    let again = change(LayerChange::AddGrant(write_grant(2)), Vec::new())
        .await
        .unwrap();
    assert_ne!(again, version, "only the test table differs");

    // The history lists each version, the newest in force.
    let policy = stand.policy.clone();
    let history = stand
        .store
        .read(move |tx| policy.versions(tx))
        .await
        .unwrap();
    assert_eq!(history.len(), 4, "{history:?}");
    assert!(history.last().unwrap().in_force);
    assert!(history.iter().any(|row| row.version == version));
    assert_eq!(history.iter().filter(|row| row.in_force).count(), 1);
    assert_eq!(
        history[0].published_by, None,
        "the built-in set has no publisher"
    );

    // Removing a grant that is there, and one that is not.
    change(LayerChange::RemoveGrant(write_grant(2)), Vec::new())
        .await
        .unwrap();
    assert_eq!(can(2).await, Answer::Deny);
    let missing = change(LayerChange::RemoveGrant(write_grant(2)), Vec::new())
        .await
        .unwrap_err();
    assert_eq!(missing.class(), FailureClass::Input);
}

#[tokio::test]
async fn the_catalog_is_asked_without_recording() {
    let stand = stand("catalog", BuiltIn::server()).await;
    publish(&stand, reporting_layers(), PublishProof::default())
        .await
        .unwrap();
    let Resource::Tool(update) = tool() else {
        unreachable!()
    };
    let Resource::Tool(get) = tool_in("example", "example_order_get") else {
        unreachable!()
    };
    let read_context = Context {
        effect: EffectClass::Read,
        ..Context::plain(workspace(), environment())
    };
    let write_context = Context {
        effect: EffectClass::Write,
        ..read_context.clone()
    };
    let catalog = vec![
        (update.clone(), write_context.clone()),
        (get.clone(), read_context),
    ];
    let raw = stand.folder.raw();
    let rows = count(&raw, "SELECT COUNT(*) FROM decisions");
    let policy = stand.policy.clone();
    let (list, who, selected, absent) = {
        let (update, get) = (update.clone(), get.clone());
        stand
            .store
            .read(move |tx| {
                let one = PrincipalRef::person(person(1));
                let list = policy.tools_allowed(tx, &one, &catalog);
                let who = policy.who_can(
                    tx,
                    &update,
                    &write_context,
                    &[
                        one.clone(),
                        PrincipalRef::person(person(2)),
                        PrincipalRef::person(person(4)),
                    ],
                );
                let selected = policy.toolset_tools(
                    tx,
                    &ToolsetName::new("reporting").unwrap(),
                    &[update.clone(), get.clone()],
                );
                let absent = policy.toolset_tools(tx, &ToolsetName::new("none").unwrap(), &[]);
                Ok((list, who, selected, absent))
            })
            .await
            .unwrap()
    };
    assert_eq!(list, vec![update, get.clone()]);
    assert_eq!(who, vec![person(1), person(2)]);
    assert_eq!(selected, Some(vec![get]));
    assert_eq!(absent, None);
    assert_eq!(count(&raw, "SELECT COUNT(*) FROM decisions"), rows);
}
