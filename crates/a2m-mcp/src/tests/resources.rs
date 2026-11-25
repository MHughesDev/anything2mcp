//! Resources and the subscription to them (decision 0478), with raw
//! messages: the templates come from the registry, a URI that no template
//! matches or that names no watch of the caller's person is not found, and
//! a subscription honours only such watches.

use serde_json::{Value as Json, json};

use super::wire::{Http, h, m};
use super::world::World;
use crate::McpSettings;

fn code(answer: &Json) -> i64 {
    answer["error"]["code"]
        .as_i64()
        .unwrap_or_else(|| panic!("not an error: {answer}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resources_are_read_by_their_template_and_an_unknown_uri_is_not_found() {
    let world = World::ready().await;
    let http = Http::new(world.service("use", McpSettings::local()));

    // The one template, in both revisions: the face of `watch.changes`.
    for (message, stateless) in [
        (h(1, "resources/templates/list", json!({})), false),
        (m(2, "resources/templates/list", json!({})), true),
    ] {
        let answer = http.post(&message).await.only().clone();
        let templates = answer["result"]["resourceTemplates"].as_array().unwrap();
        assert_eq!(templates.len(), 1, "{answer}");
        assert_eq!(
            templates[0]["uriTemplate"],
            "a2m://watches/{watch_id}/changes{?after}"
        );
        assert_eq!(templates[0]["mimeType"], "application/json");
        assert_eq!(templates[0]["name"], "watch.changes");
        assert_eq!(answer["result"]["ttlMs"].is_u64(), stateless, "{answer}");
        // No fixed resource: each watch is reached by its URI.
        let listed = http
            .post(&if stateless {
                m(3, "resources/list", json!({}))
            } else {
                h(3, "resources/list", json!({}))
            })
            .await
            .only()
            .clone();
        assert_eq!(listed["result"]["resources"], json!([]), "{listed}");
    }

    // A URI no template matches, one that adds a field the template does
    // not take, and a watch that does not exist: not found, -32002 in the
    // handshake revision and -32602 in the stateless one, and nothing is
    // decoded or guessed.
    for uri in [
        "a2m://watches",
        "file:///etc/passwd",
        "a2m://watches/watch_01a213bb3e4170feb32df88a89d5a283/changes?limit=5",
        "a2m://watches/..%2F/changes",
        "a2m://watches/watch_01a213bb3e4170feb32df88a89d5a283/changes",
        "a2m://watches/watch_01a213bb3e4170feb32df88a89d5a283/changes?after=3",
    ] {
        let old = http
            .post(&h(4, "resources/read", json!({"uri": uri})))
            .await
            .only()
            .clone();
        assert_eq!(code(&old), -32002, "{uri}: {old}");
        assert_eq!(old["error"]["data"]["code"], "not_found", "{uri}: {old}");
        let new = http
            .post_with(
                &m(5, "resources/read", json!({"uri": uri})),
                &[("mcp-name", uri)],
            )
            .await
            .only()
            .clone();
        assert_eq!(code(&new), -32602, "{uri}: {new}");
    }
    // A read names its URI as text.
    let answer = http
        .post(&h(6, "resources/read", json!({"uri": 7})))
        .await
        .only()
        .clone();
    assert_eq!(code(&answer), -32602);
    drop(world);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_subscription_honours_only_watches_of_the_callers_person() {
    let world = World::ready().await;
    let http = Http::new(world.service("use", McpSettings::local()));

    // Nothing that is a watch of the caller's person, and a kind of
    // notification this server never says: the acknowledgment honours
    // none of it, and the subscription ends at once, gracefully, with the
    // request's own answer.
    let reply = http
        .post(&m(
            9,
            "subscriptions/listen",
            json!({"notifications": {
                "resourceSubscriptions": [
                    "a2m://watches/watch_01a213bb3e4170feb32df88a89d5a283/changes",
                    "file:///etc/passwd",
                ],
                "toolsListChanged": true,
            }}),
        ))
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.messages.len(), 2, "{:?}", reply.messages);
    let acknowledged = &reply.messages[0];
    assert_eq!(
        acknowledged["method"],
        "notifications/subscriptions/acknowledged"
    );
    assert_eq!(
        acknowledged["params"]["notifications"],
        json!({"resourceSubscriptions": []})
    );
    assert_eq!(
        acknowledged["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        9
    );
    let ended = &reply.messages[1];
    assert_eq!(ended["id"], 9);
    assert_eq!(ended["result"]["resultType"], "complete");
    assert_eq!(
        ended["result"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        9
    );

    // A filter that is not an object is refused.
    let refused = http
        .post(&m(10, "subscriptions/listen", json!({"notifications": []})))
        .await
        .only()
        .clone();
    assert_eq!(code(&refused), -32602);
    // The handshake revision has no subscription.
    let old = http
        .post(&h(11, "subscriptions/listen", json!({"notifications": {}})))
        .await
        .only()
        .clone();
    assert_eq!(code(&old), -32601);
    let old = http
        .post(&h(
            12,
            "resources/subscribe",
            json!({"uri": "a2m://watches/x/changes"}),
        ))
        .await
        .only()
        .clone();
    assert_eq!(code(&old), -32601);
    drop(world);
}
