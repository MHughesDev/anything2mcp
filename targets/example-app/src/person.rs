//! Stands for a person using the application in a window of the engine's own
//! browser that the engine watches and never drives (decision 0392, point 1).
//!
//! A test sets a walk through the control interface, `POST
//! /__control/person/walk`: a list of steps. While one is set, every page the
//! application serves carries a script of its own that takes the steps in
//! order, across pages, as a person would: it goes to a page by its address,
//! waits until the page shows a text, lingers, types into a form and sends
//! it, or sends a request as a page's own script does. Where it stands is
//! kept in the tab's own session storage, so the walk goes on when a page
//! loads. Nothing of the engine types, presses, or navigates there. `DELETE
//! /__control/person/walk` ends it, and so does a reset.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::AppState;

/// The longest page the walk's script is added to, in bytes.
const LONGEST_PAGE: usize = 4 * 1024 * 1024;

/// One step of a person's walk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum WalkStep {
    /// Go to a path of the application, as a person following a link does.
    Go(String),
    /// Wait until the element with this id shows exactly this text.
    WaitFor {
        /// The element's id.
        id: String,
        /// The text it shows.
        text: String,
    },
    /// Look at the page for this long, in milliseconds, as a person reads it.
    Linger(u64),
    /// Type a text into a field of a form.
    Type {
        /// The form's id.
        form: String,
        /// The field's name.
        field: String,
        /// What is typed.
        text: String,
    },
    /// Send the form with this id, as its button would.
    Submit(String),
    /// Send a JSON body with `POST` to a path, as a page's own script does,
    /// and wait for the answer.
    PostJson {
        /// The path.
        to: String,
        /// The body, as JSON.
        body: serde_json::Value,
    },
}

/// The script that takes the steps. Each step is taken once: the place is
/// moved on before the step is taken, so a page that loads goes on from
/// the next.
fn script(steps: &[WalkStep]) -> String {
    let steps = serde_json::to_string(steps)
        .unwrap_or_else(|_| "[]".to_owned())
        .replace('<', "\\u003c");
    format!(
        "<script>\n(() => {{\n\
         const steps = {steps};\n\
         const key = \"example-person-walk\";\n\
         const seen = (id, text) => {{ const element = document.getElementById(id); return element !== null && element.textContent === text; }};\n\
         const until = (id, text) => new Promise((done) => {{ if (seen(id, text)) {{ done(); return; }}\n\
           const watcher = new MutationObserver(() => {{ if (seen(id, text)) {{ watcher.disconnect(); done(); }} }});\n\
           watcher.observe(document, {{subtree: true, childList: true, characterData: true, attributes: true}}); }});\n\
         const walk = async () => {{\n\
           let at = Number(sessionStorage.getItem(key) || \"0\");\n\
           while (at < steps.length) {{\n\
             const step = steps[at];\n\
             at += 1;\n\
             sessionStorage.setItem(key, String(at));\n\
             if (step.go !== undefined) {{ location.href = step.go; return; }}\n\
             if (step.wait_for !== undefined) {{ await until(step.wait_for.id, step.wait_for.text); }}\n\
             if (step.linger !== undefined) {{ await new Promise((done) => setTimeout(done, step.linger)); }}\n\
             if (step.type !== undefined) {{ document.getElementById(step.type.form).elements[step.type.field].value = step.type.text; }}\n\
             if (step.submit !== undefined) {{ document.getElementById(step.submit).requestSubmit(); }}\n\
             if (step.post_json !== undefined) {{\n\
               await fetch(step.post_json.to, {{method: \"POST\", headers: {{\"Content-Type\": \"application/json\"}}, body: JSON.stringify(step.post_json.body)}}).catch(() => null);\n\
             }}\n\
           }}\n\
         }};\n\
         if (document.readyState === \"loading\") {{ addEventListener(\"DOMContentLoaded\", walk); }} else {{ walk(); }}\n\
         }})();\n</script>\n"
    )
}

/// Sets the walk every page then carries.
pub(crate) async fn set_walk(
    State(state): State<AppState>,
    axum::Json(steps): axum::Json<Vec<WalkStep>>,
) -> StatusCode {
    state.lock().await.walk = Some(steps);
    StatusCode::NO_CONTENT
}

/// Ends the walk: no page carries it after this.
pub(crate) async fn end_walk(State(state): State<AppState>) -> StatusCode {
    state.lock().await.walk = None;
    StatusCode::NO_CONTENT
}

/// Adds the walk's script to each page the application serves while a walk
/// is set. Anything that is not a page is left as it is.
pub(crate) async fn walks(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let Some(steps) = state.lock().await.walk.clone() else {
        return response;
    };
    let is_page = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));
    if !is_page {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, LONGEST_PAGE).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut page = String::from_utf8_lossy(&bytes).into_owned();
    let walk = script(&steps);
    match page.rfind("</body>") {
        Some(at) => page.insert_str(at, &walk),
        None => page.push_str(&walk),
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(page))
}
