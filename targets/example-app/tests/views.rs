//! The views of the test application in forms other than JSON: each serves
//! the data the store holds, with the content type a service would send.

use std::error::Error;

use example_app::harness::TestApp;
use example_app::views::{ORDERS_NAMESPACE, label, status_line};
use reqwest::StatusCode;

type Outcome = Result<(), Box<dyn Error>>;

async fn fetch(app: &TestApp, path: &str) -> Result<(StatusCode, String, Vec<u8>), Box<dyn Error>> {
    let response = app.http().get(app.url(path)).send().await?;
    let status = response.status();
    let kind = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    Ok((status, kind, response.bytes().await?.to_vec()))
}

#[tokio::test]
async fn each_view_serves_the_stored_data_in_its_own_form() -> Outcome {
    let app = TestApp::start().await?;
    let order = app.stored_order("A-100").await?.ok_or("no seed order")?;

    let (status, kind, body) = fetch(&app, "/views/xml/orders/A-100").await?;
    let text = String::from_utf8(body)?;
    assert_eq!(
        (status, kind.as_str()),
        (StatusCode::OK, "application/xml; charset=utf-8")
    );
    assert!(text.contains(&format!("xmlns:ord=\"{ORDERS_NAMESPACE}\"")));
    assert!(text.contains("<ord:customer>Example Customer One</ord:customer>"));
    assert_eq!(text.matches("<item code=").count(), order.items.len());

    let (status, _, body) = fetch(&app, "/views/xml/orders/A-100/items/L-101").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(body)?.contains("version=\"1\""));
    let (status, kind, _) = fetch(&app, "/views/xml/orders/A-999").await?;
    assert_eq!(
        (status, kind.starts_with("text/plain")),
        (StatusCode::NOT_FOUND, true)
    );

    let (_, _, body) = fetch(&app, "/views/xml/declared").await?;
    assert!(String::from_utf8(body)?.contains("<!DOCTYPE order ["));

    let mut orders = Vec::new();
    for id in ["A-100", "A-101", "A-102"] {
        orders.push(app.stored_order(id).await?.ok_or("no seed order")?);
    }
    let (_, kind, body) = fetch(&app, "/views/text/status").await?;
    assert_eq!(kind, "text/plain; charset=utf-8");
    assert_eq!(
        String::from_utf8(body)?,
        status_line(&orders.iter().collect::<Vec<_>>())
    );

    // The old device's line is ISO-8859-1, with no character set named.
    let (_, kind, body) = fetch(&app, "/views/text/legacy").await?;
    assert_eq!(kind, "text/plain");
    assert!(body.contains(&0xB7) && std::str::from_utf8(&body).is_err());

    let (_, kind, body) = fetch(&app, "/views/bytes/orders/A-100/label").await?;
    assert_eq!(kind, "application/octet-stream");
    assert_eq!(body, label(&order));
    Ok(())
}
