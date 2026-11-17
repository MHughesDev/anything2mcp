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
use a2m_policy::{
    APPROVAL_MISSING, APPROVAL_REFUSED, APPROVAL_STALE, ActionName, ActionRef, Answer,
    ApprovalSurface, ApproveWrites, Approved, BuiltIn, Channel, Context, Decision, Declined, Grant,
    IdentityMode, KILL_SWITCH_ON, KillScope, KillSwitch, LIMIT_EXCEEDED, LimitCheck, ModuleName,
    Outcome, POLICY_EVALUATION_FAILED, POLICY_TESTS_FAILED, PRESENCE_REQUIRED, Policy,
    PolicyHealth, PolicyLayers, PolicyTest, PreApproval, PresenceCheck, PrincipalRef, PublishProof,
    Question, Quota, Request, Resource, Role, RoleBinding, Scope, Subject, ToolRef,
};
use a2m_telemetry::ManualClock;
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
        .with_clock(Arc::clone(clock) as Arc<dyn a2m_telemetry::Clock>)
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
        effect: EffectClass::Write,
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
            identity_mode: IdentityMode::PerUserSession,
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
