//! Memory runs short (spec section 224, package 9.3g): while the engine's
//! browsers hold their ceiling, a new run's browser work is not started and
//! the open answers `busy` with a retry time; with the ceiling raised the
//! run goes; and a browser past its own limit is replaced between two runs,
//! never inside one (decision 0425).

use crate::tab::Status;
use crate::{ActTab, MEMORY_CEILING, Primitive, test_support::launches_under};

use super::*;

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

/// What the browser holds now, measured as the crate measures it, with
/// each process's kind and figure and the browser's sum written to the test
/// output, one line each, every line starting `a2m-memory:`, so that a CI
/// log can be searched for them (package OS4). CI runs these tests a second
/// time with their output shown.
async fn held(browser: &Browser, when: &str) -> u64 {
    let processes = browser
        .inner
        .processes(Duration::from_secs(5))
        .await
        .expect("the browser's memory is measured");
    let system = std::env::consts::OS;
    for one in &processes {
        let figure = one.bytes.map_or_else(
            || "ended before it was measured".to_owned(),
            |bytes| bytes.to_string(),
        );
        println!(
            "a2m-memory: {system} {when}: process {} kind {} holds {figure}",
            one.id, one.kind
        );
    }
    let sum = processes
        .iter()
        .filter_map(|one| one.bytes)
        .fold(0_u64, u64::saturating_add);
    println!(
        "a2m-memory: {system} {when}: browser of {} processes holds {sum} ({} MiB)",
        processes.len(),
        sum / (1024 * 1024)
    );
    sum
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_ceiling_below_the_browsers_use_a_new_runs_browser_work_is_not_started_and_with_it_raised_the_run_goes()
 {
    let app = TestApp::start().await.unwrap();
    let settings = BrowserSettings {
        memory_retry: Duration::from_millis(2500),
        ..test_support::settings()
    };
    let stand = Stand::new("memory-short", settings, &[]);
    let root = stand.folder.0.join("profiles");
    let clerk = profile("clerk");

    // The first run reads, and ends.
    let browser = stand.open(&clerk).await;
    let first = browser.subsystem();
    let tab = reads(&browser, &app).await;
    let use_now = held(&browser, "one tab on the form").await;
    assert!(use_now > 0);
    drop((tab, browser));
    let launches = launches_under(&root);

    // The ceiling is set below what the browser holds. The next run's open
    // starts nothing: it answers `busy`, `memory_short`, with the retry time
    // of the settings, and no browser is started, none handed out, and no
    // tab opened.
    stand.browsers.set_memory_ceiling(use_now / 2);
    let refused = stand
        .browsers
        .open(&clerk, Visibility::Background)
        .await
        .unwrap_err();
    assert!(
        matches!(
            &refused,
            BrowserError::LaunchFailed(LaunchReason::MemoryShort(short))
                if short.held.is_some_and(|held| held >= use_now / 2) && short.ceiling == use_now / 2
        ),
        "{refused}"
    );
    assert_eq!(refused.dispatch(), Dispatch::NotSent);
    let failure = refused.failure().unwrap();
    assert_eq!(failure.class(), FailureClass::Busy);
    assert_eq!(failure.code(), &codes::MEMORY_SHORT);
    assert_eq!(failure.retry_after_ms(), Some(2500));
    assert_eq!(launches_under(&root), launches);
    let listing = stand.browsers.list();
    assert_eq!(listing.len(), 1);
    assert_eq!(
        (listing[0].subsystem, listing[0].tabs, listing[0].runs),
        (first, 0, 1)
    );
    // A browser for another profile is not started either.
    let other = stand
        .browsers
        .open(&profile("auditor"), Visibility::Background)
        .await
        .unwrap_err();
    assert_eq!(other.failure().unwrap().code(), &codes::MEMORY_SHORT);
    assert_eq!(launches_under(&root), launches);
    assert_eq!(stand.supervisor.list().len(), 1);

    // With the ceiling raised, the run goes, on the same browser.
    stand.browsers.set_memory_ceiling(MEMORY_CEILING);
    let browser = stand.open(&clerk).await;
    assert_eq!(browser.subsystem(), first);
    let tab = reads(&browser, &app).await;
    assert_eq!(*tab.page.status.borrow(), Status::Open);
    assert_eq!(launches_under(&root), launches);
    drop((tab, browser));
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_browser_past_its_memory_limit_is_replaced_between_two_runs_and_never_inside_one() {
    let app = TestApp::start().await.unwrap();
    // A limit any browser is past from its start.
    let settings = BrowserSettings {
        memory_limit: 1,
        ..test_support::settings()
    };
    let stand = Stand::new("memory-limit", settings, &[]);
    let clerk = profile("clerk");

    // A run reads, and holds its tab.
    let browser = stand.open(&clerk).await;
    let first = browser.subsystem();
    let tab = reads(&browser, &app).await;
    assert!(held(&browser, "one tab held, limit 1").await > 1);

    // Inside the run, the browser past its limit is kept: an open while
    // its tab is held hands out the same process, and the tab still reads.
    let inside = stand.open(&clerk).await;
    assert_eq!(inside.subsystem(), first);
    must(tab.locate(&textbox("Quantity"), soon()).await);
    drop((inside, tab, browser));

    // Between the two runs it is replaced: the next run has a new process,
    // the old one is gone from the supervisor's list, and the run reads.
    let browser = stand.open(&clerk).await;
    assert_ne!(browser.subsystem(), first);
    let running = stand.supervisor.list();
    assert_eq!(running.len(), 1, "{running:?}");
    assert_eq!(running[0].id, browser.subsystem());
    let tab = reads(&browser, &app).await;
    assert_eq!(stand.browsers.list()[0].runs, 1);
    drop((tab, browser));
    stand.end().await;
}
