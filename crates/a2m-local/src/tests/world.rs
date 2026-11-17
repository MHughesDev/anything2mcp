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
        kernel
            .publish_plan(&engine::plan(&app.url("")))
            .await
            .unwrap();
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
