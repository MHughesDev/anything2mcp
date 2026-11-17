//! The driver against a real browser and the test application, reached
//! only through the driver handle. Every write is checked by asking the
//! application's control interface what it stored.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{Decimal, FailureClass, PackPayload, Plan, Value, Values};
use a2m_expr::Normalizer;
use a2m_pack_web_browser::MASK;
use a2m_provider::{
    Authority, Deadline, Destinations, DriverHandle, Posture, Prepared, Sent, StepFailure,
    StepInput,
};
use common::{Scene, Write, compile_changed};
use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;
use example_app::store::format_length_in;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const GET: &str = "example_line_item_get";
const UPDATE: &str = "example_line_item_update";

async fn scene(name: &str) -> (TestApp, Scene) {
    let app = TestApp::start().await.unwrap();
    let scene = Scene::new(name, &app.url(""));
    (app, scene)
}

fn item(order: &str, code: &str) -> Values {
    Values::new()
        .with("order_id", order)
        .with("item_code", code)
}

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

/// The plan with one file of the fixture changed.
fn changed(scene: &Scene, path: &str, change: impl Fn(&str) -> String) -> Plan {
    compile_changed(&scene.stand.registry, "example", path, change)
        .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)))
        .plan
}

/// Reads one line item with a plan that differs from the fixture's.
async fn read_with(scene: &mut Scene, plan: Plan) -> Result<common::Ran, StepFailure> {
    let kept = std::mem::replace(&mut scene.plan, plan);
    let (handle, mut prepared) = scene.reader().await;
    let read = scene
        .read(&handle, &mut prepared, GET, &item("A-100", "L-101"))
        .await;
    handle.release(prepared, read.is_err()).await;
    scene.plan = kept;
    read
}

#[tokio::test(flavor = "multi_thread")]
async fn one_read_gives_what_the_form_shows_and_five_runs_do_the_same_thing() {
    let (app, scene) = scene("read").await;
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    let mut runs = Vec::new();
    for _ in 0..5 {
        let (handle, mut prepared) = scene.reader().await;
        let read = scene
            .read(&handle, &mut prepared, GET, &item("A-100", "L-101"))
            .await
            .unwrap();
        assert!(!read.absent);
        // Raw text, as the page shows it: the engine normalizes it.
        assert_eq!(
            read.values.get("description"),
            Some(&Value::from(stored.description.as_str()))
        );
        assert_eq!(
            read.values.get("quantity"),
            Some(&Value::from(stored.quantity.to_string()))
        );
        assert_eq!(
            read.values.get("length_in"),
            Some(&Value::from(format_length_in(stored.length_sixteenths)))
        );
        runs.push((read.means(), read.values));
        assert!(handle.release(prepared, false).await.complete);
    }
    assert!(runs.windows(2).all(|pair| pair[0] == pair[1]), "{runs:?}");
    // What was done names the template and which locator found each
    // field, never the value that filled a placeholder or a field.
    let means = format!("{:?}", runs[0].0);
    assert!(
        means.contains("/orders/{order_id}/items/{item_code}"),
        "{means}"
    );
    assert!(means.contains("locator.description"), "{means}");
    assert!(
        !means.contains("A-100") && !means.contains(&stored.description),
        "{means}"
    );
    // A read wrote nothing.
    assert!(app.writes().await.unwrap().is_empty());
    scene.stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rows_are_read_by_their_headings_and_a_record_that_is_not_there_is_absent() {
    let (app, scene) = scene("rows").await;
    let (handle, mut prepared) = scene.reader().await;
    let order = Values::new().with("order_id", "A-100");
    let listed = scene
        .read(&handle, &mut prepared, "example_order_line_list", &order)
        .await
        .unwrap();
    let stored = app.stored_order("A-100").await.unwrap().unwrap();
    let codes: Vec<Value> = stored
        .items
        .values()
        .map(|item| Value::from(item.item_code.as_str()))
        .collect();
    let quantities: Vec<Value> = stored
        .items
        .values()
        .map(|item| Value::from(item.quantity.to_string()))
        .collect();
    assert_eq!(codes.len(), 3);
    assert_eq!(listed.values.get("item_codes"), Some(&Value::List(codes)));
    assert_eq!(
        listed.values.get("quantities"),
        Some(&Value::List(quantities))
    );
    // An order with no line items is an empty list, not a failure.
    let empty = scene
        .read(
            &handle,
            &mut prepared,
            "example_order_line_list",
            &Values::new().with("order_id", "A-102"),
        )
        .await
        .unwrap();
    assert_eq!(
        empty.values.get("item_codes"),
        Some(&Value::List(Vec::new()))
    );

    // A line item that does not exist: the screen declares how that looks.
    let missing = scene
        .read(&handle, &mut prepared, GET, &item("A-100", "L-999"))
        .await
        .unwrap();
    assert!(missing.absent && missing.values.is_empty());
    // A value cannot leave its segment of the address: it is one segment,
    // whatever it holds, and the application knows no such order.
    let escaping = scene
        .read(
            &handle,
            &mut prepared,
            GET,
            &item("../../__control/reset?x=", "L-101"),
        )
        .await
        .unwrap();
    assert!(escaping.absent);
    assert!(handle.release(prepared, false).await.complete);
    assert!(app.stored_item("A-100", "L-101").await.unwrap().is_some());
    scene.stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_screen_is_drift_and_what_has_no_rule_stops() {
    let (_app, mut scene) = scene("drift").await;
    let form = "screens/line_item.yaml";

    // The field is no longer where any of its locators says.
    let plan = changed(&scene, form, |text| {
        text.replace("name: Description}", "name: Summary}")
            .replace("{label: Description}", "{label: Summary}")
    });
    let gone = read_with(&mut scene, plan).await.unwrap_err();
    assert_eq!(
        class_and_code(&gone),
        (FailureClass::Drift, "locator_not_found")
    );
    assert!(gone.failure.message().contains("`description`"), "{gone}");

    // A locator that matches several elements never picks the first.
    let plan = changed(&scene, form, |text| {
        text.replace("- {role: textbox, name: Description}\n", "")
            .replace("- {label: Description}", "- {css: 'input'}")
    });
    let several = read_with(&mut scene, plan).await.unwrap_err();
    assert_eq!(
        class_and_code(&several),
        (FailureClass::Drift, "locator_ambiguous")
    );

    // The element is found, and is not where its fingerprint says.
    for (from, to) in [
        (
            "{role: group, name: Dimensions}",
            "{role: group, name: Sizes}",
        ),
        ("near_text: inches", "near_text: feet"),
    ] {
        let plan = changed(&scene, form, |text| text.replace(from, to));
        let moved = read_with(&mut scene, plan).await.unwrap_err();
        assert_eq!(
            class_and_code(&moved),
            (FailureClass::Drift, "fingerprint_mismatch"),
            "{to}"
        );
    }

    // A plan that names a control this driver has no rule for: compiled by
    // a pack that knew more. It is a stop, never a guess at a text box.
    let mut plan = scene.plan.clone();
    let section = plan
        .packs
        .iter_mut()
        .find(|section| section.pack == "web")
        .unwrap();
    let text = String::from_utf8(section.payload.as_bytes().to_vec()).unwrap();
    let other: serde_json::Value =
        serde_json::from_str(&text.replace(r#""control":"number""#, r#""control":"slider""#))
            .unwrap();
    section.payload = PackPayload::encode(&other).unwrap();
    let unknown = read_with(&mut scene, plan).await.unwrap_err();
    assert_eq!(
        class_and_code(&unknown),
        (FailureClass::UnknownVariant, "unknown_control")
    );

    // A page that is neither ready nor in a state the screen declares.
    let mut plan = changed(&scene, form, |text| {
        text.replace("name: \"Line Item Details\"}}", "name: \"Line Item\"}}")
    });
    for operation in &mut plan.operations {
        for binding in &mut operation.bindings {
            for step in &mut binding.act {
                step.timeout_ms = 1500;
            }
        }
    }
    let strange = read_with(&mut scene, plan).await.unwrap_err();
    assert_eq!(
        class_and_code(&strange),
        (FailureClass::UnknownVariant, "unexpected_page")
    );
    assert_eq!(strange.sent, Sent::NotSent);
    scene.stand.end().await;
}

/// The field type's own rule for an output, as the engine loads it.
fn normalizer(plan: &Plan, operation: &str, output: &str) -> Normalizer {
    let operation = plan
        .operations
        .iter()
        .find(|known| known.name.as_str() == operation)
        .unwrap();
    let output = operation
        .outputs
        .iter()
        .find(|known| known.name == output)
        .unwrap();
    let field_type = &plan.field_types[output.field_type.unwrap().get()];
    Normalizer::load(plan.expression(field_type.normalizer.unwrap()).unwrap()).unwrap()
}

fn update(description: &str, quantity: i64, length: &str) -> Values {
    item("A-100", "L-101")
        .with("description", description)
        .with("quantity", quantity)
        .with("length_in", Decimal::parse(length).unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn one_write_is_stored_once_and_reads_back_as_the_field_type_compares_it() {
    let (app, scene) = scene("write").await;
    let binding = scene.binding(UPDATE);
    let inputs = update("Oak rail, sanded", 7, "12.50");

    // Without the ledger's token the committing step is refused before
    // the driver sees it: the form is filled, and nothing is saved.
    let (handle, mut prepared) = scene.reader().await;
    let refused = scene
        .run(&handle, &mut prepared, &binding.act, &inputs, None)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Policy, "intent_missing")
    );
    assert_eq!(refused.sent, Sent::NotSent);
    handle.release(prepared, true).await;
    assert!(app.writes().await.unwrap().is_empty());

    // With it: the page is filled, saved, and shows its confirmation.
    let write = Write::new(&["A-100", "L-101"]).await;
    let (handle, mut prepared) = scene
        .try_prepare(Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let acted = scene
        .run(
            &handle,
            &mut prepared,
            &binding.act,
            &inputs,
            Some(write.authority()),
        )
        .await
        .unwrap();
    let means = format!("{:?}", acted.means());
    assert!(
        means.contains("strategy") && means.contains("fill"),
        "{means}"
    );
    assert!(!means.contains("Oak rail"), "{means}");
    // Evidence at the two checkpoints: before acting, and at the
    // confirmation.
    let snapshots = acted.captures("snapshot");
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots[1].contains("Saved."), "{}", snapshots[1]);

    // The application says what it stored, and how many times.
    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 1);
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!(stored.description, "Oak rail, sanded");
    assert_eq!(stored.quantity, 7);
    assert_eq!(stored.length_sixteenths, 200);

    // The read-back reaches the screen afresh and reads the record.
    let back = scene
        .run(&handle, &mut prepared, &binding.read_back, &inputs, None)
        .await
        .unwrap();
    assert!(!back.absent);
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(
        back.values.get("description"),
        Some(&Value::from("Oak rail, sanded"))
    );
    // The application shows `12.5` for the `12.50` that was sent. Under
    // the field type's rule the two are one value, and another is not.
    let shown = back.values.get("length_in").unwrap();
    assert_eq!(shown, &Value::from("12.5"));
    let rule = normalizer(&scene.plan, UPDATE, "length_in");
    let intended = rule.normalize(inputs.get("length_in").unwrap()).unwrap();
    assert!(rule.normalize(shown).unwrap().agrees_with(&intended));
    let other = rule
        .normalize(&Value::from(Decimal::parse("12.5625").unwrap()))
        .unwrap();
    assert!(!rule.normalize(shown).unwrap().agrees_with(&other));
    let quantity = normalizer(&scene.plan, UPDATE, "quantity");
    assert!(
        quantity
            .normalize(back.values.get("quantity").unwrap())
            .unwrap()
            .agrees_with(&quantity.normalize(&Value::from(7_i64)).unwrap())
    );
    scene.stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_the_application_refuses_is_rejected_and_stores_nothing() {
    let (app, scene) = scene("refused").await;
    let binding = scene.binding(UPDATE);
    let write = Write::new(&["A-100", "L-101"]).await;
    let (handle, mut prepared) = scene
        .try_prepare(Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let refused = scene
        .run(
            &handle,
            &mut prepared,
            &binding.act,
            &update("Oak rail", 0, "12.5"),
            Some(write.authority()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&refused),
        (FailureClass::Rejected, "app_validation_error")
    );
    // The application answered, so nothing is in doubt.
    assert_eq!(refused.sent, Sent::Answered);
    handle.release(prepared, true).await;
    assert!(app.writes().await.unwrap().is_empty());
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!(stored.quantity, 2);

    // A run that may only look does not touch a control.
    let (handle, mut prepared) = scene
        .try_prepare(Some(write.fence), Posture::Discovery)
        .await
        .unwrap();
    let looked = scene
        .run(
            &handle,
            &mut prepared,
            &binding.act,
            &update("Oak rail", 3, "12.5"),
            Some(write.authority()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&looked),
        (FailureClass::Policy, "read_only_posture")
    );
    handle.release(prepared, true).await;

    // A surface the run may not reach is not opened.
    let mut elsewhere = Scene::over(scene.stand, scene.plan, &app.url(""));
    elsewhere.destinations = Destinations::new(["http://127.0.0.1:9".to_owned()]);
    let outside = elsewhere
        .try_prepare(None, Posture::Live)
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&outside),
        (FailureClass::Policy, "surface_not_declared")
    );
    assert!(app.writes().await.unwrap().is_empty());
    elsewhere.stand.end().await;
}

/// Carries out the last step of a write, the committing one, on its own.
async fn commit(
    scene: &Scene,
    handle: &DriverHandle,
    prepared: &mut Prepared,
    inputs: &Values,
    write: &Write,
) -> Result<a2m_provider::StepOutcome, StepFailure> {
    let step = scene.binding(UPDATE).act.last().unwrap();
    assert!(step.commits);
    let args = scene.args(step, inputs);
    let (token, record, key) = write.authority();
    let input = StepInput {
        args: &args,
        record: Some(record),
        key: Some(key),
        stamp: None,
        deadline: Deadline::after(Duration::from_secs(120)),
    };
    handle
        .act(prepared, step, &input, Authority::Write(token))
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_browser_lost_after_the_committing_click_is_possible_commit_and_never_a_retry() {
    let (app, scene) = scene("lost").await;
    let binding = scene.binding(UPDATE);
    let inputs = update("Sent before the crash", 5, "3");
    let write = Write::new(&["A-100", "L-101"]).await;
    let (handle, mut prepared) = scene
        .try_prepare(Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let before = &binding.act[..binding.act.len() - 1];
    scene
        .run(&handle, &mut prepared, before, &inputs, None)
        .await
        .unwrap();

    // The application will hold the save for a long time once it arrives.
    app.set_fault(
        &Fault::once(FaultKind::Delay { ms: 600_000 }).on_paths("/api/orders/A-100/items/L-101"),
    )
    .await
    .unwrap();
    let kill = async {
        // The fault is used up when the request arrives: the stand, not a
        // guess at timing, says the save has left the page.
        while !app.faults().await.unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let running = scene.stand.supervisor.list();
        assert_eq!(running.len(), 1);
        common::kill(running[0].process);
    };
    let (lost, ()) = tokio::join!(
        commit(&scene, &handle, &mut prepared, &inputs, &write),
        async {
            tokio::time::timeout(Duration::from_secs(60), kill)
                .await
                .unwrap();
        }
    );
    let lost = lost.unwrap_err();
    assert_eq!(
        class_and_code(&lost),
        (FailureClass::PossibleCommit, "no_confirmation")
    );
    assert_eq!(lost.sent, Sent::Unknown);
    assert!(handle.release(prepared, true).await.complete);

    // Nothing was sent a second time: the application counts no write
    // beyond the one that was held, which died with the browser.
    assert!(app.write_count("A-100", "L-101").await.unwrap() <= 1);
    app.clear_faults().await.unwrap();

    // The next run gets a fresh browser, and reads what is there.
    let (handle, mut prepared) = scene.reader().await;
    let read = scene
        .read(&handle, &mut prepared, GET, &item("A-100", "L-101"))
        .await
        .unwrap();
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!(
        read.values.get("description"),
        Some(&Value::from(stored.description.as_str()))
    );
    assert!(handle.release(prepared, false).await.complete);
    scene.stand.end().await;
}

/// A made-up password, planted in the page a test serves.
const PLANTED: &str = "secret-for-tests-4711";

/// One page with a password, a check box, and a select, which the test
/// application does not have. It answers every request with the same
/// page, on a port of its own, until the test ends.
async fn sign_in_page() -> (String, tokio::task::JoinHandle<()>) {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Sign in</title></head>\
         <body><main><h1>Sign in</h1><form id=\"f\">\
         <p><label for=\"a\">Account</label> <input id=\"a\" name=\"account\"></p>\
         <p><label for=\"p\">Password</label> <input id=\"p\" type=\"password\" value=\"{PLANTED}\"></p>\
         <p><label><input type=\"checkbox\" id=\"r\"> Remember this account</label></p>\
         <p><label for=\"g\">Region</label> <select id=\"g\">\
         <option value=\"south\">South</option><option value=\"north\">North</option></select></p>\
         <p><button type=\"submit\">Enter</button></p><p id=\"status\" role=\"status\"></p></form>\
         <script>const f = document.getElementById(\"f\");\
         f.addEventListener(\"submit\", (event) => {{ event.preventDefault();\
         document.getElementById(\"status\").textContent = \"Entered as \" + f.elements.account.value\
         + \" \" + document.getElementById(\"r\").checked + \" \" + document.getElementById(\"g\").value; }});\
         </script></main></body></html>"
    );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let page = page.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut chunk = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut chunk).await {
                        Ok(read) if read > 0 => request.extend_from_slice(&chunk[..read]),
                        _ => return,
                    }
                }
                let answer = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                );
                let _ = stream.write_all(answer.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    (address, server)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_password_on_the_page_is_never_read_traced_or_logged() {
    let (address, server) = sign_in_page().await;
    let scene = Scene::new("password", &address);

    // A read of the password is refused, whatever the target's files ask.
    let (handle, mut prepared) = scene.reader().await;
    let asked = scene
        .read(
            &handle,
            &mut prepared,
            "example_account_password_get",
            &Values::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        class_and_code(&asked),
        (FailureClass::Policy, "password_read_refused")
    );
    assert!(!format!("{asked:?}").contains(PLANTED));
    handle.release(prepared, true).await;

    // The other controls of the page are set, each by its own rule, and
    // the action is confirmed, with the password on the page all along.
    let binding = scene.binding("example_account_enter");
    let inputs = Values::new()
        .with("account", "clerk")
        .with("remember", true)
        .with("region", "north");
    let write = Write::new(&["clerk"]).await;
    let (handle, mut prepared) = scene
        .try_prepare(Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let acted = scene
        .run(
            &handle,
            &mut prepared,
            &binding.act,
            &inputs,
            Some(write.authority()),
        )
        .await
        .unwrap();
    assert!(handle.release(prepared, false).await.complete);
    // The page's own script says what its controls held when it was
    // entered, and the browser's accessibility tree carries that: the
    // check box and the select were set, by another path than the
    // driver's own read.
    let snapshots = acted.captures("snapshot");
    assert_eq!(snapshots.len(), 2);
    assert!(
        snapshots[1].contains("Entered as clerk true north"),
        "{}",
        snapshots[1]
    );
    let strategies = format!("{:?}", acted.means());
    assert!(
        strategies.contains("\"check\"") && strategies.contains("\"select\""),
        "{strategies}"
    );
    // The password is masked in the evidence, and is in none of it.
    assert!(snapshots.iter().all(|snapshot| snapshot.contains(MASK)));
    for capture in &acted.captures {
        let text = String::from_utf8_lossy(&capture.bytes);
        assert!(!text.contains(PLANTED), "{}: {text}", capture.name);
    }
    // Nor in anything the engine logged.
    let logs = scene.stand.folder.path().join("logs");
    for entry in std::fs::read_dir(&logs).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap_or_default();
        assert!(!String::from_utf8_lossy(&bytes).contains(PLANTED));
    }
    scene.stand.end().await;
    server.abort();
}
