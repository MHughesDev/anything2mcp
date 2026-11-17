//! Case 33: an order's delivery details, edited on a page made of the browser's own
//! controls (a select, a check box, a radio group, a date field, a text area), with a
//! confirmation dialog before saving and a button that stays disabled until the date
//! is valid. The page saves through the JSON interface, like the line-item form.

use std::collections::BTreeMap;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::AppState;
use crate::pages::escape;
use crate::store::{FieldProblem, OrderStatus, Store};

const MAX_NOTES_CHARS: usize = 500;

/// How an order is shipped: the select.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shipping {
    /// The usual way.
    Standard,
    /// Faster, at a cost.
    Express,
    /// On a pallet.
    Freight,
}

/// How urgent an order is: the radio group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Whenever convenient.
    Low,
    /// The usual.
    Normal,
    /// Before anything else.
    High,
}

/// One order's delivery details exactly as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDelivery {
    /// The select.
    pub shipping: Shipping,
    /// The check box.
    pub gift_wrap: bool,
    /// The radio group.
    pub priority: Priority,
    /// The date field, as `YYYY-MM-DD`.
    pub deliver_on: String,
    /// The text area, exactly as sent.
    pub notes: String,
    /// Starts at 1 and goes up by one on every stored change.
    pub version: u64,
}

/// The values a caller asks to store. Every field is required, the version too.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryInput {
    shipping: Shipping,
    gift_wrap: bool,
    priority: Priority,
    deliver_on: String,
    notes: String,
    version: u64,
}

/// Every order's delivery details, and how many times each was written.
#[derive(Debug)]
pub(crate) struct Deliveries {
    stored: BTreeMap<String, StoredDelivery>,
    writes: BTreeMap<String, u64>,
}

impl Default for Deliveries {
    fn default() -> Self {
        let seed = |deliver_on: &str| StoredDelivery {
            shipping: Shipping::Standard,
            gift_wrap: false,
            priority: Priority::Normal,
            deliver_on: deliver_on.to_owned(),
            notes: String::new(),
            version: 1,
        };
        Self {
            stored: [
                ("A-100".to_owned(), seed("2026-12-01")),
                ("A-101".to_owned(), seed("2026-11-20")),
                ("A-102".to_owned(), seed("2026-12-15")),
            ]
            .into_iter()
            .collect(),
            writes: BTreeMap::new(),
        }
    }
}

impl Deliveries {
    pub(crate) fn get(&self, order_id: &str) -> Option<&StoredDelivery> {
        self.stored.get(order_id)
    }

    pub(crate) fn write_count(&self, order_id: &str) -> u64 {
        self.writes.get(order_id).copied().unwrap_or(0)
    }
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/orders/{order_id}/delivery", get(delivery_page))
        .merge(api_routes())
}

/// The JSON routes alone, also served under the signed-in prefixes.
pub(crate) fn api_routes() -> Router<AppState> {
    Router::new().route(
        "/api/orders/{order_id}/delivery",
        get(get_delivery).put(put_delivery),
    )
}

fn view(order_id: &str, delivery: &StoredDelivery) -> Response {
    let etag = format!("\"{}\"", delivery.version);
    let mut body = json!(delivery);
    body["order_id"] = json!(order_id);
    ([(header::ETAG, etag)], Json(body)).into_response()
}

fn refusal(status: StatusCode, error: &str, message: &str) -> Response {
    (status, Json(json!({"error": error, "message": message}))).into_response()
}

async fn get_delivery(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    match inner.deliveries.get(&order_id) {
        Some(delivery) => view(&order_id, delivery),
        None => refusal(StatusCode::NOT_FOUND, "not_found", "No such order."),
    }
}

async fn put_delivery(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
    body: Result<Json<DeliveryInput>, JsonRejection>,
) -> Response {
    let Json(input) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return refusal(
                StatusCode::BAD_REQUEST,
                "bad_request",
                &rejection.body_text(),
            );
        }
    };
    let mut inner = state.lock().await;
    let inner = &mut *inner;
    let status = inner.store.order(&order_id).map(|o| o.status);
    let Some(current) = inner.deliveries.stored.get(&order_id) else {
        return refusal(StatusCode::NOT_FOUND, "not_found", "No such order.");
    };
    if status != Some(OrderStatus::Draft) {
        return refusal(
            StatusCode::CONFLICT,
            "order_not_draft",
            "The order has been submitted and can no longer be changed.",
        );
    }
    let problems = validate(&input);
    if !problems.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "validation",
                "message": "The delivery details were not saved.",
                "problems": problems,
            })),
        )
            .into_response();
    }
    if input.version != current.version {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "version_conflict",
                "message": "The delivery details were changed by someone else. Reload them and try again.",
                "current_version": current.version,
            })),
        )
            .into_response();
    }
    let stored = StoredDelivery {
        shipping: input.shipping,
        gift_wrap: input.gift_wrap,
        priority: input.priority,
        deliver_on: input.deliver_on,
        notes: input.notes,
        version: current.version + 1,
    };
    inner
        .deliveries
        .stored
        .insert(order_id.clone(), stored.clone());
    *inner.deliveries.writes.entry(order_id.clone()).or_insert(0) += 1;
    view(&order_id, &stored)
}

fn validate(input: &DeliveryInput) -> Vec<FieldProblem> {
    let mut problems = Vec::new();
    if !is_date(&input.deliver_on) {
        problems.push(FieldProblem {
            field: "deliver_on".to_owned(),
            message: "Delivery date must be a date from 2000-01-01 to 2099-12-31.".to_owned(),
        });
    }
    if input.notes.chars().count() > MAX_NOTES_CHARS {
        problems.push(FieldProblem {
            field: "notes".to_owned(),
            message: "Notes must be 500 characters or fewer.".to_owned(),
        });
    }
    problems
}

/// Whether the text is a real calendar date written `YYYY-MM-DD`, in 2000 to 2099.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = text.get(range)?;
        part.chars()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (Some(year), Some(month), Some(day)) = (number(0..4), number(5..7), number(8..10)) else {
        return false;
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (2000..=2099).contains(&year) && (1..=days).contains(&day)
}

async fn delivery_page(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    let (Some(_), Some(delivery)) = (
        Store::order(&inner.store, &order_id),
        inner.deliveries.get(&order_id),
    ) else {
        let body = "<h1>Not found</h1>\n<p>No such order.</p>";
        return (StatusCode::NOT_FOUND, Html(page("Not found", body))).into_response();
    };
    let id = escape(&order_id);
    let selected = |on: bool| if on { " selected" } else { "" };
    let checked = |on: bool| if on { " checked" } else { "" };
    let shipping = [
        (Shipping::Standard, "standard", "Standard"),
        (Shipping::Express, "express", "Express"),
        (Shipping::Freight, "freight", "Freight"),
    ]
    .iter()
    .map(|(value, code, label)| {
        format!(
            "<option value=\"{code}\"{}>{label}</option>",
            selected(*value == delivery.shipping)
        )
    })
    .collect::<String>();
    let priority = [
        (Priority::Low, "low", "Low"),
        (Priority::Normal, "normal", "Normal"),
        (Priority::High, "high", "High"),
    ]
    .iter()
    .map(|(value, code, label)| {
        format!(
            "<label><input type=\"radio\" name=\"priority\" value=\"{code}\"{}> {label}</label>\n",
            checked(*value == delivery.priority)
        )
    })
    .collect::<String>();
    let body = format!(
        "<h1>Delivery for order {id}</h1>\n\
         <form id=\"delivery\" data-order=\"{id}\" data-version=\"{version}\">\n\
         <p><label for=\"shipping\">Shipping</label> <select id=\"shipping\" name=\"shipping\">{shipping}</select></p>\n\
         <p><input type=\"checkbox\" id=\"gift_wrap\" name=\"gift_wrap\"{gift}> <label for=\"gift_wrap\">Gift wrap</label></p>\n\
         <fieldset>\n<legend>Priority</legend>\n{priority}</fieldset>\n\
         <p><label for=\"deliver_on\">Delivery date</label> \
         <input type=\"date\" id=\"deliver_on\" name=\"deliver_on\" min=\"2000-01-01\" max=\"2099-12-31\" value=\"{date}\" required></p>\n\
         <p><label for=\"notes\">Notes</label> <textarea id=\"notes\" name=\"notes\" rows=\"4\" maxlength=\"500\">{notes}</textarea></p>\n\
         <p><button type=\"submit\" id=\"save\">Save</button></p>\n\
         <p id=\"status\" role=\"status\"></p>\n\
         <ul id=\"problems\" role=\"alert\"></ul>\n\
         </form>\n<script>\n{DELIVERY_SCRIPT}</script>",
        version = delivery.version,
        gift = checked(delivery.gift_wrap),
        date = escape(&delivery.deliver_on),
        notes = escape(&delivery.notes),
    );
    Html(page(&format!("Delivery for order {order_id}"), &body)).into_response()
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{} - Example Orders</title>\n</head>\n<body>\n\
         <nav><a href=\"/orders\">Orders</a></nav>\n<main>\n{body}\n</main>\n</body>\n</html>\n",
        escape(title)
    )
}

/// Keeps Save disabled until the date is valid, asks for confirmation in a native
/// dialog, then saves through the JSON interface.
const DELIVERY_SCRIPT: &str = r#"const form = document.getElementById("delivery");
const save = document.getElementById("save");
const status = document.getElementById("status");
const problems = document.getElementById("problems");
const date = form.elements.deliver_on;
const refresh = () => { save.disabled = !(date.value && date.checkValidity()); };
date.addEventListener("input", refresh);
date.addEventListener("change", refresh);
refresh();
form.addEventListener("submit", async (event) => {
  event.preventDefault();
  status.textContent = "";
  problems.replaceChildren();
  if (!window.confirm("Save the delivery details?")) {
    status.textContent = "Not saved.";
    return;
  }
  const body = {
    shipping: form.elements.shipping.value,
    gift_wrap: form.elements.gift_wrap.checked,
    priority: form.elements.priority.value,
    deliver_on: date.value,
    notes: form.elements.notes.value,
    version: Number(form.dataset.version),
  };
  const address = "/api/orders/" + encodeURIComponent(form.dataset.order) + "/delivery";
  const response = await fetch(address, {
    method: "PUT",
    headers: {"Content-Type": "application/json"},
    body: JSON.stringify(body),
  });
  const answer = await response.json();
  if (response.ok) {
    form.dataset.version = String(answer.version);
    status.textContent = "Saved.";
    return;
  }
  status.textContent = answer.message || "The delivery details were not saved.";
  for (const problem of answer.problems || []) {
    const line = document.createElement("li");
    line.textContent = problem.message;
    problems.append(line);
  }
});
"#;

#[cfg(test)]
mod tests {
    use super::is_date;

    #[test]
    fn only_real_dates_are_dates() {
        for good in ["2026-12-01", "2028-02-29", "2000-02-29", "2099-12-31"] {
            assert!(is_date(good), "{good}");
        }
        for bad in [
            "2026-02-29",
            "2100-02-28",
            "1999-12-31",
            "2026-13-01",
            "2026-04-31",
            "2026-1-01",
            "26-12-01xx",
            "2026/12/01",
            "+026-12-01",
            "",
        ] {
            assert!(!is_date(bad), "{bad}");
        }
    }
}
