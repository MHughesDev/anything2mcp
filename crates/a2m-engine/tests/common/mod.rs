//! What the tests share: a test pack whose driver talks to the built-in test
//! application over its JSON interface, registered through the provider
//! contract as a real pack is; a hand-built plan for the imaginary
//! `example` target; a `Control` over a real store, so that intent tokens
//! are the ledger's own; and the part of the engine's caller: it claims the
//! job, applies the outcome, and resumes.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use a2m_auth::{ExpirySign, SessionLease};
use a2m_core::{
    Address, ApprovalId, AuthPlan, AuthProviderName, BindingPlan, BindingStatus, CanonicalJson,
    Digest, EffectClass, Environment, EnvironmentIndex, EnvironmentKind, EnvironmentPlan, Epoch,
    EvidenceLevel, ExprIndex, FailsAs, Failure, FailureClass, FailureCode, FieldFormat,
    FieldTypeIndex, FieldTypePlan, Finding, FindingKind, Hazard, IdentityPlan, IdentityStrategy,
    InputIndex, InputPlan, Mode, OperationIndex, OperationName, OperationPlan, OutputPlan,
    PLAN_FORMAT, PackPayload, PermissionManifest, PipelineIndex, PipelineLimits, PipelinePlan,
    PipelineStep, Plan, PlanExpr, PlanHeader, PrincipalId, ProbePlan, ProducedBy, RecordKey,
    RecordState, Rounding, RunId, RunState, Sensitivity, SignInMode, Source, SourcePlan, Step,
    StepArg, StepKind, Subject, SurfaceIndex, SurfaceKind, SurfaceName, SurfacePlan, TargetName,
    TargetPlan, Timestamp, ToolAnnotations, ToolPlan, ToolRuns, Value, ValueType, Values, Verb,
    Version, WhitespaceRule, WorkspaceId,
};
use a2m_engine::{
    BoxFuture, Clearance, Conditions, Control, Engine, EngineDeps, EngineEvent, EngineSettings,
    Evidence, GuardQuestion, JobKind, JobOutcome, JobSpec, KeptCapture, LeaseClock, Parallelism,
    Preview, RecordSnapshot, RunSnapshot, SessionWant, ToolRef, decode_values,
};
use a2m_expr::{DecimalMark, FieldRule, LibraryVersion, Whitespace};
use a2m_ledger::{
    Acted, ActedToken, BlobRef, Durability, EventKind, Fence, IntentToken, LockHeld, LockRow,
    NewBlob, NewRun, Opened, ReadBack, Reconciled, RecordIntent, RecordRef, RecordRow,
    RetentionClass, RunRow, Settled, Store, StoreConfig, ToolKind, WriteClearance,
};
use a2m_provider::{
    Cleanup, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, Driver, DriverHealth, FindingSink, HostServices, Observed, Pack, PackContext,
    Posture, PrepareContext, Prepared, ProviderSettings, Registrar, Registry, RegistryBuilder,
    Roots, SandboxPort, StepFailure, StepInput, StepOutcome, Supervisor, SupervisorLimits,
    SurfaceDescription, SurfaceRef,
};
use a2m_sources::{
    HiddenRule, Locate, ReaderOptions, RowField, SourceInput, SourceSpec, Sources, TemplateVersion,
};
use a2m_telemetry::{CaptureLevel, FinishedTrace, ManualClock, TelemetrySettings};
use a2m_vault::{MemoryKeyStore, OwnerRef, RandomSource, Vault};
use example_app::harness::TestApp;

// ---- small things ----

/// Random bytes that are the same on every run: a counter, hashed.
pub struct Counted(AtomicU64);

impl Counted {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(0)))
    }
}

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

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-engine-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn failure(class: FailureClass, code: &'static str, message: &str) -> Failure {
    Failure::new(
        class,
        FailureCode::fixed(code),
        message,
        "Look at the test.",
    )
}

pub const START: i64 = 1_800_000_000_000;

pub fn workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([1; 16])
}

// ---- the test pack: a driver over the test application's JSON interface ----

pub const FIRST_MODE: &str = "app_json";
pub const SECOND_MODE: &str = "app_json_spare";

const MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "example_json"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[modes]]
name = "app_json"
archetype = "request"
verbs = ["read", "upsert"]
steps = ["get_order", "get_item", "put_item", "get_stock", "add_item", "reserve_code", "find_item", "check_item", "taken", "scan"]
absence_steps = ["scan"]
rank = 40
session_reads = 4

[[modes]]
name = "app_json_spare"
archetype = "request"
verbs = ["read", "upsert"]
steps = ["get_order", "get_item", "put_item", "get_stock", "add_item", "reserve_code", "find_item", "check_item", "taken", "scan"]
rank = 50
session_reads = 4

[[surface_kinds]]
name = "json_app"
archetype = "request"
session = true

[points]
driver = ["app_json", "app_json_spare"]
discoverer = ["look"]

[[discoverers]]
name = "look"
archetype = "request"
kinds = ["call"]
sources = ["live_system"]
max_level = "discovered"
bounds = { seconds = 60, bytes = 1000000, depth = 4 }
files = ["description"]

[[finding_kinds]]
name = "call"
surface_kinds = ["json_app"]

[[failure_codes]]
class = "rejected"
code = "example_refused"

[[failure_codes]]
class = "stale"
code = "version_conflict"

[[failure_codes]]
class = "transient"
code = "example_unavailable"

[[failure_codes]]
class = "drift"
code = "example_shape_changed"

[[failure_codes]]
class = "unknown_variant"
code = "example_unexpected"

[[failure_codes]]
class = "possible_commit"
code = "ambiguous_match"

[[failure_codes]]
class = "policy"
code = "example_too_large"

[[failure_codes]]
class = "input"
code = "example_bad_shape"

[[failure_codes]]
class = "stale"
code = "example_gone"

[[failure_codes]]
class = "busy"
code = "example_busy"

[[failure_codes]]
class = "transient"
code = "example_not_covered"

[[failure_codes]]
class = "unknown_variant"
code = "example_nothing_recorded"

[effects]
network = "surfaces"
sessions = ["json_app"]
"#;

/// Something a test does as a step reaches a driver.
pub type StepHook = Box<dyn Fn(&str) + Send + Sync>;

/// What a test can make the driver do.
/// Each destination one `prepare` was given, with the scope it carried.
pub type ScopedDestinations = Vec<(String, Option<a2m_core::ScopedHost>)>;

#[derive(Default)]
pub struct Hooks {
    /// Every step that reached a driver, and every call the control plane
    /// was asked to store, in the order they happened.
    pub log: Mutex<Vec<String>>,
    /// The next write is sent, and its answer is lost.
    pub lose_answer: AtomicBool,
    /// The next write fails before anything is sent.
    pub fail_unsent: AtomicBool,
    /// Every step of the first mode fails as drift.
    pub first_mode_drifts: AtomicBool,
    /// Every step is refused as it would be when the session has ended.
    pub session_ended: AtomicBool,
    /// The posture each `prepare` was given.
    pub postures: Mutex<Vec<String>>,
    /// A step of this kind asks, before it acts, whether another record
    /// holds this value under this name; the answers, in order.
    pub asks_elsewhere: Mutex<Option<(String, String, Value)>>,
    pub elsewhere: Mutex<Vec<a2m_provider::RecordedElsewhere>>,
    /// Every step that reached a driver, with the mode it reached.
    pub by_mode: Mutex<Vec<(String, String)>>,
    /// Every step that reached a driver, with the arguments, the key, and
    /// the stamp it was given.
    pub given: Mutex<Vec<Given>>,
    /// Called as each step reaches a driver, with the step's kind.
    pub before: Mutex<Vec<StepHook>>,
    /// A step of this kind panics in the driver.
    pub panics_on: Mutex<Option<String>>,
    /// A step of this kind never ends in the driver.
    pub hangs_on: Mutex<Option<String>>,
    /// Preparing a driver never ends.
    pub prepare_hangs: AtomicBool,
    /// The next lookups find nothing, this many times, whatever is stored.
    pub lookup_lags: AtomicU32,
    /// The next step of this kind is carried out, and then fails with this
    /// class and code, as a failure the system answered with.
    pub fails_after: Mutex<Option<(String, FailureClass, &'static str)>>,
    /// A save takes this long before it is sent: longer, when a test says
    /// so, than the step's time limit.
    pub save_takes_ms: AtomicU64,
    /// A save is carried out away from the step's future, as a driver does
    /// that commits on a thread of its own: it goes on when the future is
    /// dropped. It is sent once `late_gate` is opened.
    pub detached_save: AtomicBool,
    /// Told when a detached save is in flight.
    pub in_flight: tokio::sync::Notify,
    /// Opened by the test to let a detached save reach the application.
    pub late_gate: tokio::sync::Notify,
    /// The detached save, to wait for its end.
    pub late: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// A read of an order says how it was done, and captures the answer.
    pub tells_acting: AtomicBool,
    /// A read of an order also reads the application's hand-off of it and
    /// answers the hand-off's `client_secret` as the customer: the
    /// application's own answer carries a value a person registered as a
    /// secret.
    pub echoes_handoff: AtomicBool,
    /// What the system says with each failure it answers with.
    pub says: Mutex<Option<String>>,
    /// The project folder and the destinations each `prepare` was given.
    pub prepared_with: Mutex<Vec<(Option<PathBuf>, Vec<String>)>>,
    /// The scope each destination carried, as each `prepare` was given it.
    pub scoped_with: Mutex<Vec<ScopedDestinations>>,
    /// How many orders were read, for what a read measures.
    pub reads: AtomicU64,
    /// A step of this kind returns, beside what it observed, a capture
    /// named `screen`, of `text/plain`, that holds this text.
    pub captures_with: Mutex<Option<(String, String)>>,
    /// What every `taken` look answers while it is set, whatever is
    /// stored: `absent`, or `nothing`, a look that did not cover what it
    /// looked in.
    pub taken_answer: Mutex<Option<&'static str>>,
    /// The code a step hands out before the save is recorded with the
    /// order it is for, so that a record has two recorded values.
    pub records_order: AtomicBool,
    /// The step that hands out a code before the save records nothing for
    /// reconcile.
    pub records_nothing: AtomicBool,
    /// Every step of this mode never ends.
    pub mode_hangs: Mutex<Option<String>>,
    /// Every step of this mode fails as the system's answer that it is
    /// unavailable.
    pub mode_fails: Mutex<Option<String>>,
    /// What a step of this mode observes has this value under this name,
    /// in place of what the application holds.
    pub mode_alters: Mutex<Option<(String, String, Value)>>,
}

/// What one step was given.
#[derive(Debug, Clone)]
pub struct Given {
    pub kind: String,
    pub args: Values,
    pub key: Option<RecordKey>,
    pub stamp: Option<String>,
    /// The address the driver was prepared for.
    pub address: String,
    /// What the record's earlier steps recorded, as the handle handed it.
    pub recorded: Values,
}

impl Hooks {
    pub fn note(&self, what: impl Into<String>) {
        self.log.lock().unwrap().push(what.into());
    }

    pub fn seen(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    pub fn count(&self, what: &str) -> usize {
        self.seen().iter().filter(|entry| *entry == what).count()
    }

    /// Calls `f` with the kind of every step that reaches a driver.
    pub fn on_step(&self, f: impl Fn(&str) + Send + Sync + 'static) {
        self.before.lock().unwrap().push(Box::new(f));
    }

    /// The steps of this kind that reached a driver, as they were given.
    pub fn given(&self, kind: &str) -> Vec<Given> {
        self.given
            .lock()
            .unwrap()
            .iter()
            .filter(|given| given.kind == kind)
            .cloned()
            .collect()
    }

    /// How many steps reached the driver of a mode.
    pub fn steps_of(&self, mode: &str) -> usize {
        self.by_mode
            .lock()
            .unwrap()
            .iter()
            .filter(|(held, _)| held == mode)
            .count()
    }
}

struct AppState {
    base: String,
    http: reqwest::Client,
    /// The posture the context was prepared in.
    posture: Posture,
}

impl AppState {
    /// The pack's half of "cannot write". Live, the driver sends what a
    /// step asks for. Where a declared test record may be written, it
    /// sends a step that changes the system only when the step is the one
    /// that commits, which the handle has checked against the intent
    /// token of that record; a step that changes the system and says it
    /// commits nothing is refused there too. Anywhere else it sends
    /// nothing that changes the system, whatever the step says of itself.
    fn refuses(&self, step: &Step) -> bool {
        match self.posture {
            Posture::Live => false,
            Posture::Sandbox { test_records: true } => !step.commits,
            Posture::Sandbox {
                test_records: false,
            }
            | Posture::Discovery => true,
        }
    }
}

fn read_only() -> StepFailure {
    StepFailure::not_sent(Failure::new(
        FailureClass::Policy,
        a2m_provider::codes::READ_ONLY_POSTURE,
        "The driver sends no write where nothing may be written.",
        "Run the operation live, or against a declared test record.",
    ))
}

struct AppDriver {
    mode: Mode,
    hooks: Arc<Hooks>,
}

fn text(args: &Values, name: &str) -> Result<String, StepFailure> {
    match args.get(name) {
        Some(Value::Text(text)) => Ok(text.clone()),
        Some(Value::Decimal(number)) => Ok(number.to_string()),
        Some(Value::Int(number)) => Ok(number.to_string()),
        _ => Err(StepFailure::not_sent(failure(
            FailureClass::UnknownVariant,
            "example_unexpected",
            &format!("The step was given no `{name}`."),
        ))),
    }
}

fn unavailable(what: &str) -> Failure {
    failure(FailureClass::Transient, "example_unavailable", what)
}

/// Turns the application's item view into the values a step observed.
fn item_values(body: &serde_json::Value) -> Values {
    let mut values = Values::new();
    if let Some(description) = body["description"].as_str() {
        values.insert("description", description);
    }
    if let Some(quantity) = body["quantity"].as_i64() {
        values.insert("quantity", quantity);
    }
    if let Some(length) = body["length_in"].as_str() {
        values.insert("length_in", length);
    }
    if let Some(version) = body["version"].as_i64() {
        values.insert("version", version);
    }
    if let Some(code) = body["item_code"].as_str() {
        values.insert("item_code", code);
    }
    values
}

fn ambiguous() -> Failure {
    failure(
        FailureClass::PossibleCommit,
        "ambiguous_match",
        "The lookup found more than one record.",
    )
}

impl AppDriver {
    async fn get(
        &self,
        state: &AppState,
        path: &str,
    ) -> Result<(u16, serde_json::Value), StepFailure> {
        let response = state
            .http
            .get(format!("{}{path}", state.base))
            .send()
            .await
            .map_err(|_| StepFailure::unknown(unavailable("The application did not answer.")))?;
        let status = response.status().as_u16();
        let body = response.json().await.unwrap_or(serde_json::Value::Null);
        match status {
            401 => Err(StepFailure::answered(Failure::new(
                FailureClass::SessionExpired,
                a2m_auth::codes::NEEDS_LOGIN,
                "The application asked for a sign-in.",
                "Sign in again.",
            ))),
            500..=599 => Err(StepFailure::answered(unavailable(
                "The application answered that it is unavailable.",
            ))),
            _ => Ok((status, body)),
        }
    }
}

impl AppDriver {
    /// The line items of an order, as the application shows them.
    async fn items(
        &self,
        state: &AppState,
        order: &str,
    ) -> Result<Vec<serde_json::Value>, StepFailure> {
        let (status, body) = self.get(state, &format!("/api/orders/{order}")).await?;
        if status != 200 {
            return Err(StepFailure::answered(failure(
                FailureClass::Rejected,
                "example_refused",
                "The application has no such order.",
            )));
        }
        Ok(body["items"].as_array().cloned().unwrap_or_default())
    }

    /// The code the imaginary system assigns to the next new line item of
    /// an order: the test application takes a code from its caller, so the
    /// driver stands in for the part of a system that hands one out.
    async fn free_code(&self, state: &AppState, order: &str) -> Result<String, StepFailure> {
        let items = self.items(state, order).await?;
        let taken: Vec<&str> = items
            .iter()
            .filter_map(|item| item["item_code"].as_str())
            .collect();
        let code = (1..10_000)
            .map(|number| format!("N-{number:03}"))
            .find(|code| !taken.contains(&code.as_str()));
        code.ok_or_else(|| StepFailure::not_sent(unavailable("The order has no free code left.")))
    }
}

impl AppDriver {
    fn act_inner<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let hooks = &self.hooks;
            hooks.note(step.kind.as_str());
            hooks
                .by_mode
                .lock()
                .unwrap()
                .push((self.mode.to_string(), step.kind.to_string()));
            let recorded = prepared.recorded().clone();
            let recorded_code = recorded.get("item_code").cloned();
            hooks.given.lock().unwrap().push(Given {
                recorded,
                kind: step.kind.to_string(),
                args: input.args.clone(),
                key: input.key.cloned(),
                stamp: input.stamp.map(str::to_owned),
                address: prepared
                    .state::<AppState>()
                    .map(|state| state.base.clone())
                    .unwrap_or_default(),
            });
            for call in hooks.before.lock().unwrap().iter() {
                call(step.kind.as_str());
            }
            if hooks.panics_on.lock().unwrap().as_deref() == Some(step.kind.as_str()) {
                panic!("the test driver was told to panic in `{}`", step.kind);
            }
            if hooks.hangs_on.lock().unwrap().as_deref() == Some(step.kind.as_str())
                || hooks.mode_hangs.lock().unwrap().as_deref() == Some(self.mode.as_str())
            {
                std::future::pending::<()>().await;
            }
            if hooks.mode_fails.lock().unwrap().as_deref() == Some(self.mode.as_str()) {
                return Err(StepFailure::answered(unavailable(
                    "The application answered that it is unavailable.",
                )));
            }
            if hooks.session_ended.load(Ordering::SeqCst) {
                return Err(StepFailure::answered(Failure::new(
                    FailureClass::SessionExpired,
                    a2m_auth::codes::NEEDS_LOGIN,
                    "The application asked for a sign-in.",
                    "Sign in again.",
                )));
            }
            if hooks.first_mode_drifts.load(Ordering::SeqCst) && self.mode.as_str() == FIRST_MODE {
                return Err(StepFailure::answered(failure(
                    FailureClass::Drift,
                    "example_shape_changed",
                    "The answer no longer has the shape the target describes.",
                )));
            }
            let state = prepared
                .state::<AppState>()
                .ok_or_else(|| StepFailure::not_sent(unavailable("The driver lost its state.")))?;
            let args = input.args;
            let fails_after = {
                let mut planted = hooks.fails_after.lock().unwrap();
                if planted
                    .as_ref()
                    .is_some_and(|(kind, _, _)| kind == step.kind.as_str())
                {
                    planted.take()
                } else {
                    None
                }
            };
            let done = match step.kind.as_str() {
                "get_order" => {
                    let order = text(args, "order_id")?;
                    let (status, body) = self.get(state, &format!("/api/orders/{order}")).await?;
                    if status != 200 {
                        return Err(StepFailure::answered(failure(
                            FailureClass::Rejected,
                            "example_refused",
                            "The application has no such order.",
                        )));
                    }
                    let lines = body["items"].as_array().map_or(0, Vec::len);
                    let mut body = body;
                    if hooks.echoes_handoff.load(Ordering::SeqCst) {
                        let (_, handoff) = self
                            .get(state, &format!("/api/orders/{order}/handoff"))
                            .await?;
                        body["customer"] = handoff["client_secret"].clone();
                    }
                    let read = hooks.reads.fetch_add(1, Ordering::SeqCst);
                    let captures = if hooks.tells_acting.load(Ordering::SeqCst) {
                        vec![
                            a2m_provider::Capture::acting(&a2m_telemetry::Acting {
                                means: vec![
                                    a2m_telemetry::Detail::new("step", "get_order"),
                                    a2m_telemetry::Detail::new("path", "/api/orders/{order}"),
                                ],
                                measured: vec![a2m_telemetry::Detail::new(
                                    "read",
                                    read.to_string(),
                                )],
                            }),
                            a2m_provider::Capture {
                                name: "response".to_owned(),
                                media_type: "application/json".to_owned(),
                                bytes: body.to_string().into_bytes(),
                            },
                        ]
                    } else {
                        Vec::new()
                    };
                    Ok(StepOutcome::Done(Observed {
                        values: Values::new()
                            .with("order_id", body["order_id"].as_str().unwrap_or_default())
                            .with("customer", body["customer"].as_str().unwrap_or_default())
                            .with("status", body["status"].as_str().unwrap_or_default())
                            .with("lines", i64::try_from(lines).unwrap_or(0)),
                        captures,
                        ..Observed::default()
                    }))
                }
                "get_stock" => {
                    let item = text(args, "item_code")?;
                    let (_, body) = self.get(state, &format!("/api/stock/{item}")).await?;
                    Ok(StepOutcome::Done(Observed {
                        values: Values::new()
                            .with("in_stock", body["in_stock"].as_i64().unwrap_or(0)),
                        ..Observed::default()
                    }))
                }
                // A confirmation: the record is there after the save. It
                // is no lookup, so a record that is not there is the
                // system's own answer that it took nothing.
                "check_item" => {
                    let order = text(args, "order_id")?;
                    let item = text(args, "item_code")?;
                    let path = format!("/api/orders/{order}/items/{item}");
                    let (status, _) = self.get(state, &path).await?;
                    if status == 200 {
                        Ok(StepOutcome::Done(Observed::default()))
                    } else {
                        Err(StepFailure::answered(failure(
                            FailureClass::Rejected,
                            "example_refused",
                            "The application does not hold the line item.",
                        )))
                    }
                }
                "get_item" => {
                    let order = text(args, "order_id")?;
                    let item = text(args, "item_code")?;
                    let path = format!("/api/orders/{order}/items/{item}");
                    let (status, body) = self.get(state, &path).await?;
                    match status {
                        200 => Ok(StepOutcome::Done(Observed {
                            values: item_values(&body),
                            ..Observed::default()
                        })),
                        404 => Ok(StepOutcome::Absent(Observed::default())),
                        _ => Err(StepFailure::answered(failure(
                            FailureClass::UnknownVariant,
                            "example_unexpected",
                            "The application answered a read in a way no rule covers.",
                        ))),
                    }
                }
                // A look for a record of no identity, by the code its earlier
                // step recorded: found, absent, or a look that did not
                // cover the order.
                "taken" => {
                    let order = text(args, "order_id")?;
                    let code = match &recorded_code {
                        Some(Value::Text(code)) => code.clone(),
                        _ => {
                            return Err(StepFailure::not_sent(failure(
                                FailureClass::UnknownVariant,
                                "example_nothing_recorded",
                                "The look was handed no recorded code.",
                            )));
                        }
                    };
                    match *hooks.taken_answer.lock().unwrap() {
                        Some("absent") => return Ok(StepOutcome::Absent(Observed::default())),
                        Some(_) => {
                            return Err(StepFailure::answered(failure(
                                FailureClass::Transient,
                                "example_not_covered",
                                "The look did not cover the order.",
                            )));
                        }
                        None => {}
                    }
                    let path = format!("/api/orders/{order}/items/{code}");
                    let (status, body) = self.get(state, &path).await?;
                    match status {
                        200 => Ok(StepOutcome::Done(Observed {
                            values: item_values(&body).with("order_id", order.as_str()),
                            ..Observed::default()
                        })),
                        404 => Ok(StepOutcome::Absent(Observed::default())),
                        _ => Err(StepFailure::answered(failure(
                            FailureClass::UnknownVariant,
                            "example_unexpected",
                            "The application answered a look in a way no rule covers.",
                        ))),
                    }
                }
                // A lookup of a record of no identity through every line
                // item of the order, by the stamp its save wrote: the mode's
                // manifest states its "absent" authoritative. It may lag
                // behind the write, as `find_item` may.
                "scan" => {
                    let order = text(args, "order_id")?;
                    let Some(stamp) = input.stamp else {
                        return Err(StepFailure::not_sent(failure(
                            FailureClass::UnknownVariant,
                            "example_unexpected",
                            "The scan was given no stamp to look for.",
                        )));
                    };
                    if matches!(*hooks.taken_answer.lock().unwrap(), Some("nothing")) {
                        return Err(StepFailure::answered(failure(
                            FailureClass::Transient,
                            "example_not_covered",
                            "The scan did not cover the order.",
                        )));
                    }
                    if hooks
                        .lookup_lags
                        .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                            left.checked_sub(1)
                        })
                        .is_ok()
                    {
                        return Ok(StepOutcome::Absent(Observed::default()));
                    }
                    let wanted = format!("ref:{stamp}");
                    let items = self.items(state, &order).await?;
                    match items
                        .iter()
                        .find(|item| item["description"].as_str() == Some(wanted.as_str()))
                    {
                        Some(item) => Ok(StepOutcome::Done(Observed {
                            values: item_values(item).with("order_id", order.as_str()),
                            ..Observed::default()
                        })),
                        None => Ok(StepOutcome::Absent(Observed::default())),
                    }
                }
                "put_item" => {
                    if state.refuses(step) {
                        return Err(read_only());
                    }
                    if hooks.fail_unsent.swap(false, Ordering::SeqCst) {
                        return Err(StepFailure::not_sent(failure(
                            FailureClass::UnknownVariant,
                            "example_unexpected",
                            "The request could not be built.",
                        )));
                    }
                    let order = text(args, "order_id")?;
                    let item = text(args, "item_code")?;
                    let mut body = serde_json::json!({
                        "description": text(args, "description")?,
                        "quantity": args.get("quantity").and_then(Value::as_int).unwrap_or(0),
                        "length_in": text(args, "length_in")?,
                    });
                    // The version the record was last read at: sent when
                    // the record exists, so a change by someone else is a
                    // clean refusal.
                    if let Some(Value::Record(current)) = args.get("current")
                        && let Some(version) = current.get("version").and_then(Value::as_int)
                    {
                        body["version"] = version.into();
                    }
                    let slow = hooks.save_takes_ms.load(Ordering::SeqCst);
                    if slow > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(slow)).await;
                    }
                    let request = state
                        .http
                        .put(format!("{}/api/orders/{order}/items/{item}", state.base))
                        .json(&body);
                    let sent = if hooks.detached_save.swap(false, Ordering::SeqCst) {
                        let (answer, answered) = tokio::sync::oneshot::channel();
                        let gate = Arc::clone(hooks);
                        let task = tokio::spawn(async move {
                            gate.late_gate.notified().await;
                            let _ = answer.send(request.send().await);
                        });
                        *hooks.late.lock().unwrap() = Some(task);
                        hooks.in_flight.notify_one();
                        match answered.await {
                            Ok(sent) => sent,
                            Err(_) => {
                                return Err(StepFailure::unknown(unavailable(
                                    "The save ended without an answer.",
                                )));
                            }
                        }
                    } else {
                        request.send().await
                    };
                    if hooks.lose_answer.swap(false, Ordering::SeqCst) {
                        return Err(StepFailure::unknown(unavailable(
                            "The connection was lost after the request was sent.",
                        )));
                    }
                    let response = sent.map_err(|_| {
                        StepFailure::unknown(unavailable("The application did not answer."))
                    })?;
                    let status = response.status().as_u16();
                    let answer: serde_json::Value =
                        response.json().await.unwrap_or(serde_json::Value::Null);
                    match status {
                        200 | 201 => Ok(StepOutcome::Done(Observed {
                            values: item_values(&answer),
                            ..Observed::default()
                        })),
                        409 if answer["error"] == "version_conflict" => {
                            Err(StepFailure::answered(failure(
                                FailureClass::Stale,
                                "version_conflict",
                                "The application refused the version sent with the write.",
                            )))
                        }
                        500..=599 => Err(StepFailure::answered(unavailable(
                            "The application answered that it is unavailable.",
                        ))),
                        _ => Err(StepFailure::answered(failure(
                            FailureClass::Rejected,
                            "example_refused",
                            "The application refused the line item.",
                        ))),
                    }
                }
                // A code handed out before the save: known before the
                // commit point, and recorded for reconcile.
                "reserve_code" => {
                    let order = text(args, "order_id")?;
                    let code = self.free_code(state, &order).await?;
                    let key =
                        RecordKey::new(vec![order.as_str().into(), code.as_str().into()]).unwrap();
                    Ok(StepOutcome::Done(Observed {
                        values: Values::new().with("item_code", code.as_str()),
                        assigned_key: Some(key),
                        for_reconcile: if hooks.records_nothing.load(Ordering::SeqCst) {
                            Values::new()
                        } else if hooks.records_order.load(Ordering::SeqCst) {
                            Values::new()
                                .with("item_code", code.as_str())
                                .with("order_id", order.as_str())
                        } else {
                            Values::new().with("item_code", code.as_str())
                        },
                        captures: Vec::new(),
                    }))
                }
                // A create whose code the system assigns. With no
                // description of its own, the record's stamp is written
                // into that field.
                "add_item" => {
                    if state.refuses(step) {
                        return Err(read_only());
                    }
                    let order = text(args, "order_id")?;
                    let code = match args.get("code") {
                        Some(Value::Text(code)) => code.clone(),
                        _ => self.free_code(state, &order).await?,
                    };
                    let description = match (args.get("description"), input.stamp) {
                        (Some(Value::Text(text)), _) => text.clone(),
                        (_, Some(stamp)) => format!("ref:{stamp}"),
                        _ => {
                            return Err(StepFailure::not_sent(failure(
                                FailureClass::UnknownVariant,
                                "example_unexpected",
                                "The step was given no description and no stamp.",
                            )));
                        }
                    };
                    let body = serde_json::json!({
                        "description": description,
                        "quantity": args.get("quantity").and_then(Value::as_int).unwrap_or(0),
                        "length_in": text(args, "length_in")?,
                    });
                    let sent = state
                        .http
                        .put(format!("{}/api/orders/{order}/items/{code}", state.base))
                        .json(&body)
                        .send()
                        .await;
                    if hooks.lose_answer.swap(false, Ordering::SeqCst) {
                        return Err(StepFailure::unknown(unavailable(
                            "The connection was lost after the request was sent.",
                        )));
                    }
                    let response = sent.map_err(|_| {
                        StepFailure::unknown(unavailable("The application did not answer."))
                    })?;
                    let status = response.status().as_u16();
                    let answer: serde_json::Value =
                        response.json().await.unwrap_or(serde_json::Value::Null);
                    match status {
                        200 | 201 => Ok(StepOutcome::Done(Observed {
                            values: item_values(&answer),
                            assigned_key: Some(
                                RecordKey::new(vec![order.into(), code.into()]).unwrap(),
                            ),
                            ..Observed::default()
                        })),
                        _ => Err(StepFailure::answered(failure(
                            FailureClass::Rejected,
                            "example_refused",
                            "The application refused the line item.",
                        ))),
                    }
                }
                // A lookup: by a description the write sets, or, with none
                // given, by the record's stamp.
                "find_item" => {
                    let order = text(args, "order_id")?;
                    let wanted = match (args.get("description"), input.stamp) {
                        (Some(Value::Text(text)), _) => text.clone(),
                        (_, Some(stamp)) => format!("ref:{stamp}"),
                        _ => {
                            return Err(StepFailure::not_sent(failure(
                                FailureClass::UnknownVariant,
                                "example_unexpected",
                                "The lookup was given nothing to look for.",
                            )));
                        }
                    };
                    if hooks
                        .lookup_lags
                        .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                            left.checked_sub(1)
                        })
                        .is_ok()
                    {
                        return Ok(StepOutcome::Absent(Observed::default()));
                    }
                    let items = self.items(state, &order).await?;
                    let found: Vec<&serde_json::Value> = items
                        .iter()
                        .filter(|item| item["description"].as_str() == Some(wanted.as_str()))
                        .collect();
                    match found.as_slice() {
                        [] => Ok(StepOutcome::Absent(Observed::default())),
                        [item] => {
                            let code = item["item_code"].as_str().unwrap_or_default();
                            Ok(StepOutcome::Done(Observed {
                                values: item_values(item),
                                assigned_key: Some(
                                    RecordKey::new(vec![order.into(), code.into()]).unwrap(),
                                ),
                                ..Observed::default()
                            }))
                        }
                        _ => Err(StepFailure::answered(ambiguous())),
                    }
                }
                _ => Err(StepFailure::not_sent(failure(
                    FailureClass::UnknownVariant,
                    "example_unexpected",
                    "The driver has no such step.",
                ))),
            };
            match (done, fails_after) {
                // The step was carried out, and the driver did not accept
                // what came back.
                (Ok(_), Some((_, class, code))) => Err(StepFailure::answered(failure(
                    class,
                    code,
                    "The driver did not accept the answer to a step it had sent.",
                ))),
                (done, _) => done,
            }
        })
    }
}

impl Driver for AppDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        _lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move {
            self.hooks
                .postures
                .lock()
                .unwrap()
                .push(format!("{:?}", cx.posture));
            self.hooks.prepared_with.lock().unwrap().push((
                cx.paths
                    .project()
                    .map(|project| project.as_path().to_path_buf()),
                cx.destinations.iter().map(str::to_owned).collect(),
            ));
            self.hooks.scoped_with.lock().unwrap().push(
                cx.destinations
                    .iter()
                    .map(|address| {
                        (
                            address.to_owned(),
                            cx.destinations.scope_of(address).cloned(),
                        )
                    })
                    .collect(),
            );
            if self.hooks.prepare_hangs.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            Ok(Prepared::new(AppState {
                base: cx.surface.surface.address.clone(),
                // The workspace's one maker of clients: one built by hand
                // stops the process in a build with TLS on (decision 0157).
                http: a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
                    .build()
                    .unwrap(),
                posture: cx.posture,
            }))
        })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let asks = self.hooks.asks_elsewhere.lock().unwrap().clone();
            if let Some((kind, name, value)) = asks
                && kind == step.kind.as_str()
            {
                let answer = prepared
                    .recorded_elsewhere(&name, &value, input.deadline)
                    .await;
                self.hooks.elsewhere.lock().unwrap().push(answer);
            }
            let mut done = self.act_inner(prepared, step, input).await;
            let altered = self.hooks.mode_alters.lock().unwrap().clone();
            if let (Ok(StepOutcome::Done(observed)), Some((mode, name, value))) =
                (&mut done, altered)
                && mode == self.mode.as_str()
            {
                observed.values.insert(name, value);
            }
            let planted = self.hooks.captures_with.lock().unwrap().clone();
            if let (Ok(StepOutcome::Done(observed)), Some((kind, text))) = (&mut done, planted)
                && kind == step.kind.as_str()
            {
                observed.captures.push(a2m_provider::Capture {
                    name: "screen".to_owned(),
                    media_type: "text/plain".to_owned(),
                    bytes: text.into_bytes(),
                });
            }
            // What the system says with a failure it answered with.
            match (done, self.hooks.says.lock().unwrap().clone()) {
                (Err(failure), Some(said)) if failure.sent == a2m_provider::Sent::Answered => {
                    Err(failure.with_said(said))
                }
                (done, _) => done,
            }
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        DriverHealth::Ready
    }

    fn release<'a>(&'a self, _prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move { Cleanup::complete() })
    }
}

/// A discoverer that tries to write through its port, and then reads.
/// Crates above the engine share this helper by path: they make one with
/// `default()`, so a field added here does not break them.
#[derive(Default)]
pub struct WritingDiscoverer {
    pub results: Mutex<Vec<Result<StepOutcome, StepFailure>>>,
    /// The discoverer never ends.
    pub hangs: AtomicBool,
    /// The discoverer panics after its first finding.
    pub panics: AtomicBool,
    /// The project folder each discovery's scope was handed.
    pub projects: Mutex<Vec<Option<PathBuf>>>,
    /// The files each discovery's scope was handed, by name, with what
    /// reading each gave.
    pub handed: Mutex<Vec<Handed>>,
}

/// The files one discovery's scope was handed: each name, and the bytes
/// reading it gave.
pub type Handed = Vec<(String, Vec<u8>)>;

impl Discoverer for WritingDiscoverer {
    fn name(&self) -> DiscovererName {
        DiscovererName::new("look").unwrap()
    }

    fn suits(&self, _surface: &SurfaceDescription) -> bool {
        true
    }

    fn discover<'a>(
        &'a self,
        port: &'a dyn SandboxPort,
        scope: &'a DiscoveryScope<'a>,
        out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
            self.projects.lock().unwrap().push(
                scope
                    .paths
                    .project()
                    .map(|project| project.as_path().to_path_buf()),
            );
            let handed = scope
                .paths
                .iter()
                .map(|(name, path)| (name.to_owned(), path.read(scope.bounds.bytes).unwrap()))
                .collect();
            self.handed.lock().unwrap().push(handed);
            // One finding, as a discoverer of a live system makes it.
            let finding = Finding::about(
                Subject::Address(Address::new("http://example.invalid", "/api/orders").unwrap()),
                FindingKind::new("call").unwrap(),
                "GET /api/orders/{order_id}",
                Source::LiveSystem,
                ProducedBy {
                    provider: "example_json".to_owned(),
                    version: "0.1.0".to_owned(),
                    component: None,
                },
                Timestamp::from_unix_millis(START),
            )
            .level(EvidenceLevel::Discovered)
            .build()
            .unwrap();
            out.found(finding.clone())?;
            // A discoverer that floods: as many more findings as the
            // job's focus asks for.
            let flood = scope
                .focus
                .get("flood")
                .and_then(Value::as_int)
                .unwrap_or(0);
            for _ in 0..flood {
                out.found(finding.clone())?;
            }
            if self.hangs.load(Ordering::SeqCst) {
                // Ended by the time its manifest declares, which is long
                // enough for any honest look on a slow machine: the clock
                // stops while it hangs, as for a call to `Control` that
                // hangs (see `StoreControl::hang`).
                tokio::time::pause();
                let _runs_again = ClockRunsAgain;
                std::future::pending::<()>().await;
            }
            assert!(
                !self.panics.load(Ordering::SeqCst),
                "the test discoverer was told to panic"
            );
            let args = Values::new()
                .with("order_id", "A-100")
                .with("item_code", "L-900")
                .with("description", "Planted by a discoverer")
                .with("quantity", 1_i64)
                .with("length_in", "1");
            let input = StepInput {
                args: &args,
                record: None,
                key: None,
                stamp: None,
                deadline: scope.deadline,
            };
            // A planted step that commits, the same step dressed as one
            // that commits nothing, and a read. Neither of the first two
            // may change the system.
            let write = step("put_item", &[], true, false);
            let first = port.act(&write, &input).await;
            let dressed = step("put_item", &[], false, false);
            let second = port.act(&dressed, &input).await;
            let read = step("get_item", &[], false, true);
            let third = port.act(&read, &input).await;
            let mut results = self.results.lock().unwrap();
            results.push(first);
            results.push(second);
            results.push(third);
            Ok(DiscoveryReport::default())
        })
    }
}

struct Plain;

impl Describer for Plain {
    fn describe(&self, findings: &[Finding], _form: DescribeForm) -> Description {
        Description {
            text: format!("{} findings", findings.len()),
            ..Description::default()
        }
    }
}

pub struct AppPack {
    pub hooks: Arc<Hooks>,
    pub discoverer: Arc<WritingDiscoverer>,
}

impl Pack for AppPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        for name in [FIRST_MODE, SECOND_MODE] {
            let mode = Mode::new(name).unwrap();
            to.driver(
                mode.clone(),
                Arc::new(AppDriver {
                    mode,
                    hooks: Arc::clone(&self.hooks),
                }),
            )?;
        }
        to.discoverer(self.discoverer.clone(), Arc::new(Plain))?;
        Ok(())
    }
}

// ---- the plan, built by hand ----

pub struct PlanBuilder {
    plan: Plan,
}

pub fn expr(plan: &mut Plan, source: &str, result: ValueType) -> ExprIndex {
    plan.expressions.push(PlanExpr {
        source: source.to_owned(),
        result,
        library: LibraryVersion::CURRENT.get(),
    });
    ExprIndex::new(u32::try_from(plan.expressions.len() - 1).unwrap())
}

fn field_type(plan: &mut Plan, name: &str, rule: &FieldRule, sensitivity: Sensitivity) -> u32 {
    let compiled = rule.normalizer().unwrap();
    let ty = rule.value_type();
    let normalizer = expr(plan, compiled.source(), ty.clone());
    let format = match rule {
        FieldRule::Text { whitespace, .. } => FieldFormat::Text {
            case_sensitive: true,
            whitespace: match whitespace {
                Whitespace::Exact => WhitespaceRule::Exact,
                _ => WhitespaceRule::Trim,
            },
        },
        _ => FieldFormat::Number {
            decimal_mark: a2m_core::DecimalMark::Point,
        },
    };
    plan.field_types.push(FieldTypePlan {
        name: name.to_owned(),
        base: rule.base(),
        ty,
        places: None,
        rounding: None,
        step: None,
        unit: None,
        display_unit: None,
        currency: None,
        zone: None,
        codes: Vec::new(),
        sensitivity,
        untrusted: false,
        format,
        normalizer: Some(normalizer),
    });
    u32::try_from(plan.field_types.len() - 1).unwrap()
}

pub fn step(kind: &str, args: &[(&str, ExprIndex)], commits: bool, lookup: bool) -> Step {
    Step {
        kind: StepKind::new(kind).unwrap(),
        refers_to: None,
        args: args
            .iter()
            .map(|(name, value)| StepArg {
                name: (*name).to_owned(),
                value: *value,
            })
            .collect(),
        timeout_ms: 20_000,
        fails_as: FailsAs {
            class: if commits {
                FailureClass::PossibleCommit
            } else {
                FailureClass::Transient
            },
            code: FailureCode::fixed("example_timed_out"),
        },
        commits,
        lookup,
        idempotent: false,
        when: None,
        payload: PackPayload::none(),
    }
}

fn input(name: &str, field_type: u32, ty: ValueType, required: bool) -> InputPlan {
    InputPlan {
        name: name.to_owned(),
        field_type: FieldTypeIndex::new(field_type),
        ty,
        required,
        default: None,
        description: String::new(),
    }
}

fn tool(name: &OperationName, runs: ToolRuns) -> ToolPlan {
    ToolPlan {
        name: name.clone(),
        runs,
        module: None,
        title: name.to_string(),
        description: String::new(),
        description_by: a2m_core::DescriptionBy::Builder,
        input_schema: CanonicalJson::none(),
        output_schema: CanonicalJson::none(),
        annotations: ToolAnnotations {
            read_only: false,
            destructive: false,
            idempotent: true,
            open_world: false,
        },
    }
}

pub const ORDER_GET: &str = "example_order_get";
pub const STOCK_GET: &str = "example_stock_get";
pub const ITEM_UPSERT: &str = "example_item_upsert";
pub const ITEM_UPSERT_EXACT: &str = "example_item_upsert_exact";
pub const SECURE_GET: &str = "example_secure_order_get";
pub const ORDER_FILL: &str = "example_order_fill";
/// A create found again by a declared lookup: `find`.
pub const NOTE_ADD: &str = "example_note_add";
/// A create found again by its stamp: `stamped`.
pub const REF_ADD: &str = "example_ref_add";
/// A create whose key the system hands out before the save.
pub const SLOT_ADD: &str = "example_slot_add";
/// A pipeline over the rows of a sheet.
pub const SHEET_ENTER: &str = "example_sheet_enter";
/// A pipeline that creates a header and then its lines.
pub const KIT_ENTER: &str = "example_kit_enter";

/// The sheet the pipelines read: delimited text with a header row.
pub fn sheet(rows: &[(&str, &str, &str, &str)]) -> Vec<u8> {
    let mut text = String::from("Code,Description,Quantity,Length\n");
    for (code, description, quantity, length) in rows {
        text.push_str(&format!("{code},{description},{quantity},{length}\n"));
    }
    text.into_bytes()
}

/// The plan of the imaginary `example` target, over the test application
/// at `address`.
pub fn plan(address: &str) -> Plan {
    let name = |text: &str| OperationName::new(text).unwrap();
    let mut plan = Plan {
        header: PlanHeader {
            format: PLAN_FORMAT,
            minimum_engine: Version::new(0, 0, 1),
            compiler: Version::new(0, 0, 1),
            target: TargetName::new("example").unwrap(),
            version: Version::new(1, 0, 0),
            sources: Digest::of(b"hand-built"),
        },
        target: TargetPlan {
            title: "Example".to_owned(),
            modules: Vec::new(),
            default_environment: EnvironmentIndex::new(0),
            locale: None,
            timezone: None,
            capture: None,
            exclusive_session: false,
            drift_risk: None,
            authorization: None,
        },
        surfaces: Vec::new(),
        environments: vec![EnvironmentPlan {
            name: Environment::new("test").unwrap(),
            kind: EnvironmentKind::Test,
            addresses: Vec::new(),
            test_records: Vec::new(),
            approve_writes: None,
        }],
        auth: Vec::new(),
        field_types: Vec::new(),
        expressions: Vec::new(),
        operations: Vec::new(),
        pipelines: Vec::new(),
        tools: Vec::new(),
        packs: Vec::new(),
        tests: Vec::new(),
    };
    for surface in ["app", "secure"] {
        plan.surfaces.push(SurfacePlan {
            name: SurfaceName::new(surface).unwrap(),
            kind: SurfaceKind::new("json_app").unwrap(),
            address: address.to_owned(),
            same_session_as: None,
            stands: None,
            rate: None,
            programs: Vec::new(),
            payload: PackPayload::none(),
        });
    }
    // The second surface needs a sign-in; the first needs none.
    plan.auth.push(AuthPlan {
        surfaces: vec![SurfaceIndex::new(1)],
        mode: SignInMode::ManagedSession,
        provider: AuthProviderName::new("example_login").unwrap(),
        probe: ProbePlan::Operation {
            operation: OperationIndex::new(0),
            inputs: Values::new().with("order_id", "A-100"),
        },
        expiry_signals: Vec::new(),
        idle_ms: None,
        absolute_ms: None,
        sign_out: None,
        settings: Vec::new(),
        credentials: Vec::new(),
        attempt_limit: 1,
        movable: false,
    });

    let trimmed = FieldRule::Text {
        case_sensitive: true,
        whitespace: Whitespace::Trim,
    };
    let exact = FieldRule::Text {
        case_sensitive: true,
        whitespace: Whitespace::Exact,
    };
    let identifier = field_type(&mut plan, "identifier", &trimmed, Sensitivity::Public);
    let free_text = field_type(&mut plan, "free_text", &trimmed, Sensitivity::Internal);
    let exact_text = field_type(&mut plan, "exact_text", &exact, Sensitivity::Internal);
    let count = field_type(
        &mut plan,
        "count",
        &FieldRule::Integer {
            mark: DecimalMark::Point,
        },
        Sensitivity::Internal,
    );
    let inches = field_type(
        &mut plan,
        "inches",
        &FieldRule::Decimal {
            places: 4,
            rounding: Rounding::HalfUp,
            mark: DecimalMark::Point,
        },
        Sensitivity::Internal,
    );

    let order_id = expr(&mut plan, "input.order_id", ValueType::Text);
    let item_code = expr(&mut plan, "input.item_code", ValueType::Text);
    let description = expr(&mut plan, "input.description", ValueType::Text);
    let quantity = expr(&mut plan, "input.quantity", ValueType::Int);
    let length = expr(&mut plan, "input.length_in", ValueType::Decimal);
    // What the record held when it was last read, as the driver read it.
    let optional = |name: &str, ty: ValueType| a2m_core::FieldOf {
        name: name.to_owned(),
        ty,
        required: false,
    };
    let current = expr(
        &mut plan,
        "current",
        ValueType::Record(vec![
            optional("description", ValueType::Text),
            optional("quantity", ValueType::Int),
            optional("length_in", ValueType::Text),
            optional("version", ValueType::Int),
            optional("item_code", ValueType::Text),
        ]),
    );
    let no_manifest = || PermissionManifest {
        surfaces: vec![SurfaceIndex::new(0)],
        uploads: Vec::new(),
        downloads: Vec::new(),
        destinations: Vec::new(),
    };
    let binding = |mode: &str, surface: u32, act: Vec<Step>, read_back: Vec<Step>| BindingPlan {
        mode: Mode::new(mode).unwrap(),
        verb: Verb::new(if read_back.is_empty() {
            "read"
        } else {
            "upsert"
        })
        .unwrap(),
        status: BindingStatus::Verified,
        surface: SurfaceIndex::new(surface),
        act,
        confirm: Vec::new(),
        read_back,
        lookup: Vec::new(),
        settle_ms: 0,
        evidence: a2m_core::WriteEvidence::ReadBack,
        precondition: None,
    };
    let order_outputs = || {
        vec![
            OutputPlan {
                name: "order_id".to_owned(),
                ty: ValueType::Text,
                field_type: Some(FieldTypeIndex::new(identifier)),
                may_be_absent: false,
            },
            OutputPlan {
                name: "customer".to_owned(),
                ty: ValueType::Text,
                field_type: Some(FieldTypeIndex::new(free_text)),
                may_be_absent: false,
            },
            OutputPlan {
                name: "status".to_owned(),
                ty: ValueType::Text,
                field_type: Some(FieldTypeIndex::new(identifier)),
                may_be_absent: false,
            },
            OutputPlan {
                name: "lines".to_owned(),
                ty: ValueType::Int,
                field_type: None,
                may_be_absent: false,
            },
        ]
    };
    let read_operation = |text: &str, bindings: Vec<BindingPlan>| OperationPlan {
        name: name(text),
        effect: EffectClass::Read,
        hazard: Hazard::None,
        inputs: vec![input("order_id", identifier, ValueType::Text, true)],
        outputs: order_outputs(),
        identity: IdentityPlan {
            strategy: IdentityStrategy::Natural,
            key: vec![InputIndex::new(0)],
        },
        commit_points: 0,
        bindings,
        manifest: no_manifest(),
        examples: Vec::new(),
        decision_points: Vec::new(),
    };

    // 0: read an order, through two bindings in order.
    let get_order = || step("get_order", &[("order_id", order_id)], false, false);
    plan.operations.push(read_operation(
        ORDER_GET,
        vec![
            binding(FIRST_MODE, 0, vec![get_order()], Vec::new()),
            binding(SECOND_MODE, 0, vec![get_order()], Vec::new()),
        ],
    ));
    // 1: read stock, an endpoint that fails every third call.
    plan.operations.push(OperationPlan {
        name: name(STOCK_GET),
        effect: EffectClass::Read,
        hazard: Hazard::None,
        inputs: vec![input("item_code", identifier, ValueType::Text, true)],
        outputs: vec![OutputPlan {
            name: "in_stock".to_owned(),
            ty: ValueType::Int,
            field_type: None,
            may_be_absent: false,
        }],
        identity: IdentityPlan {
            strategy: IdentityStrategy::Natural,
            key: vec![InputIndex::new(0)],
        },
        commit_points: 0,
        bindings: vec![binding(
            FIRST_MODE,
            0,
            vec![step("get_stock", &[("item_code", item_code)], false, false)],
            Vec::new(),
        )],
        manifest: no_manifest(),
        examples: Vec::new(),
        decision_points: Vec::new(),
    });
    // 2 and 3: create or update a line item. In the second the description
    // is compared as written, and the application trims what it stores.
    for (text, description_type) in [(ITEM_UPSERT, free_text), (ITEM_UPSERT_EXACT, exact_text)] {
        let key = [("order_id", order_id), ("item_code", item_code)];
        plan.operations.push(OperationPlan {
            name: name(text),
            effect: EffectClass::Write,
            hazard: Hazard::None,
            inputs: vec![
                input("order_id", identifier, ValueType::Text, true),
                input("item_code", identifier, ValueType::Text, true),
                input("description", description_type, ValueType::Text, true),
                input("quantity", count, ValueType::Int, true),
                input("length_in", inches, ValueType::Decimal, true),
            ],
            outputs: vec![OutputPlan {
                name: "version".to_owned(),
                ty: ValueType::Int,
                field_type: None,
                may_be_absent: false,
            }],
            identity: IdentityPlan {
                strategy: IdentityStrategy::Composite,
                key: vec![InputIndex::new(0), InputIndex::new(1)],
            },
            commit_points: 1,
            bindings: vec![binding(
                FIRST_MODE,
                0,
                vec![step(
                    "put_item",
                    &[
                        ("order_id", order_id),
                        ("item_code", item_code),
                        ("description", description),
                        ("quantity", quantity),
                        ("length_in", length),
                        ("current", current),
                    ],
                    true,
                    false,
                )],
                vec![step("get_item", &key, false, true)],
            )],
            manifest: no_manifest(),
            examples: Vec::new(),
            decision_points: Vec::new(),
        });
    }
    // 4: a read on the surface that needs a sign-in.
    plan.operations.push(read_operation(
        SECURE_GET,
        vec![binding(FIRST_MODE, 1, vec![get_order()], Vec::new())],
    ));

    // 5 to 7: creates whose code the system assigns, each found again in
    // its own way.
    let result_code = expr(&mut plan, "result.item_code", ValueType::Text);
    let created_outputs = || {
        vec![
            OutputPlan {
                name: "item_code".to_owned(),
                ty: ValueType::Text,
                field_type: Some(FieldTypeIndex::new(identifier)),
                may_be_absent: false,
            },
            OutputPlan {
                name: "version".to_owned(),
                ty: ValueType::Int,
                field_type: None,
                may_be_absent: false,
            },
        ]
    };
    let read_created = || {
        vec![step(
            "get_item",
            &[("order_id", order_id), ("item_code", result_code)],
            false,
            true,
        )]
    };
    let create = |text: &str,
                  strategy: IdentityStrategy,
                  inputs: Vec<InputPlan>,
                  key: Vec<InputIndex>,
                  act: Vec<Step>,
                  lookup: Vec<Step>| {
        let mut binding = binding(FIRST_MODE, 0, act, read_created());
        binding.lookup = lookup;
        OperationPlan {
            name: name(text),
            effect: EffectClass::Write,
            hazard: Hazard::None,
            inputs,
            outputs: created_outputs(),
            identity: IdentityPlan { strategy, key },
            commit_points: 1,
            bindings: vec![binding],
            manifest: no_manifest(),
            examples: Vec::new(),
            decision_points: Vec::new(),
        }
    };
    // 5: `find`: the lookup searches on the description the write sets.
    plan.operations.push(create(
        NOTE_ADD,
        IdentityStrategy::Find,
        vec![
            input("order_id", identifier, ValueType::Text, true),
            input("description", free_text, ValueType::Text, true),
            input("quantity", count, ValueType::Int, true),
            input("length_in", inches, ValueType::Decimal, true),
        ],
        vec![InputIndex::new(0), InputIndex::new(1)],
        vec![step(
            "add_item",
            &[
                ("order_id", order_id),
                ("description", description),
                ("quantity", quantity),
                ("length_in", length),
            ],
            true,
            false,
        )],
        vec![step(
            "find_item",
            &[("order_id", order_id), ("description", description)],
            false,
            true,
        )],
    ));
    // 6: `stamped`: the write puts the record's stamp in the description,
    // and the lookup searches for it.
    let stamped_inputs = || {
        vec![
            input("order_id", identifier, ValueType::Text, true),
            input("quantity", count, ValueType::Int, true),
            input("length_in", inches, ValueType::Decimal, true),
        ]
    };
    let by_stamp = || vec![step("find_item", &[("order_id", order_id)], false, true)];
    plan.operations.push(create(
        REF_ADD,
        IdentityStrategy::Stamped,
        stamped_inputs(),
        vec![InputIndex::new(0)],
        vec![step(
            "add_item",
            &[
                ("order_id", order_id),
                ("quantity", quantity),
                ("length_in", length),
            ],
            true,
            false,
        )],
        by_stamp(),
    ));
    // 7: the code is handed out by a step before the save, and the save
    // and the read-back use it.
    plan.operations.push(create(
        SLOT_ADD,
        IdentityStrategy::Stamped,
        stamped_inputs(),
        vec![InputIndex::new(0)],
        vec![
            step("reserve_code", &[("order_id", order_id)], false, false),
            step(
                "add_item",
                &[
                    ("order_id", order_id),
                    ("code", result_code),
                    ("quantity", quantity),
                    ("length_in", length),
                ],
                true,
                false,
            ),
        ],
        by_stamp(),
    ));

    for (position, operation) in plan.operations.clone().iter().enumerate() {
        plan.tools.push(tool(
            &operation.name,
            ToolRuns::Operation(OperationIndex::new(u32::try_from(position).unwrap())),
        ));
    }

    // A pipeline: check the order is a draft, enter two line items, read
    // the order again.
    let draft = expr(&mut plan, "result.status == \"draft\"", ValueType::Bool);
    let first_code = expr(&mut plan, "input.first_code", ValueType::Text);
    let second_code = expr(&mut plan, "input.second_code", ValueType::Text);
    let enter = |id: &str, code: ExprIndex| PipelineStep {
        id: id.to_owned(),
        operation: OperationIndex::new(2),
        with: vec![
            StepArg {
                name: "order_id".to_owned(),
                value: order_id,
            },
            StepArg {
                name: "item_code".to_owned(),
                value: code,
            },
            StepArg {
                name: "description".to_owned(),
                value: description,
            },
            StepArg {
                name: "quantity".to_owned(),
                value: quantity,
            },
            StepArg {
                name: "length_in".to_owned(),
                value: length,
            },
        ],
        for_each: None,
        checkpoint: None,
        when: None,
        expect: None,
    };
    let read = |id: &str, expect: Option<ExprIndex>| PipelineStep {
        id: id.to_owned(),
        operation: OperationIndex::new(0),
        with: vec![StepArg {
            name: "order_id".to_owned(),
            value: order_id,
        }],
        for_each: None,
        checkpoint: None,
        when: None,
        expect,
    };
    plan.pipelines.push(PipelinePlan {
        name: name(ORDER_FILL),
        effect: EffectClass::Write,
        hazard: Hazard::None,
        inputs: vec![
            input("order_id", identifier, ValueType::Text, true),
            input("first_code", identifier, ValueType::Text, true),
            input("second_code", identifier, ValueType::Text, true),
            input("description", free_text, ValueType::Text, true),
            input("quantity", count, ValueType::Int, true),
            input("length_in", inches, ValueType::Decimal, true),
        ],
        source: None,
        limits: PipelineLimits::default(),
        steps: vec![
            read("check", Some(draft)),
            enter("first", first_code),
            enter("second", second_code),
            read("after", None),
        ],
        success: Vec::new(),
        headroom_ms: 0,
    });
    plan.tools.push(tool(
        &name(ORDER_FILL),
        ToolRuns::Pipeline(PipelineIndex::new(0)),
    ));

    // The source both sheet pipelines read: four columns, one row a record.
    let limits = a2m_sources::Limits {
        bytes: 1024 * 1024,
        unpacked_bytes: 1024 * 1024,
        rows: 1000,
        cells: 10_000,
        cell_bytes: 1024,
    };
    let layout = Sources::builtin()
        .describe(
            "csv",
            &SourceInput {
                bytes: &sheet(&[]),
                limits,
                options: ReaderOptions::default(),
            },
        )
        .unwrap();
    let row_field = |name: &str, column: &str, field_type: u32| RowField {
        name: name.to_owned(),
        field_type: FieldTypeIndex::new(field_type),
        column: Some(column.to_owned()),
        required: true,
    };
    let source = || SourcePlan {
        template: "example_sheet".to_owned(),
        reader: "csv".to_owned(),
        mapping: Vec::new(),
        rows: None,
        payload: PackPayload::encode(&SourceSpec {
            versions: vec![TemplateVersion {
                version: "1".to_owned(),
                fingerprint: layout.fingerprint.clone(),
            }],
            locate: Locate::Document,
            columns: ["Code", "Description", "Quantity", "Length"]
                .map(str::to_owned)
                .to_vec(),
            fields: vec![
                row_field("code", "Code", identifier),
                row_field("description", "Description", free_text),
                row_field("quantity", "Quantity", count),
                row_field("length", "Length", inches),
            ],
            key: vec!["code".to_owned()],
            missing: Vec::new(),
            hidden: HiddenRule::Reject,
            options: ReaderOptions::default(),
        })
        .unwrap(),
    };
    let column = |name: &str, ty: ValueType| a2m_core::FieldOf {
        name: name.to_owned(),
        ty,
        required: true,
    };
    let rows = expr(
        &mut plan,
        "source.rows",
        ValueType::List(Box::new(ValueType::Record(vec![
            column("code", ValueType::Text),
            column("description", ValueType::Text),
            column("quantity", ValueType::Int),
            column("length", ValueType::Decimal),
        ]))),
    );
    let row_code = expr(&mut plan, "row.code", ValueType::Text);
    let row_description = expr(&mut plan, "row.description", ValueType::Text);
    let row_quantity = expr(&mut plan, "row.quantity", ValueType::Int);
    let row_length = expr(&mut plan, "row.length", ValueType::Decimal);
    let arg = |name: &str, value: ExprIndex| StepArg {
        name: name.to_owned(),
        value,
    };
    let lines = |code: ExprIndex| PipelineStep {
        id: "lines".to_owned(),
        operation: OperationIndex::new(2),
        with: vec![
            arg("order_id", order_id),
            arg("item_code", code),
            arg("description", row_description),
            arg("quantity", row_quantity),
            arg("length_in", row_length),
        ],
        for_each: Some(rows),
        checkpoint: None,
        when: None,
        expect: None,
    };
    // 1: check the order is a draft, enter every row, read the order again.
    plan.pipelines.push(PipelinePlan {
        name: name(SHEET_ENTER),
        effect: EffectClass::Write,
        hazard: Hazard::None,
        inputs: vec![input("order_id", identifier, ValueType::Text, true)],
        source: Some(source()),
        limits: PipelineLimits::default(),
        steps: vec![
            read("check", Some(draft)),
            lines(row_code),
            read("after", None),
        ],
        success: Vec::new(),
        headroom_ms: 0,
    });
    plan.tools.push(tool(
        &name(SHEET_ENTER),
        ToolRuns::Pipeline(PipelineIndex::new(1)),
    ));
    // 2: a header whose code the system assigns, then its lines, each
    // under a code made from the header's.
    let title = expr(&mut plan, "input.title", ValueType::Text);
    let one = expr(&mut plan, "1", ValueType::Int);
    let unit = expr(&mut plan, "decimal(\"1\")", ValueType::Decimal);
    let under_header = expr(
        &mut plan,
        "steps.header.result.item_code + \"-\" + row.code",
        ValueType::Text,
    );
    plan.pipelines.push(PipelinePlan {
        name: name(KIT_ENTER),
        effect: EffectClass::Write,
        hazard: Hazard::None,
        inputs: vec![
            input("order_id", identifier, ValueType::Text, true),
            input("title", free_text, ValueType::Text, true),
        ],
        source: Some(source()),
        limits: PipelineLimits::default(),
        steps: vec![
            PipelineStep {
                id: "header".to_owned(),
                operation: OperationIndex::new(5),
                with: vec![
                    arg("order_id", order_id),
                    arg("description", title),
                    arg("quantity", one),
                    arg("length_in", unit),
                ],
                for_each: None,
                checkpoint: None,
                when: None,
                expect: None,
            },
            lines(under_header),
        ],
        success: Vec::new(),
        headroom_ms: 0,
    });
    plan.tools.push(tool(
        &name(KIT_ENTER),
        ToolRuns::Pipeline(PipelineIndex::new(2)),
    ));
    plan
}

/// The plan changed so that the create whose code a step hands out before
/// the save is a record of no identity: it is found again only by the code
/// its earlier step recorded, through a read-back that looks for it
/// (`taken`), as a take from a queue is by the message id it recorded. A
/// write of no identity is irreversible.
pub fn keyless_slot_add(plan: &mut Plan) {
    for operation in &mut plan.operations {
        if operation.name.as_str() == SLOT_ADD {
            // A record that cannot be found by a key is written only by an
            // irreversible operation.
            operation.effect = EffectClass::Irreversible;
            operation.identity = IdentityPlan {
                strategy: IdentityStrategy::None,
                key: Vec::new(),
            };
            for binding in &mut operation.bindings {
                let order_id = binding.read_back[0].args[0].value;
                binding.read_back = vec![step("taken", &[("order_id", order_id)], false, true)];
                binding.lookup = Vec::new();
            }
        }
    }
}

/// As [`keyless_slot_add`], and the binding also has a lookup that scans
/// the order for the record's stamp (`scan`), whose "absent" the first
/// mode's manifest states authoritative, with a settle time of `settle_ms`.
pub fn keyless_slot_add_scanned(plan: &mut Plan, settle_ms: u32) {
    keyless_slot_add(plan);
    for operation in &mut plan.operations {
        if operation.name.as_str() == SLOT_ADD {
            for binding in &mut operation.bindings {
                let order_id = binding.read_back[0].args[0].value;
                binding.lookup = vec![step("scan", &[("order_id", order_id)], false, true)];
                binding.settle_ms = settle_ms;
            }
        }
    }
}

/// The plan changed so that the upsert is irreversible and its only
/// evidence is the system's acknowledgement (`acknowledged`): nothing is
/// read back after its save. Every pipeline, which enters line items
/// through it, becomes irreversible too, and no tool is read only.
pub fn acknowledged_upsert(plan: &mut Plan) {
    for operation in &mut plan.operations {
        if operation.name.as_str() == ITEM_UPSERT {
            operation.effect = EffectClass::Irreversible;
            for binding in &mut operation.bindings {
                binding.evidence = a2m_core::WriteEvidence::Acknowledged;
                binding.read_back.clear();
            }
        }
    }
    for pipeline in &mut plan.pipelines {
        pipeline.effect = EffectClass::Irreversible;
    }
    for tool in &mut plan.tools {
        tool.annotations.read_only = false;
    }
}

// ---- the control plane: a real store behind `Control` ----

/// What a test can make the control plane do.
#[derive(Default)]
pub struct Faults {
    /// The next `intend` is refused before anything is stored.
    pub intend_refused: AtomicBool,
    /// The next `intend` stores the intents, and the answer is lost.
    pub intend_answer_lost: AtomicBool,
    /// Evidence cannot be stored, this many times, after a write was sent.
    pub evidence_full: AtomicU32,
    /// The policy decision at guard is a refusal.
    pub denied: AtomicBool,
    /// Every lease is refused: a person must sign in.
    pub no_session: AtomicBool,
    /// The store cannot commit once a write has been sent: `acted` is
    /// refused.
    pub acted_refused: AtomicBool,
    /// The store cannot commit when the record is to be marked verified.
    pub verified_refused: AtomicBool,
    /// The evidence store takes nothing at all.
    pub evidence_down: AtomicBool,
    /// What a step learned before its commit point cannot be stored.
    pub before_commit_refused: AtomicBool,
    /// The first piece of evidence handed over once a save has reached the
    /// driver cannot be stored: the outputs of the record.
    pub outputs_refused: AtomicBool,
    /// The call of this name never answers.
    pub hangs_on: Mutex<Option<&'static str>>,
    /// When `abort_armed` is set, the calls to `Control` answered before
    /// the next one never answers, and [`Faults::aborted`] is notified: the
    /// test then drops the job's future, as a process killed at that point
    /// would leave it.
    pub abort_after: AtomicU32,
    /// Whether `abort_after` applies.
    pub abort_armed: AtomicBool,
    /// The calls to `Control` made since the count was last reset.
    pub calls: AtomicU32,
    /// Notified when the call after `abort_after` is held.
    pub aborted: tokio::sync::Notify,
}

pub struct StoreControl {
    pub store: Store,
    /// The key to the policy engine's tables, which this stand-in for the
    /// policy engine takes from the store, as `Policy::open` does.
    pub policy_key: Arc<a2m_ledger::PolicyKey>,
    pub plan: Arc<Plan>,
    pub fence: Mutex<Fence>,
    pub hooks: Arc<Hooks>,
    pub faults: Faults,
    /// An approval a person gave, not yet used.
    pub approval: Mutex<Option<ApprovalId>>,
    pub results: Mutex<Vec<Values>>,
    pub traces: Mutex<Vec<FinishedTrace>>,
    pub events: Mutex<Vec<EngineEvent>>,
    pub questions: Mutex<Vec<GuardQuestion>>,
    pub wants: Mutex<Vec<SessionWant>>,
    pub expiry_reports: AtomicU32,
    pub modes: Mutex<Vec<(RecordRef, Mode)>>,
    pub sent: AtomicBool,
    /// What was recorded before each record's commit point.
    pub before_commit: Mutex<Vec<(RecordRef, BlobRef)>>,
    /// Every piece of evidence the engine handed over, as it was given.
    pub evidence_given: Mutex<Vec<Vec<u8>>>,
    /// Whether the control plane keeps captures, and those it kept.
    pub keeps_captures: AtomicBool,
    pub captures: Mutex<Vec<(KeptCapture, BlobRef)>>,
    /// Whether the control plane ends a record on the system's
    /// acknowledgement, and the records it ended so.
    pub keeps_acknowledgements: AtomicBool,
    pub acknowledged: Mutex<Vec<RecordRef>>,
    /// The project folder of the request that started the run.
    pub project: Mutex<Option<PathBuf>>,
    /// The files of the project's `sources` folder the call that started a
    /// discovery hands it, by name.
    pub discovery_files: Mutex<std::collections::BTreeMap<String, String>>,
    /// The answerer `other_records` hands out, and every operation and
    /// record it was asked for.
    pub others: Mutex<Option<Arc<dyn a2m_provider::OtherRecords>>>,
    pub others_asked: Mutex<Vec<(String, RecordRef)>>,
    /// Whether the control plane keeps shadow comparisons, and those the
    /// engine handed it, in order.
    pub keeps_shadow: AtomicBool,
    pub shadows: Mutex<Vec<a2m_engine::ShadowCompared>>,
    /// The shadow's rate budget is refused, as a surface whose callers'
    /// share is all that is left refuses it.
    pub shadow_rate_refused: AtomicBool,
    /// The requests granted to shadows through `shadow_rate`.
    pub shadow_requests: AtomicU32,
    /// Keeping a comparison fails.
    pub shadow_write_fails: AtomicBool,
    /// Keeping a comparison never ends.
    pub shadow_write_hangs: AtomicBool,
}

/// Starts the clock of the runtime again when a call that hangs is given
/// up, which drops the call.
struct ClockRunsAgain;

impl Drop for ClockRunsAgain {
    fn drop(&mut self) {
        tokio::time::resume();
    }
}

impl StoreControl {
    fn fence(&self) -> Fence {
        *self.fence.lock().unwrap()
    }

    /// Never answers, when the test asked for that of this call.
    ///
    /// The engine's own limit on a call to `Control` is what must end it,
    /// and that limit must never end an honest call however slow the
    /// machine is. So every call is made in real time under the engine's
    /// limit, and only this one is made in the test's time: the clock of
    /// the runtime stops when the call starts to hang, and, with nothing
    /// else to do, the runtime moves it straight on to the engine's limit.
    /// The clock runs again when the engine gives the call up. A test's
    /// runtime runs on one thread, which is what lets a test stop its clock.
    /// Counts one call to `Control`. Once `abort_after` calls have been
    /// answered, with the abort armed, this one never answers.
    async fn called(&self) {
        let made = self.faults.calls.fetch_add(1, Ordering::SeqCst);
        if self.faults.abort_armed.load(Ordering::SeqCst)
            && made >= self.faults.abort_after.load(Ordering::SeqCst)
        {
            self.faults.aborted.notify_one();
            std::future::pending::<()>().await;
        }
    }

    async fn hang(&self, call: &'static str) {
        let hangs = *self.faults.hangs_on.lock().unwrap() == Some(call);
        if hangs {
            tokio::time::pause();
            let _runs_again = ClockRunsAgain;
            std::future::pending::<()>().await;
        }
    }

    /// A person approves a diff. The policy engine's stand-in: one row in
    /// its own table, with the key it took from the store, for the run's
    /// tool version, with an expiry and a presence check, as the policy
    /// engine writes it.
    pub async fn approve(&self, diff: Digest) {
        let run = self.fence().run;
        let key = Arc::clone(&self.policy_key);
        let id = self
            .store
            .write(Durability::Synced, move |tx| {
                let id = ApprovalId::from_parts(tx.now(), tx.random()?)?;
                let now = tx.now();
                let version = tx.ledger().run(run)?.target_version;
                let expires = a2m_core::Timestamp::from_unix_millis(
                    now.as_unix_millis().saturating_add(3_600_000),
                );
                tx.policy_tables(&key)?.execute(
                    "INSERT INTO approvals
                        (id, workspace_id, scope, run_id, diff_hash, tool_version_hash, approver_id,
                         channel, presence_check, granted_at, expires_at)
                     VALUES (?1, ?2, 'run', ?3, ?4, ?5, ?6, 'local_prompt', 'os_prompt', ?7, ?8)",
                    &[
                        id.to_string().into(),
                        workspace().to_string().into(),
                        run.to_string().into(),
                        diff.to_string().into(),
                        version.to_string().into(),
                        PrincipalId::from_bytes([9; 16]).to_string().into(),
                        now.into(),
                        expires.into(),
                    ],
                )?;
                Ok(id)
            })
            .await
            .unwrap();
        *self.approval.lock().unwrap() = Some(id);
    }
}

impl Control for StoreControl {
    fn plan<'a>(&'a self, _hash: &'a Digest) -> BoxFuture<'a, Result<Arc<Plan>, Failure>> {
        Box::pin(async move {
            self.called().await;
            Ok(Arc::clone(&self.plan))
        })
    }

    fn decide<'a>(&'a self, question: &'a GuardQuestion) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            self.called().await;
            self.hang("decide").await;
            self.questions.lock().unwrap().push(question.clone());
            if self.faults.denied.load(Ordering::SeqCst) {
                return Err(failure(
                    FailureClass::Policy,
                    "not_granted",
                    "The caller may not call this tool.",
                ));
            }
            Ok(())
        })
    }

    fn session_lease<'a>(
        &'a self,
        want: &'a SessionWant,
    ) -> BoxFuture<'a, Result<SessionLease, Failure>> {
        Box::pin(async move {
            self.called().await;
            self.wants.lock().unwrap().push(want.clone());
            // No test signs a person in, so every lease is refused as the
            // keeper refuses one for a session that needs a person.
            Err(Failure::new(
                FailureClass::SessionExpired,
                a2m_auth::codes::NEEDS_LOGIN,
                "The session has ended and a person must sign in.",
                "Sign in again, then resume the run.",
            ))
        })
    }

    fn report_expiry<'a>(
        &'a self,
        _lease: &'a SessionLease,
        _sign: ExpirySign,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.called().await;
            self.expiry_reports.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn rate<'a>(
        &'a self,
        _surface: &'a SurfaceName,
        want: u32,
    ) -> BoxFuture<'a, Result<u32, Failure>> {
        Box::pin(async move {
            self.called().await;
            Ok(want)
        })
    }

    fn store_preview<'a>(&'a self, preview: &'a Preview) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            self.called().await;
            let fence = self.fence();
            let preview = preview.clone();
            self.hooks.note("store_preview");
            self.store
                .write(Durability::Synced, move |tx| {
                    let mut ledger = tx.ledger();
                    ledger.plan_records(&fence, &preview.records)?;
                    ledger.store_diff(&fence, preview.diff, Some(preview.diff_blob))
                })
                .await
        })
    }

    fn approval<'a>(&'a self, _diff: &'a Digest) -> BoxFuture<'a, Result<Clearance, Failure>> {
        Box::pin(async move {
            self.called().await;
            let fence = self.fence();
            let Some(approval) = *self.approval.lock().unwrap() else {
                return Ok(Clearance::Waits);
            };
            // Marking the approval used and moving the run happen in one
            // transaction; the ledger checks the approval is for this diff.
            let key = Arc::clone(&self.policy_key);
            let cleared = self
                .store
                .write(Durability::Synced, move |tx| {
                    let now = tx.now();
                    tx.policy_tables(&key)?.execute(
                        "UPDATE approvals SET used_at = ?2
                         WHERE id = ?1 AND used_at IS NULL AND revoked_at IS NULL",
                        &[approval.to_string().into(), now.into()],
                    )?;
                    tx.ledger()
                        .begin_writes(&fence, WriteClearance::Approval(approval))
                })
                .await;
            cleared.map(|()| Clearance::Cleared)
        })
    }

    fn lock<'a>(&'a self, record: RecordRef) -> BoxFuture<'a, Result<LockHeld, Failure>> {
        Box::pin(async move {
            self.called().await;
            let fence = self.fence();
            self.hooks.note("lock");
            self.store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().take_lock(&fence, record)
                })
                .await
        })
    }

    fn intend<'a>(
        &'a self,
        intents: Vec<RecordIntent>,
    ) -> BoxFuture<'a, Result<Vec<IntentToken>, Failure>> {
        Box::pin(async move {
            self.called().await;
            self.hang("intend").await;
            if self.faults.intend_refused.swap(false, Ordering::SeqCst) {
                return Err(failure(
                    FailureClass::Busy,
                    "store_unavailable",
                    "The store cannot commit.",
                ));
            }
            for intent in &intents {
                if let Some(mode) = &intent.mode {
                    self.modes
                        .lock()
                        .unwrap()
                        .push((intent.record, mode.clone()));
                }
            }
            let attempts: Vec<RecordRef> = intents.iter().map(|intent| intent.record).collect();
            let tokens = self.store.store_intents(&self.fence(), intents).await?;
            // A new attempt of a record records its own values before its
            // save; what an earlier attempt recorded is not this one's, as
            // the runner reads it from the ledger (decision 0185).
            self.before_commit
                .lock()
                .unwrap()
                .retain(|(record, _)| !attempts.contains(record));
            self.hooks.note("intend");
            if self.faults.intend_answer_lost.swap(false, Ordering::SeqCst) {
                return Err(failure(
                    FailureClass::Busy,
                    "store_unavailable",
                    "The store's answer was lost.",
                ));
            }
            Ok(tokens)
        })
    }

    fn acted<'a>(
        &'a self,
        token: IntentToken,
        acted: Acted,
    ) -> BoxFuture<'a, Result<ActedToken, Failure>> {
        Box::pin(async move {
            self.called().await;
            self.sent.store(true, Ordering::SeqCst);
            self.hang("acted").await;
            if self.faults.acted_refused.load(Ordering::SeqCst) {
                return Err(failure(
                    FailureClass::Busy,
                    "store_unavailable",
                    "The store cannot commit.",
                ));
            }
            self.hooks.note("acted");
            self.store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().mark_acted(token, acted)
                })
                .await
        })
    }

    fn verified<'a>(
        &'a self,
        token: &'a ActedToken,
        read_back: ReadBack,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            self.called().await;
            if self.faults.verified_refused.load(Ordering::SeqCst) {
                return Err(failure(
                    FailureClass::Busy,
                    "store_unavailable",
                    "The store cannot commit.",
                ));
            }
            self.hooks.note("verified");
            // The store's transaction runs elsewhere and owns what it is
            // given, and the proof is borrowed: the record is settled by
            // name, under the lease the proof was made in. The ledger makes
            // the same comparison of digests for a record that is `acted`.
            let record = token.record();
            let fence = Fence {
                run: record.run,
                epoch: token.epoch(),
            };
            let settled = self
                .store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().settle(
                        &fence,
                        record,
                        Reconciled::Landed {
                            read: read_back,
                            acted: a2m_ledger::Acted::default(),
                        },
                    )
                })
                .await?;
            match settled {
                Settled::Verified => Ok(()),
                other => Err(failure(
                    FailureClass::UnknownVariant,
                    "ledger_state_unexpected",
                    &format!("The record was settled as {other:?}, not verified."),
                )),
            }
        })
    }

    fn failed<'a>(
        &'a self,
        record: RecordRef,
        why: &'a Failure,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            self.called().await;
            let fence = self.fence();
            let why = why.clone();
            self.hooks.note(format!("failed:{}", why.class()));
            self.store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().mark_failed(&fence, record, &why, evidence)
                })
                .await
        })
    }

    fn settle<'a>(
        &'a self,
        record: RecordRef,
        seen: Reconciled,
    ) -> BoxFuture<'a, Result<Settled, Failure>> {
        Box::pin(async move {
            self.called().await;
            let fence = self.fence();
            let settled = self
                .store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().settle(&fence, record, seen)
                })
                .await?;
            self.hooks.note(format!("settle:{settled:?}"));
            Ok(settled)
        })
    }

    fn send<'a>(&'a self, event: EngineEvent) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            self.called().await;
            let fence = self.fence();
            match &event {
                EngineEvent::Moved(to) => {
                    let to = *to;
                    self.hooks.note(format!("moved:{to}"));
                    self.store
                        .write(Durability::Synced, move |tx| {
                            tx.ledger().set_state(&fence, to, None)
                        })
                        .await?;
                }
                EngineEvent::Result(values) => {
                    self.results.lock().unwrap().push(values.clone());
                }
                EngineEvent::Trace(trace) => {
                    self.traces.lock().unwrap().push(trace.clone());
                }
                EngineEvent::BeforeCommit { record, outputs } => {
                    if self.faults.before_commit_refused.load(Ordering::SeqCst) {
                        return Err(failure(
                            FailureClass::Busy,
                            "store_unavailable",
                            "The store cannot commit.",
                        ));
                    }
                    self.hooks.note("before_commit");
                    self.before_commit.lock().unwrap().push((*record, *outputs));
                    let outputs = *outputs;
                    self.store
                        .write(Durability::Synced, move |tx| {
                            tx.ledger()
                                .append_event(
                                    &fence,
                                    EventKind::Need,
                                    &serde_json::json!({"before_commit": true}),
                                    Some(outputs),
                                )
                                .map(|_| ())
                        })
                        .await?;
                }
                _ => {}
            }
            self.events.lock().unwrap().push(event);
            Ok(())
        })
    }

    fn evidence<'a>(&'a self, blob: Evidence) -> BoxFuture<'a, Result<BlobRef, Failure>> {
        Box::pin(async move {
            self.called().await;
            if self.faults.evidence_down.load(Ordering::SeqCst) {
                return Err(failure(
                    FailureClass::Busy,
                    "storage_full",
                    "The evidence store is full.",
                ));
            }
            if self.sent.load(Ordering::SeqCst)
                && self
                    .faults
                    .evidence_full
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        left.checked_sub(1)
                    })
                    .is_ok()
            {
                return Err(failure(
                    FailureClass::Busy,
                    "storage_full",
                    "The evidence store is full.",
                ));
            }
            let saved = self.hooks.count("put_item") + self.hooks.count("add_item") > 0;
            if saved && self.faults.outputs_refused.swap(false, Ordering::SeqCst) {
                return Err(failure(
                    FailureClass::Busy,
                    "storage_full",
                    "The evidence store is full.",
                ));
            }
            self.evidence_given.lock().unwrap().push(blob.bytes.clone());
            self.store
                .blobs()
                .put(NewBlob {
                    workspace: workspace(),
                    owner: OwnerRef::service("engine-test")?,
                    kind: blob.kind,
                    class: blob.class,
                    plain: blob.bytes,
                })
                .await
        })
    }

    fn keep_capture<'a>(&'a self, capture: KeptCapture) -> BoxFuture<'a, Result<BlobRef, Failure>> {
        Box::pin(async move {
            self.called().await;
            if !self.keeps_captures.load(Ordering::SeqCst) {
                return Err(failure(
                    FailureClass::Busy,
                    "capture_not_kept",
                    "No capture is kept.",
                ));
            }
            self.hooks.note(format!("capture:{}", capture.name));
            let blob = self
                .store
                .blobs()
                .put(NewBlob {
                    workspace: workspace(),
                    owner: OwnerRef::service("engine-test")?,
                    kind: a2m_ledger::BlobKind::Snapshot,
                    class: RetentionClass::Short,
                    plain: capture.bytes.clone(),
                })
                .await?;
            self.captures.lock().unwrap().push((capture, blob));
            Ok(blob)
        })
    }

    fn acknowledged<'a>(
        &'a self,
        token: &'a ActedToken,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            self.called().await;
            if !self.keeps_acknowledgements.load(Ordering::SeqCst) {
                return Err(failure(
                    FailureClass::Busy,
                    "acknowledgement_not_kept",
                    "No acknowledgement is kept.",
                ));
            }
            // As the action registry's control plane does: the ledger ends
            // the record, from the run's own stored diff.
            let Some(evidence) = evidence else {
                return Err(failure(
                    FailureClass::Busy,
                    "acknowledgement_not_kept",
                    "An acknowledgement is kept only with its evidence.",
                ));
            };
            self.store.store_acknowledged(token, evidence).await?;
            self.hooks.note("acknowledged");
            self.acknowledged.lock().unwrap().push(token.record());
            Ok(())
        })
    }

    fn project_folder(&self) -> Option<PathBuf> {
        self.project.lock().unwrap().clone()
    }

    fn discovery_files(&self) -> std::collections::BTreeMap<String, String> {
        self.discovery_files.lock().unwrap().clone()
    }

    fn other_records(
        &self,
        operation: &a2m_core::OperationName,
        asking: RecordRef,
    ) -> Option<Arc<dyn a2m_provider::OtherRecords>> {
        self.others_asked
            .lock()
            .unwrap()
            .push((operation.to_string(), asking));
        self.others.lock().unwrap().clone()
    }

    fn keeps_shadow(&self) -> bool {
        self.keeps_shadow.load(Ordering::SeqCst)
    }

    fn shadow_rate<'a>(
        &'a self,
        _surface: &'a SurfaceName,
        want: u32,
    ) -> BoxFuture<'a, Result<bool, Failure>> {
        Box::pin(async move {
            self.called().await;
            if self.shadow_rate_refused.load(Ordering::SeqCst) {
                return Ok(false);
            }
            self.shadow_requests.fetch_add(want, Ordering::SeqCst);
            Ok(true)
        })
    }

    fn shadow<'a>(
        &'a self,
        compared: a2m_engine::ShadowCompared,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            self.called().await;
            self.hooks.note("shadow");
            self.shadows.lock().unwrap().push(compared.clone());
            // As the action registry's control plane does: the ledger keeps
            // it under the run and the lease, by the shadow binding's digest.
            let comparison = a2m_ledger::ShadowComparison {
                fence: self.fence(),
                operation: compared.operation,
                binding: compared.shadow.digest,
                mode: compared.shadow.mode,
                position: u32::try_from(compared.shadow.position).unwrap(),
                against: compared.verified.digest,
                writes: compared.writes,
                outcome: compared.outcome,
                input: compared.input,
                requests: compared.requests,
            };
            if self.shadow_write_fails.load(Ordering::SeqCst) {
                return Err(Failure::new(
                    a2m_core::FailureClass::Busy,
                    a2m_core::FailureCode::fixed("store_unavailable"),
                    "The test's store refuses the comparison.",
                    "Nothing to do.",
                ));
            }
            if self.shadow_write_hangs.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            self.store
                .write(Durability::Synced, move |tx| {
                    tx.shadow().record(&comparison).map(|_| ())
                })
                .await
        })
    }
}

// ---- the engine's caller ----

/// One test's whole world: the test application, the registry with the
/// test pack, the engine, and one run in a real store.
pub struct Scene {
    pub app: TestApp,
    pub hooks: Arc<Hooks>,
    pub discoverer: Arc<WritingDiscoverer>,
    pub engine: Engine,
    pub control: Arc<StoreControl>,
    pub plan_hash: Digest,
    pub attempt: AtomicU32,
    pub conditions: Mutex<Conditions>,
    pub pin: Mutex<Option<Mode>>,
    /// The one binding the next jobs run, when a test names one.
    pub binding: Mutex<Option<a2m_engine::BindingPin>>,
    pub lease: LeaseClock,
    /// The engine's clock: a test moves it.
    pub clock: Arc<ManualClock>,
    /// The bytes of the source input the next jobs carry.
    pub source: Mutex<Option<Arc<Vec<u8>>>>,
    /// The environment the next jobs name; the target's default when none.
    pub environment: Mutex<Option<Environment>>,
    /// The vault whose redactor the engine and the registry were given.
    pub vault: Arc<Vault>,
    /// The engine's telemetry, as the registry was given it.
    pub telemetry: a2m_telemetry::Telemetry,
    logs: PathBuf,
    runs: AtomicU64,
    _folder: Folder,
}

async fn open_store(folder: &Folder) -> (Store, Arc<Vault>) {
    let config = StoreConfig::in_dir(folder.path().join("store"))
        .with_clock(Arc::new(ManualClock::starting_at(
            Timestamp::from_unix_millis(START),
        )))
        .with_random(Counted::new());
    let store = Store::open(config).await.unwrap();
    let vault = Arc::new(Vault::with_random(
        Arc::new(MemoryKeyStore::with_random(Counted::new())),
        Arc::new(store.key_records()),
        Counted::new(),
    ));
    store.attach(Arc::clone(&vault)).await.unwrap();
    (store, vault)
}

impl Scene {
    /// A scene with a run of `tool` opened, in `queued`.
    pub async fn new(tool: &str, change: impl FnOnce(&mut Plan)) -> Self {
        Self::with_settings(tool, change, |_| {}).await
    }

    /// A scene whose engine has other settings than the defaults.
    pub async fn with_settings(
        tool: &str,
        change: impl FnOnce(&mut Plan),
        settings: impl FnOnce(&mut EngineSettings),
    ) -> Self {
        Self::with_packs(tool, change, settings, |_, _| {}).await
    }

    /// A scene whose registry holds more packs than the test pack: `more`
    /// adds them to the builder, and is given the redactor of the vault.
    pub async fn with_packs(
        tool: &str,
        change: impl FnOnce(&mut Plan),
        settings: impl FnOnce(&mut EngineSettings),
        more: impl FnOnce(&mut RegistryBuilder, &Arc<dyn a2m_core::Redact>),
    ) -> Self {
        let app = TestApp::start().await.unwrap();
        let folder = Folder::new("scene");
        let hooks = Arc::new(Hooks::default());
        let discoverer = Arc::new(WritingDiscoverer::default());
        let logs = folder.path().join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let (store, vault) = open_store(&folder).await;
        let redact: Arc<dyn a2m_core::Redact> = vault.redactor();
        let telemetry =
            a2m_telemetry::start(&TelemetrySettings::default(), Arc::clone(&redact), &logs)
                .unwrap();
        let host = HostServices {
            supervisor: Supervisor::new(SupervisorLimits::default()),
            roots: Roots::none(),
            redact: Arc::clone(&redact),
            telemetry: telemetry.clone(),
        };
        let pack = AppPack {
            hooks: Arc::clone(&hooks),
            discoverer: Arc::clone(&discoverer),
        };
        let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
        builder.add_pack(&pack);
        more(&mut builder, &redact);
        let (registry, problems) = builder.build();
        assert!(problems.is_empty(), "{problems:?}");
        let registry: Arc<Registry> = Arc::new(registry);

        let mut plan = plan(&app.url(""));
        change(&mut plan);
        let encoded = plan.encode().unwrap();
        let plan = Arc::new(plan);
        let tool_name = OperationName::new(tool).unwrap();
        let (effect, kind) = match plan.tool(&tool_name).map(|tool| tool.runs) {
            Some(ToolRuns::Operation(index)) => {
                (plan.operation(index).unwrap().effect, ToolKind::Operation)
            }
            Some(ToolRuns::Pipeline(index)) => {
                (plan.pipeline(index).unwrap().effect, ToolKind::Pipeline)
            }
            None => (EffectClass::Read, ToolKind::Operation),
        };
        let hash = encoded.hash;
        let fence = store
            .write(Durability::Synced, move |tx| {
                let mut ledger = tx.ledger();
                let Opened::New(opened) = ledger.open_run(NewRun {
                    workspace: workspace(),
                    principal: PrincipalId::from_bytes([2; 16]),
                    client_label: "test".to_owned(),
                    tool_name,
                    tool_kind: kind,
                    target: TargetName::new("example").unwrap(),
                    target_version: hash,
                    environment: Environment::new("test").unwrap(),
                    identity: None,
                    effect,
                    input_hash: Digest::of(b"input"),
                    input: None,
                    retention: RetentionClass::Long,
                })?
                else {
                    panic!("the run exists already");
                };
                ledger.new_epoch(opened.run)
            })
            .await
            .unwrap();
        let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
        let mut engine_settings = EngineSettings::default();
        settings(&mut engine_settings);
        let engine = Engine::new(EngineDeps {
            registry,
            redact,
            keyed: vault.keyed_digest().await.unwrap(),
            clock: clock.clone(),
            settings: engine_settings,
        });
        let policy_key = Arc::new(store.policy_key().unwrap());
        let control = Arc::new(StoreControl {
            store,
            policy_key,
            plan,
            fence: Mutex::new(fence),
            hooks: Arc::clone(&hooks),
            faults: Faults::default(),
            approval: Mutex::new(None),
            results: Mutex::new(Vec::new()),
            traces: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            questions: Mutex::new(Vec::new()),
            wants: Mutex::new(Vec::new()),
            expiry_reports: AtomicU32::new(0),
            modes: Mutex::new(Vec::new()),
            sent: AtomicBool::new(false),
            before_commit: Mutex::new(Vec::new()),
            evidence_given: Mutex::new(Vec::new()),
            keeps_captures: AtomicBool::new(false),
            captures: Mutex::new(Vec::new()),
            keeps_acknowledgements: AtomicBool::new(false),
            acknowledged: Mutex::new(Vec::new()),
            project: Mutex::new(None),
            discovery_files: Mutex::new(std::collections::BTreeMap::new()),
            others: Mutex::new(None),
            others_asked: Mutex::new(Vec::new()),
            keeps_shadow: AtomicBool::new(false),
            shadow_rate_refused: AtomicBool::new(false),
            shadow_requests: AtomicU32::new(0),
            shadow_write_fails: AtomicBool::new(false),
            shadow_write_hangs: AtomicBool::new(false),
            shadows: Mutex::new(Vec::new()),
        });
        Self {
            app,
            hooks,
            discoverer,
            engine,
            control,
            plan_hash: hash,
            attempt: AtomicU32::new(1),
            conditions: Mutex::new(Conditions::default()),
            pin: Mutex::new(None),
            binding: Mutex::new(None),
            lease: LeaseClock::unbounded(),
            clock,
            source: Mutex::new(None),
            environment: Mutex::new(None),
            vault,
            telemetry,
            logs,
            runs: AtomicU64::new(0),
            _folder: folder,
        }
    }

    /// Opens another run of `tool` in the same store, against the same
    /// test application: a second call by the same caller.
    pub async fn another_run(&self, tool: &str) {
        let plan = Arc::clone(&self.control.plan);
        let tool_name = OperationName::new(tool).unwrap();
        let (effect, kind) = match plan.tool(&tool_name).map(|tool| tool.runs) {
            Some(ToolRuns::Operation(index)) => {
                (plan.operation(index).unwrap().effect, ToolKind::Operation)
            }
            Some(ToolRuns::Pipeline(index)) => {
                (plan.pipeline(index).unwrap().effect, ToolKind::Pipeline)
            }
            None => (EffectClass::Read, ToolKind::Operation),
        };
        let hash = self.plan_hash;
        let number = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        let fence = self
            .control
            .store
            .write(Durability::Synced, move |tx| {
                let mut ledger = tx.ledger();
                let Opened::New(opened) = ledger.open_run(NewRun {
                    workspace: workspace(),
                    principal: PrincipalId::from_bytes([2; 16]),
                    client_label: "test".to_owned(),
                    tool_name,
                    tool_kind: kind,
                    target: TargetName::new("example").unwrap(),
                    target_version: hash,
                    environment: Environment::new("test").unwrap(),
                    identity: None,
                    effect,
                    input_hash: Digest::of(&number.to_le_bytes()),
                    input: None,
                    retention: RetentionClass::Long,
                })?
                else {
                    panic!("the run exists already");
                };
                ledger.new_epoch(opened.run)
            })
            .await
            .unwrap();
        *self.control.fence.lock().unwrap() = fence;
        *self.control.approval.lock().unwrap() = None;
        self.control.sent.store(false, Ordering::SeqCst);
        self.control.modes.lock().unwrap().clear();
        self.control.before_commit.lock().unwrap().clear();
        self.attempt.store(1, Ordering::SeqCst);
    }

    pub fn run(&self) -> RunId {
        self.control.fence().run
    }

    pub async fn run_row(&self) -> RunRow {
        let run = self.run();
        self.control
            .store
            .read(move |tx| tx.ledger().run(run))
            .await
            .unwrap()
    }

    pub async fn records(&self) -> Vec<RecordRow> {
        let run = self.run();
        self.control
            .store
            .read(move |tx| tx.ledger().records(run))
            .await
            .unwrap()
    }

    /// The record locks the run holds, as the ledger has them.
    pub async fn locks(&self) -> Vec<LockRow> {
        let run = self.run();
        self.control
            .store
            .read(move |tx| tx.ledger().locks(run))
            .await
            .unwrap()
    }

    pub async fn record_states(&self) -> Vec<RecordState> {
        self.records().await.iter().map(|row| row.state).collect()
    }

    pub async fn values(&self, blob: Option<BlobRef>) -> Option<Values> {
        let blob = blob?;
        let opened = self.control.store.blobs().get(&blob).await.unwrap();
        Some(decode_values(opened.expose()).unwrap())
    }

    /// When the ledger last recorded what followed the record's current
    /// send: its `acted` mark, or the failure that ended the attempt.
    async fn sent_by(&self, record: &RecordRow) -> Option<a2m_core::Timestamp> {
        let after = record.intent_seq?;
        let run = self.run();
        let events = self
            .control
            .store
            .read(move |tx| tx.ledger().events(run, after, u32::MAX))
            .await
            .unwrap();
        let place = |event: &a2m_ledger::RunEvent| {
            let payload: serde_json::Value = serde_json::from_str(&event.payload).ok()?;
            Some((payload["step"].as_u64()?, payload["record"].as_u64()?))
        };
        let marked = events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::Acted | EventKind::Failed))
            .filter(|event| {
                place(event)
                    == Some((
                        u64::from(record.record.step_seq),
                        u64::from(record.record.record_seq),
                    ))
            })
            .map(|event| event.at)
            .max_by_key(|at| at.as_unix_millis());
        marked.or(record.acted_at)
    }

    /// What the runner fills when it claims a job for a run that has been
    /// worked on before: the ledger's rows, with their stored values.
    async fn snapshot(&self) -> Option<RunSnapshot> {
        let row = self.run_row().await;
        if row.state == RunState::Queued {
            return None;
        }
        let modes = self.control.modes.lock().unwrap().clone();
        let recorded = self.control.before_commit.lock().unwrap().clone();
        let mut records = Vec::new();
        for record in self.records().await {
            let before_commit = recorded
                .iter()
                .rev()
                .find(|(held, _)| *held == record.record)
                .map(|(_, blob)| *blob);
            records.push(RecordSnapshot {
                record: record.record,
                key: record.key.clone(),
                operation: record.operation.clone(),
                state: record.state,
                intended: record.intended,
                intended_values: Values::new(),
                before: self.values(record.before).await,
                idempotency_token: record.idempotency_token.clone(),
                assigned_key: record.assigned_key.clone(),
                outputs: self.values(record.outputs).await,
                before_commit: self.values(before_commit).await,
                mode: modes
                    .iter()
                    .rev()
                    .find(|(held, _)| *held == record.record)
                    .map(|(_, mode)| mode.clone()),
                failure: record.failure.clone(),
                acknowledged: record.acknowledged(),
                intent_at: record.intent_at,
                sent_by: self.sent_by(&record).await,
            });
        }
        let approved = match row.diff {
            Some(blob) => {
                let opened = self.control.store.blobs().get(&blob).await.unwrap();
                Some(a2m_engine::Diff::decode(opened.expose()).unwrap())
            }
            None => None,
        };
        Some(RunSnapshot {
            state: row.state,
            diff: row.diff_hash,
            cleared: row.cleared_to_write(),
            records,
            approved,
        })
    }

    pub async fn job(&self, kind: JobKind, tool: &str, input: Values) -> JobSpec {
        let environment = self.environment.lock().unwrap().clone();
        let source = self.source.lock().unwrap().clone();
        JobSpec {
            run: self.run(),
            workspace: workspace(),
            attempt: self.attempt.load(Ordering::SeqCst),
            epoch: self.control.fence().epoch,
            kind,
            plan: self.plan_hash,
            tool: ToolRef::Tool(OperationName::new(tool).unwrap()),
            environment,
            identity: None,
            input,
            source,
            snapshot: self.snapshot().await,
            conditions: self.conditions.lock().unwrap().clone(),
            pin: self.pin.lock().unwrap().clone(),
            binding: self.binding.lock().unwrap().clone(),
            parallelism: Parallelism::default(),
            lease: self.lease.clone(),
            capture: CaptureLevel::Standard,
        }
    }

    /// Runs one job and applies its outcome to the run, as the engine's
    /// caller does.
    pub async fn work(&self, kind: JobKind, tool: &str, input: Values) -> JobOutcome {
        let job = self.job(kind, tool, input).await;
        let outcome = self.engine.run_job(&job, self.control.as_ref()).await;
        self.apply(&outcome).await;
        outcome
    }

    pub async fn apply(&self, outcome: &JobOutcome) {
        let Some(to) = outcome.run_state() else {
            return;
        };
        if self.run_row().await.state == to {
            return;
        }
        let fence = self.control.fence();
        let why = outcome.failure().cloned();
        self.control
            .store
            .write(Durability::Synced, move |tx| {
                tx.ledger().set_state(&fence, to, why.as_ref())
            })
            .await
            .unwrap();
    }

    /// The runner is gone and another claims the run: a new lease.
    pub async fn new_lease(&self) -> Epoch {
        let run = self.run();
        let fence = self
            .control
            .store
            .write(Durability::Synced, move |tx| tx.ledger().new_epoch(run))
            .await
            .unwrap();
        *self.control.fence.lock().unwrap() = fence;
        self.attempt.fetch_add(1, Ordering::SeqCst);
        self.control.sent.store(false, Ordering::SeqCst);
        fence.epoch
    }

    /// Runs a discovery job on a surface of the plan.
    pub async fn discover(&self, surface: &str) -> JobOutcome {
        self.discover_with(surface, Values::new()).await
    }

    /// Runs a discovery job with a focus: what the job's input carries.
    pub async fn discover_with(&self, surface: &str, focus: Values) -> JobOutcome {
        let mut job = self.job(JobKind::Discover, ORDER_GET, focus).await;
        job.tool = ToolRef::Surface(SurfaceName::new(surface).unwrap());
        self.engine.run_job(&job, self.control.as_ref()).await
    }

    /// Every step of the kinds given that reached a driver, in order.
    pub fn steps(&self, kinds: &[&str]) -> Vec<String> {
        self.hooks
            .seen()
            .into_iter()
            .filter(|entry| kinds.contains(&entry.as_str()))
            .collect()
    }

    /// Every line the engine logged to file, and every line it holds in
    /// memory, as one text.
    pub fn logged(&self) -> String {
        let mut text = self.telemetry.log_lines().join("\n");
        for entry in std::fs::read_dir(&self.logs).unwrap().flatten() {
            text.push('\n');
            text.push_str(&String::from_utf8_lossy(
                &std::fs::read(entry.path()).unwrap(),
            ));
        }
        text
    }

    pub fn last_result(&self) -> Values {
        self.control
            .results
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap()
    }
}

/// The inputs of one line item.
pub fn item(order: &str, code: &str, description: &str, quantity: i64, length: &str) -> Values {
    Values::new()
        .with("order_id", order)
        .with("item_code", code)
        .with("description", description)
        .with("quantity", quantity)
        .with("length_in", length)
}

/// The key of a line item, as the engine makes it.
pub fn item_key(order: &str, code: &str) -> RecordKey {
    RecordKey::new(vec![Value::from(order), Value::from(code)]).unwrap()
}

// ---- what the tests of writes share ----

pub fn completed(outcome: &JobOutcome) -> bool {
    matches!(
        outcome,
        JobOutcome::Finished {
            state: RunState::Completed,
            failure: None
        }
    )
}

pub fn waits(outcome: &JobOutcome) -> bool {
    matches!(
        outcome,
        JobOutcome::Rests {
            state: RunState::NeedsApproval,
            failure: None
        }
    )
}

/// The position of the first entry of the log that is `what`.
pub fn at(log: &[String], what: &str) -> usize {
    log.iter()
        .position(|entry| entry == what)
        .unwrap_or_else(|| panic!("the log has no `{what}`: {log:?}"))
}

/// Previews a write, has a person approve its diff, and leaves the run
/// waiting for the execute.
pub async fn approved(scene: &Scene, tool: &str, input: &Values) {
    let outcome = scene.work(JobKind::Preview, tool, input.clone()).await;
    assert!(waits(&outcome), "{outcome:?}");
    let diff = scene
        .run_row()
        .await
        .diff_hash
        .expect("a preview stores a diff");
    scene.control.approve(diff).await;
}

/// The inputs of a create whose code the system assigns.
pub fn slot(order: &str, quantity: i64, length: &str) -> Values {
    Values::new()
        .with("order_id", order)
        .with("quantity", quantity)
        .with("length_in", length)
}

/// The class and code of the failure an outcome carries.
pub fn failed_as(outcome: &JobOutcome) -> (FailureClass, String) {
    let failure = outcome
        .failure()
        .unwrap_or_else(|| panic!("the outcome carries no failure: {outcome:?}"));
    (failure.class(), failure.code().to_string())
}
