//! The server store against a real PostgreSQL server: the server side of the
//! stand, `example_db::server`. Every write is proven by asking the server
//! what it stored, through the server's own client, never through the pack.
//!
//! A machine with no server says each case was skipped. With
//! `A2M_REQUIRE_POSTGRES=1` a missing server fails the test instead.

use std::sync::Arc;
use std::time::Duration;

use a2m_auth::{
    AskPerson, AuthEntry, CredentialRef, InProcessWork, Keeper, KeeperWorker, LeaseRequest,
    PersonAnswer, PersonAsk, ProbeOutcome, ProviderSet, Purpose, RunProbe, SessionKey,
    SessionLease, SessionRef, SessionStore, StoredCredentials, Wait,
};
use a2m_core::{
    AccountName, Archetype, BindingPlan, Decimal, DecisionId, Environment, Epoch, FailureClass,
    Finding, Identity, NoSecrets, PackPayload, Plan, RecordKey, RunId, Secret, TargetName, Value,
    Values, WorkspaceId,
};
use a2m_files::{Drafter, TargetRoot};
use a2m_provider::{
    BoxFuture, CheckedPaths, Deadline, Destinations, DiscoveryScope, DriverHandle, FindingSink,
    Posture, Prepared, ReadOnlyPort, Registry, Sent, StepFailure, StepOutcome, SurfaceDescription,
    SurfaceRef, codes as contract,
};
use a2m_ledger::Fence;
use example_db::server::{
    ADMIN_PASSWORD, ADMIN_ROLE, APP_PASSWORD, APP_ROLE, OWNER_PASSWORD, OWNER_ROLE, RelayFault,
    TestServer,
};

use super::{Folder, Run, Write, compile_folder, failed, inputs, registry_of, values};
use crate::discover::TableFound;
use crate::names::{DATABASE_SERVER, codes};

const LINE: [&str; 2] = ["A-100", "L-101"];
const LINE_KEY: [(&str, &str); 2] = [("order_id", "A-100"), ("item_code", "L-101")];

fn fixtures() -> std::path::PathBuf {
    super::fixtures().with_file_name("example-server")
}

fn decimal(text: &str) -> Value {
    Value::Decimal(Decimal::parse(text).unwrap())
}

fn update(length: &str, price: &str, version: i64) -> Values {
    inputs(&[("order_id", LINE[0]), ("item_code", LINE[1])])
        .with("length", decimal(length))
        .with("unit_price", decimal(price))
        .with("row_version", version)
}

fn server() -> Option<TestServer> {
    let server = TestServer::start().unwrap();
    if server.is_none() {
        eprintln!("skipped: this machine has no PostgreSQL server for the stand");
    }
    server
}

// ---- a real keeper, for a real lease ----

struct NoPerson;

impl AskPerson for NoPerson {
    fn ask<'a>(&'a self, _session: &'a SessionRef, _ask: PersonAsk) -> BoxFuture<'a, PersonAnswer> {
        Box::pin(async { PersonAnswer::NoAnswer })
    }
}

struct SignedInProbe;

impl RunProbe for SignedInProbe {
    fn probe<'a>(&'a self, _session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome> {
        Box::pin(async { ProbeOutcome::SignedIn })
    }
}

/// A session keeper over memory whose one session signs in with the provider
/// `http_basic`: a user and a stored password.
struct Signed {
    keeper: Keeper,
    key: SessionKey,
    entry: Arc<AuthEntry>,
    _store: a2m_ledger::Store,
}

impl Signed {
    async fn new(user: &str, password: &str) -> Self {
        let config = a2m_ledger::StoreConfig::in_memory()
            .with_clock(Arc::new(a2m_telemetry::ManualClock::starting_at(
                a2m_core::Timestamp::from_unix_millis(1_800_000_000_000),
            )))
            .with_random(super::counted());
        let store = a2m_ledger::Store::open(config).await.unwrap();
        let vault = Arc::new(a2m_vault::Vault::with_random(
            Arc::new(a2m_vault::MemoryKeyStore::with_random(super::counted())),
            Arc::new(store.key_records()),
            super::counted(),
        ));
        store.attach(Arc::clone(&vault)).await.unwrap();
        let clock = Arc::new(a2m_telemetry::ManualClock::starting_at(
            a2m_core::Timestamp::from_unix_millis(1_800_000_000_000),
        ));
        let sessions = SessionStore::new(store.clone());
        let credentials = StoredCredentials::new(sessions.clone(), Arc::clone(&vault));
        let providers = ProviderSet::neutral();
        let worker = Arc::new(KeeperWorker::new(
            providers.clone(),
            Arc::clone(&vault),
            Arc::new(credentials.clone()),
            clock.clone(),
        ));
        let work = Arc::new(InProcessWork::new(
            Arc::clone(&worker),
            Arc::new(NoPerson),
            Arc::new(SignedInProbe),
        ));
        let keeper = Keeper::builder(sessions, providers, work, clock)
            .local_worker(worker)
            .build();
        let key = SessionKey {
            workspace: WorkspaceId::from_bytes([7; 16]),
            target: TargetName::new("example").unwrap(),
            environment: Environment::new("test").unwrap(),
            identity: Identity::Account(AccountName::new("service").unwrap()),
        };
        let reference = CredentialRef::new("vault://example/service/password").unwrap();
        credentials
            .put(
                &key.workspace,
                &key.identity,
                &reference,
                &Secret::new(password),
            )
            .await
            .unwrap();
        let entry = AuthEntry::new(a2m_auth::ProviderName::new("http_basic").unwrap())
            .setting("user", user)
            .credential("password", reference);
        Self {
            keeper,
            key,
            entry: Arc::new(entry),
            _store: store,
        }
    }

    async fn lease(&self) -> SessionLease {
        self.keeper
            .lease(LeaseRequest {
                key: self.key.clone(),
                entry: Arc::clone(&self.entry),
                run: RunId::from_bytes([3; 16]),
                decided: DecisionId::from_bytes([4; 16]),
                acting_for: None,
                wait: Wait::DEFAULT,
                headroom: None,
                purpose: Purpose::Run,
            })
            .await
            .unwrap()
    }
}

// ---- the scene ----

/// The registry with only this pack, the server fixtures compiled and pointed
/// at the test's server, and a session that signs in as `user`.
struct ServerScene {
    registry: Registry,
    plan: Plan,
    surface: SurfaceRef,
    signed: Signed,
    _folder: Folder,
    server: TestServer,
}

fn compiled(registry: &Registry) -> Plan {
    match compile_folder(registry, &fixtures()) {
        Ok(compiled) => compiled.plan,
        Err(problems) => panic!("the server fixtures do not compile: {problems:#?}"),
    }
}

fn surface_at(plan: &Plan, address: &str, port: u16, database: &str) -> SurfaceRef {
    let mut surface = plan.surfaces[0].clone();
    address.clone_into(&mut surface.address);
    surface.payload = PackPayload::encode(&serde_json::json!({
        "store": "postgres",
        "port": port,
        "database": database,
    }))
    .unwrap();
    SurfaceRef {
        target: plan.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface,
    }
}

impl ServerScene {
    async fn new() -> Option<Self> {
        Self::as_role(APP_ROLE, APP_PASSWORD).await
    }

    async fn as_role(user: &str, password: &str) -> Option<Self> {
        let server = server()?;
        let folder = Folder::new("server");
        let registry = registry_of(&crate::pack(), &folder, Arc::new(NoSecrets));
        let plan = compiled(&registry);
        let surface = surface_at(
            &plan,
            &server.address().ip().to_string(),
            server.address().port(),
            server.database(),
        );
        Some(Self {
            registry,
            plan,
            surface,
            signed: Signed::new(user, password).await,
            _folder: folder,
            server,
        })
    }

    fn handle(&self) -> DriverHandle {
        self.registry.driver(&super::mode()).unwrap()
    }

    fn binding(&self, operation: &str) -> &BindingPlan {
        &self
            .plan
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap_or_else(|| panic!("the server fixtures have no operation {operation}"))
            .bindings[0]
    }

    async fn try_prepare(
        &self,
        handle: &DriverHandle,
        posture: Posture,
        fence: Fence,
    ) -> Result<Prepared, StepFailure> {
        let lease = self.signed.lease().await;
        let supervisor = handle.supervisor(Vec::new());
        let cx = a2m_provider::PrepareContext {
            run: fence.run,
            epoch: fence.epoch,
            surface: &self.surface,
            plan: &self.plan,
            posture,
            deadline: Deadline::after(Duration::from_secs(30)),
            redact: &NoSecrets,
            destinations: &Destinations::default(),
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        handle.prepare(&cx, Some(&lease)).await
    }

    async fn prepare_as(&self, posture: Posture, fence: Fence) -> Run<'_> {
        let handle = self.handle();
        let prepared = self.try_prepare(&handle, posture, fence).await.unwrap();
        Run {
            plan: &self.plan,
            handle,
            prepared: Some(prepared),
            wait: Duration::from_secs(30),
        }
    }

    async fn prepare(&self) -> Run<'_> {
        self.prepare_as(Posture::Live, fence()).await
    }

    /// A live run with a real intent token for the record `key`.
    async fn live(&self, key: &[&str]) -> (Run<'_>, Write) {
        let key = RecordKey::new(key.iter().map(|part| Value::from(*part)).collect()).unwrap();
        let store = super::ledger().await;
        let (fence, record, token) = super::write_run(&store, key.clone()).await;
        let run = self.prepare_as(Posture::Live, fence).await;
        (
            run,
            Write {
                _store: store,
                record,
                key,
                token,
            },
        )
    }

    fn line(&self, column: &str) -> Option<String> {
        self.server
            .stored_row("order_lines", &LINE_KEY)
            .unwrap()
            .unwrap()
            .remove(column)
            .flatten()
    }
}

fn fence() -> Fence {
    Fence {
        run: RunId::from_bytes([7; 16]),
        epoch: Epoch::new(1),
    }
}

// ---- reads ----

#[tokio::test]
async fn numbers_and_moments_are_read_exactly_and_a_type_with_no_rule_stops() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let mut run = scene.prepare().await;
    let read = scene.binding("example_line_item_get");
    let got = values(
        run.steps(&read.act, &inputs(&[("order_id", "A-100"), ("item_code", "L-101")]), None)
            .await
            .unwrap(),
    );
    // A `numeric` arrives as its exact text at its scale, never a float.
    assert_eq!(got.get("length"), Some(&Value::from("150.5000")));
    assert_eq!(got.get("unit_price"), Some(&Value::from("12.50")));
    assert_eq!(got.get("row_version"), Some(&Value::Int(1)));

    // A moment with its zone arrives as the same moment in UTC.
    let submitted = scene.binding("example_order_submitted");
    let got = values(
        run.steps(&submitted.act, &inputs(&[("order_id", "A-101")]), None)
            .await
            .unwrap(),
    );
    assert_eq!(
        got.get("submitted_at"),
        Some(&Value::from("2026-03-03T13:05:00.25Z"))
    );
    run.release().await;

    // A moment with no zone, and a float: no rule reads either.
    for operation in ["example_order_entered", "example_order_discount"] {
        let mut run = scene.prepare().await;
        let binding = scene.binding(operation);
        let refused = run
            .steps(&binding.act, &inputs(&[("order_id", "A-100")]), None)
            .await;
        assert_eq!(
            failed(refused),
            (
                FailureClass::UnknownVariant,
                codes::VALUE_KIND_UNSUPPORTED,
                Sent::Answered
            ),
            "{operation}"
        );
        run.release().await;
    }
}

#[tokio::test]
async fn no_input_changes_a_statement() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let mut run = scene.prepare().await;
    let read = scene.binding("example_line_item_get");
    let widened = inputs(&[("order_id", "A-100' OR '1'='1"), ("item_code", "L-101")]);
    let outcome = run.steps(&read.act, &widened, None).await.unwrap();
    assert!(matches!(outcome, StepOutcome::Absent(_)));
    run.release().await;
}

#[tokio::test]
async fn a_read_that_changes_data_is_refused_by_the_servers_read_only_transaction() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let before = scene
        .server
        .value("SELECT last_value FROM shipments_shipment_id_seq")
        .unwrap();
    let mut run = scene.prepare().await;
    let binding = scene.binding("example_lines_counted_on");
    let refused = run
        .steps(&binding.act, &inputs(&[("order_id", "A-100")]), None)
        .await;
    assert_eq!(
        failed(refused),
        (
            FailureClass::Policy,
            codes::READ_STATEMENT_WROTE,
            Sent::Answered
        )
    );
    run.release().await;
    let after = scene
        .server
        .value("SELECT last_value FROM shipments_shipment_id_seq")
        .unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn a_statement_past_its_time_limit_is_stopped_by_the_server() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let mut run = scene.prepare().await;
    let binding = scene.binding("example_lines_slowly");
    let stopped = run
        .steps(&binding.act, &inputs(&[("order_id", "A-100")]), None)
        .await;
    assert_eq!(
        failed(stopped),
        (codes::TRANSIENT, codes::TIMEOUT, Sent::Answered)
    );
    run.release().await;
    // Nothing of the statement still runs on the server.
    let running = scene
        .server
        .value("SELECT count(*) FROM pg_stat_activity WHERE query LIKE '%generate_series%' AND state = 'active' AND pid <> pg_backend_pid()")
        .unwrap();
    assert_eq!(running.as_deref(), Some("0"));
}

// ---- writes ----

#[tokio::test]
async fn an_update_is_written_once_exactly_and_reads_back() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let binding = scene.binding("example_line_item_update");
    let (mut run, write) = scene.live(&LINE).await;
    let wanted = update("151.25", "13.10", 1);
    run.steps(&binding.act, &wanted, Some(&write)).await.unwrap();
    assert_eq!(scene.server.write_count("order_lines", &LINE).unwrap(), 1);
    // The server holds the exact decimals at the column's scale.
    assert_eq!(scene.line("length_in").as_deref(), Some("151.2500"));
    assert_eq!(scene.line("unit_price").as_deref(), Some("13.10"));
    assert_eq!(scene.line("row_version").as_deref(), Some("2"));
    let read = values(run.steps(&binding.read_back, &wanted, None).await.unwrap());
    assert_eq!(read.get("length"), Some(&Value::from("151.2500")));
    assert_eq!(read.get("row_version"), Some(&Value::Int(2)));
    run.release().await;
    assert_eq!(scene.server.write_count("order_lines", &LINE).unwrap(), 1);
}

#[tokio::test]
async fn a_stale_version_and_a_refused_constraint_write_nothing() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let binding = scene.binding("example_line_item_update");
    let (mut run, write) = scene.live(&LINE).await;
    let stale = run
        .steps(&binding.act, &update("151.25", "13.10", 7), Some(&write))
        .await;
    assert_eq!(
        failed(stale),
        (FailureClass::Stale, codes::VERSION_CONFLICT, Sent::Answered)
    );
    run.release().await;

    let describe = scene.binding("example_line_item_describe");
    let (mut run, write) = scene.live(&LINE).await;
    let refused = run
        .steps(
            &describe.act,
            &inputs(&[
                ("order_id", LINE[0]),
                ("item_code", LINE[1]),
                ("description", "   "),
            ]),
            Some(&write),
        )
        .await;
    assert_eq!(
        failed(refused),
        (
            FailureClass::Rejected,
            codes::CONSTRAINT_VIOLATED,
            Sent::Answered
        )
    );
    run.release().await;
    assert_eq!(scene.server.write_count("order_lines", &LINE).unwrap(), 0);
    assert_eq!(scene.line("description").as_deref(), Some("Left rail"));
}

#[tokio::test]
async fn a_key_the_server_assigns_is_known_before_the_commit() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let binding = scene.binding("example_shipment_create");
    let (mut run, write) = scene.live(&["A-100"]).await;
    let wanted = inputs(&[
        ("order_id", "A-100"),
        ("carrier", "Example Freight"),
        ("external_ref", "SHIP-0002"),
    ]);
    let mut assigned = None;
    for step in &binding.act {
        let args = super::args_of(&scene.plan, step, &wanted);
        let outcome = run
            .step_with(step, &args, step.commits.then_some(&write))
            .await
            .unwrap();
        if let StepOutcome::Done(observed) = outcome
            && observed.assigned_key.is_some()
        {
            // Known here, before the commit: the server shows nothing yet.
            assert_eq!(scene.server.write_count("shipments", &["2"]).unwrap(), 0);
            assigned = observed.assigned_key;
        }
    }
    run.release().await;
    assert_eq!(scene.server.write_count("shipments", &["2"]).unwrap(), 1);
    let assigned = assigned.expect("the key was assigned before the commit");
    assert_eq!(assigned.parts(), &[Value::Int(2)]);
    let stored = scene
        .server
        .stored_row("shipments", &[("shipment_id", "2")])
        .unwrap()
        .unwrap();
    assert_eq!(stored["external_ref"].as_deref(), Some("SHIP-0002"));
}

/// Whether a write that declares no hazard commits now, in a new run.
async fn credit_write(scene: &ServerScene, cents: i64) -> Result<StepOutcome, StepFailure> {
    let binding = scene.binding("example_customer_credit_update");
    let (mut run, write) = scene.live(&["C-001"]).await;
    let wanted = inputs(&[("customer_id", "C-001")]).with("credit_limit_cents", cents);
    let outcome = run.steps(&binding.act, &wanted, Some(&write)).await;
    run.release().await;
    outcome
}

fn credit(scene: &ServerScene) -> Option<String> {
    scene
        .server
        .stored_row("customers", &[("customer_id", "C-001")])
        .unwrap()
        .unwrap()
        .remove("credit_limit_cents")
        .flatten()
}

#[tokio::test]
async fn what_a_write_reaches_beyond_its_record_is_read_from_the_servers_catalog() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    // `customers` has no trigger, rule, computed column, or cascading key,
    // so a write that declares no hazard commits.
    credit_write(&scene, 600_000).await.unwrap();
    assert_eq!(credit(&scene).as_deref(), Some("600000"));

    // Each of these makes a write to `customers` reach other data. The
    // trigger is written in capitals: the server folds a name not quoted.
    let reaches = [
        (
            "CREATE FUNCTION example_touch() RETURNS trigger LANGUAGE plpgsql AS $t$ BEGIN RETURN NEW; END $t$; \
             CREATE TRIGGER customers_touch BEFORE UPDATE ON CUSTOMERS FOR EACH ROW EXECUTE FUNCTION example_touch();",
            "DROP TRIGGER customers_touch ON customers; DROP FUNCTION example_touch();",
        ),
        (
            "CREATE RULE customers_noted AS ON UPDATE TO customers DO ALSO NOTIFY customers_changed;",
            "DROP RULE customers_noted ON customers;",
        ),
        (
            "ALTER TABLE customers ADD COLUMN credit_limit_doubled integer GENERATED ALWAYS AS (credit_limit_cents * 2) STORED;",
            "ALTER TABLE customers DROP COLUMN credit_limit_doubled;",
        ),
        (
            "CREATE TABLE customer_flags (customer_id text REFERENCES customers (customer_id) ON UPDATE CASCADE);",
            "DROP TABLE customer_flags;",
        ),
    ];
    for (add, remove) in reaches {
        scene
            .server
            .execute(&format!("SET ROLE {OWNER_ROLE}; {add}"))
            .unwrap();
        let stopped = credit_write(&scene, 700_000).await;
        assert_eq!(
            failed(stopped),
            (FailureClass::Drift, codes::SCHEMA_CHANGED, Sent::NotSent),
            "{add}"
        );
        assert_eq!(credit(&scene).as_deref(), Some("600000"), "{add}");
        scene
            .server
            .execute(&format!("SET ROLE {OWNER_ROLE}; {remove}"))
            .unwrap();
    }
    credit_write(&scene, 700_000).await.unwrap();
    assert_eq!(credit(&scene).as_deref(), Some("700000"));
}

#[tokio::test]
async fn a_lost_answer_after_the_commit_was_sent_is_a_write_in_doubt_reconciled_by_its_key() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let binding = scene.binding("example_line_item_update");
    let wanted = update("151.25", "13.10", 1);
    let (mut run, write) = scene.live(&LINE).await;
    scene.server.arm(RelayFault::LoseCommitAnswer);
    let lost = run.steps(&binding.act, &wanted, Some(&write)).await;
    assert!(scene.server.fault_met());
    assert_eq!(
        failed(lost),
        (
            FailureClass::PossibleCommit,
            contract::NO_CONFIRMATION,
            Sent::Unknown
        )
    );
    // It landed, once: the driver cannot know; the server does.
    assert_eq!(scene.server.write_count("order_lines", &LINE).unwrap(), 1);
    // The session sends nothing more.
    for step in binding.act.iter().chain(&binding.read_back) {
        let args = super::args_of(&scene.plan, step, &wanted);
        let again = run
            .step_with(step, &args, step.commits.then_some(&write))
            .await;
        assert_eq!(
            failed(again),
            (
                FailureClass::Transient,
                contract::CONNECTION_LOST,
                Sent::NotSent
            )
        );
    }
    run.release().await;
    // Reconcile: a new run reads the record back by its key, and finds the
    // intended values.
    let mut run = scene.prepare().await;
    let read = values(run.steps(&binding.read_back, &wanted, None).await.unwrap());
    assert_eq!(read.get("length"), Some(&Value::from("151.2500")));
    run.release().await;
    assert_eq!(scene.server.write_count("order_lines", &LINE).unwrap(), 1);
}

#[tokio::test]
async fn a_commit_that_never_reached_the_server_is_in_doubt_and_reconcile_finds_it_absent() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let binding = scene.binding("example_line_item_update");
    let wanted = update("151.25", "13.10", 1);
    let (mut run, write) = scene.live(&LINE).await;
    scene.server.arm(RelayFault::DropCommit);
    let lost = run.steps(&binding.act, &wanted, Some(&write)).await;
    assert!(scene.server.fault_met());
    assert_eq!(failed(lost).0, FailureClass::PossibleCommit);
    run.release().await;
    assert_eq!(scene.server.write_count("order_lines", &LINE).unwrap(), 0);
    let mut run = scene.prepare().await;
    let read = values(run.steps(&binding.read_back, &wanted, None).await.unwrap());
    assert_eq!(read.get("length"), Some(&Value::from("150.5000")));
    assert_eq!(read.get("row_version"), Some(&Value::Int(1)));
    run.release().await;
}

#[tokio::test]
async fn a_connection_ended_between_the_write_and_the_commit_writes_nothing() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let binding = scene.binding("example_line_item_update");
    let wanted = update("151.25", "13.10", 1);
    let (mut run, write) = scene.live(&LINE).await;
    let (commit, before) = binding.act.split_last().unwrap();
    run.steps(before, &wanted, None).await.unwrap();
    assert!(scene.server.terminate(APP_ROLE).unwrap() >= 1);
    let ended = run.step_with(commit, &Values::new(), Some(&write)).await;
    // Either the driver saw the link end before it sent `commit`, or it
    // sent it into a link that had ended: never a success.
    let (class, _, sent) = failed(ended);
    assert!(
        matches!(
            (class, sent),
            (FailureClass::Transient, Sent::NotSent) | (FailureClass::PossibleCommit, Sent::Unknown)
        ),
        "{class:?} {sent:?}"
    );
    run.release().await;
    assert_eq!(scene.server.write_count("order_lines", &LINE).unwrap(), 0);
    assert_eq!(scene.line("row_version").as_deref(), Some("1"));
}

#[tokio::test]
async fn a_lock_held_by_another_connection_is_busy_and_writes_nothing() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let holder = scene
        .server
        .hold_lock("order_lines", "order_id = 'A-100'")
        .unwrap();
    let binding = scene.binding("example_line_item_update");
    let (mut run, write) = scene.live(&LINE).await;
    // The step's own time is short, so the lock wait ends inside it.
    run.wait = Duration::from_millis(500);
    let busy = run
        .steps(&binding.act, &update("151.25", "13.10", 1), Some(&write))
        .await;
    assert_eq!(
        failed(busy),
        (FailureClass::Busy, codes::STORE_LOCKED, Sent::Answered)
    );
    run.release().await;
    drop(holder);
    assert_eq!(scene.server.write_count("order_lines", &LINE).unwrap(), 0);
}

// ---- the identity, the link, and the pool ----

#[tokio::test]
async fn an_identity_that_owns_the_tables_or_is_a_superuser_is_refused_before_anything_runs() {
    let Some(scene) = ServerScene::as_role(OWNER_ROLE, OWNER_PASSWORD).await else {
        return;
    };
    let handle = scene.handle();
    let refused = scene
        .try_prepare(&handle, Posture::Live, fence())
        .await
        .map(|_| ());
    let failure = refused.unwrap_err();
    assert_eq!(failure.failure.code(), &codes::IDENTITY_TOO_POWERFUL);
    assert_eq!(failure.sent, Sent::NotSent);
    // The stand's administrator is a superuser, on a server of its own.
    if std::env::var("A2M_TEST_POSTGRES").is_err() {
        let Some(scene) = ServerScene::as_role(ADMIN_ROLE, ADMIN_PASSWORD).await else {
            return;
        };
        let handle = scene.handle();
        let failure = scene
            .try_prepare(&handle, Posture::Live, fence())
            .await
            .map(|_| ())
            .unwrap_err();
        assert_eq!(failure.failure.code(), &codes::IDENTITY_TOO_POWERFUL);
    }
}

#[tokio::test]
async fn a_wrong_password_is_refused_by_the_server() {
    let Some(scene) = ServerScene::as_role(APP_ROLE, "secret-for-tests-wrong").await else {
        return;
    };
    let handle = scene.handle();
    let failure = scene
        .try_prepare(&handle, Posture::Live, fence())
        .await
        .map(|_| ())
        .unwrap_err();
    assert_eq!(failure.failure.code(), &codes::DENIED_BY_STORE);
    assert!(!failure.failure.message().contains("secret-for-tests"));
}

#[tokio::test]
async fn a_server_off_the_loopback_address_or_with_no_session_is_refused_before_anything_is_sent()
{
    let folder = Folder::new("server-link");
    let registry = registry_of(&crate::pack(), &folder, Arc::new(NoSecrets));
    let plan = compiled(&registry);
    let handle = registry.driver(&super::mode()).unwrap();
    for (address, code) in [
        // Documentation addresses: nothing is ever sent to them.
        ("192.0.2.10", &codes::LINK_NOT_ENCRYPTED),
        ("localhost", &codes::LINK_NOT_ENCRYPTED),
        ("db.example.test", &codes::LINK_NOT_ENCRYPTED),
        // On the loopback address, but with no session to sign in with.
        ("127.0.0.1", &codes::CREDENTIAL_NOT_CARRIED),
    ] {
        let surface = surface_at(&plan, address, 5432, "example");
        let supervisor = handle.supervisor(Vec::new());
        let cx = a2m_provider::PrepareContext {
            run: fence().run,
            epoch: fence().epoch,
            surface: &surface,
            plan: &plan,
            posture: Posture::Live,
            deadline: Deadline::after(Duration::from_secs(5)),
            redact: &NoSecrets,
            destinations: &Destinations::default(),
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let failure = handle.prepare(&cx, None).await.map(|_| ()).unwrap_err();
        assert_eq!(failure.failure.code(), code, "{address}");
        assert_eq!(failure.sent, Sent::NotSent);
    }
}

/// Waits until the server counts `count` connections of the app role: a
/// connection closed by the client ends on the server a moment later.
async fn connections_become(scene: &ServerScene, count: u64) {
    for _ in 0..100 {
        if scene.server.connections_of(APP_ROLE).unwrap() == count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the server never counted {count} connections of the app role");
}

#[tokio::test]
async fn connections_are_kept_for_the_session_and_closed_after_a_failure() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let read = scene.binding("example_line_item_get");
    let key = inputs(&[("order_id", "A-100"), ("item_code", "L-101")]);
    for _ in 0..3 {
        let mut run = scene.prepare().await;
        run.steps(&read.act, &key, None).await.unwrap();
        run.release().await;
    }
    // Three runs of one session, one connection.
    connections_become(&scene, 1).await;
    // A run in which a step failed closes what it used.
    let binding = scene.binding("example_line_item_update");
    let (mut run, write) = scene.live(&LINE).await;
    let stale = run
        .steps(&binding.act, &update("151.25", "13.10", 7), Some(&write))
        .await;
    assert_eq!(failed(stale).0, FailureClass::Stale);
    run.release().await;
    connections_become(&scene, 0).await;
}

// ---- discovery ----

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), a2m_core::Failure> {
        self.0.push(finding);
        Ok(())
    }
}

#[tokio::test]
async fn discovery_reads_the_servers_catalog_and_drafts_reads_only() {
    let Some(scene) = ServerScene::new().await else {
        return;
    };
    let description = SurfaceDescription {
        kind: DATABASE_SERVER,
        archetype: Archetype::Request,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = scene.registry.discoverers_for(&description).remove(0);
    let handle = scene.handle();
    let prepared = scene
        .try_prepare(&handle, Posture::Discovery, fence())
        .await
        .unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(Duration::from_secs(30)),
        paths: &CheckedPaths::none(),
        redact: &NoSecrets,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    discoverer.discover(&port, &scope, &mut kept).await.unwrap();
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    let tables: Vec<TableFound> = kept
        .0
        .iter()
        .map(|finding| serde_json::from_str(finding.found()).unwrap())
        .collect();
    let lines = tables.iter().find(|table| table.table == "order_lines").unwrap();
    assert_eq!(
        lines
            .columns
            .iter()
            .filter(|column| column.key_position > 0)
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["order_id", "item_code"]
    );
    assert!(lines.triggers.contains(&"order_lines_bump_version".to_owned()));
    let orders = tables.iter().find(|table| table.table == "orders").unwrap();
    assert!(orders.cascades_to.contains(&"order_lines".to_owned()));
    assert!(tables.iter().any(|table| table.table == "order_summaries" && table.view));
    // Discovery wrote nothing.
    assert!(scene.server.writes().unwrap().is_empty());

    let folder = Folder::new("server-draft");
    let root = folder.path().join("example");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(fixtures().join("target.yaml"), root.join("target.yaml")).unwrap();
    let of = |name: &str| {
        kept.0
            .iter()
            .find(|finding| {
                serde_json::from_str::<TableFound>(finding.found()).unwrap().table == name
            })
            .unwrap()
            .clone()
    };
    let report = Drafter::new(scene.registry.compiler_set(), &TargetRoot::new(&root))
        .draft(&[of("order_lines")])
        .unwrap();
    assert_eq!(report.written.len(), 1);
    let drafted =
        std::fs::read_to_string(root.join("statements").join("order_lines.yaml")).unwrap();
    assert!(drafted.contains("form: \"table\""), "{drafted}");
    assert!(
        !drafted.contains("SELECT") && !drafted.contains("write"),
        "{drafted}"
    );
}
