//! Package 2.5 in the action registry: who carries a sign-in out decides
//! whether a refused stored credential is tried again (decision 0269). An
//! agent's `session.login` is a program asking, and at the attempt limit
//! it stops at once with nothing sent; the person's own press on the
//! engine's page of the sign-in is theirs to try, once more.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use a2m_actions::PresenceProof;
use a2m_core::{AuthProviderName, FailureClass};
use a2m_policy::PresenceCheck;
use serde_json::{Map, json};

use super::kernel::engine;
use super::kernel::{Options, World, world};

const MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "example_stored_logins"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[points]
auth_provider = ["example_stored_login"]

[effects]
sessions = ["json_app"]
"#;

/// What the system the provider signs in to holds: whether it refuses the
/// credential, and every sign-in it was sent.
#[derive(Default)]
struct Side {
    refuses: AtomicBool,
    sent: AtomicU32,
}

struct StoredPack(Arc<Side>);

impl a2m_provider::Pack for StoredPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(
        &self,
        _cx: &a2m_provider::PackContext,
        to: &mut a2m_provider::Registrar<'_>,
    ) -> Result<(), a2m_core::Failure> {
        to.auth_provider(Arc::new(StoredLogin(Arc::clone(&self.0))))
    }
}

/// Signs in with what is stored, with nobody there, as a form sign-in does:
/// the system says whether it took the credential.
struct StoredLogin(Arc<Side>);

impl a2m_auth::AuthProvider for StoredLogin {
    fn name(&self) -> a2m_auth::ProviderName {
        a2m_auth::ProviderName::fixed("example_stored_login")
    }

    fn needs(&self) -> a2m_auth::ProviderNeeds {
        a2m_auth::ProviderNeeds {
            person: false,
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
            self.0.sent.fetch_add(1, Ordering::SeqCst);
            if self.0.refuses.load(Ordering::SeqCst) {
                return Err(a2m_core::Failure::new(
                    FailureClass::SessionExpired,
                    a2m_auth::codes::CREDENTIAL_REFUSED,
                    "The system refused the stored credential.",
                    "Have the person sign in.",
                ));
            }
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
        cx: &'a a2m_auth::SignInContext<'a>,
    ) -> a2m_auth::BoxFuture<'a, Result<a2m_auth::Material, a2m_core::Failure>> {
        self.sign_in(cx)
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

fn code_of(response: &a2m_actions::Response) -> String {
    response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"))
        .code
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_press_retries_a_credential_an_agents_ask_may_not() {
    let side = Arc::new(Side::default());
    side.refuses.store(true, Ordering::SeqCst);
    let mut options = Options::with_user_settings("[sessions]\nlogin_wait_minutes = 1\n");
    options.extra_pack = Some(Arc::new(StoredPack(Arc::clone(&side))));
    let world: World = world(options).await;
    *world.surface.pages.lock().unwrap() = true;
    let mut compiled_target = engine::compiled_target(&world.app.url(""));
    compiled_target.auth[0].provider = AuthProviderName::new("example_stored_login").unwrap();
    compiled_target.auth[0].attempt_limit = 1;
    world
        .kernel
        .publish_compiled_target(&compiled_target)
        .await
        .unwrap();

    // The agent asks; the stored credential is sent once and refused.
    let login = json!({"target": "example"});
    let first = world.call("session.login", login.clone()).await;
    assert!(!first.ok, "{first:?}");
    assert_eq!(code_of(&first), "credential_refused", "{first:?}");
    assert_eq!(side.sent.load(Ordering::SeqCst), 1);

    // At the limit, the agent's asks stop at once: nothing is sent again,
    // so a system that locks an account after failures counts no more.
    for _ in 0..3 {
        let again = world.call("session.login", login.clone()).await;
        assert_eq!(code_of(&again), "credential_refused", "{again:?}");
    }
    assert_eq!(side.sent.load(Ordering::SeqCst), 1);

    // The person is shown the sign-in on the engine's page and presses
    // Carry out: theirs is tried, once, and now holds.
    side.refuses.store(false, Ordering::SeqCst);
    let def = world
        .kernel
        .actions()
        .find("session.login")
        .unwrap()
        .clone();
    let mut input = Map::new();
    input.insert("target".to_owned(), json!("example"));
    world
        .kernel
        .ask_person(&def, &world.caller, input, (None, None))
        .await
        .unwrap();
    let ask = world
        .surface
        .acts
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|ask| ask.action.as_str() == "session.login")
        .cloned()
        .unwrap();
    let person = world.caller.principal.principal().id;
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person, world.now()).for_act(ask.act);
    let pressed = world
        .surface
        .desk()
        .carry_out(ask.act, proof)
        .await
        .unwrap();
    assert!(pressed.ok, "{pressed:?}");
    assert_eq!(pressed.result.as_ref().unwrap()["state"], "signed_in");
    assert_eq!(side.sent.load(Ordering::SeqCst), 2);

    // The agent's next ask finds the session that holds, and sends nothing.
    let held = world.ok("session.login", login).await;
    assert_eq!(held["state"], "signed_in", "{held}");
    assert_eq!(side.sent.load(Ordering::SeqCst), 2);
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}
