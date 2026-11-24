//! The person surface on one machine, with the real kernel: a write rests,
//! its page shows the stored change and is the only way to the approval
//! desk, a press needs the page's one-time value, a press is not an
//! approval, an approval is for the change the page showed, one prompt is
//! up at a time and ends with the window, silence is a decline, and the
//! listener is open only while a page waits.

use std::sync::Arc;
use std::time::Duration;

use a2m_actions::{
    ApprovalAsk, CloseReason, NOT_READ_BACK, PersonSurface, PresenceAsk, Response, SessionAsk,
    SignInAsk,
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
    a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap()
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
    // A write that is read back after it is sent is not said to be one
    // that is not, and the binding the approval binds it to is named, with
    // what its evidence means (decision 0185).
    assert!(!text.contains("not read back"), "{text}");
    assert!(
        text.contains(
            "<p>Written by the binding <code>app_json</code>, and by no other: it is read back and compared with what was intended after it is sent.</p><table>"
        ),
        "{text}"
    );

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
/// The upsert ended on the system's acknowledgement, as
/// `acknowledged_upsert` makes it: keyed, with no read-back step, so its
/// binding has nothing to read the record with. Its preview is a `write`
/// (decision 0196): the page counts it apart and shows the program's own
/// words where the record's current values would stand, never a value and
/// never "does not exist".
async fn a_write_that_will_not_be_read_back_says_so_on_its_page_before_it_is_approved() {
    let prompt = TestPrompt::answering(PromptAnswer::Dismissed);
    let world = World::ready_with(
        Arc::clone(&prompt) as _,
        LONG,
        super::world::engine::acknowledged_upsert,
    )
    .await;
    let http = http();
    let rested = rest(&world).await;
    let (text, _) = open(&http, &rested.link).await;
    // Beside the record, after its heading and before its fields, in the
    // words the desk gives.
    let said = format!(
        ": write</h3><p><strong>This write is {NOT_READ_BACK}.</strong> Nothing will read it back or compare it after it is sent.</p><p>Written by the binding <code>app_json</code>, and by no other: it is accepted by the system and not read back: nothing compares it after it is sent.</p><table>"
    );
    assert!(text.contains(&said), "{text}");
    // Counted apart from the creates and changes.
    assert!(
        text.contains("0 to create, 0 to change, 0 already as intended, 1 to write, whose current values cannot be read: 1 records in all."),
        "{text}"
    );
    // Every field's Now column holds the program's words, set apart.
    let words = format!(
        "<td><em class=\"product\">This program's words: {}</em></td>",
        a2m_actions::NOW_CANNOT_BE_READ
    );
    assert_eq!(text.matches(&words).count(), 3, "{text}");
    assert!(!text.contains("does not exist"), "{text}");
    assert_eq!(prompt.asked(), 0);
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
            effect: a2m_core::EffectClass::Write,
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

/// A prompt that does not keep its deadline: it answers that the person
/// passed, a second after the window closed.
struct Late;

impl crate::PresencePrompt for Late {
    fn available(&self) -> a2m_actions::BoxFuture<'_, Availability> {
        Box::pin(async { Availability::ready(PromptName::Polkit, "A late prompt of a test.") })
    }

    fn ask(
        &self,
        _text: crate::PromptText,
        deadline: tokio::time::Instant,
    ) -> a2m_actions::BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            tokio::time::sleep_until(deadline + Duration::from_secs(1)).await;
            PASSED
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_prompt_that_answers_after_the_window_records_nothing() {
    let world = World::ready(Arc::new(Late) as _, Duration::from_secs(3)).await;
    let http = http();
    let rested = rest(&world).await;
    let (_, once) = open(&http, &rested.link).await;
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::FORBIDDEN, "{said}");
    assert!(said.contains("its time ended"), "{said}");
    within("the page's end", world.surface.settled()).await;
    assert_ne!(status(&world, &rested.run).await, "needs_approval");
    assert_ne!(status(&world, &rested.run).await, "completed");
    assert_eq!(written(&world).await, 0);

    // A protected setting's prompt that answers late proves nobody either.
    let person = world.caller.principal.principal().id;
    let late = world
        .surface
        .confirm_presence(PresenceAsk {
            principal: person,
            action: "config.set".parse().unwrap(),
            effect: EffectClass::Write,
            summary: "Loosen a protected setting.".to_owned(),
        })
        .await
        .unwrap_err();
    assert_eq!(*late.code(), codes::PRESENCE_FAILED);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_prompt_but_a_pages_is_raised_while_a_page_waits_for_its_person() {
    let prompt = TestPrompt::answering(PASSED);
    let world = World::ready(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    let rested = rest(&world).await;

    // The page keeps its origin on a press and gives it to no other, and
    // says which prompt is its own.
    let page = http.get(&rested.link).send().await.unwrap();
    assert_eq!(page.headers()["referrer-policy"], "same-origin");
    let (text, once) = open(&http, &rested.link).await;
    assert!(
        text.contains("A prompt that comes up before you press"),
        "{text}"
    );

    // While the person reads the page, a protected setting asks for the
    // presence prompt: none is raised, so no dialog comes up that the
    // person could take for the page's.
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
    assert_eq!(prompt.asked(), 0);

    // The press raises the page's prompt.
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::OK, "{said}");
    assert_eq!(prompt.asked(), 1);
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
    // Where this build has no prompt for the operating system, the machine's
    // own report is the report of no prompt. Linux, macOS, and Windows each
    // have one, and say for themselves what they can do.
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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

    let person = world.caller.principal.principal().id;
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

    // A protected setting cannot be loosened here: no proof is given.
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
    world.kernel.close(CloseReason::Asked).await;
}

/// Decision 0220. A remedy that holds a path and a command, as the one for
/// a policy file that is not installed does, reaches the approval page and
/// the answer to a press escaped as any text is: a folder's name cannot
/// become markup.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_remedy_that_holds_a_path_and_a_command_reaches_the_page_escaped() {
    let copy = "/home/example/<b>x</b>&y/polkit/example.policy";
    let remedy = format!(
        "A person with administrator rights installs the policy file with this one command: `sudo sh -c 'cat {copy} > \"$f\"'`. The engine never runs it."
    );
    let why = Availability::not_available(
        PresenceReason::PolicyNotInstalled,
        "The policy file is not installed.",
        remedy.clone(),
    );
    let prompt = TestPrompt::answering(PromptAnswer::NotAvailable(why));
    let world = World::ready(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    let rested = rest(&world).await;
    let escaped = "/home/example/&lt;b&gt;x&lt;/b&gt;&amp;y/polkit/example.policy";
    let (text, once) = open(&http, &rested.link).await;
    assert!(text.contains(escaped), "{text}");
    assert!(text.contains("sudo sh -c &#39;cat"), "{text}");
    assert!(text.contains("&quot;$f&quot;"), "{text}");
    assert!(!text.contains("<b>x</b>"), "{text}");
    let (answered, said) = press(&http, &rested, "approve", &once).await;
    assert_eq!(answered, StatusCode::FORBIDDEN);
    assert!(said.contains(escaped), "{said}");
    assert!(!said.contains("<b>x</b>"), "{said}");
    // `doctor` is given the text as it is, to escape at its own door.
    let report = a2m_actions::PersonSurface::presence(&world.surface).await;
    assert_eq!(report.fix, remedy);
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
    // The check is the prompt's own word, and the proof names the prompt
    // (decision 0145).
    assert_eq!(proof.check(), PresenceCheck::Polkit);
    assert_eq!(proof.prompt(), Some("polkit"));
    assert_eq!(proof.principal(), person);
    // What the surface reports to `doctor.run` is the prompt it can raise.
    let report = a2m_actions::PersonSurface::presence(&world.surface).await;
    assert_eq!(report.kind, "polkit");
    assert_eq!(report.reason, "ready");
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

/// A person's act that waits on its page, as its caller sees it.
struct Asked {
    link: String,
    origin: String,
}

fn asked_of(response: &Response) -> Asked {
    assert_eq!(
        response.exit,
        a2m_actions::ExitClass::PersonNeeded,
        "{response:?}"
    );
    let result = response.result.clone().unwrap();
    assert_eq!(result["state"], "needs_person");
    let link = result["link"].as_str().unwrap().to_owned();
    let origin = link[..link.find("/act/").unwrap()].to_owned();
    Asked { link, origin }
}

async fn press_act(http: &Client, asked: &Asked, which: &str, once: &str) -> (StatusCode, String) {
    let answer = http
        .post(format!("{}/{which}", asked.link))
        .header(ORIGIN, &asked.origin)
        .header(CONTENT_TYPE, FORM)
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap();
    (answer.status(), answer.text().await.unwrap())
}

const RELEASE: &str = "killswitch.set";

fn release() -> Json {
    json!({"scope": "everything", "on": false})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_act_an_agent_asks_for_is_carried_out_once_from_its_page() {
    let prompt = TestPrompt::answering(PASSED);
    let world = World::ready(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    let read = || {
        world.call(
            "tool.call",
            json!({"tool": super::world::engine::ORDER_GET, "input": {"order_id": "A-100"}}),
        )
    };
    // An agent may stop everything; only a person releases it.
    assert!(
        world
            .call(RELEASE, json!({"scope": "everything", "on": true}))
            .await
            .ok
    );
    assert!(!read().await.ok);

    // The agent's ask is not refused: it is shown to the person on a page,
    // every value as text, never as markup.
    let marked = world
        .call(
            RELEASE,
            json!({"scope": "everything", "on": false, "target": DESCRIPTION}),
        )
        .await;
    let marked = asked_of(&marked);
    let (text, once) = open(&http, &marked.link).await;
    assert!(!text.contains("<script>alert(1)"), "{text}");
    assert!(
        text.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{text}"
    );
    assert!(text.contains(RELEASE));
    // A press from another origin, or without the page's value, does
    // nothing and raises no prompt.
    let foreign = http
        .post(format!("{}/confirm", marked.link))
        .header(ORIGIN, "http://127.0.0.1:1")
        .header(CONTENT_TYPE, FORM)
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        press_act(&http, &marked, "confirm", "0000").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(prompt.asked(), 0);
    let (_, once) = open(&http, &marked.link).await;
    assert_eq!(
        press_act(&http, &marked, "decline", &once).await.0,
        StatusCode::OK
    );
    assert_eq!(prompt.asked(), 0);
    assert!(!read().await.ok, "a decline does nothing");

    // The ask itself: one press raises one prompt, and the act is carried
    // out as the person, once.
    let asked = asked_of(&world.call(RELEASE, release()).await);
    let (text, once) = open(&http, &asked.link).await;
    assert!(text.contains("everything"), "{text}");
    let (status, done) = press_act(&http, &asked, "confirm", &once).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(done.contains("Done"), "{done}");
    assert_eq!(prompt.asked(), 1);
    assert!(read().await.ok, "the kill switch is released");
    // A second press does nothing: the page is gone and no prompt rises.
    let second = http
        .post(format!("{}/confirm", asked.link))
        .header(ORIGIN, &asked.origin)
        .header(CONTENT_TYPE, FORM)
        .body(format!("once={once}"))
        .send()
        .await;
    // The listener closed with the last page, or it answers that nothing
    // waits there.
    if let Ok(answer) = second {
        assert_eq!(answer.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(prompt.asked(), 1);
    // The agent's same call gives back what was done, and does nothing more.
    let again = world.call(RELEASE, release()).await;
    assert!(again.ok, "{again:?}");
    assert_eq!(again.result.unwrap()["on"], false);
    assert_eq!(prompt.asked(), 1);

    // A prompt that is not passed carries nothing out, and the page waits.
    assert!(
        world
            .call(RELEASE, json!({"scope": "everything", "on": true}))
            .await
            .ok
    );
    prompt.set(PromptAnswer::Denied);
    let asked = asked_of(
        &world
            .call(
                RELEASE,
                json!({"scope": "everything", "on": false, "target": "x"}),
            )
            .await,
    );
    let (_, once) = open(&http, &asked.link).await;
    assert_eq!(
        press_act(&http, &asked, "confirm", &once).await.0,
        StatusCode::FORBIDDEN
    );
    assert!(!read().await.ok);
    assert_eq!(world.surface.waiting(), 1);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_act_nobody_carries_out_ends_with_its_window_and_nothing_is_done() {
    let prompt = TestPrompt::answering(PASSED);
    let world = World::ready(Arc::clone(&prompt) as _, Duration::from_millis(50)).await;
    let http = http();
    assert!(
        world
            .call(RELEASE, json!({"scope": "everything", "on": true}))
            .await
            .ok
    );
    let asked = asked_of(&world.call(RELEASE, release()).await);
    tokio::time::timeout(Duration::from_secs(60), world.surface.settled())
        .await
        .unwrap();
    assert!(http.get(&asked.link).send().await.is_err());
    assert_eq!(prompt.asked(), 0);
    // The caller is told that nobody carried it out.
    let told = world.call(RELEASE, release()).await;
    assert_eq!(told.error.unwrap().code, "approval_refused");
    let held = world
        .call(
            "tool.call",
            json!({"tool": super::world::engine::ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(!held.ok);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_act_whose_input_the_page_cannot_show_whole_is_not_carried_out_from_it() {
    let prompt = TestPrompt::answering(PASSED);
    let world = World::ready(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    assert!(
        world
            .call(RELEASE, json!({"scope": "everything", "on": true}))
            .await
            .ok
    );
    // A value longer than the page shows: what would be carried out is not
    // what the person could read.
    let long = format!("T-{}-end", "x".repeat(600));
    let asked = world
        .call(
            RELEASE,
            json!({"scope": "everything", "on": false, "target": long}),
        )
        .await;
    let asked = asked_of(&asked);
    let (text, once) = open(&http, &asked.link).await;
    assert!(
        text.contains("so this act cannot be carried out here."),
        "{text}"
    );
    assert!(!text.contains("-end"), "{text}");
    // A press raises no prompt and carries nothing out; the act still
    // waits, and a decline works.
    let (status, said) = press_act(&http, &asked, "confirm", &once).await;
    assert_eq!(status, StatusCode::CONFLICT, "{said}");
    assert!(said.contains("not shown whole"), "{said}");
    assert_eq!(prompt.asked(), 0);
    let (_, once) = open(&http, &asked.link).await;
    let (status, said) = press_act(&http, &asked, "decline", &once).await;
    assert_eq!(status, StatusCode::OK, "{said}");
    assert_eq!(prompt.asked(), 0);
    // Everything is still stopped: the release was not carried out.
    let read = world
        .call(
            "tool.call",
            json!({"tool": super::world::engine::ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(!read.ok, "{read:?}");
    world.kernel.close(CloseReason::Asked).await;
}

#[test]
fn a_record_whose_binding_is_chosen_later_shows_every_binding_it_could_take_and_what_each_means() {
    use a2m_actions::{BINDING_LATER, MAY_NOT_BE_READ_BACK, PreviewBinding, PreviewRecord};
    let record = PreviewRecord {
        operation: "example_item_upsert".to_owned(),
        key: "A-100 / L-1".to_owned(),
        action: "create",
        fields: Vec::new(),
        not_read_back: Some(MAY_NOT_BE_READ_BACK),
        binding_later: Some(BINDING_LATER),
        bindings: vec![
            PreviewBinding {
                mode: "app_json".to_owned(),
                evidence: "read_back",
            },
            PreviewBinding {
                mode: "<b>spare</b>".to_owned(),
                evidence: "acknowledged",
            },
            PreviewBinding {
                mode: "app_json_spare".to_owned(),
                evidence: "response",
            },
        ],
        notices: Vec::new(),
    };
    let text = crate::surface::written_by_html(&record);
    assert_eq!(
        text,
        format!(
            "<p><strong>This write {MAY_NOT_BE_READ_BACK}.</strong> A binding that is only acknowledged reads nothing back and compares nothing after it is sent.</p>\
             <p>This record is {BINDING_LATER}. It is written by one of these bindings, and by no other:</p><ul>\
             <li><code>app_json</code>: it is read back and compared with what was intended after it is sent.</li>\
             <li><code>&lt;b&gt;spare&lt;/b&gt;</code>: it is accepted by the system and not read back: nothing compares it after it is sent.</li>\
             <li><code>app_json_spare</code>: the answer the system gives to the write is compared with what was intended; it is not read back apart.</li></ul>"
        )
    );
}

/// The page a person publishes from names each surface's kind and where it
/// leads in every environment, prints each surface notice in its fixed
/// words, and every lint warning with its code; a record's notices stand
/// beside it. Every word is escaped, and a character that reorders text is
/// shown as its escape (AC-1, decisions 0193, 0194, and 0199). The values
/// are the action registry's own types.
#[test]
fn the_publish_page_shows_where_each_surface_leads_its_notices_and_every_warning() {
    use crate::surface::version_shown::{notices_html, version_html};
    use a2m_actions::{ActSurface, ActWarning, PreviewNotice};
    let notice = PreviewNotice {
        code: a2m_core::SurfaceNotice::UnencryptedCredentials.as_str(),
        words: a2m_core::SurfaceNotice::UnencryptedCredentials.words(),
    };
    let surfaces = [
        ActSurface {
            name: "api".to_owned(),
            kind: "http".to_owned(),
            address: "http://203.0.113.7/<b>orders</b>".to_owned(),
            addresses: vec![
                (
                    "test".to_owned(),
                    "http://203.0.113.7/<b>orders</b>".to_owned(),
                ),
                (
                    "live".to_owned(),
                    "https://orders.example.com/\u{202e}gro".to_owned(),
                ),
            ],
            notices: vec![notice],
        },
        ActSurface {
            name: "files".to_owned(),
            kind: "file".to_owned(),
            address: "prices".to_owned(),
            addresses: Vec::new(),
            notices: Vec::new(),
        },
    ];
    let warnings = [ActWarning {
        code: "http.unencrypted_credentials".to_owned(),
        message: "target.yaml: The surface `api` accepts \"credentials\" over <http>.".to_owned(),
    }];
    let page = version_html(&surfaces, &warnings);
    assert!(
        page.contains("<code>api</code>, a surface of the kind <code>http</code>, leads to <code>http://203.0.113.7/&lt;b&gt;orders&lt;/b&gt;</code> in the environment <code>test</code>; <code>https://orders.example.com/\\u{202e}gro</code> in the environment <code>live</code>."),
        "{page}"
    );
    assert!(!page.contains('\u{202e}'), "{page}");
    assert!(
        page.contains("<code>files</code>, a surface of the kind <code>file</code>, leads to <code>prices</code>.</li>"),
        "{page}"
    );
    assert!(
        page.contains(&format!(
            "<strong>{}</strong> (<code>unencrypted_credentials</code>)",
            a2m_core::SurfaceNotice::UnencryptedCredentials.words()
        )),
        "{page}"
    );
    assert_eq!(page.matches("class=\"notice\"").count(), 1, "{page}");
    assert!(
        page.contains("<code>http.unencrypted_credentials</code>: target.yaml: The surface `api` accepts &quot;credentials&quot; over &lt;http&gt;."),
        "{page}"
    );
    assert!(!page.contains("<b>") && !page.contains("<http>"), "{page}");
    // Beside a record: its surfaces' notices, and nothing where none.
    assert_eq!(notices_html(&[]), "");
    assert!(notices_html(&[notice]).contains(notice.words));
    // Nothing to show is nothing at all.
    assert_eq!(version_html(&[], &[]), "");
}

/// A record whose surface carries a notice shows it beside the record,
/// after the words on how it is written and before its fields, escaped;
/// a record whose surfaces carry none shows nothing in its place.
#[test]
fn a_records_notices_stand_beside_it_before_its_fields() {
    use a2m_actions::{ApprovalPreview, PreviewBinding, PreviewNotice, PreviewRecord};
    let notice = PreviewNotice {
        code: a2m_core::SurfaceNotice::UnencryptedCredentials.as_str(),
        words: a2m_core::SurfaceNotice::UnencryptedCredentials.words(),
    };
    let record = |notices: Vec<PreviewNotice>| PreviewRecord {
        operation: "example_item_upsert".to_owned(),
        key: "A-100 / L-1".to_owned(),
        action: "create",
        bindings: vec![PreviewBinding {
            mode: "app_json".to_owned(),
            evidence: "read_back",
        }],
        notices,
        ..PreviewRecord::default()
    };
    let change = crate::surface::Change {
        head: ApprovalPreview {
            run: RunId::from_bytes([7; 16]),
            diff_hash: Digest::of(b"a change"),
            tool: "example_item_upsert".to_owned(),
            target: "example".to_owned(),
            environment: "test".to_owned(),
            create: 2,
            update: 0,
            unchanged: 0,
            write: 0,
            total: 2,
            offset: 0,
            records: Vec::new(),
        },
        records: vec![record(vec![notice]), record(Vec::new())],
        whole: true,
    };
    let page = crate::surface::change_html(&change);
    assert!(
        page.contains(&format!(
            "and by no other: it is read back and compared with what was intended after it is sent.</p><p class=\"notice\"><strong>{}</strong> (<code>unencrypted_credentials</code>)</p><table>",
            escape_like_page(notice.words)
        )),
        "{page}"
    );
    assert_eq!(page.matches("class=\"notice\"").count(), 1, "{page}");
    // No `write` records: no words on them in the counts.
    assert!(
        page.contains("2 to create, 0 to change, 0 already as intended: 2 records in all."),
        "{page}"
    );
}

/// The page's escaping of fixed words, for the assertion above.
fn escape_like_page(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// The one-time value and the address a form of `page` posts to.
fn form_on(page: &str) -> (String, String) {
    let value = |marker: &str| {
        let from = page.find(marker).unwrap() + marker.len();
        page[from..from + page[from..].find('"').unwrap()].to_owned()
    };
    (value("name=\"once\" value=\""), value("action=\""))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sign_in_that_asks_for_a_code_asks_on_the_persons_own_page_and_keeps_it_nowhere() {
    use super::world::RIGHT_CODE;
    let prompt = TestPrompt::answering(PASSED);
    let world = World::signing_in(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    let login = || world.call("session.login", json!({"target": "example"}));

    // Nobody is pressing a sign-in's page: the keeper's request for a code
    // reaches nobody, and says so.
    let person = world.caller.principal.principal().id;
    let unreachable = world
        .surface
        .ask_session(SessionAsk {
            session: a2m_core::SessionId::from_bytes([7; 16]),
            workspace: world.caller.principal.workspace(),
            target: "example".parse().unwrap(),
            environment: "test".parse().unwrap(),
            identity: Identity::Person(person),
            ask: PersonAsk::EnterCode {
                reason: PersonReason::SecondFactor,
            },
        })
        .await;
    assert!(matches!(unreachable, PersonAnswer::Unreachable));

    // The agent asks for the sign-in with no secret; the provider needs a
    // person, so the person is asked on the act's page, which names no
    // stored credential.
    let asked = asked_of(&login().await);
    let (text, once) = open(&http, &asked.link).await;
    assert!(text.contains("<code>session.login</code>"), "{text}");
    assert!(!text.contains("name=\"secret."), "{text}");
    // The press raises the prompt; the provider asks for a code; the
    // press's answer is the page that takes it.
    let (status, page) = press_act(&http, &asked, "confirm", &once).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("The sign-in asks for a code"), "{page}");
    assert_eq!(prompt.asked(), 1);
    let (once, action) = form_on(&page);
    let post = |body: String| {
        http.post(format!("{}{action}", asked.origin))
            .header(ORIGIN, &asked.origin)
            .header(CONTENT_TYPE, FORM)
            .body(body)
            .send()
    };
    // A post without the page's value hands nothing on, and ends the
    // sign-in: nothing is signed in, and the page is gone.
    let refused = post(format!("once=0000&code={RIGHT_CODE}")).await.unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    world.surface.settled().await;
    let gone = post(format!("once={once}&code={RIGHT_CODE}")).await;
    assert!(gone.is_err() || gone.unwrap().status() == StatusCode::NOT_FOUND);
    // Asked for again, the sign-in is a new act on a new page, and this
    // time the code goes with the page's value.
    let asked = asked_of(&login().await);
    let (_, once) = open(&http, &asked.link).await;
    let (_, page) = press_act(&http, &asked, "confirm", &once).await;
    let (once, action) = form_on(&page);
    let done = http
        .post(format!("{}{action}", asked.origin))
        .header(ORIGIN, &asked.origin)
        .header(CONTENT_TYPE, FORM)
        .body(format!("once={once}&code={RIGHT_CODE}"))
        .send()
        .await
        .unwrap();
    assert_eq!(done.status(), StatusCode::OK);
    let said = done.text().await.unwrap();
    assert!(said.contains("was carried out as you, once"), "{said}");
    assert!(!said.contains(RIGHT_CODE), "{said}");

    // The agent's same call is told a session holds, at the level the
    // provider confirms; the code is in nothing it is told.
    let signed = login().await;
    assert!(signed.ok, "{signed:?}");
    let shown = signed.to_json().to_string();
    assert!(!shown.contains(RIGHT_CODE), "{shown}");
    let result = signed.result.unwrap();
    assert_eq!(result["state"], "signed_in", "{result}");
    assert_eq!(result["level"], "confirmed", "{result}");
    let sessions = world.call("session.status", json!({})).await;
    let shown = sessions.to_json().to_string();
    assert!(shown.contains("\"valid\""), "{shown}");
    assert!(!shown.contains(RIGHT_CODE), "{shown}");
    world.kernel.close(CloseReason::Asked).await;
}

/// A provider that signs in with two stored credentials, a username and a
/// password, has its page ask for each role, each in a field of its own
/// that is served empty; what the person types is never shown back, on
/// the page, in its answer, or to the agent. A press that leaves one out
/// stops by name, and the page asked next says the other is stored and
/// still shows no value of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sign_in_page_asks_for_each_role_the_provider_needs_and_shows_none_back() {
    use super::world::{PASSWORD_REF, PASSWORDS_SEEN, USER_REF};
    const TYPED_USER: &str = "invented-person-7c1d";
    const TYPED_PASSWORD: &str = "invented-password-4e9b2a";
    let prompt = TestPrompt::answering(PASSED);
    let world = World::signing_in_by_password(Arc::clone(&prompt) as _, LONG).await;
    let http = http();
    let login = || world.call("session.login", json!({"target": "example"}));
    let press_with = |asked: &Asked, once: &str, typed: &str| {
        http.post(format!("{}/confirm", asked.link))
            .header(ORIGIN, &asked.origin)
            .header(CONTENT_TYPE, FORM)
            .body(format!("once={once}{typed}"))
            .send()
    };
    let field = |role: &str, required: bool| {
        format!(
            "name=\"secret.{role}\" type=\"password\" autocomplete=\"off\"{}>",
            if required { " required" } else { "" }
        )
    };

    // The agent asks with no secret. Nothing is stored, so the person is
    // asked on the act's page for both roles the entry names, each with
    // the reference it is sealed under.
    let asked = asked_of(&login().await);
    let (text, once) = open(&http, &asked.link).await;
    for (role, reference) in [("user", USER_REF), ("password", PASSWORD_REF)] {
        assert!(text.contains(&field(role, true)), "{text}");
        assert!(text.contains(reference), "{text}");
    }
    assert!(!text.contains("value=\"invented"), "{text}");

    // The person types the username alone. The sign-in stops by name for
    // want of the password; the answer shows neither.
    let answered = press_with(&asked, &once, &format!("&secret.user={TYPED_USER}"))
        .await
        .unwrap();
    let said = answered.text().await.unwrap();
    assert!(said.contains("did not succeed"), "{said}");
    assert!(said.contains(PASSWORD_REF), "{said}");
    assert!(!said.contains(TYPED_USER), "{said}");
    world.surface.settled().await;
    assert!(PASSWORDS_SEEN.lock().unwrap().is_empty());

    // Asked again, the page says the username is stored, asks for it no
    // more, and still shows no value of it; the password is asked for.
    let asked = asked_of(&login().await);
    let (text, once) = open(&http, &asked.link).await;
    assert!(text.contains(&field("user", false)), "{text}");
    assert!(text.contains(&field("password", true)), "{text}");
    assert!(text.contains("Stored already"), "{text}");
    assert!(!text.contains(TYPED_USER), "{text}");
    let answered = press_with(&asked, &once, &format!("&secret.password={TYPED_PASSWORD}"))
        .await
        .unwrap();
    assert_eq!(answered.status(), StatusCode::OK);
    let said = answered.text().await.unwrap();
    assert!(said.contains("was carried out as you, once"), "{said}");
    assert!(
        !said.contains(TYPED_USER) && !said.contains(TYPED_PASSWORD),
        "{said}"
    );
    assert_eq!(prompt.asked(), 2);

    // Both reached the provider, each under its own role.
    assert_eq!(
        *PASSWORDS_SEEN.lock().unwrap(),
        [(TYPED_USER.to_owned(), TYPED_PASSWORD.to_owned())]
    );
    // The agent is told a session holds, and nothing typed.
    let signed = login().await;
    assert!(signed.ok, "{signed:?}");
    let shown = signed.to_json().to_string();
    assert!(shown.contains("\"signed_in\""), "{shown}");
    assert!(
        !shown.contains(TYPED_USER) && !shown.contains(TYPED_PASSWORD),
        "{shown}"
    );
    world.kernel.close(CloseReason::Asked).await;
}

/// The page a person publishes from shows the sign-ins before the tools,
/// a sign-in whose read is not drafted as not confirmed, where each sign-in
/// sends in each environment, and for each tool where it sends, what it
/// starts, and what it sets. Every text is escaped, a character that
/// reorders text is shown as its escape, and a secret variable is named and
/// never valued (decisions 0252 and 0264).
#[test]
fn the_act_page_shows_sign_ins_before_tools_and_escapes_and_hides_every_secret_value() {
    use crate::surface::review_shown::review_html;
    use a2m_files::{
        BindingReview, BindingStates, CredentialShown, EnvironmentVariable, PROBE_PENDING,
        ProbeReview, ProgramRun, REACH_NOT_STATED, Reach, ReachGroup, ReviewItem, ReviewSummary,
        RunReview, SCRIPT_NOTICE, SentAddress, SentGroup, SentTo, SentToReview, SignInReview,
        VariableValue,
    };
    let surface = || a2m_core::SurfaceName::new("api").unwrap();
    let sign_in = |entry: usize, probe: ProbeReview| SignInReview {
        entry,
        surfaces: vec![surface()],
        provider: "bearer_static".parse().unwrap(),
        mode: a2m_core::SignInMode::Custom,
        credentials: vec![CredentialShown {
            role: "token".to_owned(),
            reference: "vault://example/api/token".parse().unwrap(),
        }],
        probe,
        sent_to: vec![SentToReview {
            environment: "test".parse().unwrap(),
            sent_to: SentTo::Addresses {
                groups: vec![SentGroup {
                    scope: a2m_core::AddressScope::Name,
                    addresses: vec![SentAddress {
                        address: "https://api.example.com/<i>v1</i>".to_owned(),
                        what: "the surface `api`".to_owned(),
                    }],
                }],
                leaves_this_machine: true,
            },
        }],
    };
    let review = ReviewSummary {
        items: vec![ReviewItem {
            tool: a2m_core::OperationName::new("example_report_run").unwrap(),
            effect: EffectClass::Write,
            hazard: a2m_core::Hazard::None,
            commit_points: 1,
            surfaces: vec![surface()],
            destinations: Vec::new(),
            uploads: Vec::new(),
            downloads: Vec::new(),
            place: None,
            bindings: vec![BindingReview {
                operation: a2m_core::OperationName::new("example_report_run").unwrap(),
                mode: a2m_core::Mode::new("cli").unwrap(),
                surface: surface(),
                surface_programs: Vec::new(),
                states: BindingStates::Stated {
                    runs: vec![RunReview {
                        run: ProgramRun::Script {
                            interpreter: "/usr/bin/example-interpreter".to_owned(),
                            interpreter_digests: vec![Digest::of(b"interpreter")],
                            arguments: vec!["-I".to_owned()],
                            script: "scripts/<run>.example\u{202e}".to_owned(),
                            script_digest: Digest::of(b"script"),
                        },
                        notice: Some(SCRIPT_NOTICE),
                    }],
                    environment: vec![
                        EnvironmentVariable {
                            name: "EXAMPLE_MODE".to_owned(),
                            value: VariableValue::Set("a&b".to_owned()),
                        },
                        EnvironmentVariable {
                            name: "EXAMPLE_KEY".to_owned(),
                            value: VariableValue::Secret,
                        },
                    ],
                },
            }],
            reach: Reach::Partly {
                known: vec![ReachGroup {
                    scope: a2m_core::AddressScope::ThisMachine,
                    addresses: vec!["http://127.0.0.1:8080".to_owned()],
                }],
                says: REACH_NOT_STATED,
            },
        }],
        sign_ins: vec![
            sign_in(
                1,
                ProbeReview::Pending {
                    name: "example_session_check".to_owned(),
                    says: PROBE_PENDING,
                },
            ),
            sign_in(
                0,
                ProbeReview::Declared {
                    name: "example_report_get".to_owned(),
                },
            ),
        ],
    };
    let page = review_html(&review);
    let at = |text: &str| page.find(text).unwrap_or_else(|| panic!("{text}: {page}"));
    // Sign-ins first, the one not confirmed first among them; then tools.
    assert!(
        at("How it signs in") < at("What each tool may do"),
        "{page}"
    );
    assert!(at("Not confirmed") < at("example_report_get"), "{page}");
    assert!(at("Not confirmed") < at("example_report_run"), "{page}");
    assert!(page.contains(&escape_like_page(PROBE_PENDING)), "{page}");
    // Where the credential goes, off this machine, in bold, escaped.
    assert!(
        page.contains(
            "<code>https://api.example.com/&lt;i&gt;v1&lt;/i&gt;</code>, the surface `api`"
        ),
        "{page}"
    );
    assert!(
        page.contains("<strong>it may leave this machine</strong>"),
        "{page}"
    );
    assert!(
        page.contains("<code>vault://example/api/token</code>"),
        "{page}"
    );
    // The tool: where it sends first, what it starts, what it sets.
    assert!(
        at(&escape_like_page(REACH_NOT_STATED)) < at("Starts the interpreter"),
        "{page}"
    );
    assert!(
        page.contains("<code>scripts/&lt;run&gt;.example\\u{202e}</code>"),
        "{page}"
    );
    assert!(page.contains(&escape_like_page(SCRIPT_NOTICE)), "{page}");
    assert!(page.contains(&Digest::of(b"script").to_string()), "{page}");
    assert!(
        page.contains("<code>EXAMPLE_MODE</code>: set to <code>a&amp;b</code>"),
        "{page}"
    );
    assert!(
        page.contains("<code>EXAMPLE_KEY</code>: secret, not shown"),
        "{page}"
    );
    // Nothing a target wrote is markup on the page.
    for raw in ["<i>", "<run>", "a&b", "\u{202e}"] {
        assert!(!page.contains(raw), "{raw}: {page}");
    }
}
