//! The OpenAPI 3.1 document, generated from the registry's route faces and
//! nothing else: one operation per route, its input and output schemas from
//! the action, and the response shape shared by all.

use a2m_actions::{ActionRegistry, REST_PREFIX, RestRouteFace, rest_routes};
use serde_json::{Map, Value as Json, json};

use crate::events::RUN_EVENTS_PATH;

/// The path of the document itself, served by this crate and not an action.
pub const OPENAPI_PATH: &str = "/api/v1/openapi.json";

/// The version of the OpenAPI specification the document follows.
pub const OPENAPI_VERSION: &str = "3.1.0";

/// The document for every REST route of the registry, for the route of a
/// run's events, and for the document's own route.
#[must_use]
pub fn openapi(registry: &ActionRegistry) -> Json {
    document(&rest_routes(registry))
}

pub(crate) fn document(routes: &[RestRouteFace]) -> Json {
    let mut paths: Map<String, Json> = Map::new();
    for route in routes {
        let item = paths
            .entry(route.path.clone())
            .or_insert_with(|| Json::Object(Map::new()));
        if let Json::Object(item) = item {
            item.insert(route.method.to_ascii_lowercase(), operation(route));
        }
    }
    // The one route beside the document's own that no action generates.
    let events = paths
        .entry(RUN_EVENTS_PATH.to_owned())
        .or_insert_with(|| Json::Object(Map::new()));
    if let (Json::Object(item), Json::Object(described)) =
        (events, crate::events::operation(envelope))
    {
        item.extend(described);
    }
    paths.insert(
        OPENAPI_PATH.to_owned(),
        json!({"get": {
            "operationId": "openapi_document",
            "summary": "This document: every route of the REST interface, generated from the action registry.",
            "responses": {"200": {
                "description": "The document.",
                "content": {"application/json": {"schema": {"type": "object"}}},
            }},
        }}),
    );
    json!({
        "openapi": OPENAPI_VERSION,
        "jsonSchemaDialect": "https://json-schema.org/draft/2020-12/schema",
        "info": {
            "title": "Anything2MCP REST interface",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Every route is generated from an action of the registry. The body of every answer is the whole response: ok, action, then result, warnings, and next, or error.",
        },
        "servers": [{"url": "/"}],
        "paths": paths,
        "components": {"schemas": {
            "Error": {
                "type": "object",
                "required": ["class", "code", "message", "fix", "retry_safe"],
                "properties": {
                    "class": {"type": "string"},
                    "code": {"type": "string"},
                    "message": {"type": "string"},
                    "fix": {"type": "string"},
                    "retry_safe": {"type": "boolean"},
                },
            },
            "Next": {
                "type": "object",
                "required": ["action", "args", "when"],
                "properties": {
                    "action": {"type": "string"},
                    "args": {"type": "object"},
                    "when": {"type": "string"},
                },
            },
            "Failed": envelope(None),
        }},
    })
}

/// The operation identifier of a route: the action, and whether it is the
/// alias.
#[must_use]
pub fn operation_id(route: &RestRouteFace) -> String {
    let base = route.action.replace('.', "_");
    if route.alias {
        format!("{base}_alias")
    } else {
        base
    }
}

fn operation(route: &RestRouteFace) -> Json {
    let properties = route
        .input_schema
        .get("properties")
        .and_then(Json::as_object)
        .cloned()
        .unwrap_or_default();
    let required: Vec<&str> = route
        .input_schema
        .get("required")
        .and_then(Json::as_array)
        .map(|list| list.iter().filter_map(Json::as_str).collect())
        .unwrap_or_default();
    let mut parameters = Vec::new();
    for field in &route.path_fields {
        parameters.push(json!({
            "name": field,
            "in": "path",
            "required": true,
            "schema": properties.get(field).cloned().unwrap_or_else(|| json!({"type": "string"})),
        }));
    }
    let mut op = Map::new();
    op.insert("operationId".to_owned(), Json::String(operation_id(route)));
    op.insert("summary".to_owned(), Json::String(route.summary.clone()));
    op.insert("x-action".to_owned(), Json::String(route.action.clone()));
    // Whether the clients have a method for the action: the generators read
    // this, not the routes alone, so an action without an SDK face, or with
    // no handler yet, gets no method (`RestRouteFace::sdk`; decisions 0420
    // and 0437).
    op.insert("x-sdk".to_owned(), Json::Bool(route.sdk));
    if route.method == "GET" {
        for (name, schema) in &properties {
            if route.path_fields.contains(name) {
                continue;
            }
            parameters.push(json!({
                "name": name,
                "in": "query",
                "required": required.contains(&name.as_str()),
                "schema": schema,
            }));
        }
    } else {
        parameters.push(json!({
            "name": "Idempotency-Key",
            "in": "header",
            "required": false,
            "description": "Makes a repeated write the same write.",
            "schema": {"type": "string", "minLength": 1, "maxLength": 200},
        }));
        let mut body = strip(&route.input_schema);
        if let Json::Object(body) = &mut body {
            if let Some(Json::Object(props)) = body.get_mut("properties") {
                for field in &route.path_fields {
                    props.remove(field);
                }
            }
            if let Some(Json::Array(list)) = body.get_mut("required") {
                list.retain(|name| {
                    !route
                        .path_fields
                        .iter()
                        .any(|field| name.as_str() == Some(field))
                });
            }
        }
        let body_required = body
            .get("required")
            .and_then(Json::as_array)
            .is_some_and(|list| !list.is_empty());
        op.insert(
            "requestBody".to_owned(),
            json!({
                "required": body_required,
                "content": {"application/json": {"schema": body}},
            }),
        );
    }
    if !parameters.is_empty() {
        op.insert("parameters".to_owned(), Json::Array(parameters));
    }
    let answered = envelope(Some(strip(&route.output_schema)));
    op.insert(
        "responses".to_owned(),
        json!({
            "200": {
                "description": "The action did what was asked.",
                "content": {"application/json": {"schema": answered}},
            },
            "202": {
                "description": "The run rests until a person answers: a sign-in, an approval, or presence.",
                "content": {"application/json": {"schema": answered}},
            },
            "default": {
                "description": "A failure: its class, code, message, and fix.",
                "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Failed"}}},
            },
        }),
    );
    Json::Object(op)
}

/// The shape of every answer, with the action's output as its result.
fn envelope(result: Option<Json>) -> Json {
    let mut properties = json!({
        "ok": {"type": "boolean"},
        "action": {"type": "string"},
        "error": {"$ref": "#/components/schemas/Error"},
        "warnings": {"type": "array", "items": {"type": "string"}},
        "next": {"$ref": "#/components/schemas/Next"},
        "decision": {"type": "string"},
    });
    if let (Some(result), Json::Object(props)) = (result, &mut properties) {
        props.insert("result".to_owned(), result);
    }
    json!({
        "type": "object",
        "required": ["ok", "action", "warnings"],
        "properties": properties,
    })
}

/// A schema without its own `$schema` line: the document names the dialect
/// once.
fn strip(schema: &Json) -> Json {
    let mut schema = schema.clone();
    if let Json::Object(map) = &mut schema {
        map.remove("$schema");
    }
    schema
}

/// Whether a path is under the interface's prefix.
pub(crate) fn under_prefix(path: &str) -> bool {
    path.starts_with(REST_PREFIX)
}
