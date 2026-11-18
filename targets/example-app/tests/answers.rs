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
