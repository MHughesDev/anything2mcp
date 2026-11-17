//! Tool lists and tool calls, as data and dispatches.
//!
//! Nothing here knows the protocol's types. A list is built at the moment
//! it is asked for: the MCP faces of the actions the toolset lists, then
//! the targets' published tools, read through the actions `tool.search`
//! and `tool.describe`. A call is one dispatch of the action a name
//! stands for. Every one of these goes through the dispatcher as the
//! caller, so this crate reads nothing the caller may not.

use std::sync::atomic::{AtomicU64, Ordering};

use a2m_actions::{
    BuiltInToolset, Caller, ErrorBody, Kernel, McpResultFace, Request, Response,
    ToolAnnotationsFace, mcp_tools,
};
use a2m_core::{FailureClass, OperationName};
use a2m_policy::ToolsetName;
use serde_json::{Map, Value as Json, json};

use crate::settings::{Listing, McpSettings};

/// How many tools one `tool.search` is asked for.
const PAGE: usize = 50;

/// The key of the following call in a result's `_meta`.
pub(crate) const NEXT_KEY: &str = "anything2mcp/next";
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
    /// No tool of that name is on this endpoint.
    Unknown,
    /// The dispatcher's answer.
    Answered(Box<Rendered>),
}

/// A response as an MCP result carries it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rendered {
    pub(crate) face: McpResultFace,
    /// The following call and the warnings, for the result's `_meta`.
    pub(crate) meta: Map<String, Json>,
}

impl Core {
    async fn dispatch(&self, action: &str, input: Json, caller: &Caller) -> Response {
        self.dispatched.fetch_add(1, Ordering::Relaxed);
        self.kernel
            .dispatch(Request::new(action, input, caller.clone()))
            .await
    }

    /// The tools this endpoint lists: the same tools in the same order
    /// every time, for as long as nothing is published.
    ///
    /// # Errors
    ///
    /// The dispatcher's refusal of the search or of a description.
    pub(crate) async fn list(&self, caller: &Caller) -> Result<ToolList, ErrorBody> {
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
        let mut names = Vec::new();
        let mut past_limit = 0;
        let mut offset = 0;
        loop {
            let found = self
                .dispatch(
                    "tool.search",
                    json!({"limit": PAGE, "offset": offset}),
                    caller,
                )
                .await;
            let result = answered(found)?;
            let page = result["tools"].as_array().cloned().unwrap_or_default();
            for tool in &page {
                let listed = match self.settings.listing {
                    Listing::Default => tool["kind"] == "pipeline",
                    Listing::Full => true,
                };
                let Some(name) = tool["name"].as_str().filter(|_| listed) else {
                    continue;
                };
                if tools.len() + names.len() < limit {
                    names.push(name.to_owned());
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
        for name in names {
            let described = self
                .dispatch("tool.describe", json!({"tool": name}), caller)
                .await;
            let tool = answered(described)?;
            let hint = |key: &str| tool["annotations"][key].as_bool().unwrap_or(false);
            tools.push(ToolEntry {
                title: tool["title"].as_str().unwrap_or(&name).to_owned(),
                description: tool["description"].as_str().unwrap_or_default().to_owned(),
                input_schema: object(tool["input_schema"].clone()),
                annotations: ToolAnnotationsFace {
                    read_only: hint("read_only"),
                    // A tool whose description does not say is treated as
                    // one that may do what cannot be undone.
                    destructive: tool["annotations"]["destructive"].as_bool().unwrap_or(true),
                    idempotent: hint("idempotent"),
                    open_world: tool["annotations"]["open_world"].as_bool().unwrap_or(true),
                },
                name,
            });
        }
        Ok(ToolList { tools, past_limit })
    }

    /// Calls the tool `name` with `arguments`. A name of an action this
    /// toolset lists is that action; any other name is a published tool,
    /// run by `tool.call`. The input is validated by the dispatcher against
    /// the full schema, whatever a list showed.
    pub(crate) async fn call(
        &self,
        name: &str,
        arguments: Map<String, Json>,
        caller: &Caller,
    ) -> Called {
        let registry = self.kernel.actions();
        if let Some(def) = registry.by_tool_name(name) {
            // An action this toolset does not list is reached through
            // `a2m_action_run`, never by a name the list does not hold.
            if !registry
                .toolset(self.builtin)
                .iter()
                .any(|listed| listed.name == def.name)
            {
                return Called::Unknown;
            }
            let action = def.name.to_string();
            let response = self
                .dispatch(&action, Json::Object(arguments), caller)
                .await;
            return Called::Answered(Box::new(self.render(&response)));
        }
        if OperationName::new(name).is_err() {
            return Called::Unknown;
        }
        // Whether such a tool is published is the dispatcher's to say. A
        // name it does not know is an unknown tool, which the protocol
        // answers differently from a call that failed.
        let described = self
            .dispatch("tool.describe", json!({"tool": name}), caller)
            .await;
        if let Some(error) = &described.error {
            if error.class == FailureClass::Input {
                return Called::Unknown;
            }
            return Called::Answered(Box::new(self.render(&described)));
        }
        let response = self
            .dispatch(
                "tool.call",
                json!({"tool": name, "input": Json::Object(arguments)}),
                caller,
            )
            .await;
        Called::Answered(Box::new(self.render(&response)))
    }

    /// The one rendering of a response: the registry's, with the following
    /// call and the warnings beside it. Structured content holds the
    /// result alone, because it must fit the tool's output schema.
    fn render(&self, response: &Response) -> Rendered {
        let mut meta = Map::new();
        if let Some(next) = &response.next {
            let tool = self
                .kernel
                .actions()
                .find(&next.action)
                .filter(|def| def.faces.mcp)
                .map_or_else(|| next.action.clone(), a2m_actions::tool_name);
            meta.insert(
                NEXT_KEY.to_owned(),
                json!({"tool": tool, "args": next.args, "when": next.when}),
            );
        }
        if !response.warnings.is_empty() {
            meta.insert(WARNINGS_KEY.to_owned(), json!(response.warnings));
        }
        Rendered {
            face: McpResultFace::of(response),
            meta,
        }
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
