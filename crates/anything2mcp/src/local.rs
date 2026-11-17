//! The local engine: the kernel of one machine, served on the local
//! channel. This is where the role crates are joined, because none of
//! them may name another's types:
//!
//! - the channel of `a2m-local` carries one router;
//! - the router is the REST router of `a2m-api` and, for each built-in
//!   toolset, the MCP endpoint of `a2m-mcp`;
//! - one layer of this crate stands in front of both. It turns the
//!   [`Peer`] the channel saw connect into a caller the kernel verified,
//!   and refuses a request with none;
//! - the person surface is [`LocalSurface`], the only holder of the
//!   approval desk.
//!
//! The channel is bound before the kernel is opened. Binding takes the
//! engine lock, so a second engine on the same data directory is refused
//! before it opens the store; and a client that connects while the kernel
//! opens waits for its hello instead of finding nothing.

use std::sync::Arc;
use std::time::Duration;

use a2m_actions::{
    BoxFuture, BuiltInToolset, Caller, CloseReason, ErrorBody, ExitClass, FrontDoor, Kernel,
    PersonSurface, REST_PREFIX, Request, Response, Settings, codes,
};
use a2m_api::{ApiSettings, EXIT_CLASS_HEADER};
use a2m_core::{Failure, FailureClass, Version};
use a2m_local::{
    Channel, ChannelControl, ClientLabel, IdleHolds, IdlePolicy, LocalSurface, LockPlace, OsUser,
    Peer, PresencePrompt, StopReason, SurfaceParts,
};
use a2m_mcp::{McpService, McpSettings, mcp_path};
use a2m_policy::ToolsetName;
use axum::Router;
use axum::extract::{Request as HttpRequest, State};
use axum::http::{HeaderMap, HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response as HttpResponse};
use serde_json::json;

use crate::engine::{Engine, EngineOptions, Serves, load_settings};
use crate::wire;

/// The action that stops the engine. The dispatcher decides whether its
/// caller may; this process then carries it out.
const STOP_ACTION: &str = "engine.stop";
/// The most a request to stop may send.
const STOP_BODY_LIMIT: usize = 64 * 1024;

/// How the line begins that `serve` writes to standard error about the
/// presence check of this machine, before the line that says it is ready.
pub const PRESENCE_LINE: &str = "anything2mcp: approvals on this machine:";

/// The version of this program, as the hello of the local channel states
/// it.
pub(crate) fn program_version() -> Result<Version, Failure> {
    Ok(crate::version().parse::<Version>()?)
}

/// What keeps an engine with no connection from stopping: a page that
/// waits for a person, and a run in hand, whether a runner works on it or
/// it rests for a person. A run whose caller has gone is thus not lost
/// with an idle engine.
struct Holds {
    surface: LocalSurface,
    kernel: Kernel,
    place: LockPlace,
    control: ChannelControl,
}

/// Whether the engine's data directory is still the one it opened. When it
/// is not, the engine stops: it serves nothing more, and its kernel, whose
/// store is no longer where any later engine would look, closes. Asked on
/// every request, and when an idle engine looks at its holds, so that an
/// engine whose socket went with its data directory, and so gets no
/// request, still notices at its idle time (decision 0181).
fn in_place(place: &LockPlace, control: &ChannelControl) -> bool {
    let holds = place.holds();
    if !holds {
        control.stop(StopReason::DataGone);
    }
    holds
}

impl IdleHolds for Holds {
    fn busy(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            if !in_place(&self.place, &self.control) {
                return false;
            }
            if self.surface.waiting() > 0 {
                return true;
            }
            // A store that cannot say what is in hand is no reason to
            // stop under it: the engine stays, and its health says why.
            self.kernel
                .runs_in_hand()
                .await
                .map_or(true, |runs| runs.any())
        })
    }
}

fn minutes(settings: &Settings, key: &str) -> Option<Duration> {
    let minutes = u64::try_from(settings.int(key)?).ok()?;
    (minutes > 0).then(|| Duration::from_secs(minutes * 60))
}

/// What the layer in front of the front doors needs.
#[derive(Clone)]
struct Door {
    kernel: Kernel,
    control: ChannelControl,
    place: LockPlace,
}

/// A response as the REST interface renders one: the status from its one
/// table, the exit class as a header, and the whole response as the body.
fn render(response: &Response) -> HttpResponse {
    let mut answer = (a2m_api::status_of(response), response.to_json().to_string()).into_response();
    let headers = answer.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        EXIT_CLASS_HEADER,
        HeaderValue::from_static(response.exit.as_str()),
    );
    answer
}

fn refused(action: &str, failure: &Failure) -> Response {
    Response {
        ok: false,
        action: action.to_owned(),
        result: None,
        error: Some(ErrorBody {
            class: failure.class(),
            code: failure.code().to_string(),
            message: failure.message().to_owned(),
            fix: failure.next_step().to_owned(),
            // Nothing was handed to the dispatcher.
            retry_safe: true,
        }),
        warnings: Vec::new(),
        next: None,
        exit: match failure.class() {
            FailureClass::Input => ExitClass::Malformed,
            _ => ExitClass::Failed,
        },
        decision: None,
    }
}

/// The caller a peer of the channel is, once the kernel has checked it.
///
/// Every caller of the channel is an agent, whatever it says of itself.
/// The label of its hello and anything in its headers are its own words,
/// and any program of the user can say them: none of them makes it a
/// person for any decision. So a person's act asked through the channel is
/// refused as one an agent asked, a pre-approval rule that asks for a
/// caller who is no agent is never met through it, and the decision
/// records the caller as an agent. A person is shown only by the presence
/// prompt, raised from a press on the page that shows what is approved.
/// The label still says which door the request came through, for the
/// audit log.
async fn caller_of(kernel: &Kernel, peer: &Peer) -> Result<Caller, Failure> {
    let principal = match kernel.verify_caller(peer.credential()).await {
        Ok(principal) => principal,
        // Only a kernel whose store never opened names nobody: it answers
        // its own status, and nothing else, to nobody.
        Err(failure) if *failure.code() == codes::ENGINE_DEGRADED => kernel.status_only_caller(),
        Err(failure) => return Err(failure),
    };
    let door = match &peer.label {
        ClientLabel::CommandLine => FrontDoor::Command,
        ClientLabel::Mcp(_) => FrontDoor::Mcp,
        ClientLabel::Sdk(_) => FrontDoor::Sdk,
    };
    let mut caller = Caller::new(principal, door, true);
    caller.client = peer.label.as_text();
    Ok(caller)
}

fn text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// The layer in front of both front doors. A request reaches them only
/// with a caller the kernel verified, made from the peer the channel saw
/// connect; nothing a request says names its caller.
async fn admit(State(door): State<Door>, mut request: HttpRequest, next: Next) -> HttpResponse {
    // An engine whose data directory is gone fails closed: it takes no
    // request, says why, and stops (spec section 224).
    if !in_place(&door.place, &door.control) {
        return render(&refused("", &a2m_local::codes::data_directory_gone()));
    }
    let Some(peer) = request.extensions().get::<Peer>().cloned() else {
        return render(&refused(
            "",
            &Failure::new(
                FailureClass::Policy,
                a2m_api::codes::CALLER_MISSING,
                "The request did not come through the local channel.",
                "Call through `anything2mcp`, which connects to the engine as you.",
            ),
        ));
    };
    let named = text(request.headers(), wire::PROJECT_HEADER).map(wire::decode_path);
    let project = match named {
        None => None,
        // The one type both doors read (decision 0132): the REST router and
        // each MCP endpoint behind this layer hand it to the dispatcher,
        // which checks the folder before any handler sees it.
        Some(Some(folder)) => Some(a2m_actions::ProjectFolder::new(folder)),
        Some(None) => {
            return render(&refused(
                "",
                &Failure::new(
                    FailureClass::Input,
                    codes::SCHEMA_INVALID,
                    "The project folder the request names cannot be read.",
                    "Call through `anything2mcp`, which names the folder it was run in.",
                ),
            ));
        }
    };
    let caller = match caller_of(&door.kernel, &peer).await {
        Ok(caller) => caller,
        Err(failure) => return render(&refused("", &failure)),
    };
    let stop = format!("{REST_PREFIX}/actions/{STOP_ACTION}");
    if request.method() == Method::POST && request.uri().path() == stop {
        return stop_engine(&door, caller, request).await;
    }
    request.extensions_mut().insert(caller);
    if let Some(project) = project {
        request.extensions_mut().insert(project);
    }
    next.run(request).await
}

/// `engine.stop`, which acts on this process. The dispatcher is asked
/// first, so that the one decider decides and the decision is recorded;
/// it answers that the action is the client end's to carry out, and this
/// process, which serves the channel, then stops serving.
async fn stop_engine(door: &Door, mut caller: Caller, request: HttpRequest) -> HttpResponse {
    // The body is read so that the answer can be delivered; an input is
    // not taken, because the action has none.
    let _ = axum::body::to_bytes(request.into_body(), STOP_BODY_LIMIT).await;
    caller.door = FrontDoor::Rest;
    let mut response = door
        .kernel
        .dispatch(Request::new(STOP_ACTION, json!({}), caller))
        .await;
    let allowed = response
        .error
        .as_ref()
        .is_some_and(|error| error.code == codes::CLIENT_ACTION.as_str());
    if allowed {
        door.control.stop(StopReason::Asked);
        response.ok = true;
        response.error = None;
        response.result = Some(json!({"stopping": true}));
        response.exit = ExitClass::Succeeded;
    }
    render(&response)
}

/// The one router of the local engine: the REST router and the MCP
/// endpoint of each built-in toolset, behind [`admit`].
fn service(kernel: &Kernel, control: ChannelControl, place: LockPlace) -> Result<Router, Failure> {
    let mut router = a2m_api::router(kernel.clone(), ApiSettings::default())?;
    let settings = McpSettings::from_settings(kernel.settings())?;
    for toolset in [BuiltInToolset::Use, BuiltInToolset::Build] {
        let name = ToolsetName::new(toolset.as_str())?;
        let endpoint = McpService::new(kernel.clone(), name, settings.clone())?;
        router = router.nest_service(&mcp_path(endpoint.toolset()), endpoint.http());
    }
    Ok(router.layer(axum::middleware::from_fn_with_state(
        Door {
            kernel: kernel.clone(),
            control,
            place,
        },
        admit,
    )))
}

/// The local engine, bound and open, not yet serving.
pub struct LocalEngine {
    engine: Engine,
    surface: LocalSurface,
    channel: Channel,
    service: Router,
    idle: IdlePolicy,
}

impl std::fmt::Debug for LocalEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalEngine")
            .field("engine", &self.engine)
            .finish_non_exhaustive()
    }
}

impl LocalEngine {
    /// Binds the local channel, which takes the engine lock, and opens the
    /// kernel behind it with [`LocalSurface`] as its person surface. The
    /// person surface of `options` is not used; `prompt` is the presence
    /// prompt of this machine, which `a2m_local::system_prompt` gives.
    ///
    /// # Errors
    ///
    /// `busy`/`engine_running` when an engine already runs for this user
    /// and data directory; `busy`/`channel_unavailable` when the channel
    /// cannot be opened; what [`Engine::open`] answers for settings that
    /// do not load; `input`/`mcp_not_served` and `input`/`route_conflict`
    /// from the front doors.
    pub async fn start(
        mut options: EngineOptions,
        prompt: Arc<dyn PresencePrompt>,
    ) -> Result<Self, Failure> {
        let settings = load_settings(&options)?;
        let channel = Channel::bind(&options.paths, program_version()?).await?;
        // The caller of the channel is the user the operating system
        // reports, so that is the name the kernel is opened for.
        options.os_user = OsUser::current()?.as_str().to_owned();
        let surface = LocalSurface::new(SurfaceParts {
            prompt,
            clock: Arc::clone(&options.clock),
            random: Arc::clone(&options.random),
            window: minutes(&settings, "policy.approval_window_minutes")
                .unwrap_or(Duration::from_secs(15 * 60)),
        });
        options.person = Arc::new(surface.clone()) as Arc<dyn PersonSurface>;
        let idle_after = minutes(&settings, "engine.idle_stop_minutes");
        // The channel holds the engine lock; the engine takes none of its
        // own.
        let engine = Engine::open_loaded(options, settings, None, Serves::AnyProject).await?;
        let idle = match idle_after {
            Some(after) => IdlePolicy::after(
                after,
                Arc::new(Holds {
                    surface: surface.clone(),
                    kernel: engine.kernel().clone(),
                    place: channel.lock_place(),
                    control: channel.control(),
                }),
            ),
            None => IdlePolicy::never(),
        };
        let service = match service(engine.kernel(), channel.control(), channel.lock_place()) {
            Ok(service) => service,
            Err(failure) => {
                engine.close().await;
                return Err(failure);
            }
        };
        Ok(Self {
            engine,
            surface,
            channel,
            service,
            idle,
        })
    }

    /// The engine behind the channel.
    #[must_use]
    pub const fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The person surface of this engine.
    #[must_use]
    pub const fn surface(&self) -> &LocalSurface {
        &self.surface
    }

    /// What approvals on this machine need, as one line for a person:
    /// the kind of presence check, the reason, and what to do where
    /// nothing can be raised. `serve` says it before it serves. It asks
    /// the prompt what it can do and raises nothing.
    pub async fn presence_line(&self) -> String {
        let presence = self.surface.presence().await;
        let prompt = presence
            .prompt
            .map_or_else(String::new, |prompt| format!(" ({})", prompt.as_str()));
        let said = format!("{} {}", presence.message, presence.remedy);
        format!(
            "{PRESENCE_LINE} {}, {}{prompt}. {}",
            presence.kind.as_str(),
            presence.reason.as_str(),
            said.trim_end()
        )
    }

    /// The handle that stops the engine once it serves.
    #[must_use]
    pub fn control(&self) -> ChannelControl {
        self.channel.control()
    }

    /// Serves the channel until the engine is asked to stop or has been
    /// idle for `engine.idle_stop_minutes`, then closes the kernel. A page
    /// that still waits for a person is lost with the engine, which is a
    /// decline. The engine lock is held until the kernel has closed, so
    /// that no second engine opens the store while this one still writes.
    pub async fn serve(self) -> StopReason {
        let (stopped, lock) = self.channel.serve(self.service, self.idle).await;
        let why = match stopped {
            StopReason::Idle => CloseReason::Idle,
            StopReason::Restart => CloseReason::Restart,
            StopReason::Asked | StopReason::Failed | StopReason::DataGone => CloseReason::Asked,
        };
        self.engine.close_because(why).await;
        drop(lock);
        stopped
    }
}
