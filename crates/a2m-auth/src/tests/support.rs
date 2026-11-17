//! What the tests share: a keeper over memory with a clock the test moves,
//! a provider that does what the test scripts, a person who answers as
//! told, and a sink that shows what a driver would have been given.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_core::{
    AccountName, DecisionId, Environment, Failure, Identity, NoSecrets, PrincipalId, Redact,
    RunId, Secret, TargetName, Timestamp, WorkspaceId,
};
use a2m_telemetry::{Clock, LogLevel, ManualClock, Telemetry};
use a2m_vault::{BoxFuture, MemoryKeyRecords, MemoryKeyStore, Vault};
use tokio::sync::Semaphore;

use crate::failure;
use crate::{
    AskPerson, AuthEntry, AuthProvider, ChallengeScheme, CredentialRef, CredentialSink,
    ExpirySign, InProcessWork, Keeper, KeeperWorker, LeaseRequest, Material, PersonAnswer,
    PersonAsk, ProbeOutcome, ProviderName, ProviderNeeds, ProviderSet, Purpose, RawSign,
    RunProbe, SessionKey, SessionLease, SessionRef, SessionStore, SignInContext, SignedIn,
    StoredCredentials, Wait,
};

/// The start of every test's clock.
pub(super) const START: i64 = 1_800_000_000_000;

pub(super) const HOUR: Duration = Duration::from_secs(3600);

/// What a scripted provider does when it is next asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Works,
    Refused,
    WantsPerson,
    WantsSecondFactor,
    Unreachable,
}

/// A provider that does what the test scripts, and counts what it was asked.
pub(super) struct Fake {
    pub(super) needs: Mutex<ProviderNeeds>,
    pub(super) on_sign_in: Mutex<Step>,
    pub(super) on_refresh: Mutex<Step>,
    sign_ins: AtomicU32,
    refreshes: AtomicU32,
    /// A renewal waits here until the test lets it through.
    gate: Semaphore,
    held: Mutex<bool>,
}

impl Fake {
    pub(super) fn new(needs: ProviderNeeds) -> Self {
        Self {
            needs: Mutex::new(needs),
            on_sign_in: Mutex::new(Step::Works),
            on_refresh: Mutex::new(Step::Works),
            sign_ins: AtomicU32::new(0),
            refreshes: AtomicU32::new(0),
            gate: Semaphore::new(0),
            held: Mutex::new(false),
        }
    }

    pub(super) fn sign_ins(&self) -> u32 {
        self.sign_ins.load(Ordering::SeqCst)
    }

    pub(super) fn refreshes(&self) -> u32 {
        self.refreshes.load(Ordering::SeqCst)
    }

    pub(super) fn hold_renewals(&self) {
        *self.held.lock().unwrap() = true;
    }

    pub(super) fn release_renewals(&self) {
        *self.held.lock().unwrap() = false;
        self.gate.add_permits(1000);
    }

    fn made(&self, step: Step, now: Timestamp) -> Result<Material, Failure> {
        match step {
            Step::Works => {
                let token = format!(
                    "PLANTED-fake-{}-{}",
                    self.sign_ins(),
                    self.refreshes()
                );
                Ok(Material::new(self.name())
                    .with_secret("token", Secret::new(token))
                    .expiring_at(Some(Timestamp::from_unix_millis(
                        now.as_unix_millis() + 3_600_000,
                    ))))
            }
            Step::Refused => Err(failure::credential_refused("the test")),
            Step::WantsPerson => Err(failure::needs_login("the test wants a person")),
            Step::WantsSecondFactor => Err(failure::needs_mfa()),
            Step::Unreachable => Err(failure::upstream_unavailable("the test is down")),
        }
    }
}

impl AuthProvider for Fake {
    fn name(&self) -> ProviderName {
        ProviderName::new("fake").unwrap()
    }

    fn needs(&self) -> ProviderNeeds {
        *self.needs.lock().unwrap()
    }

    fn check(&self, _entry: &AuthEntry) -> Result<(), Failure> {
        Ok(())
    }

    fn sign_in<'a>(
        &'a self,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(async move {
            self.sign_ins.fetch_add(1, Ordering::SeqCst);
            let step = *self.on_sign_in.lock().unwrap();
            self.made(step, cx.now())
        })
    }

    fn is_signed_in<'a>(
        &'a self,
        _material: &'a Material,
        _now: Timestamp,
    ) -> BoxFuture<'a, SignedIn> {
        Box::pin(async { SignedIn::Yes { until: None } })
    }

    fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign> {
        matches!(sign, RawSign::Status { code: 401, .. }).then_some(ExpirySign::Refused)
    }

    fn refresh<'a>(
        &'a self,
        _material: &'a Material,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(async move {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            let held = *self.held.lock().unwrap();
            if held {
                let _permit = self.gate.acquire().await.unwrap();
            }
            let step = *self.on_refresh.lock().unwrap();
            self.made(step, cx.now())
        })
    }

    fn sign_out<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async { Ok(()) })
    }

    fn inject(&self, material: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure> {
        sink.header("Authorization", material.secret("token").unwrap());
        Ok(())
    }
}

type Answer = Box<dyn Fn(&PersonAsk) -> PersonAnswer + Send + Sync>;

/// The session's own person, who answers as the test says and is never
/// reached any other way.
pub(super) struct Person {
    pub(super) asks: Mutex<Vec<PersonAsk>>,
    answer: Mutex<Answer>,
}

impl Person {
    fn new() -> Self {
        Self {
            asks: Mutex::new(Vec::new()),
            answer: Mutex::new(Box::new(|_| PersonAnswer::NoAnswer)),
        }
    }

    pub(super) fn answers(&self, answer: impl Fn(&PersonAsk) -> PersonAnswer + Send + Sync + 'static) {
        *self.answer.lock().unwrap() = Box::new(answer);
    }

    pub(super) fn notices(&self) -> usize {
        self.asks
            .lock()
            .unwrap()
            .iter()
            .filter(|ask| matches!(ask, PersonAsk::Notify { .. }))
            .count()
    }
}

impl AskPerson for Person {
    fn ask<'a>(&'a self, _session: &'a SessionRef, ask: PersonAsk) -> BoxFuture<'a, PersonAnswer> {
        Box::pin(async move {
            let answer = (self.answer.lock().unwrap())(&ask);
            self.asks.lock().unwrap().push(ask);
            answer
        })
    }

    fn return_address(&self) -> Option<String> {
        Some("http://127.0.0.1:1/returned".to_owned())
    }
}

/// The target's probe, with the outcome the test sets.
pub(super) struct Probe {
    pub(super) outcome: Mutex<ProbeOutcome>,
    calls: AtomicU32,
}

impl Probe {
    pub(super) fn calls(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }
}

impl RunProbe for Probe {
    fn probe<'a>(&'a self, _session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcome.lock().unwrap().clone()
        })
    }
}

/// A driver's client, as far as credentials go: it shows what was placed.
#[derive(Default)]
pub(super) struct Sink {
    pub(super) placed: Vec<(String, String)>,
}

impl CredentialSink for Sink {
    fn header(&mut self, name: &str, value: &Secret) {
        self.placed
            .push((format!("header {name}"), value.expose().to_owned()));
    }

    fn cookie(&mut self, origin: &str, name: &str, value: &Secret) {
        self.placed
            .push((format!("cookie {origin} {name}"), value.expose().to_owned()));
    }

    fn tls_identity(&mut self, certificate: &[u8], key: &Secret) {
        let certificate = String::from_utf8_lossy(certificate);
        self.placed
            .push((format!("tls {certificate}"), key.expose().to_owned()));
    }

    fn challenge_credential(&mut self, scheme: ChallengeScheme, user: &str, secret: &Secret) {
        self.placed.push((
            format!("challenge {} {user}", scheme.as_str()),
            secret.expose().to_owned(),
        ));
    }
}

pub(super) fn placed(lease: &SessionLease) -> Vec<(String, String)> {
    let mut sink = Sink::default();
    lease.apply(&mut sink).unwrap();
    sink.placed
}

pub(super) fn workspace() -> WorkspaceId {
    WorkspaceId::from_bytes([7; 16])
}

pub(super) fn person_id(number: u8) -> PrincipalId {
    PrincipalId::from_bytes([number; 16])
}

pub(super) fn account_key(account: &str) -> SessionKey {
    SessionKey {
        workspace: workspace(),
        target: TargetName::new("example").unwrap(),
        environment: Environment::new("test").unwrap(),
        identity: Identity::Account(AccountName::new(account).unwrap()),
    }
}

pub(super) fn person_key(number: u8) -> SessionKey {
    SessionKey {
        identity: Identity::Person(person_id(number)),
        ..account_key("unused")
    }
}

pub(super) fn request(key: &SessionKey, entry: &Arc<AuthEntry>, wait: Wait) -> LeaseRequest {
    let acting_for = match &key.identity {
        Identity::Person(person) => Some(*person),
        Identity::Account(_) => None,
    };
    LeaseRequest {
        key: key.clone(),
        entry: Arc::clone(entry),
        run: RunId::from_bytes([3; 16]),
        decided: DecisionId::from_bytes([4; 16]),
        acting_for,
        wait,
        headroom: None,
        purpose: Purpose::Run,
    }
}

pub(super) fn fake_entry() -> Arc<AuthEntry> {
    Arc::new(AuthEntry::new(ProviderName::new("fake").unwrap()))
}

pub(super) fn reference(name: &str) -> CredentialRef {
    CredentialRef::new(&format!("vault://example/{name}")).unwrap()
}

/// A provider that works from a stored credential and renews silently.
pub(super) const STORED_RENEWING: ProviderNeeds = ProviderNeeds {
    person: false,
    silent_renewal: true,
    light_prompt: false,
    movable: true,
    confirms: true,
};

/// A provider that only holds a stored credential.
pub(super) const STORED_HELD: ProviderNeeds = ProviderNeeds {
    person: false,
    silent_renewal: false,
    light_prompt: false,
    movable: true,
    confirms: false,
};

/// A provider a person signs in to, which renews silently.
pub(super) const PERSON_RENEWING: ProviderNeeds = ProviderNeeds {
    person: true,
    silent_renewal: true,
    light_prompt: false,
    movable: true,
    confirms: true,
};

/// A provider a person signs in to, with no way to renew.
pub(super) const PERSON_ONLY: ProviderNeeds = ProviderNeeds {
    person: true,
    silent_renewal: false,
    light_prompt: false,
    movable: true,
    confirms: true,
};

/// One engine: a keeper and its worker in one process, over rows, keys, and
/// a clock that outlive it.
pub(super) struct World {
    pub(super) clock: Arc<ManualClock>,
    keys: Arc<MemoryKeyStore>,
    records: Arc<MemoryKeyRecords>,
    pub(super) store: SessionStore,
    pub(super) vault: Arc<Vault>,
    pub(super) credentials: StoredCredentials,
    pub(super) fake: Arc<Fake>,
    pub(super) person: Arc<Person>,
    pub(super) probe: Arc<Probe>,
    pub(super) worker: Arc<KeeperWorker>,
    pub(super) keeper: Keeper,
    pub(super) telemetry: Telemetry,
}

impl World {
    pub(super) fn new(needs: ProviderNeeds) -> Self {
        Self::over(
            Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START))),
            Arc::new(MemoryKeyStore::new()),
            Arc::new(MemoryKeyRecords::new()),
            SessionStore::in_memory(),
            Arc::new(Fake::new(needs)),
            Arc::new(Person::new()),
            Arc::new(Probe {
                outcome: Mutex::new(ProbeOutcome::SignedIn),
                calls: AtomicU32::new(0),
            }),
        )
    }

    /// The engine after a restart: the rows, the keys, and the world outside
    /// are the same; everything held in memory is gone.
    pub(super) fn restarted(&self) -> Self {
        Self::over(
            Arc::clone(&self.clock),
            Arc::clone(&self.keys),
            Arc::clone(&self.records),
            self.store.clone(),
            Arc::clone(&self.fake),
            Arc::clone(&self.person),
            Arc::clone(&self.probe),
        )
    }

    fn over(
        clock: Arc<ManualClock>,
        keys: Arc<MemoryKeyStore>,
        records: Arc<MemoryKeyRecords>,
        store: SessionStore,
        fake: Arc<Fake>,
        person: Arc<Person>,
        probe: Arc<Probe>,
    ) -> Self {
        let vault = Arc::new(Vault::new(keys.clone(), records.clone()));
        let credentials = StoredCredentials::new(store.clone(), Arc::clone(&vault));
        let providers = ProviderSet::neutral()
            .with(Arc::clone(&fake) as Arc<dyn AuthProvider>)
            .unwrap();
        let worker = Arc::new(KeeperWorker::new(
            providers.clone(),
            Arc::clone(&vault),
            Arc::new(credentials.clone()),
            clock.clone(),
        ));
        let work = Arc::new(InProcessWork::new(
            Arc::clone(&worker),
            person.clone(),
            probe.clone(),
        ));
        // The telemetry is given a redactor that knows no secret, so that a
        // secret the keeper logged would be seen by the test.
        let telemetry = Telemetry::in_memory(
            LogLevel::Trace,
            Arc::new(NoSecrets) as Arc<dyn Redact>,
            clock.clone() as Arc<dyn Clock>,
        );
        let keeper = Keeper::builder(store.clone(), providers, work, clock.clone())
            .local_worker(Arc::clone(&worker))
            .telemetry(telemetry.clone())
            .build();
        Self {
            clock,
            keys,
            records,
            store,
            vault,
            credentials,
            fake,
            person,
            probe,
            worker,
            keeper,
            telemetry,
        }
    }

    pub(super) async fn store_credential(&self, key: &SessionKey, name: &str, value: &str) {
        self.credentials
            .put(
                &key.workspace,
                &key.identity,
                &reference(name),
                &Secret::new(value),
            )
            .await
            .unwrap();
    }

    pub(super) fn is_registered(&self, value: &str) -> bool {
        self.vault.redactor().contains_secret(value)
    }

    pub(super) async fn state(&self, key: &SessionKey) -> a2m_core::SessionState {
        self.store.find(key).await.unwrap().unwrap().state
    }

    pub(super) async fn kinds(&self, key: &SessionKey) -> Vec<&'static str> {
        let row = self.store.find(key).await.unwrap().unwrap();
        self.keeper
            .timeline(row.id)
            .await
            .unwrap()
            .iter()
            .map(|event| event.kind.as_str())
            .collect()
    }
}

/// Lets every task that can run, run. On the one-thread runtime the tests
/// use, a task stops only where the test itself holds it, so this returns
/// with every task at such a point.
pub(super) async fn settle() {
    for _ in 0..200 {
        tokio::task::yield_now().await;
    }
}

/// Runs `work` on a one-thread runtime with the telemetry receiving every
/// log line the keeper writes.
pub(super) fn logged<T>(telemetry: &Telemetry, work: impl Future<Output = T>) -> T {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    telemetry.scoped(|| runtime.block_on(work))
}

/// Fails if any text holds a planted value.
pub(super) fn assert_clean(texts: &[String], planted: &[&str]) {
    for text in texts {
        for value in planted {
            assert!(!text.contains(value), "`{value}` appears in: {text}");
        }
    }
}

/// Decodes a query string or a form body.
pub(super) fn decode_form(text: &str) -> BTreeMap<String, String> {
    fn part(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == b'%' && at + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                at += 3;
            } else {
                out.push(if bytes[at] == b'+' { b' ' } else { bytes[at] });
                at += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (part(name), part(value))
        })
        .collect()
}
