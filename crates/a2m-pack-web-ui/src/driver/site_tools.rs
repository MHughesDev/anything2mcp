//! The `call_tool` step of the `site_tools` mode: a tool the page registered
//! for agents, called by its name from the acting tab.
//!
//! A tool runs the page's own script. What it does is whatever the script
//! does, and what the page says of it (its description, its schema, its
//! hints) is the page's own claim. The step therefore calls only a tool whose
//! name and digest are the ones the binding holds, sends only the arguments
//! the binding gives, in the types its schema names, and returns the answer
//! as text for an output the operation's author declared untrusted. A tool
//! that is gone, that has changed, or that answers with an error is a stop
//! with a name; a call whose answer does not arrive is a write whose end is
//! not known.

use a2m_core::{Step, Value};
use a2m_pack_web_browser::{BrowserError, Deadline, SiteTool};
use a2m_provider::{Observed, StepFailure, StepInput, StepOutcome};
use a2m_telemetry::Detail;

use super::{Session, acting, browser_failure, codes, fail, until};
use crate::codes::shown;
use crate::lower::TOOL_ARG;
use crate::model::{CallToolStep, TabKind};

use std::time::Duration;

/// How long the page is given to register a tool it has not announced.
const OFFERED_WITHIN: Duration = Duration::from_secs(5);
/// The most text a tool's answer may hold.
const MOST_ANSWER: usize = 256 * 1024;

/// An argument as JSON: the form a tool's input takes.
fn json_of(value: &Value) -> Option<serde_json::Value> {
    Some(match value {
        Value::Text(text) => serde_json::Value::String(text.clone()),
        Value::Int(number) => serde_json::Value::from(*number),
        Value::Bool(flag) => serde_json::Value::Bool(*flag),
        // An exact number travels as its text, never as a float.
        Value::Decimal(number) => serde_json::Value::String(number.to_string()),
        _ => return None,
    })
}

/// Holds the arguments to the tool's own schema where it states one: no
/// name the schema does not list, every name it requires, and each in the
/// type it names.
fn held_to_schema(
    tool: &SiteTool,
    given: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), StepFailure> {
    let Ok(schema) = serde_json::from_str::<serde_json::Value>(&tool.input_schema) else {
        return Ok(());
    };
    if let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) {
        for (name, value) in given {
            let Some(property) = properties.get(name) else {
                return Err(codes::not_usable(format!(
                    "the tool does not list an argument named `{}`",
                    shown(name)
                )));
            };
            let fits = match property.get("type").and_then(|value| value.as_str()) {
                Some("string") => value.is_string(),
                Some("integer") => value.is_i64() || value.is_u64(),
                Some("number") => value.is_number(),
                Some("boolean") => value.is_boolean(),
                _ => true,
            };
            if !fits {
                return Err(codes::not_usable(format!(
                    "the argument `{}` is not of the type the tool's schema names",
                    shown(name)
                )));
            }
        }
    }
    for needed in schema
        .get("required")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
    {
        if !given.contains_key(needed) {
            return Err(codes::not_usable(format!(
                "the tool requires an argument named `{}`, which the binding does not give",
                shown(needed)
            )));
        }
    }
    Ok(())
}

/// The text of a tool's answer: the text of its content when it answers in
/// the form of a tool of a server, and what it said otherwise.
fn text_of(output: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(output) else {
        return output.to_owned();
    };
    let texts: Vec<&str> = value
        .get("content")
        .and_then(|content| content.as_array())
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
        .collect();
    if texts.is_empty() {
        output.to_owned()
    } else {
        texts.join("\n")
    }
}

impl Session {
    pub(super) async fn call_tool(
        &self,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        self.may_act(step)?;
        let payload: CallToolStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let acting_tab = self
            .acting
            .as_ref()
            .filter(|_| self.on == TabKind::Act)
            .ok_or_else(|| codes::not_usable("a tool is called only from a tab that may act"))?;
        let deadline = until(step, input);
        let what = format!("the tool `{}`", shown(&payload.tool));

        // The tool the page offers, which the page may register a moment
        // after it loads, is the one the binding was written for.
        let soon = Deadline::after(deadline.remaining().min(OFFERED_WITHIN));
        let tool = match acting_tab.site_tool(&payload.tool, soon).await {
            Ok(tool) => tool,
            Err(BrowserError::NotFound)
                if !acting_tab
                    .site_tools_possible(deadline)
                    .await
                    .unwrap_or(true) =>
            {
                return Err(StepFailure::not_sent(fail(
                    &codes::row::SITE_TOOLS_UNAVAILABLE,
                    "The page has no place to register tools (`document.modelContext` is not there): the browser is of a version that does not offer it, or the page is not a secure context.".to_owned(),
                    codes::row::SITE_TOOLS_UNAVAILABLE.next,
                )));
            }
            Err(BrowserError::NotFound) => {
                return Err(StepFailure::not_sent(fail(
                    &codes::row::SITE_TOOL_NOT_OFFERED,
                    format!(
                        "The page registered no tool named `{}`.",
                        shown(&payload.tool)
                    ),
                    codes::row::SITE_TOOL_NOT_OFFERED.next,
                )));
            }
            Err(BrowserError::Unexpected(why)) => {
                return Err(StepFailure::not_sent(fail(
                    &codes::row::SITE_TOOLS_UNAVAILABLE,
                    format!(
                        "The browser cannot list the tools of a page: {}.",
                        shown(&why)
                    ),
                    codes::row::SITE_TOOLS_UNAVAILABLE.next,
                )));
            }
            Err(error) => return Err(browser_failure(&error, &what)),
        };
        if tool.digest.to_string() != payload.digest {
            return Err(StepFailure::not_sent(fail(
                &codes::row::SITE_TOOL_CHANGED,
                format!(
                    "The page's tool `{}` is not the one the binding was written for: its digest is {}, and the binding holds {}.",
                    shown(&payload.tool),
                    tool.digest,
                    shown(&payload.digest)
                ),
                codes::row::SITE_TOOL_CHANGED.next,
            )));
        }

        // The arguments, as the binding gave them.
        let mut given = serde_json::Map::new();
        for name in &payload.args {
            let value = input
                .args
                .get(&format!("{TOOL_ARG}{name}"))
                .and_then(json_of)
                .ok_or_else(|| {
                    codes::not_usable(format!(
                        "the argument `{}` has no usable value",
                        shown(name)
                    ))
                })?;
            given.insert(name.clone(), value);
        }
        held_to_schema(&tool, &given)?;
        let text = serde_json::Value::Object(given).to_string();

        let answer = acting_tab
            .call_site_tool(&payload.tool, &text, deadline)
            .await
            .map_err(|error| browser_failure(&error, &what))?;
        if answer.status != "Completed" {
            return Err(StepFailure::unknown(fail(
                &a2m_provider::codes::row::NO_CONFIRMATION,
                format!(
                    "The call of {what} ended with the status `{}`: {}.",
                    shown(&answer.status),
                    shown(&answer.error_text)
                ),
                "Reconcile the record: read it back before anything is written again.",
            )));
        }
        let result = text_of(&answer.output);
        if result.len() > MOST_ANSWER {
            return Err(StepFailure::unknown(fail(
                &codes::row::READ_TOO_LARGE,
                format!("The answer of {what} is larger than the limit."),
                codes::row::READ_TOO_LARGE.next,
            )));
        }
        let mut observed = Observed::default();
        if let Some(name) = payload.result {
            observed.values.insert(name, Value::Text(result));
        }
        observed.captures.push(acting(
            vec![
                Detail::new("step", "call_tool"),
                Detail::new("tool", payload.tool.as_str()),
                Detail::new("digest", tool.digest.to_string()),
                Detail::new("status", answer.status.as_str()),
            ],
            Vec::new(),
        ));
        Ok(StepOutcome::Done(observed))
    }
}
