//! The person surface on one machine, with the real kernel: a write rests,
//! its page shows the stored change and is the only way to the approval
//! desk, a press needs the page's one-time value, a press is not an
//! approval, an approval is for the change the page showed, one prompt is
//! up at a time and ends with the window, silence is a decline, and the
//! listener is open only while a page waits.

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
use super::world::{PASSED, TestPrompt, World, within};
use crate::{
    Availability, NoPrompt, PresenceKind, PresenceReason, PromptAnswer, PromptName, codes,
    presence_available,
};

const LONG: Duration = Duration::from_secs(900);
const FORM: &str = "application/x-www-form-urlencoded";

fn http() -> Client {
    Client::builder().no_proxy().build().unwrap()
}

/// The description the write carries: text from a system, with markup in
/// it. The page must show it as text.
const DESCRIPTION: &str = "<script>alert(1)</script> New <b>rail</b> & \"more\" {{once}}";

fn item() -> Json {
    json!({
        "order_id": "A-102",
        "item_code": "L-300",
        "description": DESCRIPTION,
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
    rest_with(world, item()).await
}

async fn rest_with(world: &World, input: Json) -> Rested {
    let rested = world
        .call("tool.call", json!({"tool": ITEM_UPSERT, "input": input}))
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

/// Resumes an approved run, as the result's next step says.
async fn resume_approved(world: &World, run: &str) -> Response {
    world.call("run.resume", json!({"run_id": run})).await
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
    // It shows the stored change: where the write goes, how many records,
    // and each field of each record, from what to what. The first twelve
    // characters of the hash stand beside Approve.
    for shown in [
        "Target <code>example</code>",
        "1 to create, 0 to change, 0 already as intended: 1 records in all.",
        "<th scope=\"row\">description</th><td><em>does not exist</em></td>",
        "<th scope=\"row\">quantity</th><td><em>does not exist</em></td><td>3</td><td>yes</td>",
        &format!("Change <code>{}</code>", &rested.diff_hash[..12]),
        // The prompt this machine has, in the prompt's own words.
        "The prompt of a test.",
    ] {
        assert!(text.contains(shown), "{shown} in {text}");
    }
    // A value from a system is text on the page, never markup, and never
    // a place of the page to fill.
    assert!(
        text.contains(
            "&lt;script&gt;alert(1)&lt;/script&gt; New &lt;b&gt;rail&lt;/b&gt; &amp; &quot;more&quot; {{once}}"
        ),
        "{text}"
    );
    assert!(!text.contains("<script") && !text.contains("<b>"), "{text}");
    assert!(!text.contains("src=") && !text.contains("href="));

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
    prompt.set(PASSED);
    let (_, once) = open(&http, &rested.link).await;
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::OK, "{said}");
    assert_eq!(prompt.asked(), 2);
    // The answer names the prompt that was passed.
    assert!(said.contains("(polkit)"), "{said}");
    // Each prompt was given the end of the page's window as its deadline.
    let deadlines = prompt.deadlines();
    assert_eq!(deadlines[0], deadlines[1]);
    assert!(deadlines[0] <= tokio::time::Instant::now() + LONG);

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
    assert_eq!(stored.description, DESCRIPTION);
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
    let prompt = TestPrompt::answering(PASSED);
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

    // The page shows the stored change or nothing. The desk reads the
    // diff back and the hash is not the one this page was opened for, so
    // the page shows nothing, gives no one-time value, and cannot press.
    let shown = http.get(&forged.link).send().await.unwrap();
    assert_eq!(shown.status(), StatusCode::CONFLICT);
    let text = shown.text().await.unwrap();
    assert!(!text.contains("name=\"once\""), "{text}");
    let (answered, said) = press(&http, &forged, "approve", "0").await;
    assert_eq!(answered, StatusCode::FORBIDDEN, "{said}");
    assert_eq!(prompt.asked(), 0);
    let still = world
        .call("run.resume", json!({"run_id": rested.run}))
        .await;
    assert_eq!(still.result.unwrap()["status"], "needs_approval");
    assert_eq!(written(&world).await, 0);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_press_for_a_change_that_no_longer_waits_raises_no_prompt() {
    let prompt = TestPrompt::answering(PASSED);
    let world = World::ready(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    let rested = rest(&world).await;
    // The person has the page open, with its one-time value.
    let (_, once) = open(&http, &rested.link).await;
    // Meanwhile the run is cancelled by the program that started it.
    let cancelled = world
        .call("run.cancel", json!({"run_id": rested.run}))
        .await;
    assert!(cancelled.ok, "{cancelled:?}");

    // The person is present and presses Approve. Nothing is recorded: the
    // change this page showed no longer waits. The prompt is not even
    // raised, and the page goes.
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::CONFLICT, "{said}");
    assert_eq!(prompt.asked(), 0);
    world.surface.settled().await;
    assert_eq!(world.surface.waiting(), 0);
    assert_eq!(written(&world).await, 0);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_prompt_is_up_at_a_time_and_a_second_press_is_told_busy() {
    let prompt = TestPrompt::answering(PASSED);
    let world = World::ready(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    let first = rest(&world).await;
    let second = rest_with(
        &world,
        json!({
            "order_id": "A-102",
            "item_code": "L-301",
            "description": "Another rail",
            "quantity": 1,
            "length_in": "20",
        }),
    )
    .await;
    assert_eq!(world.surface.waiting(), 2);

    // The person presses Approve on the first page, and its prompt is up:
    // nobody has answered it yet.
    prompt.hold();
    let (_, once) = open(&http, &first.link).await;
    let pressing = tokio::spawn({
        let http = http.clone();
        let first = rest_like(&first);
        async move { press(&http, &first, "approve", &once).await }
    });
    prompt.raised(1).await;

    // A press on the other page raises no second prompt and records
    // nothing: a person never answers a prompt meant for another change.
    let (_, once) = open(&http, &second.link).await;
    let (answered, said) = press(&http, &second, "approve", &once).await;
    assert_eq!(answered, StatusCode::SERVICE_UNAVAILABLE, "{said}");
    assert!(said.contains("Another presence prompt is up"), "{said}");
    // Nor does a protected setting get a prompt of its own meanwhile.
    let person = world.caller.principal.principal().id;
    let busy = world
        .surface
        .confirm_presence(PresenceAsk {
            principal: person,
            action: "config.set".parse().unwrap(),
            effect: EffectClass::Write,
            summary: "Loosen a protected setting.".to_owned(),
        })
        .await
        .unwrap_err();
    assert_eq!(busy.class(), FailureClass::Busy);
    assert_eq!(*busy.code(), codes::PROMPT_BUSY);
    assert_eq!(prompt.asked(), 1);
    assert_eq!(status(&world, &second.run).await, "needs_approval");

    // The person passes the prompt: the first change is approved, and
    // only that one. The second page still waits and can now be answered.
    prompt.release();
    let (answered, said) = within("the first press", pressing).await.unwrap();
    assert_eq!(answered, StatusCode::OK, "{said}");
    assert_eq!(world.surface.waiting(), 1);
    let (_, once) = open(&http, &second.link).await;
    let (answered, said) = press(&http, &second, "decline", &once).await;
    assert_eq!(answered, StatusCode::OK, "{said}");
    world.surface.settled().await;
    let done = resume_approved(&world, &first.run).await;
    assert_eq!(done.result.as_ref().unwrap()["status"], "completed");
    assert_eq!(written(&world).await, 1);
    assert_eq!(world.app.write_count("A-102", "L-301").await.unwrap(), 0);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_prompt_nobody_answers_ends_with_the_window_as_a_decline() {
    // The person would pass the prompt, and leaves it up.
    let prompt = TestPrompt::answering(PASSED);
    prompt.hold();
    let world = World::ready(Arc::clone(&prompt) as _, Duration::from_secs(4)).await;
    let http = http();
    let rested = rest(&world).await;
    let (_, once) = open(&http, &rested.link).await;

    // The press waits for the prompt, and the prompt ends at the end of
    // the page's window, dismissed. Nothing is approved.
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::FORBIDDEN, "{said}");
    assert!(said.contains("its time ended"), "{said}");
    assert_eq!(prompt.asked(), 1);
    // The window ended, which is a decline: the page is gone, the run has
    // ended, and nothing is written, even if the person answers now.
    prompt.release();
    within("the page's end", world.surface.settled()).await;
    assert!(http.get(&rested.link).send().await.is_err());
    assert_eq!(status(&world, &rested.run).await, "expired");
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
    let prompt = TestPrompt::answering(PASSED);
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
    // The prompt of an operating system whose own is not built. The
    // system's prompt is not used here: on a prepared desktop it would
    // raise a real dialog.
    let world = World::ready(Arc::new(NoPrompt), LONG).await;
    let none = world.surface.presence().await;
    assert_eq!(none.kind, PresenceKind::NotAvailable);
    assert_eq!(none.prompt, None);
    assert_eq!(none.reason, PresenceReason::NotBuilt);
    assert_eq!(none.reason.as_str(), "not_built");
    assert!(!none.proves_presence());
    // What this machine's own prompt says of itself raises nothing, and
    // is ready only as a prompt of the operating system.
    let own = presence_available().await;
    assert_eq!(own.proves_presence(), own.reason == PresenceReason::Ready);
    assert_eq!(own.prompt.is_some(), own.kind == PresenceKind::OsPrompt);
    #[cfg(not(target_os = "linux"))]
    assert_eq!(own, none);
    let http = http();
    let rested = rest(&world).await;

    // The page says that no prompt is available, and why, before
    // anything is pressed; a press says the same and records nothing.
    let (text, once) = open(&http, &rested.link).await;
    assert!(
        text.contains("No presence prompt is available on this machine"),
        "{text}"
    );
    assert!(text.contains(&none.message), "{text}");
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::FORBIDDEN);
    assert!(said.contains("Nothing was approved"), "{said}");
    assert!(said.contains(&none.message), "{said}");
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
    let prompt = TestPrompt::answering(PASSED);
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
    // A dialog that proves no person, a prompt that was closed, one that
    // was not passed, and one that could not be raised give none.
    let no_agent = Availability::not_available(
        PresenceReason::NoAgent,
        "Nothing can show the prompt.",
        "Start the agent.",
    );
    for answer in [
        PromptAnswer::Confirmed,
        PromptAnswer::Dismissed,
        PromptAnswer::Denied,
        PromptAnswer::NotAvailable(no_agent),
    ] {
        prompt.set(answer.clone());
        let refused = world.surface.confirm_presence(ask()).await.unwrap_err();
        assert_eq!(refused.class(), FailureClass::Policy, "{answer:?}");
        assert_eq!(*refused.code(), codes::PRESENCE_FAILED, "{answer:?}");
    }
    assert_eq!(prompt.asked(), 5);
    assert_eq!(PromptName::Polkit.as_str(), "polkit");
    assert_eq!(
        PromptName::LocalAuthentication.as_str(),
        "local_authentication"
    );
    assert_eq!(PromptName::WindowsHello.as_str(), "windows_hello");
    // Nothing waited, so nothing ever listened.
    assert_eq!(world.surface.waiting(), 0);
    world.kernel.close(CloseReason::Asked).await;
}
