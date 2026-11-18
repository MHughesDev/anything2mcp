//! Decision 0248: a sign-in goes only to an address the target declares,
//! what the target declares counts only once a person has published it, and
//! a credential belongs to the target it was stored for.
//!
//! Two small services stand in for the world: the identity service the
//! target declares, and "elsewhere", an address it does not. Each keeps every
//! request it hears, so a test shows that elsewhere heard nothing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_core::{FailureClass, Secret};
use a2m_vault::{ItemId, OwnerRef, SealContext, SealedKind};
use axum::Router;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;

use super::support::{STORED_HELD, World, account_key, decode_form, reference, request};
use crate::{AuthEntry, ProviderName, SignInDestinations, Wait, codes};

const CLIENT_SECRET: &str = "secret-for-tests-client-0248";

/// What one service heard, and how it answers.
#[derive(Default)]
struct Heard {
    /// Each request's body, in order.
    bodies: Vec<String>,
    /// Each request's `authorization` header, in order.
    proofs: Vec<String>,
    /// When set, every request is sent here instead, with a redirect.
    redirect_to: Option<String>,
}

type Shared = Arc<Mutex<Heard>>;

async fn answer(heard: Shared, headers: HeaderMap, body: String) -> Response {
    let mut heard = heard.lock().unwrap();
    heard.bodies.push(body);
    heard.proofs.push(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned(),
    );
    if let Some(to) = &heard.redirect_to {
        return (
            StatusCode::TEMPORARY_REDIRECT,
            [(header::LOCATION, to.clone())],
        )
            .into_response();
    }
    let issued = heard.bodies.len();
    (
        StatusCode::OK,
        serde_json::json!({
            "access_token": format!("example-token-{issued}"),
            "token_type": "Bearer",
            "expires_in": 3600,
        })
        .to_string(),
    )
        .into_response()
}

/// Starts a service on a free port of this machine; gives its origin.
async fn serve(heard: &Shared) -> String {
    let shared = Arc::clone(heard);
    let app = Router::new().route(
        "/{*path}",
        any(move |headers: HeaderMap, body: String| answer(Arc::clone(&shared), headers, body)),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

fn heard(service: &Shared) -> usize {
    service.lock().unwrap().bodies.len()
}

/// Whether the client secret reached `service` in any form it was sent.
fn heard_the_secret(service: &Shared) -> bool {
    let heard = service.lock().unwrap();
    heard.bodies.iter().any(|body| {
        decode_form(body)
            .get("client_secret")
            .is_some_and(|value| value == CLIENT_SECRET)
    }) || heard.proofs.iter().any(|proof| !proof.is_empty())
}

/// A client-credentials entry whose token endpoint is `token`, for a target
/// that declares surfaces at `declared`, as the drafts give it.
fn drafted(token: &str, declared: &[&str]) -> AuthEntry {
    let mut entry = AuthEntry::new(ProviderName::new("oauth_client_credentials").unwrap())
        .setting("token_endpoint", &format!("{token}/token"))
        .setting("client_id", "client-1")
        .setting("client_auth", "body")
        .credential("client_secret", reference("client/secret"));
    entry.surfaces.destinations =
        SignInDestinations::new(declared.iter().map(|address| (*address).to_owned()));
    entry
}

#[tokio::test]
async fn a_published_sign_in_sends_only_to_an_address_the_target_declares() {
    let world = World::new(STORED_HELD).await;
    let key = account_key("robot");
    world
        .store_credential(&key, "client/secret", CLIENT_SECRET)
        .await;
    let (declared, elsewhere) = (Shared::default(), Shared::default());
    let (declared_at, elsewhere_at) = (serve(&declared).await, serve(&elsewhere).await);

    // The token endpoint is elsewhere: refused by name before anything is
    // sent, when the addresses are asked for and when a lease is.
    let outside = Arc::new(drafted(&elsewhere_at, &[&declared_at]).published());
    let refused = world
        .keeper
        .sign_in_addresses(&key.target, &outside)
        .unwrap_err();
    assert_eq!(refused.code(), &codes::DESTINATION_NOT_DECLARED);
    assert_eq!(refused.class(), FailureClass::Policy);
    let refused = world
        .keeper
        .lease(request(&key, &outside, Wait::DEFAULT))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::DESTINATION_NOT_DECLARED);
    assert!(refused.message().contains("token_endpoint"), "{refused}");
    assert_eq!(heard(&elsewhere), 0);

    // A declared endpoint that redirects elsewhere: the redirect is not
    // followed, and elsewhere hears nothing.
    declared.lock().unwrap().redirect_to = Some(format!("{elsewhere_at}/token"));
    let inside = Arc::new(drafted(&declared_at, &[&declared_at]).published());
    let addresses = world
        .keeper
        .sign_in_addresses(&key.target, &inside)
        .unwrap();
    assert_eq!(addresses.len(), 1);
    assert_eq!(addresses[0].address, format!("{declared_at}/token"));
    let refused = world
        .keeper
        .lease(request(&key, &inside, Wait::DEFAULT))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::UNEXPECTED_RESPONSE, "{refused}");
    world.keeper.idle().await;
    assert_eq!(heard(&declared), 1);
    assert_eq!(heard(&elsewhere), 0);
    assert!(!heard_the_secret(&elsewhere));
}

#[tokio::test]
async fn a_sign_in_the_drafts_declare_sends_nothing_until_its_person_carries_it_out() {
    let world = World::new(STORED_HELD).await;
    let key = account_key("robot");
    world
        .store_credential(&key, "client/secret", CLIENT_SECRET)
        .await;
    let service = Shared::default();
    let at = serve(&service).await;
    let draft = Arc::new(drafted(&at, &[&at]));

    // A run under the drafts, with the credential stored: no sign-in.
    let refused = world
        .keeper
        .lease(request(&key, &draft, Wait::DEFAULT))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::NEEDS_LOGIN);
    assert!(refused.message().contains("drafts"), "{refused}");
    // A sign-in asked for on the drafts' word alone: none.
    let refused = world
        .keeper
        .sign_in(&key, Arc::clone(&draft), None)
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::NEEDS_LOGIN);
    world.keeper.idle().await;
    assert_eq!(heard(&service), 0);

    // The person, shown every address, carries out one sign-in.
    let shown = Arc::new(AuthEntry {
        surfaces: draft.surfaces.clone().shown_to_person(),
        ..(*draft).clone()
    });
    let ticket = world
        .keeper
        .sign_in(&key, Arc::clone(&shown), None)
        .await
        .unwrap();
    world
        .keeper
        .signed_in(ticket.session, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(heard(&service), 1);
    assert!(heard_the_secret(&service));
    // A run under the same drafts may use the session it made.
    world
        .keeper
        .lease(request(&key, &draft, Wait::DEFAULT))
        .await
        .unwrap();
    assert!(world.keeper.allows(ticket.session, &draft));
    // Drafts that move the token endpoint elsewhere may not.
    let other = Shared::default();
    let other_at = serve(&other).await;
    let moved = Arc::new(drafted(&other_at, &[&at, &other_at]));
    assert!(!world.keeper.allows(ticket.session, &moved));
    let refused = world
        .keeper
        .lease(request(&key, &moved, Wait::DEFAULT))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::NEEDS_LOGIN);

    // The token runs out: a session made on the drafts' word is not
    // renewed without its person, under the drafts or a published version.
    world.clock.advance(Duration::from_secs(3600));
    let refused = world
        .keeper
        .lease(request(&key, &draft, Wait::NoWait))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::NEEDS_LOGIN);
    let published = Arc::new((*draft).clone().published());
    let refused = world
        .keeper
        .lease(request(&key, &published, Wait::NoWait))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::NEEDS_LOGIN);
    world.keeper.idle().await;
    assert_eq!(heard(&service), 1);
    assert_eq!(heard(&other), 0);

    // Under a published version the keeper signs in and renews without
    // anyone, at the published addresses.
    let ticket = world
        .keeper
        .sign_in(&key, Arc::clone(&published), None)
        .await
        .unwrap();
    world
        .keeper
        .signed_in(ticket.session, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(heard(&service), 2);
    world.clock.advance(Duration::from_secs(3600));
    world
        .keeper
        .lease(request(&key, &published, Wait::DEFAULT))
        .await
        .unwrap();
    world.keeper.idle().await;
    assert_eq!(heard(&service), 3);
    assert_eq!(heard(&other), 0);
}

#[tokio::test]
async fn a_credential_opens_only_for_the_target_it_was_stored_for() {
    let world = World::new(STORED_HELD).await;
    let key = account_key("robot");
    let service = Shared::default();
    let at = serve(&service).await;

    // Another target's reference, named by this target: refused by name,
    // and nothing is opened or sent.
    let theirs = a2m_core::CredentialRef::new("vault://other/client/secret").unwrap();
    world
        .credentials
        .put(
            &key.workspace,
            &key.identity,
            &theirs,
            &Secret::new(CLIENT_SECRET),
        )
        .await
        .unwrap();
    let mut borrowed = drafted(&at, &[&at]).published();
    borrowed
        .credentials
        .insert("client_secret".to_owned(), theirs.clone());
    let borrowed = Arc::new(borrowed);
    let refused = world
        .keeper
        .lease(request(&key, &borrowed, Wait::DEFAULT))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::CREDENTIAL_OF_ANOTHER_TARGET);
    assert_eq!(refused.class(), FailureClass::Input);
    assert!(!refused.message().contains(CLIENT_SECRET));
    assert_eq!(heard(&service), 0);

    // A credential sealed before the target was part of its context, as
    // every one stored before decision 0248 was: it does not open, and
    // the person is told to store it again.
    let ours = reference("client/secret");
    let old = SealContext {
        workspace: key.workspace,
        owner: OwnerRef::of(&key.identity),
        kind: SealedKind::Credential,
        item: ItemId::new(ours.as_str()).unwrap(),
    };
    let sealed = world
        .vault
        .seal(&old, CLIENT_SECRET.as_bytes())
        .await
        .unwrap();
    world
        .store
        .put_credential(
            key.workspace,
            key.identity.to_string(),
            ours.clone(),
            sealed,
        )
        .await
        .unwrap();
    let entry = Arc::new(drafted(&at, &[&at]).published());
    let refused = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::CREDENTIAL_UNREADABLE, "{refused}");
    world.keeper.idle().await;
    assert_eq!(heard(&service), 0);

    // Stored again, it opens, for this target alone.
    world
        .store_credential(&key, "client/secret", CLIENT_SECRET)
        .await;
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    assert_eq!(heard(&service), 1);
    assert!(heard_the_secret(&service));
}
