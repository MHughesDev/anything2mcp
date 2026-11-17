//! The driver handle: what it refuses before a driver is called, what it
//! refuses after, and that a discoverer cannot write.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_core::{
    ApprovalId, Digest, EffectClass, Environment, Epoch, EvidenceLevel, Failure, FailureClass,
    OperationName, Plan, PrincipalId, RecordKey, RunState, Step, TargetName, Timestamp, Value,
    Values, Version, WorkspaceId,
};
use a2m_files::CompileMode;
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, Owner, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind, WriteClearance,
};
use a2m_provider::{
    Authority, CheckedPaths, Deadline, Destinations, DriverHandle, Posture, PrepareContext,
    Prepared, ProviderSettings, ReadOnlyPort, Registry, RegistryBuilder, Roots, Sent, StepFailure,
    StepInput, StepOutcome, SurfaceRef, codes,
};
use a2m_telemetry::ManualClock;
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};
use common::{Behaviour, Folder, TestPack, failure, host, mode, step_kind};

// ---- a real store, for real intent tokens ----

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
    Arc::new(Counted(AtomicU64::new(0)))
}

async fn store() -> Store {
    let config = StoreConfig::in_memory()
        .with_clock(Arc::new(ManualClock::starting_at(
            Timestamp::from_unix_millis(1_800_000_000_000),
        )))
        .with_random(counted());
    let store = Store::open(config).await.unwrap();
    let vault = Arc::new(Vault::with_random(
        Arc::new(MemoryKeyStore::with_random(counted())),
        Arc::new(store.key_records()),
        counted(),
    ));
    store.attach(vault).await.unwrap();
    store
}

fn key(name: &str) -> RecordKey {
    RecordKey::new(vec![Value::from(name)]).unwrap()
}

fn intended(name: &str) -> Digest {
    Digest::of(format!("intended {name}").as_bytes())
}

/// A write run with the named records planned, approved, and running.
async fn running(store: &Store, caller: u8, names: &[&str]) -> Fence {
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    let workspace = WorkspaceId::from_bytes([1; 16]);
    store
        .write(Durability::Synced, move |tx| {
            let diff = Digest::of(b"diff");
            let fence = {
                let mut ledger = tx.ledger();
                let Opened::New(opened) = ledger.open_run(NewRun {
                    workspace,
                    principal: PrincipalId::from_bytes([caller; 16]),
                    client_label: "test".to_owned(),
                    tool_name: OperationName::new("example_order_update").unwrap(),
                    tool_kind: ToolKind::Operation,
                    target: TargetName::new("example").unwrap(),
                    target_version: Digest::of(b"plan"),
                    environment: Environment::new("test").unwrap(),
                    identity: None,
                    effect: EffectClass::Write,
                    input_hash: Digest::of(names.join(",").as_bytes()),
                    input: None,
                    retention: RetentionClass::Long,
                })?
                else {
                    panic!("the run exists already");
                };
                let fence = ledger.new_epoch(opened.run)?;
                ledger.set_state(&fence, RunState::Validating, None)?;
                let records: Vec<_> = names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| PlannedRecord {
                        step_seq: 0,
                        record_seq: u32::try_from(at).unwrap(),
                        key: key(name),
                        operation: OperationName::new("example_order_update").unwrap(),
                        intended: intended(name),
                        intended_values: None,
                        before: None,
                        unchanged: false,
                    })
                    .collect();
                ledger.plan_records(&fence, &records)?;
                ledger.store_diff(&fence, diff, None)?;
                ledger.set_state(&fence, RunState::NeedsApproval, None)?;
                fence
            };
            // The policy engine's stand-in: an approval, used once.
            let approval = ApprovalId::from_parts(tx.now(), tx.random()?)?;
            let now = tx.now();
            tx.tables(Owner::Policy).execute(
                "INSERT INTO approvals
                    (id, workspace_id, scope, run_id, diff_hash, approver_id, channel, presence_check, granted_at, used_at)
                 VALUES (?1, ?2, 'run', ?3, ?4, ?5, 'local', 'confirmed', ?6, ?6)",
                &[
                    approval.to_string().into(),
                    workspace.to_string().into(),
                    fence.run.to_string().into(),
                    diff.to_string().into(),
                    PrincipalId::from_bytes([9; 16]).to_string().into(),
                    now.into(),
                ],
            )?;
            tx.ledger()
                .begin_writes(&fence, WriteClearance::Approval(approval))?;
            Ok(fence)
        })
        .await
        .unwrap()
}

fn record(fence: &Fence, record_seq: u32) -> RecordRef {
    RecordRef {
        run: fence.run,
        step_seq: 0,
        record_seq,
    }
}

async fn token(store: &Store, fence: Fence, record_seq: u32, name: &str) -> IntentToken {
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .take_lock(&fence, record(&fence, record_seq))
                .map(|_| ())
        })
        .await
        .unwrap();
    let intents = vec![RecordIntent {
        record: record(&fence, record_seq),
        intended: intended(name),
        mode: None,
    }];
    store
        .store_intents(&fence, intents)
        .await
        .unwrap()
        .remove(0)
}

// ---- the plan, the registry, and one prepared driver ----

struct Scene {
    _folder: Folder,
    pack: TestPack,
    registry: Registry,
    plan: Plan,
    surface: SurfaceRef,
}

fn scene() -> Scene {
    let folder = Folder::new("handle");
    let pack = TestPack::new("alpha");
    let mut builder =
        RegistryBuilder::new(ProviderSettings::default(), host(&folder, Roots::none()));
    builder.add_pack(&pack);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    // A real plan, compiled from the example target by the compiler's own
    // test pack. Its steps are of the kinds `fetch` and `send`.
    let set = a2m_files::testing::stand_set().unwrap();
    let sources = a2m_files::load(&a2m_files::testing::example_root(), &set).unwrap();
    let plan = a2m_files::compile(&sources, &set, CompileMode::Draft, Version::new(1, 0, 0))
        .unwrap()
        .plan;
    let surface = SurfaceRef {
        target: plan.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface: plan.surfaces[0].clone(),
    };
    Scene {
        _folder: folder,
        pack,
        registry,
        plan,
        surface,
    }
}

impl Scene {
    /// A step of the plan, with the marks a test needs.
    fn step(&self, kind: &str, commits: bool, lookup: bool) -> Step {
        let mut step = self
            .plan
            .operations
            .iter()
            .flat_map(|operation| &operation.bindings)
            .flat_map(|binding| binding.act.iter().chain(&binding.read_back))
            .next()
            .expect("the example plan has a step")
            .clone();
        step.kind = step_kind(kind);
        step.commits = commits;
        step.lookup = lookup;
        step
    }

    async fn prepared(&self, fence: Option<Fence>, posture: Posture) -> (DriverHandle, Prepared) {
        let handle = self.registry.driver(&mode()).unwrap();
        let fence = fence.unwrap_or(Fence {
            run: a2m_core::RunId::from_bytes([7; 16]),
            epoch: Epoch::new(1),
        });
        let supervisor = handle.supervisor(Vec::new());
        let cx = PrepareContext {
            run: fence.run,
            epoch: fence.epoch,
            surface: &self.surface,
            plan: &self.plan,
            posture,
            deadline: Deadline::after(Duration::from_secs(30)),
            redact: &a2m_core::NoSecrets,
            destinations: &Destinations::default(),
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let prepared = handle.prepare(&cx, None).await.unwrap();
        (handle, prepared)
    }

    fn reached(&self) -> Vec<String> {
        self.pack.reached.lock().unwrap().clone()
    }

    fn behave(&self, behaviour: Behaviour) {
        *self.pack.behaviour.lock().unwrap() = behaviour;
    }
}

fn input<'a>(
    args: &'a Values,
    record: Option<RecordRef>,
    key: Option<&'a RecordKey>,
) -> StepInput<'a> {
    StepInput {
        args,
        record,
        key,
        stamp: None,
        deadline: Deadline::after(Duration::from_secs(30)),
    }
}

fn refused_unsent(result: Result<StepOutcome, StepFailure>) -> Failure {
    let failure = result.unwrap_err();
    assert_eq!(failure.sent, Sent::NotSent);
    failure.failure
}

#[tokio::test]
async fn a_committing_step_reaches_the_driver_only_with_the_token_of_its_record() {
    let scene = scene();
    let store = store().await;
    let fence = running(&store, 1, &["one", "two"]).await;
    let (handle, mut prepared) = scene.prepared(Some(fence), Posture::Live).await;
    let send = scene.step("send", true, false);
    let args = Values::new();
    let key_one = key("one");

    // No token: refused, and the driver is never called.
    let refused = refused_unsent(
        handle
            .act(
                &mut prepared,
                &send,
                &input(&args, Some(record(&fence, 0)), None),
                Authority::Read,
            )
            .await,
    );
    assert_eq!(
        (refused.class(), refused.code()),
        (FailureClass::Policy, &codes::INTENT_MISSING)
    );
    assert!(scene.reached().is_empty());

    // The token of another record of the same run: refused.
    let token_two = token(&store, fence, 1, "two").await;
    let refused = refused_unsent(
        handle
            .act(
                &mut prepared,
                &send,
                &input(&args, Some(record(&fence, 0)), Some(&key_one)),
                Authority::Write(&token_two),
            )
            .await,
    );
    assert_eq!(refused.code(), &codes::INTENT_MISSING);
    assert!(refused.message().contains("another record"), "{refused}");
    assert!(scene.reached().is_empty());

    // The token of another run: refused.
    let other = running(&store, 2, &["three"]).await;
    let token_other = token(&store, other, 0, "three").await;
    let refused = refused_unsent(
        handle
            .act(
                &mut prepared,
                &send,
                &input(&args, Some(record(&fence, 0)), None),
                Authority::Write(&token_other),
            )
            .await,
    );
    assert!(refused.message().contains("another run"), "{refused}");
    assert!(scene.reached().is_empty());

    // The right token: the step reaches the driver.
    let token_one = token(&store, fence, 0, "one").await;
    let outcome = handle
        .act(
            &mut prepared,
            &send,
            &input(&args, Some(record(&fence, 0)), Some(&key_one)),
            Authority::Write(&token_one),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, StepOutcome::Done(_)));
    assert_eq!(scene.reached(), ["send"]);

    // A token stored under another lease number: refused.
    let (stale_handle, mut stale) = scene
        .prepared(
            Some(Fence {
                run: fence.run,
                epoch: Epoch::new(fence.epoch.get() + 1),
            }),
            Posture::Live,
        )
        .await;
    let refused = refused_unsent(
        stale_handle
            .act(
                &mut stale,
                &send,
                &input(&args, Some(record(&fence, 0)), None),
                Authority::Write(&token_one),
            )
            .await,
    );
    assert!(refused.message().contains("lease number"), "{refused}");
    assert_eq!(scene.reached(), ["send"]);
}

#[tokio::test]
async fn nothing_commits_outside_a_posture_that_may_write() {
    let scene = scene();
    let store = store().await;
    let fence = running(&store, 1, &["one"]).await;
    let token_one = token(&store, fence, 0, "one").await;
    let send = scene.step("send", true, false);
    let args = Values::new();
    for posture in [
        Posture::Discovery,
        Posture::Sandbox {
            test_records: false,
        },
    ] {
        let (handle, mut prepared) = scene.prepared(Some(fence), posture).await;
        let refused = refused_unsent(
            handle
                .act(
                    &mut prepared,
                    &send,
                    &input(&args, Some(record(&fence, 0)), None),
                    Authority::Write(&token_one),
                )
                .await,
        );
        assert_eq!(
            (refused.class(), refused.code()),
            (FailureClass::Policy, &codes::READ_ONLY_POSTURE)
        );
    }
    assert!(scene.reached().is_empty());
}

#[tokio::test]
async fn what_a_driver_returns_is_checked() {
    let scene = scene();
    let store = store().await;
    let fence = running(&store, 1, &["one"]).await;
    let (handle, mut prepared) = scene.prepared(Some(fence), Posture::Live).await;
    let args = Values::new();
    let read = input(&args, None, None);

    // A step of a kind the mode did not declare never reaches the driver.
    let refused = refused_unsent(
        handle
            .act(
                &mut prepared,
                &scene.step("click", false, false),
                &read,
                Authority::Read,
            )
            .await,
    );
    assert_eq!(
        (refused.class(), refused.code()),
        (FailureClass::UnknownVariant, &codes::STEP_KIND_UNKNOWN)
    );
    assert!(scene.reached().is_empty());

    // Absent is allowed from a lookup, and nowhere else.
    scene.behave(Behaviour::Absent);
    let found = handle
        .act(
            &mut prepared,
            &scene.step("fetch", false, true),
            &read,
            Authority::Read,
        )
        .await
        .unwrap();
    assert!(matches!(found, StepOutcome::Absent(_)));
    let failure = handle
        .act(
            &mut prepared,
            &scene.step("fetch", false, false),
            &read,
            Authority::Read,
        )
        .await
        .unwrap_err();
    assert_eq!(
        (
            failure.failure.class(),
            failure.failure.code(),
            failure.sent
        ),
        (
            FailureClass::UnknownVariant,
            &codes::UNEXPECTED_ABSENT,
            Sent::Answered
        )
    );

    // Values for reconcile pass on a step before the commit point, and are
    // refused on the committing step itself.
    scene.behave(Behaviour::DoneWithReconcile);
    let StepOutcome::Done(observed) = handle
        .act(
            &mut prepared,
            &scene.step("fetch", false, false),
            &read,
            Authority::Read,
        )
        .await
        .unwrap()
    else {
        panic!("done");
    };
    assert!(observed.for_reconcile.contains("assigned"));
    let token_one = token(&store, fence, 0, "one").await;
    let write = input(&args, Some(record(&fence, 0)), None);
    let failure = handle
        .act(
            &mut prepared,
            &scene.step("send", true, false),
            &write,
            Authority::Write(&token_one),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.failure.code(), &codes::RECONCILE_TOO_LATE);

    // A declared code passes; an invented one is refused.
    scene.behave(Behaviour::Fail(StepFailure::answered(failure_of(
        FailureClass::Rejected,
        "example_refused",
    ))));
    let failure = handle
        .act(
            &mut prepared,
            &scene.step("fetch", false, false),
            &read,
            Authority::Read,
        )
        .await
        .unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "example_refused");
    scene.behave(Behaviour::Fail(StepFailure::answered(failure_of(
        FailureClass::Rejected,
        "made_up",
    ))));
    let failure = handle
        .act(
            &mut prepared,
            &scene.step("fetch", false, false),
            &read,
            Authority::Read,
        )
        .await
        .unwrap_err();
    assert_eq!(
        (
            failure.failure.class(),
            failure.failure.code(),
            failure.sent
        ),
        (
            FailureClass::UnknownVariant,
            &codes::UNDECLARED_CODE,
            Sent::Answered
        )
    );

    // A doubtful write is always possible_commit, whatever the driver said.
    scene.behave(Behaviour::Fail(StepFailure::unknown(failure_of(
        FailureClass::Transient,
        "timeout",
    ))));
    let failure = handle
        .act(
            &mut prepared,
            &scene.step("send", true, false),
            &write,
            Authority::Write(&token_one),
        )
        .await
        .unwrap_err();
    assert_eq!(
        (
            failure.failure.class(),
            failure.failure.code(),
            failure.sent
        ),
        (
            FailureClass::PossibleCommit,
            &codes::NO_CONFIRMATION,
            Sent::Unknown
        )
    );
    // The same doubt on a step that commits nothing stays transient.
    let failure = handle
        .act(
            &mut prepared,
            &scene.step("fetch", false, false),
            &read,
            Authority::Read,
        )
        .await
        .unwrap_err();
    assert_eq!(failure.failure.class(), FailureClass::Transient);
    assert!(handle.release(prepared, true).await.complete);
}

fn failure_of(class: FailureClass, code: &'static str) -> Failure {
    failure(class, code)
}

struct Kept(Vec<a2m_core::Finding>);

impl a2m_provider::FindingSink for Kept {
    fn found(&mut self, finding: a2m_core::Finding) -> Result<(), Failure> {
        self.0.push(finding);
        Ok(())
    }
}

#[tokio::test]
async fn a_discoverer_cannot_write() {
    // By its types: `Discoverer::discover` is given a `SandboxPort`, a
    // scope, and a sink. The port's one method takes no `Authority`, so no
    // token can be passed; a `DriverHandle` comes only from the registry,
    // which no pack is given; a token has no public constructor. What is
    // left to prove is that a committing step sent through the port is
    // refused before the driver sees it.
    let scene = scene();
    let (handle, prepared) = scene.prepared(None, Posture::Discovery).await;
    let port = ReadOnlyPort::new(handle, prepared);
    let surface = a2m_provider::SurfaceDescription {
        kind: a2m_core::SurfaceKind::new("alpha_surface").unwrap(),
        archetype: a2m_core::Archetype::Request,
        address: String::new(),
        facts: Values::new(),
    };
    let discoverer = scene.registry.discoverers_for(&surface).remove(0);
    let focus = Values::new();
    let scope = a2m_provider::DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(Duration::from_secs(30)),
        paths: &CheckedPaths::none(),
        redact: &a2m_core::NoSecrets,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());

    *scene.pack.discoverer.step.lock().unwrap() = Some(scene.step("send", true, false));
    discoverer.discover(&port, &scope, &mut kept).await.unwrap();
    let result = scene
        .pack
        .discoverer
        .port_result
        .lock()
        .unwrap()
        .take()
        .unwrap();
    let refused = refused_unsent(result);
    assert_eq!(refused.class(), FailureClass::Policy);
    assert!(scene.reached().is_empty(), "the write reached the driver");

    // A reading step passes through the same port.
    *scene.pack.discoverer.step.lock().unwrap() = Some(scene.step("fetch", false, false));
    discoverer.discover(&port, &scope, &mut kept).await.unwrap();
    let result = scene
        .pack
        .discoverer
        .port_result
        .lock()
        .unwrap()
        .take()
        .unwrap();
    assert!(result.is_ok());
    assert_eq!(scene.reached(), ["fetch"]);

    // A finding of a kind, or at a level, the manifest does not allow is refused.
    let finding = |kind: &str, level: EvidenceLevel| common::finding(kind, level);
    scene
        .pack
        .discoverer
        .findings
        .lock()
        .unwrap()
        .push(finding("call", EvidenceLevel::Discovered));
    discoverer.discover(&port, &scope, &mut kept).await.unwrap();
    assert_eq!(kept.0.len(), 1);
    scene
        .pack
        .discoverer
        .findings
        .lock()
        .unwrap()
        .push(finding("screen", EvidenceLevel::Discovered));
    let failure = discoverer
        .discover(&port, &scope, &mut kept)
        .await
        .unwrap_err();
    assert_eq!(failure.code(), &codes::FINDING_NOT_DECLARED);
    assert_eq!(kept.0.len(), 1);
    assert_eq!(
        discoverer
            .describe(&kept.0, a2m_provider::DescribeForm::Summary)
            .text,
        "1 findings"
    );
}
