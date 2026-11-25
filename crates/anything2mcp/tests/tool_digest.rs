//! Package YL2 through the product (decisions 0325 and 0439): what an MCP
//! client reads of a published tool. Its definition in `tools/list`
//! carries `anything2mcp/digest` in its `_meta`, the SHA-256 of its name,
//! description, input schema, output schema, and annotations in canonical
//! JSON, and `a2m_tool_describe`, called through `tools/call`, gives the
//! same digest and the output schema, in which the output the compiled
//! target marks untrusted carries `x-anything2mcp-untrusted: true` and the
//! one it does not mark carries nothing (spec section 502). The digest
//! changes when, and only when, the tool's definition changes: a version
//! published again from the same files keeps it, and a changed description
//! changes it, as `diff` says.
//!
//! The target is the order system's JSON interface, published from its
//! fixtures: nothing is sent to any system. The MCP door is the product's
//! own, served in this process as `serve` serves it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use common::{Machine, World};
use serde_json::{Value as Json, json};

/// The read whose definition is followed.
const TOOL: &str = "example_order_get";

/// The customer's name is free text the order system gives, so its field
/// type is marked untrusted; the status is a word and is not.
const TYPES_YAML: &str = "types:
  customer_name:
    base: text
    untrusted: true
";

/// One MCP message to the `use` toolset's door, every tool listed, as an
/// agent at this machine sends it: the answer's `result`.
async fn mcp(world: &World, id: u64, method: &str, params: Json) -> Json {
    use a2m_mcp::{Listing, McpService, McpSettings};
    let settings = McpSettings {
        listing: Listing::Full,
        ..McpSettings::local()
    };
    let service = McpService::new(
        world.engine.kernel().clone(),
        a2m_policy::ToolsetName::new("use").unwrap(),
        settings,
    )
    .unwrap();
    let revision = "2026-07-28";
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": revision,
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "digest", "version": "0"},
    });
    let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    let mut request = axum::http::Request::post("/mcp/use")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", revision)
        .header("mcp-method", method);
    if method == "tools/call" {
        request = request.header("mcp-name", params["name"].as_str().unwrap());
    }
    let mut request = request
        .body(axum::body::Body::from(message.to_string()))
        .unwrap();
    request
        .extensions_mut()
        .insert(world.engine.caller(a2m_actions::FrontDoor::Mcp, true));
    let response = service.handle(request).await;
    assert_eq!(response.status().as_u16(), 200);
    let body = axum::body::to_bytes(response.into_body(), 1 << 24)
        .await
        .unwrap();
    let answer: Json = serde_json::from_slice(&body).unwrap();
    assert!(answer.get("error").is_none(), "{answer}");
    answer["result"].clone()
}

/// The tool's definition as `tools/list` gives it.
async fn listed(world: &World) -> Json {
    let list = mcp(world, 1, "tools/list", json!({})).await;
    list["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("{list}"))
        .iter()
        .find(|tool| tool["name"] == TOOL)
        .unwrap_or_else(|| panic!("{TOOL} is not listed: {list}"))
        .clone()
}

/// What `a2m_tool_describe` gives for the tool, through `tools/call`.
async fn described(world: &World) -> Json {
    let called = mcp(
        world,
        2,
        "tools/call",
        json!({"name": "a2m_tool_describe", "arguments": {"tool": TOOL}}),
    )
    .await;
    assert_ne!(called["isError"], true, "{called}");
    called["structuredContent"].clone()
}

/// The tool's digest, as the list and the description both give it: the
/// two are one value, of 64 lowercase hexadecimal characters.
async fn digest(world: &World) -> String {
    let listed = listed(world).await;
    let digest = listed["_meta"]["anything2mcp/digest"]
        .as_str()
        .unwrap_or_else(|| panic!("no digest in the tool's `_meta`: {listed}"))
        .to_owned();
    assert_eq!(digest.len(), 64, "{digest}");
    assert!(
        digest
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "{digest}"
    );
    assert_eq!(described(world).await["digest"], digest.as_str());
    digest
}

async fn publish(world: &World, version: &str) {
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    assert_eq!(published.result()["version"], version);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tools_digest_changes_with_its_definition_alone_and_an_untrusted_output_is_marked() {
    let machine = Machine::new(
        "orders",
        &[("customer: {type: text}", "customer: {type: customer_name}")],
    );
    let target = machine.project.path().join("targets").join("example");
    std::fs::write(target.join("types.yaml"), TYPES_YAML).unwrap();
    let world = machine.open().await;
    publish(&world, "1.0.0").await;

    // The output the compiled target marks untrusted carries the mark in
    // the served schema; the one it does not mark carries none.
    let schema = described(&world).await["output_schema"].clone();
    let customer = &schema["properties"]["customer"];
    assert_eq!(customer["x-anything2mcp-untrusted"], true, "{schema}");
    let status = &schema["properties"]["status"];
    assert!(status.get("x-anything2mcp-untrusted").is_none(), "{schema}");
    assert!(
        !schema
            .to_string()
            .contains("\"x-anything2mcp-untrusted\":false"),
        "{schema}"
    );
    let first = digest(&world).await;

    // The same files published again under the next version: the version
    // differs and the definition does not, so the digest is the same.
    publish(&world, "2.0.0").await;
    let listed_again = listed(&world).await;
    assert_eq!(listed_again["_meta"]["anything2mcp/version"], "2.0.0");
    assert_eq!(digest(&world).await, first);

    // A changed description changes the digest, and `diff` says it is the
    // description that changed.
    let operations = target.join("operations").join("orders.yaml");
    let text = std::fs::read_to_string(&operations).unwrap();
    let changed = text.replace(
        "Reads the status and the customer of one order.",
        "Reads the status of one order and the name of its customer.",
    );
    assert_ne!(changed, text);
    std::fs::write(&operations, changed).unwrap();
    publish(&world, "3.0.0").await;
    let third = digest(&world).await;
    assert_ne!(third, first);

    let unchanged = world
        .ok(&["diff", "example", "--from", "1.0.0", "--to", "2.0.0"])
        .await;
    let class = |diffed: &Json| {
        diffed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["tool"] == TOOL)
            .unwrap_or_else(|| panic!("{diffed}"))
            .clone()
    };
    assert_eq!(class(&unchanged)["class"], "unchanged", "{unchanged}");
    let diffed = world
        .ok(&["diff", "example", "--from", "2.0.0", "--to", "3.0.0"])
        .await;
    let tool = class(&diffed);
    assert_eq!(tool["class"], "compatible", "{diffed}");
    assert_eq!(
        tool["changes"][0]["change"], "description_changed",
        "{diffed}"
    );
    world.close().await;
}
