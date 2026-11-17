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

/// The built-in topics beside the actions and the guides.
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
        "settings",
        "Settings come from eight layers, lowest first: built-in defaults, the profile, a preset, \
the installation's file, the user's file, the project's `anything2mcp.toml`, `A2M_<GROUP>__<KEY>` \
variables, and a command's flags. The upper four may tighten a protected setting and never \
loosen it. An unknown key is an error. `config.show` prints every value and the layer that \
set it.",
    ),
];

/// The built-in guides, as written in `guides/`: a first line
/// `name: <name>`, a blank line, then the page.
const GUIDE_FILES: &[&str] = &[
    include_str!("../../guides/guidance.md"),
    include_str!("../../guides/target-files.md"),
    include_str!("../../guides/operations.md"),
    include_str!("../../guides/identity.md"),
    include_str!("../../guides/sources.md"),
    include_str!("../../guides/sessions.md"),
    include_str!("../../guides/approvals.md"),
    include_str!("../../guides/failures.md"),
    include_str!("../../guides/pack-web.md"),
    include_str!("../../guides/pack-http.md"),
    include_str!("../../guides/pack-mcp.md"),
    include_str!("../../guides/pack-command.md"),
    include_str!("../../guides/pack-data.md"),
    include_str!("../../guides/pack-file.md"),
    include_str!("../../guides/pack-stream.md"),
    include_str!("../../guides/pack-device.md"),
    include_str!("../../guides/pack-desktop.md"),
];

/// One guide: its stable name and its page. `None` for a file whose first
/// line is not `name: <name>` followed by a blank line; the test holds
/// every file to that.
pub(crate) fn guide(file: &str) -> Option<(&str, &str)> {
    let (first, rest) = file.split_once('\n')?;
    let name = first.trim_end_matches('\r').strip_prefix("name: ")?;
    let page = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    (!name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
    .then_some((name, page))
}

/// Every guide, in the order of [`GUIDE_FILES`].
pub(crate) fn guides() -> impl Iterator<Item = (&'static str, &'static str)> {
    GUIDE_FILES.iter().filter_map(|file| guide(file))
}

/// The text in backticks on `line`, in order.
pub(crate) fn quoted(line: &str) -> impl Iterator<Item = &str> {
    line.split('`').skip(1).step_by(2)
}

/// What a guide says, with a line at its end for each action it names:
/// whether that action is built, read from the registry as the page is
/// asked for, so that a page never sends a caller to an action that
/// answers `action_not_built` without saying so.
fn guide_text(registry: &crate::registry::ActionRegistry, page: &str) -> String {
    let mut named: Vec<&ActionDef> = Vec::new();
    for word in page.lines().flat_map(quoted) {
        if let Some(def) = registry.find(word)
            && !named.iter().any(|seen| seen.name == def.name)
        {
            named.push(def);
        }
    }
    let mut text = page.trim_end().to_owned();
    if !named.is_empty() {
        text.push_str("\n\n## The actions this page names\n\n");
        for def in named {
            text.push_str(&format!(
                "- `{}`: {}\n",
                def.name,
                if def.built() {
                    "built."
                } else {
                    "not built yet: a call answers `action_not_built`."
                }
            ));
        }
    }
    text
}

handler!(
    /// `docs.get`: a reference page: a guide, one of the topics above, or
    /// an action's page, generated from the registry. Offline, and only
    /// when asked for.
    docs_get,
    |kernel, call| {
        let mut topics: Vec<String> = guides().map(|(name, _)| name.to_owned()).collect();
        topics.extend(TOPICS.iter().map(|(name, _)| (*name).to_owned()));
        topics.extend(kernel.actions().iter().map(|def| def.name.to_string()));
        let Some(topic) = call.text("topic") else {
            return Ok(Output::ok(json!({
                "topic": "",
                "text": "Pass a topic. Start with `guidance`. Every action's name is a topic, and so are the guides and the other names listed here.",
                "topics": topics,
            })));
        };
        if let Some((name, page)) = guides().find(|(name, _)| *name == topic) {
            let text = guide_text(kernel.actions(), page);
            return Ok(Output::ok(
                json!({"topic": name, "text": text, "topics": topics}),
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::{GUIDE_FILES, TOPICS, guide, guide_text, guides, quoted};
    use crate::registry::ActionRegistry;

    #[test]
    fn every_guide_is_named_on_its_first_line_once_and_every_page_docs_get_is_sent_to_exists() {
        // Every file in the folder is served, each under a name of its own.
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("guides");
        let on_disk = std::fs::read_dir(&folder)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "md")
            })
            .count();
        assert_eq!(
            on_disk,
            GUIDE_FILES.len(),
            "a guide in {folder:?} is not served"
        );
        let mut names = Vec::new();
        for file in GUIDE_FILES {
            let first = file.lines().next().unwrap_or("");
            let (name, page) = guide(file).unwrap_or_else(|| panic!("not `name: <name>`: {first}"));
            assert_eq!(first.trim_end_matches('\r'), format!("name: {name}"));
            assert!(!page.trim().is_empty(), "{name}");
            assert!(!names.contains(&name), "two guides are named {name}");
            assert!(TOPICS.iter().all(|(topic, _)| *topic != name), "{name}");
            names.push(name);
        }

        let registry = ActionRegistry::kernel_only();
        let exists = |topic: &str| {
            names.contains(&topic)
                || TOPICS.iter().any(|(name, _)| *name == topic)
                || registry.find(topic).is_some()
        };
        let mut sent = 0;
        for (name, page) in guides() {
            for line in page.lines() {
                // A page names what to ask `docs.get` for after it, on the
                // same line.
                let Some((_, after)) = line.split_once("`docs.get`") else {
                    continue;
                };
                for topic in quoted(after).filter(|word| *word != "docs.get") {
                    sent += 1;
                    assert!(exists(topic), "{name} sends `docs.get` to `{topic}`");
                }
            }
        }
        assert!(sent > 10, "{sent}");
    }

    #[test]
    fn a_guide_says_of_each_action_it_names_whether_it_is_built() {
        let registry = ActionRegistry::kernel_only();
        let page = "Call `run.resume`, then `run.undo`, then `run.resume` again.";
        let text = guide_text(&registry, page);
        let built = |name: &str| registry.find(name).unwrap().built();
        assert!(built("run.resume") && !built("run.undo"));
        assert!(text.contains("- `run.resume`: built."), "{text}");
        assert!(
            text.contains("- `run.undo`: not built yet: a call answers `action_not_built`."),
            "{text}"
        );
        assert_eq!(text.matches("- `run.resume`").count(), 1, "{text}");
        // A page that names no action is served as written.
        assert_eq!(
            guide_text(&registry, "No action here.\n"),
            "No action here."
        );
    }
}
