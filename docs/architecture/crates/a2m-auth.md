# `a2m-auth`

The session keeper and the auth providers. The keeper is the only code that signs in, checks, renews, and ends a session; it hands the engine a `SessionLease` and never hands anyone a credential. The neutral providers live here; providers that must drive a surface live in the pack that owns the surface.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 4, Services |
| May depend on | Layers 1 to 3 |
| Depends on | `a2m-core`; `a2m-vault` to seal saved state; `a2m-ledger` for its tables; `a2m-telemetry`. Outside: an HTTP client for identity services' token endpoints, and OAuth and certificate crates, each to be verified |
| Built in | Phase 2: slices 2.3 to 2.6, groups C, D, E, H. Phase 7: moving sessions between runners, the operating system's broker, hosted sign-in |
| Carries out | Spec sections [15](../../spec/15-identity-sign-in-and-sessions.md), [16](../../spec/16-identity-on-a-shared-server.md), [44](../../spec/44-session-keeper.md), [49](../../spec/49-approval-and-sign-in-surfaces.md) (sign-in), [52](../../spec/52-when-the-engine-itself-fails.md) (the key store unreachable); the [auth provider interface](../../reference/technical-reference.md#auth-provider-interface); decisions 0030, 0032, and 0035 |

## Owns

- The session state machine: the eight states of section 44, and every transition between them.
- The three clocks: the declared ones, and the ones it learns from each session's history.
- When to check, when to renew, when to ask a person, and when to let a session lapse.
- One renewal per session at a time, and a stored credential tried once.
- `SessionLease`, `KeeperWork`, `AuthProvider`, and `CredentialSink`.
- The neutral providers, and the fixed rules that turn sign-in findings into a recommended mode and provider for `auth.detect`.
- The session timeline.

It must never: call a crate above it; read, copy, or decrypt a cookie store it does not own; return a credential to any caller; decide who may use a session (that is `ActAs` in `a2m-policy`); retry a refused stored credential; try to outlast the identity provider's maximum age; run the target's probe itself.

## Where each provider lives

Section 55 left this open, and package A7 of the long-term plan was to settle it. It is settled: **a provider lives in `a2m-auth` when it only holds or exchanges a credential. It lives in a pack when it needs a surface of the system.**

| Provider | Lives in | Why |
| --- | --- | --- |
| `api_key`, `bearer_static`, `http_basic` | `a2m-auth` | A stored value placed in a header |
| `http_digest`, `windows_integrated` | `a2m-auth` | The provider holds the credential and injects it; carrying out the challenge and response on the connection is the HTTP driver's work |
| `client_certificate` | `a2m-auth` | A certificate and key placed in the client's TLS identity |
| `oauth_client_credentials`, `obo_token` | `a2m-auth` | A token exchange with the identity service, not with the system |
| `oauth_authorization_code` | `a2m-auth` | A token exchange; the person's consent is asked through `PersonSurface` |
| `totp` | `a2m-auth` | Computes a code from a stored secret; a pack's provider enters it |
| `os_broker` | `a2m-auth`, per operating system | The operating system's own interface. User devices only; Phase 7 |
| `form_login`, `sso_oidc`, `sso_saml` | `a2m-pack-web-ui` | A person, or a stored credential, works through pages in the engine's own browser |
| `upstream_mcp` | `a2m-pack-mcp` | The upstream server's own authorization discovery; consent through `PersonSurface` |

**Consent in a browser, outside the web pack.** A provider that needs a person to agree in a browser, `oauth_authorization_code` or `upstream_mcp`, asks through `PersonSurface`, which `a2m-actions` defines and `a2m-local` and `a2m-api` implement. The person is given a link and opens it in their own browser. Nothing drives that browser, and no pack borrows another pack's browser. The keeper reaches `PersonSurface` through `KeeperWork::ask_person`, not through a handle of its own, so that the keeper has one way out and that way never calls upward. The provider receives the authorization response through `SignInContext`, which the worker fills when the surface reports the person's answer.

## Interface

```rust
pub struct SessionKey { pub workspace: WorkspaceId, pub target: TargetName, pub environment: Environment, pub identity: Identity }

pub struct Keeper { /* states; timers; one renewal slot per session; providers; work; store; vault */ }
impl Keeper {
    pub fn new(store: Store, vault: Arc<dyn Seal>, providers: ProviderSet, work: Arc<dyn KeeperWork>) -> Keeper;
    /// The engine's only way to a session, through Control. Waits on `signing_in`, `suspect`,
    /// `renewing`, and `needs_person` up to the request's wait; otherwise `needs_login`.
    pub async fn lease(&self, req: LeaseRequest) -> Result<SessionLease, Failure>;
    /// A driver's sign of expiry, relayed by the engine. Never acted on by the driver.
    pub async fn report(&self, lease: &SessionLease, sign: ExpirySign);
    pub async fn sign_in(&self, key: &SessionKey, person: PersonRef) -> Result<SignInTicket, Failure>; // session.login
    pub async fn status(&self, who: &PrincipalRef) -> Result<Vec<SessionStatus>, Failure>;               // session.status
    pub async fn revoke(&self, key: &SessionKey, by: &PrincipalRef) -> Result<(), Failure>;              // session.revoke
    pub async fn tick(&self, now: Instant);   // timers: probes due, windows ended, warnings of the third clock
}

pub struct LeaseRequest {
    pub key: SessionKey, pub run: RunId,
    pub decided: DecisionId,          // the ActAs decision a2m-actions obtained; recorded, not re-made
    pub wait: Wait,                   // Interactive(5 min by default) | NoWait for pipelines
    pub headroom: Option<Duration>,   // a pipeline's declared headroom
    pub purpose: Purpose,             // Run | Probe
}

/// A session as one run may use it. Carries no secret and cannot print one.
pub struct SessionLease { /* session id, epoch, state at grant, home runner, provider, granted_at */ }
impl SessionLease {
    pub fn session(&self) -> SessionId;  pub fn epoch(&self) -> u64;
    /// The engine-owned profile directory, for a session held in a browser. None otherwise.
    pub fn profile(&self) -> Option<&ProfileRef>;
    /// Places the session's credentials into the driver's client, through the provider's `inject`.
    pub fn apply(&self, sink: &mut dyn CredentialSink) -> Result<(), Failure>;
}

/// Implemented by a pack's client: an HTTP request builder, a connection, a process environment.
pub trait CredentialSink: Send {
    fn header(&mut self, name: &str, value: &Secret);
    fn cookie(&mut self, origin: &str, name: &str, value: &Secret);
    fn tls_identity(&mut self, cert: &[u8], key: &Secret);
    /// For challenge-and-response schemes: the driver carries out the scheme with this credential.
    fn challenge_credential(&mut self, scheme: ChallengeScheme, user: &str, secret: &Secret);
}

/// The keeper's work, run on the session's home runner. a2m-actions implements it by submitting a job.
pub trait KeeperWork: Send + Sync {
    async fn probe(&self, s: &SessionRef) -> ProbeOutcome;          // runs the target's declared probe as a read
    async fn perform(&self, s: &SessionRef, w: Work) -> WorkOutcome; // Renew(Rung) | SignIn(Window) | SignOut | Export
    /// A sign-in or consent request to the session's own person, forwarded to PersonSurface.
    async fn ask_person(&self, s: &SessionRef, ask: PersonAsk) -> PersonAnswer; // a link to open; a code to enter
}
pub enum Rung { Silent, LightPrompt, FullSignIn }

/// The other half of the keeper, on the home runner. The runner calls it for a job of kind
/// `session_keepalive`; it is the only caller of an AuthProvider.
pub struct KeeperWorker { /* providers; vault */ }
impl KeeperWorker { pub async fn run(&self, job: KeeperJob, surface: &mut dyn SignInSurface) -> WorkOutcome; }

pub trait AuthProvider: Send + Sync {
    fn name(&self) -> ProviderName;
    fn needs(&self) -> ProviderNeeds;   // a surface or none; whether it can renew silently; whether its state can move
    async fn sign_in(&self, cx: &mut SignInContext<'_>) -> Result<Material, Failure>;
    async fn is_signed_in(&self, m: &Material) -> SignedIn;         // the provider's own view: a token's expiry
    fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign>;
    async fn refresh(&self, m: &Material) -> Result<Material, Failure>;
    async fn sign_out(&self, m: &Material) -> Result<(), Failure>;
    fn inject(&self, m: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure>;
}
/// A session's credentials. Holds `Secret`s; no Debug that shows them; serialized only to be sealed.
pub struct Material { /* opaque */ }

/// auth.detect: fixed rules from sign-in findings to a recommended mode and provider, with reasons.
pub fn recommend(findings: &[Finding], available: &ProviderSet) -> Recommendation;
```

Contracts that the signatures do not show:

- **The keeper never calls upward.** A check or a renewal that needs the target, or a window on the home runner, goes through `KeeperWork`. `a2m-actions` turns it into a job for `a2m-sched`, with the session's home runner as a requirement. The runner runs it: a probe as an ordinary read through the engine, with a lease of purpose `Probe`, which the keeper grants in `valid`, `suspect`, and `renewing`; a renewal or sign-in by calling `KeeperWorker`. In one process both halves are the same code.
- **One renewer.** A renewal starts only when the keeper moves the session's row from `suspect` or `valid` to `renewing` by a compare-and-set on its epoch, in a `Tx`. The one caller that wins submits the work; everyone else waits on the outcome. This holds across processes, because the row is in the store.
- **The ladder is climbed once.** Within one renewal, each rung is tried at most once, in order. A stored credential counts against the auth file's `attempt_limit`, default 1; when refused, the session goes to `needs_person` and an alert is raised.
- **The keeper is told the identity, it does not choose it.** `LeaseRequest` carries the `ActAs` decision that `a2m-actions` obtained from `a2m-policy`. The keeper records it with the lease and also refuses a lease whose identity is not the session's. A person's saved state is sealed under that owner's key, so a lease for the wrong owner could not open it anyway.
- **A lease is a snapshot, not a promise.** It is valid while the session's epoch is unchanged. A driver that sees a sign of expiry reports it and stops the step with `session_expired`; the engine relays the report and waits for the one renewal. Reads and idempotent writes are retried once after it; nothing else is.
- **Credentials reach a driver only through `apply`.** Every `Secret` the keeper opens is registered with the redactor before `inject` places it. `Material` and `SessionLease` print as `[redacted]`.
- **A browser session is the profile.** For providers in `a2m-pack-web-ui`, `Material` names the engine-owned profile directory and holds exported state only when the session is movable. The keeper never touches a browser; the pack does, behind `SignInSurface`, which the runner supplies.
- **The clocks are learned by rule.** For each session the keeper records how long it lasted and what ended it. A clock not declared in the auth file is estimated as the shortest lifetime seen for that target and identity with the same ending, and probes are timed at half the idle estimate, 15 minutes by default, with jitter. No model, no fitting.
- **Only while wanted.** After the keep-alive window since last use, 8 hours by default on one machine, the keeper stops probing and lets the session lapse.
- **`recommend` is mechanical.** Packs' discoverers return findings of kind "sign-in method": a redirect to an identity host, a challenge header, a request for a client certificate, a token's audience decoded locally. `recommend` maps them through a fixed table to a mode, a provider that is enabled, and the reasons. Where no row matches, it opens a question in the unknowns ledger instead of guessing.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-actions` as the in-process `Control`, and `a2m-runner` on the control-plane side of the runner link | `lease`, `report`, on the engine's behalf. The engine reaches the keeper only through `Control` and names nothing here but `SessionLease`, which its driver receives |
| `a2m-actions` | `Keeper::new`, `sign_in`, `status`, `revoke`, `tick`, `recommend` for `auth.detect`; implements `KeeperWork`, forwarding `ask_person` to `PersonSurface` |
| `a2m-provider` | `AuthProvider`, `SessionLease`, `CredentialSink`, to register providers and to hand the lease to a driver |
| `a2m-runner`, and the in-process runner in `a2m-actions` | `KeeperWorker::run` |
| Packs | Implement `AuthProvider` and `CredentialSink`; receive `SessionLease` |

It uses `a2m-ledger` for its tables, `a2m-vault` to seal and open saved state under the owner's key, and `a2m-telemetry` for the timeline's metrics.

## State

Tables, through `a2m-ledger`: `sessions` as in the ledger schema; `session_events`, the timeline; `session_state`, the sealed saved state. In memory on the control-plane half: timers and one renewal slot per session. On the home runner: `Material` for live sessions, in memory only.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `session_expired` | `needs_login` | `absent`, `expired`, `revoked`, or a wait that ran out |
| `session_expired` | `needs_mfa` | The identity provider demands a second factor and no person answered in time |
| `session_expired` | `device_required` | The session is not movable and its home runner is not reachable |
| `session_expired` | `headroom_short` (new) | The learned clocks leave less time than the pipeline's declared headroom |
| `busy` | `session_in_use` | The session's parallelism is used up |
| `edge_blocked` | `challenge_page` | A pack's provider met a challenge during sign-in; it is a stop, never something to get past |
| `busy` | the vault's code | The key store is unreachable and a session must be restored or saved |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Ten runs that meet an expired session cause one renewal | The keeper's ten-runs-one-renewal test, slice 2.5; hypothesis H-04 |
| A refused stored credential is not tried again | A test with the test application's lockout counter |
| No transition outside section 44's table | A table-driven test over every pair of states |
| No credential appears in a lease, a status, the timeline, a log, or a failure | The planted-secret test of slice 2.1, run against every provider here |
| `AuthProvider` is called only by the keeper's two halves | A source check, and the layering: no crate below the contract can name a pack's provider |
| An expired session stops every call with `needs_login` | Slice 2.4 |

## Build order

1. The state machine, `SessionLease`, `lease` and `report`, with an in-memory stand-in for the store. Slice 2.4.
2. `KeeperWork` and the probe as a job, with the in-process runner. Slice 2.4.
3. The first provider, `form_login` in `a2m-pack-web-ui`, and the sign-in window. Slice 2.3, which can start in parallel with step 1 once `AuthProvider` is fixed.
4. Renewal, the ladder, one renewer. Slice 2.5.
5. The neutral providers, then `recommend` for `auth.detect`. Slice 2.6.
6. Learned clocks, the keep-alive window, sessions that end with the browser. Phase 2, group E.
7. Saved state in the vault, and moving it. Phases 2 and 7.

## Open points

- How reliably a fresh engine-owned profile signs in silently through the device's account. Research brief 30; hypothesis H-05.
- Whether `SignInSurface` is a type here or in `a2m-provider`. It is here because the keeper's worker needs it; the contract crate may re-export it.
- Where the authorization response of `oauth_authorization_code` is received on a server, where no loopback address reaches the person's browser: the console's address, most likely.
- The shape of the auth file's provider settings for each neutral provider.
