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
    ApprovalId, AuthPlan, AuthProviderName, BindingPlan, BindingStatus, CanonicalJson, Digest,
    EffectClass, Environment, EnvironmentIndex, EnvironmentKind, EnvironmentPlan, Epoch, ExprIndex,
    FailsAs, Failure, FailureClass, FailureCode, FieldFormat, FieldTypeIndex, FieldTypePlan,
    Finding, Hazard, IdentityPlan, IdentityStrategy, InputIndex, InputPlan, Mode, OperationIndex,
    OperationName, OperationPlan, OutputPlan, PLAN_FORMAT, PackPayload, PermissionManifest,
    PipelineIndex, PipelineLimits, PipelinePlan, PipelineStep, Plan, PlanExpr, PlanHeader,
    PrincipalId, ProbePlan, RecordKey, RecordState, Rounding, RunId, RunState, Sensitivity,
    SignInMode, Step, StepArg, StepKind, SurfaceIndex, SurfaceKind, SurfaceName, SurfacePlan,
    TargetName, TargetPlan, Timestamp, ToolAnnotations, ToolPlan, ToolRuns, Value, ValueType,
    Values, Verb, Version, WhitespaceRule, WorkspaceId,
};
use a2m_engine::{
    BoxFuture, Clearance, Conditions, Control, Engine, EngineDeps, EngineEvent, EngineSettings,
    Evidence, GuardQuestion, JobKind, JobOutcome, JobSpec, LeaseClock, Parallelism, Preview,
    RecordSnapshot, RunSnapshot, SessionWant, ToolRef, decode_values,
};
use a2m_expr::{DecimalMark, FieldRule, LibraryVersion, Whitespace};
use a2m_ledger::{
    Acted, ActedToken, BlobRef, Durability, EventKind, Fence, IntentToken, LockHeld, NewBlob,
    NewRun, Opened, Owner, ReadBack, Reconciled, RecordIntent, RecordRef, RecordRow,
    RetentionClass, RunRow, Settled, Store, StoreConfig, ToolKind, WriteClearance,
};
use a2m_provider::{
    Cleanup, DescribeForm, Describer, Description, Discoverer, DiscovererName, DiscoveryReport,
    DiscoveryScope, Driver, DriverHealth, FindingSink, HostServices, Observed, Pack, PackContext,
    PrepareContext, Prepared, ProviderSettings, Registrar, Registry, RegistryBuilder, Roots,
    SandboxPort, StepFailure, StepInput, StepOutcome, Supervisor, SupervisorLimits,
    SurfaceDescription, SurfaceRef,
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
steps = ["get_order", "get_item", "put_item", "get_stock"]
rank = 40
session_reads = 4

[[modes]]
name = "app_json_spare"
archetype = "request"
verbs = ["read", "upsert"]
steps = ["get_order", "get_item", "put_item", "get_stock"]
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

[effects]
network = "surfaces"
sessions = ["json_app"]
"#;

/// What a test can make the driver do.
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
}

struct AppState {
    base: String,
    http: reqwest::Client,
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
    values
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
            Ok(Prepared::new(AppState {
                base: cx.surface.surface.address.clone(),
                http: reqwest::Client::new(),
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
            let hooks = &self.hooks;
            hooks.note(step.kind.as_str());
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
            match step.kind.as_str() {
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
                    Ok(StepOutcome::Done(Observed {
                        values: Values::new()
                            .with("order_id", body["order_id"].as_str().unwrap_or_default())
                            .with("customer", body["customer"].as_str().unwrap_or_default())
                            .with("status", body["status"].as_str().unwrap_or_default())
                            .with("lines", i64::try_from(lines).unwrap_or(0)),
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
                "put_item" => {
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
                    let sent = state
                        .http
                        .put(format!("{}/api/orders/{order}/items/{item}", state.base))
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
                _ => Err(StepFailure::not_sent(failure(
                    FailureClass::UnknownVariant,
                    "example_unexpected",
                    "The driver has no such step.",
                ))),
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
pub struct WritingDiscoverer {
    pub results: Mutex<Vec<Result<StepOutcome, StepFailure>>>,
}

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
        _out: &'a mut dyn FindingSink,
    ) -> BoxFuture<'a, Result<DiscoveryReport, Failure>> {
        Box::pin(async move {
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
            // A planted step that commits, and the same step dressed as a
            // read: neither may reach the system.
            let write = step("put_item", &[], true, false);
            let first = port.act(&write, &input).await;
            let read = step("get_item", &[], false, true);
            let second = port.act(&read, &input).await;
            let mut results = self.results.lock().unwrap();
            results.push(first);
            results.push(second);
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

fn expr(plan: &mut Plan, source: &str, result: ValueType) -> ExprIndex {
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
        prefer_response: false,
        precondition: None,
    };
    let order_outputs = || {
        vec![
            OutputPlan {
                name: "order_id".to_owned(),
                ty: ValueType::Text,
                field_type: Some(FieldTypeIndex::new(identifier)),
            },
            OutputPlan {
                name: "customer".to_owned(),
                ty: ValueType::Text,
                field_type: Some(FieldTypeIndex::new(free_text)),
            },
            OutputPlan {
                name: "status".to_owned(),
                ty: ValueType::Text,
                field_type: Some(FieldTypeIndex::new(identifier)),
            },
            OutputPlan {
                name: "lines".to_owned(),
                ty: ValueType::Int,
                field_type: None,
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
    plan
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
}

pub struct StoreControl {
    pub store: Store,
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
}

impl StoreControl {
    fn fence(&self) -> Fence {
        *self.fence.lock().unwrap()
    }

    /// A person approves a diff. The policy engine's stand-in: one row in
    /// its own table.
    pub async fn approve(&self, diff: Digest) {
        let run = self.fence().run;
        let id = self
            .store
            .write(Durability::Synced, move |tx| {
                let id = ApprovalId::from_parts(tx.now(), tx.random()?)?;
                let now = tx.now();
                tx.tables(Owner::Policy).execute(
                    "INSERT INTO approvals
                        (id, workspace_id, scope, run_id, diff_hash, approver_id, channel, presence_check, granted_at)
                     VALUES (?1, ?2, 'run', ?3, ?4, ?5, 'local', 'confirmed', ?6)",
                    &[
                        id.to_string().into(),
                        workspace().to_string().into(),
                        run.to_string().into(),
                        diff.to_string().into(),
                        PrincipalId::from_bytes([9; 16]).to_string().into(),
                        now.into(),
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
        Box::pin(async move { Ok(Arc::clone(&self.plan)) })
    }

    fn decide<'a>(&'a self, question: &'a GuardQuestion) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
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
            self.expiry_reports.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn rate<'a>(
        &'a self,
        _surface: &'a SurfaceName,
        want: u32,
    ) -> BoxFuture<'a, Result<u32, Failure>> {
        Box::pin(async move { Ok(want) })
    }

    fn store_preview<'a>(&'a self, preview: &'a Preview) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
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
            let fence = self.fence();
            let Some(approval) = *self.approval.lock().unwrap() else {
                return Ok(Clearance::Waits);
            };
            // Marking the approval used and moving the run happen in one
            // transaction; the ledger checks the approval is for this diff.
            let cleared = self
                .store
                .write(Durability::Synced, move |tx| {
                    let now = tx.now();
                    tx.tables(Owner::Policy).execute(
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
            let tokens = self.store.store_intents(&self.fence(), intents).await?;
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
            self.hooks.note("acted");
            self.sent.store(true, Ordering::SeqCst);
            self.store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().mark_acted(token, acted)
                })
                .await
        })
    }

    fn verified<'a>(
        &'a self,
        token: ActedToken,
        read_back: ReadBack,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            self.hooks.note("verified");
            self.store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().mark_verified(&token, read_back)
                })
                .await
        })
    }

    fn failed<'a>(
        &'a self,
        record: RecordRef,
        why: &'a Failure,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
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
                EngineEvent::BeforeCommit { outputs, .. } => {
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
    pub lease: LeaseClock,
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
        let app = TestApp::start().await.unwrap();
        let folder = Folder::new("scene");
        let hooks = Arc::new(Hooks::default());
        let discoverer = Arc::new(WritingDiscoverer {
            results: Mutex::new(Vec::new()),
        });
        let logs = folder.path().join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let (store, vault) = open_store(&folder).await;
        let redact: Arc<dyn a2m_core::Redact> = vault.redactor();
        let host = HostServices {
            supervisor: Supervisor::new(SupervisorLimits::default()),
            roots: Roots::none(),
            redact: Arc::clone(&redact),
            telemetry: a2m_telemetry::start(
                &TelemetrySettings::default(),
                Arc::clone(&redact),
                &logs,
            )
            .unwrap(),
        };
        let pack = AppPack {
            hooks: Arc::clone(&hooks),
            discoverer: Arc::clone(&discoverer),
        };
        let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
        builder.add_pack(&pack);
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
        let engine = Engine::new(EngineDeps {
            registry,
            redact,
            keyed: vault.keyed_digest().await.unwrap(),
            clock: Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START))),
            settings: EngineSettings::default(),
        });
        let control = Arc::new(StoreControl {
            store,
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
            lease: LeaseClock::unbounded(),
            _folder: folder,
        }
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

    pub async fn record_states(&self) -> Vec<RecordState> {
        self.records().await.iter().map(|row| row.state).collect()
    }

    async fn values(&self, blob: Option<BlobRef>) -> Option<Values> {
        let blob = blob?;
        let opened = self.control.store.blobs().get(&blob).await.unwrap();
        Some(decode_values(opened.expose()).unwrap())
    }

    /// What the runner fills when it claims a job for a run that has been
    /// worked on before: the ledger's rows, with their stored values.
    async fn snapshot(&self) -> Option<RunSnapshot> {
        let row = self.run_row().await;
        if row.state == RunState::Queued {
            return None;
        }
        let modes = self.control.modes.lock().unwrap().clone();
        let mut records = Vec::new();
        for record in self.records().await {
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
                before_commit: None,
                mode: modes
                    .iter()
                    .rev()
                    .find(|(held, _)| *held == record.record)
                    .map(|(_, mode)| mode.clone()),
                failure: record.failure.clone(),
            });
        }
        Some(RunSnapshot {
            state: row.state,
            diff: row.diff_hash,
            cleared: row.cleared_to_write(),
            records,
        })
    }

    pub async fn job(&self, kind: JobKind, tool: &str, input: Values) -> JobSpec {
        JobSpec {
            run: self.run(),
            workspace: workspace(),
            attempt: self.attempt.load(Ordering::SeqCst),
            epoch: self.control.fence().epoch,
            kind,
            plan: self.plan_hash,
            tool: ToolRef::Tool(OperationName::new(tool).unwrap()),
            environment: None,
            identity: None,
            input,
            source: None,
            snapshot: self.snapshot().await,
            conditions: self.conditions.lock().unwrap().clone(),
            pin: self.pin.lock().unwrap().clone(),
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
