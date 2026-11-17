//! The output contract, applied to every response the same way: one JSON
//! object on standard output for a program, readable text for a person at
//! a terminal, the failure with its class, code, message, and next step on
//! standard error, and the exit code from the response's exit class alone.
//!
//! Nothing here looks at what a result holds: text is the same JSON, laid
//! out as indented lines.
//!
//! Exit code 4, the engine could not be reached or started, is not raised
//! in this build: every command runs the engine in its own process.

use a2m_actions::{ErrorBody, ExitClass, PROGRAM, Response};
use a2m_core::Failure;
use serde_json::Value as Json;

use super::Finished;
use super::parse::{Tree, command_line};

/// A response for a failure met before any action was dispatched.
pub(crate) fn refused(action: &str, failure: &Failure, exit: ExitClass) -> Response {
    Response {
        ok: false,
        action: action.to_owned(),
        result: None,
        error: Some(ErrorBody {
            class: failure.class(),
            code: failure.code().to_string(),
            message: failure.message().to_owned(),
            fix: failure.next_step().to_owned(),
            retry_safe: true,
        }),
        warnings: Vec::new(),
        next: None,
        exit,
        decision: None,
    }
}

fn scalar(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        Json::Null => "none".to_owned(),
        other => other.to_string(),
    }
}

fn is_scalar(value: &Json) -> bool {
    match value {
        Json::Object(map) => map.is_empty(),
        Json::Array(items) => items.is_empty(),
        _ => true,
    }
}

fn empty(value: &Json) -> &'static str {
    match value {
        Json::Object(_) => "{}",
        Json::Array(_) => "[]",
        _ => "",
    }
}

/// Lays a JSON value out as indented lines: `name: value` for a field, a
/// dash for an item of a list.
fn lines(value: &Json, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match value {
        Json::Object(map) if !map.is_empty() => {
            for (name, value) in map {
                if is_scalar(value) {
                    out.push_str(&format!("{pad}{name}: {}\n", shown(value, indent + 2)));
                } else {
                    out.push_str(&format!("{pad}{name}:\n"));
                    lines(value, indent + 2, out);
                }
            }
        }
        Json::Array(items) if !items.is_empty() => {
            for item in items {
                if is_scalar(item) {
                    out.push_str(&format!("{pad}- {}\n", shown(item, indent + 2)));
                } else {
                    out.push_str(&format!("{pad}-\n"));
                    lines(item, indent + 2, out);
                }
            }
        }
        other => out.push_str(&format!("{pad}{}\n", shown(other, indent + 2))),
    }
}

/// A value on one line; text of several lines continues indented.
fn shown(value: &Json, indent: usize) -> String {
    let text = match value {
        Json::Object(_) | Json::Array(_) => empty(value).to_owned(),
        other => scalar(other),
    };
    let pad = " ".repeat(indent);
    text.trim_end()
        .lines()
        .collect::<Vec<_>>()
        .join(&format!("\n{pad}"))
}

/// A failure as a person reads it, on standard error.
fn failure_text(error: &ErrorBody) -> String {
    format!(
        "{PROGRAM}: {}\n  class: {}\n  code: {}\n  fix: {}\n",
        error.message, error.class, error.code, error.fix
    )
}

/// Renders one response.
pub(crate) fn render(response: &Response, tree: &Tree, json: bool) -> Finished {
    let mut stdout = String::new();
    let mut stderr = String::new();
    if json {
        stdout.push_str(&response.to_json().to_string());
        stdout.push('\n');
    } else {
        if let Some(result) = &response.result {
            lines(result, 0, &mut stdout);
        }
        if let Some(next) = &response.next {
            let line =
                command_line(tree, &next.action, &next.args).unwrap_or_else(|| next.action.clone());
            stdout.push_str(&format!("\nnext, {}:\n  {line}\n", next.when));
        }
    }
    for warning in &response.warnings {
        stderr.push_str(&format!("warning: {warning}\n"));
    }
    if let Some(error) = &response.error {
        stderr.push_str(&failure_text(error));
    }
    Finished {
        code: response.exit_code(),
        stdout,
        stderr,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::lines;

    #[test]
    fn a_result_is_laid_out_as_indented_lines() {
        let mut out = String::new();
        lines(
            &json!({
                "target": "example",
                "ok": true,
                "problems": [],
                "tools": ["example_order_get"],
                "nested": {"count": 2, "rows": [{"code": "L-1"}]},
                "text": "first\nsecond",
                "nothing": null,
            }),
            0,
            &mut out,
        );
        assert_eq!(
            out,
            "nested:\n  count: 2\n  rows:\n    -\n      code: L-1\nnothing: none\nok: true\nproblems: []\ntarget: example\ntext: first\n  second\ntools:\n  - example_order_get\n"
        );
    }
}
