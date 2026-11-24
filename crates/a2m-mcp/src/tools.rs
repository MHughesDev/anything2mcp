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
    BuiltInToolset, Caller, ErrorBody, Followed, Kernel, McpResultFace, ProjectFolder, Request,
    Response, RunEvents, ToolAnnotationsFace, mcp_tools,
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
/// The key of a published tool's target in the tool's `_meta`.
pub(crate) const TOOL_TARGET_KEY: &str = "anything2mcp/target";
/// The key of a published tool's version in the tool's `_meta`.
pub(crate) const TOOL_VERSION_KEY: &str = "anything2mcp/version";
/// The `_meta` key of who wrote a published tool's description.
pub(crate) const TOOL_DESCRIPTION_BY_KEY: &str = "anything2mcp/description_by";

/// The `_meta` key of why a published tool cannot be called now.
pub(crate) const TOOL_UNAVAILABLE_KEY: &str = "anything2mcp/unavailable";
/// The sentence a published tool a drift stopped begins its description
/// with (decision 0396). The product's own words, never the system's.
pub(crate) const UNAVAILABLE_SENTENCE: &str =
    "Unavailable: the system changed and this tool was stopped until its target is repaired.";

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
    /// How many calls began to follow the run they started. Read by the
    /// tests that hold a run back until its call is being followed.
    pub(crate) following: tokio::sync::watch::Sender<u64>,
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
    /// What the tool's `_meta` carries: for a published tool, its target and
    /// version under this product's own prefix; empty for a control tool.
    pub(crate) meta: Map<String, Json>,
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
    /// The idempotency key the call carried in its `_meta`, under
    /// [`KEY_META`]: the same call with the same key is the same run.
    pub(crate) key: Option<String>,
}

/// The key of a call's idempotency key in a request's `_meta`. A request's
/// JSON-RPC id is the connection's and not a key: it is not carried over a
/// new connection, and the stateless revision has no connection at all.
pub(crate) const KEY_META: &str = "anything2mcp/idempotency_key";

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
    /// Its state is a resting one, and the job that brought it there is
    /// still in a runner's hands: it rests as soon as that job is recorded
    /// as ended, which the kernel's wait tells at once because the
    /// follower says it last saw the run unsettled.
    Settling,
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
    /// What was last read of the run: the number of its last event, and
    /// whether it was settled then.
    seen: Followed,
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
    fn new(run: RunId, seen: Followed, wait: Duration) -> Self {
        Self {
            run,
            seen,
            until: Instant::now() + wait,
            stand: Stand::Working,
            verified: 0,
            failed: 0,
            started: None,
        }
    }

    /// Whether the run may still say something within the caller's wait.
    pub(crate) fn open(&self) -> bool {
        matches!(self.stand, Stand::Working | Stand::Settling) && Instant::now() < self.until
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
        self.dispatch_keyed(action, input, None, asker).await
    }

    /// The same, with the idempotency key the call carried in its `_meta`.
    /// The dispatcher checks it.
    async fn dispatch_keyed(
        &self,
        action: &str,
        input: Json,
        key: Option<&str>,
        asker: &Asker,
    ) -> Response {
        self.dispatched.fetch_add(1, Ordering::Relaxed);
        let mut request = Request::new(action, input, asker.caller.clone())
            .in_project_folder(asker.project.as_ref());
        request.idempotency_key = key.map(str::to_owned);
        self.kernel.dispatch(request).await
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
                meta: Map::new(),
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
                // is too large to list is left to the search tool. Any
                // other answer without tools ends the list.
                if result["truncated"] != true {
                    break;
                }
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
            key: None,
        })
    }

    /// Makes the call, whole: one dispatch, answered when the action is
    /// done. The input is validated by the dispatcher against the full
    /// schema, whatever a list showed.
    pub(crate) async fn call(&self, route: Route, asker: &Asker) -> Called {
        let response = self
            .dispatch_keyed(&route.action, route.input, route.key.as_deref(), asker)
            .await;
        self.called(&response, route.published)
    }

    /// Starts a call that waits for a run without the wait, so that the
    /// run can be followed. `Err` holds the answer when there is no run to
    /// follow: the call was refused, its input was not taken, or its run
    /// already rests or has ended.
    pub(crate) async fn start(
        &self,
        route: &Route,
        wait: Duration,
        asker: &Asker,
    ) -> Result<Following, Called> {
        let response = self
            .dispatch_keyed(
                &route.action,
                without_wait(&route.input),
                route.key.as_deref(),
                asker,
            )
            .await;
        let called = self.called(&response, route.published);
        let run = response
            .result
            .as_ref()
            .filter(|_| response.error.is_none())
            .and_then(|result| {
                let run = result[RUN_FIELD].as_str()?.parse::<RunId>().ok()?;
                // A run that already rests or has ended is answered: the
                // call that found it so has said all there is to say.
                let state = result["status"].as_str()?.parse::<RunState>().ok()?;
                if !active(state) {
                    return None;
                }
                // The events the answer already counted are not told again,
                // and the run was being worked on when it answered.
                let after = result["events"].as_u64().unwrap_or(0);
                Some((
                    run,
                    Followed {
                        after,
                        settled: Some(false),
                    },
                ))
            });
        match (run, called) {
            (Some((run, seen)), Called::Answered(started)) => {
                let mut following = Following::new(run, seen, wait);
                following.started = Some(started);
                Ok(following)
            }
            (_, called) => Err(called),
        }
    }

    /// The run a call continues, to be followed from its last event on, or
    /// `None` when the caller started no such run.
    pub(crate) async fn resume_from(&self, run: RunId, asker: &Asker) -> Option<Following> {
        let mut seen = Followed::default();
        loop {
            let page = self
                .kernel
                .events(
                    &asker.caller.principal,
                    run,
                    seen.after,
                    a2m_actions::EVENTS_PAGE,
                )
                .await
                .ok()?;
            let full = page.events.len() >= a2m_actions::EVENTS_PAGE as usize;
            seen = page.followed();
            if !full {
                break;
            }
        }
        Some(Following::new(run, seen, Duration::from_secs(LONGEST_WAIT)))
    }

    /// Waits for the followed run to say something, and returns what to
    /// tell the caller about it. It waits on the kernel's notice that a
    /// run changed, from what was last read of the run, so that a run that
    /// came to rest since is told at once. It only reads, so it may be
    /// dropped at any moment.
    pub(crate) async fn follow(&self, following: &mut Following, asker: &Asker) -> Vec<String> {
        let wait = following.until.saturating_duration_since(Instant::now());
        let page = self
            .kernel
            .next_events_from(
                &asker.caller.principal,
                following.run,
                following.seen,
                EVENTS,
                wait,
            )
            .await;
        let Ok(page) = page else {
            following.stand = Stand::Lost;
            return Vec::new();
        };
        following.seen = page.followed();
        following.stand = if page.ended {
            Stand::Ended
        } else if page.settled() {
            Stand::Rests
        } else if active(page.state) {
            Stand::Working
        } else {
            Stand::Settling
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
            self.dispatch_keyed(
                &route.action,
                without_wait(&route.input),
                route.key.as_deref(),
                asker,
            )
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
        Called::Answered(Box::new(render(
            response,
            self.kernel.actions(),
            self.builtin,
        )))
    }
}

/// The one rendering of a response: the registry's. A success carries the
/// following call inside its structured content, as `next`, which every
/// output schema admits (spec section 221). Its warnings go into the
/// result's `_meta`, because the output schemas take no other key. A
/// failure carries its class, code, message, and next step, and nothing
/// else. A `next` whose tool this toolset does not list names
/// `a2m_action_run` with the action, so that a caller is never pointed at
/// a tool its list does not hold.
fn render(
    response: &Response,
    registry: &a2m_actions::ActionRegistry,
    toolset: BuiltInToolset,
) -> Rendered {
    let mut meta = Map::new();
    if response.error.is_none() && !response.warnings.is_empty() {
        meta.insert(WARNINGS_KEY.to_owned(), json!(response.warnings));
    }
    Rendered {
        face: McpResultFace::of_in(response, registry, toolset),
        meta,
    }
}

/// Whether a run in this state is being worked on, or will be.
const fn active(state: RunState) -> bool {
    matches!(
        state,
        RunState::Queued | RunState::Validating | RunState::Running
    )
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
    // The target and the version, as the compiled target the tool was
    // published in states them, and who wrote the description, `rule` or
    // `builder` (decision 0307, point 6).
    let mut meta = Map::new();
    for (key, field) in [
        (TOOL_TARGET_KEY, "target"),
        (TOOL_VERSION_KEY, "version"),
        (TOOL_DESCRIPTION_BY_KEY, "description_by"),
    ] {
        if let Some(text) = tool[field].as_str() {
            meta.insert(key.to_owned(), json!(text));
        }
    }
    // A list never holds a tool with nothing to choose it by.
    let mut description = tool["description"]
        .as_str()
        .filter(|text| !text.is_empty())
        .unwrap_or(&title)
        .to_owned();
    // A tool a drift stopped is listed all the same, with its schema, and
    // says so first (decision 0396). Only `false` marks it: a listing that
    // does not say is the one the dispatcher gave before tools were marked.
    if tool["available"] == false {
        description = format!("{UNAVAILABLE_SENTENCE} {description}");
        let why = match &tool["unavailable"] {
            Json::Object(why) => Json::Object(why.clone()),
            _ => json!({"reason": "drift"}),
        };
        meta.insert(TOOL_UNAVAILABLE_KEY.to_owned(), why);
    }
    ToolEntry {
        name: name.to_owned(),
        meta,
        description,
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

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use a2m_actions::{ActionRegistry, BuiltInToolset, ExitClass, Next, Response};
    use serde_json::json;

    use super::render;

    #[test]
    fn a_tool_a_drift_stopped_is_listed_with_its_schema_and_says_so_first() {
        let schema = json!({"type": "object", "properties": {"order_id": {"type": "string"}}});
        let mut tool = json!({
            "name": "example_line_item_update",
            "title": "Update a line item",
            "description": "Changes one line item.",
            "target": "example",
            "version": "1.0.0",
            "input_schema": schema,
            "available": true,
        });
        let open = super::entry("example_line_item_update", &tool);
        assert_eq!(open.description, "Changes one line item.");
        assert!(!open.meta.contains_key(super::TOOL_UNAVAILABLE_KEY));

        let why = json!({"reason": "drift", "knowledge": "line_item", "marked_at": 7});
        tool["available"] = json!(false);
        tool["unavailable"] = why.clone();
        let stopped = super::entry("example_line_item_update", &tool);
        assert_eq!(
            stopped.description,
            format!("{} Changes one line item.", super::UNAVAILABLE_SENTENCE)
        );
        assert_eq!(stopped.meta[super::TOOL_UNAVAILABLE_KEY], why);
        assert_eq!(stopped.input_schema, open.input_schema);
        assert_eq!(stopped.annotations, open.annotations);
    }

    #[test]
    fn a_next_step_names_a_tool_the_callers_toolset_lists() {
        let registry = ActionRegistry::kernel_only();
        let answer = |action: &str| Response {
            ok: true,
            action: "tool.call".to_owned(),
            result: Some(json!({"run_id": "r_1", "status": "failed"})),
            error: None,
            warnings: Vec::new(),
            next: Some(Next::new(
                action,
                json!({"run_id": "r_1"}),
                "to see what happened",
            )),
            exit: ExitClass::Failed,
            decision: None,
        };
        // `trace.show` is not on the `use` list: it is reached through
        // `a2m_action_run`.
        let rendered = render(&answer("trace.show"), &registry, BuiltInToolset::Use);
        let next = &rendered.face.structured.unwrap()["next"];
        assert_eq!(next["tool"], "a2m_action_run", "{next}");
        assert_eq!(next["args"]["action"], "trace.show", "{next}");
        // `run.get` is, and is named as itself.
        let rendered = render(&answer("run.get"), &registry, BuiltInToolset::Use);
        assert_eq!(
            rendered.face.structured.unwrap()["next"]["tool"],
            "a2m_run_get"
        );
    }
}
