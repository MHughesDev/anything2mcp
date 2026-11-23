//! The answers a reader must refuse, and the search behind `POST`
//! (decision 0249): each is what its module says, and the search stores
//! nothing.

use std::error::Error;

use example_app::answers::{ECHOED_SECRET, long_namespace_document};
use example_app::harness::TestApp;
use reqwest::StatusCode;

type Outcome = Result<(), Box<dyn Error>>;

#[tokio::test]
async fn the_search_behind_post_reads_and_stores_nothing() -> Outcome {
    let app = TestApp::start().await?;
    let response = app
        .http()
        .post(app.url("/views/search/orders?status=draft"))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let found: serde_json::Value = response.json().await?;
    assert_eq!(found, serde_json::json!({"orders": ["A-100", "A-102"]}));
    let refused = app
        .http()
        .post(app.url("/views/search/orders?status=gone"))
        .send()
        .await?;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(app.writes().await?.is_empty());
    assert_eq!(
        app.seen().await?,
        ["POST /views/search/orders", "POST /views/search/orders"]
    );
    Ok(())
}

#[tokio::test]
async fn each_hostile_answer_is_served_as_its_module_says() -> Outcome {
    let app = TestApp::start().await?;
    let body = |path: &'static str| {
        let app = &app;
        async move {
            let response = app.http().get(app.url(path)).send().await?;
            Ok::<_, reqwest::Error>((response.status(), response.bytes().await?.to_vec()))
        }
    };
    let (status, namespace) = body("/views/hostile/namespace").await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(namespace, long_namespace_document().into_bytes());
    // Under the default ceiling of an answer, 8,000,000 bytes.
    assert!(namespace.len() < 8_000_000 && namespace.len() > 4_000_000);
    let (_, control) = body("/views/hostile/control").await?;
    let control = String::from_utf8(control)?;
    assert!(control.contains("&#x1B;") && control.contains('\u{7}'));
    let (_, text) = body("/views/hostile/control-text").await?;
    assert!(text.contains(&0x1b));
    let (_, label) = body("/views/hostile/secret").await?;
    assert!(
        label
            .windows(ECHOED_SECRET.len())
            .any(|part| part == ECHOED_SECRET.as_bytes())
    );
    let (status, _) = body("/views/hostile/other").await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    Ok(())
}

/// Case 35: the shipment's optional `carrier` is answered as the control
/// interface says, and the description lists it as not required and not
/// nullable.
#[tokio::test]
async fn the_shipments_optional_carrier_is_answered_as_it_is_told_to() -> Outcome {
    use example_app::answers::CarrierAnswer;

    let app = TestApp::start().await?;
    let address = app.url("/api/orders/A-100/shipment");
    let answer = || async {
        let response = app.http().get(&address).send().await?;
        assert_eq!(response.status(), StatusCode::OK);
        response.json::<serde_json::Value>().await
    };
    let carrier = serde_json::json!({"name": "Example Freight", "tracking": "T-100"});
    assert_eq!(
        answer().await?,
        serde_json::json!({"order_id": "A-100", "carrier": carrier})
    );
    app.set_carrier(CarrierAnswer::Null).await?;
    assert_eq!(
        answer().await?,
        serde_json::json!({"order_id": "A-100", "carrier": null})
    );
    app.set_carrier(CarrierAnswer::Missing).await?;
    assert_eq!(answer().await?, serde_json::json!({"order_id": "A-100"}));
    app.set_carrier(CarrierAnswer::Text).await?;
    assert_eq!(
        answer().await?,
        serde_json::json!({"order_id": "A-100", "carrier": "Example Freight"})
    );
    let missing = app
        .http()
        .get(app.url("/api/orders/Z-1/shipment"))
        .send()
        .await?;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let described: serde_json::Value = app
        .http()
        .get(app.url("/api/openapi.json"))
        .send()
        .await?
        .json()
        .await?;
    let schema = &described["components"]["schemas"]["Shipment"];
    assert_eq!(schema["required"], serde_json::json!(["order_id"]));
    assert!(schema["properties"]["carrier"].get("nullable").is_none());

    // A reset restores the object.
    app.reset().await?;
    assert_eq!(
        answer().await?,
        serde_json::json!({"order_id": "A-100", "carrier": carrier})
    );
    assert!(app.writes().await?.is_empty());
    Ok(())
}

/// Case 37: the same switch answers the hand-off's optional `packages` as
/// `null` or leaves it out, and its required `carrier_code` as `null`.
#[tokio::test]
async fn the_handoffs_optional_packages_and_required_code_follow_the_switch() -> Outcome {
    use example_app::answers::CarrierAnswer;

    let app = TestApp::start().await?;
    let address = app.url("/api/orders/A-100/handoff");
    let answer = || async {
        app.http()
            .get(&address)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await
    };
    assert_eq!(answer().await?["packages"], 2);
    app.set_carrier(CarrierAnswer::Null).await?;
    assert!(answer().await?["packages"].is_null());
    app.set_carrier(CarrierAnswer::Missing).await?;
    assert!(answer().await?.get("packages").is_none());
    app.set_carrier(CarrierAnswer::RequiredNull).await?;
    let held = answer().await?;
    assert!(held["carrier_code"].is_null());
    assert_eq!(held["packages"], 2);
    Ok(())
}

/// Case 36: the hand-off answers with a `client_secret`, the pickup needs an
/// `api_key`, and the description says so.
#[tokio::test]
async fn the_handoff_holds_an_invented_secret_and_the_pickup_needs_a_key() -> Outcome {
    use example_app::answers::HANDOFF_SECRET;

    let app = TestApp::start().await?;
    let handoff: serde_json::Value = app
        .http()
        .get(app.url("/api/orders/A-100/handoff?access_token=x"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(handoff["client_secret"], HANDOFF_SECRET);
    assert_eq!(handoff["carrier_code"], "EXF");
    for (query, status) in [
        ("", StatusCode::BAD_REQUEST),
        ("?api_key=x", StatusCode::OK),
    ] {
        let address = app.url(&format!("/api/orders/A-100/pickup{query}"));
        assert_eq!(app.http().get(address).send().await?.status(), status);
    }
    let described: serde_json::Value = app
        .http()
        .get(app.url("/api/openapi.json"))
        .send()
        .await?
        .json()
        .await?;
    assert!(
        described["components"]["schemas"]["Handoff"]["properties"]["client_secret"].is_object()
    );
    assert!(app.writes().await?.is_empty());
    Ok(())
}
