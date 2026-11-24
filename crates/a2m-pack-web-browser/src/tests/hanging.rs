//! A tab whose page never returns from its own script: the step ends at its
//! time limit as a timeout, the tab is closed, the next step has a new tab,
//! and after the set number of hangs the browser is recycled between two
//! runs, never inside one (spec section 224, package 9.3f).

use crate::protocol::GetTargets;
use crate::tab::Status;
use crate::{ActTab, Primitive};

use super::*;

/// The page of the stand whose own script, run as its document is parsed,
/// never returns: the document never finishes, and the page answers
/// nothing after.
const HANGS: &str = "/traps/hangs";

/// The settings of this test: a hang is called after a short probe, and
/// the browser is recycled after two.
fn settings() -> BrowserSettings {
    BrowserSettings {
        hangs_before_recycle: 2,
        hang_probe: Duration::from_millis(1500),
        ..test_support::settings()
    }
}

/// Whether the browser lists a page by this name.
async fn listed(browser: &Browser, target: &str) -> bool {
    let targets = browser
        .inner
        .client
        .call(None, &GetTargets {}, soon())
        .await
        .unwrap();
    targets
        .target_infos
        .iter()
        .any(|listed| listed.target_id == target)
}

/// Waits until the tab is called hung and its page closed, and checks that
/// the browser lists no page of it.
async fn closed_as_hung(browser: &Browser, tab: &ActTab) {
    let mut closed = tab.page.hang_closed.subscribe();
    tokio::time::timeout_at(soon().at(), closed.wait_for(|closed| *closed))
        .await
        .expect("the tab is called hung and closed")
        .unwrap();
    assert_eq!(*tab.page.status.borrow(), Status::Hung);
    assert!(!listed(browser, &tab.page.target).await);
}

/// A run's read on a page of the stand: the form, and one of its fields.
async fn reads(browser: &Browser, app: &TestApp) -> ActTab {
    let tab = must(browser.acting_tab(&environment(), &origins(app)).await);
    must(
        tab.goto(&Address::new(&app.url(FORM)).unwrap(), soon())
            .await,
    );
    must(
        tab.wait(&Primitive::Visible(textbox("Quantity")), soon())
            .await,
    );
    must(tab.locate(&textbox("Description"), soon()).await);
    tab
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hung_tab_ends_its_step_as_a_timeout_is_closed_and_repeated_hangs_recycle_the_browser_between_runs()
 {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::new("hanging", settings(), &[]);
    let clerk = profile("clerk");
    let hangs = Address::new(&app.url(HANGS)).unwrap();

    // The first run. Its step goes to the page that hangs, under a time
    // limit of two seconds, and ends at it as a timeout: `transient`.
    let browser = stand.open(&clerk).await;
    let first = browser.subsystem();
    let hung = must(browser.acting_tab(&environment(), &origins(&app)).await);
    let ended = hung
        .goto(&hangs, Deadline::after(Duration::from_secs(2)))
        .await
        .unwrap_err();
    assert_eq!(
        ended,
        BrowserError::Timeout {
            dispatch: Dispatch::NotSent
        }
    );
    let failure = ended.failure().unwrap();
    assert_eq!(failure.class(), FailureClass::Transient);
    assert_eq!(failure.code(), &codes::TIMEOUT);

    // The page answers not even the probe: the tab is closed, and the
    // browser lists no page of it. A later call on it is a timeout at once,
    // and nothing of it reaches the page.
    closed_as_hung(&browser, &hung).await;
    assert_eq!(
        hung.locate(&css("p"), soon()).await.unwrap_err(),
        BrowserError::Timeout {
            dispatch: Dispatch::NotSent
        }
    );

    // The next step opens a new tab in the same browser, and it reads.
    let next = reads(&browser, &app).await;
    assert_ne!(next.page.target, hung.page.target);
    assert_eq!(browser.subsystem(), first);
    let listing = stand.browsers.list();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].hangs, 1);
    drop((next, hung));

    // The second run has the same browser: one hang is fewer than two.
    let browser = stand.open(&clerk).await;
    assert_eq!(browser.subsystem(), first);
    // Its step is ended by its caller's own limit, as the engine ends a
    // step that passes its time: the call is dropped, and the page that
    // did not answer is probed all the same.
    let hung = must(browser.acting_tab(&environment(), &origins(&app)).await);
    let dropped = tokio::time::timeout(Duration::from_secs(2), hung.goto(&hangs, soon())).await;
    assert!(dropped.is_err(), "{dropped:?}");
    closed_as_hung(&browser, &hung).await;
    assert_eq!(stand.browsers.list()[0].hangs, 2);

    // Two hangs are the set number, and the run still holds its tab: the
    // browser is not recycled inside a run.
    let inside = stand.open(&clerk).await;
    assert_eq!(inside.subsystem(), first);
    drop((inside, hung, browser));

    // Between the two runs it is: the next run has a new browser, the old
    // one is gone from the supervisor's list, and the next run reads.
    let browser = stand.open(&clerk).await;
    assert_ne!(browser.subsystem(), first);
    let running = stand.supervisor.list();
    assert_eq!(running.len(), 1, "{running:?}");
    assert_eq!(running[0].id, browser.subsystem());
    let tab = reads(&browser, &app).await;
    let listing = stand.browsers.list();
    assert_eq!((listing.len(), listing[0].hangs), (1, 0));
    drop((tab, browser));
    stand.end().await;
}
