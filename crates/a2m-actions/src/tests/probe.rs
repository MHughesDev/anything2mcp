//! Package 2.4: the keeper's probe, run through the product. The probe of a
//! session is its auth file entry's `probe` read, run by the runner loop as
//! an ordinary read through the engine under a lease of purpose `Probe` (spec
//! section 215), with the inputs the compiled target gives it (decision
//! 0121). The keeper's clock is the kernel's manual one.
//!
//! The test pack's driver stands for the system: with its `session_ended`
//! switch on, every step is refused as a system refuses a session that has
//! ended. The sign-in is the test provider `example_login`'s, which needs
//! the person and counts each sign-in it makes, so that a test sees that
//! none was made without the person.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use a2m_auth::{SessionKey, SignInBy};
use a2m_core::{CompiledProbe, Environment, Identity, SessionState, TargetName, Values};
use serde_json::json;

use super::kernel::engine::{self, SECURE_GET};
use super::kernel::{Options, World, world};

const MINUTE: Duration = Duration::from_secs(60);

// ---- a provider that needs the person -------------------------------------

const LOGIN_MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "example_logins"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[points]
auth_provider = ["example_login"]

[effects]
sessions = ["json_app"]
"#;

/// A pack with the one auth provider the hand-built compiled target names.
pub(super) struct LoginPack(pub(super) Arc<AtomicU32>);

impl a2m_provider::Pack for LoginPack {
    fn manifest(&self) -> &'static str {
        LOGIN_MANIFEST
    }

    fn register(
        &self,
        _cx: &a2m_provider::PackContext,
        to: &mut a2m_provider::Registrar<'_>,
    ) -> Result<(), a2m_core::Failure> {
        to.auth_provider(Arc::new(LoginProvider(Arc::clone(&self.0))))
    }
}

/// Signs in only when its person does, and counts every sign-in. It cannot
/// renew: a session that ended waits for its person.
struct LoginProvider(Arc<AtomicU32>);

impl a2m_auth::AuthProvider for LoginProvider {
    fn name(&self) -> a2m_auth::ProviderName {
        a2m_auth::ProviderName::fixed("example_login")
    }

    fn needs(&self) -> a2m_auth::ProviderNeeds {
        a2m_auth::ProviderNeeds {
            person: true,
            silent_renewal: false,
            light_prompt: false,
            movable: false,
            confirms: true,
        }
    }

    fn check(&self, _entry: &a2m_auth::AuthEntry) -> Result<(), a2m_core::Failure> {
        Ok(())
    }

    fn sign_in<'a>(
        &'a self,
        _cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<a2m_auth::Material, a2m_core::Failure>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(a2m_auth::Material::new(self.name()).with_plain("signed_in", "yes"))
        })
    }

    fn is_signed_in<'a>(
        &'a self,
        _material: &'a a2m_auth::Material,
        _now: a2m_core::Timestamp,
    ) -> a2m_auth::BoxFuture<'a, a2m_auth::SignedIn> {
        Box::pin(async { a2m_auth::SignedIn::Yes { until: None } })
    }

    fn detect_expiry(&self, _sign: &a2m_auth::RawSign) -> Option<a2m_auth::ExpirySign> {
        None
    }

    fn refresh<'a>(
        &'a self,
        _material: &'a a2m_auth::Material,
        _cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<a2m_auth::Material, a2m_core::Failure>> {
        Box::pin(async {
            Err(a2m_core::Failure::new(
                a2m_core::FailureClass::SessionExpired,
                a2m_auth::codes::NEEDS_LOGIN,
                "The test provider does not renew.",
                "Sign in again.",
            ))
        })
    }

    fn sign_out<'a>(
        &'a self,
        _material: &'a a2m_auth::Material,
        _cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<(), a2m_core::Failure>> {
        Box::pin(async { Ok(()) })
    }

    fn inject(
        &self,
        _material: &a2m_auth::Material,
        _sink: &mut dyn a2m_auth::CredentialSink,
    ) -> Result<(), a2m_core::Failure> {
        Ok(())
    }
}

// ---- the world -------------------------------------------------------------

/// The hand-built compiled target, whose signed-in surface is checked with
/// the read `example_secure_order_get` of order `A-100`, with the idle
/// timeout given.
fn probed_compiled_target(address: &str, idle: Option<Duration>) -> a2m_core::CompiledTarget {
    let mut compiled_target = engine::compiled_target(address);
    let operation = compiled_target
        .operations
        .iter()
        .position(|operation| operation.name.as_str() == SECURE_GET)
        .and_then(|at| u32::try_from(at).ok())
        .map(a2m_core::OperationIndex::new)
        .unwrap();
    compiled_target.auth[0].probe = CompiledProbe::Operation {
        operation,
        inputs: Values::new().with("order_id", "A-100"),
    };
    compiled_target.auth[0].idle_ms = idle.map(|idle| u64::try_from(idle.as_millis()).unwrap());
    compiled_target
}

impl World {
    /// A kernel whose calls do not wait for a sign-in and whose sessions
    /// are kept alive for an hour after their last use, with the runner
    /// loop started and `compiled_target` published.
    async fn probed(idle: Option<Duration>) -> Self {
        let mut options = Options::with_user_settings(
            "[sessions]\nlogin_wait_minutes = 0\nkeep_alive_hours = 1\n",
        );
        options.login = true;
        let world = world(options).await;
        world.kernel.start_runner_loop(4).await.unwrap();
        world
            .kernel
            .publish_compiled_target(&probed_compiled_target(&world.app.url(""), idle))
            .await
            .unwrap();
        world
    }

    fn session_key(&self) -> SessionKey {
        SessionKey {
            workspace: self.caller.principal.workspace(),
            target: TargetName::new("example").unwrap(),
            environment: Environment::new("test").unwrap(),
            identity: Identity::Person(self.caller.principal.principal().id),
        }
    }

    /// The person signs in themself, once.
    async fn person_signs_in(&self) {
        let compiled_target = probed_compiled_target(&self.app.url(""), None);
        let entry = crate::control::entry_of(
            &compiled_target,
            &compiled_target.auth[0],
            &Environment::new("test").unwrap(),
            true,
        );
        let keeper = self.kernel.keeper().unwrap();
        keeper
            .sign_in_by(
                &self.session_key(),
                Arc::new(entry),
                Some(self.caller.principal.principal().id),
                SignInBy::Person,
            )
            .await
            .unwrap();
        keeper.idle().await;
        assert_eq!(self.session_state().await, Some(SessionState::Valid));
    }

    async fn session_state(&self) -> Option<SessionState> {
        let key = self.session_key();
        self.kernel
            .keeper()
            .unwrap()
            .status(&key.workspace, Some(&key.identity))
            .await
            .unwrap()
            .into_iter()
            .find(|session| session.target == key.target)
            .map(|session| session.state)
    }

    /// Calls the signed-in read and gives the status its run rests in.
    async fn secure_read(&self) -> String {
        let response = self
            .call(
                "tool.call",
                json!({"tool": SECURE_GET, "input": {"order_id": "A-100"}}),
            )
            .await;
        let result = response
            .result
            .unwrap_or_else(|| panic!("no result: {:?}", response.error));
        result["status"].as_str().unwrap().to_owned()
    }

    /// How many reads of an order reached the system: the calls' and the
    /// probes'.
    fn reads(&self) -> usize {
        self.hooks.count("get_order")
    }

    /// One pass of the kernel's timers, and the end of whatever keeper work
    /// it started.
    async fn tend_once(&self) {
        self.kernel.tend().await;
        self.kernel.keeper().unwrap().idle().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_probe_that_finds_the_session_ended_moves_it_to_needs_person_and_the_next_call_returns_needs_login()
 {
    let world = World::probed(None).await;
    world.person_signs_in().await;
    assert_eq!(world.secure_read().await, "completed");
    assert_eq!(world.reads(), 1);

    // The system ends the session. Nothing has been sent to it since, so
    // the session is still valid as the keeper knows it.
    world.hooks.session_ended.store(true, Ordering::SeqCst);
    assert_eq!(world.session_state().await, Some(SessionState::Valid));

    // The probe is due a quarter of an hour after the last use. It runs as
    // a read through the engine, finds the session ended, and the keeper,
    // which cannot renew without the person, waits for them.
    world.clock.advance(15 * MINUTE);
    world.tend_once().await;
    assert!(world.reads() >= 2, "no probe reached the system");
    assert_eq!(world.session_state().await, Some(SessionState::NeedsPerson));

    // The next call returns `needs_login` from the session's state alone:
    // the system would answer it now, and no step is sent to it.
    world.hooks.session_ended.store(false, Ordering::SeqCst);
    let reads = world.reads();
    assert_eq!(world.secure_read().await, "needs_login");
    assert_eq!(world.reads(), reads);

    // Only the person's own sign-in was ever made.
    assert_eq!(world.logins.load(Ordering::SeqCst), 1);
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_probe_runs_once_per_half_idle_and_stops_after_the_keep_alive_window() {
    // The system signs a session out after twenty idle minutes: the probe
    // is due every ten.
    let world = World::probed(Some(20 * MINUTE)).await;
    world.person_signs_in().await;
    assert_eq!(world.secure_read().await, "completed");
    assert_eq!(world.reads(), 1);

    // Not before ten minutes.
    world.clock.advance(10 * MINUTE - Duration::from_secs(1));
    world.tend_once().await;
    assert_eq!(world.reads(), 1);

    // At ten minutes, once, however often the timers run.
    world.clock.advance(Duration::from_secs(1));
    world.tend_once().await;
    world.tend_once().await;
    assert_eq!(world.reads(), 2);
    assert_eq!(world.session_state().await, Some(SessionState::Valid));

    // And again ten minutes later. A probe is not a use: the window runs
    // from the call.
    world.clock.advance(10 * MINUTE);
    world.tend_once().await;
    assert_eq!(world.reads(), 3);

    // An hour after its last use the session is no longer wanted: it is
    // not probed again, and lapses as it stands.
    world.clock.advance(40 * MINUTE);
    world.tend_once().await;
    world.clock.advance(10 * MINUTE);
    world.tend_once().await;
    assert_eq!(world.reads(), 3);
    assert_eq!(world.session_state().await, Some(SessionState::Valid));
    assert_eq!(world.logins.load(Ordering::SeqCst), 1);
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_probe_that_cannot_run_changes_nothing() {
    let world = World::probed(None).await;
    world.person_signs_in().await;
    assert_eq!(world.secure_read().await, "completed");

    // With no runner to carry the probe out, it is not made, and the
    // session stays as it is although the system has ended it.
    world.kernel.stop_runner_loop().await;
    world.hooks.session_ended.store(true, Ordering::SeqCst);
    world.clock.advance(15 * MINUTE);
    world.tend_once().await;
    assert_eq!(world.reads(), 1);
    assert_eq!(world.session_state().await, Some(SessionState::Valid));
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}
