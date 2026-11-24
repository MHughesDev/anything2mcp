//! The tools a page publishes for an agent, through the browser's `WebMCP`
//! domain (research note 25): the tools the page registered, as the browser
//! announced them, and a call of one by its name.
//!
//! A tool is run by the page's own script, so what it does is whatever the
//! page's code does. Everything about it that the page says (its
//! description, its schema, its hints) is the page's claim and no more. The
//! engine binds to a tool by its name and the digest of its description and
//! schema; a name that is gone, or whose digest differs, is the crate
//! above's to stop on. Nothing here puts script of the engine's own in the
//! page.

use std::collections::BTreeMap;
use std::time::Duration;

use a2m_core::Digest;
use serde_json::Value;

use super::{ActTab, BrowserError, Deadline, Dispatch, Page};
use crate::protocol::{Evaluate, WebMcpCancel, WebMcpEnable, WebMcpInvoke};

/// The longest text of a tool's description or schema kept.
const MOST_TEXT: usize = 16 * 1024;
/// How many tools a page may announce before the rest are left out.
const MOST_TOOLS: usize = 256;
/// The longest an answer's text may be.
const MOST_ANSWER: usize = 1024 * 1024;

/// One tool a page registered, as the browser announced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteTool {
    /// The tool's name, which is all a plan binds to besides the digest.
    pub name: String,
    /// What the page says it does. The page's word, and untrusted text.
    pub description: String,
    /// The schema of its input, as the compact text the page's own
    /// JSON gave.
    pub input_schema: String,
    /// Whether the page claims the tool only reads. A claim, never evidence.
    pub read_only_claim: bool,
    /// The digest of the name, the description, and the schema.
    pub digest: Digest,
    /// The frame the tool was registered in, which a call names.
    frame_id: String,
}

impl SiteTool {
    fn from_announced(value: &Value) -> Option<Self> {
        let name = value.get("name")?.as_str()?.to_owned();
        let description = value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let input_schema = match value.get("inputSchema") {
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => String::new(),
        };
        if name.is_empty() || description.len() > MOST_TEXT || input_schema.len() > MOST_TEXT {
            return None;
        }
        let read_only_claim = match value.get("annotations") {
            Some(Value::Object(map)) => map
                .get("readOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            Some(Value::Array(list)) => list.iter().any(|item| item.as_str() == Some("readOnly")),
            _ => false,
        };
        let digest = digest_of(&name, &description, &input_schema);
        let frame_id = value
            .get("frameId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        Some(Self {
            name,
            description,
            input_schema,
            read_only_claim,
            digest,
            frame_id,
        })
    }
}

/// The digest a plan holds for a tool: the three texts, each with its
/// length, so that no two cuts of one text are the same.
#[must_use]
pub fn digest_of(name: &str, description: &str, input_schema: &str) -> Digest {
    let mut bytes = Vec::new();
    for part in [name, description, input_schema] {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }
    Digest::of(&bytes)
}

/// How a call of a tool ended, as the page said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteAnswer {
    /// The status the browser reported: `Completed`, `Error`, or `Canceled`.
    pub status: String,
    /// What the tool returned, as text. Untrusted, and bounded.
    pub output: String,
    /// What the browser says of an error, when it says anything.
    pub error_text: String,
}

/// What the tab holds of the page's tools.
#[derive(Default)]
pub(super) struct SiteState {
    /// Whether the domain was switched on for this tab.
    enabled: bool,
    tools: BTreeMap<String, SiteTool>,
    /// The answers of calls, by the browser's number for the call.
    answers: BTreeMap<String, SiteAnswer>,
}

impl Page {
    fn site(&self) -> std::sync::MutexGuard<'_, SiteState> {
        self.site
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The browser told of tools that the page registered.
    pub(super) fn tools_added(&self, params: &Value) {
        let mut site = self.site();
        for value in params
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if site.tools.len() >= MOST_TOOLS {
                break;
            }
            if let Some(tool) = SiteTool::from_announced(value) {
                site.tools.insert(tool.name.clone(), tool);
            }
        }
        drop(site);
        self.site_changes.send_modify(|count| *count += 1);
    }

    /// The browser told of tools that the page took away.
    pub(super) fn tools_removed(&self, params: &Value) {
        let mut site = self.site();
        for value in params
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                site.tools.remove(name);
            }
        }
        drop(site);
        self.site_changes.send_modify(|count| *count += 1);
    }

    /// The browser told how a call ended.
    pub(super) fn tool_responded(&self, params: &Value) {
        let Some(number) = params
            .get("invocationId")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return;
        };
        let text = |name: &str| match params.get(name) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        };
        let mut output = text("output");
        if output.len() > MOST_ANSWER {
            output.truncate(MOST_ANSWER);
        }
        let answer = SiteAnswer {
            status: text("status"),
            output: self.redact.redact(&output),
            error_text: self.redact.redact(&text("errorText")),
        };
        self.site().answers.insert(number, answer);
        self.site_changes.send_modify(|count| *count += 1);
    }

    /// Whether the page has a place to register tools: whether
    /// `document.modelContext` is there. A browser of a version that does
    /// not offer it, or one that offers it only to a trial, and a page that
    /// is not a secure context, have none; a page then registers nothing,
    /// and that is not the same as a page that registered no tool the call
    /// wants. A typeof, read in the page's own world and nothing else.
    async fn has_a_place_for_tools(&self, deadline: Deadline) -> Result<bool, BrowserError> {
        let answer = self
            .call(
                &Evaluate {
                    expression: "typeof document.modelContext",
                    context_id: None,
                    return_by_value: true,
                    await_promise: false,
                },
                deadline,
            )
            .await?;
        Ok(answer
            .result
            .read::<String>()
            .is_some_and(|kind| kind != "undefined"))
    }

    async fn enable_site_tools(&self, deadline: Deadline) -> Result<(), BrowserError> {
        if self.site().enabled {
            return Ok(());
        }
        self.call(&WebMcpEnable {}, deadline)
            .await
            .map_err(|error| match error {
                // A browser without the domain says so as a protocol error.
                BrowserError::Unexpected(why) => BrowserError::Unexpected(format!(
                    "the browser has no tools of pages to offer: {why}"
                )),
                other => other,
            })?;
        self.site().enabled = true;
        Ok(())
    }
}

impl ActTab {
    /// Whether the page has a place to register tools at all. When it has
    /// none, a tool that is not announced is not a tool the page dropped but
    /// a browser that cannot be told of one.
    ///
    /// # Errors
    ///
    /// As a call of the browser.
    pub async fn site_tools_possible(&self, deadline: Deadline) -> Result<bool, BrowserError> {
        self.page.has_a_place_for_tools(deadline).await
    }

    /// The tool the page registered under `name`, once the browser has
    /// announced it: the page may register it a moment after it loads.
    ///
    /// # Errors
    ///
    /// [`BrowserError::NotFound`] when no tool of that name was announced
    /// before the deadline, and [`BrowserError::Unexpected`] when the
    /// browser has no such domain. Nothing was sent to the page's tools.
    pub async fn site_tool(
        &self,
        name: &str,
        deadline: Deadline,
    ) -> Result<SiteTool, BrowserError> {
        let page = &self.page;
        page.enable_site_tools(deadline).await?;
        let mut changes = page.site_changes.subscribe();
        loop {
            if let Some(tool) = page.site().tools.get(name).cloned() {
                return Ok(tool);
            }
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        return Err(BrowserError::Disconnected { dispatch: Dispatch::NotSent });
                    }
                }
                () = tokio::time::sleep_until(deadline.at()) => {
                    return Err(BrowserError::NotFound);
                }
            }
        }
    }

    /// Calls the tool of that name with `input`, which is JSON text, and
    /// waits for the page's answer. Called once: a call whose answer does
    /// not arrive is cancelled and its end is not known.
    ///
    /// # Errors
    ///
    /// Before the call was sent, [`BrowserError::NotFound`] for a tool that
    /// is not there. After it, [`BrowserError::Timeout`] with
    /// [`Dispatch::Sent`] when the answer did not arrive, and the other
    /// errors of a call as the browser's link gives them.
    pub async fn call_site_tool(
        &self,
        name: &str,
        input: &str,
        deadline: Deadline,
    ) -> Result<SiteAnswer, BrowserError> {
        let page = &self.page;
        page.enable_site_tools(deadline).await?;
        let Some(frame_id) = page
            .site()
            .tools
            .get(name)
            .map(|tool| tool.frame_id.clone())
        else {
            return Err(BrowserError::NotFound);
        };
        let input: Value = serde_json::from_str(input)
            .map_err(|_| BrowserError::Unexpected("the input of a tool is not JSON".to_owned()))?;
        let mut changes = page.site_changes.subscribe();
        let invoked = page
            .call(
                &WebMcpInvoke {
                    frame_id: &frame_id,
                    tool_name: name,
                    input: &input,
                },
                deadline,
            )
            .await
            .map_err(|error| match error {
                BrowserError::Timeout { .. } => BrowserError::Timeout {
                    dispatch: Dispatch::Sent,
                },
                other => other,
            })?;
        let number = invoked.invocation_id;
        loop {
            if let Some(answer) = page.site().answers.remove(&number) {
                return Ok(answer);
            }
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        return Err(BrowserError::Disconnected { dispatch: Dispatch::Sent });
                    }
                }
                () = tokio::time::sleep_until(deadline.at()) => {
                    // The page may still be running it: it is cancelled,
                    // for a moment's wait, and its end is not known.
                    let cancel = Deadline::after(Duration::from_secs(2));
                    let _ = page.call(&WebMcpCancel { invocation_id: &number }, cancel).await;
                    return Err(BrowserError::Timeout { dispatch: Dispatch::Sent });
                }
            }
        }
    }
}
