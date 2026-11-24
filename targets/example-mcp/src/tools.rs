//! The tools the stand offers over the `example` orders: their definitions as
//! listed, with any drift a test switched on, and what a call to each does.

use serde_json::{Map, Value, json};

use crate::faults::{Drift, Settings};
use crate::state::State;
use crate::store::{INSTRUCTION_TEXT, OrderStatus, StoredOrder, WriteAction, WriteRecord};

/// The tools listed when no drift is set, in the order they are listed.
pub const TOOLS: [&str; 7] = [
    "orders_get",
    "orders_list",
    "orders_search",
    "orders_create",
    "orders_update",
    "orders_report",
    "orders_delete",
];

/// The tool the `tool_added` drift brings.
pub const ADDED_TOOL: &str = "orders_archive";

fn order_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "order_id": {"type": "string"},
            "customer": {"type": "string"},
            "status": {"type": "string", "enum": ["draft", "submitted", "archived"]},
            "note": {"type": "string"},
            "reference": {"type": ["string", "null"]},
            "version": {"type": "integer"}
        },
        "required": ["order_id", "customer", "status", "note", "reference", "version"],
        "additionalProperties": false
    })
}

fn input(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn definition(name: &str) -> Option<Value> {
    let order_id = json!({"order_id": {"type": "string"}});
    Some(match name {
        "orders_get" => json!({
            "name": name,
            "title": "Get an order",
            "description": "Returns one order by its identifier.",
            "inputSchema": input(order_id, &["order_id"]),
            "outputSchema": order_schema(),
            "annotations": {"readOnlyHint": true}
        }),
        "orders_list" => json!({
            "name": name,
            "title": "List orders",
            "description": "Lists orders, all of them or those with a status or a reference.",
            "inputSchema": input(json!({
                "status": {"type": "string", "enum": ["draft", "submitted", "archived"]},
                "reference": {"type": "string"}
            }), &[]),
            "outputSchema": {
                "type": "object",
                "properties": {"orders": {"type": "array", "items": order_schema()}},
                "required": ["orders"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true}
        }),
        "orders_search" => json!({
            "name": name,
            "title": "Search orders",
            "description": "Finds orders whose customer or note contains the text.",
            "inputSchema": input(json!({"query": {"type": "string"}}), &["query"]),
            "annotations": {"readOnlyHint": true}
        }),
        "orders_create" => json!({
            "name": name,
            "title": "Create an order",
            "description": "Creates a draft order and returns the identifier it was given.",
            "inputSchema": input(json!({
                "customer": {"type": "string"},
                "note": {"type": "string"},
                "reference": {"type": "string"}
            }), &["customer"]),
            "outputSchema": {
                "type": "object",
                "properties": {"order_id": {"type": "string"}, "version": {"type": "integer"}},
                "required": ["order_id", "version"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false}
        }),
        "orders_update" => json!({
            "name": name,
            "title": "Update an order",
            "description": "Changes one field of a draft order.",
            "inputSchema": input(json!({
                "order_id": {"type": "string"},
                "field": {"type": "string", "enum": ["customer", "note", "reference"]},
                "value": {"type": "string"},
                "expected_version": {"type": "integer"}
            }), &["order_id", "field", "value"]),
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true}
        }),
        "orders_report" => json!({
            "name": name,
            "title": "Report on orders",
            "description": "Counts the orders by status. Slow.",
            "inputSchema": input(json!({}), &[]),
            "outputSchema": {
                "type": "object",
                "properties": {
                    "draft": {"type": "integer"},
                    "submitted": {"type": "integer"},
                    "archived": {"type": "integer"}
                },
                "required": ["draft", "submitted", "archived"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true}
        }),
        "orders_delete" => json!({
            "name": name,
            "title": "Delete an order",
            "description": "Deletes an order. This system always refuses.",
            "inputSchema": input(order_id, &["order_id"]),
            "annotations": {"readOnlyHint": false, "destructiveHint": true}
        }),
        ADDED_TOOL => json!({
            "name": name,
            "title": "Archive an order",
            "description": "Archives an order, whatever its status.",
            "inputSchema": input(order_id, &["order_id"]),
            "annotations": {"readOnlyHint": false, "destructiveHint": true}
        }),
        _ => return None,
    })
}

fn add_required(schema: &mut Value, property: &str) {
    if !schema.is_object() {
        *schema = json!({"type": "object", "properties": {}, "required": []});
    }
    schema["properties"][property] = json!({"type": "string"});
    match schema["required"].as_array_mut() {
        Some(required) => required.push(json!(property)),
        None => schema["required"] = json!([property]),
    }
}

fn apply(drift: &Drift, tool: &mut Value) {
    let name = tool["name"].as_str().unwrap_or_default().to_owned();
    match drift {
        Drift::InputSchemaChanged { tool: t } if *t == name => {
            add_required(&mut tool["inputSchema"], "reason");
        }
        Drift::OutputSchemaChanged { tool: t } if *t == name => {
            add_required(&mut tool["outputSchema"], "region");
        }
        Drift::AnnotationsChanged { tool: t } if *t == name => {
            let was = tool["annotations"]["readOnlyHint"]
                .as_bool()
                .unwrap_or(false);
            tool["annotations"]["readOnlyHint"] = json!(!was);
        }
        Drift::TextRows { tool: t } if *t == name => {
            if let Some(definition) = tool.as_object_mut() {
                definition.remove("outputSchema");
            }
        }
        Drift::DescriptionChanged { tool: t } if *t == name => {
            let was = tool["description"].as_str().unwrap_or_default();
            tool["description"] = json!(format!("{was} {INSTRUCTION_TEXT}"));
        }
        _ => {}
    }
}

/// The tool list as the server reports it now.
pub(crate) fn definitions(settings: &Settings) -> Vec<Value> {
    let removed = |name: &str| {
        settings
            .drift
            .iter()
            .any(|d| matches!(d, Drift::ToolRemoved { tool } if tool == name))
    };
    let added = settings.drift.contains(&Drift::ToolAdded);
    TOOLS
        .iter()
        .copied()
        .chain(added.then_some(ADDED_TOOL))
        .filter(|name| !removed(name))
        .filter_map(definition)
        .map(|mut tool| {
            for drift in &settings.drift {
                apply(drift, &mut tool);
            }
            tool
        })
        .collect()
}

/// What running a tool gave.
pub(crate) struct ToolRun {
    /// The tool result as the protocol carries it.
    pub(crate) result: Value,
    /// Whether it is an error result.
    pub(crate) is_error: bool,
    /// How long the tool takes, in milliseconds.
    pub(crate) delay_ms: u64,
}

fn text_result(text: String) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": false})
}

/// A structured result also carries its content as text, for clients that
/// read only text.
fn structured_result(structured: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": structured.to_string()}],
        "structuredContent": structured,
        "isError": false
    })
}

fn order_json(order: &StoredOrder) -> Value {
    json!({
        "order_id": order.order_id,
        "customer": order.customer,
        "status": order.status.name(),
        "note": order.note,
        "reference": order.reference,
        "version": order.version
    })
}

fn optional<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(format!("The argument {key} must be a string.")),
    }
}

fn required<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    optional(args, key)?.ok_or_else(|| format!("The argument {key} is required."))
}

fn written(state: &mut State, call: u64, order_id: &str, action: WriteAction, field: Option<&str>) {
    state.truth.writes.push(WriteRecord {
        call,
        order_id: order_id.to_owned(),
        action,
        field: field.map(str::to_owned),
    });
}

/// Runs the tool. The caller has checked that the tool is listed.
fn run_listed(
    state: &mut State,
    call: u64,
    name: &str,
    args: &Map<String, Value>,
) -> Result<(Value, u64), String> {
    let reason_needed = state
        .settings
        .drift
        .iter()
        .any(|d| matches!(d, Drift::InputSchemaChanged { tool } if tool == name));
    if reason_needed {
        required(args, "reason")?;
    }
    let store = &mut state.truth.store;
    match name {
        "orders_get" => {
            let order_id = required(args, "order_id")?;
            let order = store
                .orders
                .get(order_id)
                .ok_or_else(|| format!("There is no order {order_id}."))?;
            Ok((structured_result(order_json(order)), 0))
        }
        "orders_list" => {
            let status = match optional(args, "status")? {
                Some(name) => Some(
                    OrderStatus::from_name(name)
                        .ok_or_else(|| format!("{name} is not a status."))?,
                ),
                None => None,
            };
            let orders: Vec<Value> = store
                .list(status, optional(args, "reference")?)
                .into_iter()
                .map(order_json)
                .collect();
            Ok((structured_result(json!({"orders": orders})), 0))
        }
        "orders_search" => {
            let found: Vec<Value> = store
                .search(required(args, "query")?)
                .into_iter()
                .map(order_json)
                .collect();
            Ok((text_result(Value::Array(found).to_string()), 0))
        }
        "orders_create" => {
            let created = store.create(
                required(args, "customer")?,
                optional(args, "note")?.unwrap_or_default(),
                optional(args, "reference")?,
            )?;
            written(state, call, &created.order_id, WriteAction::Created, None);
            let answer = json!({"order_id": created.order_id, "version": created.version});
            Ok((structured_result(answer), 0))
        }
        "orders_update" => {
            let order_id = required(args, "order_id")?;
            let field = required(args, "field")?;
            let expected = match args.get("expected_version") {
                None | Some(Value::Null) => None,
                Some(version) => Some(
                    version
                        .as_u64()
                        .ok_or("The argument expected_version must be a whole number.")?,
                ),
            };
            let updated = store.update(order_id, field, required(args, "value")?, expected)?;
            written(state, call, order_id, WriteAction::Updated, Some(field));
            Ok((
                text_result(format!(
                    "Order {order_id} updated; it is now at version {}.",
                    updated.version
                )),
                0,
            ))
        }
        "orders_report" => {
            let count = |status| store.list(Some(status), None).len();
            let report = json!({
                "draft": count(OrderStatus::Draft),
                "submitted": count(OrderStatus::Submitted),
                "archived": count(OrderStatus::Archived)
            });
            Ok((structured_result(report), state.settings.slow_ms))
        }
        "orders_delete" => {
            required(args, "order_id")?;
            Err("Orders cannot be deleted in this system.".to_owned())
        }
        ADDED_TOOL => {
            let order_id = required(args, "order_id")?;
            store.archive(order_id)?;
            written(state, call, order_id, WriteAction::Archived, None);
            Ok((text_result(format!("Order {order_id} archived.")), 0))
        }
        other => Err(format!("The tool {other} does nothing.")),
    }
}

/// A structured result as the `text_rows` drift has a tool answer: JSON as
/// text with no structured content, one content item for each row of a
/// list (a result whose one member is a list), the record as one item, and
/// one empty text for a record the stand does not hold.
fn as_text_rows(result: &Value) -> Value {
    let item = |value: &Value| json!({"type": "text", "text": value.to_string()});
    let content: Vec<Value> = match result.get("structuredContent") {
        Some(Value::Object(members)) if members.len() == 1 => match members.values().next() {
            Some(Value::Array(rows)) => rows.iter().map(item).collect(),
            _ => vec![item(&Value::Object(members.clone()))],
        },
        Some(other) => vec![item(other)],
        None => return result.clone(),
    };
    json!({"content": content, "isError": false})
}

/// Runs a listed tool. A refusal is an error result, and stores nothing.
pub(crate) fn run(state: &mut State, call: u64, name: &str, args: &Map<String, Value>) -> ToolRun {
    let text_rows = state
        .settings
        .drift
        .iter()
        .any(|d| matches!(d, Drift::TextRows { tool } if tool == name));
    if text_rows && name == "orders_get" {
        let held = required(args, "order_id")
            .is_ok_and(|order_id| state.truth.store.orders.contains_key(order_id));
        if !held {
            return ToolRun {
                result: json!({"content": [{"type": "text", "text": ""}], "isError": false}),
                is_error: false,
                delay_ms: 0,
            };
        }
    }
    let ran = run_listed(state, call, name, args);
    let ran = if text_rows {
        ran.map(|(result, delay)| (as_text_rows(&result), delay))
    } else {
        ran
    };
    match ran {
        Ok((result, delay_ms)) => ToolRun {
            result,
            is_error: false,
            delay_ms,
        },
        Err(message) => ToolRun {
            result: json!({"content": [{"type": "text", "text": message}], "isError": true}),
            is_error: true,
            delay_ms: 0,
        },
    }
}
