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
    Digest, EffectClass, Environment, Epoch, EvidenceLevel, Failure, FailureClass, OperationName,
    Plan, PrincipalId, RecordKey, RunState, Step, TargetName, Timestamp, Value, Values,
    WorkspaceId,
};
use a2m_files::CompileMode;
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind,
};
use a2m_provider::{
    Authority, CheckedPaths, Deadline, Destinations, DriverHandle, Posture, PrepareContext,
    Prepared, ProviderSettings, ReadOnlyPort, Registry, RegistryBuilder, Roots, Sent, StepFailure,
    StepInput, StepOutcome, SurfaceRef, codes,
};
use a2m_telemetry::ManualClock;
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};
use common::{
    Behaviour, Folder, OneSecret, TestPack, failure, host_with, manifest, mode, step_kind,
};

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
            // The policy engine's stand-in: an approval, used once, made
            // through the ledger's own way for tests.
            let clearance = a2m_ledger::testing::approve(tx, &fence)?;
            tx.ledger().begin_writes(&fence, clearance)?;
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
    scene_of(TestPack::new("alpha"), Arc::new(a2m_core::NoSecrets))
}

/// A scene with the pack and the host's redactor given.
fn scene_of(pack: TestPack, redact: Arc<dyn a2m_core::Redact>) -> Scene {
    let folder = Folder::new("handle");
    let mut builder = RegistryBuilder::new(
        ProviderSettings::default(),
        host_with(&folder, Roots::none(), redact),
    );
    builder.add_pack(&pack);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    // A real plan, compiled from the example target by the compiler's own
    // test pack. Its steps are of the kinds `fetch` and `send`.
    let set = a2m_files::testing::stand_set().unwrap();
    let sources = a2m_files::load(&a2m_files::testing::example_root(), &set).unwrap();
    let plan = a2m_files::compile(&sources, &set, CompileMode::Draft, a2m_files::DRAFT_VERSION)
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

/// A write that was sent is settled only by the system's own verdict.
/// `policy` and `input` say that nothing was sent, and the kernel releases
/// a record's lock on them without reading the record: from a committing
/// step that did leave, they are a write in doubt.
#[tokio::test]
async fn a_committing_step_that_was_sent_is_never_settled_by_a_class_that_means_unsent() {
    let pack = TestPack::with_manifest(
        "alpha",
        manifest(
            "alpha",
            "request",
            "\n[[failure_codes]]\nclass = \"policy\"\ncode = \"example_limit\"\n\n[[failure_codes]]\nclass = \"input\"\ncode = \"example_shape\"\n\n[[failure_codes]]\nclass = \"stale\"\ncode = \"example_changed\"\n",
        ),
    );
    let scene = scene_of(pack, Arc::new(a2m_core::NoSecrets));
    let store = store().await;
    let fence = running(&store, 1, &["one"]).await;
    let (handle, mut prepared) = scene.prepared(Some(fence), Posture::Live).await;
    let args = Values::new();
    let token_one = token(&store, fence, 0, "one").await;
    let write = input(&args, Some(record(&fence, 0)), None);
    let read = input(&args, None, None);
    let send = scene.step("send", true, false);

    for (class, code) in [
        (FailureClass::Policy, "example_limit"),
        (FailureClass::Input, "example_shape"),
    ] {
        // Answered, as a driver says of a reply it read: in doubt.
        scene.behave(Behaviour::Fail(StepFailure::answered(failure_of(
            class, code,
        ))));
        let failure = handle
            .act(&mut prepared, &send, &write, Authority::Write(&token_one))
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
            ),
            "{class}/{code}"
        );
        // Not sent at all: the class stands, and nothing is in doubt.
        scene.behave(Behaviour::Fail(StepFailure::not_sent(failure_of(
            class, code,
        ))));
        let failure = handle
            .act(&mut prepared, &send, &write, Authority::Write(&token_one))
            .await
            .unwrap_err();
        assert_eq!(
            (failure.failure.class(), failure.sent),
            (class, Sent::NotSent)
        );
        // On a step that commits nothing, the class stands as well.
        scene.behave(Behaviour::Fail(StepFailure::answered(failure_of(
            class, code,
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
            (failure.failure.class(), failure.sent),
            (class, Sent::Answered)
        );
    }

    // The system's own verdicts on the committing step stand: it refused
    // the write, or it answered that the record had changed.
    for (class, code) in [
        (FailureClass::Rejected, "example_refused"),
        (FailureClass::Stale, "example_changed"),
    ] {
        scene.behave(Behaviour::Fail(StepFailure::answered(failure_of(
            class, code,
        ))));
        let failure = handle
            .act(&mut prepared, &send, &write, Authority::Write(&token_one))
            .await
            .unwrap_err();
        assert_eq!(
            (failure.failure.class(), failure.sent),
            (class, Sent::Answered)
        );
    }
    assert!(handle.release(prepared, true).await.complete);
}

/// The planted-secret test of rule 4. A driver that forgot to redact
/// returns a registered secret in a value, in a value for reconcile, in a
/// capture's name, in a capture of text, and in a failure. None of it
/// passes the handle.
#[tokio::test]
async fn a_secret_a_driver_returns_does_not_pass_the_handle() {
    const SECRET: &str = "example-token-planted-0148";
    let has_secret = |bytes: &[u8]| {
        bytes
            .windows(SECRET.len())
            .any(|window| window == SECRET.as_bytes())
    };
    let scene = scene_of(TestPack::new("alpha"), Arc::new(OneSecret(SECRET)));
    let (handle, mut prepared) = scene.prepared(None, Posture::Live).await;
    let args = Values::new();
    let read = input(&args, None, None);
    let fetch = scene.step("fetch", false, false);

    // Bytes that are not text cannot be searched, and pass as they are.
    let picture = vec![0xff, 0xfe, 0x00, 0x01];
    scene.behave(Behaviour::Saw(a2m_provider::Observed {
        values: Values::new()
            .with("note", format!("the key is {SECRET}"))
            .with("quantity", 12_i64)
            .with(
                "lines",
                Value::List(vec![Value::Text(format!("{SECRET} again"))]),
            ),
        assigned_key: None,
        for_reconcile: Values::new().with("handle", SECRET),
        captures: vec![
            a2m_provider::Capture {
                name: format!("response-{SECRET}"),
                media_type: "application/json".to_owned(),
                bytes: format!("{{\"token\":\"{SECRET}\"}}").into_bytes(),
            },
            a2m_provider::Capture {
                name: "screen".to_owned(),
                media_type: "image/png".to_owned(),
                bytes: picture.clone(),
            },
        ],
    }));
    let StepOutcome::Done(observed) = handle
        .act(&mut prepared, &fetch, &read, Authority::Read)
        .await
        .unwrap()
    else {
        panic!("done");
    };
    let shown = format!("{observed:?}");
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(
        observed
            .captures
            .iter()
            .all(|capture| !has_secret(&capture.bytes))
    );
    // What is not a secret is as the driver returned it.
    assert_eq!(observed.values.get("quantity"), Some(&Value::Int(12)));
    assert_eq!(
        observed.values.get("note"),
        Some(&Value::Text(format!("the key is {}", a2m_core::REDACTED)))
    );
    assert_eq!(
        observed.captures[0].bytes,
        format!("{{\"token\":\"{}\"}}", a2m_core::REDACTED).into_bytes()
    );
    assert_eq!(observed.captures[1].bytes, picture);

    // A failure's sentence and next step.
    scene.behave(Behaviour::Fail(StepFailure::answered(Failure::new(
        FailureClass::Rejected,
        a2m_core::FailureCode::fixed("example_refused"),
        format!("The system refused the token {SECRET}."),
        format!("Sign in again with {SECRET}."),
    ))));
    let failure = handle
        .act(&mut prepared, &fetch, &read, Authority::Read)
        .await
        .unwrap_err();
    let shown = format!("{failure:?}");
    assert!(!shown.contains(SECRET), "{shown}");
    assert_eq!(failure.failure.code().as_str(), "example_refused");
    assert!(handle.release(prepared, false).await.complete);
}

/// A mode may name the kinds of step that always save. The handle then
/// refuses one sent as a step that commits nothing, whoever built the step:
/// the plan, or a discoverer through the read-only port.
#[tokio::test]
async fn a_step_of_a_kind_that_always_commits_is_refused_when_sent_as_a_read() {
    let declares = manifest("alpha", "request", "").replace(
        "steps = [\"fetch\", \"send\"]",
        "steps = [\"fetch\", \"send\"]\ncommit_steps = [\"send\"]",
    );
    let scene = scene_of(
        TestPack::with_manifest("alpha", declares),
        Arc::new(a2m_core::NoSecrets),
    );
    let (handle, mut prepared) = scene.prepared(None, Posture::Discovery).await;
    let args = Values::new();
    let read = input(&args, None, None);
    let refused = refused_unsent(
        handle
            .act(
                &mut prepared,
                &scene.step("send", false, false),
                &read,
                Authority::Read,
            )
            .await,
    );
    assert_eq!(
        (refused.class(), refused.code()),
        (FailureClass::Policy, &codes::INTENT_MISSING)
    );
    // Through the port a discoverer is given, too.
    let port = ReadOnlyPort::new(handle.clone(), prepared);
    let refused = refused_unsent(
        a2m_provider::SandboxPort::act(&port, &scene.step("send", false, false), &read).await,
    );
    assert_eq!(refused.code(), &codes::INTENT_MISSING);
    assert!(scene.reached().is_empty(), "{:?}", scene.reached());
    // A step of another kind is carried out.
    a2m_provider::SandboxPort::act(&port, &scene.step("fetch", false, false), &read)
        .await
        .unwrap();
    assert_eq!(scene.reached(), ["fetch"]);
    let (handle, prepared) = port.into_parts();
    assert!(handle.release(prepared, false).await.complete);
}

/// "Absent" proves that a record is not there only from steps whose kinds
/// the mode's manifest states in `absence_steps`; by default no kind is
/// stated, and nothing proves absence (decision 0185).
#[tokio::test]
async fn absence_is_proved_only_by_the_kinds_the_manifest_states() {
    let plain = scene();
    let (handle, prepared) = plain.prepared(None, Posture::Live).await;
    assert!(!handle.proves_absence(&[plain.step("fetch", false, true)]));
    assert!(handle.release(prepared, false).await.complete);

    let declares = manifest("alpha", "request", "").replace(
        "steps = [\"fetch\", \"send\"]",
        "steps = [\"fetch\", \"send\"]\nabsence_steps = [\"fetch\"]",
    );
    let scene = scene_of(
        TestPack::with_manifest("alpha", declares),
        Arc::new(a2m_core::NoSecrets),
    );
    let (handle, prepared) = scene.prepared(None, Posture::Live).await;
    let fetch = scene.step("fetch", false, true);
    assert!(handle.proves_absence(std::slice::from_ref(&fetch)));
    // Every step must be of a stated kind, and there must be one.
    assert!(!handle.proves_absence(&[fetch.clone(), scene.step("send", false, false)]));
    assert!(!handle.proves_absence(&[]));
    assert!(handle.release(prepared, false).await.complete);
}

/// A code that states the kernel's own bookkeeping never comes from a
/// driver, also where a step's own `fails_as` would let it pass.
#[tokio::test]
async fn a_driver_cannot_return_a_code_that_is_the_kernels_alone() {
    let scene = scene();
    let (handle, mut prepared) = scene.prepared(None, Posture::Live).await;
    let args = Values::new();
    let read = input(&args, None, None);
    let mut fetch = scene.step("fetch", false, false);
    fetch.fails_as.class = FailureClass::Busy;
    fetch.fails_as.code = a2m_core::FailureCode::fixed("blocked_by_parent");
    scene.behave(Behaviour::Fail(StepFailure::answered(failure_of(
        FailureClass::Busy,
        "blocked_by_parent",
    ))));
    let failure = handle
        .act(&mut prepared, &fetch, &read, Authority::Read)
        .await
        .unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code()),
        (FailureClass::UnknownVariant, &codes::UNDECLARED_CODE)
    );
    // The step's own code, when it is not the kernel's, passes.
    fetch.fails_as.code = a2m_core::FailureCode::fixed("example_busy");
    scene.behave(Behaviour::Fail(StepFailure::answered(failure_of(
        FailureClass::Busy,
        "example_busy",
    ))));
    let failure = handle
        .act(&mut prepared, &fetch, &read, Authority::Read)
        .await
        .unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "example_busy");
    assert!(handle.release(prepared, false).await.complete);
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

/// What a record's earlier steps recorded reaches the driver for the step
/// it is handed with, and for no other (decision 0162).
#[tokio::test]
async fn what_earlier_steps_recorded_reaches_the_driver_for_its_step_alone() {
    let scene = scene();
    let (handle, mut prepared) = scene.prepared(None, Posture::Live).await;
    let args = Values::new();
    let read = input(&args, None, None);
    let fetch = scene.step("fetch", false, false);
    scene.behave(Behaviour::EchoRecorded);
    let recorded = Values::new().with("message_id", "M-7");
    let StepOutcome::Done(observed) = handle
        .act_recorded(&mut prepared, &fetch, &read, Authority::Read, &recorded)
        .await
        .unwrap()
    else {
        panic!("done");
    };
    assert_eq!(observed.values, recorded);
    assert!(prepared.recorded().is_empty());
    let StepOutcome::Done(observed) = handle
        .act(&mut prepared, &fetch, &read, Authority::Read)
        .await
        .unwrap()
    else {
        panic!("done");
    };
    assert!(observed.values.is_empty());
    assert!(handle.release(prepared, false).await.complete);
}

/// How a step was done travels as the contract's own capture, and what the
/// system said is kept apart from the sentence, redacted, through a write
/// in doubt as well (decision 0162).
#[tokio::test]
async fn how_a_step_was_done_and_what_the_system_said_pass_the_handle() {
    const SECRET: &str = "example-token-planted-0162";
    let scene = scene_of(TestPack::new("alpha"), Arc::new(OneSecret(SECRET)));
    let (handle, mut prepared) = scene.prepared(None, Posture::Live).await;
    let args = Values::new();
    let read = input(&args, None, None);
    let fetch = scene.step("fetch", false, false);
    let acting = a2m_telemetry::Acting {
        means: vec![a2m_telemetry::Detail::new("step", "fetch")],
        measured: vec![a2m_telemetry::Detail::new("bytes", "120")],
    };
    scene.behave(Behaviour::Saw(a2m_provider::Observed {
        captures: vec![a2m_provider::Capture::acting(&acting)],
        ..a2m_provider::Observed::default()
    }));
    let StepOutcome::Done(observed) = handle
        .act(&mut prepared, &fetch, &read, Authority::Read)
        .await
        .unwrap()
    else {
        panic!("done");
    };
    assert_eq!(observed.acting(), Some(acting));

    scene.behave(Behaviour::Fail(
        StepFailure::answered(failure_of(FailureClass::Rejected, "example_refused"))
            .with_said(format!("quantity must be positive ({SECRET})")),
    ));
    let failure = handle
        .act(&mut prepared, &fetch, &read, Authority::Read)
        .await
        .unwrap_err();
    assert_eq!(
        failure.failure.said(),
        Some(format!("quantity must be positive ({})", a2m_core::REDACTED).as_str())
    );
    assert!(!failure.failure.message().contains("positive"));
    assert!(handle.release(prepared, false).await.complete);
}
