//! Case 32, for the interface driver: the form of one line item, served again
//! under `/leaving/{how}/...`, on a page that tries to take the driver to
//! another origin, or answers in a way that cannot be told from what it showed
//! before. The driver's proofs that it types, clicks, and reads nothing where it
//! should not, and that a save it cannot see answered is in doubt.
//!
//! "The other name" is this same application under the other loopback name, as
//! in `escapes`. Every move to it aims at `/traps/write/leaving/{how}`, which
//! counts what arrives, so a test asks the control interface and shows zero.
//!
//! - `/leaving/load/orders/{order}/items/{item}`: while it loads, the page moves
//!   itself to the other name.
//! - `/leaving/click/...`: the save is sent as on the plain form, and the page
//!   then moves itself to the other name instead of saying "Saved."
//! - `/leaving/window/...`: Save opens a window on the other name, and the save
//!   is sent, with no "Saved." either.
//! - `/leaving/stale/...`: once something is typed, the form shows a problem,
//!   as a page that kept an earlier refusal would; Save works as on the plain
//!   form.
//! - `/leaving/saved/...`: the form says "Saved." before anything is done.
//! - `/leaving/loads/...`: the form loads an image and sends a `fetch` with
//!   `GET` to the other name while it loads, as a page that loads from
//!   another origin does (decision 0292).

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::AppState;
use crate::pages;

/// In a page's script: this application's address under the other name.
const OTHER: &str = "location.protocol + \"//\" + \
    (location.hostname === \"127.0.0.1\" ? \"localhost\" : \"127.0.0.1\") + \":\" + location.port";

/// The most a form page may be, read back to be added to.
const MOST: usize = 1 << 20;

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route(
        "/leaving/{how}/orders/{order_id}/items/{item_code}",
        get(leaving),
    )
}

/// The save, as the plain form's script sends it, in a block of its own so
/// that its names do not meet the form's.
fn save(then: &str) -> String {
    format!(
        "{{\nconst f = document.getElementById(\"line-item\");\n\
         addEventListener(\"submit\", async (event) => {{\n\
         event.preventDefault();\nevent.stopPropagation();\n{{before}}\n\
         const body = {{description: f.elements.description.value,\n\
         quantity: Number.parseInt(f.elements.quantity.value, 10) || 0,\n\
         length_in: f.elements.length_in.value}};\n\
         if (f.dataset.version) body.version = Number(f.dataset.version);\n\
         const code = f.elements.item_code.value.trim();\n\
         await fetch(\"/api/orders/\" + encodeURIComponent(f.dataset.order) + \"/items/\" + encodeURIComponent(code), \
         {{method: \"PUT\", headers: {{\"Content-Type\": \"application/json\"}}, body: JSON.stringify(body)}});\n\
         {then}\n}}, true);\n}}"
    )
}

/// What the page adds to the plain form, by `how`.
fn added(how: &str) -> Option<String> {
    let trap = |name: &str| format!("{OTHER} + \"/traps/write/leaving/{name}\"");
    Some(match how {
        "load" => format!("location.href = {};", trap("load")),
        "click" => save(&format!("location.href = {};", trap("click"))).replace("{before}", ""),
        "window" => save("").replace("{before}", &format!("window.open({});", trap("window"))),
        "stale" => {
            "addEventListener(\"input\", () => { document.getElementById(\"problems\").innerHTML = \
                    \"<li>The quantity was refused earlier.</li>\"; }, {once: true});"
                .to_owned()
        }
        "saved" => "document.getElementById(\"status\").textContent = \"Saved.\";".to_owned(),
        "loads" => format!(
            "const logo = new Image();\nlogo.alt = \"Logo\";\nlogo.src = {};\n\
             document.body.append(logo);\nfetch({}, {{mode: \"no-cors\"}}).catch(() => {{}});",
            trap("loads-image"),
            trap("loads-fetch")
        ),
        _ => return None,
    })
}

async fn leaving(
    state: State<AppState>,
    Path((how, order_id, item_code)): Path<(String, String, String)>,
) -> Response {
    let Some(script) = added(&how) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let form = pages::item_form(state, Path((order_id, item_code))).await;
    let (parts, body) = form.into_parts();
    let Ok(bytes) = to_bytes(body, MOST).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let page = String::from_utf8_lossy(&bytes).replacen(
        "</body>",
        &format!("<script>\n{script}\n</script>\n</body>"),
        1,
    );
    Response::from_parts(parts, Body::from(page))
}
