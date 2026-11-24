//! A sign-in through the application's own sign-in page (`form_login`,
//! decision 0245), carried out by the session keeper through the provider
//! the pack registers, against the test application's `/secure` pages,
//! whose control interface says who signed in and how many times anyone
//! tried.
//!
//! The keeper here is the product's: its rows in a ledger on disk, the
//! vault, the stored credentials, and the keeper's worker in the same
//! process. The probe is the target's declared read, run through the `ui`
//! driver under a lease of its own, as the engine runs it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use a2m_auth::{
    AskPerson, AuthEntry, CredentialRef, EntrySurfaces, ExpirySign, InProcessWork, Keeper,
    KeeperWorker, LeaseRequest, PersonAnswer, PersonAsk, ProbeOutcome, ProviderSet, Purpose,
    Returned, RunProbe, SessionKey, SessionLease, SessionRef, SessionStore, StoredCredentials,
    Wait, WeakKeeper,
};
use a2m_core::{
    CompiledTarget, DecisionId, Environment, Failure, FailureClass, Identity, PrincipalId, Redact,
    RunId, Secret, SessionState, TargetName, Timestamp, Value, Values, WorkspaceId,
};
use a2m_ledger::{Store, StoreConfig};
use a2m_pack_web_browser::test_support::{self, Screen, screen_required};
use a2m_provider::{Authority, Deadline, Posture, Sent, StepFailure, StepInput, StepOutcome};
use a2m_telemetry::ManualClock;
use a2m_vault::{BoxFuture, MemoryKeyStore, Vault};
use common::{Folder, Scene, Stand, compile, compile_changed, page_http_mode, ui_mode};
use example_app::auth::AuthSettings;
use example_app::faults::{Fault, FaultKind};
use example_app::harness::{PASSWORD, SECOND_FACTOR_CODE, SESSION_COOKIE, TestApp, USERNAME};

const USER_REF: &str = "vault://example/app/user";
const PASSWORD_REF: &str = "vault://example/app/password";

fn workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([7; 16])
}

fn person() -> PrincipalId {
    PrincipalId::from_bytes([1; 16])
}

fn key() -> SessionKey {
    SessionKey {
        workspace: workspace(),
        target: TargetName::new("example").unwrap(),
        environment: Environment::new("test").unwrap(),
        identity: Identity::Person(person()),
    }
}

/// What the person does when asked: answers with the code they read on
/// their own device, or is not there.
#[derive(Clone, Copy, PartialEq, Eq)]
enum There {
    WithCode,
    Away,
}

struct Person {
    there: Mutex<There>,
    asked: Mutex<Vec<PersonAsk>>,
}

impl AskPerson for Person {
    fn ask<'a>(&'a self, _session: &'a SessionRef, ask: PersonAsk) -> BoxFuture<'a, PersonAnswer> {
        Box::pin(async move {
            self.asked.lock().unwrap().push(ask.clone());
            match (ask, *self.there.lock().unwrap()) {
                (PersonAsk::EnterCode { .. }, There::WithCode) => PersonAnswer::Done(
                    Returned::new().with("code", Secret::new(SECOND_FACTOR_CODE)),
                ),
                (PersonAsk::Notify { .. }, There::WithCode) => PersonAnswer::Done(Returned::new()),
                _ => PersonAnswer::Unreachable,
            }
        })
    }
}

/// The target's probe: its declared read, through the `ui` driver, under a
/// lease of purpose `Probe`.
///
/// The keeper holds the probe, so the probe holds the scene and the keeper
/// weakly: a strong reference would be a cycle, the scene would never be
/// dropped, and its stand's folder, with the browser's profiles in it,
/// would be left under the temporary folder after every test; a strong
/// keeper would keep the keeper's store and worker after the world ended
/// (decision 0269, point 5).
#[derive(Default)]
struct Probe {
    world: OnceLock<(Weak<Scene>, WeakKeeper, Arc<AuthEntry>)>,
}

impl RunProbe for Probe {
    fn probe<'a>(&'a self, _session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome> {
        Box::pin(async move {
            let Some((scene, keeper, entry)) = self.world.get() else {
                return ProbeOutcome::Unknown;
            };
            let Some(scene) = scene.upgrade() else {
                return ProbeOutcome::Unknown;
            };
            let Some(keeper) = keeper.upgrade() else {
                return ProbeOutcome::Unknown;
            };
            let mut asked = request(entry, Wait::NoWait);
            asked.purpose = Purpose::Probe;
            let Ok(lease) = keeper.lease(asked).await else {
                return ProbeOutcome::Unknown;
            };
            match read(&scene, &lease).await {
                Ok(_) => ProbeOutcome::SignedIn,
                Err(failure) if failure.failure.class() == FailureClass::SessionExpired => {
                    ProbeOutcome::SignedOut
                }
                Err(_) => ProbeOutcome::Unknown,
            }
        })
    }
}

fn request(entry: &Arc<AuthEntry>, wait: Wait) -> LeaseRequest {
    LeaseRequest {
        key: key(),
        entry: Arc::clone(entry),
        run: RunId::from_bytes([3; 16]),
        decided: DecisionId::from_bytes([4; 16]),
        acting_for: Some(person()),
        wait,
        headroom: None,
        purpose: Purpose::Run,
    }
}

/// The probe's read, which is also the run's: who is signed in.
async fn read(scene: &Scene, lease: &SessionLease) -> Result<Values, StepFailure> {
    let (handle, mut prepared) = scene
        .try_prepare_leased(&ui_mode(), None, Posture::Live, Some(lease))
        .await?;
    let ran = scene
        .read(
            &handle,
            &mut prepared,
            "example_account_get",
            &Values::new(),
        )
        .await;
    let _ = handle.release(prepared, false).await;
    ran.map(|ran| ran.values)
}

/// The `page_http` read of an order's totals, from inside the signed-in
/// orders page, under the session's lease.
async fn totals(scene: &Scene, lease: &SessionLease) -> Result<Values, StepFailure> {
    let mode = page_http_mode();
    let (handle, mut prepared) = scene
        .try_prepare_leased(&mode, None, Posture::Live, Some(lease))
        .await?;
    let ran = scene
        .read_in(
            &mode,
            &handle,
            &mut prepared,
            "example_order_totals_get",
            &Values::new().with("order_id", "A-100"),
        )
        .await;
    let _ = handle.release(prepared, false).await;
    ran.map(|ran| ran.values)
}

/// The application, the pack, the keeper, and the person.
struct World {
    app: TestApp,
    home: Folder,
    scene: Arc<Scene>,
    keeper: Keeper,
    vault: Arc<Vault>,
    entry: Arc<AuthEntry>,
    person: Arc<Person>,
    telemetry_dir: PathBuf,
}

impl World {
    async fn new(name: &str, published: bool) -> Self {
        Self::storing(name, published, PASSWORD).await
    }

    /// The same, with `password` stored as the person's password.
    async fn storing(name: &str, published: bool, password: &str) -> Self {
        let app = TestApp::start().await.unwrap();
        Self::built(name, published, password, "secure", app).await
    }

    /// The same, for the fixture target `fixture` over `app`: each surface
    /// at the address `app` serves it from.
    async fn built(
        name: &str,
        published: bool,
        password: &str,
        fixture: &str,
        app: TestApp,
    ) -> Self {
        Self::shown(name, published, password, fixture, app, None).await
    }

    /// A sign-in the person carries out in a window on `screen`: from the
    /// published version, or from drafts the person was shown on their page.
    async fn window(name: &str, published: bool, fixture: &str, screen: &Screen) -> Self {
        let app = TestApp::start_with_identity_provider().await.unwrap();
        Self::shown(name, published, PASSWORD, fixture, app, Some(screen)).await
    }

    /// The same, with the browsers showing a window on `screen`.
    async fn shown(
        name: &str,
        published: bool,
        password: &str,
        fixture: &str,
        app: TestApp,
        screen: Option<&Screen>,
    ) -> Self {
        let home = Folder::new(&format!("{name}-home"));
        let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
            1_800_000_000_000,
        )));
        let ledger = Store::open(StoreConfig::in_dir(home.path()).with_clock(clock.clone()))
            .await
            .unwrap();
        let vault = Arc::new(Vault::new(
            Arc::new(MemoryKeyStore::new()),
            Arc::new(ledger.key_records()),
        ));
        ledger.attach(Arc::clone(&vault)).await.unwrap();
        // The pack's browsers redact with the vault's redactor, as the
        // product's do.
        let settings = screen.map_or_else(test_support::settings, Screen::settings);
        let stand = Stand::with_settings(
            name,
            vault.redactor() as Arc<dyn Redact>,
            Vec::new(),
            settings,
        );
        let mut compiled_target: CompiledTarget = compile(&stand.registry, fixture).compiled_target;
        let origin = app.url("").trim_end_matches('/').to_owned();
        for surface in &mut compiled_target.surfaces {
            match surface.name.as_str() {
                "app" => surface.address.clone_from(&origin),
                _ => {
                    surface.address = app
                        .provider_url("")
                        .unwrap_or_default()
                        .trim_end_matches('/')
                        .to_owned();
                }
            }
        }
        // The identity provider a window's sign-in passes through is where
        // the stand serves it.
        if let Some(provider) = app.provider_url("") {
            for setting in &mut compiled_target.auth[0].settings {
                if setting.name == "identity_providers" {
                    setting.value = provider.trim_end_matches('/').to_owned();
                }
            }
        }
        let surfaces = EntrySurfaces::of(
            &compiled_target,
            &compiled_target.auth[0],
            &key().environment,
        );
        let mut entry = entry_of(&compiled_target.auth[0]);
        entry.surfaces = match (published, screen.is_some()) {
            (true, _) => surfaces.published(),
            // A drafts' sign-in in a window: the person's page showed every
            // address before the press (decision 0248).
            (false, true) => surfaces.shown_to_person(),
            (false, false) => surfaces,
        };
        let entry = Arc::new(entry);
        let telemetry_dir = stand.folder.path().to_path_buf();
        let scene = Arc::new(Scene::over(stand, compiled_target, &origin));

        let store = SessionStore::new(ledger);
        let credentials = StoredCredentials::new(store.clone(), Arc::clone(&vault));
        let identity = Identity::Person(person());
        for (reference, value) in [(USER_REF, USERNAME), (PASSWORD_REF, password)] {
            credentials
                .put(
                    &workspace(),
                    &identity,
                    &CredentialRef::new(reference).unwrap(),
                    &Secret::new(value),
                )
                .await
                .unwrap();
        }
        let mut providers = ProviderSet::neutral();
        for provider in scene.stand.registry.auth_providers() {
            providers = providers.with(provider).unwrap();
        }
        let person = Arc::new(Person {
            there: Mutex::new(There::WithCode),
            asked: Mutex::new(Vec::new()),
        });
        let probe = Arc::new(Probe::default());
        let worker = Arc::new(KeeperWorker::new(
            providers.clone(),
            Arc::clone(&vault),
            Arc::new(credentials),
            clock.clone(),
        ));
        let work = Arc::new(InProcessWork::new(
            Arc::clone(&worker),
            person.clone(),
            probe.clone(),
        ));
        let keeper = Keeper::builder(store, providers, work, clock)
            .local_worker(worker)
            .build();
        assert!(
            probe
                .world
                .set((
                    Arc::downgrade(&scene),
                    keeper.downgrade(),
                    Arc::clone(&entry)
                ))
                .is_ok()
        );
        Self {
            app,
            home,
            scene,
            keeper,
            vault,
            entry,
            person,
            telemetry_dir,
        }
    }

    /// `session.login`, as the product's handler makes it: the keeper
    /// starts the sign-in, and the handler waits for how it ended.
    async fn sign_in(&self) -> Result<SessionState, Failure> {
        let ticket = self
            .keeper
            .sign_in(&key(), Arc::clone(&self.entry), Some(person()))
            .await?;
        let status = self
            .keeper
            .signed_in(ticket.session, Duration::from_secs(120))
            .await?;
        Ok(status.state)
    }

    async fn lease(&self) -> Result<SessionLease, Failure> {
        self.keeper.lease(request(&self.entry, Wait::NoWait)).await
    }

    async fn end(self) {
        self.keeper.idle().await;
        if let Some(browsers) = self.scene.stand.browsers.get() {
            let cleanup = browsers.shutdown().await;
            assert!(cleanup.complete, "{cleanup:?}");
        }
    }
}

/// The keeper's entry, field by field from the compiled target, as the engine
/// makes it (decision 0121); the compiled target's surfaces are added by the
/// caller.
fn entry_of(auth: &a2m_core::CompiledAuth) -> AuthEntry {
    let mut entry = AuthEntry::new(auth.provider.clone());
    for setting in &auth.settings {
        entry = entry.setting(&setting.name, &setting.value);
    }
    for credential in &auth.credentials {
        entry = entry.credential(&credential.role, credential.reference.clone());
    }
    entry.attempt_limit = auth.attempt_limit;
    entry.movable = auth.movable;
    entry
}

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

/// What every session cookie of the stand starts with; the rest is the
/// application's own salt and a number.
const COOKIE_PREFIX: &str = "example-session-";

#[tokio::test(flavor = "multi_thread")]
async fn a_right_password_signs_in_once_an_expired_session_is_renewed_and_no_secret_is_anywhere() {
    let world = World::new("signin-right", true).await;

    // The sign-in: one attempt, one session, at the application.
    assert_eq!(world.sign_in().await.unwrap(), SessionState::Valid);
    let report = world.app.auth_report().await.unwrap();
    assert_eq!((report.sign_in_attempts, report.sign_ins), (1, 1));

    // A read under the session: the application says who is signed in.
    // The username is a stored credential, so its echo is masked.
    let lease = world.lease().await.unwrap();
    let values = read(&world.scene, &lease).await.unwrap();
    let shown = format!("{values:?}");
    assert!(shown.contains("Signed in as [redacted]."), "{shown}");

    // The application ends the session. The read meets the sign-in page,
    // which the target declares as a sign of expiry: a named stop, not an
    // unexpected page, and nothing is signed in by the driver.
    world.app.expire_sessions().await.unwrap();
    let stopped = read(&world.scene, &lease).await.unwrap_err();
    assert_eq!(stopped.failure.class(), FailureClass::SessionExpired);
    assert_eq!(stopped.failure.code().as_str(), "needs_login");
    assert_eq!(world.app.auth_report().await.unwrap().sign_in_attempts, 1);

    // The engine tells the keeper; the keeper probes, and signs in again
    // with the stored credential, once.
    world.keeper.report(&lease, ExpirySign::Refused).await;
    world.keeper.idle().await;
    let report = world.app.auth_report().await.unwrap();
    assert_eq!((report.sign_in_attempts, report.sign_ins), (2, 2));
    let renewed = world.lease().await.unwrap();
    assert!(renewed.epoch() > lease.epoch());
    let values = read(&world.scene, &renewed).await.unwrap();
    assert!(format!("{values:?}").contains("Signed in as [redacted]."));
    // The old lease places nothing any more.
    assert!(read(&world.scene, &lease).await.is_err());

    // The password, the username, and every session cookie: in no value,
    // no failure, no timeline, and no file of the engine's home, the
    // browser's data directories and the logs included.
    let mut planted = vec![PASSWORD.to_owned(), USERNAME.to_owned()];
    planted.push(COOKIE_PREFIX.to_owned());
    let _ = SESSION_COOKIE;
    let timeline = world.keeper.timeline(renewed.session()).await.unwrap();
    let told = format!("{values:?} {stopped:?} {timeline:?} {renewed:?}");
    for secret in &planted {
        assert!(!told.contains(secret.as_str()), "{secret} in {told}");
        for folder in [world.home.path(), world.telemetry_dir.as_path()] {
            let written = files_holding(folder, secret);
            assert!(written.is_empty(), "{secret} is written in {written:?}");
        }
    }
    // The person was asked for nothing, and never sent to a link.
    assert!(world.person.asked.lock().unwrap().is_empty());
    let _ = &world.vault;
    world.end().await;
}

/// One step of the run's read, through the `ui` driver, as the engine
/// hands it over: its arguments, a deadline, and no authority to write.
async fn one_step(
    scene: &Scene,
    handle: &a2m_provider::DriverHandle,
    prepared: &mut a2m_provider::Prepared,
    step: &a2m_core::Step,
) -> Result<StepOutcome, StepFailure> {
    let args = scene.args(step, &Values::new());
    let input = StepInput::new(&args, Deadline::after(Duration::from_secs(60)));
    handle.act(prepared, step, &input, Authority::Read).await
}

/// Waits until the profile's browser is seen to have exited: the engine's
/// link to it is closed.
async fn exited(browsers: &a2m_pack_web_browser::Browsers) {
    let gone = async {
        while !browsers
            .list()
            .iter()
            .all(|open| open.health == a2m_pack_web_browser::BrowserHealth::Exited)
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(60), gone)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_browser_lost_under_a_read_is_started_again_from_its_profile_and_the_read_completes_signed_in()
 {
    let world = World::new("signin-lost", true).await;
    assert_eq!(world.sign_in().await.unwrap(), SessionState::Valid);
    let lease = world.lease().await.unwrap();
    let scene = &world.scene;
    let supervisor = scene.stand.supervisor.clone();
    let browsers = scene.stand.browsers.get().unwrap();
    let (handle, mut prepared) = scene
        .try_prepare_leased(&ui_mode(), None, Posture::Live, Some(&lease))
        .await
        .unwrap();
    let open = browsers.list();
    assert_eq!(open.len(), 1);
    let profile = open[0].profile.clone();
    let steps = scene.binding("example_account_get").act.clone();
    let kinds: Vec<&str> = steps.iter().map(|step| step.kind.as_str()).collect();
    assert_eq!(kinds, ["goto", "wait", "read"]);
    assert!(steps.iter().all(|step| !step.commits));

    // The first loss: the browser is killed while the page of the read's
    // `goto` is loading, which the application holds. The fault is used up
    // when the request arrives: the stand, not a guess at timing, says the
    // page is on its way.
    world
        .app
        .set_fault(&Fault::once(FaultKind::Delay { ms: 600_000 }).on_paths("/secure/orders"))
        .await
        .unwrap();
    let kill = async {
        while !world.app.faults().await.unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let running = supervisor.list();
        assert_eq!(running.len(), 1);
        common::kill(running[0].process);
        running[0].process
    };
    let (lost, first) = tokio::join!(one_step(scene, &handle, &mut prepared, &steps[0]), async {
        tokio::time::timeout(Duration::from_secs(60), kill)
            .await
            .unwrap()
    });
    let lost = lost.unwrap_err();
    assert_eq!(
        (lost.failure.class(), lost.failure.code().as_str()),
        (FailureClass::Transient, "connection_lost")
    );
    // The engine tries a `transient` step that commits nothing again. The
    // `goto` is done in a new browser of the same profile, and the killed
    // one is reaped.
    one_step(scene, &handle, &mut prepared, &steps[0])
        .await
        .unwrap();
    let running = supervisor.list();
    assert_eq!(running.len(), 1);
    let second = running[0].process;
    assert_ne!(second, first);
    one_step(scene, &handle, &mut prepared, &steps[1])
        .await
        .unwrap();

    // The second loss: between the read's `wait` and its `read`. The read
    // is made in a third browser, on its page reached again by the `goto`
    // and the `wait` done again in a tab that can only read.
    common::kill(second);
    exited(&browsers).await;
    let read = one_step(scene, &handle, &mut prepared, &steps[2])
        .await
        .unwrap();
    let StepOutcome::Done(observed) = read else {
        panic!("the read found its record absent");
    };
    let shown = format!("{:?}", observed.values);
    assert!(shown.contains("Signed in as [redacted]."), "{shown}");

    // The sign-in survived both: the application counts the one sign-in,
    // the keeper made no other, and the session's state was placed again
    // from the run's lease into each new browser of the same profile.
    world.keeper.idle().await;
    let report = world.app.auth_report().await.unwrap();
    assert_eq!((report.sign_in_attempts, report.sign_ins), (1, 1));
    let open = browsers.list();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].profile, profile);
    let running = supervisor.list();
    assert_eq!(running.len(), 1);
    assert!(![first, second].contains(&running[0].process));
    assert!(world.person.asked.lock().unwrap().is_empty());

    // And no browser process is left.
    assert!(handle.release(prepared, false).await.complete);
    world.app.clear_faults().await.unwrap();
    world.end().await;
    assert!(supervisor.list().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_password_is_refused_after_one_attempt_and_never_tried_again() {
    const WRONG: &str = "secret-for-tests-not-the-password";
    let world = World::storing("signin-wrong", true, WRONG).await;
    let refused = world.sign_in().await.unwrap_err();
    assert_eq!(refused.class(), FailureClass::SessionExpired);
    assert_eq!(refused.code().as_str(), "credential_refused");
    let report = world.app.auth_report().await.unwrap();
    assert_eq!(
        (
            report.sign_in_attempts,
            report.failed_sign_ins,
            report.sign_ins
        ),
        (1, 1, 0)
    );

    // Runs ask for the session: the keeper does not try the credential
    // again, however often it is asked.
    for _ in 0..3 {
        let asked = world.lease().await.unwrap_err();
        assert_eq!(asked.class(), FailureClass::SessionExpired);
    }
    // Nor when the person asks for the sign-in again, twice: the provider
    // refuses the refused credential before any page is opened.
    for _ in 0..2 {
        let again = world.sign_in().await.unwrap_err();
        assert_eq!(again.class(), FailureClass::SessionExpired);
        assert_eq!(again.code().as_str(), "credential_refused");
    }
    world.keeper.idle().await;
    assert_eq!(world.app.auth_report().await.unwrap().sign_in_attempts, 1);
    assert!(!format!("{refused:?}").contains(WRONG));
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_factor_is_answered_by_the_person_and_the_code_is_kept_nowhere() {
    let world = World::new("signin-code", true).await;
    world
        .app
        .set_auth_settings(&AuthSettings {
            second_factor: true,
            ..AuthSettings::default()
        })
        .await
        .unwrap();
    assert_eq!(world.sign_in().await.unwrap(), SessionState::Valid);
    let report = world.app.auth_report().await.unwrap();
    assert_eq!((report.sign_ins, report.failed_codes), (1, 0));
    // Asked once, for a code, and for nothing else.
    let asked = world.person.asked.lock().unwrap().clone();
    assert!(
        matches!(asked.as_slice(), [PersonAsk::EnterCode { .. }]),
        "{asked:?}"
    );
    let lease = world.lease().await.unwrap();
    let values = read(&world.scene, &lease).await.unwrap();
    assert!(format!("{values:?}").contains("Signed in as [redacted]."));
    world.keeper.idle().await;
    for secret in [SECOND_FACTOR_CODE, PASSWORD, COOKIE_PREFIX] {
        for folder in [world.home.path(), world.telemetry_dir.as_path()] {
            let written = files_holding(folder, secret);
            assert!(written.is_empty(), "{secret} is written in {written:?}");
        }
    }
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_factor_asked_with_nobody_there_is_a_named_stop() {
    let world = World::new("signin-away", true).await;
    world
        .app
        .set_auth_settings(&AuthSettings {
            second_factor: true,
            ..AuthSettings::default()
        })
        .await
        .unwrap();
    *world.person.there.lock().unwrap() = There::Away;
    let stopped = world.sign_in().await.unwrap_err();
    assert_eq!(stopped.class(), FailureClass::SessionExpired);
    assert_eq!(stopped.code().as_str(), "needs_mfa");
    let report = world.app.auth_report().await.unwrap();
    assert_eq!(
        (
            report.sign_in_attempts,
            report.sign_ins,
            report.failed_codes
        ),
        (1, 0, 0)
    );
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_locked_account_is_a_named_block_and_a_page_with_no_declared_sign_is_a_stop() {
    let world = World::new("signin-signs", true).await;
    // The application answers the sign-in with a page that is none of the
    // declared signs.
    world
        .app
        .set_fault(
            &Fault::once(FaultKind::RedirectAfterCommit {
                location: "/escapes/quiet".to_owned(),
            })
            .on_paths("/secure/sign-in"),
        )
        .await
        .unwrap();
    let unknown = world.sign_in().await.unwrap_err();
    assert_eq!(unknown.class(), FailureClass::UnknownVariant);
    assert_eq!(unknown.code().as_str(), "unexpected_page");
    assert_eq!(world.app.auth_report().await.unwrap().sign_in_attempts, 1);
    world.end().await;

    // An account the application has locked: the block is named, and
    // nothing gets past it.
    let world = World::new("signin-locked", true).await;
    for _ in 0..5 {
        let _ = world
            .app
            .http()
            .post(world.app.url("/secure/sign-in"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body("username=example-user&password=wrong")
            .send()
            .await
            .unwrap();
    }
    assert!(world.app.auth_report().await.unwrap().locked);
    let blocked = world.sign_in().await.unwrap_err();
    assert_eq!(blocked.class(), FailureClass::EdgeBlocked);
    assert_eq!(blocked.code().as_str(), "sign_in_blocked");
    let report = world.app.auth_report().await.unwrap();
    assert_eq!((report.sign_in_attempts, report.sign_ins), (6, 0));
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_declared_by_drafts_fills_in_and_sends_nothing() {
    let world = World::new("signin-draft", false).await;
    let before = world.app.seen().await.unwrap();
    let refused = world.sign_in().await.unwrap_err();
    assert_eq!(refused.class(), FailureClass::SessionExpired);
    assert_eq!(refused.code().as_str(), "needs_login");
    // The application heard nothing: no page was opened, no field filled.
    assert_eq!(world.app.seen().await.unwrap(), before);
    assert_eq!(world.app.auth_report().await.unwrap().sign_in_attempts, 0);
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_request_stops_by_name_at_each_declared_sign_of_expiry_and_reads_again_once_renewed()
{
    let world = World::new("signin-page-http", true).await;
    assert_eq!(world.sign_in().await.unwrap(), SessionState::Valid);
    let stored = world.app.stored_order("A-100").await.unwrap().unwrap();
    let quantity: u32 = stored.items.values().map(|item| item.quantity).sum();
    let expected = Value::Int(i64::from(quantity));

    // Signed in: the page's own session reads the totals.
    let lease = world.lease().await.unwrap();
    let values = totals(&world.scene, &lease).await.unwrap();
    assert_eq!(values.get("total_quantity"), Some(&expected));

    // The application ends the session. The page is answered with the
    // sign-in page, which the target declares as a sign: a named stop
    // before anything is sent from it.
    world.app.expire_sessions().await.unwrap();
    let refused_before = world.app.auth_report().await.unwrap().refused_sessions;
    let stopped = totals(&world.scene, &lease).await.unwrap_err();
    assert_eq!(stopped.failure.class(), FailureClass::SessionExpired);
    assert_eq!(stopped.failure.code().as_str(), "needs_login");
    assert_eq!(stopped.sent, Sent::NotSent);
    assert!(stopped.failure.message().contains("sign_in"), "{stopped:?}");
    let report = world.app.auth_report().await.unwrap();
    // The page was refused; the request was never made.
    assert_eq!(report.refused_sessions, refused_before + 1);
    assert_eq!(report.sign_in_attempts, 1);

    // The keeper renews once; the read succeeds under the new lease.
    world.keeper.report(&lease, ExpirySign::Refused).await;
    world.keeper.idle().await;
    let report = world.app.auth_report().await.unwrap();
    assert_eq!((report.sign_in_attempts, report.sign_ins), (2, 2));
    let renewed = world.lease().await.unwrap();
    let values = totals(&world.scene, &renewed).await.unwrap();
    assert_eq!(values.get("total_quantity"), Some(&expected));

    // The session ends after the page loaded and before its request: the
    // application's own guard answers the request with 401, which the
    // target declares as a sign. A named stop, and the read was answered.
    world
        .app
        .set_fault(&Fault::once(FaultKind::ExpireSessions).on_paths("/secure/api/"))
        .await
        .unwrap();
    let stopped = totals(&world.scene, &renewed).await.unwrap_err();
    assert_eq!(stopped.failure.class(), FailureClass::SessionExpired);
    assert_eq!(stopped.failure.code().as_str(), "needs_login");
    assert!(stopped.failure.message().contains("401"), "{stopped:?}");
    world.keeper.report(&renewed, ExpirySign::Refused).await;
    world.keeper.idle().await;
    let report = world.app.auth_report().await.unwrap();
    assert_eq!((report.sign_in_attempts, report.sign_ins), (3, 3));
    let third = world.lease().await.unwrap();
    let values = totals(&world.scene, &third).await.unwrap();
    assert_eq!(values.get("total_quantity"), Some(&expected));

    // A sign nobody declared is never guessed: the same target without
    // the status among its signs reads the same 401 as an answer no entry
    // declares, and the keeper is not told the session ended.
    let origin = world.app.url("").trim_end_matches('/').to_owned();
    let stand = Stand::new(
        "signin-page-http-undeclared",
        world.vault.redactor() as Arc<dyn Redact>,
    );
    let mut compiled_target = compile_changed(&stand.registry, "secure", "auth.yaml", |text| {
        text.replace("      - {status: 401}\n", "")
    })
    .unwrap()
    .compiled_target;
    compiled_target.surfaces[0].address.clone_from(&origin);
    let undeclared = Scene::over(stand, compiled_target, &origin);
    world
        .app
        .set_fault(&Fault::once(FaultKind::ExpireSessions).on_paths("/secure/api/"))
        .await
        .unwrap();
    let stopped = totals(&undeclared, &third).await.unwrap_err();
    assert_eq!(stopped.failure.class(), FailureClass::UnknownVariant);
    assert_eq!(stopped.failure.code().as_str(), "unexpected_status");
    undeclared.stand.end().await;

    // A sign of a form the pack does not know, a redirect among them, or a
    // status that is not a client error, stops the run before anything is
    // sent.
    for sign in ["{redirect: sign_in}", "{status: 302}"] {
        let stand = Stand::new(
            "signin-page-http-unknown",
            world.vault.redactor() as Arc<dyn Redact>,
        );
        let mut compiled_target = compile_changed(&stand.registry, "secure", "auth.yaml", |text| {
            text.replace("{status: 401}", sign)
        })
        .unwrap()
        .compiled_target;
        compiled_target.surfaces[0].address.clone_from(&origin);
        let unknown = Scene::over(stand, compiled_target, &origin);
        let refused = totals(&unknown, &third).await.unwrap_err();
        assert_eq!(
            refused.failure.code().as_str(),
            "step_not_usable",
            "{sign}: {:?}",
            refused.failure
        );
        assert_eq!(refused.sent, Sent::NotSent);
        unknown.stand.end().await;
    }
    world.end().await;
}

/// A screen for a window, or `None` when this machine has none and none is
/// required: then the test says it did not run.
fn screen() -> Option<Screen> {
    match Screen::start() {
        Ok(screen) => Some(screen),
        Err(why) if !screen_required() => {
            eprintln!("NOT RUN, no screen on this machine: {why}");
            None
        }
        Err(why) => panic!("no screen, and A2M_REQUIRE_DESKTOP=1 says one must be had: {why}"),
    }
}

/// Every file under the engine's home and the logs holds none of these.
fn written_nowhere(world: &World, secrets: &[&str]) {
    for secret in secrets {
        for folder in [world.home.path(), world.telemetry_dir.as_path()] {
            let written = files_holding(folder, secret);
            assert!(written.is_empty(), "{secret} is written in {written:?}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_signs_in_in_a_window_from_drafts_and_the_engine_types_and_reads_nothing() {
    let Some(screen) = screen() else {
        return;
    };
    // Nobody there: no window is opened, and the application hears nothing.
    let world = World::window("window-away", false, "window", &screen).await;
    *world.person.there.lock().unwrap() = There::Away;
    let stopped = world.sign_in().await.unwrap_err();
    assert_eq!(stopped.class(), FailureClass::SessionExpired);
    assert_eq!(stopped.code().as_str(), "needs_login");
    assert_eq!(world.app.auth_report().await.unwrap().sign_in_attempts, 0);
    assert!(world.scene.stand.supervisor.list().is_empty());
    world.end().await;

    // The person signs in on the application's own page, which a script of
    // the stand types into, as a person would: the engine sent no key.
    let world = World::window("window-own", false, "window", &screen).await;
    world
        .app
        .set_auth_settings(&AuthSettings {
            person_at_the_window: true,
            ..AuthSettings::default()
        })
        .await
        .unwrap();
    assert_eq!(world.sign_in().await.unwrap(), SessionState::Valid);
    let report = world.app.auth_report().await.unwrap();
    assert_eq!((report.sign_in_attempts, report.sign_ins), (1, 1));
    // The person was told once, and asked for nothing.
    let asked = world.person.asked.lock().unwrap().clone();
    assert!(
        matches!(asked.as_slice(), [PersonAsk::Notify { .. }]),
        "{asked:?}"
    );
    // The window is gone; a run reads in the background with the state.
    assert!(world.scene.stand.supervisor.list().is_empty());
    let lease = world.lease().await.unwrap();
    let values = read(&world.scene, &lease).await.unwrap();
    assert!(format!("{values:?}").contains("Signed in as"), "{values:?}");
    let timeline = world.keeper.timeline(lease.session()).await.unwrap();
    let told = format!("{values:?} {timeline:?} {lease:?}");
    assert!(!told.contains(PASSWORD), "{told}");
    assert!(!told.contains(COOKIE_PREFIX), "{told}");
    world.keeper.idle().await;
    written_nowhere(&world, &[PASSWORD, COOKIE_PREFIX]);
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn single_sign_on_crosses_to_the_identity_provider_and_back_and_a_stray_origin_is_a_named_stop()
 {
    let Some(screen) = screen() else {
        return;
    };
    let world = World::window("window-sso", true, "window_sso", &screen).await;
    world
        .app
        .set_auth_settings(&AuthSettings {
            person_at_the_window: true,
            ..AuthSettings::default()
        })
        .await
        .unwrap();
    assert_eq!(world.sign_in().await.unwrap(), SessionState::Valid);
    let report = world.app.auth_report().await.unwrap();
    // The person signed in at the identity provider, once; its one-use
    // code came back to the application once; nothing was refused.
    assert_eq!(report.identity_provider_sign_ins, 1);
    assert_eq!((report.codes_used, report.returns_refused), (1, 0));
    assert_eq!(report.sign_ins, 1);
    let lease = world.lease().await.unwrap();
    let values = read(&world.scene, &lease).await.unwrap();
    assert!(format!("{values:?}").contains("Signed in as"), "{values:?}");
    world.keeper.idle().await;
    // The identity provider's own cookie is not the session's: only the
    // application's origin's cookies were taken.
    written_nowhere(
        &world,
        &[PASSWORD, COOKIE_PREFIX, "example-provider-session-"],
    );
    let timeline = world.keeper.timeline(lease.session()).await.unwrap();
    assert!(!format!("{timeline:?}").contains("provider-session"));
    world.end().await;

    // The identity provider sends the window to an origin nobody declared:
    // the browser ends the move before it leaves, and the sign-in stops by
    // name.
    let world = World::window("window-stray", true, "window_sso", &screen).await;
    world
        .app
        .set_auth_settings(&AuthSettings {
            person_at_the_window: true,
            identity_provider_strays: true,
            ..AuthSettings::default()
        })
        .await
        .unwrap();
    let stopped = world.sign_in().await.unwrap_err();
    assert_eq!(stopped.class(), FailureClass::Policy, "{stopped:?}");
    assert_eq!(stopped.code().as_str(), "surface_not_declared");
    let report = world.app.auth_report().await.unwrap();
    assert_eq!(report.identity_provider_sign_ins, 1);
    assert_eq!((report.strays_arrived, report.sign_ins), (0, 0));
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_in_a_browser_refuses_an_entry_that_also_covers_a_surface_of_another_kind() {
    let stand = Stand::new("signin-kinds", Arc::new(a2m_core::NoSecrets));
    for fixture in ["secure", "window"] {
        let compiled_target: CompiledTarget = compile(&stand.registry, fixture).compiled_target;
        let mut entry = entry_of(&compiled_target.auth[0]);
        entry.surfaces = EntrySurfaces::of(
            &compiled_target,
            &compiled_target.auth[0],
            &key().environment,
        )
        .published();
        let provider = stand
            .registry
            .auth_providers()
            .into_iter()
            .find(|provider| provider.name() == entry.provider)
            .unwrap();
        provider.check(&entry).unwrap();
        // The same entry, covering an `http` surface too: the session's
        // cookies would go to that surface's driver as a header.
        let mut other = entry.surfaces.surfaces[0].clone();
        other.name = a2m_core::SurfaceName::new("api").unwrap();
        other.kind = a2m_core::SurfaceKind::new("http").unwrap();
        entry.surfaces.surfaces.push(other);
        let refused = provider.check(&entry).unwrap_err();
        assert_eq!(
            refused.code().as_str(),
            "auth_settings_invalid",
            "{fixture}"
        );
        assert!(refused.message().contains("`api`"), "{refused}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_password_goes_into_the_checked_field_alone_and_never_into_a_page_with_a_foreign_frame() {
    // The page moves the focus from the password field to a note field
    // whenever it takes it: the password is set on the checked field by
    // its node, and the note receives nothing.
    let world = World::new("signin-focus", true).await;
    world
        .app
        .set_auth_settings(&AuthSettings {
            focus_thief: true,
            ..AuthSettings::default()
        })
        .await
        .unwrap();
    assert_eq!(world.sign_in().await.unwrap(), SessionState::Valid);
    let report = world.app.auth_report().await.unwrap();
    assert_eq!(report.typed_elsewhere, 0);
    assert_eq!((report.sign_in_attempts, report.sign_ins), (1, 1));
    world.end().await;

    // A frame of another origin on the sign-in page: a named stop before
    // anything is entered, and nothing is sent.
    let app = TestApp::start_with_identity_provider().await.unwrap();
    let world = World::built("signin-frame", true, PASSWORD, "secure", app).await;
    world
        .app
        .set_auth_settings(&AuthSettings {
            foreign_frame: true,
            ..AuthSettings::default()
        })
        .await
        .unwrap();
    let stopped = world.sign_in().await.unwrap_err();
    assert_eq!(stopped.class(), FailureClass::UnknownVariant, "{stopped:?}");
    assert_eq!(stopped.code().as_str(), "unexpected_page");
    assert!(stopped.message().contains("frame"), "{stopped}");
    assert_eq!(world.app.auth_report().await.unwrap().sign_in_attempts, 0);
    world.end().await;
}
