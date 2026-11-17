//! The pages a person, or the interface driver, uses. Plain HTML with one small
//! script on the form, and no build step.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;

use crate::AppState;
use crate::store::{OrderStatus, StoredItem, format_length_in};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(|| async { Redirect::to("/orders") }))
        .route("/orders", get(order_list))
        .route("/orders/{order_id}", get(order_detail))
        .route("/orders/{order_id}/items/{item_code}", get(item_form))
}

/// Makes text safe to place inside HTML, in content or in a quoted attribute.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn page(title: &str, body: &str) -> Html<String> {
    let title = escape(title);
    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{title} - Example Orders</title>\n</head>\n<body>\n\
         <nav><a href=\"/orders\">Orders</a></nav>\n<main>\n{body}\n</main>\n</body>\n</html>\n"
    ))
}

fn missing(what: &str) -> Response {
    let body = format!("<h1>Not found</h1>\n<p>No such {}.</p>", escape(what));
    (StatusCode::NOT_FOUND, page("Not found", &body)).into_response()
}

fn status_word(status: OrderStatus) -> &'static str {
    match status {
        OrderStatus::Draft => "Draft",
        OrderStatus::Submitted => "Submitted",
    }
}

async fn order_list(State(state): State<AppState>) -> Html<String> {
    let inner = state.lock().await;
    let mut rows = String::new();
    for order in inner.store.orders() {
        let id = escape(&order.order_id);
        rows.push_str(&format!(
            "<tr><td><a href=\"/orders/{id}\">{id}</a></td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape(&order.customer),
            status_word(order.status),
            order.items.len(),
        ));
    }
    page(
        "Orders",
        &format!(
            "<h1>Orders</h1>\n<table>\n<caption>All orders</caption>\n\
             <thead><tr><th>Order</th><th>Customer</th><th>Status</th><th>Lines</th></tr></thead>\n\
             <tbody>\n{rows}</tbody>\n</table>"
        ),
    )
}

async fn order_detail(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    let Some(order) = inner.store.order(&order_id) else {
        return missing("order");
    };
    let id = escape(&order.order_id);
    let mut rows = String::new();
    for item in order.items.values() {
        let code = escape(&item.item_code);
        rows.push_str(&format!(
            "<tr><td><a href=\"/orders/{id}/items/{code}\">{code}</a></td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape(&item.description),
            item.quantity,
            format_length_in(item.length_sixteenths),
        ));
    }
    let add = match order.status {
        OrderStatus::Draft => {
            format!("<p><a href=\"/orders/{id}/items/new\">Add line item</a></p>")
        }
        OrderStatus::Submitted => {
            "<p>This order has been submitted and cannot be changed.</p>".to_owned()
        }
    };
    let body = format!(
        "<h1>Order {id}</h1>\n<dl>\n<dt id=\"order-customer\">Customer</dt><dd aria-labelledby=\"order-customer\">{}</dd>\n\
         <dt id=\"order-status\">Status</dt><dd aria-labelledby=\"order-status\">{}</dd>\n</dl>\n\
         <table>\n<caption>Line items</caption>\n\
         <thead><tr><th>Item code</th><th>Description</th><th>Quantity</th><th>Length (in)</th></tr></thead>\n\
         <tbody>\n{rows}</tbody>\n</table>\n{add}",
        escape(&order.customer),
        status_word(order.status),
    );
    page(&format!("Order {}", order.order_id), &body).into_response()
}

/// The form for one line item. The address `.../items/new` gives an empty form.
pub(crate) async fn item_form(
    State(state): State<AppState>,
    Path((order_id, item_code)): Path<(String, String)>,
) -> Response {
    let inner = state.lock().await;
    let Some(order) = inner.store.order(&order_id) else {
        return missing("order");
    };
    let item: Option<&StoredItem> = order.items.get(&item_code);
    if item.is_none() && item_code != "new" {
        return missing("line item");
    }
    let id = escape(&order.order_id);
    let (code, description, quantity, length, version, code_lock) = match item {
        Some(item) => (
            escape(&item.item_code),
            escape(&item.description),
            item.quantity.to_string(),
            format_length_in(item.length_sixteenths),
            item.version.to_string(),
            " readonly",
        ),
        None => (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "",
        ),
    };
    let body = format!(
        "<h1>Order {id}</h1>\n\
         <form id=\"line-item\" data-order=\"{id}\" data-version=\"{version}\">\n\
         <h2>Line Item Details</h2>\n\
         <p><label for=\"item_code\">Item code</label> \
         <input id=\"item_code\" name=\"item_code\" value=\"{code}\" required{code_lock}></p>\n\
         <p><label for=\"description\">Description</label> \
         <input id=\"description\" name=\"description\" value=\"{description}\" required></p>\n\
         <p><label for=\"quantity\">Quantity</label> \
         <input id=\"quantity\" name=\"quantity\" inputmode=\"numeric\" value=\"{quantity}\" required></p>\n\
         <fieldset>\n<legend>Dimensions</legend>\n\
         <p><label for=\"length\">Length</label> \
         <input id=\"length\" name=\"length_in\" value=\"{length}\" aria-describedby=\"length-hint\" required> \
         <span id=\"length-hint\">inches</span></p>\n</fieldset>\n\
         <p><button type=\"submit\">Save</button></p>\n\
         <p id=\"status\" role=\"status\"></p>\n\
         <ul id=\"problems\" role=\"alert\"></ul>\n\
         </form>\n<script>\n{FORM_SCRIPT}</script>"
    );
    page("Line item", &body).into_response()
}

/// Saves the form through the JSON interface and shows what the application answered.
const FORM_SCRIPT: &str = r#"const form = document.getElementById("line-item");
const status = document.getElementById("status");
const problems = document.getElementById("problems");
form.addEventListener("submit", async (event) => {
  event.preventDefault();
  status.textContent = "";
  problems.replaceChildren();
  const code = form.elements.item_code.value.trim();
  const body = {
    description: form.elements.description.value,
    quantity: Number.parseInt(form.elements.quantity.value, 10) || 0,
    length_in: form.elements.length_in.value,
  };
  if (form.dataset.version) body.version = Number(form.dataset.version);
  const address = "/api/orders/" + encodeURIComponent(form.dataset.order) + "/items/" + encodeURIComponent(code);
  const response = await fetch(address, {
    method: "PUT",
    headers: {"Content-Type": "application/json"},
    body: JSON.stringify(body),
  });
  const answer = await response.json();
  if (response.ok) {
    form.dataset.version = String(answer.version);
    form.elements.item_code.readOnly = true;
    status.textContent = "Saved.";
    return;
  }
  status.textContent = answer.message || "The line item was not saved.";
  for (const problem of answer.problems || []) {
    const line = document.createElement("li");
    line.textContent = problem.message;
    problems.append(line);
  }
});
"#;

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn text_from_records_cannot_become_markup() {
        assert_eq!(
            escape(r#"<script>alert("x")</script> & 'y'"#),
            "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; &#39;y&#39;"
        );
    }
}
