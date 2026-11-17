//! The person surface on one machine, with the real kernel: a write rests,
//! its page is the only way to the approval desk, a press needs the page's
//! one-time value, a press is not an approval, an approval is for the
//! change the page showed, silence is a decline, and the listener is open
//! only while a page waits.

use std::sync::Arc;
use std::time::Duration;

use a2m_actions::{
    ApprovalAsk, CloseReason, PersonSurface, PresenceAsk, Response, SessionAsk, SignInAsk,
};
use a2m_auth::{PersonAnswer, PersonAsk, PersonReason};
use a2m_core::{Digest, EffectClass, FailureClass, Identity, RunId};
use a2m_policy::PresenceCheck;
use reqwest::header::{CONTENT_TYPE, HOST, ORIGIN};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value as Json, json};

use super::world::engine::ITEM_UPSERT;
use super::world::{TestPrompt, World};
use crate::{PresenceKind, PromptAnswer, codes, presence_available, system_prompt};

const LONG: Duration = Duration::from_secs(900);
const FORM: &str = "application/x-www-form-urlencoded";

fn http() -> Client {
    Client::builder().no_proxy().build().unwrap()
}

fn item() -> Json {
    json!({
        "order_id": "A-102",
        "item_code": "L-300",
        "description": "New rail",
        "quantity": 3,
        "length_in": "150.5",
    })
}

/// A write that rests at `needs_approval`, as its caller sees it.
struct Rested {
    run: String,
    link: String,
    origin: String,
    diff_hash: String,
}

async fn rest(world: &World) -> Rested {
    let rested = world
        .call("tool.call", json!({"tool": ITEM_UPSERT, "input": item()}))
        .await;
    let result = rested.result.clone().unwrap();
    assert_eq!(result["status"], "needs_approval", "{rested:?}");
    let link = result["link"].as_str().unwrap().to_owned();
    assert!(link.starts_with("http://127.0.0.1:"), "{link}");
    let origin = link[..link.find("/approve/").unwrap()].to_owned();
    Rested {
        run: result["run_id"].as_str().unwrap().to_owned(),
        origin,
        link,
        diff_hash: result["diff"]["hash"].as_str().unwrap().to_owned(),
    }
}

async fn status(world: &World, run: &str) -> String {
    let got = world.call("run.get", json!({"run_id": run})).await;
    got.result.unwrap()["status"].as_str().unwrap().to_owned()
}

/// Resumes an approved run, as the result's next step says. The kernel's
/// `run.resume` now and then answers in the moment between the runner's
/// claim of the job and its work on it, with the run still at rest; a
/// caller then resumes again, and so does this.
async fn resume_approved(world: &World, run: &str) -> Response {
    let mut tries = 0;
    loop {
        let resumed = world.call("run.resume", json!({"run_id": run})).await;
        let rests = resumed.result.as_ref().unwrap()["status"] == "needs_approval";
        tries += 1;
        if !rests || tries == 5 {
            return resumed;
        }
    }
}

async fn written(world: &World) -> usize {
    world.app.write_count("A-102", "L-300").await.unwrap()
}

/// Opens the page and returns its text and its one-time value.
async fn open(http: &Client, link: &str) -> (String, String) {
    let page = http.get(link).send().await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let text = page.text().await.unwrap();
    let marker = "name=\"once\" value=\"";
    let from = text.find(marker).unwrap() + marker.len();
    let once = text[from..from + text[from..].find('"').unwrap()].to_owned();
    (text, once)
}

/// Presses `which` on the page, as the page's own form would.
async fn press(http: &Client, rested: &Rested, which: &str, once: &str) -> (StatusCode, String) {
    let answer = http
        .post(format!("{}/{which}", rested.link))
        .header(ORIGIN, &rested.origin)
        .header(CONTENT_TYPE, FORM)
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap();
    (answer.status(), answer.text().await.unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_is_approved_only_through_its_page_with_the_one_time_value_and_the_prompt() {
    let prompt = TestPrompt::answering(PromptAnswer::Dismissed);
    let world = World::ready(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    let rested = rest(&world).await;
    assert_eq!(world.surface.waiting(), 1);
    // The run is looked at again and again while it waits: one page.
    let again = world.call("run.get", json!({"run_id": rested.run})).await;
    assert_eq!(again.result.unwrap()["link"], rested.link.as_str());
    assert_eq!(world.surface.waiting(), 1);

    // The page names the tool, the run, and the hash of the change, loads
    // nothing from elsewhere, and runs no script.
    let answer = http.get(&rested.link).send().await.unwrap();
    let policy = answer.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(policy.starts_with("default-src 'none'"), "{policy}");
    assert_eq!(answer.headers()["x-frame-options"], "DENY");
    let text = answer.text().await.unwrap();
    for shown in [ITEM_UPSERT, rested.run.as_str(), rested.diff_hash.as_str()] {
        assert!(text.contains(shown), "{shown} in {text}");
    }
    assert!(!text.contains("<script") && !text.contains("src=") && !text.contains("href="));

    // The listener answers its page and its two presses. Every other path
    // and method, a front door's included, answers nothing.
    let code = rested.link.rsplit('/').next().unwrap();
    let elsewhere = [
        (Method::GET, "/".to_owned()),
        (Method::GET, "/approve".to_owned()),
        (Method::GET, format!("/approve/{code}x")),
        (Method::GET, format!("/approve/{code}/approve")),
        (Method::GET, format!("/approve/{code}/decline")),
        (Method::POST, format!("/approve/{code}")),
        (Method::PUT, format!("/approve/{code}/approve")),
        (Method::DELETE, format!("/approve/{code}")),
        (Method::POST, format!("/approve/{code}/approve/now")),
        (Method::GET, "/api/v1/tools".to_owned()),
        (Method::POST, "/api/v1/actions/run.resume".to_owned()),
        (Method::POST, "/mcp".to_owned()),
        (Method::GET, "/local/hello".to_owned()),
    ];
    for (method, path) in elsewhere {
        let answer = http
            .request(method.clone(), format!("{}{path}", rested.origin))
            .header(ORIGIN, &rested.origin)
            .header(CONTENT_TYPE, FORM)
            .body("once=0")
            .send()
            .await
            .unwrap();
        assert!(
            matches!(
                answer.status(),
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ),
            "{method} {path}: {}",
            answer.status()
        );
    }

    // A press without the page's one-time value records nothing, and
    // spends the value of the page that was open.
    let (_, once) = open(&http, &rested.link).await;
    let (refused, _) = press(&http, &rested, "approve", "").await;
    assert_eq!(refused, StatusCode::FORBIDDEN);
    let (refused, _) = press(&http, &rested, "approve", &once).await;
    assert_eq!(refused, StatusCode::FORBIDDEN);
    assert_eq!(prompt.asked(), 0);

    // A press that does not come from the page itself is stopped before it
    // is looked at: another site's, one with no origin, one for another
    // host. The page's value is not spent by them.
    let (_, once) = open(&http, &rested.link).await;
    let approve = format!("{}/approve", rested.link);
    let body = format!("once={once}");
    let foreign = [
        http.post(&approve).header(ORIGIN, "http://example.invalid"),
        http.post(&approve),
        http.post(&approve)
            .header(ORIGIN, &rested.origin)
            .header(HOST, "example.invalid"),
        http.get(&rested.link).header(HOST, "localhost:1"),
        http.get(&rested.link).header(ORIGIN, "null"),
    ];
    for request in foreign {
        let answer = request
            .header(CONTENT_TYPE, FORM)
            .body(body.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(answer.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(prompt.asked(), 0);

    // A press is not an approval. With the page's value the prompt is
    // raised; it is not passed, and nothing is approved or written.
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::FORBIDDEN, "{said}");
    assert_eq!(prompt.asked(), 1);
    assert_eq!(status(&world, &rested.run).await, "needs_approval");
    assert_eq!(written(&world).await, 0);
    // The value was for one press.
    let (again, _) = press(&http, &rested, "approve", &once).await;
    assert_eq!(again, StatusCode::FORBIDDEN);
    assert_eq!(prompt.asked(), 1);

    // The person passes the prompt: the approval is recorded, for the
    // change the page showed.
    prompt.set(PromptAnswer::Present);
    let (_, once) = open(&http, &rested.link).await;
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::OK, "{said}");
    assert_eq!(prompt.asked(), 2);

    // Nothing waits any more, so nothing listens: the port is closed. A
    // look at the run before it is resumed opens no second page.
    world.surface.settled().await;
    assert!(http.get(&rested.link).send().await.is_err());
    let looked = world.call("run.get", json!({"run_id": rested.run})).await;
    let looked = looked.result.unwrap();
    assert_eq!(looked["status"], "needs_approval");
    assert!(looked["link"].is_null(), "{looked}");
    assert!(looked["person"].as_str().unwrap().contains("approved"));
    assert_eq!(world.surface.waiting(), 0);

    let done = resume_approved(&world, &rested.run).await;
    assert!(done.ok, "{:?} {:?}", done.error, done.result);
    assert_eq!(done.result.as_ref().unwrap()["status"], "completed");
    // The test application holds exactly one write, with the values.
    assert_eq!(written(&world).await, 1);
    let stored = world
        .app
        .stored_item("A-102", "L-300")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.description, "New rail");
    assert_eq!(stored.quantity, 3);
    // The ledger tells it as a person's approval through the local prompt.
    let trace = world
        .call("trace.show", json!({"run_id": rested.run}))
        .await;
    let events = trace.result.unwrap()["events"].to_string();
    assert!(events.contains("approval_bound"), "{events}");

    world.surface.settled().await;
    assert_eq!(world.surface.waiting(), 0);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_approval_is_only_for_the_change_the_page_showed() {
    let prompt = TestPrompt::answering(PromptAnswer::Present);
    let world = World::ready(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    let rested = rest(&world).await;
    let run: RunId = rested.run.parse().unwrap();

    // A page for the same run that shows another change than the one that
    // waits. It takes the place of the true page, whose address is gone.
    let other = Digest::of(b"another change");
    let shown = world
        .surface
        .ask_approval(ApprovalAsk {
            run,
            workspace: world.caller.principal.workspace(),
            started_by: world.caller.principal.principal().id,
            tool: ITEM_UPSERT.to_owned(),
            diff_hash: other,
            summary: "Another change.".to_owned(),
        })
        .await
        .unwrap();
    let forged = Rested {
        link: shown.url.unwrap(),
        ..rest_like(&rested)
    };
    assert_ne!(forged.link, rested.link);
    assert_eq!(world.surface.waiting(), 1);
    assert_eq!(
        http.get(&rested.link).send().await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    let (text, once) = open(&http, &forged.link).await;
    assert!(text.contains(&other.to_string()));

    // The person is present and presses Approve. Nothing is recorded: the
    // change this page showed is not the one that waits. The prompt is not
    // even raised.
    let (answered, said) = press(&http, &forged, "approve", &once).await;
    assert_eq!(answered, StatusCode::CONFLICT, "{said}");
    assert_eq!(prompt.asked(), 0);
    world.surface.settled().await;
    let still = world
        .call("run.resume", json!({"run_id": rested.run}))
        .await;
    assert_eq!(still.result.unwrap()["status"], "needs_approval");
    assert_eq!(written(&world).await, 0);
    world.kernel.close(CloseReason::Asked).await;
}

fn rest_like(rested: &Rested) -> Rested {
    Rested {
        run: rested.run.clone(),
        link: rested.link.clone(),
        origin: rested.origin.clone(),
        diff_hash: rested.diff_hash.clone(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silence_is_a_decline_and_the_listener_closes() {
    // The person would pass the prompt, and never comes.
    let prompt = TestPrompt::answering(PromptAnswer::Present);
    let world = World::ready(Arc::clone(&prompt) as _, Duration::from_millis(50)).await;
    let http = http();
    let rested = rest(&world).await;

    tokio::time::timeout(Duration::from_secs(60), world.surface.settled())
        .await
        .unwrap();
    assert_eq!(world.surface.waiting(), 0);
    assert!(http.get(&rested.link).send().await.is_err());
    assert_eq!(prompt.asked(), 0);

    // Nothing was approved, so the run does not go on and nothing is
    // written.
    let after = world
        .call("run.resume", json!({"run_id": rested.run}))
        .await;
    let result = after.result.clone().unwrap();
    assert_ne!(result["status"], "completed", "{after:?}");
    // The answer stands: the run gets no second page.
    assert!(result["link"].is_null(), "{after:?}");
    assert_eq!(world.surface.waiting(), 0);
    assert_eq!(written(&world).await, 0);
    let trace = world
        .call("trace.show", json!({"run_id": rested.run}))
        .await;
    let events = trace.result.unwrap()["events"].to_string();
    assert!(!events.contains("approval_bound"), "{events}");
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_no_presence_prompt_nothing_is_approved_and_a_decline_still_works() {
    // The prompt this build ships: none, on every operating system.
    assert_eq!(presence_available(), PresenceKind::NotAvailable);
    let world = World::ready(system_prompt(), LONG).await;
    assert_eq!(world.surface.presence(), PresenceKind::NotAvailable);
    let http = http();
    let rested = rest(&world).await;

    let (_, once) = open(&http, &rested.link).await;
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::FORBIDDEN);
    assert!(said.contains("Nothing was approved"), "{said}");
    assert_eq!(status(&world, &rested.run).await, "needs_approval");

    // A protected setting cannot be loosened here: no proof is given.
    let person = world.caller.principal.principal().id;
    let refused = world
        .surface
        .confirm_presence(PresenceAsk {
            principal: person,
            action: "config.set".parse().unwrap(),
            effect: EffectClass::Write,
            summary: "Loosen a protected setting.".to_owned(),
        })
        .await
        .unwrap_err();
    assert_eq!(refused.class(), FailureClass::Policy);
    assert_eq!(*refused.code(), codes::PRESENCE_FAILED);

    // What is not built says so: no page for a sign-in, and no way to the
    // session's person.
    let sign_in = world
        .surface
        .ask_sign_in(SignInAsk {
            session: None,
            workspace: world.caller.principal.workspace(),
            target: "example".parse().unwrap(),
            environment: "prod".parse().unwrap(),
            identity: Identity::Person(person),
            reason: PersonReason::SignIn,
        })
        .await
        .unwrap();
    assert_eq!(sign_in.url, None);
    assert!(world.surface.return_address().is_none());
    let unreachable = world
        .surface
        .ask_session(SessionAsk {
            session: a2m_core::SessionId::from_bytes([7; 16]),
            workspace: world.caller.principal.workspace(),
            target: "example".parse().unwrap(),
            environment: "prod".parse().unwrap(),
            identity: Identity::Person(person),
            ask: PersonAsk::Notify {
                reason: PersonReason::SignIn,
            },
        })
        .await;
    assert!(matches!(unreachable, PersonAnswer::Unreachable));

    // Declining needs no proof of presence, and ends the wait.
    let (_, once) = open(&http, &rested.link).await;
    let (answered, said) = press(&http, &rested, "decline", &once).await;
    assert_eq!(answered, StatusCode::OK, "{said}");
    world.surface.settled().await;
    assert!(http.get(&rested.link).send().await.is_err());
    let after = world
        .call("run.resume", json!({"run_id": rested.run}))
        .await;
    let result = after.result.unwrap();
    assert_ne!(result["status"], "completed");
    assert!(result["link"].is_null(), "{result}");
    assert!(result["person"].as_str().unwrap().contains("declined"));
    assert_eq!(written(&world).await, 0);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proof_of_presence_comes_only_from_a_passed_prompt() {
    let prompt = TestPrompt::answering(PromptAnswer::Present);
    let world = World::ready(Arc::clone(&prompt) as _, LONG).await;
    let person = world.caller.principal.principal().id;
    let ask = || PresenceAsk {
        principal: person,
        action: "config.set".parse().unwrap(),
        effect: EffectClass::Write,
        summary: "Loosen a protected setting.".to_owned(),
    };
    let proof = world.surface.confirm_presence(ask()).await.unwrap();
    assert_eq!(proof.check(), PresenceCheck::OsPrompt);
    assert_eq!(proof.principal(), person);
    // A dialog that proves no person, and a dismissed prompt, give none.
    for answer in [PromptAnswer::Confirmed, PromptAnswer::Dismissed] {
        prompt.set(answer);
        let refused = world.surface.confirm_presence(ask()).await.unwrap_err();
        assert_eq!(*refused.code(), codes::PRESENCE_FAILED, "{answer:?}");
    }
    assert_eq!(prompt.asked(), 3);
    // Nothing waited, so nothing ever listened.
    assert_eq!(world.surface.waiting(), 0);
    world.kernel.close(CloseReason::Asked).await;
}
