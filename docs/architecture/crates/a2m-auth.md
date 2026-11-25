# `a2m-auth`

The session keeper and the auth providers. The keeper is the only code that signs in, checks, renews, and ends a session; it hands the engine a `SessionLease` and never hands anyone a credential. The neutral providers live here; providers that must drive a surface live in the pack that owns the surface.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 4, Services |
| May depend on | Layers 1 to 3 |
| Depends on | `a2m-core`; `a2m-vault` to seal saved state and for the redactor; `a2m-ledger` for its tables; `a2m-logs` for the clock, logs, and metrics. Outside: `reqwest` for identity services' token endpoints; `sha1` and `sha2` for one-time codes and proof keys; `serde`, `serde_json`, `tokio`, `tracing`, `zeroize`; and, behind the feature `tls`, `rustls` with the ring provider ([record 0084](../../decisions/0084-the-session-keepers-outside-crates-and-tls-for-identity-services.md)), `rustls-platform-verifier`, and `webpki-roots` ([record 0157](../../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)). The OAuth exchanges and the encodings are this crate's own code |
| Roadmap packages | Phase 2: slices 2.3 to 2.6, groups C, D, E, H. Phase 7: moving sessions between runners, the operating system's broker, hosted sign-in |
| Carries out | Spec sections [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md), [602](../../spec/6-running-it/602-identity-on-a-shared-server.md), [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md), [503](../../spec/5-security/503-approval-and-sign-in-surfaces.md) (sign-in), [224](../../spec/2-kernel/224-when-the-engine-itself-fails.md) (the key store unreachable); the [auth provider interface](../../reference/technical-reference.md#auth-provider-interface); decisions 0030, 0032, and 0035; [decision 0138](../../decisions/0138-the-vault-and-the-session-keeper-after-their-safety-audit.md), which records what a safety audit changed |

## Owns

- The session state machine: the eight states of section 215, and every transition between them.
- The three clocks: the declared ones, and the ones it learns from each session's history.
- When to check, when to renew, when to ask a person, and when to let a session lapse.
- One renewal per session at a time, and a stored credential tried once.
- `SessionLease`, `KeeperWork`, `AuthProvider`, `CredentialSink`, and `AskPerson`.
- The neutral providers, and the fixed rules that turn sign-in findings into a recommended mode and provider for `auth.detect`.
- The session timeline.

It must never: call a crate above it; read, copy, or decrypt a cookie store it does not own; return a credential to any caller; decide who may use a session (that is `ActAs` in `a2m-policy`); retry a refused stored credential; try to outlast the identity provider's maximum age; run the target's probe itself.

## Where each provider lives

Section 202 left this open, and package A7 of the long-term plan was to settle it. It is settled: **a provider lives in `a2m-auth` when it only holds or exchanges a credential. It lives in a pack when it needs a surface of the system.**

| Provider | Lives in | Why |
| --- | --- | --- |
| `api_key`, `bearer_static`, `http_basic` | `a2m-auth` | A stored value placed in a header |
| `http_digest`, `windows_integrated` | `a2m-auth` | The provider holds the credential and injects it; carrying out the challenge and response on the connection is the HTTP driver's work |
| `client_certificate` | `a2m-auth` | A certificate and key placed in the client's TLS identity |
| `oauth_client_credentials`, `obo_token` | `a2m-auth` | A token exchange with the identity service, not with the system |
| `oauth_authorization_code` | `a2m-auth` | A token exchange; the person's consent is asked through `PersonSurface` |
| `totp` | `a2m-auth` | Computes a code from a stored secret; a pack's provider enters it. It is a function of the sign-in context, not a provider of sessions |
| `os_broker` | `a2m-auth`, per operating system | The operating system's own interface. User devices only; Phase 7 |
| `form_login`, `sso_oidc`, `sso_saml` | `a2m-pack-web-ui` | A person, or a stored credential, works through pages in the engine's own browser |
| `upstream_mcp` | `a2m-pack-mcp` | The upstream server's own authorization discovery; consent through `PersonSurface` |

**Consent in a browser, outside the web pack.** A provider that needs a person to agree in a browser, `oauth_authorization_code` or `upstream_mcp`, asks through `PersonSurface`, which `a2m-actions` defines and `a2m-local` and `a2m-api` implement. The person is given a link and opens it in their own browser. Nothing drives that browser, and no pack borrows another pack's browser. The keeper reaches `PersonSurface` through `KeeperWork::ask_person`, not through a handle of its own, so that the keeper has one way out and that way never calls upward. On the home runner the worker is handed an `AskPerson` by its runner, and the provider asks through `SignInContext::ask_person`, which returns the authorization response the person's browser came back with.

## Interface

As built. The decisions behind it are in [record 0085](../../decisions/0085-the-session-keepers-interface-as-building-it-settled-it.md); the outside crates and the TLS choice are in [record 0084](../../decisions/0084-the-session-keepers-outside-crates-and-tls-for-identity-services.md).

```rust
pub struct SessionKey { pub workspace: WorkspaceId, pub target: TargetName, pub environment: Environment, pub identity: Identity }
/// One session as the keeper's work sees it. Names only.
pub struct SessionRef { pub session: SessionId, pub key: SessionKey, pub epoch: u64, pub home: Option<RunnerId> }
/// One entry of auth.yaml as the keeper uses it; a2m-actions hands it in with each request.
pub struct AuthEntry {
    pub provider: ProviderName,
    pub settings: BTreeMap<String, String>,          // the provider's own settings, by name
    pub credentials: BTreeMap<String, CredentialRef>, // role -> `vault://...`
    pub idle: Option<Duration>, pub absolute: Option<Duration>,
    pub attempt_limit: u32,                           // default 1
    pub movable: bool,                                // default true
    pub surfaces: EntrySurfaces,                      // decisions 0245 and 0248: empty by default
}
impl AuthEntry { pub fn published(self) -> Self; /* new, setting, credential, required, optional, only */ }
/// The entry's surfaces as the compiled target holds them in the environment, the compiled target's pack sections of their
/// kinds, every address the target declares there, whether the compiled target is a published version's (only the
/// caller that loaded one says so), and whether the person carried out this one sign-in on a page that
/// showed every address (only the caller that took the act says so). Decision 0248.
pub struct EntrySurfaces {
    pub surfaces: Vec<CompiledSurface>, pub sections: Vec<PackSection>,
    pub from_published: bool, pub shown_to_person: bool, pub destinations: SignInDestinations,
}
impl EntrySurfaces {
    pub fn of(compiled_target: &CompiledTarget, auth: &CompiledAuth, environment: &Environment) -> Self;
    pub fn published(self) -> Self; pub fn shown_to_person(self) -> Self;
}
/// Where a sign-in may send: the target's surface addresses in the environment, compared by origin
/// for http and https, as written otherwise. No setter.
pub struct SignInDestinations { /* new(addresses), allows(&str), iter(), is_empty() */ }
/// One address a sign-in sends a credential, a token, or the person to, and what it is.
pub struct SignInAddress { pub what: String, pub address: String }
pub fn sign_in_addresses(provider: &dyn AuthProvider, entry: &AuthEntry) -> Vec<SignInAddress>;

pub struct Keeper { /* Clone; an Arc inside */ }
impl Keeper {
    pub fn builder(store: SessionStore, providers: ProviderSet, work: Arc<dyn KeeperWork>, clock: Arc<dyn Clock>) -> KeeperBuilder;
    pub async fn lease(&self, req: LeaseRequest) -> Result<SessionLease, Failure>;
    pub async fn report(&self, lease: &SessionLease, sign: ExpirySign);
    pub async fn sign_in(&self, key: &SessionKey, entry: Arc<AuthEntry>, acting_for: Option<PrincipalId>) -> Result<SignInTicket, Failure>; // session.login asked by a program: sign_in_by(.., SignInBy::Asked)
    pub async fn sign_in_by(&self, key: &SessionKey, entry: Arc<AuthEntry>, acting_for: Option<PrincipalId>, by: SignInBy) -> Result<SignInTicket, Failure>; // Asked | Person (decision 0269)
    pub fn downgrade(&self) -> WeakKeeper; // what a probe the keeper's work holds keeps of it; WeakKeeper::upgrade() -> Option<Keeper>
    pub async fn status(&self, workspace: &WorkspaceId, identity: Option<&Identity>) -> Result<Vec<SessionStatus>, Failure>;              // session.status, session.list
    pub async fn signed_in(&self, session: SessionId, limit: Duration) -> Result<SessionStatus, Failure>; // waits for the outcome of sign_in; starts nothing (decision 0205)
    pub async fn timeline(&self, session: SessionId) -> Result<Vec<SessionEvent>, Failure>;
    pub async fn revoke(&self, key: &SessionKey, by: EndedBy) -> Result<(), Failure>;   // session.revoke (Owner), session.end (Administrator)
    pub async fn tick(&self);          // timers; reads the keeper's clock
    pub async fn idle(&self);          // waits until no work it started is in flight
    pub fn waiting(&self) -> usize;    // calls waiting for a session now
    pub fn needs(&self, provider: &ProviderName) -> Result<ProviderNeeds, Failure>;
    pub fn sign_in_addresses(&self, target: &TargetName, entry: &AuthEntry) -> Result<Vec<SignInAddress>, Failure>; // checked first
    pub fn allows(&self, session: SessionId, entry: &AuthEntry) -> bool; // a drafts' entry sends only where its person allowed
}
impl KeeperBuilder {   // local_worker(Arc<KeeperWorker>), settings(KeeperSettings), logs_and_traces(LogsAndTraces), random(Arc<dyn RandomSource>), build()
}
pub struct SessionStore { /* the keeper's tables in the store */ }
impl SessionStore { pub fn new(store: Store) -> SessionStore; }   // the Store of a2m-ledger

pub struct LeaseRequest {
    pub key: SessionKey, pub entry: Arc<AuthEntry>, pub run: RunId,
    pub decided: DecisionId,              // the ActAs decision; recorded with the lease, not re-made
    pub acting_for: Option<PrincipalId>,  // the person the run acts for; a person's session is leased to nobody else
    pub wait: Wait,                       // Interactive(Duration) | NoWait; Wait::DEFAULT is 5 minutes
    pub headroom: Option<Duration>,
    pub purpose: Purpose,                 // Run | Probe
}

/// A session as one run may use it. Carries no secret and cannot print one.
pub struct SessionLease { /* a SessionGrant, and a handle to the home runner's live credentials */ }
impl SessionLease {
    pub fn session(&self) -> SessionId;   pub fn epoch(&self) -> u64;   pub fn state(&self) -> SessionState;
    pub fn profile(&self) -> Option<&ProfileRef>;
    pub fn grant(&self) -> &SessionGrant;                 // plain data; crosses the runner link
    pub fn apply(&self, sink: &mut dyn CredentialSink) -> Result<(), Failure>;
    pub fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign>;
}
/// Serializable. session, epoch, state, home, provider, granted_at, run, decided, purpose, profile.
pub struct SessionGrant { /* all fields public */ }

pub trait CredentialSink: Send {
    fn header(&mut self, name: &str, value: &Secret);
    fn cookie(&mut self, origin: &str, name: &str, value: &Secret);
    fn tls_identity(&mut self, certificate: &[u8], key: &Secret);
    fn challenge_credential(&mut self, scheme: ChallengeScheme, user: &str, secret: &Secret); // Digest | Ntlm | Negotiate
}

/// The keeper's work, run on the session's home runner. a2m-actions implements it by submitting a job.
pub trait KeeperWork: Send + Sync {
    fn probe<'a>(&'a self, session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome>;   // SignedIn | SignedOut | Unknown
    fn perform(&self, job: KeeperJob) -> BoxFuture<'_, WorkOutcome>;
    fn ask_person<'a>(&'a self, session: &'a SessionRef, ask: PersonAsk) -> BoxFuture<'a, PersonAnswer>;
}
pub struct KeeperJob { pub session: SessionRef, pub entry: Arc<AuthEntry>, pub work: Work }
/// SignOut and Forget end the session at the job's epoch: credentials held at that epoch or an
/// earlier one are dropped, newer ones stay, and nothing of an earlier epoch is held again.
pub enum Work { SignIn, Renew(Rung), SignOut, Forget, Restore(Sealed) }
pub enum Rung { Silent, LightPrompt, FullSignIn }
pub enum WorkOutcome {
    Held { epoch: u64, expires_at: Option<Timestamp>, saved: Option<Sealed>, profile: Option<ProfileRef>, confirmed: bool },
    NeedsPerson(PersonReason), Refused,
    Ended,              // dropped, or not held because the session had ended or moved on meanwhile
    Failed(Failure),    // a provider's failure, with every registered secret masked
}

/// The other half of the keeper, on the home runner; the only caller of sign_in, refresh, sign_out.
pub struct KeeperWorker { /* providers; vault; stored credentials; clock; live credentials in memory */ }
impl KeeperWorker {
    pub fn new(providers: ProviderSet, vault: Arc<Vault>, credentials: Arc<dyn Credentials>, clock: Arc<dyn Clock>) -> KeeperWorker;
    pub async fn run(&self, job: KeeperJob, person: &dyn AskPerson) -> WorkOutcome;
    pub fn bind(&self, grant: SessionGrant) -> SessionLease;   // a grant from the link becomes a lease here
    pub fn holds(&self, session: SessionId, epoch: u64) -> bool;   // at that epoch, and not ended since
}
/// KeeperWork for one process: the job goes straight to the worker.
pub struct InProcessWork { /* new(worker, Arc<dyn AskPerson>, Arc<dyn RunProbe>) */ }
pub trait RunProbe: Send + Sync { fn probe<'a>(&'a self, session: &'a SessionRef) -> BoxFuture<'a, ProbeOutcome>; } // one that leases through the keeper holds a WeakKeeper

/// "Ask a person to do this." a2m-actions joins it to PersonSurface.
pub trait AskPerson: Send + Sync {
    fn ask<'a>(&'a self, session: &'a SessionRef, ask: PersonAsk) -> BoxFuture<'a, PersonAnswer>;
    fn return_address(&self) -> Option<String> { None }   // where a person's browser comes back to after a link
}
pub enum PersonAsk { OpenLink { url: String, reason: PersonReason }, EnterCode { reason: PersonReason }, Notify { reason: PersonReason } }
pub enum PersonAnswer { Done(Returned), Declined, NoAnswer, Unreachable }   // Returned: named values, each a Secret; new, with, get, iter, is_empty, only
pub enum PersonReason { SignIn, Consent, SecondFactor, CredentialRefused }

pub trait AuthProvider: Send + Sync {
    fn name(&self) -> ProviderName;
    fn needs(&self) -> ProviderNeeds;   // person, silent_renewal, light_prompt, movable, confirms
    fn check(&self, entry: &AuthEntry) -> Result<(), Failure>;   // the entry is usable; nothing is opened
    fn destinations(&self, entry: &AuthEntry) -> Vec<SignInAddress> { Vec::new() } // what it sends to itself (decision 0248)
    fn sign_in<'a>(&'a self, cx: &'a SignInContext<'a>) -> BoxFuture<'a, Result<Material, Failure>>;
    fn is_signed_in<'a>(&'a self, material: &'a Material, now: Timestamp) -> BoxFuture<'a, SignedIn>;
    fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign>;
    fn refresh<'a>(&'a self, material: &'a Material, cx: &'a SignInContext<'a>) -> BoxFuture<'a, Result<Material, Failure>>;
    fn sign_out<'a>(&'a self, material: &'a Material, cx: &'a SignInContext<'a>) -> BoxFuture<'a, Result<(), Failure>>;
    fn inject(&self, material: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure>;
}
/// What a provider is given: session(), entry(), now(), rung(), credential(role), has_credential(role),
/// register(&Secret), random(&mut [u8]), ask_person(PersonAsk), return_address(), one_time_code(),
/// check_address(what, address) just before a request the provider makes itself.
pub struct SignInContext<'a> { /* opaque */ }
/// A session's credentials: named secret parts, named plain parts, an expiry, a profile. Not Clone,
/// not Serialize; Debug shows names only. Built with new / with_secret / with_plain / expiring_at / with_profile.
pub struct Material { /* opaque */ }
/// The providers handed to the keeper. `neutral()` holds the ones built here; `with` adds a pack's.
pub struct ProviderSet { /* new(), neutral(), with(Arc<dyn AuthProvider>) -> Result<Self, Failure>, get(&ProviderName), names() */ }

/// Where the worker gets a stored credential from, by reference.
pub trait Credentials: Send + Sync {
    fn open<'a>(&'a self, session: &'a SessionKey, reference: &'a CredentialRef) -> BoxFuture<'a, Result<Secret, Failure>>;
}
pub struct StoredCredentials { /* new(SessionStore, Arc<Vault>); put(workspace, identity, reference, &Secret); holds(workspace, identity, reference) -> bool, opening nothing */ }

/// The module `tls`: the one way an HTTP client of the product is made (record 0157).
pub mod tls {
    pub struct Trust { /* system(), with_named_roots(&[&[u8]]) -> Result<Self, Failure>, roots_source(), named_roots() */ }
    pub enum RootsSource { Platform, Bundled, NotBuilt }
    pub fn install(trust: Trust) -> Result<(), Failure>;   // once, when the engine starts
    pub fn installed() -> Result<Trust, Failure>;          // what install set, or the system's roots
    pub fn client_builder(trust: &Trust) -> reqwest::ClientBuilder;
    pub fn refusal(error: &reqwest::Error, what: &str) -> Option<Failure>; // a handshake that failed, named
    pub const fn enabled() -> bool;
    pub const CODES: &[(FailureClass, &str)];
}
```

Contracts that the signatures do not show:

- **The keeper never calls upward.** A check or a renewal that needs the target, or a sign-in on the home runner, goes through `KeeperWork`. `a2m-actions` turns it into a job for `a2m-sched`, with the session's home runner as a requirement. The runner runs it: a probe as an ordinary read through the engine, with a lease of purpose `Probe`, which the keeper grants in `valid`, `suspect`, and `renewing`; a renewal or sign-in by calling `KeeperWorker::run`. In one process both halves are the same code, joined by `InProcessWork`.
- **The legal moves.** Section 215 lists the states; the moves between them are the table on `legal_move`, and every write of a state passes it:

| From | To | When |
| --- | --- | --- |
| `absent`, `expired`, `revoked`, `needs_person` | `signing_in` | A sign-in starts |
| `signing_in` | `valid` | The sign-in succeeded |
| `signing_in` | `needs_person` | A stored credential was refused |
| `signing_in` | `absent` | The sign-in was abandoned or could not be made |
| `valid` | `suspect` | A check failed, or a driver reported a sign of expiry |
| `valid` | `renewing` | The credential's own expiry was reached |
| `valid` | `expired` | The maximum age was reached, or the state held in memory was lost |
| `suspect` | `valid` | The one re-check passed |
| `suspect` | `renewing` | The one re-check failed |
| `renewing` | `valid` | The renewal succeeded |
| `renewing` | `needs_person` | The renewal needs a person |
| `renewing` | `expired` | The renewal failed and nobody can be asked |
| `needs_person` | `valid` | The person answered a light prompt |
| `needs_person` | `expired` | The session lapsed while it waited for a person |
| any but `absent` and `revoked` | `revoked` | The session was ended on purpose |

- **One renewer.** A renewal starts only when the keeper moves the session's row from `valid` or `suspect` to `renewing` by one `UPDATE` that names the state and the epoch it expects, in a transaction of the store. The one caller that wins starts the work, as a task of its own so that it finishes even if that caller goes away; everyone waits on the outcome. This holds across processes, because the row is in the store. The epoch increases whenever the credentials are replaced or ended.
- **A call starts work once.** A call that started a sign-in or a renewal, or waited through one, never starts a second. When that work failed, the call is given the failure the work ended with.
- **The ladder is climbed once.** Within one renewal, each rung is tried at most once, in order, and only if the provider says it has it. A stored credential counts against the auth file's `attempt_limit`, default 1: each automatic sign-in or renewal with it that the other side did not confirm is counted, each refusal, and each sign-in that may have sent it and ended in an answer nobody can read (any class but `input`, `policy`, `busy`, `transient`, and a handshake refusal); the count is cleared when a provider that exchanges the credential succeeds or the target's probe passes. At the limit a refusal sends the session to `needs_person` with the reason `credential_refused`, the person surface is told once, and calls stop at once instead of waiting; an unreadable answer sends it there with the reason `sign_in`. Failing to reach the identity service is not a refusal and is not counted ([decision 0269](../../decisions/0269-a-stored-credentials-attempts-outlive-a-restart-and-sign-ins-without-the-person-are-limited.md)).
- **The count outlives a restart and names its credential.** It is the session row's `stored_attempts`, with `attempted_with`, the mark of the stored credential it is about: a digest of each reference with the digest of its sealed envelope, never of a value. A count with another mark than the credential stored now counts as none. `sign_in` is `sign_in_by(.., SignInBy::Asked)`: for a provider that needs no person it is refused at the limit and keeps the count; `SignInBy::Person`, the person's own sign-in, is tried and starts the count again.
- **Sign-ins without the person are limited.** At most `AUTOMATIC_SIGN_INS` (4) sign-ins that made the session hold without its person (a lease's, the renewal's full sign-in, and one a program asked for) within `AUTOMATIC_SIGN_IN_WINDOW` (one hour), counted from the timeline back to the person's own sign-in. When four fall within the window, the next is not made until the person signs in themself: the renewal moves the session to `needs_person`, a lease or an ask stops with `needs_login`. A declared maximum age shorter than the window adds the sign-ins it makes, at most sixty.
- **A stored credential signs in without anyone.** A session that is `absent` or `expired`, whose provider needs no person, is signed in by the keeper on the first lease that asks, within the attempt limit. A provider that needs a person is never started by a lease: the person starts it with `session.login`.
- **A call returns on the record; `idle` waits for the rest.** A call is answered as soon as the session's row decides it. The task that wrote the row may still have steps to take after its last write: the notice to the person, the home runner dropping what it held, a metric. `idle` covers all of it: a session is marked as having work in flight before the row says the work has begun, the marks are counted so that two pieces of work on one session overlap safely, and the mark is dropped when the task ends. Anything that reads what the work leaves behind, a test above all, waits on `idle` first. `waiting` counts only calls that are waiting now; a call that is cancelled is no longer counted.
- **What waits for what.** An interactive call waits up to its wait for anything. A call with `NoWait`, a pipeline, waits for the keeper's own work (a check, a silent renewal, a stored-credential sign-in) up to the keeper's work limit, and never for a person. A wait is counted on the keeper's clock and looked at on each `tick`, so tests move time and nothing sleeps.
- **A sign-in goes only where a published version says** ([decision 0248](../../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)). Before any work, the keeper checks the entry: the provider's own `check`; every credential reference belongs to the session's target (`vault://<target>/...`, else `input`/`credential_of_another_target`); and every address the provider names in `destinations` is allowed by the entry's `SignInDestinations` (else `policy`/`destination_not_declared`). The worker checks again before it calls the provider, and the OAuth providers check each address just before the request. A drafts' entry signs in only when it is marked `shown_to_person`, and renews never: a lease's automatic sign-in, a renewal under a drafts' entry, and `sign_in` with an entry neither published nor shown stop with `needs_login`. A session whose last sign-in was under drafts is not renewed without its person; the mark is the column `signed_in_under` of the session's record, written with the move to `valid`, so it holds across a restart, and a record that does not say, as every one kept before migration `0027`, reads as drafted ([decision 0374](../../decisions/0374-a-sessions-mark-that-it-was-made-under-drafts-is-kept-in-its-record.md)). A lease under a drafts' entry is given only when every address the entry sends the session's credentials to is one the person allowed for that session (in memory; after a restart, none, which refuses such a run until the person signs in again).
- **A credential belongs to its target.** `StoredCredentials` seals a credential with `SealContext::credential`, whose item names the target the reference belongs to, and opens it under the session's target. One sealed for another target, or before the target was in the context, fails as `input`/`credential_unreadable`, which tells the person to store it again.
- **The keeper is told the identity, it does not choose it.** `LeaseRequest` carries the `ActAs` decision that `a2m-actions` obtained from `a2m-policy`, and the person the run acts for. The keeper records the decision with the lease, and refuses with `policy`, `identity_not_allowed` a lease or a sign-in on a person's session for any run that does not act for that person. A person's saved state is sealed under that owner's key, so a lease for the wrong owner could not open it anyway.
- **A lease is a snapshot, not a promise.** It names an epoch. `apply` asks the home runner for the credentials a lease of that epoch may place, and refuses with `session_expired`, `lease_stale` when the credentials held are of another epoch, or the session has ended at a later one, whatever is held. So a lease from before a renewal, an expiry, or a revoke places nothing. A driver that sees a sign of expiry asks `detect_expiry`, stops the step with `session_expired`, and the engine relays the report; a report for an epoch that is no longer the session's changes nothing.
- **An end is remembered where the credentials are.** The session's record is in the store, which the home runner's half cannot read when a lease is applied: `apply` does not wait, and the runner may be another machine. So the order of a session's changes is carried to it by epochs. `SignOut` and `Forget` name the epoch they end: the epoch of the record once it was ended, or the epoch of credentials that were never made the session's. The worker drops what it holds at that epoch or an earlier one, leaves newer credentials alone, and remembers the epoch. From then on nothing of an earlier epoch is held, by a restore that was slow above all, and no lease of an earlier epoch places anything. Work that finds the session ended, or newer credentials held, when it comes to hold what it made, holds nothing and answers `Ended`.
- **The store orders a restore, a lease, and a passed check against every change.** Each is written by one statement that names the epoch it was made for, and is not written when the session was renewed or ended meanwhile. A restore that is not written is undone: the keeper has the worker drop what it brought back. A lease that is not written is not given, and the call looks at the record again. Work whose compare-and-set is lost has what it holds dropped by the epoch it was held at, unless other work made the session valid at that very epoch meanwhile.
- **Credentials reach a driver only through `apply`.** `apply` puts the redactor in front of the driver's sink: whatever a provider places, including a value it built from its parts such as a Basic header, is registered before the sink sees it. Every secret the worker opens, receives from an exchange, or is given by a person is registered before it is held. `Material`, `SessionLease`, and `Returned` print names and `[redacted]`.
- **A provider's failure is redacted before it leaves the worker.** A provider must put no secret in a failure. The worker does not rely on it: the failure of a sign-in, a renewal, a sign-out, or an `inject` passes the vault's redactor, sentence and next step, before the keeper keeps it or a call is given it. This holds back every secret that was registered, which is every one the provider was handed or registered itself. A value nobody registered, such as text a provider copied from a page, cannot be recognized; the contract forbids it and nothing here can enforce that.
- **A person is reached by a link or a code, never by driving a browser.** A provider asks through `SignInContext::ask_person`; the worker forwards to the `AskPerson` its runner supplied. `PersonAsk::OpenLink` is answered with what the person's browser came back with at `return_address`. The keeper's own notices (`PersonAsk::Notify`) go through `KeeperWork::ask_person`. There is no `SignInSurface` type: a pack's provider that opens a window of its own holds its pack's handles from the moment the pack made it.
- **A browser session is the profile.** For providers in `a2m-pack-web-ui`, `Material` names the engine-owned profile directory by a `ProfileRef`, a name and never a path, and holds exported state only when the session is movable. The keeper never touches a browser.
- **Saved state.** When the entry and the provider both allow it, the worker seals the material under the owner's key (kind `session_state`, item the session's identifier) and the keeper stores the envelope in the same transaction as the move to `valid`. After a restart, the first lease on a `valid` session whose credentials this process does not hold restores them from the envelope, once. A session with nothing saved, or whose envelope no longer opens, moves to `expired`; a key store that does not answer is the vault's `busy` failure and changes nothing.
- **The clocks.** The declared `absolute` age ends a session on a tick or on the lease that finds it past; nothing renews around it. The declared `idle` timeout times the probe at half of it, 15 minutes when undeclared, and only while the session was used within the keep-alive window, 8 hours by default. Learned clocks and jitter are not built.

## The neutral providers, and their settings

Each is checked by `AuthProvider::check`, which refuses a setting or a credential role it does not know.

| Provider | Settings | Credential roles | Places |
| --- | --- | --- | --- |
| `api_key` | `header`; optional `prefix` | `key` | The header named, with the prefix and the key |
| `bearer_static` | none | `token` | `Authorization: Bearer` and the token |
| `http_basic` | `user` | `password` | `Authorization: Basic` and the encoded pair |
| `http_digest` | `user` | `password` | A challenge credential, scheme `digest` |
| `windows_integrated` | `user`; optional `scheme`, `negotiate` or `ntlm` | `password` | A challenge credential, scheme `negotiate` or `ntlm` |
| `client_certificate` | none | `certificate`, `key` | The client's TLS identity |
| `oauth_client_credentials` | `token_endpoint`, `client_id`; optional `scope`, `audience`, `client_auth` (`basic` or `body`) | `client_secret` | `Authorization: Bearer` and the access token |
| `oauth_authorization_code` | `authorization_endpoint`, `token_endpoint`, `client_id`; optional `scope`, `audience`, `client_auth` | `client_secret`, optional | The same |

A token endpoint and an authorization endpoint must be addresses the target declares for its surfaces in the session's environment (decision 0248; the table of every provider's addresses is there); a redirect from the token endpoint is not followed and is `unexpected_response`. A token endpoint must be an `https` address, or `http` to a loopback address written as an IP address in `127.0.0.0/8` or `::1`; a name, `localhost` included, is not one, as in the packs (decision 0192). The one function that judges it, `link::loopback`, asks `ScopedHost::may_leave_this_machine` of `a2m-core` of the host as the URL parser writes it, the question every check of a plain link asks; `::ffff:127.0.0.1` and an address with a zone are not loopback either ([decision 0240](../../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)). The authorization-code provider always uses a proof key (RFC 7636) and a state value, and exchanges nothing whose state is not its own. `totp` is not a provider of sessions: `SignInContext::one_time_code` computes the code (RFC 6238) from the role `totp_secret` and the settings `totp_digits`, `totp_period`, and `totp_algorithm`, for the provider that signs a dedicated account in.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-actions` as the in-process `ControlPort`, and `a2m-runner` on the control-plane side of the runner link | `lease`, `report`, on the engine's behalf. The engine reaches the keeper only through `ControlPort` and names nothing here but `SessionLease`, which its driver receives |
| `a2m-actions` | `Keeper::builder`, `SessionStore::new`, `sign_in` and `sign_in_by`, `status`, `timeline`, `revoke`, `tick`, `idle`; `StoredCredentials` for `secret.set`; implements `KeeperWork`, `AskPerson`, and `RunProbe`, forwarding to `PersonSurface` |
| `a2m-provider` | `AuthProvider`, `ProviderSet`, `SessionLease`, `CredentialSink`, to register providers and to hand the lease to a driver |
| `a2m-runner`, and the in-process runner in `a2m-actions` | `KeeperWorker::run`, `KeeperWorker::bind` |
| Packs | Implement `AuthProvider` and `CredentialSink`; receive `SessionLease`; build `Material` |

It uses `a2m-ledger` for its tables, `a2m-vault` to seal and open saved state and credentials under the owner's key and to register secrets with the redactor, and `a2m-logs` for the clock, the log, and the timeline's metrics.

## State

Tables, through `a2m-ledger`, made by its migration `0006`, with `sessions.attempted_with` added by `0024`: `sessions`; `session_events`, the timeline; `session_state`, the sealed saved state; `session_credentials`, the stored credentials, sealed. The statements are plain functions over the store's transaction in `src/tables.rs`. In memory on the control-plane half: the entry each session was last used under, the sessions with work in flight, and the failure of each session's last work. On the home runner: `Material` for live sessions, in memory only.

Metrics, through the logs-and-traces handle: `a2m.session.renewals`, `a2m.session.forced_sign_ins`, `a2m.session.checks_failed`, and the histogram `a2m.session.renewal.duration`.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `session_expired` | `needs_login` | `absent`, `expired`, `revoked`, a wait that ran out, a person who is wanted and has not come, or a session signed in too often without its person |
| `session_expired` | `needs_mfa` | The identity provider demands a second factor and no person answered in time |
| `session_expired` | `credential_refused` | A stored credential was refused, or its attempts are spent. It is not sent again as it is stored, by a lease or by a program that asks, until the person stores it again or signs in themself |
| `session_expired` | `lease_stale` | A lease is applied after the credentials it was granted for were replaced, or after the session was ended |
| `session_expired` | `device_required` | The lease is applied in a process that does not hold the session's credentials |
| `session_expired` | `headroom_short` | The declared clocks leave less time than the pipeline's declared headroom |
| `policy` | `identity_not_allowed` | The run does not act for the person the session belongs to |
| `input` | `auth_provider_unknown`, `auth_provider_duplicate` | An auth entry names a provider nobody registered; two providers share a name |
| `input` | `auth_settings_invalid` | An auth entry lacks a setting its provider needs, or holds one it does not know |
| `input` | `credential_missing` | A credential reference names nothing that is stored |
| `input` | `tls_not_enabled` | An `https` token endpoint in a build without the feature `tls`; named roots handed to `tls::Trust` in such a build |
| `input` | `tls_setup_failed` | No TLS configuration could be made, or `tls::install` was called after the trust was set |
| `input` | `trust_roots_unreadable` | A file of `network.trust_roots` holds no certificate, or one that cannot be read |
| `unknown_variant` | `certificate_untrusted`, `certificate_name_mismatch`, `certificate_expired`, `certificate_not_verified`, `tls_handshake_failed` | A connection whose certificate does not lead to a trusted authority, is for another name, or has expired; one refused for another reason, or by a system verifier that does not say which; a handshake that failed for a reason that is not the certificate. Nothing was sent. Given by `tls::refusal` to the session keeper and the packs |
| `transient` | `upstream_unavailable` | The identity service did not answer, or answered that it cannot serve |
| `unknown_variant` | `unexpected_response` | The identity service answered with something no rule covers |
| `unknown_variant` | `session_move_illegal` | The keeper was asked for a move that is not in the table; a fault of the keeper |
| `edge_blocked` | `challenge_page` | A pack's provider met a challenge during sign-in; it is a stop, never something to get past. Produced by the pack |
| `busy` | the vault's code | The key store is unreachable and a session must be restored or saved. The call fails at once; the saved state is kept, nothing is signed in in its place, and the next call after the store answers restores it ([decision 0352](../../decisions/0352-the-faults-of-the-kernels-own-parts-as-proven-and-what-a-verified-record-keeps.md)) |

Not produced yet: `busy`, `session_in_use`, the per-session parallelism of section 211.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Ten runs that meet an expired session cause one renewal | `ten_runs_meet_an_expired_session_and_exactly_one_renewal_happens`, held on one thread so that all ten are seen waiting; `one_renewal_also_under_real_parallelism`; and against a token endpoint in `client_credentials_are_exchanged_once_renewed_once_and_shown_nowhere`. Hypothesis H-04 |
| A sign-in sends only to a declared address, and a redirect elsewhere is not followed | `a_published_sign_in_sends_only_to_an_address_the_target_declares`, against two services, one the target declares and one it does not, which counts what it hears |
| A drafts' sign-in sends nothing until its person carries it out, and is never renewed without them | `a_sign_in_the_drafts_declare_sends_nothing_until_its_person_carries_it_out`, which also shows a published sign-in renewed without anyone; across a restart, `a_session_made_under_drafts_is_not_renewed_under_a_published_version_after_a_restart`, `a_session_made_under_a_published_version_renews_after_a_restart`, and `a_store_from_before_the_mark_opens_and_its_sessions_count_as_made_under_drafts` (decision 0374) |
| A credential opens only for the target it was stored for | `a_credential_opens_only_for_the_target_it_was_stored_for`, and the vault's `a_credential_opens_only_for_the_target_it_was_stored_for` |
| A refused stored credential is not tried again, across a restart, until the person stores it again or signs in themself | `a_refused_stored_credential_is_not_sent_again_after_a_restart_until_the_person_acts`, over a store file closed and opened again by a second keeper; `a_refused_sign_in_a_person_asked_for_is_not_tried_again_by_a_lease`; `a_refused_stored_credential_is_not_tried_again`, `a_held_credential_the_system_refuses_is_offered_once_and_no_more`, and the token endpoint's own count of requests. The test application's lockout counter is still owed |
| A sign-in whose answer nobody can read is an attempt | `a_sign_in_whose_answer_nobody_can_read_is_counted_and_not_sent_again`: by a lease, by a program that asked, and in the renewal's full sign-in |
| Sign-ins without the person are limited over time | `a_session_signed_in_again_too_often_without_its_person_waits_for_the_person`, on the keeper's manual clock |
| A closed engine's keeper is freed | `a_closed_engines_keeper_is_freed_when_its_probe_holds_it_weakly`, which also shows a strong probe keeps it |
| No transition outside the table of moves | `every_pair_of_states_is_legal_only_when_the_table_lists_it`, over all 64 pairs; every write of a state passes `legal_move` |
| No credential appears in a lease, a status, the timeline, a log, or a failure | The planted-secret tests, run against every neutral provider: `a_held_credential_reaches_only_the_drivers_client_and_appears_nowhere_else` and the two OAuth tests, which also read the keeper's log lines |
| Every secret is registered with the redactor | The same tests ask the redactor about each stored, received, and placed value |
| `AuthProvider` is called only by the keeper's two halves | `a_provider_is_called_only_by_the_keepers_two_halves`, a source check, and the layering |
| An expired session stops every call with `needs_login` | `an_expired_session_with_no_renewal_path_gives_needs_login_to_every_call` |
| A lease is refused for another person's session | `a_lease_is_refused_for_another_persons_session` |
| A revoked session's credentials are never held or placed again | `a_restore_that_ends_after_a_revoke_brings_nothing_back`, the order of the audit's finding, each step let through by the test; `a_restore_is_undone_when_the_session_moved_on_while_it_ran`, where the sign-out never reaches the worker and the keeper's own check must do it; `the_worker_keeps_what_is_newer_than_an_end_and_never_holds_what_ended`, the worker alone |
| An end that arrives late leaves newer credentials alone | `the_worker_keeps_what_is_newer_than_an_end_and_never_holds_what_ended`; `work_that_ends_after_a_revoke_keeps_nothing` for the other side: what is held at the ended epoch is dropped |
| A provider's failure carries no registered secret to a caller | `a_providers_failure_is_redacted_before_it_leaves_the_worker`, with a provider that breaks its contract |
| Saved state survives a restart and opens for its session owner alone | `saved_state_survives_a_restart_and_opens_for_its_owner_alone`, over a store file that is closed and opened again |
| `idle` and `waiting` say what is so | `a_call_returns_on_the_record_and_idle_waits_for_what_the_work_does_after_it`, `work_that_overlaps_on_one_session_keeps_it_busy_until_the_last_ends`, `a_call_that_is_cancelled_while_it_waits_is_no_longer_counted` |
| A refresh that fails ends in the right state and never loops | `a_refused_refresh_goes_to_the_person_and_is_never_tried_again`, `an_identity_service_that_is_down_fails_each_call_once_and_never_loops`, `a_second_factor_nobody_gives_is_needs_mfa` |

## Build order

1. Done: the state machine, `SessionLease`, `lease` and `report`, on the store's tables. Slice 2.4.
2. Done: `KeeperWork`, `KeeperWorker`, and `InProcessWork`. The probe as a job of the scheduler is `a2m-actions`' half, built in slice 2.4 ([decision 0353](../../decisions/0353-a-session-is-probed-by-a-keeper-job-run-through-the-engine-in-a-slot-kept-for-it.md)): a probe's lease is refused with `needs_login` outside `valid`, `suspect`, and `renewing`, and starts nothing; a probe that answers `unknown` changes no state of a valid session, and the next is due a whole interval later.
3. Not built: the first provider that needs a surface, `form_login` in `a2m-pack-web-ui`, and the sign-in window. Slice 2.3. `AuthProvider` is fixed, so it can start.
4. Done: renewal, the ladder, one renewer. Slice 2.5. Proven through the product against the test application, and with two keepers over one store, whose calls that did not renew hear of the other's renewal at their keeper's `tick` ([decision 0361](../../decisions/0361-one-renewal-for-ten-runs-through-the-product-and-another-engines-renewal-heard-at-the-tick.md)).
5. Done: the neutral providers `api_key`, `bearer_static`, `http_basic`, `http_digest`, `windows_integrated`, `client_certificate`, `oauth_client_credentials`, `oauth_authorization_code`, and one-time codes; and `recommend` (`recommend.rs`), the fixed rules that turn `sign_in_method` findings into a mode, a provider, and a reason for `auth.detect`, marked `inferred` ([decision 0367](../../decisions/0367-auth-detect-recommends-a-mode-and-a-provider-by-fixed-rules-and-marks-it-inferred.md)): thirteen signs (`SignInSign`), one row each, a question for a sign no row holds, `sso_oidc` for a move to another origin unless a `SAMLRequest` form post was seen. Not built: `obo_token`, which needs the caller's own token carried from the front door to the worker; `os_broker`, Phase 7; revoking a token at the identity service on sign-out. Slice 2.6.
6. Partly done: the declared clocks, the keep-alive window, the probe at half the idle timeout, and the end of interrupted work, in `tick`. Not built: learned clocks, jitter, sessions that end with the browser, per-session parallelism. Phase 2, group E.
7. Partly done: saved state sealed in the vault and restored after a restart. Not built: moving a session to another runner, and a home runner other than the local engine. Phases 2 and 7.
8. Done: what the safety audit of [decision 0138](../../decisions/0138-the-vault-and-the-session-keeper-after-their-safety-audit.md) found: ends remembered by epoch on the home runner, a restore and a lease ordered by the store, and provider failures redacted.
9. Done, behind the feature `tls`: the module `tls` ([record 0157](../../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)). `Trust` (the named roots, checked first by the same rules on every system, then the platform's verifier, or the bundled roots on a machine with none; TLS 1.2 and 1.3), `install` and `installed` (one trust for the process, set when the engine starts), `client_builder` (the one way an HTTP client is made), and `refusal` (the named failure of a request that failed in its handshake). Not built: reading `network.trust_roots` in the product crate, mutual TLS, a server listener.

10. Done: `link::loopback` asks the one question of a plain link, and is pinned over the shared corpus of address spellings with its old form kept beside it ([decision 0240](../../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)).

11. Done: what audit 5 found ([decision 0248](../../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)): `SignInDestinations`, `AuthProvider::destinations`, the checks before any work and before each request, the drafts' gate on sign-in and renewal, the allowances a person gave a session, credentials bound to their target; and a person's refused sign-in counted as an attempt.

12. Done: what audit 6 found in the keeper ([decision 0269](../../decisions/0269-a-stored-credentials-attempts-outlive-a-restart-and-sign-ins-without-the-person-are-limited.md)): the count of attempts kept in the record with the mark of its credential, `sign_in_by` and `SignInBy`, an unreadable answer counted, sign-ins without the person limited over time, and `WeakKeeper`. Not built: `a2m-actions` passing `SignInBy::Person` for the person's own press.

## Open points

- Settled: a renewal whose runner holds nothing at the session's epoch, after a restart or because another engine over the same store signed the session in, opens the saved state on that runner before its silent or light rung ([decision 0403](../../decisions/0403-a-renewal-opens-the-saved-state-first-when-its-runner-holds-nothing.md)). For a home runner other than the local engine, which is not built, the keeper cannot see what the runner holds.
- How reliably a fresh engine-owned profile signs in silently through the device's account. Research brief 30; hypothesis H-05.
- Settled: there is no `SignInSurface` type. The worker is handed an `AskPerson`, and a pack's provider holds its own pack's handles (record 0085).
- Where the authorization response of `oauth_authorization_code` is received on a server, where no loopback address reaches the person's browser: the console's address, most likely. Here it is whatever `AskPerson::return_address` gives.
- Settled: the shape of the auth file's provider settings for each neutral provider is the table above (record 0085). `a2m-files` must carry a `settings` map and a `credentials` map of role to reference in an `auth.yaml` entry.
- A failure to reach the identity service during a silent renewal costs a person's session a new sign-in, because the rung is not tried twice. Whether one later retry is safe is open.
- `is_signed_in` is part of the contract and the keeper does not call it yet; the stored expiry is what it reads.
- Settled: `tls::client_builder` takes no proxy from the environment: it returns a builder with `.no_proxy()`, so `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and the system's settings are never read, and a token exchange with a loopback identity service cannot be sent in the clear to another machine. The source rule `no_http_client_of_the_workspace_takes_a_proxy_from_the_environment` holds both helpers to it and refuses any `reqwest` proxy in the workspace; the test `a_token_exchange_never_goes_to_a_proxy_the_environment_names` proves it with a listener standing in as the proxy ([record 0197](../../decisions/0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md)). A proxy a person names would be a protected setting passed to the helper as an argument and shown by `doctor.run`; it is not built.
- Settled: switching the feature `tls` on needs no provider installed for the process. Every HTTP client of this crate and of the HTTP and MCP packs is made by `tls::client_builder`, whose configuration states ring; a source test holds the three crates to it ([record 0157](../../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)). A client made any other way in a build with the feature on still stops the process when it is made: the product crate's own test clients are such a case until they use the function.
- **A sign-out that does not reach the home runner leaves its credentials there.** The record is ended, so no new lease is given, and a restore that follows is undone; but a lease given before the end still places what the runner holds until a `SignOut` or a `Forget` reaches it. The keeper logs it. Sending the end again until the runner answers belongs with the job that carries the keeper's work to a runner, which `a2m-actions` has not built.
- **Two pieces of work can hold at one epoch:** a light prompt that is still running when the person starts a sign-in of their own. Both made genuine credentials of the same session. The later one to hold stays in memory; the one whose compare-and-set won is what is saved. If the loser's drop arrives between the winner's hold and its compare-and-set, the winner's credentials are dropped with it, and the next lease restores them from the saved state or, for a session that is not movable, finds it expired.
- **What the home runner remembers of an end is in memory.** After a restart it holds no credentials either, so nothing is lost by that; a restore after the restart is ordered by the store alone.
