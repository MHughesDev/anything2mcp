//! Throwaway spike code. The shared harness alone, to measure what every
//! candidate's build and dependency count already contain.
use spike_browser_common as common;

#[tokio::main]
async fn main() {
    let app = common::TestApp::start()
        .await
        .expect("the test application starts");
    println!("{}", app.url(common::FORM_PATH));
    let _ = serde_json::json!({});
}
