//! A kernel on a temporary data directory with memory key stores, the test
//! pack registered, and the `example` target published.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Caller, CallerCredential, FrontDoor, Kernel,
    KernelParts, Paths, PersonSurface, PresenceAsk, PresenceProof, Request, Response, SessionAsk,
    Settings, SignInAsk, StorePlace, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::{Failure, FailureClass, Timestamp};
use a2m_policy::{PresenceCheck, ToolsetName};
use a2m_provider::{RegistryBuilder, Roots, Supervisor, SupervisorLimits};
use a2m_telemetry::{Clock, ManualClock};
use a2m_vault::MemoryKeyStore;
use example_app::harness::TestApp;
use serde_json::Value as Json;

use super::engine::{self, AppPack, Counted, Folder, START};
use crate::{McpPeer, McpService, McpSettings};

pub const USER: &str = "pat";

/// The person surface of a test: it keeps the desk it was handed, and
/// answers the presence prompt as the test set it.
pub struct Surface {
    pub desk: Mutex<Option<Arc<ApprovalDesk>>>,
    pub approvals: AtomicU32,
    pub person_present: Mutex<bool>,
    clock: Arc<dyn Clock>,
}

impl PersonSurface for Surface {
    fn take_desk(&self, desk: ApprovalDesk) {
        *self.desk.lock().unwrap() = Some(Arc::new(desk));
    }

    fn ask_approval(&self, ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            self.approvals.fetch_add(1, Ordering::SeqCst);
            Ok(SurfaceLink {
                url: Some(format!("http://127.0.0.1/approve/{}", ask.run)),
                text: "Open the approval page.".to_owned(),
            })
        })
    }

    fn ask_sign_in(&self, _ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            Ok(SurfaceLink {
                url: None,
                text: "Sign in at the engine's own window.".to_owned(),
            })
        })
    }

    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            if *self.person_present.lock().unwrap() {
                Ok(PresenceProof::new(
                    PresenceCheck::OsPrompt,
                    ask.principal,
                    self.clock.now(),
                ))
            } else {
                Err(Failure::new(
                    FailureClass::Policy,
                    a2m_policy::PRESENCE_REQUIRED,
                    "Nobody answered the presence prompt.",
                    "Answer the prompt at the machine.",
                ))
            }
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        Box::pin(async move { PersonAnswer::NoAnswer })
    }
}

pub struct World {
    pub app: TestApp,
    pub kernel: Kernel,
    pub surface: Arc<Surface>,
    _folder: Folder,
}

impl World {
    /// A healthy kernel with the `example` target published. With
    /// `runner`, runs are carried out; without, a run stays queued, which
    /// is how a test holds a call open.
    pub async fn open(runner: bool) -> Self {
        let app = TestApp::start().await.unwrap();
        let folder = Folder::new("mcp");
        let clock: Arc<dyn Clock> =
            Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
        let prepared = Kernel::prepare(KernelParts {
            settings: Settings::defaults(),
            paths: Paths::under(folder.path()),
            place: StorePlace::Memory,
            key_store: Arc::new(MemoryKeyStore::with_random(Counted::new())),
            clock: Arc::clone(&clock),
            random: Counted::new(),
            os_user: USER.to_owned(),
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
        let surface = Arc::new(Surface {
            desk: Mutex::new(None),
            approvals: AtomicU32::new(0),
            person_present: Mutex::new(true),
            clock,
        });
        let kernel = Kernel::open(
            prepared,
            registry,
            problems,
            Arc::clone(&surface) as Arc<dyn PersonSurface>,
        )
        .await;
        if runner {
            kernel.start_runner_loop(4).await.unwrap();
        }
        kernel
            .publish_plan(&engine::plan(&app.url("")))
            .await
            .unwrap();
        Self {
            app,
            kernel,
            surface,
            _folder: folder,
        }
    }

    pub async fn ready() -> Self {
        Self::open(true).await
    }

    /// The endpoint of a toolset over this kernel.
    pub fn service(&self, toolset: &str, settings: McpSettings) -> McpService {
        McpService::new(
            self.kernel.clone(),
            ToolsetName::new(toolset).unwrap(),
            settings,
        )
        .unwrap()
    }

    /// One dispatch straight at the kernel, as the local user: how a test
    /// sets something up without going through the door it tests.
    pub async fn direct(&self, action: &str, input: Json) -> Response {
        let principal = self
            .kernel
            .verify_caller(CallerCredential::LocalUser(USER.to_owned()))
            .await
            .unwrap();
        let caller = Caller::new(principal, FrontDoor::Command, false);
        self.kernel
            .dispatch(Request::new(action, input, caller))
            .await
    }
}

/// The peer the local channel would have seen.
pub fn peer() -> McpPeer {
    McpPeer {
        user: USER.to_owned(),
        label: "conformance".to_owned(),
    }
}
