//! The test application's later cases: signing in (13 to 15), tokens and keys (29),
//! the served description (30), the faults "lost after commit" (17) and "changed
//! shape" (31), the browser's own controls (33), pages that write by themselves
//! (32), and answers from the edge (23, 34). Every write is proven by asking the
//! control interface.

use std::error::Error;

use example_app::auth::{AuthSettings, Credential};
use example_app::details::{Priority, Shipping};
use example_app::faults::{Fault, FaultKind};
use example_app::harness::{
    API_KEY, API_KEY_HEADER, BEARER_TOKEN, LOCKOUT_AFTER, PASSWORD, SESSION_COOKIE, TestApp,
    USERNAME,
};
use reqwest::{Client, StatusCode, header, redirect};
use serde_json::{Value, json};

type Outcome = Result<(), Box<dyn Error>>;

const UPDATE: &str =
    r#"{"description": "Left rail", "quantity": 3, "length_in": "150.5", "version": 1}"#;

fn no_redirects() -> Result<Client, reqwest::Error> {
    example_app::harness::client_builder()
        .redirect(redirect::Policy::none())
        .build()
}

fn location(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

async fn sign_in(
    app: &TestApp,
    client: &Client,
    password: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    client
        .post(app.url("/secure/sign-in"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!(
            "username={USERNAME}&password={password}&next=%2Fsecure%2Forders"
        ))
        .send()
        .await
}

fn update() -> Result<Value, serde_json::Error> {
    serde_json::from_str(UPDATE)
}

/// Case 13: the form, the cookie, the guarded page and interface, and signing out.
#[tokio::test]
async fn case_13_a_session_from_the_sign_in_form_opens_the_signed_in_part() -> Outcome {
    let app = TestApp::start().await?;
    let client = no_redirects()?;

    let form = client
        .get(app.url("/secure/sign-in"))
        .send()
        .await?
        .text()
        .await?;
    for control in [
        "name=\"username\"",
        "name=\"password\" type=\"password\"",
        "<button type=\"submit\">Sign in</button>",
    ] {
        assert!(form.contains(control), "{control}");
    }

    let page = client.get(app.url("/secure/orders")).send().await?;
    assert_eq!(page.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&page), "/secure/sign-in?next=/secure/orders");
    let api = client
        .put(app.url("/secure/api/orders/A-100/items/L-101"))
        .json(&update()?)
        .send()
        .await?;
    assert_eq!(api.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(api.json::<Value>().await?["error"], "needs_login");
    assert_eq!(app.write_count("A-100", "L-101").await?, 0);

    let refused = sign_in(&app, &client, "secret-for-tests-wrong").await?;
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    assert!(refused.headers().get(header::SET_COOKIE).is_none());

    let signed_in = sign_in(&app, &client, PASSWORD).await?;
    assert_eq!(signed_in.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&signed_in), "/secure/orders");
    let cookie = signed_in
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .ok_or("no cookie")?;
    assert!(cookie.contains("HttpOnly") && cookie.contains("Path=/secure"));
    let cookie = cookie.split(';').next().ok_or("empty cookie")?.to_owned();
    assert!(cookie.starts_with(SESSION_COOKIE));

    let page = client
        .get(app.url("/secure/orders"))
        .header(header::COOKIE, &cookie)
        .send()
        .await?;
    assert_eq!(page.status(), StatusCode::OK);
    assert!(page.text().await?.contains("Signed in as example-user."));
    let api = client
        .put(app.url("/secure/api/orders/A-100/items/L-101"))
        .header(header::COOKIE, &cookie)
        .json(&update()?)
        .send()
        .await?;
    assert_eq!(api.status(), StatusCode::OK);
    let stored = app.stored_item("A-100", "L-101").await?.ok_or("missing")?;
    assert_eq!((stored.quantity, stored.version), (3, 2));

    let out = client
        .post(app.url("/secure/sign-out"))
        .header(header::COOKIE, &cookie)
        .send()
        .await?;
    assert_eq!(location(&out), "/secure/sign-in");
    let after = client
        .get(app.url("/secure/api/orders"))
        .header(header::COOKIE, &cookie)
        .send()
        .await?;
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);

    let report = app.auth_report().await?;
    assert_eq!(
        (
            report.sign_in_attempts,
            report.failed_sign_ins,
            report.sign_ins,
            report.sign_outs,
            report.active_sessions,
            report.refused_sessions,
        ),
        (2, 1, 1, 1, 0, 3)
    );
    assert!(!report.locked);
    Ok(())
}

/// Case 13: the lockout counter shows a refused credential was not tried again.
#[tokio::test]
async fn case_13_failed_sign_ins_lock_the_account_and_are_counted() -> Outcome {
    let app = TestApp::start().await?;
    let client = no_redirects()?;
    for _ in 0..LOCKOUT_AFTER {
        sign_in(&app, &client, "secret-for-tests-wrong").await?;
    }
    let report = app.auth_report().await?;
    assert_eq!(
        (report.failed_sign_ins, report.locked),
        (LOCKOUT_AFTER, true)
    );

    let locked = sign_in(&app, &client, PASSWORD).await?;
    assert_eq!(locked.status(), StatusCode::LOCKED);
    let report = app.auth_report().await?;
    assert_eq!(report.sign_in_attempts, LOCKOUT_AFTER + 1);
    assert_eq!(
        report.failed_sign_ins, LOCKOUT_AFTER,
        "a locked attempt checks nothing"
    );
    assert_eq!(report.sign_ins, 0);

    app.reset().await?;
    assert_eq!(app.auth_report().await?, Default::default());
    assert_eq!(
        sign_in(&app, &client, PASSWORD).await?.status(),
        StatusCode::SEE_OTHER
    );
    Ok(())
}

/// Case 13: the one-time code step, and the harness's own sign-in.
#[tokio::test]
async fn case_13_the_second_factor_asks_for_the_code() -> Outcome {
    let app = TestApp::start().await?;
    app.set_auth_settings(&AuthSettings {
        second_factor: true,
        single_session: false,
    })
    .await?;
    let client = no_redirects()?;
    let first = sign_in(&app, &client, PASSWORD).await?;
    assert_eq!(location(&first), "/secure/sign-in/code");
    let pending = first
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .ok_or("no pending cookie")?
        .to_owned();
    assert!(
        !pending.starts_with(SESSION_COOKIE),
        "no session before the code"
    );
    let page = client
        .get(app.url("/secure/sign-in/code"))
        .header(header::COOKIE, &pending)
        .send()
        .await?
        .text()
        .await?;
    assert!(page.contains("autocomplete=\"one-time-code\""));
    let wrong = client
        .post(app.url("/secure/sign-in/code"))
        .header(header::COOKIE, &pending)
        .body("code=000000")
        .send()
        .await?;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    let cookie = app.session_cookie().await?;
    let page = client
        .get(app.url("/secure/orders"))
        .header(header::COOKIE, cookie)
        .send()
        .await?;
    assert_eq!(page.status(), StatusCode::OK);
    let report = app.auth_report().await?;
    assert_eq!((report.failed_codes, report.sign_ins), (1, 1));
    Ok(())
}

/// Cases 14 and 15: the control interface ends sessions; one session per account.
#[tokio::test]
async fn case_14_and_15_sessions_end_when_told_to() -> Outcome {
    let app = TestApp::start().await?;
    let client = no_redirects()?;
    let ask = async |cookie: &str| -> Result<reqwest::Response, reqwest::Error> {
        client
            .get(app.url("/secure/api/orders"))
            .header(header::COOKIE, cookie)
            .send()
            .await
    };

    let cookie = app.session_cookie().await?;
    assert_eq!(ask(&cookie).await?.status(), StatusCode::OK);
    app.expire_sessions().await?;
    let expired = ask(&cookie).await?;
    assert_eq!(expired.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(expired.json::<Value>().await?["error"], "session_expired");
    let page = client
        .get(app.url("/secure/orders"))
        .header(header::COOKIE, &cookie)
        .send()
        .await?;
    assert_eq!(
        location(&page),
        "/secure/sign-in?next=/secure/orders&reason=session_expired"
    );

    let cookie = app.session_cookie().await?;
    app.revoke_sessions().await?;
    assert_eq!(
        ask(&cookie).await?.json::<Value>().await?["error"],
        "session_revoked"
    );

    app.set_auth_settings(&AuthSettings {
        second_factor: false,
        single_session: true,
    })
    .await?;
    let first = app.session_cookie().await?;
    let second = app.session_cookie().await?;
    assert_eq!(ask(&second).await?.status(), StatusCode::OK);
    assert_eq!(
        ask(&first).await?.json::<Value>().await?["error"],
        "session_revoked"
    );
    assert_eq!(app.auth_report().await?.active_sessions, 1);
    Ok(())
}

/// Case 29: a bearer token and an API key, refused with the standard header.
#[tokio::test]
async fn case_29_the_interface_takes_a_token_or_a_key_and_refuses_a_revoked_one() -> Outcome {
    let app = TestApp::start().await?;
    let http = app.http();
    let bearer = "/secure/bearer/api/orders/A-100/items/L-101";

    let missing = http.put(app.url(bearer)).json(&update()?).send().await?;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        missing.headers()[header::WWW_AUTHENTICATE],
        "Bearer realm=\"example\""
    );
    let wrong = http
        .put(app.url(bearer))
        .bearer_auth("example-token-wrong")
        .json(&update()?)
        .send()
        .await?;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        wrong.headers()[header::WWW_AUTHENTICATE],
        "Bearer realm=\"example\", error=\"invalid_token\""
    );
    assert_eq!(app.write_count("A-100", "L-101").await?, 0);

    let right = http
        .put(app.url(bearer))
        .bearer_auth(BEARER_TOKEN)
        .json(&update()?)
        .send()
        .await?;
    assert_eq!(right.status(), StatusCode::OK);
    assert_eq!(app.write_count("A-100", "L-101").await?, 1);

    app.revoke(Credential::BearerToken).await?;
    let revoked = http
        .get(app.url("/secure/bearer/api/orders"))
        .bearer_auth(BEARER_TOKEN)
        .send()
        .await?;
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);

    let key = "/secure/key/api/orders/A-100";
    let missing = http.get(app.url(key)).send().await?;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert!(missing.headers().contains_key(header::WWW_AUTHENTICATE));
    let right = http
        .get(app.url(key))
        .header(API_KEY_HEADER, API_KEY)
        .send()
        .await?;
    assert_eq!(right.status(), StatusCode::OK);
    let wrong = http
        .get(app.url(key))
        .header(API_KEY_HEADER, "example-key-wrong")
        .send()
        .await?;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    app.revoke(Credential::ApiKey).await?;
    let revoked = http
        .get(app.url(key))
        .header(API_KEY_HEADER, API_KEY)
        .send()
        .await?;
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);

    let report = app.auth_report().await?;
    assert_eq!((report.refused_tokens, report.refused_keys), (2, 2));
    assert!(report.bearer_token_revoked && report.api_key_revoked);
    Ok(())
}

/// Case 30: the description names exactly the routes the application serves.
#[tokio::test]
async fn case_30_the_served_description_matches_the_routes() -> Outcome {
    let app = TestApp::start().await?;
    let response = app.http().get(app.url("/api/openapi.json")).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    let document: Value = response.json().await?;
    assert_eq!(document["openapi"], "3.1.0");
    let paths = document["paths"].as_object().ok_or("no paths")?;

    let mut described = Vec::new();
    for (path, operations) in paths {
        for (method, operation) in operations.as_object().ok_or("not an object")? {
            let mut address = path.clone();
            for parameter in operation["parameters"].as_array().into_iter().flatten() {
                let name = parameter["name"].as_str().ok_or("no name")?;
                let example = parameter["example"].as_str().ok_or("no example")?;
                address = address.replace(&format!("{{{name}}}"), example);
            }
            let method = reqwest::Method::from_bytes(method.to_uppercase().as_bytes())?;
            let status = app
                .http()
                .request(method.clone(), app.url(&address))
                .send()
                .await?
                .status();
            assert!(
                status != StatusCode::NOT_FOUND && status != StatusCode::METHOD_NOT_ALLOWED,
                "{method} {address} is described but answers {status}"
            );
            described.push(format!("{method} {path}"));
        }
    }
    described.sort();
    let served = [
        "GET /api/orders",
        "GET /api/orders/{order_id}",
        "GET /api/orders/{order_id}/delivery",
        "GET /api/orders/{order_id}/items/{item_code}",
        "GET /api/orders/{order_id}/totals",
        "GET /api/stock/{item_code}",
        "PUT /api/orders/{order_id}/delivery",
        "PUT /api/orders/{order_id}/items/{item_code}",
    ];
    assert_eq!(
        described, served,
        "every served route is described, and no other"
    );
    Ok(())
}

/// Case 17: the write is stored and no answer comes; a read does not use it up.
#[tokio::test]
async fn case_17_a_write_lost_after_commit_is_stored_once_and_never_answered() -> Outcome {
    let app = TestApp::start().await?;
    app.set_fault(&Fault::once(FaultKind::LostAfterCommit).on_paths("/api/orders/A-100/items"))
        .await?;
    let item = app.url("/api/orders/A-100/items/L-101");
    assert_eq!(app.http().get(&item).send().await?.status(), StatusCode::OK);

    let lost = app.http().put(&item).json(&update()?).send().await;
    assert!(lost.is_err(), "no answer arrives: {lost:?}");
    let stored = app.stored_item("A-100", "L-101").await?.ok_or("missing")?;
    assert_eq!((stored.quantity, stored.version), (3, 2));
    assert_eq!(app.write_count("A-100", "L-101").await?, 1);
    assert!(app.faults().await?.is_empty(), "the fault was used up");

    app.set_fault(&Fault::once(FaultKind::LostAfterCommit).on_paths("/api/orders/A-100/delivery"))
        .await?;
    let body = json!({"shipping": "express", "gift_wrap": true, "priority": "high",
        "deliver_on": "2026-12-24", "notes": "", "version": 1});
    let lost = app
        .http()
        .put(app.url("/api/orders/A-100/delivery"))
        .json(&body)
        .send()
        .await;
    assert!(lost.is_err());
    assert_eq!(app.delivery_write_count("A-100").await?, 1);
    Ok(())
}

/// Case 31: an answer in a changed shape, then the usual one.
#[tokio::test]
async fn case_31_an_answer_comes_in_a_changed_shape_when_told_to() -> Outcome {
    let app = TestApp::start().await?;
    app.set_fault(&Fault::once(FaultKind::ChangedShape {
        field: "length_in".to_owned(),
        renamed_to: "length_inches".to_owned(),
    }))
    .await?;
    let url = app.url("/api/orders/A-100");
    let changed: Value = app.http().get(&url).send().await?.json().await?;
    assert_eq!(changed["items"][0]["length_inches"], "150.5");
    assert!(changed["items"][0].get("length_in").is_none());
    let usual: Value = app.http().get(&url).send().await?.json().await?;
    assert_eq!(usual["items"][0]["length_in"], "150.5");
    Ok(())
}

/// Case 33: the browser's own controls, saved through the interface.
#[tokio::test]
async fn case_33_delivery_details_are_saved_from_native_controls() -> Outcome {
    let app = TestApp::start().await?;
    let page = app
        .http()
        .get(app.url("/orders/A-100/delivery"))
        .send()
        .await?
        .text()
        .await?;
    for control in [
        "<select id=\"shipping\" name=\"shipping\">",
        "<option value=\"standard\" selected>Standard</option>",
        "<input type=\"checkbox\" id=\"gift_wrap\"",
        "<input type=\"radio\" name=\"priority\" value=\"normal\" checked>",
        "<input type=\"date\" id=\"deliver_on\"",
        "<textarea id=\"notes\"",
        "window.confirm(",
        "save.disabled = ",
    ] {
        assert!(page.contains(control), "{control}");
    }

    let url = app.url("/api/orders/A-100/delivery");
    let body = json!({"shipping": "freight", "gift_wrap": true, "priority": "high",
        "deliver_on": "2026-12-24", "notes": "Ring twice.\nBack door.", "version": 1});
    let saved = app.http().put(&url).json(&body).send().await?;
    assert_eq!(saved.status(), StatusCode::OK);
    let stored = app.stored_delivery("A-100").await?.ok_or("missing")?;
    assert_eq!(
        (
            stored.shipping,
            stored.gift_wrap,
            stored.priority,
            stored.version
        ),
        (Shipping::Freight, true, Priority::High, 2)
    );
    assert_eq!(stored.notes, "Ring twice.\nBack door.");

    let mut bad = body.clone();
    bad["deliver_on"] = json!("2026-02-30");
    bad["version"] = json!(2);
    let refused = app.http().put(&url).json(&bad).send().await?;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let stale = app.http().put(&url).json(&body).send().await?;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let submitted = app
        .http()
        .put(app.url("/api/orders/A-101/delivery"))
        .json(&body)
        .send()
        .await?;
    assert_eq!(submitted.status(), StatusCode::CONFLICT);
    assert_eq!(app.delivery_write_count("A-100").await?, 1);
    assert_eq!(app.delivery_write_count("A-101").await?, 0);
    Ok(())
}

/// Case 32: loading the trap pages writes nothing by itself; following a link does.
#[tokio::test]
async fn case_32_trap_pages_count_every_write_they_cause() -> Outcome {
    let app = TestApp::start().await?;
    for page in [
        "/traps/on-load",
        "/traps/worker",
        "/traps/socket",
        "/traps/links",
    ] {
        let response = app.http().get(app.url(page)).send().await?;
        assert_eq!(response.status(), StatusCode::OK, "{page}");
    }
    let script = app
        .http()
        .get(app.url("/traps/worker.js"))
        .send()
        .await?
        .text()
        .await?;
    assert!(script.contains("/traps/write/worker"));
    assert!(
        app.trap_writes().await?.is_empty(),
        "fetching the pages ran no script"
    );

    app.http()
        .get(app.url("/traps/write/orders/A-100/delete"))
        .send()
        .await?;
    app.http()
        .post(app.url("/traps/write/on-load"))
        .send()
        .await?;
    let writes = app.trap_writes().await?;
    assert_eq!(writes.get("orders/A-100/delete"), Some(&1));
    assert_eq!(writes.get("on-load"), Some(&1));
    app.reset().await?;
    assert!(app.trap_writes().await?.is_empty());
    Ok(())
}

/// Cases 23 and 34: a redirect to another host name, an edge block, a rate limit.
#[tokio::test]
async fn case_23_and_34_answers_from_the_edge() -> Outcome {
    let app = TestApp::start().await?;
    let client = no_redirects()?;
    let moved = client.get(app.url("/edge/elsewhere")).send().await?;
    assert_eq!(moved.status(), StatusCode::FOUND);
    let to = location(&moved);
    let port = app.url("").rsplit(':').next().ok_or("no port")?.to_owned();
    assert_eq!(to, format!("http://localhost:{port}/orders"));

    let blocked = client.get(app.url("/edge/blocked")).send().await?;
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
    let limited = client.get(app.url("/edge/limited")).send().await?;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.headers()[header::RETRY_AFTER], "30");
    Ok(())
}
