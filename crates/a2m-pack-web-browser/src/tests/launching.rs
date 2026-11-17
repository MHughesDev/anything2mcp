//! Finding, starting, owning, and stopping a browser.

use a2m_core::Availability;

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn with_no_browser_the_failure_is_named_and_nothing_starts() {
    let mut settings = BrowserSettings::default();
    settings.program = Some(std::env::temp_dir().join("a2m-no-such-browser-program"));
    let stand = Stand::new("none", settings, &[]);
    assert_eq!(stand.browsers.availability(), Availability::ComponentMissing);
    let error = stand
        .browsers
        .open(&profile("clerk"), Visibility::Background)
        .await
        .unwrap_err();
    assert_eq!(error, BrowserError::LaunchFailed(LaunchReason::NotInstalled));
    assert_eq!(error.dispatch(), Dispatch::NotSent);
    let failure = error.failure().unwrap();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Busy, &codes::BROWSER_UNAVAILABLE)
    );
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_program_the_manifest_does_not_declare_is_not_started() {
    // The test's own program stands for one a person named in settings
    // and no manifest declares.
    let mut settings = BrowserSettings::default();
    settings.program = Some(std::env::current_exe().unwrap());
    let stand = Stand::new("undeclared", settings, &[]);
    let error = stand
        .browsers
        .open(&profile("clerk"), Visibility::Background)
        .await
        .unwrap_err();
    let failure = error.failure().unwrap();
    assert!(matches!(
        error,
        BrowserError::LaunchFailed(LaunchReason::Supervisor(_))
    ));
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Policy, &a2m_provider::codes::EFFECT_NOT_DECLARED)
    );
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_program_that_is_not_a_browser_is_closed_and_leaves_nothing() {
    // Declared, started, and not a browser: it answers nothing over the
    // pipe, so the engine does not own it and ends it.
    let program = std::env::current_exe().unwrap();
    let mut settings = BrowserSettings::default();
    settings.program = Some(program.clone());
    let stand = Stand::new("substituted", settings, &[&program]);
    let error = stand
        .browsers
        .open(&profile("clerk"), Visibility::Background)
        .await
        .unwrap_err();
    assert!(
        matches!(error, BrowserError::LaunchFailed(LaunchReason::NotOwned(_))),
        "{error}"
    );
    let failure = error.failure().unwrap();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::UnknownVariant, &codes::BROWSER_NOT_OWNED)
    );
    stand.end().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_data_directory_that_leads_outside_the_profile_root_is_refused() {
    let program = std::env::current_exe().unwrap();
    let mut settings = BrowserSettings::default();
    settings.program = Some(program.clone());
    let stand = Stand::new("link", settings, &[&program]);
    // A link planted where the profile's folder would be made.
    let outside = stand.folder.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, stand.folder.0.join("profiles").join("example")).unwrap();
    let error = stand
        .browsers
        .open(&profile("clerk"), Visibility::Background)
        .await
        .unwrap_err();
    assert!(
        matches!(error, BrowserError::LaunchFailed(LaunchReason::DataDirectory(_))),
        "{error}"
    );
    assert_eq!(
        error.failure().unwrap().code(),
        &a2m_provider::codes::PATH_NOT_ALLOWED
    );
    // Nothing was made outside, and nothing was started.
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_browser_whose_digest_is_not_a_known_one_is_closed() {
    let mut settings = test_support::settings();
    settings.known_digests = vec![Digest::of(b"another program")];
    let stand = Stand::new("digest", settings, &[]);
    let error = stand
        .browsers
        .open(&profile("clerk"), Visibility::Background)
        .await
        .unwrap_err();
    match &error {
        BrowserError::LaunchFailed(LaunchReason::NotOwned(why)) => {
            assert!(why.contains("digest"), "{why}");
        }
        // With no browser on the machine the test fails by the named failure.
        other => {
            must::<()>(Err(other.clone()));
        }
    }
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_identities_never_share_a_process_or_a_data_directory() {
    let stand = Stand::with_browser("isolation");
    let (clerk, manager) = (profile("clerk"), profile("manager"));
    let first = stand.open(&clerk).await;
    let second = stand.open(&manager).await;
    assert_ne!(first.subsystem(), second.subsystem());
    assert!(!first.product().is_empty());
    // The same profile again is the same browser, not a second one.
    let again = stand.open(&clerk).await;
    assert_eq!(first.subsystem(), again.subsystem());

    let running = stand.supervisor.list();
    assert_eq!(running.len(), 2);
    assert_ne!(running[0].process, running[1].process);
    assert_ne!(running[0].place, running[1].place);
    assert!(running.iter().all(|one| one.kind.as_str() == crate::BROWSER_KIND));
    assert!(running.iter().all(|one| one.link == "foreign"));

    // Each has its own data directory, inside the profile root.
    assert_ne!(clerk.place(), manager.place());
    let root = stand.folder.0.join("profiles");
    for key in [&clerk, &manager] {
        let directory = key.place().split('/').fold(root.clone(), |at, part| at.join(part));
        assert!(directory.is_dir(), "{}", directory.display());
        assert!(std::fs::read_dir(&directory).unwrap().count() > 0);
    }
    let listed = stand.browsers.list();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|one| one.health == BrowserHealth::Idle));

    // Closing one leaves the other running.
    assert!(stand.browsers.close(&clerk).await.complete);
    assert_eq!(stand.supervisor.list().len(), 1);
    assert_eq!(second.health(), BrowserHealth::Idle);
    assert_eq!(first.health(), BrowserHealth::Exited);
    stand.end().await;
}
