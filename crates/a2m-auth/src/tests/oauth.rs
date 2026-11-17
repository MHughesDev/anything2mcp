//! The providers that exchange a credential, against a small identity
//! service the test runs: a token endpoint that keeps what it was sent.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use a2m_core::{FailureClass, Redact, Secret, SessionState};
use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use sha2::{Digest, Sha256};

use super::support::{
    HOUR, STORED_HELD, World, account_key, assert_clean, decode_form, logged, person_id,
    person_key, placed, reference, request, settle,
};
use crate::encode::{base64, base64_url};
use crate::{
    AuthEntry, PersonAnswer, PersonAsk, PersonReason, ProviderName, Returned, Wait, codes,
};

const CLIENT_SECRET: &str = "PLANTED-client-secret-01";
const CODE: &str = "PLANTED-code-02";

/// What the identity service remembers.
#[derive(Default)]
struct Service {
    /// Every request to the token endpoint, as its form.
    requests: Vec<BTreeMap<String, String>>,
    /// Tokens issued so far.
    issued: u32,
    /// The refresh token it will accept next.
    refresh: String,
    /// The proof-key challenge of the link the person opened.
    challenge: String,
    /// Whether it is down.
    down: bool,
}

type Shared = Arc<Mutex<Service>>;

fn answer(status: StatusCode, body: serde_json::Value) -> (StatusCode, String) {
    (status, body.to_string())
}

async fn token(
    service: Shared,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, String) {
    let form = decode_form(&body);
    let mut service = service.lock().unwrap();
    service.requests.push(form.clone());
    if service.down {
        return answer(StatusCode::SERVICE_UNAVAILABLE, serde_json::json!({}));
    }
    let basic = format!("Basic {}", base64(format!("client-1:{CLIENT_SECRET}").as_bytes()));
    let proved = headers
        .get("authorization")
        .is_some_and(|value| value.as_bytes() == basic.as_bytes());
    let refuse = |error: &str| answer(StatusCode::BAD_REQUEST, serde_json::json!({ "error": error }));
    let field = |name: &str| form.get(name).map(String::as_str);
    match field("grant_type") {
        Some("client_credentials") if proved => {}
        Some("client_credentials") => {
            return answer(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({ "error": "invalid_client", "error_description": "PLANTED-description" }),
            );
        }
        Some("authorization_code") => {
            let verifier = field("code_verifier").unwrap_or_default();
            let hashed = base64_url(&Sha256::digest(verifier.as_bytes()));
            if field("code") != Some(CODE) || hashed != service.challenge {
                return refuse("invalid_grant");
            }
        }
        Some("refresh_token") => {
            if field("refresh_token") != Some(service.refresh.as_str()) {
                return refuse("invalid_grant");
            }
        }
        _ => return refuse("unsupported_grant_type"),
    }
    service.issued += 1;
    let issued = service.issued;
    let mut body = serde_json::json!({
        "access_token": format!("PLANTED-access-{issued}"),
        "token_type": "Bearer",
        // Written as text, as some services do.
        "expires_in": "3600",
    });
    if field("grant_type") != Some("client_credentials") {
        service.refresh = format!("PLANTED-refresh-{issued}");
        body["refresh_token"] = service.refresh.clone().into();
    }
    answer(StatusCode::OK, body)
}

/// Starts the identity service on a free port of this machine and returns
/// its address.
async fn serve(service: &Shared) -> String {
    let shared = Arc::clone(service);
    let app = Router::new().route(
        "/token",
        post(move |headers: HeaderMap, body: String| token(Arc::clone(&shared), headers, body)),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

fn grants(service: &Shared) -> Vec<String> {
    service
        .lock()
        .unwrap()
        .requests
        .iter()
        .map(|form| form.get("grant_type").cloned().unwrap_or_default())
        .collect()
}

fn client_credentials(address: &str) -> Arc<AuthEntry> {
    Arc::new(
        AuthEntry::new(ProviderName::new("oauth_client_credentials").unwrap())
            .setting("token_endpoint", &format!("{address}/token"))
            .setting("client_id", "client-1")
            .setting("scope", "records.read records.write")
            .credential("client_secret", reference("client/secret")),
    )
}

#[test]
fn client_credentials_are_exchanged_once_renewed_once_and_shown_nowhere() {
    let world = World::new(STORED_HELD);
    let service = Shared::default();
    let key = account_key("robot");
    let mut texts = Vec::new();

    logged(&world.telemetry, async {
        let entry = client_credentials(&serve(&service).await);
        world
            .store_credential(&key, "client/secret", CLIENT_SECRET)
            .await;

        let lease = world
            .keeper
            .lease(request(&key, &entry, Wait::DEFAULT))
            .await
            .unwrap();
        assert_eq!(
            placed(&lease),
            [(
                "header Authorization".to_owned(),
                "Bearer PLANTED-access-1".to_owned()
            )]
        );
        // What the service was sent: the grant and the scope in the body,
        // the secret in the header and not in the body.
        {
            let seen = service.lock().unwrap();
            assert_eq!(seen.requests.len(), 1);
            assert_eq!(seen.requests[0]["scope"], "records.read records.write");
            assert!(!seen.requests[0].contains_key("client_secret"));
        }

        // An hour later ten runs find the token run out: one exchange.
        world.clock.advance(HOUR);
        let runs: Vec<_> = (0..10)
            .map(|_| {
                let keeper = world.keeper.clone();
                let req = request(&key, &entry, Wait::DEFAULT);
                tokio::spawn(async move { keeper.lease(req).await })
            })
            .collect();
        for run in runs {
            let lease = run.await.unwrap().unwrap();
            assert_eq!(placed(&lease)[0].1, "Bearer PLANTED-access-2");
            texts.push(format!("{lease:?}"));
        }
        assert_eq!(grants(&service), ["client_credentials"; 2]);

        // The service goes down: the call is told so, and it is not a
        // refusal of the credential.
        world.clock.advance(HOUR);
        service.lock().unwrap().down = true;
        let down = world
            .keeper
            .lease(request(&key, &entry, Wait::DEFAULT))
            .await
            .unwrap_err();
        assert_eq!(down.class(), FailureClass::Transient);
        assert_eq!(down.code(), &codes::UPSTREAM_UNAVAILABLE);
        texts.push(format!("{down:?}"));
        service.lock().unwrap().down = false;
        world
            .keeper
            .lease(request(&key, &entry, Wait::DEFAULT))
            .await
            .unwrap();

        // A wrong secret is refused by the service, and offered only once.
        let wrong = account_key("intruder");
        world
            .store_credential(&wrong, "client/secret", "PLANTED-wrong-secret-03")
            .await;
        let before = grants(&service).len();
        for _ in 0..5 {
            let refused = world
                .keeper
                .lease(request(&wrong, &entry, Wait::DEFAULT))
                .await
                .unwrap_err();
            assert_eq!(refused.code(), &codes::CREDENTIAL_REFUSED);
            texts.push(format!("{refused:?} {refused}"));
        }
        assert_eq!(grants(&service).len(), before + 1);
        assert_eq!(world.state(&wrong).await, SessionState::NeedsPerson);
        texts.push(format!(
            "{:?}",
            world.keeper.status(&key.workspace, None).await.unwrap()
        ));
    });

    for secret in [
        CLIENT_SECRET,
        "PLANTED-access-1",
        "PLANTED-access-2",
        "Bearer PLANTED-access-2",
        "PLANTED-wrong-secret-03",
    ] {
        assert!(world.is_registered(secret), "{secret}");
    }
    texts.extend(world.telemetry.log_lines());
    assert_clean(&texts, &["PLANTED"]);
}

#[test]
fn an_authorization_code_comes_from_a_link_the_person_opens_and_the_refresh_token_rotates() {
    let world = World::new(STORED_HELD);
    let service = Shared::default();
    let key = person_key(1);
    let mut texts = Vec::new();

    logged(&world.telemetry, async {
        let address = serve(&service).await;
        let entry = Arc::new(
            AuthEntry::new(ProviderName::new("oauth_authorization_code").unwrap())
                .setting("authorization_endpoint", "https://id.example.test/authorize")
                .setting("token_endpoint", &format!("{address}/token"))
                .setting("client_id", "client-1")
                .setting("scope", "records.read"),
        );

        // The person: opens the link in their own browser, agrees, and the
        // browser comes back with the code. Nothing here drives a browser;
        // the test plays the identity service's part of the redirect.
        let opened = Arc::new(Mutex::new(Vec::new()));
        {
            let service = Arc::clone(&service);
            let opened = Arc::clone(&opened);
            world.person.answers(move |ask| match ask {
                PersonAsk::OpenLink { url, reason } => {
                    assert_eq!(*reason, PersonReason::Consent);
                    opened.lock().unwrap().push(url.clone());
                    let query = decode_form(url.split_once('?').unwrap().1);
                    assert_eq!(query["response_type"], "code");
                    assert_eq!(query["client_id"], "client-1");
                    assert_eq!(query["redirect_uri"], "http://127.0.0.1:1/returned");
                    assert_eq!(query["code_challenge_method"], "S256");
                    assert_eq!(query["scope"], "records.read");
                    service.lock().unwrap().challenge = query["code_challenge"].clone();
                    PersonAnswer::Done(
                        Returned::new()
                            .with("code", Secret::new(CODE))
                            .with("state", Secret::new(query["state"].clone())),
                    )
                }
                _ => PersonAnswer::NoAnswer,
            });
        }

        // Nobody signs in for the person: a lease alone starts nothing.
        let absent = world
            .keeper
            .lease(request(&key, &entry, Wait::NoWait))
            .await
            .unwrap_err();
        assert_eq!(absent.code(), &codes::NEEDS_LOGIN);
        assert!(opened.lock().unwrap().is_empty());

        world
            .keeper
            .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
            .await
            .unwrap();
        let lease = world
            .keeper
            .lease(request(&key, &entry, Wait::DEFAULT))
            .await
            .unwrap();
        assert_eq!(placed(&lease)[0].1, "Bearer PLANTED-access-1");
        let link = opened.lock().unwrap()[0].clone();
        assert!(link.starts_with("https://id.example.test/authorize?"));
        {
            // The service was sent the code, the same return address, and a
            // verifier that hashes to the challenge in the link.
            let seen = service.lock().unwrap();
            assert_eq!(seen.requests[0]["redirect_uri"], "http://127.0.0.1:1/returned");
            assert_eq!(seen.requests[0]["client_id"], "client-1");
            let verifier = seen.requests[0]["code_verifier"].clone();
            assert!(!link.contains(&verifier));
            assert!(world.is_registered(&verifier));
        }

        // Two renewals, each with the refresh token the last one gave.
        for round in 2..4 {
            world.clock.advance(HOUR);
            let lease = world
                .keeper
                .lease(request(&key, &entry, Wait::DEFAULT))
                .await
                .unwrap();
            assert_eq!(lease.epoch(), round);
            assert_eq!(
                placed(&lease)[0].1,
                format!("Bearer PLANTED-access-{round}")
            );
            texts.push(format!("{lease:?}"));
        }
        assert_eq!(
            grants(&service),
            ["authorization_code", "refresh_token", "refresh_token"]
        );
        {
            let seen = service.lock().unwrap();
            assert_eq!(seen.requests[1]["refresh_token"], "PLANTED-refresh-1");
            assert_eq!(seen.requests[2]["refresh_token"], "PLANTED-refresh-2");
        }

        // The service stops accepting the refresh token: the person is
        // wanted, the token is offered once, and calls stop.
        service.lock().unwrap().refresh = "PLANTED-revoked".to_owned();
        world.clock.advance(HOUR);
        for _ in 0..3 {
            let stopped = world
                .keeper
                .lease(request(&key, &entry, Wait::NoWait))
                .await
                .unwrap_err();
            assert_eq!(stopped.code(), &codes::NEEDS_LOGIN);
            texts.push(format!("{stopped:?}"));
        }
        assert_eq!(grants(&service).len(), 4);
        assert_eq!(world.state(&key).await, SessionState::NeedsPerson);

        // An answer that belongs to another sign-in is not exchanged.
        world.person.answers(|_| {
            PersonAnswer::Done(
                Returned::new()
                    .with("code", Secret::new(CODE))
                    .with("state", Secret::new("PLANTED-other-state")),
            )
        });
        world
            .keeper
            .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
            .await
            .unwrap();
        settle().await;
        assert_eq!(grants(&service).len(), 4);
        assert_eq!(world.state(&key).await, SessionState::Absent);

        // A person who says no leaves the session absent.
        world.person.answers(|_| PersonAnswer::Declined);
        world
            .keeper
            .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
            .await
            .unwrap();
        let declined = world
            .keeper
            .lease(request(&key, &entry, Wait::DEFAULT))
            .await
            .unwrap_err();
        assert_eq!(declined.code(), &codes::NEEDS_LOGIN);
        texts.push(format!("{declined:?}"));
        texts.push(format!("{:?}", world.person.asks.lock().unwrap()));
        texts.push(format!(
            "{:?}",
            world.keeper.timeline(lease.session()).await.unwrap()
        ));
    });

    for secret in [
        CODE,
        "PLANTED-access-3",
        "PLANTED-refresh-1",
        "PLANTED-refresh-3",
        "PLANTED-other-state",
    ] {
        assert!(world.vault.redactor().contains_secret(secret), "{secret}");
    }
    texts.extend(world.telemetry.log_lines());
    assert_clean(&texts, &["PLANTED"]);
}
