//! The same data as the JSON interface, in the forms an HTTP service also
//! answers in: an XML view of an order and of a line item, an XML answer
//! that declares a document type, a plain-text status line, the same in an
//! old character set, and a small binary label (decision 0217).
//!
//! Each is as real as is cheap: the content type a service would send, a
//! namespace whose prefix in the document is not the one a reader chooses,
//! a repeated element, attributes, and an `ETag` on the line item.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::AppState;
use crate::store::{OrderStatus, StoredItem, StoredOrder, format_length_in};

/// The namespace of the XML views.
pub const ORDERS_NAMESPACE: &str = "urn:example:orders";

const XML: &str = "application/xml; charset=utf-8";

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/views/xml/orders/{order_id}", get(order_xml))
        .route(
            "/views/xml/orders/{order_id}/items/{item_code}",
            get(item_xml),
        )
        .route("/views/xml/declared", get(declared_xml))
        .route("/views/text/status", get(status_text))
        .route("/views/text/legacy", get(legacy_text))
        .route("/views/bytes/orders/{order_id}/label", get(label_bytes))
}

/// Text as XML writes it, in an element or an attribute.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

const fn status_word(status: OrderStatus) -> &'static str {
    match status {
        OrderStatus::Draft => "draft",
        OrderStatus::Submitted => "submitted",
    }
}

fn not_found(what: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!("No such {what}.\n"),
    )
        .into_response()
}

/// One order as XML. The document writes the namespace with the prefix
/// `ord` on the order and as the default namespace on its items.
#[must_use]
pub fn order_document(order: &StoredOrder) -> String {
    let mut items = String::new();
    for item in order.items.values() {
        items.push_str(&format!(
            "    <item code=\"{}\" version=\"{}\">\n      <description>{}</description>\n      <quantity>{}</quantity>\n      <length unit=\"in\">{}</length>\n    </item>\n",
            escaped(&item.item_code),
            item.version,
            escaped(&item.description),
            item.quantity,
            format_length_in(item.length_sixteenths),
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ord:order xmlns:ord=\"{ORDERS_NAMESPACE}\" id=\"{}\" status=\"{}\">\n  <ord:customer>{}</ord:customer>\n  <items xmlns=\"{ORDERS_NAMESPACE}\">\n{items}  </items>\n</ord:order>\n",
        escaped(&order.order_id),
        status_word(order.status),
        escaped(&order.customer),
    )
}

/// One line item as XML.
#[must_use]
pub fn item_document(order_id: &str, item: &StoredItem) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<o:item xmlns:o=\"{ORDERS_NAMESPACE}\" order=\"{}\" code=\"{}\" version=\"{}\"><o:description>{}</o:description><o:quantity>{}</o:quantity><o:length unit=\"in\">{}</o:length></o:item>\n",
        escaped(order_id),
        escaped(&item.item_code),
        item.version,
        escaped(&item.description),
        item.quantity,
        format_length_in(item.length_sixteenths),
    )
}

async fn order_xml(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    match inner.store.order(&order_id) {
        Some(order) => ([(header::CONTENT_TYPE, XML)], order_document(order)).into_response(),
        None => not_found("order"),
    }
}

async fn item_xml(
    State(state): State<AppState>,
    Path((order_id, item_code)): Path<(String, String)>,
) -> Response {
    let inner = state.lock().await;
    match inner.store.item(&order_id, &item_code) {
        Some(item) => (
            [
                (header::CONTENT_TYPE, XML.to_owned()),
                (header::ETAG, format!("\"{}\"", item.version)),
            ],
            item_document(&order_id, item),
        )
            .into_response(),
        None => not_found("line item"),
    }
}

/// An order as XML whose document type defines entities, one built of
/// another: a reader that expanded them would read the customer's name
/// many times over.
async fn declared_xml(State(state): State<AppState>) -> Response {
    let inner = state.lock().await;
    let customer = inner
        .store
        .order("A-100")
        .map(|order| escaped(&order.customer))
        .unwrap_or_default();
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE order [\n  <!ENTITY who \"{customer}\">\n  <!ENTITY many \"&who;&who;&who;&who;&who;&who;&who;&who;\">\n]>\n<order xmlns=\"{ORDERS_NAMESPACE}\" id=\"A-100\"><customer>&many;</customer></order>\n"
    );
    ([(header::CONTENT_TYPE, XML)], document).into_response()
}

/// The line a status page shows: counts of the orders, and the newest.
#[must_use]
pub fn status_line(orders: &[&StoredOrder]) -> String {
    let draft = orders
        .iter()
        .filter(|order| order.status == OrderStatus::Draft)
        .count();
    let newest = orders.last();
    format!(
        "example order entry: {} orders, {draft} draft, {} submitted; newest {} for {}\n",
        orders.len(),
        orders.len() - draft,
        newest.map_or("none", |order| order.order_id.as_str()),
        newest.map_or("nobody", |order| order.customer.as_str()),
    )
}

async fn status_text(State(state): State<AppState>) -> Response {
    let inner = state.lock().await;
    let orders: Vec<&StoredOrder> = inner.store.orders().collect();
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        status_line(&orders),
    )
        .into_response()
}

/// The status line as an old device sends it: in ISO-8859-1, with no
/// character set named, and a character outside ASCII in it.
async fn legacy_text(State(state): State<AppState>) -> Response {
    let inner = state.lock().await;
    let count = inner.store.orders().count();
    let line = format!("orders: {count} \u{b7} status: ready\n");
    // Every character of the line is in ISO-8859-1, so each is one byte.
    let bytes: Vec<u8> = line
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect();
    ([(header::CONTENT_TYPE, "text/plain")], bytes).into_response()
}

/// A small binary label for an order: a four-byte mark, the order's
/// identifier and a zero byte, the number of its lines, then for each line
/// its code and a zero byte and its quantity as two bytes, high first.
#[must_use]
pub fn label(order: &StoredOrder) -> Vec<u8> {
    let mut bytes = b"EXL1".to_vec();
    bytes.extend_from_slice(order.order_id.as_bytes());
    bytes.push(0);
    bytes.push(u8::try_from(order.items.len()).unwrap_or(u8::MAX));
    for item in order.items.values() {
        bytes.extend_from_slice(item.item_code.as_bytes());
        bytes.push(0);
        let quantity = u16::try_from(item.quantity).unwrap_or(u16::MAX);
        bytes.extend_from_slice(&quantity.to_be_bytes());
    }
    bytes
}

async fn label_bytes(State(state): State<AppState>, Path(order_id): Path<String>) -> Response {
    let inner = state.lock().await;
    match inner.store.order(&order_id) {
        Some(order) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            label(order),
        )
            .into_response(),
        None => not_found("order"),
    }
}
