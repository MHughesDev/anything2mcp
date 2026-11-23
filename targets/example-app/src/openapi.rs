//! Case 30: a served description of the JSON interface, an OpenAPI 3.1 document at
//! `/api/openapi.json`. It is written by hand; a test holds it to the routes the
//! application serves, in both directions.

use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;

/// Where the description is served.
pub(crate) const PATH: &str = "/api/openapi.json";

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route(PATH, get(|| async { Json(document()) }))
}

/// The description of the copy served under `/secure/bearer`, at `PATH`
/// under it: the same document, which states that every operation takes
/// the bearer token. It is served openly; the operations are not.
pub(crate) fn bearer_routes() -> Router<AppState> {
    Router::new().route(
        PATH,
        get(|| async {
            let mut described = document();
            described["security"] = json!([{"bearer": []}]);
            Json(described)
        }),
    )
}

fn param(name: &str, example: &str) -> Value {
    json!({"name": name, "in": "path", "required": true, "schema": {"type": "string"}, "example": example})
}

fn answer(description: &str, schema: &str) -> Value {
    json!({"description": description, "content": {"application/json": {"schema": {"$ref": format!("#/components/schemas/{schema}")}}}})
}

fn body(schema: &str) -> Value {
    json!({"required": true, "content": {"application/json": {"schema": {"$ref": format!("#/components/schemas/{schema}")}}}})
}

/// The whole document.
fn document() -> Value {
    let order = param("order_id", "A-100");
    let item = param("item_code", "L-101");
    let not_found = answer("No such record", "Refusal");
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Example Orders",
            "version": "1",
            "description": "The JSON interface of the test application. The same routes are also served signed in: under /secure with a session cookie, under /secure/bearer with a bearer token, and under /secure/key with an API key in the X-Api-Key header."
        },
        "paths": {
            "/api/orders": {"get": {
                "operationId": "list_orders",
                "responses": {"200": {"description": "Every order", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/OrderSummary"}}}}}}
            }},
            "/api/orders/{order_id}": {"get": {
                "operationId": "get_order",
                "parameters": [order],
                "responses": {"200": answer("The order with its line items", "Order"), "404": not_found}
            }},
            "/api/orders/{order_id}/items/{item_code}": {
                "get": {
                    "operationId": "get_item",
                    "parameters": [order, item],
                    "responses": {"200": answer("The line item; its version is also in the ETag header", "Item"), "404": not_found}
                },
                "put": {
                    "operationId": "put_item",
                    "parameters": [order, item],
                    "requestBody": body("ItemInput"),
                    "responses": {
                        "200": answer("Updated", "Item"),
                        "201": answer("Created", "Item"),
                        "400": answer("The body is not a line item", "Refusal"),
                        "404": not_found,
                        "409": answer("The order is submitted, or the version is stale", "Refusal"),
                        "422": answer("One or more fields are not acceptable; nothing was stored", "Invalid")
                    }
                }
            },
            "/api/orders/{order_id}/totals": {"get": {
                "operationId": "order_totals",
                "parameters": [order],
                "responses": {"200": answer("Totals; slow on purpose", "Totals"), "404": not_found}
            }},
            "/api/stock/{item_code}": {"get": {
                "operationId": "stock",
                "parameters": [item],
                "responses": {"200": answer("Stock", "Stock"), "503": answer("Every third call fails", "Refusal")}
            }},
            "/api/orders/{order_id}/shipment": {"get": {
                "operationId": "get_shipment",
                "parameters": [order],
                "responses": {"200": answer("The shipment; its carrier is optional", "Shipment"), "404": not_found}
            }},
            "/api/orders/{order_id}/delivery": {
                "get": {
                    "operationId": "get_delivery",
                    "parameters": [order],
                    "responses": {"200": answer("The delivery details", "Delivery"), "404": not_found}
                },
                "put": {
                    "operationId": "put_delivery",
                    "parameters": [order],
                    "requestBody": body("DeliveryInput"),
                    "responses": {
                        "200": answer("Updated", "Delivery"),
                        "400": answer("The body is not delivery details", "Refusal"),
                        "404": not_found,
                        "409": answer("The order is submitted, or the version is stale", "Refusal"),
                        "422": answer("One or more fields are not acceptable; nothing was stored", "Invalid")
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Refusal": {"type": "object", "required": ["error", "message"], "properties": {
                    "error": {"type": "string"}, "message": {"type": "string"}, "current_version": {"type": "integer"}}},
                "Invalid": {"type": "object", "required": ["error", "message", "problems"], "properties": {
                    "error": {"const": "validation"}, "message": {"type": "string"},
                    "problems": {"type": "array", "items": {"type": "object", "required": ["field", "message"], "properties": {
                        "field": {"type": "string"}, "message": {"type": "string"}}}}}},
                "OrderSummary": {"type": "object", "required": ["order_id", "customer", "status", "lines"], "properties": {
                    "order_id": {"type": "string"}, "customer": {"type": "string"},
                    "status": {"enum": ["draft", "submitted"]}, "lines": {"type": "integer"}}},
                "Order": {"type": "object", "required": ["order_id", "customer", "status", "items"], "properties": {
                    "order_id": {"type": "string"}, "customer": {"type": "string"},
                    "status": {"enum": ["draft", "submitted"]},
                    "items": {"type": "array", "items": {"$ref": "#/components/schemas/Item"}}}},
                "Item": {"type": "object", "required": ["order_id", "item_code", "description", "quantity", "length_in", "version"], "properties": {
                    "order_id": {"type": "string"}, "item_code": {"type": "string"}, "description": {"type": "string"},
                    "quantity": {"type": "integer"}, "length_in": {"type": "string", "description": "Inches as decimal text, a whole number of sixteenths"},
                    "version": {"type": "integer"}}},
                "ItemInput": {"type": "object", "additionalProperties": false, "required": ["description", "quantity", "length_in"], "properties": {
                    "description": {"type": "string", "maxLength": 80}, "quantity": {"type": "integer", "minimum": 1, "maximum": 9999},
                    "length_in": {"type": "string"}, "version": {"type": "integer", "description": "Required on an update; left out on a create"}}},
                "Totals": {"type": "object", "required": ["order_id", "lines", "total_quantity", "total_length_in"], "properties": {
                    "order_id": {"type": "string"}, "lines": {"type": "integer"}, "total_quantity": {"type": "integer"}, "total_length_in": {"type": "string"}}},
                "Shipment": {"type": "object", "required": ["order_id"], "properties": {
                    "order_id": {"type": "string"},
                    "carrier": {"type": "object", "required": ["name", "tracking"], "properties": {
                        "name": {"type": "string"}, "tracking": {"type": "string"}}}}},
                "Stock": {"type": "object", "required": ["item_code", "in_stock"], "properties": {
                    "item_code": {"type": "string"}, "in_stock": {"type": "integer"}}},
                "Delivery": {"type": "object", "required": ["order_id", "shipping", "gift_wrap", "priority", "deliver_on", "notes", "version"], "properties": {
                    "order_id": {"type": "string"}, "shipping": {"enum": ["standard", "express", "freight"]}, "gift_wrap": {"type": "boolean"},
                    "priority": {"enum": ["low", "normal", "high"]}, "deliver_on": {"type": "string", "format": "date"},
                    "notes": {"type": "string"}, "version": {"type": "integer"}}},
                "DeliveryInput": {"type": "object", "additionalProperties": false, "required": ["shipping", "gift_wrap", "priority", "deliver_on", "notes", "version"], "properties": {
                    "shipping": {"enum": ["standard", "express", "freight"]}, "gift_wrap": {"type": "boolean"},
                    "priority": {"enum": ["low", "normal", "high"]}, "deliver_on": {"type": "string", "format": "date"},
                    "notes": {"type": "string", "maxLength": 500}, "version": {"type": "integer"}}}
            },
            "securitySchemes": {
                "session": {"type": "apiKey", "in": "cookie", "name": "example_session"},
                "bearer": {"type": "http", "scheme": "bearer"},
                "key": {"type": "apiKey", "in": "header", "name": "X-Api-Key"}
            }
        }
    })
}
