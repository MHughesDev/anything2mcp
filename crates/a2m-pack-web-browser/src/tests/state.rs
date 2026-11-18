//! A session's state: what the browser holds for an application's origins
//! after a sign-in through its own page, taken out, and placed into a
//! fresh browser, with nothing of it written to a data directory
//! (decision 0245).

use example_app::harness::{PASSWORD, SESSION_COOKIE, USERNAME};

use crate::{Input, Primitive};

use super::*;

/// Every file under `folder` whose bytes hold `needle`.
fn files_holding(folder: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = std::fs::read(&path)
                && bytes
                    .windows(needle.len())
                    .any(|window| window == needle.as_bytes())
            {
                found.push(path);
            }
        }
    }
    found
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_is_taken_out_placed_in_a_fresh_browser_and_never_written_to_disk() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("state");
    let origins = origins(&app);

    // A person's sign-in through the application's own page.
    let browser = stand.open(&profile("first")).await;
    let tab = must(browser.acting_tab(&environment(), &origins).await);
    must(
        tab.goto(&Address::new(&app.url("/secure/sign-in")).unwrap(), soon())
            .await,
    );
    let user = must(tab.locate(&textbox("Username"), soon()).await);
    must(
        tab.input(&user, &Input::Fill(USERNAME.to_owned()), soon())
            .await,
    );
    let password = must(
        tab.locate(
            &Query::new(By::Label {
                text: "Password".to_owned(),
            }),
            soon(),
        )
        .await,
    );
    must(
        tab.input(&password, &Input::Fill(PASSWORD.to_owned()), soon())
            .await,
    );
    let button = must(tab.locate(&Query::role("button", "Sign in"), soon()).await);
    must(tab.input(&button, &Input::Click, soon()).await);
    let orders = Primitive::Visible(Query::role("heading", "Orders"));
    must(tab.wait(&orders, soon()).await);
    assert_eq!(app.auth_report().await.unwrap().sign_ins, 1);

    // What the browser holds for the application's origin is the session.
    let state = must(browser.export_state(&origins, soon()).await);
    let session = state
        .cookies
        .iter()
        .find(|cookie| cookie.name == SESSION_COOKIE)
        .expect("the session cookie is held");
    assert!(session.http_only);
    assert_eq!(session.path, "/secure");
    let value = session.value.expose().to_owned();
    assert!(!value.is_empty());
    // Its debug form names the cookies and shows no value.
    assert!(!format!("{state:?}").contains(&value));
    // Another origin's cookies are not the application's.
    let elsewhere = Origins::of([&Address::new("https://elsewhere.example.test").unwrap()]);
    assert!(
        must(browser.export_state(&elsewhere, soon()).await)
            .cookies
            .is_empty()
    );
    drop(tab);
    let closed = stand.browsers.close(&profile("first")).await;
    assert!(closed.complete, "{closed:?}");

    // The browser kept it in memory: no file of its data directory holds it.
    let written = files_holding(&stand.folder.0, &value);
    assert!(
        written.is_empty(),
        "the session cookie is on disk: {written:?}"
    );

    // Placed into a fresh browser, it signs that browser in, once.
    let fresh = stand.open(&profile("second")).await;
    // A state is placed only for the origins of the run: for another
    // site's, nothing is placed (audit finding A7).
    let refused = fresh
        .import_state("s1:1", &state, &elsewhere, soon())
        .await
        .unwrap_err();
    assert!(
        matches!(refused, BrowserError::ForeignOrigin(_)),
        "{refused:?}"
    );
    assert!(must(
        fresh.import_state("s1:1", &state, &origins, soon()).await
    ));
    assert!(!must(
        fresh.import_state("s1:1", &state, &origins, soon()).await
    ));
    let tab = must(fresh.acting_tab(&environment(), &origins).await);
    must(
        tab.goto(&Address::new(&app.url("/secure/orders")).unwrap(), soon())
            .await,
    );
    must(tab.wait(&orders, soon()).await);
    assert_eq!(app.auth_report().await.unwrap().sign_ins, 1);
    drop(tab);
    let written = files_holding(&stand.folder.0, &value);
    assert!(
        written.is_empty(),
        "the session cookie is on disk: {written:?}"
    );
    stand.end().await;
}
