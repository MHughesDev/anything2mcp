//! What the tests share: a keeper over memory with a clock the test moves,
//! a provider that does what the test scripts, a person who answers as
//! told, and a sink that shows what a driver would have been given.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_core::{
    AccountName, DecisionId, Environment, Failure, Identity, NoSecrets, PrincipalId, Redact, RunId,
    Secret, TargetName, Timestamp, WorkspaceId,
};
use a2m_ledger::{Store, StoreConfig};
use a2m_telemetry::{Clock, LogLevel, ManualClock, Telemetry};
use a2m_vault::{BoxFuture, MemoryKeyStore, Vault};
use tokio::sync::Semaphore;

use crate::failure;
use crate::{
    AskPerson, AuthEntry, AuthProvider, ChallengeScheme, CredentialRef, CredentialSink, ExpirySign,
    InProcessWork, Keeper, KeeperJob, KeeperWork, KeeperWorker, LeaseRequest, Material,
    PersonAnswer, PersonAsk, ProbeOutcome, ProviderName, ProviderNeeds, ProviderSet, Purpose,
    RawSign, RunProbe, SessionKey, SessionLease, SessionRef, SessionStore, SignInContext, SignedIn,
    StoredCredentials, Wait, Work, WorkOutcome,
};

/// The start of every test's clock.
pub(super) const START: i64 = 1_800_000_000_000;

pub(super) const HOUR: Duration = Duration::from_secs(3600);

/// What a provider that breaks its contract puts into a failure.
pub(super) const LEAKED: &str = "PLANTED-one-time-code-493817";

/// What a scripted provider does when it is next asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Works,
    Refused,
    WantsSecondFactor,
    Unreachable,
    /// Fails with a sentence that holds a value the provider registered,
    /// which the provider contract forbids.
    Leaks,
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

    fn made(&self, step: Step, cx: &SignInContext<'_>) -> Result<Material, Failure> {
        let now = cx.now();
        match step {
            Step::Works => {
                let token = format!("PLANTED-fake-{}-{}", self.sign_ins(), self.refreshes());
                Ok(Material::new(self.name())
                    .with_secret("token", Secret::new(token))
                    .expiring_at(Some(Timestamp::from_unix_millis(
                        now.as_unix_millis() + 3_600_000,
                    ))))
            }
            Step::Refused => Err(failure::credential_refused("the test")),
            Step::WantsSecondFactor => Err(failure::needs_mfa()),
            Step::Unreachable => Err(failure::upstream_unavailable("the test is down")),
            Step::Leaks => {
                cx.register(&Secret::new(LEAKED));
                Err(Failure::new(
                    a2m_core::FailureClass::Transient,
                    crate::codes::UPSTREAM_UNAVAILABLE,
                    format!("The page answered: code {LEAKED} was not accepted."),
                    format!("Enter {LEAKED} again."),
                ))
            }
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
            self.made(step, cx)
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
            self.made(step, cx)
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

/// A place where the test can hold something the keeper's work does, and
/// let it through when it chooses.
#[derive(Default)]
pub(super) struct Gate {
    held: Mutex<bool>,
    permits: Option<Semaphore>,
}

impl Gate {
    fn new() -> Self {
        Self {
            held: Mutex::new(false),
            permits: Some(Semaphore::new(0)),
        }
    }

    /// From now on, whatever comes here waits.
    pub(super) fn hold(&self) {
        *self.held.lock().unwrap() = true;
    }

    /// From now on, whatever comes here passes. What already waits goes on
    /// waiting, until it is released.
    pub(super) fn open(&self) {
        *self.held.lock().unwrap() = false;
    }

    /// Lets the one that has waited longest through.
    pub(super) fn release_one(&self) {
        if let Some(permits) = &self.permits {
            permits.add_permits(1);
        }
    }

    /// Lets everything through, now and from now on.
    pub(super) fn release(&self) {
        *self.held.lock().unwrap() = false;
        if let Some(permits) = &self.permits {
            permits.add_permits(1000);
        }
    }

    async fn pass(&self) {
        let held = *self.held.lock().unwrap();
        if let (true, Some(permits)) = (held, &self.permits) {
            permits.acquire().await.unwrap().forget();
        }
    }
}

/// The keeper's way to its worker, with a place where the test can hold a
/// restore on its way there, and lose a sign-out.
pub(super) struct Relay {
    work: InProcessWork,
    /// A restore waits here, after it is counted, while the test holds it.
    pub(super) restores: Gate,
    restores_seen: AtomicU32,
    lose_sign_outs: AtomicBool,
}

impl Relay {
    fn new(work: InProcessWork) -> Self {
        Self {
            work,
            restores: Gate::new(),
            restores_seen: AtomicU32::new(0),
            lose_sign_outs: AtomicBool::new(false),
        }
    }

    /// How many restores the keeper has sent.
    pub(super) fn restores_seen(&self) -> u32 {
        self.restores_seen.load(Ordering::SeqCst)
    }

    /// From now on a sign-out never reaches the worker, as when the home
    /// runner cannot be reached.
    pub(super) fn lose_sign_outs(&self) {
        self.lose_sign_outs.store(true, Ordering::SeqCst);
    }
}

impl KeeperWork for Relay {
    fn probe<'a>(&'a self, session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome> {
        self.work.probe(session)
    }

    fn perform(&self, job: KeeperJob) -> BoxFuture<'_, WorkOutcome> {
        Box::pin(async move {
            match &job.work {
                Work::Restore(_) => {
                    self.restores_seen.fetch_add(1, Ordering::SeqCst);
                    self.restores.pass().await;
                }
                Work::SignOut if self.lose_sign_outs.load(Ordering::SeqCst) => {
                    return WorkOutcome::Failed(failure::upstream_unavailable(
                        "the test lost the sign-out",
                    ));
                }
                _ => {}
            }
            self.work.perform(job).await
        })
    }

    fn ask_person<'a>(
        &'a self,
        session: &'a SessionRef,
        ask: PersonAsk,
    ) -> BoxFuture<'a, PersonAnswer> {
        self.work.ask_person(session, ask)
    }
}

type Answer = Box<dyn Fn(&PersonAsk) -> PersonAnswer + Send + Sync>;

/// The session's own person, who answers as the test says and is never
/// reached any other way.
pub(super) struct Person {
    pub(super) asks: Mutex<Vec<PersonAsk>>,
    answer: Mutex<Answer>,
    /// An ask waits here, before it is recorded, while the test holds it.
    pub(super) gate: Gate,
}

impl Person {
    fn new() -> Self {
        Self {
            asks: Mutex::new(Vec::new()),
            answer: Mutex::new(Box::new(|_| PersonAnswer::NoAnswer)),
            gate: Gate::new(),
        }
    }

    pub(super) fn answers(
        &self,
        answer: impl Fn(&PersonAsk) -> PersonAnswer + Send + Sync + 'static,
    ) {
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
            self.gate.pass().await;
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
    /// A probe waits here, after it is counted, while the test holds it.
    pub(super) gate: Gate,
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
            self.gate.pass().await;
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

/// A folder of the test's own under the machine's temporary folder, removed
/// when the test is done with it.
pub(super) struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "a2m-auth-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        // Best effort: a store thread that is still closing may hold a file.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One engine: a keeper and its worker in one process, over the store, a
/// key store, and a clock that outlive it.
pub(super) struct World {
    pub(super) clock: Arc<ManualClock>,
    keys: Arc<MemoryKeyStore>,
    /// Where the store's file is, for a world that can be restarted.
    folder: Option<Folder>,
    pub(super) store: SessionStore,
    pub(super) vault: Arc<Vault>,
    pub(super) credentials: StoredCredentials,
    pub(super) fake: Arc<Fake>,
    pub(super) person: Arc<Person>,
    pub(super) probe: Arc<Probe>,
    pub(super) worker: Arc<KeeperWorker>,
    pub(super) relay: Arc<Relay>,
    pub(super) keeper: Keeper,
    pub(super) telemetry: Telemetry,
}

impl World {
    /// An engine over a store in memory.
    pub(super) async fn new(needs: ProviderNeeds) -> Self {
        Self::fresh(needs, None).await
    }

    /// An engine over a store in a file, so that it can be restarted.
    pub(super) async fn on_disk(needs: ProviderNeeds) -> Self {
        Self::fresh(needs, Some(Folder::new())).await
    }

    async fn fresh(needs: ProviderNeeds, folder: Option<Folder>) -> Self {
        Self::over(
            Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START))),
            Arc::new(MemoryKeyStore::new()),
            folder,
            Arc::new(Fake::new(needs)),
            Arc::new(Person::new()),
            Arc::new(Probe {
                outcome: Mutex::new(ProbeOutcome::SignedIn),
                calls: AtomicU32::new(0),
                gate: Gate::new(),
            }),
        )
        .await
    }

    /// The engine after a restart: the store's file, the key store, and the
    /// world outside are the same; everything held in memory is gone, and
    /// the store is opened anew.
    pub(super) async fn restarted(self) -> Self {
        let Self {
            clock,
            keys,
            folder,
            fake,
            person,
            probe,
            ..
        } = self;
        assert!(folder.is_some(), "only a world on disk can be restarted");
        Self::over(clock, keys, folder, fake, person, probe).await
    }

    async fn over(
        clock: Arc<ManualClock>,
        keys: Arc<MemoryKeyStore>,
        folder: Option<Folder>,
        fake: Arc<Fake>,
        person: Arc<Person>,
        probe: Arc<Probe>,
    ) -> Self {
        let config = match &folder {
            Some(folder) => StoreConfig::in_dir(&folder.0),
            None => StoreConfig::in_memory(),
        };
        let ledger = Store::open(config.with_clock(clock.clone())).await.unwrap();
        let vault = Arc::new(Vault::new(keys.clone(), Arc::new(ledger.key_records())));
        ledger.attach(Arc::clone(&vault)).await.unwrap();
        let store = SessionStore::new(ledger);
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
        let relay = Arc::new(Relay::new(InProcessWork::new(
            Arc::clone(&worker),
            person.clone(),
            probe.clone(),
        )));
        let work = Arc::clone(&relay) as Arc<dyn KeeperWork>;
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
            folder,
            store,
            vault,
            credentials,
            fake,
            person,
            probe,
            worker,
            relay,
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

/// Waits until `met` holds. The work it waits for runs on the store's own
/// thread, so the test gives way until that work is done; a condition that
/// never comes fails the test instead of hanging it.
pub(super) async fn until(met: impl Fn() -> bool) {
    for _ in 0..50_000_000u64 {
        if met() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the condition the test waits for never came");
}

/// Runs `work` on a runtime of its own, for a test that is not async.
pub(super) fn block<T>(work: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(work)
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
