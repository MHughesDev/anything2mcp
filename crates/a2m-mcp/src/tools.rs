//! Tool lists and tool calls, as data and dispatches.
//!
//! Nothing here knows the protocol's types. A list is built at the moment
//! it is asked for: the MCP faces of the actions the toolset lists, then
//! the targets' published tools, read a page at a time through the action
//! `tool.search`, which gives each tool in full. A call is one dispatch of
//! the action a name stands for. Every one of these goes through the
//! dispatcher as the caller, so this crate reads nothing the caller may
//! not.
//!
//! A caller that asked for progress is told what its run does. The run's
//! events are read through the kernel, which shows a caller only a run the
//! same caller started. What is said about an event is a sentence of this
//! crate with a number or a state's name in it, never text a system gave.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use a2m_actions::{
    BuiltInToolset, Caller, ErrorBody, Kernel, McpResultFace, ProjectFolder, Request, Response,
    RunEvents, ToolAnnotationsFace, mcp_tools,
};
use a2m_core::{FailureClass, OperationName, RunId, RunState};
use a2m_policy::ToolsetName;
use serde_json::{Map, Value as Json, json};

use crate::settings::{Listing, McpSettings};

/// How many tools one `tool.search` is asked for, each in full. A page
/// stays under the result size cap; one that does not is asked for again
/// at half the size.
const PAGE: usize = 20;

/// The input field that says how long an action waits for its run.
const WAIT_FIELD: &str = "wait_seconds";
/// The input field that names the run an action continues.
const RUN_FIELD: &str = "run_id";
/// How long an action waits for its run when the call does not say, in
/// seconds, as the action registry has it.
const DEFAULT_WAIT: u64 = 60;
/// The longest an action waits for its run, in seconds.
const LONGEST_WAIT: u64 = 600;
/// How many of a run's events one read asks for.
const EVENTS: u32 = 50;

/// The key of the warnings in a result's `_meta`.
pub(crate) const WARNINGS_KEY: &str = "anything2mcp/warnings";

/// What an endpoint works with.
pub(crate) struct Core {
    pub(crate) kernel: Kernel,
    pub(crate) toolset: ToolsetName,
    pub(crate) builtin: BuiltInToolset,
    pub(crate) settings: McpSettings,
    /// How many requests this endpoint handed the dispatcher. Read by the
    /// tests that prove a malformed message never gets that far.
    pub(crate) dispatched: AtomicU64,
    /// How many calls stopped waiting because their caller cancelled or
    /// closed the stream.
    pub(crate) cancelled: tokio::sync::watch::Sender<u64>,
}

/// Who asks, as the mount of the endpoint stated it: the caller the kernel
/// let in, and the project folder the client end works in, when it named
/// one. Nothing in a message sets either.
#[derive(Debug, Clone)]
pub(crate) struct Asker {
    pub(crate) caller: Caller,
    pub(crate) project: Option<ProjectFolder>,
}

/// One tool of a list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolEntry {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) input_schema: Map<String, Json>,
    pub(crate) annotations: ToolAnnotationsFace,
}

/// A tool list, and what it had to leave to the search tool.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolList {
    pub(crate) tools: Vec<ToolEntry>,
    /// Published tools that would have been listed had there been room.
    pub(crate) past_limit: usize,
}

/// What a call by name came to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Called {
    /// No tool of that name is on this endpoint. With the dispatcher's own
    /// words, when it was the dispatcher that said so.
    Unknown(Option<ErrorBody>),
    /// The dispatcher's answer.
    Answered(Box<Rendered>),
}

/// A response as an MCP result carries it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rendered {
    pub(crate) face: McpResultFace,
    /// The warnings, for the result's `_meta`.
    pub(crate) meta: Map<String, Json>,
}

/// The one dispatch a tool's name and arguments stand for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Route {
    action: String,
    input: Json,
    /// Whether the name is a published tool's own. Whether such a tool is
    /// published is the dispatcher's to say.
    published: bool,
    /// How the call's run can be followed for a caller that asked for
    /// progress.
    pub(crate) follows: Follows,
}

/// How a call's run is followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Follows {
    /// The call has no run to follow, or does not wait for one: it is
    /// dispatched whole.
    Nothing,
    /// The call starts a run and waits for it this long. It is started
    /// without the wait, and the run is followed in its place.
    Started(Duration),
    /// The call continues this run, which the caller names. It is
    /// dispatched as written, and the run is followed meanwhile.
    Continued(RunId),
}

/// Where a followed run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stand {
    /// A job for it waits or is in hand.
    Working,
    /// It rests or has stopped, and nothing happens until someone acts.
    Rests,
    /// It has ended.
    Ended,
    /// Its events could not be read.
    Lost,
}

/// A run that is being followed for a caller.
#[derive(Debug)]
pub(crate) struct Following {
    run: RunId,
    /// The number of the last event read.
    after: u64,
    /// When the wait the caller asked for is over.
    until: Instant,
    pub(crate) stand: Stand,
    verified: u64,
    failed: u64,
    /// What the call that started the run answered: the answer, should
    /// nothing better be had.
    started: Option<Box<Rendered>>,
}

impl Following {
    fn new(run: RunId, after: u64, wait: Duration) -> Self {
        Self {
            run,
            after,
            until: Instant::now() + wait,
            stand: Stand::Working,
            verified: 0,
            failed: 0,
            started: None,
        }
    }

    /// Whether the run may still say something within the caller's wait.
    pub(crate) fn open(&self) -> bool {
        self.stand == Stand::Working && Instant::now() < self.until
    }

    /// Whether nothing more will be read of the run: it has ended, its
    /// events cannot be read, or the time is up.
    pub(crate) fn over(&self) -> bool {
        matches!(self.stand, Stand::Ended | Stand::Lost) || Instant::now() >= self.until
    }

    /// What to tell the caller about a page of the run's events. Every
    /// sentence is this crate's own. It holds a count, a step's number, or
    /// the name of a state, and nothing a system said: an event's text is
    /// never passed on, so it can never read as an instruction.
    fn notes(&mut self, page: &RunEvents) -> Vec<String> {
        let mut notes = Vec::new();
        let (mut verified, mut failed, mut reread) = (0_u64, 0_u64, 0_u64);
        for event in &page.events {
            match event.kind.as_str() {
                "state_changed" => {
                    let to = event.detail["to"].as_str();
                    if let Some(state) = to.and_then(|name| name.parse::<RunState>().ok()) {
                        notes.push(format!("The run is now `{state}`."));
                    }
                }
                "step_started" => {
                    if let Some(step) = event.detail["step"].as_u64() {
                        notes.push(format!("Step {step} began."));
                    }
                }
                "planned" => {
                    if let Some(records) = event.detail["records"].as_array() {
                        notes.push(format!("{} records were planned.", records.len()));
                    }
                }
                "diff_stored" => notes.push("The preview of the change was stored.".to_owned()),
                "verified" => verified += 1,
                "failed" => failed += 1,
                "reconcile_read" => reread += 1,
                _ => {}
            }
        }
        // A run of many records says how far it is, once for each page.
        if verified > 0 {
            self.verified += verified;
            notes.push(format!("{} records verified so far.", self.verified));
        }
        if failed > 0 {
            self.failed += failed;
            notes.push(format!("{} records failed so far.", self.failed));
        }
        if reread > 0 {
            notes.push(format!("{reread} records in doubt were read again."));
        }
        notes
    }
}

impl Core {
    async fn dispatch(&self, action: &str, input: Json, asker: &Asker) -> Response {
        self.dispatched.fetch_add(1, Ordering::Relaxed);
        self.kernel
            .dispatch(
                Request::new(action, input, asker.caller.clone())
                    .in_project_folder(asker.project.as_ref()),
            )
            .await
    }

    /// The tools this endpoint lists: the same tools in the same order
    /// every time, for as long as nothing is published. The control tools
    /// first, then the published pipelines up to the limit; with a full
    /// listing, every published tool. One dispatch for each page of
    /// published tools.
    ///
    /// # Errors
    ///
    /// The dispatcher's refusal of the search.
    pub(crate) async fn list(&self, asker: &Asker) -> Result<ToolList, ErrorBody> {
        let mut tools: Vec<ToolEntry> = mcp_tools(self.kernel.actions(), self.builtin)
            .into_iter()
            .map(|face| ToolEntry {
                name: face.name,
                title: face.title,
                description: face.description,
                input_schema: object(face.input_schema),
                annotations: face.annotations,
            })
            .collect();
        let limit = match self.settings.listing {
            Listing::Default => self.settings.listed_max,
            Listing::Full => McpSettings::FULL_CEILING,
        };
        let mut past_limit = 0;
        let mut offset = 0;
        let mut size = PAGE;
        loop {
            let found = self
                .dispatch(
                    "tool.search",
                    json!({"full": true, "limit": size, "offset": offset}),
                    asker,
                )
                .await;
            let result = answered(found)?;
            let Some(page) = result["tools"].as_array() else {
                // The page was larger than the result size cap, so it holds
                // no tools. Half as many are asked for; a single tool that
                // is too large to list is left to the search tool.
                if size > 1 {
                    size /= 2;
                } else {
                    past_limit += 1;
                    offset += 1;
                }
                continue;
            };
            for tool in page {
                let listed = match self.settings.listing {
                    Listing::Default => tool["kind"] == "pipeline",
                    Listing::Full => true,
                };
                let Some(name) = tool["name"].as_str().filter(|_| listed) else {
                    continue;
                };
                if tools.len() < limit {
                    tools.push(entry(name, tool));
                } else {
                    past_limit += 1;
                }
            }
            offset += page.len();
            let total = result["total"].as_u64().unwrap_or(0);
            if page.is_empty() || u64::try_from(offset).unwrap_or(u64::MAX) >= total {
                break;
            }
        }
        Ok(ToolList { tools, past_limit })
    }

    /// The dispatch the tool `name` stands for, or `None` when no such
    /// name can be a tool of this endpoint. A name of an action this
    /// toolset lists is that action; any other name is a published tool,
    /// run by `tool.call`.
    pub(crate) fn route(&self, name: &str, arguments: Map<String, Json>) -> Option<Route> {
        let registry = self.kernel.actions();
        let (action, input, published) = if let Some(def) = registry.by_tool_name(name) {
            // An action this toolset does not list is reached through
            // `a2m_action_run`, never by a name the list does not hold.
            if !registry
                .toolset(self.builtin)
                .iter()
                .any(|listed| listed.name == def.name)
            {
                return None;
            }
            (def.name.to_string(), Json::Object(arguments), false)
        } else {
            OperationName::new(name).ok()?;
            (
                "tool.call".to_owned(),
                json!({"tool": name, "input": Json::Object(arguments)}),
                true,
            )
        };
        // An action that waits for a run says so in its input. A wait that
        // is not what the schema takes is left for the dispatcher to
        // refuse, and a call that asks for no wait gets none.
        let waits = registry.find(&action).is_some_and(|def| {
            def.input
                .fields
                .iter()
                .any(|field| field.name == WAIT_FIELD)
        });
        let wait = match input.get(WAIT_FIELD) {
            None => Some(DEFAULT_WAIT),
            Some(given) => given.as_u64().filter(|seconds| *seconds <= LONGEST_WAIT),
        };
        let follows = match wait.filter(|seconds| waits && *seconds > 0) {
            None => Follows::Nothing,
            Some(seconds) => match input.get(RUN_FIELD) {
                None => Follows::Started(Duration::from_secs(seconds)),
                Some(named) => named
                    .as_str()
                    .and_then(|text| text.parse::<RunId>().ok())
                    .map_or(Follows::Nothing, Follows::Continued),
            },
        };
        Some(Route {
            action,
            input,
            published,
            follows,
        })
    }

    /// Makes the call, whole: one dispatch, answered when the action is
    /// done. The input is validated by the dispatcher against the full
    /// schema, whatever a list showed.
    pub(crate) async fn call(&self, route: Route, asker: &Asker) -> Called {
        let response = self.dispatch(&route.action, route.input, asker).await;
        self.called(&response, route.published)
    }

    /// Starts a call that waits for a run without the wait, so that the
    /// run can be followed. `Err` holds the answer when there is no run to
    /// follow: the call was refused, or its input was not taken.
    pub(crate) async fn start(
        &self,
        route: &Route,
        wait: Duration,
        asker: &Asker,
    ) -> Result<Following, Called> {
        let response = self
            .dispatch(&route.action, without_wait(&route.input), asker)
            .await;
        let called = self.called(&response, route.published);
        let run = response
            .result
            .as_ref()
            .filter(|_| response.error.is_none())
            .and_then(|result| {
                let run = result[RUN_FIELD].as_str()?.parse::<RunId>().ok()?;
                // The events the answer already counted are not told again.
                Some((run, result["events"].as_u64().unwrap_or(0)))
            });
        match (run, called) {
            (Some((run, after)), Called::Answered(started)) => {
                let mut following = Following::new(run, after, wait);
                following.started = Some(started);
                Ok(following)
            }
            (_, called) => Err(called),
        }
    }

    /// The run a call continues, to be followed from its last event on, or
    /// `None` when the caller started no such run.
    pub(crate) async fn resume_from(&self, run: RunId, asker: &Asker) -> Option<Following> {
        let mut after = 0;
        loop {
            let page = self
                .kernel
                .events(
                    &asker.caller.principal,
                    run,
                    after,
                    a2m_actions::EVENTS_PAGE,
                )
                .await
                .ok()?;
            let full = page.events.len() >= a2m_actions::EVENTS_PAGE as usize;
            after = page.last;
            if !full {
                break;
            }
        }
        Some(Following::new(
            run,
            after,
            Duration::from_secs(LONGEST_WAIT),
        ))
    }

    /// Waits for the followed run to say something, and returns what to
    /// tell the caller about it. It waits on the kernel's notice that a
    /// run changed. It only reads, so it may be dropped at any moment.
    pub(crate) async fn follow(&self, following: &mut Following, asker: &Asker) -> Vec<String> {
        let wait = following.until.saturating_duration_since(Instant::now());
        let page = self
            .kernel
            .next_events(
                &asker.caller.principal,
                following.run,
                following.after,
                EVENTS,
                wait,
            )
            .await;
        let Ok(page) = page else {
            following.stand = Stand::Lost;
            return Vec::new();
        };
        following.after = page.last;
        following.stand = if page.ended {
            Stand::Ended
        } else if page.settled() {
            Stand::Rests
        } else {
            Stand::Working
        };
        following.notes(&page)
    }

    /// The answer to a call whose run was followed.
    ///
    /// A run that has ended, or is still worked on when the caller's wait
    /// is over, is read with `run.get`. A run that came to rest is answered
    /// by the caller's own call made once more, without a wait: the same
    /// call is the same run for as long as the run is unfinished, and only
    /// a call that starts or continues a run tells the run's person that it
    /// waits for them. Reading a run asks nobody anything.
    pub(crate) async fn finish(&self, route: Route, following: Following, asker: &Asker) -> Called {
        let response = if following.stand == Stand::Rests {
            self.dispatch(&route.action, without_wait(&route.input), asker)
                .await
        } else {
            self.dispatch(
                "run.get",
                json!({RUN_FIELD: following.run.to_string()}),
                asker,
            )
            .await
        };
        match (response.result.is_some(), following.started) {
            // The run could not be read, by a refusal that is about the
            // read and not about the run. What the call that started the
            // run answered still holds: the run's identifier, and the
            // call to make next.
            (false, Some(started)) => Called::Answered(started),
            _ => self.called(&response, false),
        }
    }

    /// A response as the answer to a call. For a published tool called by
    /// its own name, the dispatcher's `input`, `not_found` with no result
    /// says that no such tool is published: an unknown tool, which the
    /// protocol answers differently from a call that failed. A run that
    /// failed for want of a record carries a result, and is a result.
    fn called(&self, response: &Response, published: bool) -> Called {
        if let (true, Some(error), None) = (published, &response.error, &response.result)
            && error.class == FailureClass::Input
            && error.code == a2m_actions::codes::NOT_FOUND.as_str()
        {
            return Called::Unknown(Some(error.clone()));
        }
        Called::Answered(Box::new(render(response)))
    }
}

/// The one rendering of a response: the registry's. A success carries the
/// following call inside its structured content, as `next`, which every
/// output schema admits (spec section 221). Its warnings go into the
/// result's `_meta`, because the output schemas take no other key. A
/// failure carries its class, code, message, and next step, and nothing
/// else.
fn render(response: &Response) -> Rendered {
    let mut meta = Map::new();
    if response.error.is_none() && !response.warnings.is_empty() {
        meta.insert(WARNINGS_KEY.to_owned(), json!(response.warnings));
    }
    Rendered {
        face: McpResultFace::of(response),
        meta,
    }
}

/// The input of a call that waits for its run, asking for no wait.
fn without_wait(input: &Json) -> Json {
    let mut input = input.clone();
    if let Json::Object(map) = &mut input {
        map.insert(WAIT_FIELD.to_owned(), json!(0));
    }
    input
}

/// One published tool of a list, from what `tool.search` gives in full.
fn entry(name: &str, tool: &Json) -> ToolEntry {
    let hint = |key: &str| tool["annotations"][key].as_bool().unwrap_or(false);
    let title = tool["title"].as_str().unwrap_or(name).to_owned();
    ToolEntry {
        name: name.to_owned(),
        // A list never holds a tool with nothing to choose it by.
        description: tool["description"]
            .as_str()
            .filter(|text| !text.is_empty())
            .unwrap_or(&title)
            .to_owned(),
        title,
        input_schema: object(tool["input_schema"].clone()),
        annotations: ToolAnnotationsFace {
            read_only: hint("read_only"),
            // A tool whose description does not say is treated as one that
            // may do what cannot be undone.
            destructive: tool["annotations"]["destructive"].as_bool().unwrap_or(true),
            idempotent: hint("idempotent"),
            open_world: tool["annotations"]["open_world"].as_bool().unwrap_or(true),
        },
    }
}

/// A schema as the object a tool list carries, which the protocol asks to
/// be of type object.
fn object(value: Json) -> Map<String, Json> {
    let mut map = match value {
        Json::Object(map) => map,
        _ => Map::new(),
    };
    map.entry("type").or_insert_with(|| json!("object"));
    map
}

fn answered(response: Response) -> Result<Json, ErrorBody> {
    match (response.error, response.result) {
        (Some(error), _) => Err(error),
        (None, result) => Ok(result.unwrap_or(Json::Null)),
    }
}
