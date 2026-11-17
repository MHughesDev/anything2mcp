//! The `cli` driver against the command-line test stand, through the
//! driver handle. What a write did is asked of the stand's own files, never
//! of the pack's read path alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    BindingPlan, Digest, Environment, Epoch, FailureClass, NoSecrets, Plan, RecordKey, Redact,
    RunId, Step, Value, Values,
};
use a2m_ledger::{Fence, IntentToken, RecordRef};
use a2m_pack_command::codes;
use a2m_provider::{
    Authority, CheckedPaths, Deadline, Destinations, DriverHandle, Observed, Posture,
    PrepareContext, Prepared, ProgramRef, Registry, Sent, StepFailure, StepInput, StepOutcome,
    SurfaceRef,
};
use common::ledger::{key, record, running, store, token};
use common::{Folder, registry, stand_files};
use example_cli::Build;
use example_cli::faults::{Fault, FaultKind, StopHow, StopPoint, Stream};
use example_cli::harness::Stand;

/// Long enough for a loaded machine; a test waits on a condition, not for
/// this long.
const WITHIN: Duration = Duration::from_secs(60);

/// A redactor that knows one planted secret.
struct Planted(&'static str);

impl Redact for Planted {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, a2m_core::REDACTED)
    }
}

struct Scene {
    folder: Folder,
    stand: Stand,
    registry: Registry,
    plan: Plan,
    surface: SurfaceRef,
    handle: DriverHandle,
}

/// What one step is given, beside its step.
#[derive(Clone, Copy, Default)]
struct With<'a> {
    key: Option<&'a RecordKey>,
    stamp: Option<&'a str>,
    record: Option<RecordRef>,
    token: Option<&'a IntentToken>,
    within: Option<Duration>,
}

impl Scene {
    fn new() -> Self {
        Self::build(Arc::new(NoSecrets), |_| {})
    }

    fn build(redact: Arc<dyn Redact>, change: impl Fn(&mut Vec<(String, String)>)) -> Self {
        let folder = Folder::new("driver");
        let stand = Stand::start().unwrap();
        let registry = registry(&folder, redact);
        let mut files = stand_files(&stand);
        change(&mut files);
        let plan = match common::compile(&files, registry.compiler_set()) {
            Ok(compiled) => compiled.plan,
            Err(problems) => panic!("{problems:#?}"),
        };
        let surface = SurfaceRef {
            target: plan.header.target.clone(),
            environment: Environment::new("test").unwrap(),
            surface: plan.surfaces[0].clone(),
        };
        let handle = registry.driver(&codes::mode()).unwrap();
        Self {
            folder,
            stand,
            registry,
            plan,
            surface,
            handle,
        }
    }

    async fn prepare(
        &self,
        posture: Posture,
        fence: Option<Fence>,
    ) -> Result<Prepared, StepFailure> {
        let fence = fence.unwrap_or(Fence {
            run: RunId::from_bytes([7; 16]),
            epoch: Epoch::new(1),
        });
        // As the engine will: the programs the published plan declares.
        let supervisor = self.handle.supervisor(
            self.surface
                .surface
                .programs
                .iter()
                .map(ProgramRef::from)
                .collect(),
        );
        let cx = PrepareContext {
            run: fence.run,
            epoch: fence.epoch,
            surface: &self.surface,
            plan: &self.plan,
            posture,
            deadline: Deadline::after(WITHIN),
            redact: &NoSecrets,
            destinations: &Destinations::default(),
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        self.handle.prepare(&cx, None).await
    }

    async fn live(&self) -> Prepared {
        self.prepare(Posture::Live, None).await.unwrap()
    }

    fn binding(&self, operation: &str) -> &BindingPlan {
        &self
            .plan
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap_or_else(|| panic!("no operation {operation}"))
            .bindings[0]
    }

    /// Carries out steps in order, as the engine would: it stops at the
    /// first failure and at the first step that reports its record absent.
    async fn run(
        &self,
        prepared: &mut Prepared,
        steps: &[Step],
        args: &Values,
        with: With<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let mut last = StepOutcome::Done(Observed::default());
        for step in steps {
            let input = StepInput {
                args,
                record: with.record,
                key: with.key,
                stamp: with.stamp,
                deadline: Deadline::after(with.within.unwrap_or(WITHIN)),
            };
            let authority = match with.token {
                Some(token) if step.commits => Authority::Write(token),
                _ => Authority::Read,
            };
            last = self.handle.act(prepared, step, &input, authority).await?;
            if matches!(last, StepOutcome::Absent(_)) {
                break;
            }
        }
        Ok(last)
    }

    /// A read: every step of the operation's binding.
    async fn read(&self, operation: &str, args: &Values) -> Result<StepOutcome, StepFailure> {
        let mut prepared = self.live().await;
        let result = self
            .run(
                &mut prepared,
                &self.binding(operation).act,
                args,
                With::default(),
            )
            .await;
        assert!(
            self.handle
                .release(prepared, result.is_err())
                .await
                .complete
        );
        result
    }

    /// The starts the stand recorded, without the version checks.
    fn starts(&self) -> Vec<example_cli::Invocation> {
        self.stand
            .control()
            .invocations()
            .unwrap()
            .into_iter()
            .filter(|start| start.args != ["--version"])
            .collect()
    }
}

fn done(outcome: Result<StepOutcome, StepFailure>) -> Observed {
    match outcome {
        Ok(StepOutcome::Done(observed)) => observed,
        other => panic!("not done: {other:?}"),
    }
}

fn failed(outcome: Result<StepOutcome, StepFailure>) -> StepFailure {
    match outcome {
        Err(failure) => failure,
        Ok(other) => panic!("it did not fail: {other:?}"),
    }
}

fn is(failure: &StepFailure, class: FailureClass, code: &a2m_core::FailureCode, sent: Sent) {
    assert_eq!(
        (
            failure.failure.class(),
            failure.failure.code(),
            failure.sent
        ),
        (class, code, sent),
        "{failure}"
    );
}

fn sku(code: &str) -> Values {
    Values::new().with("sku", code)
}

/// What must be the same in two runs that did the same thing.
fn signature(observed: &Observed) -> Digest {
    let mut bytes = observed.values.canonical_bytes();
    for capture in &observed.captures {
        bytes.extend_from_slice(capture.name.as_bytes());
        bytes.extend_from_slice(&capture.bytes);
    }
    Digest::of(&bytes)
}

// ---- reads: C1, C2, C11, C14 ----

#[tokio::test(flavor = "multi_thread")]
async fn each_output_form_is_read_and_five_runs_of_one_read_are_the_same() {
    let scene = Scene::new();
    // JSON: typed values, with text that is not ASCII and text aimed at an agent kept as data.
    let item = done(scene.read("example_item_get", &sku("SK-1003")).await);
    assert_eq!(item.values.get("quantity"), Some(&Value::Int(15)));
    assert_eq!(
        item.values.get("name"),
        Some(&Value::from("Étiquette « fragile »"))
    );
    assert!(
        item.values
            .get("note")
            .is_some_and(|note| note.as_text().is_some())
    );

    // A table: rows of text by column.
    let listed = done(scene.read("example_item_list", &Values::new()).await);
    let rows = listed.values.get("rows").unwrap().as_list().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].as_record().unwrap().get("sku"),
        Some(&Value::from("SK-1001"))
    );
    let at = done(
        scene
            .read("example_item_list", &Values::new().with("location", "B01"))
            .await,
    );
    assert_eq!(at.values.get("rows").unwrap().as_list().unwrap().len(), 1);

    // The program was started with exactly these elements, only the
    // declared variables, and a fresh folder of the run's own, now gone.
    let starts = scene.starts();
    assert_eq!(starts[0].args, ["item", "show", "SK-1003", "--format=json"]);
    assert_eq!(starts[2].args, ["item", "list", "--location", "B01"]);
    for declared in [
        "ANYTHING2MCP_OWNER_MARK",
        "EXAMPLE_CLI_HOME",
        "LC_ALL",
        "TZ",
    ] {
        assert!(starts[0].env.contains_key(declared), "{declared}");
    }
    let undeclared: Vec<&String> = starts[0]
        .env
        .keys()
        .filter(|name| {
            ![
                "ANYTHING2MCP_OWNER_MARK",
                "EXAMPLE_CLI_HOME",
                "LC_ALL",
                "TZ",
            ]
            .contains(&name.as_str())
                && !name.eq_ignore_ascii_case("SystemRoot")
        })
        .collect();
    assert!(undeclared.is_empty(), "{undeclared:?}");
    let runs = scene.folder.path().join("runs");
    let folder_of = |cwd: &str| {
        std::path::Path::new(cwd)
            .parent()
            .and_then(std::path::Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
    };
    assert_eq!(folder_of(&starts[0].cwd).as_deref(), Some("runs"));
    assert_ne!(starts[0].cwd, starts[1].cwd);
    assert_eq!(std::fs::read_dir(&runs).unwrap().count(), 0);

    // An item that is not there is absent, by its declared exit status.
    let absent = scene.read("example_item_get", &sku("SK-9999")).await;
    assert!(matches!(absent, Ok(StepOutcome::Absent(_))), "{absent:?}");

    // Five runs of one read: one signature.
    let mut signatures = Vec::new();
    for _ in 0..5 {
        let mut prepared = scene.live().await;
        let mut all = Observed::default();
        for step in &scene.binding("example_item_get").act {
            let one = done(
                scene
                    .run(
                        &mut prepared,
                        std::slice::from_ref(step),
                        &sku("SK-1001"),
                        With::default(),
                    )
                    .await,
            );
            all.captures.extend(one.captures);
            all.values = one.values;
        }
        scene.handle.release(prepared, false).await;
        signatures.push(signature(&all));
    }
    signatures.dedup();
    assert_eq!(signatures.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn arguments_arrive_exactly_and_no_input_becomes_an_option() {
    let scene = Scene::new();
    let text = "a b; $(reboot) `x` \"q\" 'r' | > é 日本\n--yes %PATH% ^& *";
    done(
        scene
            .read("example_echo_text", &Values::new().with("text", text))
            .await,
    );
    let dashed = "--home=/elsewhere";
    done(
        scene
            .read("example_echo_text", &Values::new().with("text", dashed))
            .await,
    );
    let starts = scene.starts();
    assert_eq!(
        starts[0].args,
        ["echo", &format!("--text={text}"), "--", text]
    );
    assert_eq!(
        starts[1].args,
        ["echo", &format!("--text={dashed}"), "--", dashed]
    );

    // A value that breaks its param's pattern is refused, and nothing starts.
    for bad in ["SK-1001 --yes", "--help", "SK-1001\nitem remove", ""] {
        let refused = failed(scene.read("example_item_get", &sku(bad)).await);
        is(
            &refused,
            FailureClass::Input,
            &codes::SCHEMA_INVALID,
            Sent::NotSent,
        );
        assert!(!refused.failure.message().contains("--yes"));
    }
    assert_eq!(scene.starts().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_output_is_read_and_a_ceiling_a_time_limit_and_a_child_are_held() {
    let scene = Scene::new();
    // Rows on one stream and progress on the other, a pause after each.
    let report = Values::new().with("rows", 6_i64).with("pause", 20_i64);
    let rows = done(scene.read("example_stock_report", &report).await);
    assert_eq!(rows.values.get("rows").unwrap().as_list().unwrap().len(), 6);

    // More than the command's ceiling on standard output.
    scene
        .stand
        .control()
        .set_fault(&Fault::once(FaultKind::Flood {
            stream: Stream::Stdout,
            bytes: 200_000,
        }))
        .unwrap();
    let over = failed(scene.read("example_stock_report", &report).await);
    is(
        &over,
        FailureClass::Policy,
        &codes::LIMIT_EXCEEDED,
        Sent::Answered,
    );
    assert!(over.failure.message().contains("limits.stdout"), "{over}");

    // A read that outlives its time, with a child that would outlive it:
    // the whole group is ended, and a read may be tried again.
    scene
        .stand
        .control()
        .set_fault(&Fault::once(FaultKind::SpawnChild {
            lifetime_ms: 600_000,
        }))
        .unwrap();
    let mut prepared = scene.live().await;
    let endless = Values::new()
        .with("rows", 100_000_i64)
        .with("pause", 200_i64);
    let late = failed(
        scene
            .run(
                &mut prepared,
                &scene.binding("example_stock_report").act,
                &endless,
                With {
                    within: Some(Duration::from_secs(2)),
                    ..With::default()
                },
            )
            .await,
    );
    is(
        &late,
        FailureClass::Transient,
        &codes::TIMEOUT,
        Sent::Unknown,
    );
    scene.handle.release(prepared, true).await;
    scene
        .stand
        .control()
        .wait_until_no_children(WITHIN)
        .unwrap();
    assert!(scene.registry.supervisor().leftovers().is_empty());

    // A program that exits and leaves a child: the child is ended too.
    scene
        .stand
        .control()
        .set_fault(&Fault::once(FaultKind::SpawnChild {
            lifetime_ms: 600_000,
        }))
        .unwrap();
    done(scene.read("example_item_list", &Values::new()).await);
    scene
        .stand
        .control()
        .wait_until_no_children(WITHIN)
        .unwrap();
}

// ---- what a read's failure is: C9, C10, drift, exits ----

#[tokio::test(flavor = "multi_thread")]
async fn a_read_that_cannot_be_done_as_written_fails_by_name() {
    let scene = Scene::new();
    let fault = |kind: FaultKind| scene.stand.control().set_fault(&Fault::once(kind)).unwrap();
    let one = sku("SK-1001");
    let get = || scene.read("example_item_get", &one);

    fault(FaultKind::prompt());
    let asked = failed(get().await);
    is(
        &asked,
        FailureClass::UnknownVariant,
        &codes::UNEXPECTED_PROMPT,
        Sent::Answered,
    );

    fault(FaultKind::stop(StopPoint::Start, StopHow::Exit { code: 9 }));
    let undeclared = failed(get().await);
    is(
        &undeclared,
        FailureClass::UnknownVariant,
        &codes::UNEXPECTED_EXIT_STATUS,
        Sent::Answered,
    );

    fault(FaultKind::stop(StopPoint::Start, StopHow::Abort));
    let aborted = failed(get().await);
    is(
        &aborted,
        FailureClass::UnknownVariant,
        &codes::UNEXPECTED_EXIT_STATUS,
        Sent::Answered,
    );

    fault(FaultKind::Drift);
    let drifted = failed(get().await);
    is(
        &drifted,
        FailureClass::Drift,
        &codes::OUTPUT_UNPARSED,
        Sent::Answered,
    );
    fault(FaultKind::Drift);
    let drifted = failed(scene.read("example_item_list", &Values::new()).await);
    is(
        &drifted,
        FailureClass::Drift,
        &codes::OUTPUT_UNPARSED,
        Sent::Answered,
    );

    fault(FaultKind::stop(
        StopPoint::Start,
        StopHow::Exit { code: 75 },
    ));
    let later = failed(get().await);
    is(
        &later,
        FailureClass::Transient,
        &codes::UPSTREAM_UNAVAILABLE,
        Sent::Answered,
    );

    fault(FaultKind::RequireSession);
    let signed_out = failed(get().await);
    is(
        &signed_out,
        FailureClass::SessionExpired,
        &codes::NEEDS_LOGIN,
        Sent::Answered,
    );
}

// ---- prepare: C12 ----

#[tokio::test(flavor = "multi_thread")]
async fn another_version_and_another_build_are_refused_before_anything_runs() {
    let scene = Scene::new();
    scene
        .stand
        .control()
        .set_fault(&Fault::once(FaultKind::ReportVersion {
            version: "9.9.9".to_owned(),
        }))
        .unwrap();
    let version = scene.prepare(Posture::Live, None).await.unwrap_err();
    assert_eq!(
        (version.failure.class(), version.failure.code()),
        (FailureClass::Drift, &codes::PROGRAM_VERSION_UNSUPPORTED)
    );

    // The same program rebuilt: other bytes at the same path. A run that
    // was prepared before does not start it either.
    let mut prepared = scene.live().await;
    scene.stand.replace_program(Build::Rebuilt).unwrap();
    let before = scene.stand.control().invocations().unwrap().len();
    let changed = failed(
        scene
            .run(
                &mut prepared,
                &scene.binding("example_item_get").act,
                &sku("SK-1001"),
                With::default(),
            )
            .await,
    );
    is(
        &changed,
        FailureClass::Drift,
        &codes::PROGRAM_DIGEST_CHANGED,
        Sent::NotSent,
    );
    scene.handle.release(prepared, true).await;
    let rebuilt = scene.prepare(Posture::Live, None).await.unwrap_err();
    is(
        &rebuilt,
        FailureClass::Drift,
        &codes::PROGRAM_DIGEST_CHANGED,
        Sent::NotSent,
    );
    assert_eq!(scene.stand.control().invocations().unwrap().len(), before);

    // A program that is not there.
    std::fs::remove_file(scene.stand.program()).unwrap();
    let missing = scene.prepare(Posture::Live, None).await.unwrap_err();
    is(
        &missing,
        FailureClass::Drift,
        &codes::PROGRAM_MISSING,
        Sent::NotSent,
    );
}

// ---- writes: C3 to C8, and the faults of every write ----

/// A scene with one write run: its fence, and the token of its one record.
async fn writing(name: &str) -> (Fence, IntentToken) {
    let store = store().await;
    let fence = running(&store, &[name]).await;
    let token = token(&store, fence, 0, name).await;
    (fence, token)
}

fn set(code: &str, quantity: i64) -> Values {
    sku(code).with("quantity", quantity)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_is_made_once_and_read_back_and_a_stored_value_that_differs_is_seen() {
    let scene = Scene::new();
    let control = scene.stand.control();
    let binding = scene.binding("example_item_set_quantity");
    let (fence, token) = writing("SK-1001").await;
    let record_key = key("SK-1001");
    let mut prepared = scene.prepare(Posture::Live, Some(fence)).await.unwrap();
    let with = With {
        key: Some(&record_key),
        record: Some(record(&fence, 0)),
        token: Some(&token),
        ..With::default()
    };

    // Without the ledger's token the handle refuses, and nothing starts.
    let no_token = failed(
        scene
            .run(
                &mut prepared,
                &binding.act,
                &set("SK-1001", 12),
                With {
                    token: None,
                    ..with
                },
            )
            .await,
    );
    is(
        &no_token,
        FailureClass::Policy,
        &a2m_provider::codes::INTENT_MISSING,
        Sent::NotSent,
    );
    assert!(scene.starts().is_empty());

    // The write, its confirmation, and its read-back.
    done(
        scene
            .run(&mut prepared, &binding.act, &set("SK-1001", 12), with)
            .await,
    );
    let confirmed = done(
        scene
            .run(&mut prepared, &binding.confirm, &Values::new(), with)
            .await,
    );
    assert_eq!(confirmed.values.get("quantity"), Some(&Value::from("12")));
    let read = done(
        scene
            .run(&mut prepared, &binding.read_back, &sku("SK-1001"), with)
            .await,
    );
    assert_eq!(read.values.get("quantity"), Some(&Value::Int(12)));
    // The stand's own file says the same, and that it was written once.
    assert_eq!(
        control.stored_item("SK-1001").unwrap().unwrap().quantity,
        12
    );
    assert_eq!(control.write_count("SK-1001").unwrap(), 1);
    assert_eq!(
        scene.starts()[0].args,
        ["item", "set", "SK-1001", "--quantity=12", "--yes"]
    );

    // The program stores another value than it reports: the read-back
    // gives what is stored, for the engine to compare.
    control
        .set_fault(&Fault::once(FaultKind::RoundQuantity { multiple: 10 }))
        .unwrap();
    done(
        scene
            .run(&mut prepared, &binding.act, &set("SK-1001", 17), with)
            .await,
    );
    let reported = done(
        scene
            .run(&mut prepared, &binding.confirm, &Values::new(), with)
            .await,
    );
    assert_eq!(reported.values.get("quantity"), Some(&Value::from("17")));
    let stored = done(
        scene
            .run(&mut prepared, &binding.read_back, &sku("SK-1001"), with)
            .await,
    );
    assert_eq!(stored.values.get("quantity"), Some(&Value::Int(10)));
    assert_eq!(
        control.stored_item("SK-1001").unwrap().unwrap().quantity,
        10
    );

    // A refusal by the program's own rule: named, with its message marked
    // as untrusted, and nothing written.
    let writes = control.writes().unwrap().len();
    done(
        scene
            .run(&mut prepared, &binding.act, &set("SK-1001", 99_999), with)
            .await,
    );
    let rejected = failed(
        scene
            .run(&mut prepared, &binding.confirm, &Values::new(), with)
            .await,
    );
    is(
        &rejected,
        FailureClass::Rejected,
        &codes::APP_VALIDATION_ERROR,
        Sent::Answered,
    );
    assert!(
        rejected.failure.message().contains("untrusted text"),
        "{rejected}"
    );
    // Another start holds the program's lock: busy, and nothing written.
    let held = control.hold_write_lock().unwrap();
    done(
        scene
            .run(&mut prepared, &binding.act, &set("SK-1001", 5), with)
            .await,
    );
    let busy = failed(
        scene
            .run(&mut prepared, &binding.confirm, &Values::new(), with)
            .await,
    );
    is(
        &busy,
        FailureClass::Busy,
        &codes::PROGRAM_BUSY,
        Sent::Answered,
    );
    drop(held);
    assert_eq!(control.writes().unwrap().len(), writes);
    assert!(scene.handle.release(prepared, false).await.complete);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_whose_result_nothing_confirms_is_a_possible_commit_and_is_not_made_twice() {
    let scene = Scene::new();
    let control = scene.stand.control();
    let binding = scene.binding("example_item_set_quantity");
    let (fence, token) = writing("SK-1002").await;
    let record_key = key("SK-1002");
    let mut prepared = scene.prepare(Posture::Live, Some(fence)).await.unwrap();
    let with = With {
        key: Some(&record_key),
        record: Some(record(&fence, 0)),
        token: Some(&token),
        ..With::default()
    };
    let possible = |failure: &StepFailure| {
        is(
            failure,
            FailureClass::PossibleCommit,
            &codes::NO_CONFIRMATION,
            Sent::Unknown,
        );
    };

    // The program commits, then hangs until it is killed at its time limit.
    control
        .set_fault(&Fault::once(FaultKind::stop(
            StopPoint::AfterCommit,
            StopHow::Hang,
        )))
        .unwrap();
    let hung = failed(
        scene
            .run(
                &mut prepared,
                &binding.act,
                &set("SK-1002", 7),
                With {
                    within: Some(Duration::from_secs(3)),
                    ..with
                },
            )
            .await,
    );
    possible(&hung);
    assert_eq!(control.wait_until_hung(1, WITHIN).unwrap().len(), 1);
    // The driver started it once and did not try again; the write is
    // there, and reading the record back finds it.
    assert_eq!(scene.starts().len(), 1);
    assert_eq!(control.write_count("SK-1002").unwrap(), 1);
    let found = done(
        scene
            .run(&mut prepared, &binding.read_back, &sku("SK-1002"), with)
            .await,
    );
    assert_eq!(found.values.get("quantity"), Some(&Value::Int(7)));

    // Killed before the commit: the same answer, and the stand shows that
    // nothing was written. Only the read-back can tell the two apart.
    control
        .set_fault(&Fault::once(FaultKind::stop(
            StopPoint::BeforeCommit,
            StopHow::Abort,
        )))
        .unwrap();
    done(
        scene
            .run(&mut prepared, &binding.act, &set("SK-1002", 8), with)
            .await,
    );
    possible(&failed(
        scene
            .run(&mut prepared, &binding.confirm, &Values::new(), with)
            .await,
    ));
    assert_eq!(control.stored_item("SK-1002").unwrap().unwrap().quantity, 7);

    // An undeclared status after the commit, changed output, and a
    // question: each leaves the write unconfirmed.
    control
        .set_fault(&Fault::once(FaultKind::stop(
            StopPoint::AfterCommit,
            StopHow::Exit { code: 9 },
        )))
        .unwrap();
    done(
        scene
            .run(&mut prepared, &binding.act, &set("SK-1002", 9), with)
            .await,
    );
    possible(&failed(
        scene
            .run(&mut prepared, &binding.confirm, &Values::new(), with)
            .await,
    ));
    assert_eq!(control.stored_item("SK-1002").unwrap().unwrap().quantity, 9);

    control.set_fault(&Fault::once(FaultKind::Drift)).unwrap();
    done(
        scene
            .run(&mut prepared, &binding.act, &set("SK-1002", 10), with)
            .await,
    );
    possible(&failed(
        scene
            .run(&mut prepared, &binding.confirm, &Values::new(), with)
            .await,
    ));

    control
        .set_fault(&Fault::once(FaultKind::prompt()))
        .unwrap();
    let writes = control.write_count("SK-1002").unwrap();
    done(
        scene
            .run(&mut prepared, &binding.act, &set("SK-1002", 11), with)
            .await,
    );
    possible(&failed(
        scene
            .run(&mut prepared, &binding.confirm, &Values::new(), with)
            .await,
    ));
    assert_eq!(control.write_count("SK-1002").unwrap(), writes);
    scene.handle.release(prepared, true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_created_record_is_found_by_its_assigned_key_and_by_its_stamp() {
    let scene = Scene::new();
    let control = scene.stand.control();
    let binding = scene.binding("example_item_add");
    let (fence, token) = writing("new").await;
    let mut prepared = scene.prepare(Posture::Live, Some(fence)).await.unwrap();
    let with = With {
        stamp: Some("a2m:run-1:0"),
        record: Some(record(&fence, 0)),
        token: Some(&token),
        ..With::default()
    };
    let item = Values::new()
        .with("name", "Spring washer")
        .with("location", "A02");

    // Nothing carries the stamp yet.
    let before = scene
        .run(&mut prepared, &binding.lookup, &Values::new(), with)
        .await;
    assert!(matches!(before, Ok(StepOutcome::Absent(_))), "{before:?}");

    done(scene.run(&mut prepared, &binding.act, &item, with).await);
    let created = done(
        scene
            .run(&mut prepared, &binding.confirm, &Values::new(), with)
            .await,
    );
    // The key the program assigned is taken at once.
    let assigned = created.assigned_key.clone().unwrap();
    assert_eq!(assigned, key("SK-1004"));
    let stored = control.stored_item("SK-1004").unwrap().unwrap();
    assert_eq!(
        (stored.name.as_str(), stored.location.as_str()),
        ("Spring washer", "A02")
    );
    assert_eq!(stored.reference.as_deref(), Some("a2m:run-1:0"));
    assert_eq!(control.write_count("SK-1004").unwrap(), 1);

    // The read-back takes the assigned key; the lookup takes the stamp.
    let read = done(
        scene
            .run(
                &mut prepared,
                &binding.read_back,
                &Values::new(),
                With {
                    key: Some(&assigned),
                    token: None,
                    record: None,
                    ..with
                },
            )
            .await,
    );
    assert_eq!(read.values.get("name"), Some(&Value::from("Spring washer")));
    let found = done(
        scene
            .run(&mut prepared, &binding.lookup, &Values::new(), with)
            .await,
    );
    assert_eq!(found.values.get("sku"), Some(&Value::from("SK-1004")));

    // The program makes a duplicate when the same add runs twice. If that
    // ever happened, the lookup says so and matches neither.
    done(scene.run(&mut prepared, &binding.act, &item, with).await);
    let twice = failed(
        scene
            .run(&mut prepared, &binding.lookup, &Values::new(), with)
            .await,
    );
    is(
        &twice,
        FailureClass::PossibleCommit,
        &codes::AMBIGUOUS_MATCH,
        Sent::Answered,
    );

    // A removed record reads back as absent.
    let remove = scene.binding("example_item_remove");
    let (fence, token) = writing("SK-1004").await;
    let mut removing = scene.prepare(Posture::Live, Some(fence)).await.unwrap();
    let with = With {
        key: Some(&assigned),
        record: Some(record(&fence, 0)),
        token: Some(&token),
        ..With::default()
    };
    done(
        scene
            .run(&mut removing, &remove.act, &sku("SK-1004"), with)
            .await,
    );
    done(
        scene
            .run(&mut removing, &remove.confirm, &Values::new(), with)
            .await,
    );
    let gone = scene
        .run(&mut removing, &remove.read_back, &sku("SK-1004"), with)
        .await;
    assert!(matches!(gone, Ok(StepOutcome::Absent(_))), "{gone:?}");
    assert!(control.stored_item("SK-1004").unwrap().is_none());
    scene.handle.release(prepared, false).await;
    scene.handle.release(removing, false).await;
}

// ---- secrets and untrusted output: C13 ----

/// Whether a text holds eight characters in a row of a secret.
fn holds_part_of(text: &str, secret: &str) -> bool {
    secret
        .as_bytes()
        .windows(8)
        .any(|part| text.contains(&*String::from_utf8_lossy(part)))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_the_program_prints_reaches_no_value_capture_or_failure() {
    const SECRET: &str = "secret-for-tests-7731";
    let scene = Scene::build(Arc::new(Planted(SECRET)), |_| {});
    // The stand prints the made-up secret on both streams, as a program
    // that was given a credential might.
    for stream in [Stream::Stdout, Stream::Stderr] {
        scene
            .stand
            .control()
            .set_fault(&Fault::once(FaultKind::Say {
                stream,
                text: format!("debug: token={SECRET}\n"),
            }))
            .unwrap();
    }
    let mut prepared = scene.live().await;
    let steps = &scene.binding("example_item_get").act;
    let spawned = done(
        scene
            .run(&mut prepared, &steps[..1], &sku("SK-1001"), With::default())
            .await,
    );
    assert_eq!(spawned.captures.len(), 2);
    for capture in &spawned.captures {
        let printed = String::from_utf8_lossy(&capture.bytes);
        assert!(printed.contains(a2m_core::REDACTED), "{printed}");
        assert!(!holds_part_of(&printed, SECRET), "{printed}");
    }
    // What it printed around its output is not the declared form: the
    // read stops, and its failure quotes nothing.
    let unparsed = failed(
        scene
            .run(&mut prepared, &steps[1..], &Values::new(), With::default())
            .await,
    );
    is(
        &unparsed,
        FailureClass::Drift,
        &codes::OUTPUT_UNPARSED,
        Sent::Answered,
    );
    assert!(!holds_part_of(&format!("{unparsed:?}"), SECRET));
    scene.handle.release(prepared, true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_on_standard_error_that_a_cut_would_divide_reaches_no_failure() {
    // Longer than what a failure quotes of standard error is cut to.
    const SECRET: &str = "secret-for-tests-0123456789-abcdefghijklmnopqrstuvwxyz-9876543210";
    let scene = Scene::build(Arc::new(Planted(SECRET)), |_| {});
    let control = scene.stand.control();
    let binding = scene.binding("example_item_set_quantity");
    let (fence, token) = writing("SK-1001").await;
    let record_key = key("SK-1001");
    let with = With {
        key: Some(&record_key),
        record: Some(record(&fence, 0)),
        token: Some(&token),
        ..With::default()
    };
    // The program refuses the value by a rule of its own, with a declared
    // exit, so its own message is quoted in the failure.
    let rejected = |said: String| {
        let scene = &scene;
        async move {
            scene
                .stand
                .control()
                .set_fault(&Fault::once(FaultKind::Say {
                    stream: Stream::Stderr,
                    text: said,
                }))
                .unwrap();
            let mut prepared = scene.prepare(Posture::Live, Some(fence)).await.unwrap();
            let spawned = done(
                scene
                    .run(&mut prepared, &binding.act, &set("SK-1001", 99_999), with)
                    .await,
            );
            let refused = failed(
                scene
                    .run(&mut prepared, &binding.confirm, &Values::new(), with)
                    .await,
            );
            scene.handle.release(prepared, true).await;
            (spawned, refused)
        }
    };

    // One hundred characters, then the secret: the cut falls inside it.
    for filler in [100, 105, 60] {
        let (spawned, refused) = rejected(format!("{}{SECRET} ", "x".repeat(filler))).await;
        is(
            &refused,
            FailureClass::Rejected,
            &codes::APP_VALIDATION_ERROR,
            Sent::Answered,
        );
        let shown = format!("{refused:?}");
        assert!(shown.contains("untrusted text"), "{shown}");
        assert!(!holds_part_of(&shown, SECRET), "{filler}: {shown}");
        for capture in &spawned.captures {
            let printed = String::from_utf8_lossy(&capture.bytes);
            assert!(!holds_part_of(&printed, SECRET), "{printed}");
        }
    }
    // The secret within what is quoted is named as redacted, not dropped.
    let (_, refused) = rejected(format!("token {SECRET} ")).await;
    assert!(
        refused.failure.message().contains(a2m_core::REDACTED),
        "{refused}"
    );

    // A control character inside the secret: taking it out to clean the
    // message joins the two halves, and the whole is redacted again.
    let (first, second) = SECRET.split_at(20);
    let (_, refused) = rejected(format!("token {first}\r{second} ")).await;
    let shown = format!("{refused:?}");
    assert!(!shown.contains(SECRET), "{shown}");

    // Nothing was written by any of it.
    assert_eq!(control.write_count("SK-1001").unwrap(), 0);
}

// ---- a plan that does not hold together ----

#[tokio::test(flavor = "multi_thread")]
async fn a_command_that_writes_is_started_only_by_the_step_that_commits() {
    let scene = Scene::new();
    let control = scene.stand.control();
    let binding = scene.binding("example_item_set_quantity");
    let mut prepared = scene.live().await;
    let not_usable = |failure: &StepFailure| {
        is(
            failure,
            FailureClass::Input,
            &codes::STEP_NOT_USABLE,
            Sent::NotSent,
        );
    };

    // The write's own step, with its mark as a commit point taken off, so
    // that the handle asks for no token: the driver itself refuses.
    let mut unmarked = binding.act[0].clone();
    unmarked.commits = false;
    not_usable(&failed(
        scene
            .run(
                &mut prepared,
                &[unmarked],
                &set("SK-1001", 12),
                With::default(),
            )
            .await,
    ));
    // A read-back step that names the command that writes.
    let mut forged = binding.read_back[0].clone();
    forged.refers_to.clone_from(&binding.act[0].refers_to);
    not_usable(&failed(
        scene
            .run(
                &mut prepared,
                &[forged],
                &set("SK-1001", 12),
                With::default(),
            )
            .await,
    ));
    // The stand: nothing was started, and the item is as it was.
    assert!(scene.starts().is_empty());
    assert_eq!(control.write_count("SK-1001").unwrap(), 0);
    assert_eq!(
        control.stored_item("SK-1001").unwrap().unwrap().quantity,
        120
    );

    // A step that reads how a start ended never reads an earlier start: a
    // start that was refused leaves nothing behind for it.
    let get = &scene.binding("example_item_get").act;
    done(
        scene
            .run(&mut prepared, get, &sku("SK-1001"), With::default())
            .await,
    );
    failed(
        scene
            .run(&mut prepared, &get[..1], &sku("no code"), With::default())
            .await,
    );
    not_usable(&failed(
        scene
            .run(&mut prepared, &get[1..], &Values::new(), With::default())
            .await,
    ));
    assert_eq!(scene.starts().len(), 1);
    scene.handle.release(prepared, true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn in_the_sandbox_the_guard_lifts_only_for_the_step_that_commits() {
    let scene = Scene::new();
    let control = scene.stand.control();
    let binding = scene.binding("example_item_set_quantity");
    let (fence, token) = writing("SK-1001").await;
    let record_key = key("SK-1001");
    let with = With {
        key: Some(&record_key),
        record: Some(record(&fence, 0)),
        token: Some(&token),
        ..With::default()
    };
    let unchanged = |scene: &Scene| {
        assert!(scene.starts().is_empty());
        assert_eq!(control.write_count("SK-1001").unwrap(), 0);
        assert_eq!(
            control.stored_item("SK-1001").unwrap().unwrap().quantity,
            120
        );
    };

    // The context that writes a declared test record: a step that would
    // change the item and is not marked `commits` is refused, and the
    // stand shows nothing started and nothing changed.
    let mut writing_context = scene
        .prepare(Posture::Sandbox { test_records: true }, Some(fence))
        .await
        .unwrap();
    let mut unmarked = binding.act[0].clone();
    unmarked.commits = false;
    let refused = failed(
        scene
            .run(&mut writing_context, &[unmarked], &set("SK-1001", 12), with)
            .await,
    );
    assert_eq!(refused.sent, Sent::NotSent, "{refused}");
    unchanged(&scene);

    // Any other sandbox context keeps the guard for the step that commits.
    let mut guarded = scene
        .prepare(
            Posture::Sandbox {
                test_records: false,
            },
            Some(fence),
        )
        .await
        .unwrap();
    let guarded_write = failed(
        scene
            .run(&mut guarded, &binding.act, &set("SK-1001", 12), with)
            .await,
    );
    is(
        &guarded_write,
        FailureClass::Policy,
        &codes::READ_ONLY_POSTURE,
        Sent::NotSent,
    );
    unchanged(&scene);
    scene.handle.release(guarded, false).await;

    // The step that commits, in the context that writes: made once, and
    // read back.
    done(
        scene
            .run(
                &mut writing_context,
                &binding.act,
                &set("SK-1001", 12),
                with,
            )
            .await,
    );
    done(
        scene
            .run(&mut writing_context, &binding.confirm, &Values::new(), with)
            .await,
    );
    let read = done(
        scene
            .run(
                &mut writing_context,
                &binding.read_back,
                &sku("SK-1001"),
                with,
            )
            .await,
    );
    assert_eq!(read.values.get("quantity"), Some(&Value::Int(12)));
    assert_eq!(control.write_count("SK-1001").unwrap(), 1);
    assert!(scene.handle.release(writing_context, false).await.complete);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_is_given_a_folder_nothing_was_left_in() {
    let scene = Scene::new();
    // What an earlier engine with the same process number left behind
    // under the name this run's first folder would take.
    let fence = Fence {
        run: RunId::from_bytes([7; 16]),
        epoch: Epoch::new(1),
    };
    let run: String = fence
        .run
        .to_string()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let left = scene
        .folder
        .path()
        .join("runs")
        .join(format!("{run}-{}-1", std::process::id()));
    std::fs::create_dir_all(&left).unwrap();
    std::fs::write(left.join("left-behind"), "from an earlier attempt").unwrap();

    done(
        scene
            .read("example_echo_text", &Values::new().with("text", "plain"))
            .await,
    );
    let started_in = std::path::PathBuf::from(&scene.starts()[0].cwd);
    assert_ne!(started_in.file_name(), left.file_name());
    assert_eq!(
        started_in.parent().and_then(std::path::Path::file_name),
        left.parent().and_then(std::path::Path::file_name)
    );
    // The run's own folder is gone; what was left behind was not touched.
    assert!(!started_in.exists());
    assert_eq!(
        std::fs::read_to_string(left.join("left-behind")).unwrap(),
        "from an earlier attempt"
    );
}

// ---- discovery and drafting ----

struct Found(Vec<a2m_core::Finding>);

impl a2m_provider::FindingSink for Found {
    fn found(&mut self, finding: a2m_core::Finding) -> Result<(), a2m_core::Failure> {
        self.0.push(finding);
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_runs_only_the_declared_forms_and_drafts_commands_it_never_tries() {
    use a2m_core::{Archetype, EvidenceLevel, LimitationCode, SurfaceKind};
    use a2m_provider::{DescribeForm, DiscoveryScope, ReadOnlyPort, SurfaceDescription};

    let mut scene = Scene::new();
    let control = scene.stand.control().clone();
    let seed = control.stored_items().unwrap();

    // Where nothing may be written, a command is not started at all: a
    // program cannot be made unable to write.
    let mut prepared = scene.prepare(Posture::Discovery, None).await.unwrap();
    let get = scene.binding("example_item_get").act.clone();
    let refused = failed(
        scene
            .run(&mut prepared, &get, &sku("SK-1001"), With::default())
            .await,
    );
    is(
        &refused,
        FailureClass::Policy,
        &codes::READ_ONLY_POSTURE,
        Sent::NotSent,
    );

    let description = SurfaceDescription {
        kind: SurfaceKind::new("program").unwrap(),
        archetype: Archetype::Command,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverers = scene.registry.discoverers_for(&description);
    assert_eq!(discoverers.len(), 1);
    let port = ReadOnlyPort::new(scene.handle.clone(), prepared);
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverers[0].declared().bounds,
        deadline: Deadline::after(WITHIN),
        paths: &CheckedPaths::none(),
        redact: &NoSecrets,
        focus: &Values::new(),
    };
    let mut found = Found(Vec::new());
    let report = discoverers[0]
        .discover(&port, &scope, &mut found)
        .await
        .unwrap();
    assert!(report.bounds_hit.is_empty(), "{report:?}");
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;

    // One finding for each command the help lists: seen, never run.
    assert_eq!(found.0.len(), 9);
    for finding in &found.0 {
        assert_eq!(finding.level(), EvidenceLevel::Discovered);
        assert!(
            finding
                .limitations()
                .iter()
                .any(|limitation| limitation.code == LimitationCode::NotExercised)
        );
    }
    assert!(
        found
            .0
            .iter()
            .any(|finding| finding.found().contains("\"name\":\"item set\""))
    );
    let described = discoverers[0].describe(&found.0, DescribeForm::Summary);
    assert!(described.text.contains("9 commands"), "{}", described.text);
    assert_eq!(described.untrusted.len(), 9);

    // The stand: only the declared help and version forms were started,
    // with a bare environment, and nothing was written or changed.
    let starts = control.invocations().unwrap();
    assert!(starts.len() >= 11, "{}", starts.len());
    for start in &starts {
        assert!(
            matches!(start.args[0].as_str(), "--version" | "--help" | "help"),
            "{:?}",
            start.args
        );
        assert!(
            !start.env.contains_key("EXAMPLE_CLI_HOME"),
            "{:?}",
            start.env
        );
    }
    assert!(control.writes().unwrap().is_empty());
    assert_eq!(control.stored_items().unwrap(), seed);

    // The drafter writes one file for each command, by rule: the digest
    // and version seen, and an open question wherever the program did not
    // say. It writes no effect, and tries nothing.
    let target = Folder::new("draft");
    for (path, text) in stand_files(&scene.stand) {
        if path == "target.yaml" || path == "types.yaml" {
            std::fs::write(target.path().join(&path), text).unwrap();
        }
    }
    let root = a2m_files::TargetRoot::new(target.path());
    let set = scene.registry.compiler_set();
    let drafted = a2m_files::Drafter::new(set, &root).draft(&found.0).unwrap();
    assert_eq!(drafted.written.len(), 9, "{drafted:?}");
    assert!(
        drafted.refused.is_empty() && drafted.conflicts.is_empty(),
        "{drafted:?}"
    );
    assert!(drafted.not_covered.is_empty());
    let draft =
        std::fs::read_to_string(target.path().join("commands").join("item_set.yaml")).unwrap();
    let digest = Digest::of(&std::fs::read(scene.stand.program()).unwrap());
    assert!(draft.contains(&format!("sha256:{digest}")), "{draft}");
    assert!(
        draft.contains("--quantity") && draft.contains("if_version"),
        "{draft}"
    );
    for never in [
        "effect: read",
        "effect: write",
        "read_back",
        "3:",
        "pattern:",
    ] {
        assert!(
            !draft.contains(never),
            "the draft states `{never}`:\n{draft}"
        );
    }
    assert!(
        drafted
            .open_questions
            .iter()
            .any(|open| open.question.contains("read, write")),
        "{:?}",
        drafted.open_questions
    );
    assert_eq!(control.invocations().unwrap().len(), starts.len());

    // In the sandbox a read may be tried only against a test environment.
    scene.surface.environment = Environment::new("live").unwrap();
    let mut sandbox = scene
        .prepare(
            Posture::Sandbox {
                test_records: false,
            },
            None,
        )
        .await
        .unwrap();
    let refused = failed(
        scene
            .run(&mut sandbox, &get, &sku("SK-1001"), With::default())
            .await,
    );
    is(
        &refused,
        FailureClass::Policy,
        &codes::READ_ONLY_POSTURE,
        Sent::NotSent,
    );
    scene.handle.release(sandbox, false).await;
}
