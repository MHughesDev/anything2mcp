//! Reaching any action without listing it, and the built-in reference.

use a2m_core::{Failure, FailureCode};
use serde_json::{Value as Json, json};

use super::handler;
use crate::codes;
use crate::def::{ActionDef, Effect};
use crate::dispatch::{Next, Output, Request};
use crate::faces;

fn words_match(def: &ActionDef, query: &str) -> bool {
    let hay = format!("{} {}", def.name, def.summary).to_lowercase();
    query
        .split_whitespace()
        .all(|word| hay.contains(&word.to_lowercase()))
}

handler!(
    /// `action.search`: names and one-line summaries only.
    action_search,
    |kernel, call| {
        let query = call.text("query").unwrap_or("");
        let found: Vec<Json> = kernel
            .actions()
            .iter()
            .filter(|def| words_match(def, query))
            .map(|def| {
                json!({
                    "name": def.name.as_str(),
                    "summary": def.summary,
                    "built": def.built(),
                })
            })
            .collect();
        let total = found.len();
        Ok(
            Output::ok(json!({"actions": found, "total": total})).then(Next::new(
                "action.describe",
                json!({"action": "<name>"}),
                "to read one action's schema",
            )),
        )
    }
);

fn unknown_action(kernel: &crate::kernel::Kernel, name: &str) -> Failure {
    let near = kernel.actions().nearest(name);
    codes::input(
        codes::NOT_FOUND,
        format!("There is no action named `{}`.", codes::shown(name)),
        if near.is_empty() {
            "Call `action.search` to list the actions.".to_owned()
        } else {
            format!("Use one of: {}.", near.join(", "))
        },
    )
}

/// One action in full, as `action.describe` and `docs.get` show it.
fn described(def: &ActionDef) -> Json {
    let command = faces::command_faces_of(def);
    json!({
        "name": def.name.as_str(),
        "summary": def.summary,
        "shape": def.shape.as_str(),
        "effect": match def.effect {
            Effect::Fixed(effect) => effect.to_string(),
            Effect::OfTool => "the effect of the tool called".to_owned(),
        },
        "who": def.who.as_str(),
        "listed": def.listed.as_str(),
        "long_running": def.long_running,
        "built": def.built(),
        "command": command.usage,
        "tool": faces::tool_name(def),
        "route": format!("POST {}/actions/{}", faces::REST_PREFIX, def.name),
        "input_schema": def.input.schema.to_json(),
        "output_schema": def.output.to_json(),
        "examples": def
            .examples
            .iter()
            .filter_map(|text| serde_json::from_str::<Json>(text).ok())
            .collect::<Vec<_>>(),
        "fields": def.input.fields.iter().map(|field| json!({
            "name": field.name,
            "type": field.ty.as_str(),
            "required": field.required,
            "help": field.help,
        })).collect::<Vec<_>>(),
    })
}

handler!(
    /// `action.describe`: one action in full.
    action_describe,
    |kernel, call| {
        let name = call.required("action")?;
        let def = kernel
            .actions()
            .find(name)
            .ok_or_else(|| unknown_action(kernel, name))?;
        Ok(Output::ok(described(def)).then(Next::new(
            "action.run",
            json!({"action": def.name.as_str(), "input": {}}),
            "to run it",
        )))
    }
);

handler!(
    /// `action.run`: any action by name. The named action goes through the
    /// whole dispatcher again: its own schema, role, decision, and person
    /// need apply, and a second decision is recorded for it.
    action_run,
    |kernel, call| {
        let name = call.required("action")?.to_owned();
        if name == "action.run" {
            return Err(codes::input(
                codes::SCHEMA_INVALID,
                "`action.run` cannot run itself.",
                "Name the action you want to run.",
            ));
        }
        let input = call.input.get("input").cloned().unwrap_or(Json::Null);
        let response = kernel
            .dispatch(Request {
                action: name,
                input,
                caller: call.caller,
                idempotency_key: call.idempotency_key,
                project: call.project,
            })
            .await;
        match (response.error, response.result) {
            (Some(error), None) => Err(Failure::new(
                error.class,
                FailureCode::new(&error.code).unwrap_or(FailureCode::SCHEMA_INVALID),
                error.message,
                error.fix,
            )),
            (error, result) => Ok(Output {
                result: result.unwrap_or(Json::Null),
                warnings: response.warnings,
                next: response.next,
                exit: response.exit,
                failure: error.map(|error| {
                    Failure::new(
                        error.class,
                        FailureCode::new(&error.code).unwrap_or(FailureCode::SCHEMA_INVALID),
                        error.message,
                        error.fix,
                    )
                }),
            }),
        }
    }
);

/// The built-in topics beside the actions themselves.
const TOPICS: &[(&str, &str)] = &[
    (
        "actions",
        "Everything a person, an agent, or a program can ask for is an action, named `noun.verb`. \
Its command is `anything2mcp <noun> <verb>`, its MCP tool `a2m_<noun>_<verb>`, its route \
`POST /api/v1/actions/<noun>.<verb>`, and its SDK method `client.<noun>.<verb>()`. All four \
call one dispatcher, so a result is the same whichever way it was asked for. `action.search` \
lists them; `action.describe` gives one in full; `docs.get` with an action's name gives its \
reference page.",
    ),
    (
        "output",
        "Every response is one JSON object: `ok`, `action`, then `result`, `warnings`, and `next`, \
or `error` with `class`, `code`, `message`, `fix`, and `retry_safe`. `next` names the most \
useful following action. Exit codes: 0 succeeded, 1 the action failed, 2 the request was \
malformed, 3 a person is needed, 4 the engine could not be reached.",
    ),
    (
        "failures",
        "A failure carries a class, a stable code, a plain sentence, and a next step. The classes: \
policy, input, session_expired, edge_blocked, drift, unknown_variant, rejected, mismatch, \
possible_commit, stale, transient, busy. Only `possible_commit` means a write may have gone \
through: resume the run, which reconciles first, and never call again.",
    ),
    (
        "approvals",
        "A write rests at `needs_approval` until a person approves the exact diff, on a surface \
no caller can reach. No action, tool, command, or route approves. After the approval, call \
`run.resume` with the run's identifier.",
    ),
    (
        "settings",
        "Settings come from eight layers, lowest first: built-in defaults, the profile, a preset, \
the installation's file, the user's file, the project's `anything2mcp.toml`, `A2M_<GROUP>__<KEY>` \
variables, and a command's flags. The upper four may tighten a protected setting and never \
loosen it. An unknown key is an error. `config.show` prints every value and the layer that \
set it.",
    ),
];

handler!(
    /// `docs.get`: a reference page, generated from the registry and the
    /// topics above. Offline, and only when asked for.
    docs_get,
    |kernel, call| {
        let mut topics: Vec<String> = TOPICS.iter().map(|(name, _)| (*name).to_owned()).collect();
        topics.extend(kernel.actions().iter().map(|def| def.name.to_string()));
        let Some(topic) = call.text("topic") else {
            return Ok(Output::ok(json!({
                "topic": "",
                "text": "Pass a topic. Every action's name is a topic, and so are the names listed here.",
                "topics": topics,
            })));
        };
        if let Some((name, text)) = TOPICS.iter().find(|(name, _)| *name == topic) {
            return Ok(Output::ok(
                json!({"topic": name, "text": text, "topics": topics}),
            ));
        }
        let Some(def) = kernel.actions().find(topic) else {
            let near = codes::nearest(topic, topics.iter().map(String::as_str));
            return Err(codes::input(
                codes::NOT_FOUND,
                format!(
                    "There is no reference page named `{}`.",
                    codes::shown(topic)
                ),
                if near.is_empty() {
                    "Call `docs.get` with no topic to list the pages.".to_owned()
                } else {
                    format!("Use one of: {}.", near.join(", "))
                },
            ));
        };
        let command = faces::command_faces_of(def);
        let mut text = format!(
            "{}\n\n{}\n\nCommand: {}\n",
            def.name, def.summary, command.usage
        );
        text.push_str(&format!("MCP tool: {}\n", faces::tool_name(def)));
        text.push_str(&format!(
            "Who: {}. Effect: {}. {}\n",
            def.who.as_str(),
            match def.effect {
                Effect::Fixed(effect) => effect.to_string(),
                Effect::OfTool => "that of the tool called".to_owned(),
            },
            if def.built() {
                "Built."
            } else {
                "Not built yet: a call answers `action_not_built`."
            }
        ));
        if !def.input.fields.is_empty() {
            text.push_str("\nFields:\n");
            for field in &def.input.fields {
                text.push_str(&format!(
                    "  {} ({}{}): {}\n",
                    field.name,
                    field.ty.as_str(),
                    if field.required { ", required" } else { "" },
                    field.help
                ));
            }
        }
        for example in &def.examples {
            text.push_str(&format!("\nExample input: {example}\n"));
        }
        Ok(Output::ok(
            json!({"topic": def.name.as_str(), "text": text, "topics": topics}),
        ))
    }
);
