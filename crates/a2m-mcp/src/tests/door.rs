//! The proofs every front door owes, the lines of the checklist that hold
//! on one transport only, and the two ways the proxy is not a pure copier.

use serde_json::{Value as Json, json};

use super::engine::{ITEM_UPSERT, ORDER_GET};
use super::wire::{H, Http, M, Stdio, h, m, m_at};
use super::world::World;
use crate::{McpPeer, McpSettings, Revision};

fn code(answer: &Json) -> i64 {
    answer["error"]["code"]
        .as_i64()
        .unwrap_or_else(|| panic!("not an error: {answer}"))
}

fn call(id: i64, name: &str, arguments: Json) -> Json {
    h(
        id,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
}

/// The one text item of a result, as JSON.
fn text_of(answer: &Json) -> Json {
    serde_json::from_str(answer["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_refuses_before_it_reads_and_what_is_not_well_formed_never_reaches_the_dispatcher() {
    let world = World::ready().await;
    let service = world.service("use", McpSettings::local());
    let http = Http::new(service.clone());
    let ping = h(1, "ping", json!({})).to_string();

    // A foreign `Host`, or none, is refused before the body is read: the
    // body here is not JSON, and the answer is not about that.
    for host in ["evil.example", "localhost.evil.example", ""] {
        let reply = http.send(b"{not json", &[("host", host)]).await;
        assert_eq!(reply.status, 403, "{host}");
        assert!(reply.messages.is_empty());
    }
    for host in ["localhost", "localhost:8731", "127.0.0.1:9", "[::1]:9"] {
        assert_eq!(
            http.send(ping.as_bytes(), &[("host", host)]).await.status,
            200,
            "{host}"
        );
    }

    // Only `POST`: no stream is opened by the server, and there is no
    // session to end.
    for method in ["GET", "DELETE"] {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri("/mcp/use")
            .header("host", "localhost")
            .body(axum::body::Body::empty())
            .unwrap();
        request.extensions_mut().insert(super::world::peer());
        assert_eq!(service.handle(request).await.status().as_u16(), 405);
    }

    // The message is JSON, and its caller takes either kind of answer.
    for (headers, status) in [
        (vec![("content-type", "text/plain")], 415),
        (vec![("accept", "text/html")], 406),
        (vec![("accept", "application/json")], 406),
        (vec![("accept", "*/*")], 200),
        (
            vec![("content-type", "application/json; charset=utf-8")],
            200,
        ),
    ] {
        let mut request = axum::http::Request::post("/mcp/use").header("host", "localhost");
        for (name, value) in [
            ("content-type", "application/json"),
            ("accept", "application/json, text/event-stream"),
        ] {
            let given = headers.iter().find(|(given, _)| *given == name);
            request = request.header(name, given.map_or(value, |(_, value)| *value));
        }
        let mut request = request.body(axum::body::Body::from(ping.clone())).unwrap();
        request.extensions_mut().insert(super::world::peer());
        assert_eq!(
            service.handle(request).await.status().as_u16(),
            status,
            "{headers:?}"
        );
    }

    // A caller the channel did not name, a peer that is not this engine's
    // user, and a bearer token, which this build's kernel refuses. The
    // token is in nothing that comes back.
    let token = "example-token-0123456789";
    let nobody = Http {
        service: service.clone(),
        peer: None,
        project: None,
    };
    let reply = nobody.send(ping.as_bytes(), &[]).await;
    assert_eq!(reply.status, 401);
    assert_eq!(code(reply.only()), -32001);
    assert_eq!(reply.only()["error"]["data"]["class"], "policy");
    assert_eq!(reply.only()["error"]["data"]["code"], "caller_missing");
    let reply = nobody
        .send(
            ping.as_bytes(),
            &[("authorization", &format!("Bearer {token}"))],
        )
        .await;
    assert_eq!(reply.status, 401);
    assert_eq!(reply.only()["error"]["data"]["code"], "caller_not_verified");
    assert!(!String::from_utf8_lossy(&reply.body).contains(token));
    let stranger = Http {
        service: service.clone(),
        peer: Some(McpPeer {
            user: "sam".to_owned(),
            label: String::new(),
        }),
        project: None,
    };
    let reply = stranger.send(ping.as_bytes(), &[]).await;
    assert_eq!(reply.status, 403);
    assert_eq!(reply.only()["error"]["data"]["code"], "caller_not_verified");

    // A caller the mount already had the kernel check is taken as it is,
    // as an agent at this door.
    let checked = world
        .kernel
        .verify_caller(a2m_actions::CallerCredential::LocalUser(
            super::world::USER.to_owned(),
        ))
        .await
        .unwrap();
    let mut request = axum::http::Request::post("/mcp/use")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(axum::body::Body::from(ping.clone()))
        .unwrap();
    request.extensions_mut().insert(a2m_actions::Caller::new(
        checked,
        a2m_actions::FrontDoor::Rest,
        false,
    ));
    assert_eq!(service.handle(request).await.status().as_u16(), 200);

    // A body over the limit.
    let small = McpSettings {
        max_request_bytes: 64,
        ..McpSettings::local()
    };
    let reply = Http::new(world.service("use", small))
        .send(
            h(1, "ping", json!({"padding": "x".repeat(100)}))
                .to_string()
                .as_bytes(),
            &[],
        )
        .await;
    assert_eq!(reply.status, 413);

    // Two unrelated things that are not well formed: each is refused with
    // the JSON-RPC error and the named failure.
    for (bytes, rpc) in [
        (b"\xff\xfe not json at all".as_slice(), -32700),
        (
            br#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"}]"#.as_slice(),
            -32600,
        ),
        (
            br#"{"jsonrpc":"1.0","id":1,"method":"tools/list"}"#.as_slice(),
            -32600,
        ),
        (
            br#"{"jsonrpc":"2.0","id":{"a":1},"method":"tools/list"}"#.as_slice(),
            -32600,
        ),
    ] {
        let reply = http.send(bytes, &[]).await;
        assert_eq!(reply.status, 400);
        let answer = reply.only();
        assert_eq!(code(answer), rpc, "{answer}");
        assert_eq!(answer["error"]["data"]["class"], "input");
        assert_eq!(answer["error"]["data"]["code"], "schema_invalid");
        assert!(!answer["error"]["data"]["fix"].as_str().unwrap().is_empty());
    }
    // Parameters of the wrong shape are refused by the protocol, too.
    let reply = http
        .post(&h(
            2,
            "tools/call",
            json!({"name": "a2m_tool_search", "arguments": "text"}),
        ))
        .await;
    assert_eq!(code(reply.only()), -32602);

    // None of the above was handed to the dispatcher.
    assert_eq!(service.dispatched(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_carries_no_session_and_its_headers_must_agree_with_the_body() {
    let world = World::ready().await;
    let http = Http::new(world.service("use", McpSettings::local()));

    // `initialize` issues no session identifier, and a later request of
    // the handshake revision is served as it comes: with its header, and
    // with none.
    let reply = http
        .post(&h(
            1,
            "initialize",
            json!({"protocolVersion": H, "capabilities": {}, "clientInfo": {"name": "c", "version": "0"}}),
        ))
        .await;
    assert_eq!(reply.status, 200);
    assert!(!reply.session);
    let list = h(2, "tools/list", json!({})).to_string();
    for headers in [vec![], vec![("mcp-protocol-version", H)]] {
        let reply = http.send(list.as_bytes(), &headers).await;
        assert_eq!(reply.status, 200);
        assert!(!reply.session);
        assert!(reply.only()["result"]["tools"].is_array());
    }

    // A revision that is not served is status 400, in the body or in the
    // header alone.
    let reply = http
        .post(&m_at("2030-01-01", 3, "tools/list", json!({})))
        .await;
    assert_eq!(reply.status, 400);
    assert_eq!(code(reply.only()), -32022);
    let reply = http
        .send(list.as_bytes(), &[("mcp-protocol-version", "2025-03-26")])
        .await;
    assert_eq!(reply.status, 400);
    assert_eq!(code(reply.only()), -32022);
    assert_eq!(reply.only()["error"]["data"]["requested"], "2025-03-26");

    // Each of the three headers of the stateless revision must say what
    // the body says.
    let asked = m(
        4,
        "tools/call",
        json!({"name": "a2m_tool_search", "arguments": {}}),
    );
    let good = [
        ("mcp-protocol-version", M),
        ("mcp-method", "tools/call"),
        ("mcp-name", "a2m_tool_search"),
    ];
    let reply = http.send(asked.to_string().as_bytes(), &good).await;
    assert_eq!(reply.status, 200, "{:?}", reply.messages);
    for (at, wrong) in [(0, H), (1, "tools/list"), (2, "a2m_tool_call")] {
        let mut headers = good;
        headers[at].1 = wrong;
        let reply = http.send(asked.to_string().as_bytes(), &headers).await;
        assert_eq!(reply.status, 400, "{headers:?}");
        assert_eq!(code(reply.only()), -32020, "{headers:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_set_to_one_revision_turns_the_other_away_as_the_rule_says() {
    let world = World::ready().await;
    let only = |revision: Revision| McpSettings {
        revisions: vec![revision],
        ..McpSettings::local()
    };
    let init = h(
        1,
        "initialize",
        json!({"protocolVersion": H, "capabilities": {}, "clientInfo": {"name": "c", "version": "0"}}),
    );

    // The stateless revision alone: `initialize` is refused with the
    // revisions served, and a request that names no revision is refused.
    let modern = Http::new(world.service("use", only(Revision::Stateless)));
    let reply = modern.post(&init).await;
    assert_eq!(code(reply.only()), -32022);
    assert_eq!(reply.only()["error"]["data"]["supported"], json!([M]));
    assert_eq!(reply.only()["error"]["data"]["requested"], H);
    let reply = modern.post(&h(2, "tools/list", json!({}))).await;
    assert_eq!(code(reply.only()), -32602);
    let reply = modern.post(&m(3, "server/discover", json!({}))).await;
    assert_eq!(reply.only()["result"]["supportedVersions"], json!([M]));
    assert!(modern.post(&m(4, "tools/list", json!({}))).await.only()["result"]["tools"].is_array());

    // The handshake revision alone: discovery is an unknown method, so a
    // client that speaks both falls back; a request that names the newer
    // revision is refused with the revisions served.
    let old = Http::new(world.service("use", only(Revision::Handshake)));
    let reply = old.post(&m(5, "server/discover", json!({}))).await;
    assert_eq!(code(reply.only()), -32601);
    assert_eq!(reply.only()["id"], 5);
    let reply = old.post(&m(6, "tools/list", json!({}))).await;
    assert_eq!(code(reply.only()), -32022);
    assert_eq!(reply.only()["error"]["data"]["supported"], json!([H]));
    assert_eq!(old.post(&init).await.only()["result"]["protocolVersion"], H);
    assert!(old.post(&h(7, "tools/list", json!({}))).await.only()["result"]["tools"].is_array());

    // Settings that name no revision, and a toolset this build does not
    // list, are refused when the endpoint is made.
    let none = McpSettings {
        revisions: Vec::new(),
        ..McpSettings::local()
    };
    for (toolset, settings) in [("use", none), ("orders", McpSettings::local())] {
        let refused = crate::McpService::new(
            world.kernel.clone(),
            a2m_policy::ToolsetName::new(toolset).unwrap(),
            settings,
        )
        .unwrap_err();
        assert_eq!(refused.class(), a2m_core::FailureClass::Input);
        assert_eq!(refused.code().as_str(), "mcp_not_served");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_proxy_copies_what_the_endpoint_answers_byte_for_byte() {
    let world = World::ready().await;
    let service = world.service("use", McpSettings::local());
    let http = Http::new(service.clone());
    let mut stdio = Stdio::start(service);
    let init = json!({"protocolVersion": H, "capabilities": {}, "clientInfo": {"name": "c", "version": "0"}});
    for message in [
        h(1, "ping", json!({})),
        h("two", "initialize", init),
        m(3, "server/discover", json!({})),
        h(4, "tools/list", json!({})),
        m(5, "tools/list", json!({})),
        m(6, "nope/nope", json!({})),
        m_at("1900-01-01", 7, "tools/list", json!({})),
        call(8, "a2m_tool_call", json!({"tool": "example_nope_get"})),
        m(
            9,
            "tools/call",
            json!({"name": "example_nope_get", "arguments": {}}),
        ),
        json!({"jsonrpc": "2.0", "id": 10}),
    ] {
        let direct = http.post(&message).await;
        stdio.write(message.to_string().as_bytes()).await;
        let line = stdio.line().await.unwrap();
        assert_eq!(
            String::from_utf8(line).unwrap(),
            String::from_utf8(direct.body).unwrap(),
            "{message}"
        );
    }
    let (rest, ended) = stdio.close().await;
    assert!(rest.is_empty());
    ended.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_line_closes_that_requests_stream_and_other_requests_are_still_answered() {
    // The driver never gets ready, so the run stays in hand and the call
    // stays open.
    let world = World::ready().await;
    world
        .hooks
        .prepare_hangs
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let service = world.service("use", McpSettings::local());
    let mut stopped = service.cancellations();
    let mut stdio = Stdio::start(service);
    let slow = h(
        "slow",
        "tools/call",
        json!({
            "name": "a2m_tool_call",
            "arguments": {"tool": ORDER_GET, "input": {"order_id": "A-100"}, "wait_seconds": 600},
            "_meta": {"progressToken": "p"},
        }),
    );
    stdio.write(slow.to_string().as_bytes()).await;
    // The call is under way once its first progress arrives.
    let note = stdio.message().await;
    assert_eq!(note["method"], "notifications/progress");
    assert_eq!(note["params"]["progressToken"], "p");

    let cancel = json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": {"requestId": "slow"},
    });
    stdio.write(cancel.to_string().as_bytes()).await;
    // The server saw the stream close, and stopped waiting.
    tokio::time::timeout(std::time::Duration::from_secs(60), stopped.changed())
        .await
        .expect("the endpoint noticed the closed stream")
        .unwrap();

    // Another request is still answered, and nothing more is ever written
    // for the cancelled one.
    stdio
        .write(h("after", "ping", json!({})).to_string().as_bytes())
        .await;
    let answer = stdio.message().await;
    assert_eq!(answer["id"], "after");
    let (rest, ended) = stdio.close().await;
    assert!(rest.is_empty(), "{rest:?}");
    ended.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_rests_with_a_link_and_the_next_call_and_the_door_offers_no_way_to_approve() {
    let world = World::ready().await;
    let service = world.service("use", McpSettings::local());
    let http = Http::new(service);
    let input = json!({
        "order_id": "A-102",
        "item_code": "L-300",
        "description": "New rail",
        "quantity": 3,
        "length_in": "150.5",
    });
    // The published tool, called by its own name.
    let reply = http
        .post(&m(
            1,
            "tools/call",
            json!({"name": ITEM_UPSERT, "arguments": input}),
        ))
        .await;
    let result = &reply.only()["result"];
    let run = &result["structuredContent"];
    assert_eq!(run["status"], "needs_approval", "{result}");
    // The link is where a person goes; following it approves nothing.
    assert!(
        run["link"]
            .as_str()
            .unwrap()
            .contains(run["run_id"].as_str().unwrap())
    );
    // The following call is named inside the result, as spec section 221
    // shows it, where every client reads it; and in no second place.
    let next = &run["next"];
    assert_eq!(next["tool"], "a2m_run_resume", "{result}");
    assert_eq!(next["action"], "run.resume");
    assert_eq!(next["args"]["run_id"], run["run_id"]);
    assert!(next["when"].is_string());
    assert!(result["_meta"].get("anything2mcp/next").is_none());
    // The test application was asked to store nothing.
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 0);
    assert_eq!(
        world
            .surface
            .approvals
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    // Everything the door exposes: the methods it serves and the tools of
    // both toolsets. None of them approves, and no name an agent might
    // try is a tool.
    for toolset in ["use", "build"] {
        let http = Http::new(world.service(toolset, McpSettings::local()));
        let listed = http.post(&h(2, "tools/list", json!({}))).await;
        for tool in listed.only()["result"]["tools"].as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            assert!(!name.contains("approv"), "{name}");
        }
        for name in [
            "a2m_run_approve",
            "a2m_approval_approve",
            "a2m_approval_grant",
        ] {
            let reply = http
                .post(&call(3, name, json!({"run_id": run["run_id"]})))
                .await;
            assert_eq!(code(reply.only()), -32602, "{name}");
        }
        for method in ["approvals/approve", "run/approve", "elicitation/create"] {
            let reply = http.post(&h(4, method, json!({}))).await;
            assert!(reply.only().get("result").is_none(), "{method}");
        }
    }
    // Nor does the registry behind the generic tool hold such an action.
    assert!(
        world
            .kernel
            .actions()
            .iter()
            .all(|def| !def.name.to_string().contains("approv"))
    );
    // The run still rests, and nothing was written.
    let reply = http
        .post(&call(5, "a2m_run_get", json!({"run_id": run["run_id"]})))
        .await;
    assert_eq!(
        reply.only()["result"]["structuredContent"]["status"],
        "needs_approval"
    );
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_call_carries_the_failure_and_nothing_else() {
    let world = World::ready().await;
    let http = Http::new(world.service("use", McpSettings::local()));
    // A person stops everything; the agent's call is then refused.
    let stopped = world
        .direct("killswitch.set", json!({"scope": "everything", "on": true}))
        .await;
    assert!(stopped.ok, "{:?}", stopped.error);
    let arguments = json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}});
    for asked in [
        call(1, "a2m_tool_call", arguments),
        call(2, ORDER_GET, json!({"order_id": "A-100"})),
    ] {
        let reply = http.post(&asked).await;
        let result = &reply.only()["result"];
        assert_eq!(result["isError"], true, "{result}");
        let keys: Vec<&String> = result.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["content", "isError"], "{result}");
        let failure = text_of(reply.only());
        // A kill switch is the policy engine's, wherever it is met (spec
        // section 212).
        assert_eq!(failure["class"], "policy", "{failure}");
        assert_eq!(failure["code"], "kill_switch_on", "{failure}");
        let keys: Vec<&String> = failure.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["class", "code", "fix", "message", "retry_safe"]);
        for key in ["code", "message", "fix"] {
            assert!(!failure[key].as_str().unwrap().is_empty(), "{key}");
        }
    }
}

pub const SECRET: &str = "Example Customer Two";

/// Plants the secret: the test application is made to answer with a value
/// the vault has released as a secret, the customer's name of one order.
pub async fn plant(world: &World) {
    let stored = world
        .direct(
            "secret.set",
            json!({"name": "example_password", "value": SECRET}),
        )
        .await;
    assert!(stored.ok, "{:?}", stored.error);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_planted_secret_appears_in_nothing_the_door_sends() {
    let world = World::ready().await;
    plant(&world).await;
    let token = "example-token-for-the-planted-secret-test";
    let service = world.service("use", McpSettings::local());
    let http = Http::new(service.clone());
    let mut stdio = Stdio::start(service.clone());
    let mut sent = Vec::new();
    let read = json!({"tool": ORDER_GET, "input": {"order_id": "A-101"}});
    for (at, message) in [
        call(1, "a2m_tool_call", read),
        m(2, "tools/call", json!({"name": ORDER_GET, "arguments": {"order_id": "A-101"}, "_meta": {"progressToken": 1}})),
        // Failures that quote the caller's own input.
        call(3, "a2m_tool_call", json!({"tool": ORDER_GET, "input": {"order_id": SECRET}})),
        call(4, "a2m_tool_search", json!({"query": SECRET})),
        call(5, SECRET, json!({})),
        h(6, SECRET, json!({})),
        h(7, "tools/list", json!({"cursor": SECRET})),
    ]
    .into_iter()
    .enumerate()
    {
        let direct = http.post(&message).await;
        sent.push(String::from_utf8_lossy(&direct.body).into_owned());
        if at == 0 {
            // The read did happen, and its value was redacted.
            let run = &direct.only()["result"]["structuredContent"];
            assert_eq!(run["status"], "completed", "{run}");
            // The redactor's mark stands where the value was.
            let shown = run["result"]["customer"].as_str().unwrap();
            assert!(shown.starts_with(a2m_core::REDACTED), "{shown}");
            let run_id = run["run_id"].clone();
            for tool in ["a2m_run_get", "a2m_trace_show"] {
                let later = http.post(&call(9, tool, json!({"run_id": run_id}))).await;
                sent.push(String::from_utf8_lossy(&later.body).into_owned());
            }
        }
        sent.extend(ask_lines(&mut stdio, &message).await);
    }
    // A caller's token: refused, and repeated nowhere.
    let nobody = Http {
        service,
        peer: None,
        project: None,
    };
    let reply = nobody
        .send(
            h(8, "ping", json!({})).to_string().as_bytes(),
            &[("authorization", &format!("Bearer {token}"))],
        )
        .await;
    assert_eq!(reply.status, 401);
    sent.push(String::from_utf8_lossy(&reply.body).into_owned());
    let (rest, ended) = stdio.close().await;
    assert!(rest.is_empty());
    ended.unwrap();

    assert!(sent.len() >= 16, "{}", sent.len());
    for text in &sent {
        assert!(!text.contains(SECRET), "{text}");
        assert!(!text.contains(token), "{text}");
    }
}

/// Sends one request through the proxy and returns every line answered.
pub async fn ask_lines(stdio: &mut Stdio, message: &Json) -> Vec<String> {
    stdio.write(message.to_string().as_bytes()).await;
    let mut lines = Vec::new();
    loop {
        let line = String::from_utf8(stdio.line().await.unwrap()).unwrap();
        let last = serde_json::from_str::<Json>(&line)
            .unwrap()
            .get("id")
            .is_some();
        lines.push(line);
        if last {
            return lines;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_router_nests_and_serves_a_call_with_progress_over_a_real_socket() {
    let world = World::ready().await;
    let service = world.service("use", McpSettings::local());
    let path = crate::mcp_path(service.toolset());
    // What the product crate does: nest the endpoint, and name the peer.
    let app = axum::Router::new()
        .nest_service(&path, service.http())
        .layer(axum::Extension(super::world::peer()));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    let asked = m(
        1,
        "tools/call",
        json!({
            "name": ORDER_GET,
            "arguments": {"order_id": "A-100"},
            "_meta": {"progressToken": "t"},
        }),
    );
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}{path}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", M)
        .header("mcp-method", "tools/call")
        .header("mcp-name", ORDER_GET)
        .body(asked.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let kind = response.headers()["content-type"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(kind.starts_with("text/event-stream"), "{kind}");
    assert!(!response.headers().contains_key("mcp-session-id"));
    let text = response.text().await.unwrap();
    let progress = text.find("notifications/progress").expect("progress");
    let result = text.find("structuredContent").expect("the result");
    assert!(progress < result, "{text}");
    assert!(text.contains("Example Customer One"), "{text}");
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_published_tool_called_by_its_own_name_is_one_dispatch_and_an_unpublished_name_is_no_tool()
 {
    let world = World::ready().await;
    let service = world.service("use", McpSettings::local());
    let http = Http::new(service.clone());

    let reply = http
        .post(&call(1, ORDER_GET, json!({"order_id": "A-100"})))
        .await;
    let run = &reply.only()["result"]["structuredContent"];
    assert_eq!(run["status"], "completed", "{:?}", reply.messages);
    assert_eq!(service.dispatched(), 1);

    // A name nobody published: the dispatcher says so, once, and the
    // protocol's answer is its unknown-tool error, with the dispatcher's
    // own words as its data.
    let reply = http.post(&call(2, "example_nope_get", json!({}))).await;
    assert_eq!(code(reply.only()), -32602);
    let data = &reply.only()["error"]["data"];
    assert_eq!(data["class"], "input", "{data}");
    assert_eq!(data["code"], "not_found", "{data}");
    assert!(!data["fix"].as_str().unwrap().is_empty());
    assert_eq!(service.dispatched(), 2);

    // A published tool whose record is not there is a call that failed:
    // a result, never an unknown tool.
    let reply = http
        .post(&call(3, ORDER_GET, json!({"order_id": "A-999"})))
        .await;
    let result = &reply.only()["result"];
    assert_eq!(result["isError"], true, "{:?}", reply.messages);
    assert_eq!(service.dispatched(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_project_folder_is_what_the_mount_stated_and_nothing_a_caller_writes() {
    let world = World::ready().await;
    let project = world.folder.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    // What marks a folder as a project.
    std::fs::write(project.join("anything2mcp.toml"), "").unwrap();
    let lint = call(1, "a2m_draft_lint", json!({"target": "example"}));

    // Stated by the mount, beside the caller: the action looks in the
    // folder, and finds no such target there.
    let mut http = Http::new(world.service("build", McpSettings::local()));
    http.project = Some(a2m_actions::ProjectFolder::new(&project));
    let reply = http.post(&lint).await;
    let result = &reply.only()["result"];
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(
        result["structuredContent"]["problems"][0]["code"], "kernel.file_missing",
        "{result}"
    );

    // Not stated: the action says that it needs one. An argument that
    // names a folder is not an input the action takes, and sets nothing.
    http.project = None;
    let failure = text_of(http.post(&lint).await.only());
    assert_eq!(failure["code"], "path_not_allowed", "{failure}");
    let smuggled = call(
        2,
        "a2m_draft_lint",
        json!({"target": "example", "project": project.display().to_string()}),
    );
    let failure = text_of(http.post(&smuggled).await.only());
    assert_eq!(failure["code"], "schema_invalid", "{failure}");
}

/// The progress notes of an answer, and its response.
fn notes_of(mut messages: Vec<Json>, token: &Json) -> (Vec<String>, Json) {
    let answer = messages.pop().expect("a response");
    let mut last = 0;
    let notes = messages
        .iter()
        .map(|note| {
            assert_eq!(note["method"], "notifications/progress", "{note}");
            assert_eq!(&note["params"]["progressToken"], token, "{note}");
            let count = note["params"]["progress"].as_u64().unwrap();
            assert!(count > last, "{messages:?}");
            last = count;
            note["params"]["message"].as_str().unwrap().to_owned()
        })
        .collect();
    (notes, answer)
}

/// Holds every step back at the driver until it is opened, so that a test
/// decides when a run goes on, by what happened and not by the clock.
struct Gate {
    open: std::sync::Mutex<bool>,
    changed: std::sync::Condvar,
}

impl Gate {
    fn set(&self, open: bool) {
        *self.open.lock().unwrap() = open;
        self.changed.notify_all();
    }

    /// Waits until the gate is open. A test that never opens it fails at
    /// its own time limit, long before this one.
    fn pass(&self) {
        let closed = self.open.lock().unwrap();
        let _unused = self
            .changed
            .wait_timeout_while(closed, std::time::Duration::from_secs(300), |open| !*open)
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_says_what_the_run_does_for_a_read_a_write_that_rests_and_its_resume() {
    let world = World::ready().await;
    let service = world.service("use", McpSettings::local());
    let http = Http::new(service.clone());
    let with_token = |id: i64, name: &str, arguments: Json| {
        m(
            id,
            "tools/call",
            json!({"name": name, "arguments": arguments, "_meta": {"progressToken": "p"}}),
        )
    };
    let token = json!("p");
    let gate = std::sync::Arc::new(Gate {
        open: std::sync::Mutex::new(true),
        changed: std::sync::Condvar::new(),
    });
    world.hooks.on_step({
        let gate = std::sync::Arc::clone(&gate);
        move |_| gate.pass()
    });
    // A call whose run is held at its first step until the door follows
    // it: what the run does from there on is told as it happens.
    let followed = |message: Json| {
        let (http, gate) = (http.clone(), std::sync::Arc::clone(&gate));
        let mut begun = service.followings();
        async move {
            gate.set(false);
            let (reply, ()) = tokio::join!(http.post(&message), async {
                begun.changed().await.unwrap();
                gate.set(true);
            });
            reply
        }
    };

    // A read: the run is followed to its end, and the answer is the run
    // as `run.get` gives it, which is what the call without progress
    // answers.
    let reply = followed(with_token(1, ORDER_GET, json!({"order_id": "A-100"}))).await;
    let (notes, answer) = notes_of(reply.messages, &token);
    assert_eq!(notes[0], "The call was handed to the engine.");
    let run = &answer["result"]["structuredContent"];
    assert_eq!(run["status"], "completed", "{answer}");
    let stored = world.app.stored_order("A-100").await.unwrap().unwrap();
    assert_eq!(run["result"]["customer"], stored.customer.as_str());
    assert!(
        notes
            .iter()
            .any(|note| note == "The run is now `completed`."),
        "{notes:?}"
    );
    // Started without a wait, then read: two dispatches.
    assert_eq!(service.dispatched(), 2);

    // A write: it comes to rest for a person. The person is told, once,
    // and the answer carries the link and the call to make next, as the
    // call without progress does. Nothing was written.
    let input = json!({
        "order_id": "A-102",
        "item_code": "L-300",
        "description": "New rail",
        "quantity": 3,
        "length_in": "150.5",
    });
    let reply = followed(with_token(2, ITEM_UPSERT, input.clone())).await;
    let (notes, answer) = notes_of(reply.messages, &token);
    let run = answer["result"]["structuredContent"].clone();
    assert_eq!(run["status"], "needs_approval", "{answer}");
    assert!(
        notes
            .iter()
            .any(|note| note == "The run is now `needs_approval`."),
        "{notes:?}"
    );
    let run_id = run["run_id"].as_str().unwrap();
    assert!(run["link"].as_str().unwrap().contains(run_id), "{run}");
    assert_eq!(run["next"]["tool"], "a2m_run_resume");
    let asked = || {
        world
            .surface
            .approvals
            .load(std::sync::atomic::Ordering::SeqCst)
    };
    assert_eq!(asked(), 1);
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 0);
    // The same call again is the same run, and the person is not asked
    // again.
    let reply = http.post(&with_token(3, ITEM_UPSERT, input)).await;
    let (_, answer) = notes_of(reply.messages, &token);
    assert_eq!(answer["result"]["structuredContent"]["run_id"], run_id);
    assert_eq!(asked(), 1);

    // The person approves at the desk, which no door reaches. The resume
    // is dispatched as it was written, and followed meanwhile: its notes
    // are about what the run does from here on.
    world.approve(run_id).await;
    let reply = http
        .post(&with_token(4, "a2m_run_resume", json!({"run_id": run_id})))
        .await;
    let (notes, answer) = notes_of(reply.messages, &token);
    let done = &answer["result"]["structuredContent"];
    assert_eq!(done["status"], "completed", "{answer}");
    assert_eq!(notes[0], "The call was handed to the engine.");
    assert!(
        !notes.iter().any(|note| note.contains("needs_approval")),
        "{notes:?}"
    );
    // The test application holds the item, written once.
    let item = world
        .app
        .stored_item("A-102", "L-300")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(item.quantity, 3);
    assert_eq!(world.app.write_count("A-102", "L-300").await.unwrap(), 1);

    // No note is anything a system said: each is one of this server's own
    // sentences.
    let reply = http
        .post(&with_token(5, ORDER_GET, json!({"order_id": "A-101"})))
        .await;
    let (notes, _) = notes_of(reply.messages, &token);
    let stored = world.app.stored_order("A-101").await.unwrap().unwrap();
    for note in &notes {
        assert!(!note.contains(stored.customer.as_str()), "{note}");
        assert!(
            note.starts_with("The ")
                || note.starts_with("Step ")
                || note
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_ascii_digit()),
            "{note}"
        );
    }

    // A call that asks for no wait is dispatched as it is written.
    let before = service.dispatched();
    let reply = http
        .post(&with_token(
            6,
            "a2m_tool_call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}, "wait_seconds": 0}),
        ))
        .await;
    let (notes, answer) = notes_of(reply.messages, &token);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(answer["result"]["structuredContent"]["run_id"].is_string());
    assert_eq!(service.dispatched(), before + 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_made_again_with_its_key_is_the_same_run_and_one_write() {
    let world = World::ready().await;
    let http = Http::new(world.service("use", McpSettings::local()));
    let input = json!({
        "order_id": "A-102",
        "item_code": "L-310",
        "description": "New rail",
        "quantity": 3,
        "length_in": "150.5",
    });
    // By the tool's own name, the key in the call's `_meta`.
    let by_name = |id: i64| {
        m(
            id,
            "tools/call",
            json!({"name": ITEM_UPSERT, "arguments": input,
                "_meta": {"anything2mcp/idempotency_key": "example-key-310"}}),
        )
    };
    let first = http.post(&by_name(1)).await;
    let run = first.only()["result"]["structuredContent"]["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let again = http.post(&by_name(2)).await;
    let again = &again.only()["result"]["structuredContent"];
    assert_eq!(again["run_id"], run.as_str(), "{again}");
    assert_eq!(again["repeated"], true);

    // Approved and resumed, then made again: the finished run, one write.
    world.approve(&run).await;
    let done = http
        .post(&call(3, "a2m_run_resume", json!({"run_id": run})))
        .await;
    assert_eq!(
        done.only()["result"]["structuredContent"]["status"],
        "completed"
    );
    let finished = http.post(&by_name(4)).await;
    let finished = &finished.only()["result"]["structuredContent"];
    assert_eq!(finished["run_id"], run.as_str());
    assert_eq!(finished["status"], "completed");
    assert_eq!(world.app.write_count("A-102", "L-310").await.unwrap(), 1);

    // Through the generic call, the key as its argument: the same rules.
    let generic = |id: i64, code: &str| {
        let mut input = input.clone();
        input["item_code"] = json!(code);
        call(
            id,
            "a2m_tool_call",
            json!({"tool": ITEM_UPSERT, "input": input, "idempotency_key": "example-key-311"}),
        )
    };
    let one = http.post(&generic(5, "L-311")).await;
    let one = one.only()["result"]["structuredContent"]["run_id"].clone();
    let two = http.post(&generic(6, "L-311")).await;
    assert_eq!(two.only()["result"]["structuredContent"]["run_id"], one);
    // Another input under the key is refused by name.
    let other = http.post(&generic(7, "L-312")).await;
    let other = text_of(other.only());
    assert_eq!(other["code"], "idempotency_key_reused", "{other}");
    // A key that is not text is not a key.
    let bad = http
        .post(&m(
            8,
            "tools/call",
            json!({"name": ITEM_UPSERT, "arguments": input,
                "_meta": {"anything2mcp/idempotency_key": 7}}),
        ))
        .await;
    assert_eq!(code(bad.only()), -32602);
}
