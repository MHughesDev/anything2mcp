//! The test application's own tests: cases 1 to 4 of `docs/plan/test-application.md`,
//! the control interface, and the harness. Every test talks to a real server over HTTP
//! and proves a write by asking the control interface what was stored.

use std::error::Error;
use std::time::{Duration, Instant};

use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;
use example_app::store::{OrderStatus, WriteAction};
use reqwest::StatusCode;
use serde_json::{Value, json};

type Outcome = Result<(), Box<dyn Error>>;

const ITEM: &str = "/api/orders/A-100/items/L-101";

async fn put(
    app: &TestApp,
    path: &str,
    body: Value,
) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let response = app.http().put(app.url(path)).json(&body).send().await?;
    Ok((response.status(), response.json().await?))
}

async fn get(app: &TestApp, path: &str) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let response = app.http().get(app.url(path)).send().await?;
    Ok((response.status(), response.json().await?))
}

async fn page(app: &TestApp, path: &str) -> Result<(StatusCode, String), Box<dyn Error>> {
    let response = app.http().get(app.url(path)).send().await?;
    Ok((response.status(), response.text().await?))
}

/// The slice's proof: reset, change a record, ask the application what it stored.
#[tokio::test]
async fn a_write_is_proven_by_asking_the_application_what_it_stored() -> Outcome {
    let app = TestApp::start().await?;
    app.reset().await?;
    let before = app
        .stored_item("A-100", "L-101")
        .await?
        .ok_or("the seed item is missing")?;
    assert_eq!((before.length_sixteenths, before.version), (2408, 1));

    let body =
        json!({"description": "Left rail", "quantity": 2, "length_in": "151.25", "version": 1});
    let (status, answer) = put(&app, ITEM, body).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["length_in"], "151.25");

    let after = app
        .stored_item("A-100", "L-101")
        .await?
        .ok_or("the item vanished")?;
    assert_eq!(after.length_sixteenths, 2420);
    assert_eq!(after.version, 2);
    assert_eq!(app.write_count("A-100", "L-101").await?, 1);
    let writes = app.writes().await?;
    assert_eq!(writes.len(), 1);
    assert_eq!(
        (writes[0].request, writes[0].action),
        (1, WriteAction::Updated)
    );
    Ok(())
}

/// Case 1: a list, a detail page, and a form, over a JSON interface.
#[tokio::test]
async fn case_1_the_json_interface_lists_and_shows_records() -> Outcome {
    let app = TestApp::start().await?;
    let (status, orders) = get(&app, "/api/orders").await?;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = orders
        .as_array()
        .ok_or("not a list")?
        .iter()
        .filter_map(|o| o["order_id"].as_str())
        .collect();
    assert_eq!(ids, ["A-100", "A-101", "A-102"]);
    assert_eq!(
        orders[0],
        json!({"order_id": "A-100", "customer": "Example Customer One", "status": "draft", "lines": 3})
    );

    let (status, order) = get(&app, "/api/orders/A-100").await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(order["items"].as_array().map(Vec::len), Some(3));

    let response = app.http().get(app.url(ITEM)).send().await?;
    assert_eq!(
        response.headers().get("etag").and_then(|v| v.to_str().ok()),
        Some("\"1\"")
    );
    let item: Value = response.json().await?;
    assert_eq!(
        item,
        json!({"order_id": "A-100", "item_code": "L-101", "description": "Left rail",
               "quantity": 2, "length_in": "150.5", "version": 1})
    );

    for missing in [
        "/api/orders/Z-999",
        "/api/orders/A-100/items/L-999",
        "/api/orders/Z-999/items/L-101",
    ] {
        let (status, answer) = get(&app, missing).await?;
        assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
        assert_eq!(answer["error"], "not_found", "{missing}");
    }
    Ok(())
}

#[tokio::test]
async fn case_1_the_pages_name_their_controls() -> Outcome {
    let app = TestApp::start().await?;
    let (status, list) = page(&app, "/orders").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        list.contains("<h1>Orders</h1>") && list.contains("<a href=\"/orders/A-100\">A-100</a>")
    );

    let (status, detail) = page(&app, "/orders/A-100").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(detail.contains("<h1>Order A-100</h1>") && detail.contains("Add line item"));
    assert!(detail.contains("<a href=\"/orders/A-100/items/L-103\">L-103</a>"));

    let (status, form) = page(&app, "/orders/A-100/items/L-101").await?;
    assert_eq!(status, StatusCode::OK);
    for expected in [
        "<h2>Line Item Details</h2>",
        "<label for=\"item_code\">Item code</label>",
        "<label for=\"description\">Description</label>",
        "<label for=\"quantity\">Quantity</label>",
        "<legend>Dimensions</legend>",
        "<label for=\"length\">Length</label>",
        "name=\"length_in\" value=\"150.5\"",
        "<button type=\"submit\">Save</button>",
        "data-version=\"1\"",
    ] {
        assert!(form.contains(expected), "the form lacks {expected}");
    }

    let (status, blank) = page(&app, "/orders/A-100/items/new").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        blank.contains("data-version=\"\"")
            && blank.contains("name=\"item_code\" value=\"\" required>")
    );

    for missing in ["/orders/Z-999", "/orders/A-100/items/L-999"] {
        let (status, body) = page(&app, missing).await?;
        assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
        assert!(body.contains("<h1>Not found</h1>"), "{missing}");
    }
    Ok(())
}

#[tokio::test]
async fn case_1_a_new_record_is_created_once() -> Outcome {
    let app = TestApp::start().await?;
    assert!(app.stored_item("A-102", "L-301").await?.is_none());
    let body = json!({"description": "  Top cap  ", "quantity": 6, "length_in": "12"});
    let (status, answer) = put(&app, "/api/orders/A-102/items/L-301", body).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(answer["version"], 1);

    let stored = app
        .stored_item("A-102", "L-301")
        .await?
        .ok_or("nothing was stored")?;
    assert_eq!(
        (
            stored.description.as_str(),
            stored.quantity,
            stored.length_sixteenths
        ),
        ("Top cap", 6, 192)
    );
    let writes = app.writes().await?;
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].action, WriteAction::Created);
    Ok(())
}

/// Case 2: bad input is rejected with a message, and nothing is stored.
#[tokio::test]
async fn case_2_bad_input_is_rejected_with_messages_and_stores_nothing() -> Outcome {
    let app = TestApp::start().await?;
    let body = json!({"description": "", "quantity": 0, "length_in": "150.3", "version": 1});
    let (status, answer) = put(&app, ITEM, body).await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(answer["error"], "validation");
    assert_eq!(
        answer["problems"],
        json!([
            {"field": "description", "message": "Description is required."},
            {"field": "quantity", "message": "Quantity must be between 1 and 9999."},
            {"field": "length_in", "message": "Length must be a multiple of 1/16 inch."},
        ])
    );
    let stored = app
        .stored_item("A-100", "L-101")
        .await?
        .ok_or("the seed item is missing")?;
    assert_eq!((stored.length_sixteenths, stored.version), (2408, 1));
    assert_eq!(app.write_count("A-100", "L-101").await?, 0);

    let response = app
        .http()
        .put(app.url(ITEM))
        .body("not json")
        .header("content-type", "application/json")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.json::<Value>().await?["error"], "bad_request");

    let unknown =
        json!({"description": "x", "quantity": 1, "length_in": "1", "version": 1, "colour": "red"});
    assert_eq!(put(&app, ITEM, unknown).await?.0, StatusCode::BAD_REQUEST);
    assert!(app.writes().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn case_2_a_submitted_order_refuses_every_write() -> Outcome {
    let app = TestApp::start().await?;
    assert_eq!(
        app.stored_order("A-101").await?.map(|o| o.status),
        Some(OrderStatus::Submitted)
    );
    let body = json!({"description": "Base plate", "quantity": 2, "length_in": "24", "version": 1});
    let (status, answer) = put(&app, "/api/orders/A-101/items/L-201", body).await?;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(answer["error"], "order_not_draft");
    assert_eq!(
        app.stored_item("A-101", "L-201").await?.map(|i| i.quantity),
        Some(1)
    );
    assert!(app.writes().await?.is_empty());
    Ok(())
}

/// Case 3: every record has a version, and a stale write is refused.
#[tokio::test]
async fn case_3_a_stale_version_is_refused_and_stores_nothing() -> Outcome {
    let app = TestApp::start().await?;
    let first =
        json!({"description": "Left rail", "quantity": 3, "length_in": "150.5", "version": 1});
    assert_eq!(put(&app, ITEM, first).await?.0, StatusCode::OK);

    let stale =
        json!({"description": "Left rail", "quantity": 9, "length_in": "150.5", "version": 1});
    let (status, answer) = put(&app, ITEM, stale).await?;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(answer["error"], "version_conflict");
    assert_eq!(answer["current_version"], 2);

    let stored = app
        .stored_item("A-100", "L-101")
        .await?
        .ok_or("the item vanished")?;
    assert_eq!((stored.quantity, stored.version), (3, 2));
    assert_eq!(app.write_count("A-100", "L-101").await?, 1);

    let none = json!({"description": "Left rail", "quantity": 9, "length_in": "150.5"});
    let (status, answer) = put(&app, ITEM, none).await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(answer["problems"][0]["field"], "version");
    assert_eq!(app.write_count("A-100", "L-101").await?, 1);
    Ok(())
}

/// Case 4: one endpoint answers slowly.
#[tokio::test]
async fn case_4_the_slow_endpoint_waits_as_long_as_it_is_told() -> Outcome {
    let app = TestApp::start().await?;
    app.set_slow_ms(150).await?;
    let started = Instant::now();
    let (status, totals) = get(&app, "/api/orders/A-100/totals").await?;
    assert!(started.elapsed() >= Duration::from_millis(150));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        totals,
        json!({"order_id": "A-100", "lines": 3, "total_quantity": 8, "total_length_in": "746.25"})
    );
    Ok(())
}

/// Case 4: one endpoint fails some of the time, on a pattern a test can rely on.
#[tokio::test]
async fn case_4_the_flaky_endpoint_fails_every_third_call() -> Outcome {
    let app = TestApp::start().await?;
    let mut seen = Vec::new();
    for _ in 0..6 {
        seen.push(get(&app, "/api/stock/L-101").await?.0.as_u16());
    }
    assert_eq!(seen, [200, 200, 503, 200, 200, 503]);

    get(&app, "/api/stock/L-101").await?;
    app.reset().await?;
    let (status, answer) = get(&app, "/api/stock/L-101").await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer, json!({"item_code": "L-101", "in_stock": 25}));
    Ok(())
}

#[tokio::test]
async fn a_fault_applies_to_the_requests_it_names_and_then_clears() -> Outcome {
    let app = TestApp::start().await?;
    app.set_fault(
        &Fault::once(FaultKind::Status { status: 503 }).on_paths("/api/orders/A-100/items"),
    )
    .await?;

    assert_eq!(
        get(&app, "/api/orders").await?.0,
        StatusCode::OK,
        "another path is not affected"
    );
    let body =
        json!({"description": "Left rail", "quantity": 5, "length_in": "150.5", "version": 1});
    let (status, answer) = put(&app, ITEM, body.clone()).await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(answer["error"], "fault");
    assert_eq!(
        app.write_count("A-100", "L-101").await?,
        0,
        "a failed request stores nothing"
    );

    assert_eq!(
        put(&app, ITEM, body).await?.0,
        StatusCode::OK,
        "the fault has cleared itself"
    );
    assert_eq!(app.write_count("A-100", "L-101").await?, 1);

    let delay = Fault {
        kind: FaultKind::Delay { ms: 120 },
        path_prefix: None,
        times: None,
    };
    app.set_fault(&delay).await?;
    let started = Instant::now();
    assert_eq!(page(&app, "/orders").await?.0, StatusCode::OK);
    assert!(started.elapsed() >= Duration::from_millis(120));
    assert_eq!(app.faults().await?, [delay]);
    app.clear_faults().await?;
    assert!(app.faults().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn reset_restores_the_seed_data_and_forgets_every_write() -> Outcome {
    let app = TestApp::start().await?;
    let body = json!({"description": "Changed", "quantity": 7, "length_in": "1", "version": 1});
    assert_eq!(put(&app, ITEM, body).await?.0, StatusCode::OK);
    put(
        &app,
        "/api/orders/A-102/items/L-301",
        json!({"description": "New", "quantity": 1, "length_in": "1"}),
    )
    .await?;
    app.set_fault(&Fault {
        kind: FaultKind::Status { status: 500 },
        path_prefix: None,
        times: None,
    })
    .await?;

    app.reset().await?;

    let stored = app
        .stored_item("A-100", "L-101")
        .await?
        .ok_or("the seed item is missing")?;
    assert_eq!(
        (stored.description.as_str(), stored.quantity, stored.version),
        ("Left rail", 2, 1)
    );
    assert!(app.stored_item("A-102", "L-301").await?.is_none());
    assert!(app.writes().await?.is_empty());
    assert_eq!(
        get(&app, "/api/orders").await?.0,
        StatusCode::OK,
        "the fault was cleared too"
    );
    Ok(())
}

#[tokio::test]
async fn two_applications_share_nothing() -> Outcome {
    let (one, two) = (TestApp::start().await?, TestApp::start().await?);
    let body =
        json!({"description": "Left rail", "quantity": 9, "length_in": "150.5", "version": 1});
    assert_eq!(put(&one, ITEM, body).await?.0, StatusCode::OK);
    assert_eq!(
        one.stored_item("A-100", "L-101").await?.map(|i| i.quantity),
        Some(9)
    );
    assert_eq!(
        two.stored_item("A-100", "L-101").await?.map(|i| i.quantity),
        Some(2)
    );
    assert!(two.writes().await?.is_empty());
    Ok(())
}
