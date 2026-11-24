//! Pages that commit a value as a family of web interface does, for the input
//! commit a platform profile states (decision 0399). Each tells the
//! application what it received, as a case event that `GET /__control/cases`
//! reports, so a test asks the application and never the driver. The page of
//! an input that keeps only what is typed into it, which a framework with
//! controlled inputs is like, is `/cases/typed-input` of [`crate::cases`].
//!
//! - `/cases/commit/plain`: a page the server draws whole. Its input
//!   "Reference" is taken when the control says it changed, which it says
//!   when it loses the focus after an edit: the page tells `changed` with the
//!   value. An edit alone tells nothing. "Save reference" tells `saved`.
//! - `/cases/commit/postback`: a page of a server-rendered forms framework.
//!   A change of "Quantity" is posted to the application, which records
//!   `posted` with the value and answers with the form drawn again; the page
//!   replaces its form with the answer, so the control is a new element.
//!   "Save quantity", outside the form, tells `saved` with what it holds.
//! - `/cases/commit/stuck`: the same form, whose change is posted and
//!   recorded, and which never draws the form again.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::AppState;
use crate::cases::{TELL, page};
use crate::pages::escape;

/// The most a posted value may be.
const MOST: usize = 1024;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/cases/commit/plain", get(plain))
        .route("/cases/commit/postback", get(postback).post(posted))
        .route("/cases/commit/stuck", get(stuck))
}

async fn plain() -> Html<String> {
    page(
        "Reference",
        &format!(
            "<h1>Reference</h1>\n<label for=\"ref\">Reference</label>\n\
             <input id=\"ref\" autocomplete=\"off\">\n\
             <button id=\"save\">Save reference</button>\n\
             <p id=\"status\" role=\"status\">Nothing changed.</p>\n\
             <script>\n{TELL}\
             const input = document.getElementById(\"ref\");\n\
             input.addEventListener(\"change\", () => {{\n\
               tell(\"changed\", input.value);\n\
               document.getElementById(\"status\").textContent = \"Changed to \" + input.value + \".\";\n\
             }});\n\
             document.getElementById(\"save\").addEventListener(\"click\", () => {{\n\
               tell(\"saved\", input.value);\n\
               document.getElementById(\"status\").textContent = \"Saved \" + input.value + \".\";\n\
             }});\n\
             </script>"
        ),
    )
}

/// The form of the post-back pages, holding `value`, as the application
/// draws it.
fn form(value: &str, said: &str) -> String {
    format!(
        "<label for=\"qty\">Quantity</label>\n\
         <input id=\"qty\" name=\"qty\" autocomplete=\"off\" value=\"{}\">\n\
         <p>{}</p>\n",
        escape(value),
        escape(said)
    )
}

/// A post-back page: a change of the form is posted, and, when `redraws`,
/// the form is replaced by the application's answer.
fn postback_page(redraws: bool) -> Html<String> {
    let then = if redraws {
        "form.innerHTML = await answer.text();\n"
    } else {
        ""
    };
    page(
        "Quantity",
        &format!(
            "<h1>Quantity</h1>\n<form id=\"order\">\n{}</form>\n\
             <button id=\"save\">Save quantity</button>\n\
             <p id=\"status\" role=\"status\">Nothing saved.</p>\n\
             <script>\n{TELL}\
             const form = document.getElementById(\"order\");\n\
             form.addEventListener(\"submit\", (event) => event.preventDefault());\n\
             form.addEventListener(\"change\", async (event) => {{\n\
               const answer = await fetch(\"/cases/commit/postback\", {{method: \"POST\", body: event.target.value}});\n\
               {then}\
             }});\n\
             document.getElementById(\"save\").addEventListener(\"click\", () => {{\n\
               const value = document.getElementById(\"qty\").value;\n\
               tell(\"saved\", value);\n\
               document.getElementById(\"status\").textContent = \"Saved \" + value + \".\";\n\
             }});\n\
             </script>",
            form("", "Nothing posted.")
        ),
    )
}

async fn postback() -> Html<String> {
    postback_page(true)
}

async fn stuck() -> Html<String> {
    postback_page(false)
}

/// A change posted back: recorded, and answered with the form drawn again.
async fn posted(State(state): State<AppState>, body: Body) -> Response {
    let Ok(bytes) = to_bytes(body, MOST).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let value = String::from_utf8_lossy(&bytes).into_owned();
    state
        .lock()
        .await
        .case_events
        .push(format!("posted: {value}"));
    Html(form(&value, &format!("Posted {value}."))).into_response()
}
