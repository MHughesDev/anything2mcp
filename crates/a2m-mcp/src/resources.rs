//! Resources, and the subscription to them (decision 0478).
//!
//! A resource is a face of an action, generated from the registry: its
//! template is the action's, and `resources/read` of a URI the template
//! matches is one dispatch of that action as the caller, with the fields
//! the URI fills. Nothing here knows what an action reads. The one
//! resource this build has is a watch's changes, the face of
//! `watch.changes`.
//!
//! `subscriptions/listen`, of the stateless revision, holds a stream open
//! and says on it, by the resource's URI and nothing else, that a watch the
//! caller's person owns recorded a change; what the change was is read
//! with `resources/read`. The stream follows each watch through the
//! kernel, as a run's progress is followed, with no decision for each wait.
//! A stateless server keeps no subscription between requests: the
//! handshake revision's `resources/subscribe` needs a standing stream, and
//! is not served.

use std::sync::Arc;
use std::time::Duration;

use a2m_actions::{McpResultFace, Principal, WatchId, mcp_resources};
use axum::http::StatusCode;
use serde_json::{Map, Value as Json, json};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::codes;
use crate::http::{Answer, rpc_error};
use crate::protocol::{READER_PATIENCE, TTL_MS, object, response, result};
use crate::tools::{Asker, Core};

/// The key of a subscription's identifier in a notification's `_meta`.
const SUBSCRIPTION_KEY: &str = "io.modelcontextprotocol/subscriptionId";

/// The action whose resource can be subscribed to: a watch's changes.
const FOLLOWED_ACTION: &str = "watch.changes";

/// How long one wait for a watch's next change lasts before the stream
/// looks again whether its reader is still there.
const FOLLOW_WAIT: Duration = Duration::from_secs(60);

/// The resource-not-found error of the handshake revision; the stateless
/// revision answers -32602.
const RESOURCE_NOT_FOUND: i64 = -32002;

/// `resources/templates/list`: one template for each action that has a
/// resource face, the same for every caller.
pub(crate) fn templates(core: &Core, id: &Json, stateless: bool) -> Answer {
    let listed: Vec<Json> = mcp_resources(core.kernel.actions())
        .into_iter()
        .map(|face| {
            json!({
                "uriTemplate": face.uri_template,
                "name": face.name,
                "title": face.title,
                "description": face.description,
                "mimeType": face.mime_type,
            })
        })
        .collect();
    let mut body = object(json!({"resourceTemplates": listed}));
    if stateless {
        body.insert("ttlMs".to_owned(), json!(TTL_MS));
        body.insert("cacheScope".to_owned(), json!("public"));
    }
    Answer::Message(
        StatusCode::OK,
        response(id, result(stateless, body, Map::new())),
    )
}

/// `resources/list`: no fixed resource. Each watch is reached by the
/// template, with the URI its `watch.*` result names as `resource`.
pub(crate) fn list(id: &Json, stateless: bool) -> Answer {
    let mut body = object(json!({"resources": []}));
    if stateless {
        body.insert("ttlMs".to_owned(), json!(TTL_MS));
        body.insert("cacheScope".to_owned(), json!("public"));
    }
    Answer::Message(
        StatusCode::OK,
        response(id, result(stateless, body, Map::new())),
    )
}

/// The answer to a URI that is no resource of this server.
fn not_found(id: &Json, uri: &str, stateless: bool, said: Option<Json>) -> Answer {
    let data = said.unwrap_or_else(|| {
        codes::failure_data(&codes::input(
            a2m_actions::codes::NOT_FOUND,
            "No resource of this server has that URI.",
            "Read the URI a watch's result names as `resource`.",
        ))
    });
    rpc_error(
        StatusCode::OK,
        id,
        if stateless {
            codes::INVALID_PARAMS
        } else {
            RESOURCE_NOT_FOUND
        },
        &format!("Resource not found: {uri}"),
        Some(data),
    )
}

/// `resources/read`: one dispatch of the action the URI's template is the
/// face of, as the caller. Its result, the JSON the action's tool would
/// give, is the one item of contents.
pub(crate) async fn read(
    core: &Core,
    asker: &Asker,
    params: &Json,
    id: &Json,
    stateless: bool,
) -> Answer {
    let Some(uri) = params.get("uri").and_then(Json::as_str) else {
        return rpc_error(
            StatusCode::OK,
            id,
            codes::INVALID_PARAMS,
            "A resource is read by its URI, as text.",
            None,
        );
    };
    let registry = core.kernel.actions();
    let Some((def, input)) = registry.by_resource_uri(uri) else {
        return not_found(id, uri, stateless, None);
    };
    let action = def.name.to_string();
    let response_of = core.dispatch(&action, Json::Object(input), asker).await;
    let face = McpResultFace::of_in(&response_of, registry, core.builtin);
    if let Some(error) = &response_of.error {
        if error.class == a2m_core::FailureClass::Input
            && (error.code == a2m_actions::codes::NOT_FOUND.as_str()
                || error.code == a2m_actions::codes::SCHEMA_INVALID.as_str())
        {
            return not_found(id, uri, stateless, Some(error.to_json()));
        }
        return rpc_error(
            StatusCode::OK,
            id,
            codes::INTERNAL_ERROR,
            &error.message,
            Some(error.to_json()),
        );
    }
    let mut body = object(json!({"contents": [{
        "uri": uri,
        "mimeType": a2m_actions::RESOURCE_MIME_TYPE,
        "text": face.text,
    }]}));
    if stateless {
        // What a read gives is the caller's, and changes with each check.
        body.insert("ttlMs".to_owned(), json!(0));
        body.insert("cacheScope".to_owned(), json!("private"));
    }
    Answer::Message(
        StatusCode::OK,
        response(id, result(stateless, body, Map::new())),
    )
}

/// One resource a subscription honours: its URI as the client wrote it,
/// the watch, and the last change the client is taken to have seen.
struct Followed {
    uri: String,
    watch: WatchId,
    after: u64,
}

/// What a resource URI follows, when it is a watch of the caller's person:
/// the watch, and the change after which a change is news, the URI's
/// `after` or else the last one recorded.
async fn followed(core: &Core, principal: &Principal, uri: &str) -> Option<Followed> {
    let (def, input) = core.kernel.actions().by_resource_uri(uri)?;
    if def.name.as_str() != FOLLOWED_ACTION {
        return None;
    }
    let watch = input.get("watch_id")?.as_str()?.parse::<WatchId>().ok()?;
    let (last, _) = core.kernel.watch_last_change(principal, watch).await.ok()?;
    let after = input
        .get("after")
        .and_then(Json::as_u64)
        .map_or(last, |given| given.min(last));
    Some(Followed {
        uri: uri.to_owned(),
        watch,
        after,
    })
}

/// Sends one message of a subscription's stream. `false` when the reader
/// has gone, or has taken nothing for [`READER_PATIENCE`].
async fn send(messages: &mpsc::Sender<Json>, message: Json) -> bool {
    matches!(
        tokio::time::timeout(READER_PATIENCE, messages.send(message)).await,
        Ok(Ok(()))
    )
}

/// `subscriptions/listen`, of the stateless revision: the acknowledgment
/// first, naming the resources honoured, each a watch of the caller's
/// person; then `notifications/resources/updated` with the URI each time
/// one of them records a change; and, once every one has ended, the
/// request's own answer. A URI that is no such watch, and every other kind
/// of notification, is left out of the acknowledgment and never sent.
pub(crate) async fn listen(core: &Arc<Core>, asker: Asker, params: &Json, id: &Json) -> Answer {
    let Some(filter) = params.get("notifications").and_then(Json::as_object) else {
        return rpc_error(
            StatusCode::OK,
            id,
            codes::INVALID_PARAMS,
            "A subscription names what it listens for in `notifications`, as an object.",
            None,
        );
    };
    let asked: Vec<String> = match filter.get("resourceSubscriptions") {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Array(uris)) if uris.iter().all(Json::is_string) => uris
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_owned)
            .collect(),
        Some(_) => {
            return rpc_error(
                StatusCode::OK,
                id,
                codes::INVALID_PARAMS,
                "`resourceSubscriptions` is a list of resource URIs.",
                None,
            );
        }
    };
    let principal = asker.caller.principal.clone();
    let mut honoured = Vec::new();
    for uri in &asked {
        if honoured.iter().any(|known: &Followed| &known.uri == uri) {
            continue;
        }
        if let Some(found) = followed(core, &principal, uri).await {
            honoured.push(found);
        }
    }
    let mut agreed = Map::new();
    if filter.contains_key("resourceSubscriptions") {
        agreed.insert(
            "resourceSubscriptions".to_owned(),
            json!(
                honoured
                    .iter()
                    .map(|found| found.uri.clone())
                    .collect::<Vec<_>>()
            ),
        );
    }
    let tag = json!({SUBSCRIPTION_KEY: id});
    let (messages, stream) = mpsc::channel(8);
    let (core, id) = (Arc::clone(core), id.clone());
    tokio::spawn(async move {
        let acknowledged = json!({
            "jsonrpc": "2.0",
            "method": "notifications/subscriptions/acknowledged",
            "params": {"_meta": tag, "notifications": agreed},
        });
        if !send(&messages, acknowledged).await {
            return;
        }
        let mut following = JoinSet::new();
        for found in honoured {
            following.spawn(follow(
                Arc::clone(&core),
                principal.clone(),
                found,
                messages.clone(),
                tag.clone(),
            ));
        }
        while following.join_next().await.is_some() {}
        // Every watch followed has ended, or the reader has gone: the
        // subscription ends gracefully, with the request's own answer.
        let mut meta = Map::new();
        meta.insert(SUBSCRIPTION_KEY.to_owned(), id.clone());
        let _ = send(&messages, response(&id, result(true, Map::new(), meta))).await;
    });
    Answer::Stream(stream)
}

/// Follows one watch until it has ended and every change it recorded was
/// told, or the reader has gone, or the watch can no longer be read.
async fn follow(
    core: Arc<Core>,
    principal: Principal,
    found: Followed,
    messages: mpsc::Sender<Json>,
    tag: Json,
) {
    let mut after = found.after;
    loop {
        let page = tokio::select! {
            () = messages.closed() => return,
            page = core.kernel.watch_changes(&principal, found.watch, after, 100, FOLLOW_WAIT) => page,
        };
        let Ok(page) = page else {
            return;
        };
        if !page.changes.is_empty() {
            after = page.last;
            let updated = json!({
                "jsonrpc": "2.0",
                "method": "notifications/resources/updated",
                "params": {"_meta": tag, "uri": found.uri},
            });
            if !send(&messages, updated).await {
                return;
            }
        }
        if page.over() {
            return;
        }
    }
}
