//! The Phase 1 cases of the interface driver (cases 6 to 9, 11, and 12): pages
//! whose hard part is the page, with no data of their own. Each page tells the
//! application what happened to it by `POST /cases/events/{name}`, with a short
//! text; `GET /__control/cases` reports every event since the last reset, oldest
//! first, so a test asks the application what the page did and never the driver.
//!
//! - `/cases/rerender` (case 6): a list of rows, each with a button "Archive
//!   {name}". Every change draws the whole list again with new generated
//!   identifiers, so an identifier read before a change is gone after it.
//! - `/cases/typed-input` (case 7): an input that keeps what is typed into it
//!   only when real key presses made it. A value inserted as one edit, and a
//!   value set through the element's own setter, are cleared again.
//! - `/cases/in-page` (case 8): "Orders" and "Customers" change the page
//!   address with the history interface and show the new heading a moment
//!   later, with no new document. A `loaded` event is sent once per document.
//! - `/cases/grid` (case 9): ten thousand rows of which about fifteen exist in
//!   the page at a time; a box "Find row" jumps to the row of a key.
//! - `/cases/confirm` (case 11): the button "Archive order" asks "Archive this
//!   order?" in a dialog on the first and the third press, and not on the
//!   second, and every fourth press asks in the browser's own dialog.
//! - `/cases/frame` (case 12): a form inside a frame (`/cases/frame/inner`), and
//!   beside it a control inside an open shadow tree.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};

use crate::AppState;

/// The most an event's text may be.
const MOST: usize = 1024;
/// How many events the application keeps.
const KEPT: usize = 10_000;
/// How many rows the grid holds.
pub const GRID_ROWS: u32 = 10_000;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/cases/events/{name}", post(event))
        .route("/cases/rerender", get(rerender))
        .route("/cases/typed-input", get(typed_input))
        .route("/cases/in-page", get(in_page))
        .route("/cases/grid", get(grid))
        .route("/cases/confirm", get(confirm))
        .route("/cases/frame", get(frame))
        .route("/cases/frame/inner", get(frame_inner))
}

/// Every event the pages sent since the last reset, oldest first, as
/// `name: text`.
pub(crate) async fn report(State(state): State<AppState>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({"events": state.lock().await.case_events}))
}

async fn event(State(state): State<AppState>, Path(name): Path<String>, body: Body) -> Response {
    let Ok(bytes) = to_bytes(body, MOST).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let mut inner = state.lock().await;
    if inner.case_events.len() < KEPT {
        inner.case_events.push(format!("{name}: {text}"));
    }
    StatusCode::NO_CONTENT.into_response()
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{title} - Example Orders</title>\n</head>\n<body>\n<main>\n{body}\n</main>\n</body>\n</html>\n"
    ))
}

/// In a page's script: a function that tells the application an event. The
/// events of one page arrive in the order they were told.
const TELL: &str = "let told = Promise.resolve();\n\
                    const tell = (name, text) => {\n\
                    told = told.then(() => fetch(\"/cases/events/\" + name, \
                    {method: \"POST\", body: text}));\n\
                    return told;\n};\n";

async fn rerender() -> Html<String> {
    page(
        "Pallets",
        &format!(
            "<h1>Pallets</h1>\n<p id=\"status\" role=\"status\">Ready.</p>\n<ul id=\"rows\"></ul>\n\
             <script>\n{TELL}\
             let names = [\"Pallet\", \"Crate\", \"Barrel\"];\n\
             let drawn = 0;\n\
             const random = () => Math.random().toString(36).slice(2, 10);\n\
             function draw() {{\n\
               drawn += 1;\n\
               const list = document.getElementById(\"rows\");\n\
               list.replaceChildren();\n\
               for (const name of names) {{\n\
                 const item = document.createElement(\"li\");\n\
                 item.id = \"row-\" + random();\n\
                 const label = document.createElement(\"span\");\n\
                 label.id = \"label-\" + random();\n\
                 label.textContent = name;\n\
                 const button = document.createElement(\"button\");\n\
                 button.id = \"btn-\" + random();\n\
                 button.textContent = \"Archive \" + name;\n\
                 button.addEventListener(\"click\", () => {{\n\
                   names = names.filter((other) => other !== name);\n\
                   document.getElementById(\"status\").textContent = \"Archived \" + name + \".\";\n\
                   tell(\"archived\", name);\n\
                   draw();\n\
                 }});\n\
                 item.append(label, \" \", button);\n\
                 list.append(item);\n\
               }}\n\
               document.body.dataset.drawn = String(drawn);\n\
             }}\n\
             draw();\n\
             </script>"
        ),
    )
}

async fn typed_input() -> Html<String> {
    page(
        "Reference",
        &format!(
            "<h1>Reference</h1>\n<label for=\"ref\">Reference</label>\n\
             <input id=\"ref\" autocomplete=\"off\">\n\
             <button id=\"save\">Save reference</button>\n\
             <p id=\"status\" role=\"status\">Nothing kept.</p>\n\
             <script>\n{TELL}\
             const input = document.getElementById(\"ref\");\n\
             const status = document.getElementById(\"status\");\n\
             let keyed = false;\n\
             let kept = \"\";\n\
             input.addEventListener(\"keydown\", (event) => {{ if (event.isTrusted) keyed = true; }});\n\
             input.addEventListener(\"input\", (event) => {{\n\
               if (event.isTrusted && keyed) {{\n\
                 kept = input.value;\n\
               }} else {{\n\
                 input.value = \"\";\n\
                 kept = \"\";\n\
                 tell(\"ignored\", event.isTrusted ? \"not typed\" : \"not real\");\n\
               }}\n\
               keyed = false;\n\
               status.textContent = kept === \"\" ? \"Nothing kept.\" : \"Keeping \" + kept + \".\";\n\
             }});\n\
             document.getElementById(\"save\").addEventListener(\"click\", () => {{\n\
               tell(\"reference\", kept);\n\
               status.textContent = \"Saved \" + kept + \".\";\n\
             }});\n\
             </script>"
        ),
    )
}

async fn in_page() -> Html<String> {
    page(
        "Browse",
        &format!(
            "<h1 id=\"title\">Orders</h1>\n\
             <nav><button id=\"go-orders\">Orders</button> <button id=\"go-customers\">Customers</button></nav>\n\
             <p id=\"shown\" role=\"status\">Orders shown.</p>\n\
             <script>\n{TELL}\
             tell(\"loaded\", location.pathname);\n\
             function show(name) {{\n\
               const shown = document.getElementById(\"shown\");\n\
               shown.textContent = \"Loading.\";\n\
               setTimeout(() => {{\n\
                 document.getElementById(\"title\").textContent = name;\n\
                 shown.textContent = name + \" shown.\";\n\
               }}, 300);\n\
             }}\n\
             for (const [id, name, path] of [[\"go-orders\", \"Orders\", \"/cases/in-page\"], \
                                             [\"go-customers\", \"Customers\", \"/cases/in-page/customers\"]]) {{\n\
               document.getElementById(id).addEventListener(\"click\", () => {{\n\
                 history.pushState({{}}, \"\", path);\n\
                 tell(\"moved\", path);\n\
                 show(name);\n\
               }});\n\
             }}\n\
             </script>"
        ),
    )
}

async fn grid() -> Html<String> {
    page(
        "Order lines",
        &format!(
            "<h1>Order lines</h1>\n\
             <label for=\"find\">Find row</label>\n\
             <input id=\"find\" autocomplete=\"off\" placeholder=\"R-00001 to R-{GRID_ROWS:05}\">\n\
             <div id=\"grid\" role=\"grid\" aria-label=\"Lines\" aria-rowcount=\"{GRID_ROWS}\" tabindex=\"0\" \
                  style=\"height:240px;overflow:auto;position:relative;border:1px solid #888\">\n\
               <div id=\"space\" style=\"height:{height}px\"></div>\n\
             </div>\n\
             <script>\n{TELL}\
             const ROWS = {GRID_ROWS};\n\
             const HEIGHT = 24;\n\
             const grid = document.getElementById(\"grid\");\n\
             const space = document.getElementById(\"space\");\n\
             const key = (n) => \"R-\" + String(n).padStart(5, \"0\");\n\
             const quantity = (n) => (n * 7) % 90 + 1;\n\
             function draw() {{\n\
               const first = Math.max(0, Math.floor(grid.scrollTop / HEIGHT) - 2);\n\
               const last = Math.min(ROWS, first + 14);\n\
               for (const old of grid.querySelectorAll('[role=\"row\"]')) old.remove();\n\
               for (let n = first + 1; n <= last; n++) {{\n\
                 const row = document.createElement(\"div\");\n\
                 row.setAttribute(\"role\", \"row\");\n\
                 row.setAttribute(\"aria-rowindex\", String(n));\n\
                 row.style.cssText = \"position:absolute;left:0;right:0;height:\" + HEIGHT + \"px;top:\" + ((n - 1) * HEIGHT) + \"px\";\n\
                 for (const text of [key(n), \"Line \" + n, String(quantity(n))]) {{\n\
                   const cell = document.createElement(\"span\");\n\
                   cell.setAttribute(\"role\", \"gridcell\");\n\
                   cell.style.cssText = \"display:inline-block;width:100px\";\n\
                   cell.textContent = text;\n\
                   row.append(cell, \" \");\n\
                 }}\n\
                 grid.append(row);\n\
               }}\n\
               document.body.dataset.rowsInPage = String(grid.querySelectorAll('[role=\"row\"]').length);\n\
             }}\n\
             grid.addEventListener(\"scroll\", draw);\n\
             document.getElementById(\"find\").addEventListener(\"change\", (event) => {{\n\
               const match = /^R-(\\d{{5}})$/.exec(event.target.value);\n\
               const n = match ? Number(match[1]) : 0;\n\
               if (n < 1 || n > ROWS) {{ tell(\"find\", \"none\"); return; }}\n\
               grid.scrollTop = (n - 1) * HEIGHT;\n\
               draw();\n\
               tell(\"find\", key(n));\n\
             }});\n\
             draw();\n\
             </script>",
            height = u64::from(GRID_ROWS) * 24
        ),
    )
}

async fn confirm() -> Html<String> {
    page(
        "Archive",
        &format!(
            "<h1>Archive</h1>\n<button id=\"archive\">Archive order</button>\n\
             <p id=\"status\" role=\"status\">Not archived.</p>\n\
             <div id=\"ask\" role=\"dialog\" aria-label=\"Archive this order?\" hidden>\n\
               <p>Archive this order?</p>\n\
               <button id=\"yes\">Confirm</button> <button id=\"no\">Cancel</button>\n\
             </div>\n\
             <script>\n{TELL}\
             let presses = 0;\n\
             const ask = document.getElementById(\"ask\");\n\
             const status = document.getElementById(\"status\");\n\
             const done = () => {{ tell(\"archived\", \"press \" + presses); status.textContent = \"Archived.\"; }};\n\
             document.getElementById(\"archive\").addEventListener(\"click\", () => {{\n\
               presses += 1;\n\
               tell(\"pressed\", String(presses));\n\
               if (presses % 4 === 0) {{\n\
                 if (confirm(\"Archive this order?\")) done(); else tell(\"cancelled\", \"press \" + presses);\n\
               }} else if (presses % 2 === 1) {{\n\
                 ask.hidden = false;\n\
               }} else {{\n\
                 done();\n\
               }}\n\
             }});\n\
             document.getElementById(\"yes\").addEventListener(\"click\", () => {{ ask.hidden = true; done(); }});\n\
             document.getElementById(\"no\").addEventListener(\"click\", () => {{\n\
               ask.hidden = true;\n\
               tell(\"cancelled\", \"press \" + presses);\n\
               status.textContent = \"Not archived.\";\n\
             }});\n\
             </script>"
        ),
    )
}

async fn frame() -> Html<String> {
    page(
        "Hand-off",
        &format!(
            "<h1>Hand-off</h1>\n\
             <iframe id=\"form-frame\" title=\"Dispatch form\" src=\"/cases/frame/inner\" \
                     style=\"width:420px;height:160px\"></iframe>\n\
             <note-card id=\"card\"></note-card>\n\
             <script>\n{TELL}\
             customElements.define(\"note-card\", class extends HTMLElement {{\n\
               connectedCallback() {{\n\
                 const root = this.attachShadow({{mode: \"open\"}});\n\
                 root.innerHTML = '<label>Shadow note <input name=\"note\"></label> \
                   <button>Send note</button> <output role=\"status\"></output>';\n\
                 const input = root.querySelector(\"input\");\n\
                 const out = root.querySelector(\"output\");\n\
                 root.querySelector(\"button\").addEventListener(\"click\", () => {{\n\
                   tell(\"shadow-note\", input.value);\n\
                   out.textContent = \"Note sent.\";\n\
                 }});\n\
               }}\n\
             }});\n\
             </script>"
        ),
    )
}

async fn frame_inner() -> Html<String> {
    page(
        "Dispatch form",
        &format!(
            "<form id=\"dispatch\">\n<label>Dispatch code <input name=\"code\"></label>\n\
             <button type=\"submit\">Send code</button>\n\
             <p id=\"status\" role=\"status\"></p>\n</form>\n\
             <script>\n{TELL}\
             document.getElementById(\"dispatch\").addEventListener(\"submit\", (event) => {{\n\
               event.preventDefault();\n\
               tell(\"frame-code\", event.target.elements.code.value);\n\
               document.getElementById(\"status\").textContent = \"Code sent.\";\n\
             }});\n\
             </script>"
        ),
    )
}
