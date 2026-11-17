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

use crate::{
    LocalSurface, OsUser, PresenceKind, PresencePrompt, PromptAnswer, PromptText, SurfaceParts,
};

pub(crate) const VERSION: Version = Version::new(0, 0, 1);

/// A temporary home for one engine's paths. The folder's name is short,
/// because a socket's path may not be long.
pub(crate) fn home(name: &str) -> (Folder, Paths) {
    let folder = Folder::new(name);
    let paths = Paths::under(folder.path());
    (folder, paths)
}

/// A presence prompt that answers as the test set it, and counts.
pub(crate) struct TestPrompt {
    answer: Mutex<PromptAnswer>,
    asked: AtomicU32,
}

impl TestPrompt {
    pub(crate) fn answering(answer: PromptAnswer) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(answer),
            asked: AtomicU32::new(0),
        })
    }

    pub(crate) fn set(&self, answer: PromptAnswer) {
        *self.answer.lock().unwrap() = answer;
    }

    pub(crate) fn asked(&self) -> u32 {
        self.asked.load(Ordering::SeqCst)
    }
}

impl PresencePrompt for TestPrompt {
    fn kind(&self) -> PresenceKind {
        PresenceKind::OsPrompt
    }

    fn ask(&self, _text: PromptText) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            self.asked.fetch_add(1, Ordering::SeqCst);
            *self.answer.lock().unwrap()
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
            discoverer: Arc::new(engine::WritingDiscoverer {
                results: Mutex::new(Vec::new()),
            }),
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
        assert_eq!(kernel.health().state, HealthState::Ok, "{:?}", kernel.health());
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
