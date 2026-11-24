//! "What the server must pass, message by message", from the crate's
//! design: every line that holds on both transports, run with raw messages
//! over the HTTP service in process and through the proxy.

use a2m_actions::mcp_tool;
use serde_json::{Value as Json, json};

use super::engine::{ORDER_FILL, ORDER_GET};
use super::wire::{H, Http, M, SERVER_INFO, Stdio, Wire, h, m, m_at};
use super::world::World;
use crate::McpSettings;

/// Whether `value` fits `schema`, for the parts of JSON Schema the
/// registry's output schemas use: objects with required and no unknown
/// keys, lists, and the scalar types.
pub fn fits(schema: &Json, value: &Json) -> Result<(), String> {
    match schema["type"].as_str() {
        Some("object") => {
            let object = value
                .as_object()
                .ok_or(format!("{value} is not an object"))?;
            for key in schema["required"].as_array().into_iter().flatten() {
                let key = key.as_str().unwrap_or_default();
                if !object.contains_key(key) {
                    return Err(format!("`{key}` is missing"));
                }
            }
            for (key, inner) in object {
                match schema["properties"].get(key) {
                    Some(property) => {
                        fits(property, inner).map_err(|why| format!("{key}: {why}"))?;
                    }
                    None if schema["additionalProperties"] == false => {
                        return Err(format!("`{key}` is not in the schema"));
                    }
                    None => {}
                }
            }
            Ok(())
        }
        Some("array") => value
            .as_array()
            .ok_or(format!("{value} is not a list"))?
            .iter()
            .try_for_each(|item| fits(&schema["items"], item)),
        Some("string") => value
            .is_string()
            .then_some(())
            .ok_or(format!("{value} is not text")),
        Some("integer") => value
            .is_i64()
            .then_some(())
            .ok_or(format!("{value} is not a whole number")),
        Some("boolean") => value
            .is_boolean()
            .then_some(())
            .ok_or(format!("{value} is not true or false")),
        _ => Ok(()),
    }
}

fn error_code(answer: &Json) -> i64 {
    answer["error"]["code"]
        .as_i64()
        .unwrap_or_else(|| panic!("not an error: {answer}"))
}

/// A list's result: every tool has what a client needs to choose it.
fn check_list(result: &Json, modern: bool) {
    let tools = result["tools"].as_array().expect("tools");
    assert!(!tools.is_empty());
    for tool in tools {
        assert!(tool["name"].is_string(), "{tool}");
        assert!(!tool["description"].as_str().unwrap().is_empty(), "{tool}");
        assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
        for hint in ["readOnlyHint", "destructiveHint", "idempotentHint"] {
            assert!(tool["annotations"][hint].is_boolean(), "{tool}");
        }
    }
    assert!(result.get("nextCursor").is_none(), "a single page");
    assert_eq!(result.get("ttlMs").is_some(), modern);
    assert_eq!(result.get("cacheScope").is_some(), modern);
    check_modern(result, modern);
}

/// Every result of the stateless revision says its type and names the
/// server; a result of the handshake revision carries neither.
fn check_modern(result: &Json, modern: bool) {
    if modern {
        assert_eq!(result["resultType"], "complete", "{result}");
        assert_eq!(result["_meta"][SERVER_INFO]["name"], "anything2mcp");
        assert!(result["_meta"][SERVER_INFO]["version"].is_string());
    } else {
        assert!(result.get("resultType").is_none(), "{result}");
        assert!(result["_meta"].get(SERVER_INFO).is_none(), "{result}");
    }
}

/// A successful call's result: structured content that fits the output
/// schema, and one text item holding the same JSON.
fn check_success(world: &World, result: &Json, action: &str) -> Json {
    assert_eq!(result["isError"], false, "{result}");
    let def = world.kernel.actions().find(action).unwrap();
    let structured = result["structuredContent"].clone();
    fits(&mcp_tool(def).output_schema, &structured).unwrap_or_else(|why| panic!("{why}: {result}"));
    let content = result["content"].as_array().unwrap();
    assert_eq!(content.len(), 1, "{result}");
    assert_eq!(content[0]["type"], "text");
    let text: Json = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, structured);
    structured
}

/// A failed call's result: the failure's class, code, message, next step,
/// and whether a retry is safe, as one JSON object in one text item, and
/// nothing else.
fn check_failure(result: &Json, modern: bool, class: &str, code: &str) -> Json {
    assert_eq!(result["isError"], true, "{result}");
    assert!(result.get("structuredContent").is_none(), "{result}");
    let mut keys: Vec<&str> = result
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.retain(|key| !(modern && matches!(*key, "resultType" | "_meta")));
    assert_eq!(keys, ["content", "isError"], "{result}");
    let content = result["content"].as_array().unwrap();
    assert_eq!(content.len(), 1, "{result}");
    let failure: Json = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    let keys: Vec<&str> = failure
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["class", "code", "fix", "message", "retry_safe"]);
    assert_eq!(failure["class"], class, "{failure}");
    assert_eq!(failure["code"], code, "{failure}");
    assert!(!failure["fix"].as_str().unwrap().is_empty());
    failure
}

async fn checklist(world: &World, wire: &mut Wire) {
    let via = wire.name();

    // A body or line that is not JSON.
    let answer = wire.broken(b"{not json").await;
    assert_eq!(error_code(&answer), -32700, "{via}");
    assert_eq!(answer["id"], Json::Null);

    // A message that is not a request: a batch, and an object with no method.
    for bytes in [
        br#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":1}"#.as_slice(),
    ] {
        assert_eq!(error_code(&wire.broken(bytes).await), -32600, "{via}");
    }

    // An unknown method, in either revision.
    assert_eq!(
        error_code(&wire.answer(&h(1, "nope/nope", json!({}))).await),
        -32601
    );
    assert_eq!(
        error_code(&wire.answer(&m(2, "nope/nope", json!({}))).await),
        -32601
    );

    // An unknown notification is answered with nothing.
    wire.tell(&json!({"jsonrpc": "2.0", "method": "notifications/nope"}))
        .await;

    // A request id comes back unchanged, and `ping` is answered before any
    // `initialize`.
    for id in [json!("abc"), json!(7)] {
        let answer = wire.answer(&h(id.clone(), "ping", json!({}))).await;
        assert_eq!(answer["id"], id, "{via}");
        assert_eq!(answer["result"], json!({}));
    }

    // `initialize` is answered with the handshake revision whatever was
    // asked, the capabilities that are served, and the server's name.
    for asked in ["2024-11-05", H, M, "1900-01-01"] {
        let answer = wire
            .answer(&h(
                10,
                "initialize",
                json!({
                    "protocolVersion": asked,
                    "capabilities": {},
                    "clientInfo": {"name": "conformance", "version": "0"},
                }),
            ))
            .await;
        let result = &answer["result"];
        assert_eq!(
            result["protocolVersion"], H,
            "{via} asked {asked}: {answer}"
        );
        let served: Vec<&String> = result["capabilities"].as_object().unwrap().keys().collect();
        assert_eq!(served, ["tools"], "{answer}");
        assert!(result["capabilities"]["tools"].get("listChanged").is_none());
        assert_eq!(result["serverInfo"]["name"], "anything2mcp");
        assert!(!result["serverInfo"]["version"].as_str().unwrap().is_empty());
    }
    wire.tell(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await;

    // `ping` is gone from the stateless revision.
    assert_eq!(
        error_code(&wire.answer(&m(11, "ping", json!({}))).await),
        -32601
    );

    // `server/discover`.
    let answer = wire.answer(&m(20, "server/discover", json!({}))).await;
    let result = &answer["result"];
    assert_eq!(
        result["supportedVersions"],
        json!([M, H]),
        "{via}: {answer}"
    );
    assert!(result["capabilities"]["tools"].is_object());
    assert!(!result["instructions"].as_str().unwrap().is_empty());
    assert!(result["ttlMs"].is_u64());
    assert!(result["cacheScope"].is_string());
    check_modern(result, true);

    // A request that names a revision that is not served.
    let answer = wire
        .answer(&m_at("1900-01-01", 21, "tools/list", json!({})))
        .await;
    assert_eq!(error_code(&answer), -32022, "{via}: {answer}");
    assert_eq!(answer["error"]["data"]["supported"], json!([M, H]));
    assert_eq!(answer["error"]["data"]["requested"], "1900-01-01");

    // A request of the stateless revision that does not declare the
    // client's capabilities.
    let mut bare = m(22, "tools/list", json!({}));
    bare["params"]["_meta"]
        .as_object_mut()
        .unwrap()
        .remove("io.modelcontextprotocol/clientCapabilities");
    assert_eq!(error_code(&wire.answer(&bare).await), -32602, "{via}");

    // `tools/list`, in both revisions: the same tools in the same order.
    let old = wire.answer(&h(30, "tools/list", json!({}))).await;
    check_list(&old["result"], false);
    let new = wire.answer(&m(31, "tools/list", json!({}))).await;
    check_list(&new["result"], true);
    assert_eq!(old["result"]["tools"], new["result"]["tools"], "{via}");
    let again = wire.answer(&h(32, "tools/list", json!({}))).await;
    assert_eq!(old["result"], again["result"]);
    for asked in [
        h(33, "tools/list", json!({"cursor": "made-up"})),
        m(34, "tools/list", json!({"cursor": "made-up"})),
    ] {
        assert_eq!(error_code(&wire.answer(&asked).await), -32602, "{via}");
    }

    // `tools/call` for a known tool, in both revisions: by the control
    // tool, and by the published tool's own name.
    let input = json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}});
    let stored = world.app.stored_order("A-100").await.unwrap().unwrap();
    for (modern, asked) in [
        (
            false,
            h(
                40,
                "tools/call",
                json!({"name": "a2m_tool_call", "arguments": input}),
            ),
        ),
        (
            true,
            m(
                41,
                "tools/call",
                json!({"name": "a2m_tool_call", "arguments": input}),
            ),
        ),
        (
            false,
            h(
                42,
                "tools/call",
                json!({"name": ORDER_GET, "arguments": {"order_id": "A-100"}}),
            ),
        ),
        (
            true,
            m(
                43,
                "tools/call",
                json!({"name": ORDER_GET, "arguments": {"order_id": "A-100"}}),
            ),
        ),
    ] {
        // No progress was asked for, so none is sent.
        let answer = wire.answer(&asked).await;
        let result = &answer["result"];
        check_modern(result, modern);
        let run = check_success(world, result, "tool.call");
        assert_eq!(run["status"], "completed", "{via}: {run}");
        assert_eq!(run["tool"], ORDER_GET);
        // The value is what the test application holds.
        assert_eq!(run["result"]["customer"], stored.customer.as_str());
    }

    // An unknown tool is a protocol error: an action that does not exist,
    // an action this toolset does not list, and a tool nobody published.
    for name in [
        "a2m_nope_nope",
        "a2m_grant_add",
        "example_nope_get",
        "Not A Name",
    ] {
        for asked in [
            h(50, "tools/call", json!({"name": name, "arguments": {}})),
            m(51, "tools/call", json!({"name": name, "arguments": {}})),
        ] {
            let answer = wire.answer(&asked).await;
            assert_eq!(error_code(&answer), -32602, "{via} {name}: {answer}");
        }
    }

    // Input that fails the full schema is a result, not a protocol error.
    for (modern, asked) in [
        (
            false,
            h(
                60,
                "tools/call",
                json!({"name": "a2m_tool_search", "arguments": {"limit": "many"}}),
            ),
        ),
        (
            true,
            m(
                61,
                "tools/call",
                json!({"name": "a2m_tool_search", "arguments": {"unknown": 1}}),
            ),
        ),
    ] {
        let answer = wire.answer(&asked).await;
        check_failure(&answer["result"], modern, "input", "schema_invalid");
    }

    // A call that fails.
    for (modern, asked) in [
        (
            false,
            h(
                62,
                "tools/call",
                json!({"name": "a2m_tool_call", "arguments": {"tool": "example_nope_get"}}),
            ),
        ),
        (
            true,
            m(
                63,
                "tools/call",
                json!({"name": "a2m_tool_call", "arguments": {"tool": "example_nope_get"}}),
            ),
        ),
    ] {
        let answer = wire.answer(&asked).await;
        check_failure(&answer["result"], modern, "input", "not_found");
    }

    // A call with a progress token: progress with that token and a rising
    // count before the result, saying nothing the result does not.
    for (token, asked) in [
        (
            json!("token-one"),
            h(
                70,
                "tools/call",
                json!({"name": "a2m_tool_call", "arguments": input, "_meta": {"progressToken": "token-one"}}),
            ),
        ),
        (
            json!(77),
            m(
                71,
                "tools/call",
                json!({"name": "a2m_tool_call", "arguments": input, "_meta": {"progressToken": 77}}),
            ),
        ),
    ] {
        let mut seen = wire.ask(&asked).await;
        let answer = seen.pop().unwrap();
        check_success(world, &answer["result"], "tool.call");
        assert!(!seen.is_empty(), "{via}: progress was asked for");
        let mut last = 0.0;
        for note in &seen {
            assert_eq!(note["method"], "notifications/progress", "{note}");
            assert_eq!(note["params"]["progressToken"], token);
            let count = note["params"]["progress"].as_f64().unwrap();
            assert!(count > last, "{via}: {seen:?}");
            last = count;
            let mut keys: Vec<&str> = note["params"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.retain(|key| *key != "_meta");
            assert_eq!(keys, ["message", "progress", "progressToken"], "{note}");
            assert!(!note["params"]["message"].as_str().unwrap().is_empty());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_checklist_holds_over_http() {
    let world = World::ready().await;
    let mut wire = Wire::Http(Http::new(world.service("use", McpSettings::local())));
    checklist(&world, &mut wire).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_checklist_holds_through_the_proxy_and_the_proxy_ends_with_its_input() {
    let world = World::ready().await;
    let mut wire = Wire::Stdio(Box::new(Stdio::start(
        world.service("use", McpSettings::local()),
    )));
    checklist(&world, &mut wire).await;
    let Wire::Stdio(stdio) = wire else {
        unreachable!()
    };
    // End of input: nothing more is written, and the proxy ends well.
    let (rest, ended) = stdio.close().await;
    assert!(rest.is_empty(), "{rest:?}");
    ended.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_holds_the_control_tools_and_the_pipelines_and_a_setting_widens_or_narrows_it() {
    let world = World::ready().await;
    let names = |service: crate::McpService| async move {
        let reply = Http::new(service)
            .post(&h(1, "tools/list", json!({})))
            .await;
        reply.only()["result"]["tools"].clone()
    };
    let name_list = |tools: &Json| -> Vec<String> {
        tools
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect()
    };

    // By default: the control tools of spec section 221, then pipelines.
    // An operation is found with the search tool, and is not listed.
    let tools = names(world.service("use", McpSettings::local())).await;
    let listed = name_list(&tools);
    for control in [
        "a2m_tool_search",
        "a2m_tool_call",
        "a2m_run_get",
        "a2m_run_resume",
        "a2m_run_cancel",
        "a2m_session_status",
        "a2m_session_login",
    ] {
        assert!(
            listed.iter().any(|name| name == control),
            "{control}: {listed:?}"
        );
    }
    // The control tools come first, then the pipelines: whatever else the
    // shared compiled target publishes, this pipeline is listed after every
    // built-in tool, and this operation is not listed.
    let builtin = listed
        .iter()
        .take_while(|name| name.starts_with("a2m_"))
        .count();
    assert!(
        listed[builtin..].iter().any(|name| name == ORDER_FILL),
        "{listed:?}"
    );
    assert!(
        !listed[builtin..]
            .iter()
            .any(|name| name.starts_with("a2m_"))
    );
    assert!(!listed.iter().any(|name| name == ORDER_GET));
    assert!(listed.len() <= 19);
    // Annotations come from the effect: a search reads; a call of a tool
    // may do what its tool does.
    let of = |name: &str| {
        tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap()["annotations"]
            .clone()
    };
    assert_eq!(of("a2m_tool_search")["readOnlyHint"], true);
    assert_eq!(of("a2m_tool_search")["destructiveHint"], false);
    assert_eq!(of("a2m_tool_call")["readOnlyHint"], false);
    assert_eq!(of("a2m_tool_call")["destructiveHint"], true);
    assert_eq!(of(ORDER_FILL)["readOnlyHint"], false);
    // No output schema is sent in a list; `a2m_tool_describe` returns it.
    assert!(
        tools
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool.get("outputSchema").is_none())
    );

    // With no room past the control tools, the pipelines move to search;
    // with room for one, the first is listed.
    for room in [0, 1] {
        let narrow = McpSettings {
            listed_max: builtin + room,
            ..McpSettings::local()
        };
        let narrowed = name_list(&names(world.service("use", narrow)).await);
        assert_eq!(narrowed, listed[..builtin + room]);
    }

    // A list costs one dispatch for each page of published tools, and not
    // one for each tool: here, one.
    for listing in [crate::Listing::Default, crate::Listing::Full] {
        let service = world.service(
            "use",
            McpSettings {
                listing,
                ..McpSettings::local()
            },
        );
        names(service.clone()).await;
        assert_eq!(service.dispatched(), 1, "{listing:?}");
    }

    // A full listing holds every operation as well.
    let full = McpSettings {
        listing: crate::Listing::Full,
        ..McpSettings::local()
    };
    let all = name_list(&names(world.service("use", full)).await);
    assert!(all.iter().any(|name| name == ORDER_GET), "{all:?}");
    assert!(all.iter().any(|name| name == ORDER_FILL));

    // The `build` toolset lists the action search tools.
    let build = name_list(&names(world.service("build", McpSettings::local())).await);
    for tool in ["a2m_action_search", "a2m_action_describe", "a2m_action_run"] {
        assert!(build.iter().any(|name| name == tool), "{tool}: {build:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_the_result_cap_cut_is_read_again_in_smaller_pages() {
    let names = |world: &World| {
        let service = world.service(
            "use",
            McpSettings {
                listing: crate::Listing::Full,
                ..McpSettings::local()
            },
        );
        async move {
            let reply = Http::new(service.clone())
                .post(&h(1, "tools/list", json!({})))
                .await;
            let names: Vec<String> = reply.only()["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["name"].as_str().unwrap().to_owned())
                .collect();
            (names, service.dispatched())
        }
    };
    let plain = World::ready().await;
    let (whole, once) = names(&plain).await;
    assert_eq!(once, 1);

    // A cap that one tool in full fits under, and the page of all of them
    // does not: measured, so that the test holds whatever is published.
    let size = |input: Json| {
        let world = &plain;
        async move {
            let found = world.direct("tool.search", input).await;
            found.result.unwrap().to_string().len()
        }
    };
    let mut largest = 0;
    for offset in 0..whole.len() {
        let one = size(json!({"full": true, "limit": 1, "offset": offset})).await;
        largest = largest.max(one);
    }
    let cap = largest + 64;
    assert!(size(json!({"full": true})).await > cap, "{cap}");
    let capped = a2m_actions::Settings::load(
        &a2m_actions::Sources::none().with_flag("tools.result_cap_bytes", &cap.to_string()),
    )
    .unwrap();
    let world = World::with_settings(true, capped).await;
    let (cut, dispatches) = names(&world).await;
    assert_eq!(cut, whole);
    assert!(dispatches > 1, "{dispatches}");
}
