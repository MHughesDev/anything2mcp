//! Case 32, continued: pages that try to get out of a tab that only observes, or
//! out of the origins a tab was opened for, for the browser engine's proofs that
//! they cannot. Every attempt aims at `/traps/write/escapes/{what}`, which counts
//! it, so a test asks the control interface and shows zero.
//!
//! "The other name" is this same application under the other loopback name:
//! `localhost` when the page was asked for as `127.0.0.1`, and `127.0.0.1`
//! otherwise. To a browser it is another origin, and another site.
//!
//! - `/escapes/quiet`: a page that does nothing, for a test's own script.
//! - `/escapes/script`, `/escapes/meta`, `/escapes/form`: after loading, the page
//!   moves itself with `GET`: by script, by a refresh tag, by submitting a form.
//! - `/escapes/form-post` and `/escapes/beacon`: the page submits a form with
//!   `POST`, and sends a beacon.
//! - `/escapes/elsewhere`: the page's script moves it to the other name.
//! - `/escapes/redirect`: the answer redirects to the other name.
//! - `/escapes/download/{name}`: the answer is a file to save under that name.
//! - `/escapes/links`: a link that opens a new window, and a link to a download.
//! - `/escapes/frame`: embeds `/escapes/inner` from the other name. The inner
//!   page opens a WebSocket and sends a `POST`, in its first document and again
//!   in a second one it moves itself to, and tells its parent when it has tried.
//! - `/escapes/loads`: loads an image, a script, and a style, sends a `fetch`
//!   with `GET` and one with `POST`, and opens a WebSocket, each to
//!   `loads-{image,script,style,fetch,post,socket}` at the origin named by
//!   `from` (the other name when none is given), then says "Tried.". With
//!   `frame`, it also embeds `{frame}/escapes/loads?from={from}`, and says
//!   "The frame tried." once that page has. For the proofs that a tab sends
//!   nothing outside what it may reach (decision 0292).
//! - `/escapes/service-worker.js` and `/escapes/shared-worker.js`: workers that,
//!   when reached, send a `POST` and open a WebSocket, and answer with what
//!   became of each: the service worker when sent a message, the shared worker
//!   when a page connects to it. The service worker also answers a request for
//!   `/escapes/answered-by-worker` itself, with `from the worker`, which the
//!   application never serves.

use axum::Router;
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::AppState;

/// Where every attempt lands.
const WRITE: &str = "/traps/write/escapes";

/// In a page's script: this application's address under the other name.
const OTHER: &str = "location.protocol + \"//\" + \
    (location.hostname === \"127.0.0.1\" ? \"localhost\" : \"127.0.0.1\") + \":\" + location.port";

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/escapes/quiet", get(quiet))
        .route("/escapes/script", get(script))
        .route("/escapes/meta", get(meta))
        .route("/escapes/form", get(form))
        .route("/escapes/form-post", get(form_post))
        .route("/escapes/beacon", get(beacon))
        .route("/escapes/elsewhere", get(elsewhere))
        .route("/escapes/redirect", get(redirect))
        .route("/escapes/download/{name}", get(download))
        .route("/escapes/links", get(links))
        .route("/escapes/frame", get(frame))
        .route("/escapes/inner", get(inner))
        .route("/escapes/loads", get(loads))
        .route("/escapes/service-worker.js", get(service_worker))
        .route("/escapes/shared-worker.js", get(shared_worker))
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{title} - Example Orders</title>\n</head>\n<body>\n<main>\n\
         <h1>{title}</h1>\n{body}\n</main>\n</body>\n</html>\n"
    ))
}

/// A script that runs `act` once the page has loaded, then shows that it tried.
fn after_load(act: &str) -> String {
    format!(
        "<script>\naddEventListener(\"load\", () => setTimeout(() => {{\n{act}\n\
         const tried = document.createElement(\"p\");\ntried.id = \"tried\";\n\
         tried.textContent = \"Tried.\";\ndocument.querySelector(\"main\").append(tried);\n\
         }}, 0));\n</script>"
    )
}

fn script_type() -> [(header::HeaderName, &'static str); 1] {
    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")]
}

async fn quiet() -> Html<String> {
    page("Order notes", "<p>No notes.</p>")
}

async fn script() -> Html<String> {
    page(
        "Order archive",
        &after_load(&format!("location.href = \"{WRITE}/script\";")),
    )
}

async fn meta() -> Html<String> {
    page(
        "Order summary",
        &format!(
            "<meta http-equiv=\"refresh\" content=\"0;url={WRITE}/meta\">\n<p>One moment.</p>"
        ),
    )
}

async fn form() -> Html<String> {
    page(
        "Order search",
        &format!(
            "<form method=\"get\" action=\"{WRITE}/form\">\n\
             <input type=\"hidden\" name=\"order\" value=\"A-100\">\n</form>\n{}",
            after_load("document.forms[0].submit();")
        ),
    )
}

async fn form_post() -> Html<String> {
    page(
        "Order renewal",
        &format!(
            "<form method=\"post\" action=\"{WRITE}/form-post\">\n\
             <input type=\"hidden\" name=\"order\" value=\"A-100\">\n</form>\n{}",
            after_load("document.forms[0].submit();")
        ),
    )
}

async fn beacon() -> Html<String> {
    page(
        "Order statistics",
        &after_load(&format!(
            "navigator.sendBeacon(\"{WRITE}/beacon\", \"seen\");"
        )),
    )
}

async fn elsewhere() -> Html<String> {
    page(
        "Order partners",
        &after_load(&format!("location.href = {OTHER} + \"{WRITE}/elsewhere\";")),
    )
}

/// The same machine and port under the other loopback name, from the request.
fn other_name(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(header::HOST)?.to_str().ok()?;
    let (name, port) = host.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    let other = if name == "127.0.0.1" {
        "localhost"
    } else {
        "127.0.0.1"
    };
    Some(format!("http://{other}:{port}"))
}

async fn redirect(headers: HeaderMap) -> Response {
    match other_name(&headers) {
        Some(other) => (
            StatusCode::FOUND,
            [(header::LOCATION, format!("{other}{WRITE}/redirect"))],
        )
            .into_response(),
        None => (StatusCode::BAD_REQUEST, "no usable Host header").into_response(),
    }
}

async fn download(Path(name): Path<String>) -> Response {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    if !plain {
        return (StatusCode::BAD_REQUEST, "not a plain file name").into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        "order,total\nA-100,12.50\n",
    )
        .into_response()
}

async fn links() -> Html<String> {
    page(
        "Order documents",
        &format!(
            "<ul>\n\
             <li><a href=\"{WRITE}/window\" target=\"_blank\">Open the packing list</a></li>\n\
             <li><a href=\"/escapes/download/example-orders.csv\">Export the orders</a></li>\n\
             </ul>"
        ),
    )
}

async fn frame() -> Html<String> {
    page(
        "Order tracking",
        &format!(
            "<p>The carrier's view:</p>\n<script>\n\
             addEventListener(\"message\", (event) => {{\n\
             if (event.data !== \"second\") return;\n\
             const done = document.createElement(\"p\");\ndone.id = \"tried\";\n\
             done.textContent = \"The carrier's view has loaded twice.\";\n\
             document.querySelector(\"main\").append(done);\n}});\n\
             const view = document.createElement(\"iframe\");\n\
             view.title = \"Carrier\";\nview.src = {OTHER} + \"/escapes/inner\";\n\
             document.querySelector(\"main\").append(view);\n</script>"
        ),
    )
}

/// What the workers and the inner frame do: a `POST` and a WebSocket, each to
/// its own counted name. `{0}` is the name's ending.
const TRY_BOTH: &str = "\
const post = (name) => fetch(\"/traps/write/escapes/\" + name, {method: \"POST\"})\n\
  .then(() => \"sent\", () => \"failed\");\n\
const socket = (name) => {\n\
  try {\n\
    new WebSocket((location.protocol === \"https:\" ? \"wss://\" : \"ws://\") + location.host +\n\
      \"/traps/write/escapes/\" + name);\n\
    return \"made\";\n\
  } catch (error) {\n\
    return \"refused\";\n\
  }\n\
};\n";

async fn inner() -> Html<String> {
    page(
        "Carrier",
        &format!(
            "<p>In transit.</p>\n<script>\n{TRY_BOTH}\
             const second = location.search === \"?again\";\n\
             const which = second ? \"second\" : \"first\";\n\
             socket(\"frame-socket-\" + which);\n\
             post(\"frame-post-\" + which).then(() => {{\n\
             parent.postMessage(which, \"*\");\n\
             if (!second) location.href = \"/escapes/inner?again\";\n}});\n</script>"
        ),
    )
}

/// The origin a query names under `name`, when it names one: a scheme, then
/// a host and a port of plain characters, and nothing else. Anything else
/// is as if it were not there.
fn origin_in(query: Option<&str>, name: &str) -> Option<String> {
    let value = query?
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))?;
    let decoded = percent_decoded(value)?;
    let rest = decoded
        .strip_prefix("http://")
        .or_else(|| decoded.strip_prefix("https://"))?;
    let plain = !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']'));
    plain.then_some(decoded)
}

fn percent_decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let pair = text.get(at + 1..at + 3)?;
            out.push(u8::from_str_radix(pair, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

async fn loads(RawQuery(query): RawQuery) -> Html<String> {
    let quoted =
        |origin: Option<String>| origin.map_or_else(|| "\"\"".to_owned(), |o| format!("\"{o}\""));
    let from = quoted(origin_in(query.as_deref(), "from"));
    let frame = quoted(origin_in(query.as_deref(), "frame"));
    page(
        "Order assets",
        &format!(
            "<p>The order's assets.</p>\n<script>\n\
             const from = {from} || {OTHER};\nconst frame = {frame};\n\
             const main = document.querySelector(\"main\");\n\
             const at = (what) => from + \"{WRITE}/loads-\" + what;\n\
             const settled = [];\n\
             const loaded = (element) => settled.push(new Promise((done) => {{\n\
             element.onload = done;\nelement.onerror = done;\n}}));\n\
             const image = new Image();\nimage.alt = \"Logo\";\nloaded(image);\n\
             image.src = at(\"image\");\nmain.append(image);\n\
             const style = document.createElement(\"link\");\nstyle.rel = \"stylesheet\";\n\
             loaded(style);\nstyle.href = at(\"style\");\ndocument.head.append(style);\n\
             const script = document.createElement(\"script\");\nloaded(script);\n\
             script.src = at(\"script\");\ndocument.head.append(script);\n\
             settled.push(fetch(at(\"fetch\"), {{mode: \"no-cors\"}}).then(() => {{}}, () => {{}}));\n\
             settled.push(fetch(at(\"post\"), {{method: \"POST\", mode: \"no-cors\"}}).then(() => {{}}, () => {{}}));\n\
             try {{\nconst socket = new WebSocket(at(\"socket\").replace(/^http/, \"ws\"));\n\
             settled.push(new Promise((done) => {{ socket.onerror = done; socket.onclose = done; }}));\n\
             }} catch (_) {{}}\n\
             if (frame) {{\n\
             addEventListener(\"message\", (event) => {{\n\
             if (event.data !== \"tried\") return;\n\
             const done = document.createElement(\"p\");\ndone.id = \"frame-tried\";\n\
             done.textContent = \"The frame tried.\";\nmain.append(done);\n}});\n\
             const view = document.createElement(\"iframe\");\nview.title = \"Assets\";\n\
             view.src = frame + \"/escapes/loads?from=\" + encodeURIComponent(from);\n\
             main.append(view);\n}}\n\
             Promise.all(settled).then(() => {{\n\
             const tried = document.createElement(\"p\");\ntried.id = \"tried\";\n\
             tried.textContent = \"Tried.\";\nmain.append(tried);\n\
             if (parent !== window) parent.postMessage(\"tried\", \"*\");\n}});\n</script>"
        ),
    )
}

async fn service_worker() -> Response {
    (
        script_type(),
        format!(
            "{TRY_BOTH}\
             addEventListener(\"install\", () => skipWaiting());\n\
             addEventListener(\"activate\", (event) => event.waitUntil(clients.claim()));\n\
             addEventListener(\"fetch\", (event) => {{\n\
             if (new URL(event.request.url).pathname === \"/escapes/answered-by-worker\")\n\
             event.respondWith(new Response(\"from the worker\"));\n}});\n\
             addEventListener(\"message\", (event) => {{\n\
             const made = socket(\"service-socket\");\n\
             event.waitUntil(post(\"service-post\").then((sent) =>\n\
             event.source.postMessage(\"post \" + sent + \", socket \" + made)));\n}});\n"
        ),
    )
        .into_response()
}

async fn shared_worker() -> Response {
    (
        script_type(),
        format!(
            "{TRY_BOTH}\
             addEventListener(\"connect\", (event) => {{\n\
             const port = event.ports[0];\n\
             const made = socket(\"shared-socket\");\n\
             post(\"shared-post\").then((sent) =>\n\
             port.postMessage(\"post \" + sent + \", socket \" + made));\n}});\n"
        ),
    )
        .into_response()
}
