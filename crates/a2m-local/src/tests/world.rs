//! What the tests share: a kernel on a temporary data directory with
//! memory key stores, the test pack over the built-in test application,
//! and this crate's own person surface in the place of a test's.

#[path = "../../../a2m-engine/tests/common/mod.rs"]
pub(crate) mod engine;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_actions::{
    BoxFuture, Caller, CallerCredential, FrontDoor, HealthState, Kernel, KernelParts, Paths,
    PersonSurface, Request, Response, Settings, StorePlace,
};
use a2m_core::{Timestamp, Version};
use a2m_provider::{RegistryBuilder, Roots, Supervisor, SupervisorLimits};
use a2m_telemetry::{Clock, ManualClock};
use a2m_vault::MemoryKeyStore;
use engine::{AppPack, Counted, Folder, START};
use example_app::harness::TestApp;
use serde_json::Value as Json;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::{
    Availability, LocalSurface, OsUser, PresencePrompt, PromptAnswer, PromptName, PromptText,
    SurfaceParts,
};

pub(crate) const VERSION: Version = Version::new(0, 0, 1);

/// The longest a step of a test may take before the test fails by name
/// instead of waiting for ever.
const PATIENCE: Duration = Duration::from_secs(60);

/// `step`, or a failure that names it when it does not end.
pub(crate) async fn within<T>(what: &str, step: impl Future<Output = T>) -> T {
    match tokio::time::timeout(PATIENCE, step).await {
        Ok(done) => done,
        Err(_) => panic!("{what} did not end"),
    }
}

/// A temporary home for one engine's paths. The folder's name is short,
/// because a socket's path may not be long.
pub(crate) fn home(name: &str) -> (Folder, Paths) {
    let folder = Folder::new(name);
    let paths = Paths::under(folder.path());
    (folder, paths)
}

/// The answer of a person who passed the prompt. The test's prompt
/// stands in for an operating system's, and says it is polkit.
pub(crate) const PASSED: PromptAnswer = PromptAnswer::Present(PromptName::Polkit);

/// A presence prompt that answers as the test set it, and counts. While
/// it is held, an ask stays up as a prompt does that a person has not
/// answered yet; like a real one it ends at its deadline as dismissed.
pub(crate) struct TestPrompt {
    answer: Mutex<PromptAnswer>,
    asked: AtomicU32,
    held: watch::Sender<bool>,
    deadlines: Mutex<Vec<Instant>>,
}

impl TestPrompt {
    pub(crate) fn answering(answer: PromptAnswer) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(answer),
            asked: AtomicU32::new(0),
            held: watch::Sender::new(false),
            deadlines: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn set(&self, answer: PromptAnswer) {
        *self.answer.lock().unwrap() = answer;
    }

    pub(crate) fn asked(&self) -> u32 {
        self.asked.load(Ordering::SeqCst)
    }

    /// Keeps every ask up until [`release`](Self::release).
    pub(crate) fn hold(&self) {
        self.held.send_replace(true);
    }

    pub(crate) fn release(&self) {
        self.held.send_replace(false);
    }

    /// Waits until `count` asks have been raised.
    pub(crate) async fn raised(&self, count: u32) {
        within("the prompt's raising", async {
            while self.asked() < count {
                tokio::task::yield_now().await;
            }
        })
        .await;
    }

    /// The deadline each ask was given, in order.
    pub(crate) fn deadlines(&self) -> Vec<Instant> {
        self.deadlines.lock().unwrap().clone()
    }
}

impl PresencePrompt for TestPrompt {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async move {
            match self.answer.lock().unwrap().clone() {
                PromptAnswer::NotAvailable(why) => why,
                _ => Availability::ready(PromptName::Polkit, "The prompt of a test."),
            }
        })
    }

    fn ask(&self, _text: PromptText, deadline: Instant) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            self.deadlines.lock().unwrap().push(deadline);
            self.asked.fetch_add(1, Ordering::SeqCst);
            let mut held = self.held.subscribe();
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(deadline) => return PromptAnswer::Dismissed,
                _ = held.wait_for(|held| !*held) => {}
            }
            self.answer.lock().unwrap().clone()
        })
    }
}

pub(crate) struct World {
    pub(crate) app: TestApp,
    pub(crate) kernel: Kernel,
    pub(crate) surface: LocalSurface,
    pub(crate) caller: Caller,
    pub(crate) paths: Paths,
    _folder: Folder,
}

impl World {
    /// A healthy kernel with the runner loop started, the `example` target
    /// published, and a [`LocalSurface`] that asks `prompt` and waits
    /// `window` for an answer.
    pub(crate) async fn ready(prompt: Arc<dyn PresencePrompt>, window: Duration) -> Self {
        Self::ready_with(prompt, window, |_| {}).await
    }

    /// The same, with the published plan changed by `change` first.
    pub(crate) async fn ready_with(
        prompt: Arc<dyn PresencePrompt>,
        window: Duration,
        change: fn(&mut a2m_core::Plan),
    ) -> Self {
        Self::build(prompt, window, change, false).await
    }

    /// A world whose target's surface is signed in to through the test
    /// provider [`CODE_PROVIDER`], which asks its person for a one-time
    /// code and nothing else (decision 0205).
    pub(crate) async fn signing_in(prompt: Arc<dyn PresencePrompt>, window: Duration) -> Self {
        Self::build(prompt, window, declare_code_sign_in, true).await
    }

    /// A world whose target's surface is signed in to through the test
    /// provider [`PASSWORD_PROVIDER`], which takes a stored username and
    /// password and asks its person for nothing more.
    pub(crate) async fn signing_in_by_password(
        prompt: Arc<dyn PresencePrompt>,
        window: Duration,
    ) -> Self {
        Self::build(prompt, window, declare_password_sign_in, true).await
    }

    async fn build(
        prompt: Arc<dyn PresencePrompt>,
        window: Duration,
        change: fn(&mut a2m_core::Plan),
        codes: bool,
    ) -> Self {
        let app = TestApp::start().await.unwrap();
        let (folder, paths) = home("k");
        let clock: Arc<dyn Clock> =
            Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
        let user = OsUser::current().unwrap().to_string();
        let prepared = Kernel::prepare(KernelParts {
            settings: Settings::defaults(),
            paths: paths.clone(),
            place: StorePlace::Memory,
            key_store: Arc::new(MemoryKeyStore::with_random(Counted::new())),
            clock: Arc::clone(&clock),
            random: Counted::new(),
            os_user: user.clone(),
        })
        .await;
        let host =
            prepared.host_services(Supervisor::new(SupervisorLimits::default()), Roots::none());
        let mut builder = RegistryBuilder::new(prepared.settings().provider_settings(), host);
        builder.add_pack(&AppPack {
            hooks: Arc::new(engine::Hooks::default()),
            discoverer: Arc::new(engine::WritingDiscoverer::default()),
        });
        if codes {
            builder.add_pack(&CodePack);
        }
        let (registry, problems) = builder.build();
        assert!(problems.is_empty(), "{problems:?}");
        let surface = LocalSurface::new(SurfaceParts {
            prompt,
            clock,
            random: Arc::new(a2m_vault::OsRandom),
            window,
        });
        let kernel = Kernel::open(
            prepared,
            registry,
            problems,
            Arc::new(surface.clone()) as Arc<dyn PersonSurface>,
        )
        .await;
        assert_eq!(
            kernel.health().state,
            HealthState::Ok,
            "{:?}",
            kernel.health()
        );
        let principal = kernel
            .verify_caller(CallerCredential::LocalUser(user))
            .await
            .unwrap();
        kernel.start_runner_loop(4).await.unwrap();
        let mut plan = engine::plan(&app.url(""));
        change(&mut plan);
        kernel.publish_plan(&plan).await.unwrap();
        Self {
            app,
            kernel,
            surface,
            caller: Caller::new(principal, FrontDoor::Mcp, true),
            paths,
            _folder: folder,
        }
    }

    pub(crate) async fn call(&self, action: &str, input: Json) -> Response {
        self.kernel
            .dispatch(Request::new(action, input, self.caller.clone()))
            .await
    }
}

/// The test provider that signs in with a one-time code its person types.
pub(crate) const CODE_PROVIDER: &str = "example_code";
/// The only code it takes.
pub(crate) const RIGHT_CODE: &str = "246810";

const CODE_MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "example_codes"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[points]
auth_provider = ["example_code", "example_password"]

[effects]
sessions = ["json_app"]
"#;

/// A pack with one auth provider and nothing else.
struct CodePack;

impl a2m_provider::Pack for CodePack {
    fn manifest(&self) -> &'static str {
        CODE_MANIFEST
    }

    fn register(
        &self,
        _cx: &a2m_provider::PackContext,
        to: &mut a2m_provider::Registrar<'_>,
    ) -> Result<(), a2m_core::Failure> {
        to.auth_provider(Arc::new(CodeProvider))?;
        to.auth_provider(Arc::new(PasswordProvider))
    }
}

/// Signs in by asking the session's person for a one-time code, through
/// the keeper's own way to the person. A wrong code is refused by name.
struct CodeProvider;

impl a2m_auth::AuthProvider for CodeProvider {
    fn name(&self) -> a2m_auth::ProviderName {
        a2m_auth::ProviderName::fixed(CODE_PROVIDER)
    }

    fn needs(&self) -> a2m_auth::ProviderNeeds {
        a2m_auth::ProviderNeeds {
            person: true,
            silent_renewal: false,
            light_prompt: false,
            movable: false,
            confirms: true,
        }
    }

    fn check(&self, _entry: &a2m_auth::AuthEntry) -> Result<(), a2m_core::Failure> {
        Ok(())
    }

    fn sign_in<'a>(
        &'a self,
        cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<a2m_auth::Material, a2m_core::Failure>> {
        Box::pin(async move {
            let refused = || {
                a2m_core::Failure::new(
                    a2m_core::FailureClass::SessionExpired,
                    a2m_auth::codes::NEEDS_MFA,
                    "The one-time code was not taken.",
                    "Sign in again and type the code you are given.",
                )
            };
            // As a sign-in in a window does first: ask whether the person
            // is there. The person's own press of Carry out answers it.
            if !matches!(
                cx.ask_person(a2m_auth::PersonAsk::Notify {
                    reason: a2m_auth::PersonReason::SignIn,
                })
                .await,
                a2m_auth::PersonAnswer::Done(_)
            ) {
                return Err(refused());
            }
            let answer = cx
                .ask_person(a2m_auth::PersonAsk::EnterCode {
                    reason: a2m_auth::PersonReason::SecondFactor,
                })
                .await;
            match answer {
                a2m_auth::PersonAnswer::Done(returned) => match returned.get("code") {
                    Some(code) if code.expose() == RIGHT_CODE => {
                        Ok(a2m_auth::Material::new(self.name()).with_plain("signed_in", "yes"))
                    }
                    _ => Err(refused()),
                },
                _ => Err(refused()),
            }
        })
    }

    fn is_signed_in<'a>(
        &'a self,
        _material: &'a a2m_auth::Material,
        _now: Timestamp,
    ) -> a2m_auth::BoxFuture<'a, a2m_auth::SignedIn> {
        Box::pin(async { a2m_auth::SignedIn::Yes { until: None } })
    }

    fn detect_expiry(&self, _sign: &a2m_auth::RawSign) -> Option<a2m_auth::ExpirySign> {
        None
    }

    fn refresh<'a>(
        &'a self,
        _material: &'a a2m_auth::Material,
        _cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<a2m_auth::Material, a2m_core::Failure>> {
        Box::pin(async {
            Err(a2m_core::Failure::new(
                a2m_core::FailureClass::SessionExpired,
                a2m_auth::codes::NEEDS_LOGIN,
                "The test provider does not renew.",
                "Sign in again.",
            ))
        })
    }

    fn sign_out<'a>(
        &'a self,
        _material: &'a a2m_auth::Material,
        _cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<(), a2m_core::Failure>> {
        Box::pin(async { Ok(()) })
    }

    fn inject(
        &self,
        _material: &a2m_auth::Material,
        _sink: &mut dyn a2m_auth::CredentialSink,
    ) -> Result<(), a2m_core::Failure> {
        Ok(())
    }
}

/// The test provider that signs in with a stored username and password.
pub(crate) const PASSWORD_PROVIDER: &str = "example_password";
/// The vault references of its two credentials.
pub(crate) const USER_REF: &str = "vault://example/app/user";
pub(crate) const PASSWORD_REF: &str = "vault://example/app/password";

/// Every username and password [`PasswordProvider`] was handed, in order.
pub(crate) static PASSWORDS_SEEN: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Signs in with the two credentials its entry names, `user` and
/// `password`, as a provider that fills an application's sign-in form uses
/// them. It keeps what it was handed, so that a test sees both reached it.
struct PasswordProvider;

impl a2m_auth::AuthProvider for PasswordProvider {
    fn name(&self) -> a2m_auth::ProviderName {
        a2m_auth::ProviderName::fixed(PASSWORD_PROVIDER)
    }

    fn needs(&self) -> a2m_auth::ProviderNeeds {
        a2m_auth::ProviderNeeds {
            person: false,
            silent_renewal: false,
            light_prompt: false,
            movable: false,
            confirms: true,
        }
    }

    fn check(&self, _entry: &a2m_auth::AuthEntry) -> Result<(), a2m_core::Failure> {
        Ok(())
    }

    fn sign_in<'a>(
        &'a self,
        cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<a2m_auth::Material, a2m_core::Failure>> {
        Box::pin(async move {
            let user = cx.credential("user").await?;
            let password = cx.credential("password").await?;
            PASSWORDS_SEEN
                .lock()
                .unwrap()
                .push((user.expose().to_owned(), password.expose().to_owned()));
            Ok(a2m_auth::Material::new(self.name()).with_plain("signed_in", "yes"))
        })
    }

    fn is_signed_in<'a>(
        &'a self,
        _material: &'a a2m_auth::Material,
        _now: Timestamp,
    ) -> a2m_auth::BoxFuture<'a, a2m_auth::SignedIn> {
        Box::pin(async { a2m_auth::SignedIn::Yes { until: None } })
    }

    fn detect_expiry(&self, _sign: &a2m_auth::RawSign) -> Option<a2m_auth::ExpirySign> {
        None
    }

    fn refresh<'a>(
        &'a self,
        _material: &'a a2m_auth::Material,
        _cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<a2m_auth::Material, a2m_core::Failure>> {
        Box::pin(async {
            Err(a2m_core::Failure::new(
                a2m_core::FailureClass::SessionExpired,
                a2m_auth::codes::NEEDS_LOGIN,
                "The test provider does not renew.",
                "Sign in again.",
            ))
        })
    }

    fn sign_out<'a>(
        &'a self,
        _material: &'a a2m_auth::Material,
        _cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<(), a2m_core::Failure>> {
        Box::pin(async { Ok(()) })
    }

    fn inject(
        &self,
        _material: &a2m_auth::Material,
        _sink: &mut dyn a2m_auth::CredentialSink,
    ) -> Result<(), a2m_core::Failure> {
        Ok(())
    }
}

/// The published plan, with its first surface signed in to through
/// [`PasswordProvider`] with a username and a password stored for the
/// person, probed with the read the plan has first.
fn declare_password_sign_in(plan: &mut a2m_core::Plan) {
    declare_code_sign_in(plan);
    let Some(auth) = plan.auth.last_mut() else {
        unreachable!("the code sign-in was just declared");
    };
    auth.provider = a2m_auth::ProviderName::fixed(PASSWORD_PROVIDER);
    auth.credentials = vec![
        a2m_core::CredentialUse {
            role: "password".to_owned(),
            reference: a2m_core::CredentialRef::new(PASSWORD_REF).unwrap(),
        },
        a2m_core::CredentialUse {
            role: "user".to_owned(),
            reference: a2m_core::CredentialRef::new(USER_REF).unwrap(),
        },
    ];
}

/// The published plan, with its first surface signed in to through the
/// test provider, probed with the read the plan has first.
fn declare_code_sign_in(plan: &mut a2m_core::Plan) {
    let mut inputs = a2m_core::Values::new();
    inputs.insert("order_id", a2m_core::Value::Text("A-100".to_owned()));
    plan.auth.push(a2m_core::AuthPlan {
        surfaces: vec![a2m_core::SurfaceIndex::new(0)],
        mode: a2m_core::SignInMode::ManagedSession,
        provider: a2m_auth::ProviderName::fixed(CODE_PROVIDER),
        probe: a2m_core::ProbePlan::Operation {
            operation: a2m_core::OperationIndex::new(0),
            inputs,
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
}
