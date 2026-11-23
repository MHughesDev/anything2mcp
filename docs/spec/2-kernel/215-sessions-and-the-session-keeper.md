# 215. Sessions and the session keeper

The session keeper is the part of the kernel that owns every signed-in state. It creates sessions, checks them, renews them, shares them between runs, and ends them. No driver, run, or caller ever signs in, refreshes a token, or touches a credential itself. The aim is one session kept alive for every caller allowed to use it, so that a person approves a second factor only as often as their organization's policy requires, not once per call. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

The keeper lives in `a2m-auth`, with the auth providers that only hold or exchange a credential ([crate design](../../architecture/crates/a2m-auth.md)). How a session of one kind is made and renewed is the knowledge of the pack that knows that kind of system ([section 202](202-kernel-and-packs.md)). The browser-specific rules are the web pack's and are collected at the end of this section, under "Browser sessions in the web pack".

## Whose account, and who may use it

**The engine acts as the user, in the user's own account on the system.** What it uses is the person's account. What it never uses is a program's everyday store of that person's sign-ins: not their everyday browser profile, and not another program's own sign-in folder. The person signs in to their account in a place the engine owns, once, and from then on the engine works in that account with that person's permissions.

**A user's session belongs to that user alone** ([decision 0035](../../decisions/0035-a-user-s-session-is-used-only-by-that-user-and-agents-acting-for-them.md), an owner decision). A session signed in as a person can be used only by that person and by agents acting for that person. An agent counts as acting for a person in exactly three cases:

| Case | How the link is proved |
| --- | --- |
| The agent runs on the person's own machine | It reaches the local engine as that operating-system user ([section 222](222-front-doors-and-the-local-engine.md)) |
| The agent connects to a server with the person's sign-in | Its token was issued to that person by the identity provider, with their consent, and names them as the subject |
| The agent has its own identity | The person has delegated to it: an explicit, revocable record naming the agent, the toolset, and an expiry, created while the person was signed in (`delegation.grant`) |

Nobody else can use the session: not another user, not another user's agent, not a service account, and not an administrator. An administrator can end a person's session (`session.end`) but cannot run anything through it. The audit log records the person and, where there is one, the agent that acted for them.

On a shared server this is enforced in three places. The policy engine refuses any run whose resolved identity is not the caller's own; this is a built-in forbid that cannot be configured away ([section 212](212-policy-and-approvals.md)). The scheduler hands a session only to runs carrying its owner. The session's saved state is sealed under a key for that owner, so a lease for the wrong owner could not open it anyway ([section 216](216-the-vault-and-its-keys.md)).

**The keeper is told the identity; it does not choose it.** Policy picks the identity, never the caller: a call cannot name the account it runs as. The request for a lease carries the decision `a2m-policy` made about whom the run acts as. The keeper records that decision with the lease, and also refuses a lease whose identity is not the session's. The four identity modes a server offers, and how to choose between them, are in [section 602](../6-running-it/602-identity-on-a-shared-server.md).

## What a session is

A session is the signed-in state of one identity on one target in one environment. It is keyed by workspace, target, environment, and identity. Its live state depends on the kind of system, and the pack that knows the kind holds it behind its driver; the kernel never holds a handle to one.

| Kind of live state | Pack | Where it lives |
| --- | --- | --- |
| A browser profile with cookies and page storage | Web ([section 301](../3-packs/301-the-web-pack.md)) | A data directory the engine owns, one per identity, on the session's home runner |
| A token set: access and refresh tokens | Any pack that speaks to an interface with tokens, such as the HTTP pack | In memory on the home runner; sealed in the vault if the session may be saved |
| A credential and a small pool of open connections | Data ([section 307](../3-packs/307-the-data-pack.md)) | Held by the driver on the home runner. Runs on one identity share the pool; each step holds one connection for one transaction |
| A signed-in connection to a broker | Stream ([section 309](../3-packs/309-the-stream-pack.md)) | A long-lived connection on the home runner, shared by the runs of the session; each run has its own producer or subscription |
| A program's own sign-in folder | Command ([section 306](../3-packs/306-the-command-pack.md)) | A folder the engine owns for the session, `{session.state_dir}`, given to the program exactly as a browser is given an engine-owned profile. The program's everyday folder is never used |
| An open link to a device | Device ([section 310](../3-packs/310-the-device-pack.md)) | The link itself, held on the home runner through the supervisor ([section 204](204-supervising-subsystems.md)) |
| A desktop application's own sign-in | Desktop ([section 305](../3-packs/305-the-desktop-pack.md)) | Inside that application, in the runner's desktop session |
| Stored credentials for a service account | Any | The vault, or an outside secrets manager, by reference |

**Every session has a home:** the local engine, or one runner. The home runner holds the live state; the control plane knows its status.

**A lease is how a run reaches a session.** The engine asks for a `SessionLease` through `Control` before prepare, and its driver receives it. A lease carries no secret and cannot print one. It names the session, the session's epoch, the state at grant, and, for a session held in a browser, the engine-owned profile. Credentials reach a driver only through the lease's `apply`, which calls the provider's `inject` to place them into the driver's client: a header, a cookie, a TLS identity, or a credential for a challenge-and-response scheme that the driver then carries out on its connection. Every secret the keeper opens is registered with the redactor before `inject` places it.

**A lease is a snapshot, not a promise.** It is valid while the session's epoch is unchanged and the session has not been ended. The home runner remembers an end by the epoch it named, so credentials that reach it afterwards for an earlier epoch, by a restore that was slow, are neither held nor placed ([decision 0138](../../decisions/0138-the-vault-and-the-session-keeper-after-their-safety-audit.md)). A driver that sees a sign of expiry reports it and stops the step with `session_expired`. The engine relays the report and waits for the one renewal.

### A session with no identity

Some surfaces have no sign-in at all. Most programs need none; many fieldbus protocols have none; a file in an allowed root has none. A surface with no sign-in still gets a lease. The lease is empty, not a special case, so the engine and the driver follow one path for every surface.

Where there is no identity, nothing proves who is at the other end, and the weight falls elsewhere. For an open link on a fieldbus, consent carries it: the person names the device in the target's consent record ([section 213](213-effects-hazards-and-consent.md)), the surface is declared, and nothing else is reachable. The device is identified by what it reports before any frame that is not identification. The link is still a session's live state: one link, one session, held on the home runner, and used by one run at a time through a lock the scheduler grants. A second run waits or gets `session_in_use`.

## Ways of signing in

Each entry of a target's `auth.yaml` covers a group of surfaces that share a sign-in, and names a sign-in mode, an auth provider, and a probe ([file formats](../../reference/file-formats.md)).

| Mode | What it uses | Typical case |
| --- | --- | --- |
| `use_existing` | What the device already has: the operating system's sign-in broker for systems that take tokens (Windows Web Account Manager on machines joined to Entra ID, the macOS Enterprise SSO extension, the Linux identity broker where installed); or, in the web pack, the engine's own browser profile signing in silently through the device's account where the browser supports it. Never a program's everyday sign-in store | A managed work laptop |
| `managed_session` | A person signs in once, in a place the engine owns; the session is sealed and kept alive | Systems with their own sign-in, or no sign-in from the device |
| `custom` | Dedicated credentials: an API key, OAuth client credentials, a client certificate, Basic, NTLM or Kerberos, a database password, or a dedicated account's password and one-time-code secret where IT allows it. Vault references only | Service accounts, shared servers, interfaces with fixed keys |

**Tokens.** The keeper hands drivers short-lived tokens, in memory only. The system never imitates another application's client to obtain tokens. Where a direct token is wanted, the path is an app registration that IT approves, with delegated permission to the target's interface, so that tokens carry the user's own rights and appear openly in sign-in logs.

## Auth providers

An auth provider knows how to sign in to one kind of thing, check that the sign-in still holds, recognize a sign of expiry, refresh, sign out, and place the credential into a driver's client. The interface and the full table of providers are in the [technical reference](../../reference/technical-reference.md#auth-provider-interface). Only the session keeper calls a provider.

**Where a provider lives.** A provider lives in `a2m-auth` when it only holds or exchanges a credential. It lives in a pack when it needs a surface of the system.

| Provider | Lives in | Why |
| --- | --- | --- |
| `api_key`, `bearer_static`, `http_basic` | `a2m-auth` | A stored value placed in a header |
| `http_digest`, `windows_integrated` | `a2m-auth` | The provider holds the credential; carrying out the challenge and response on the connection is the HTTP driver's work |
| `client_certificate` | `a2m-auth` | A certificate and key placed in the client's TLS identity |
| `oauth_client_credentials`, `obo_token`, `oauth_authorization_code` | `a2m-auth` | A token exchange with the identity service, not with the system |
| `totp` | `a2m-auth` | Computes a code from a stored secret; a pack's provider enters it |
| `os_broker` | `a2m-auth`, per operating system | The operating system's own interface; user devices only |
| `form_login`, `sso_oidc`, `sso_saml` | `a2m-pack-web-ui` | A person, or a stored credential, works through pages in the engine's own browser |
| `upstream_mcp` | `a2m-pack-mcp` | The upstream server's own authorization discovery |

Further packs bring their own where a credential is bound to their kind of connection: the data pack brings a database password provider, and the stream pack a certificate provider if the neutral set lacks one.

**Consent in a browser, outside the web pack, is a link the person opens.** A provider that needs a person to agree in a browser and is not in the web pack, such as `oauth_authorization_code` or `upstream_mcp`, gives the person a link. The person opens it in their own browser. Nothing drives that browser, and no pack borrows another pack's browser. The keeper reaches the person only through its work interface, which `a2m-actions` forwards to the role's person surface ([section 503](../5-security/503-approval-and-sign-in-surfaces.md)). The provider learns the outcome by its own protocol.

**Finding out how a target signs in.** `auth.detect`, which the `add` shortcut runs, works out the method and recommends a mode and a provider, with reasons, for a person to confirm. Packs' discoverers return findings of the kind "sign-in method" ([section 218](218-discovery-and-findings.md)). They look for:

- redirects to known identity hosts, such as those of Microsoft, Okta, Google, ADFS, and Ping, and the provider's OpenID discovery document;
- `WWW-Authenticate` challenges for Basic, NTLM, Negotiate, or Bearer;
- a request for a client certificate;
- in captured traffic, whether the system's backend calls carry a cookie or an `Authorization: Bearer` token; the token's audience and scopes, decoded locally and never sent anywhere; and how the front end renews it;
- whether the device has an operating-system broker that can issue a token for that audience to a registered client.
- a redirect that carries a `SAMLRequest`, or one that carries `response_type` and `client_id` (OpenID Connect where the origin serves a discovery document, plain OAuth where it does not);
- a redirect to an identity-provider origin the target declares, or a sign-in screen the target declares.

Detection observes addresses, redirects, and the first response's headers, with the traffic finding above; it never reads a sign-in page's fields ([decision 0254](../../decisions/0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)). A passkey, a link sent by mail, and a second factor do not show in addresses, so detection names them unknown and never guesses them; a person says which it is. Several methods look alike from outside, OAuth and OpenID Connect among them, so a finding is evidence for a recommendation, never a settled method ([decision 0321](../../decisions/0321-a-sign-in-method-is-found-from-addresses-and-a-passkey-or-a-mailed-link-is-the-persons.md), [research note 35](../../research/35-web-sign-in-methods-and-engine-owned-profiles.md)).

The recommendation is mechanical: a fixed table maps findings to a mode, a provider that is enabled, and the reasons. Where no row matches, it opens a question in the unknowns ledger instead of guessing.

## Where the keeper runs

The keeper is one component with two halves. Its decisions are made in the control plane: which sessions are wanted, when to check, when to ask a person, and who may use a session. Its work runs as jobs on the session's home runner: the probe, a renewal, a sign-in. In a single process the two halves are the same code calling itself.

**The keeper never calls upward.** A check or a renewal that needs the target, or a sign-in on the home runner, goes through an interface the keeper is handed, `KeeperWork`. `a2m-actions` turns each request into a job for the scheduler, with the session's home runner as a requirement. The runner runs a probe as an ordinary read through the engine, with a lease of purpose "probe", which the keeper grants in `valid`, `suspect`, and `renewing`; it runs a renewal or a sign-in through the keeper's worker half, the only caller of an auth provider. Keeper jobs have a reserved slot, so a run waiting for a renewal can never hold the only slot the renewal needs.

## States

| State | Meaning | Calls for this session |
| --- | --- | --- |
| `absent` | Never signed in, or ended | Return `needs_login` |
| `signing_in` | A person or a stored credential is signing in | Wait |
| `valid` | Signed in and recently confirmed | Run |
| `suspect` | One check failed | Wait while it is re-checked once |
| `renewing` | Silent renewal in progress | Wait |
| `needs_person` | The identity provider demands a person: a second factor, a new password, a consent | Wait up to the sign-in wait, then `needs_login` |
| `expired` | Ended and could not be renewed | Return `needs_login` |
| `revoked` | Ended on purpose | Return `needs_login`; nothing is retried |

No transition outside this table is allowed; a table-driven test over every pair of states holds it. The legal moves between the states are listed in the [crate design](../../architecture/crates/a2m-auth.md). A session whose provider works from a stored credential alone is signed in by the first call that asks, within the attempt limit, when a published version declares its sign-in; a provider that needs a person is never started by a call ([decision 0085](../../decisions/0085-the-session-keepers-interface-as-building-it-settled-it.md)).

**A sign-in goes only where a published version says** ([decision 0248](../../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)). Every address a provider sends a credential, a token, or the person to is checked by the keeper before any request against the addresses the target declares for its surfaces in the session's environment, and a redirect is not followed. A sign-in that only the target's drafts declare uses no stored credential and renews nothing without the person: `session.login` shows the person every address first, marked as not published, and their act allows that one sign-in. A credential belongs to the target it was stored for: its reference starts `vault://<target>/`, and the vault seals it with the target in its context.

## Three clocks, only two of which can be extended

| Clock | Example | What the keeper can do |
| --- | --- | --- |
| The system's idle timeout | Signed out after 30 minutes without activity | Prevent it with a cheap read before it runs out |
| Token or cookie expiry | An access token valid for an hour | Renew silently, the way the system's own client would |
| The identity provider's maximum age | A rule that forces a fresh sign-in every so often | Nothing. A person must sign in again |

The third clock is set by the organization, not the system. Under Microsoft Entra, for example, how long a browser stays signed in is governed by Conditional Access session controls: sign-in frequency sets a maximum age, and a separate control decides whether the session survives closing the browser ([research note 14](../../research/14-sessions-and-policy-engines.md)). The keeper never tries to get around either.

**The keeper learns the clocks, by rule.** A target's auth file can declare its `idle` and `absolute` timeouts. Where it does not, the keeper records how long each session lasted and what ended it, and estimates each clock as the shortest lifetime seen for that target and identity with the same ending. No model and no fitting is involved. It uses the estimates to:

- time its checks;
- tell the person ahead of time when a sign-in will be needed again;
- hold back a long pipeline that would outlast the session, when the pipeline asks for headroom. Such a pipeline stops at once with `headroom_short` instead of failing halfway.

## Checks

- **The probe.** Each target declares one cheap signed-in read in its auth file. The keeper runs it at half the idle timeout, declared or learned, 15 minutes by default, with jitter.
- **Only while wanted.** A session is kept alive for a window after its last use: 8 hours by default on one machine, set per toolset on servers. After that the keeper stops checking and lets it lapse. Standing access nobody is using is a risk with no benefit.
- **Signs of expiry.** A redirect to an identity provider's host, a 401 or 403 that matches the target's declared pattern, a sign-in form appearing, a token past its expiry, a challenge demanding fresh claims, an authorization refusal on a connection that worked a moment ago, or an exit a command target declares as `session_expired`. A target's auth file lists its `expiry_signals`. Every driver reports these to the keeper instead of acting on them.

## Renewal

The keeper climbs a ladder and never repeats a rung:

1. **Silent.** Refresh the token, ask the operating system's broker, or let the system's own client renew, as the provider knows how.
2. **A light prompt.** The person approves a push notification or enters a code.
3. **A full sign-in.** The person signs in again.

**One renewal at a time.** Only the keeper renews, and only once per session however many runs noticed the expiry ([decision 0032](../../decisions/0032-one-live-session-per-identity-shared-by-tabs-with-a-single-renewer.md)). Two simultaneous refreshes with a rotating refresh token invalidate each other; a single renewer makes that impossible. A renewal starts only when the keeper moves the session's row from `valid` or `suspect` to `renewing` by a compare-and-set on its epoch, in one transaction. The one caller that wins submits the work; everyone else waits on the outcome. This holds across processes, because the row is in the store. Runs that hit an expired session wait for the one renewal, then retry once if they are reads or idempotent writes. Nothing else is retried.

**No loops, no lockouts.** A stored credential is tried at most `attempt_limit` times, 1 by default, counted until the other side confirms it: a successful exchange, or a passed probe. If it is refused, the session goes to `needs_person` and an alert is raised. Repeated automatic attempts are how service accounts get locked out. The count is kept in the session's record against the credential as it is stored, so a restart does not clear it, and only the person storing the credential again or signing in themself does; a sign-in that ends in an answer nobody can read counts; and a session is signed in without its person at most four times in an hour before it waits for the person ([decision 0269](../../decisions/0269-a-stored-credentials-attempts-outlive-a-restart-and-sign-ins-without-the-person-are-limited.md)).

## Asking a person

- Only the session's own identity is asked: through URL-mode elicitation in their MCP client, a system notification, the approval page, or a chat message. The surfaces themselves are in [section 503](../5-security/503-approval-and-sign-in-surfaces.md).
- Calls wait in the queue instead of failing. Interactive calls wait up to 5 minutes by default. Pipelines stop as `needs_login` and resume after the sign-in.
- Where the person signs in depends on the provider and the deployment:

| Where the session lives | Where the person signs in |
| --- | --- |
| The local engine, with a provider in the web pack | A visible window of the engine's own browser |
| The local engine, with a provider outside the web pack that needs consent | A link the person opens in their own browser |
| A runner on the person's own computer | The same, on that computer |
| A hosted user session on a server | A sign-in page that shows the server runner's browser; see [section 602](../6-running-it/602-identity-on-a-shared-server.md) |
| A service account | Nobody; stored credentials, within the attempt limit |

Where a person signs in themself in a window of the engine's own browser, the engine never asks for that password in its own interface, and learns that the sign-in finished from a declared sign, without seeing the credentials ([decision 0254](../../decisions/0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)). Where a web application's sign-in page is filled by `form_login`, the username and password are a stored credential like any other ([decision 0245](../../decisions/0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md)). A credential that a provider holds or exchanges, such as a token or a key, is typed by the person on the engine's own page of the person's act `session.login`, or stored beforehand with `secret.set`; it is sealed for that person alone and never reaches a caller, and a one-time code a provider asks for is typed on the same page, for that one sign-in, and kept nowhere ([decision 0205](../../decisions/0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md)).

## Saving and moving a session

- **Saved state.** For a movable session, the keeper exports the session's state (in the web pack, the cookies of the application's origins; page storage is not yet exported) and seals it in the vault under the owner's key. It restores the state after a restart or on another runner.
- **Where session secrets live** ([decision 0030](../../decisions/0030-where-session-secrets-live.md)). A session on a user's device never leaves it; the control plane knows only its status. A hosted user session is sealed in the vault per user and sent to a server runner wrapped to that runner's key, only for a job it holds, and kept in memory there ([section 223](223-the-runner-protocol.md)). A service account's credentials are fetched by the runner from a secrets manager by reference, where one exists.
- **Not movable.** A session is marked not movable when it is on a user's device, when the organization requires a managed device, when the session is bound to the device's hardware, or when the auth file sets `movable: false`. Where Conditional Access binds sessions to a managed device, the keeper runs on that device and does not copy the session elsewhere.
- **When a home runner is lost.** A movable session is restored elsewhere. A session that is not movable is gone, and its owner signs in again on the new runner; a run that needs it meanwhile stops with `device_required`. This is deliberate: those protections exist to stop exactly this kind of copying.

## Sharing and isolation

- All runs allowed to use an identity share its one session. Runs on one session share its live state, the way a person's tabs share a sign-in; they do not each get a copy. Copies drift: when one copy's cookie or token is rotated, the others go stale, and a session bound to its device may not survive copying at all.
- Each run has its own working space inside the session, as the pack defines it: a tab in the web pack, a connection for one transaction in the data pack, a producer or subscription in the stream pack. A device link is held by one run at a time.
- A token cache or cookie store shared by many runs is read by many and written only by the keeper.
- Different identities never share a process, a profile, or a connection. High-assurance servers add a separate operating-system user or container per identity.
- A session can be used only by runs whose identity, as chosen by policy, is the session's identity.
- Per-session parallelism, and `session_in_use` when it is used up, follow [section 211](211-concurrency.md).

## Ending a session

- `session.revoke` ends one of the caller's own sessions. `session.end` lets an administrator end someone's session without using it. The identity provider disabling the user ends their sessions too: their token stops validating, and their stored sessions are revoked on the next sync.
- On revoke the keeper closes the session's live state (the tabs, the connections, the link), calls the system's sign-out if the auth file declares one, deletes the profile directory or state folder and the saved state, and records the event.
- A person can always see their own sessions (`session.status`) and end them.

## What is recorded

A session timeline: created, checked, renewed, prompted, expired, moved, revoked, with times and causes. No cookie, token, or password appears in it, nor in a lease, a status, a log, or a failure. Metrics: time to renew, the number of forced sign-ins, and how often checks fail.

| Failure class | Code | When |
| --- | --- | --- |
| `session_expired` | `needs_login` | `absent`, `expired`, `revoked`, or a wait that ran out |
| `session_expired` | `needs_mfa` | The identity provider demands a second factor and no person answered in time |
| `session_expired` | `device_required` | The session is not movable and its home runner is not reachable |
| `session_expired` | `headroom_short` | The learned clocks leave less time than the pipeline's declared headroom |
| `busy` | `session_in_use` | The session's parallelism is used up |
| `edge_blocked` | `challenge_page` | A pack's provider met a challenge during sign-in. It is a stop, never something to get past |
| `busy` | The vault's code | The key store is unreachable and a session must be restored or saved ([section 224](224-when-the-engine-itself-fails.md)) |

| Must always hold | Proven by |
| --- | --- |
| Ten runs that meet an expired session cause one renewal | The keeper's ten-runs-one-renewal test; hypothesis H-04 |
| A refused stored credential is not tried again | A test with the test application's lockout counter |
| No transition outside the table of states | A table-driven test over every pair of states |
| No credential appears in a lease, a status, the timeline, a log, or a failure | The planted-secret test, run against every provider |
| An auth provider is called only by the keeper's two halves | A source check, and the layering |
| An expired session stops every call with `needs_login` | The test of roadmap package 2.4 |

## Browser sessions in the web pack

This part is web pack material. It is kept here because it is the session keeper's rule for the one kind of session that lives in a browser, and [section 301](../3-packs/301-the-web-pack.md) points here for it. The browser itself is in [section 302](../3-packs/302-the-web-pack-browser-engine.md).

**The engine never drives the user's everyday browser profile, and does not try** ([decision 0031](../../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md); rule 5). Since Chrome 136 the switches that open the DevTools protocol are ignored for the default profile and work only with a separate data directory, a change Google made because that route was being used to steal cookies ([research note 14](../../research/14-sessions-and-policy-engines.md)). Chrome on Windows also encrypts its cookie store so that other programs cannot read it. The web pack therefore:

- always runs the browser with its own data directory, one per identity;
- never reads, copies, or decrypts another program's cookie store, and never connects to a browser already running on a user's device;
- treats "use the existing sign-in" as meaning the sign-in the device provides (the operating system's broker, or a browser that signs its own new profile in silently through the device's account), not the user's existing browser profile.

A first sign-in in the engine's browser may therefore be needed even on a device that is already signed in.

**Runs share a live session in tabs.** Runs on the same session work in separate tabs of that session's one browser profile, sharing its live cookie store the way a person's tabs do. Tabs share cookies and local storage, and each has its own session storage, as in an ordinary browser. Each identity has its own browser process and profile directory. HTTP-mode runs on a browser session share one cookie store and token cache per session, read by many and written only by the keeper.

**Signing in.** The keeper's worker has the pack's provider open a visible window of the engine's own browser, in the profile for that identity, at the system's sign-in page. The person signs in as they always do; password managers, second factors, and passkeys work as in any browser window. The window shows which system and which identity it is for. The keeper watches for the probe to succeed, then the window is closed or left for the first run.

**Silent renewal** in the browser means loading the system in a background tab so that its own silent renewal runs.

**Sessions that end with the browser.** Some organizations do not allow a browser session to survive closing the browser. For such a session the keeper keeps the browser process running instead of saving and restoring state, and holds off the local engine's idle stop ([section 222](222-front-doors-and-the-local-engine.md)) for as long as the keep-alive window lasts. When the window ends, the browser closes and the next use needs a sign-in. The keeper detects this case when a restored session is not accepted.

**Sessions bound to the device.** Chrome ships Device Bound Session Credentials, which tie renewing a session's cookies to a key that cannot leave the machine: the site opts in at sign-in with a response header, and the browser proves the key to renew a short-lived cookie. Such a session cannot be moved, and the keeper marks it not movable. What [research note 35](../../research/35-web-sign-in-methods-and-engine-owned-profiles.md) found, from search summaries only and so not built on: that it is generally available in Chrome on Windows, behind a rollout on macOS, and absent on Linux; and that a cookie exported from a bound session stops working at its next renewal. Whether an engine-owned profile, a browser under the debugging protocol, or a machine with no hardware key store gets a bound session at all is not established. Until a run settles it, a restored session the system refuses is handled as the paragraph above says, and is also marked not movable, so that it is never restored on another runner; the person signs in again here ([decision 0321](../../decisions/0321-a-sign-in-method-is-found-from-addresses-and-a-passkey-or-a-mailed-link-is-the-persons.md)).

**Systems that allow one session per account.** The probe checks whether signing in from the engine signs the person out of their own browser. If so, the target is marked exclusive and the person is told before they first sign in. The remedies are a service account, working outside the person's hours, or accepting the trade. A browser extension that works inside the person's own profile with their consent would remove the conflict; it is noted as a possible later mode, not designed.

## Open points

- Whether a fresh engine-owned profile signs in silently through the device's account. On Chrome for Windows it is expected not to, without the vendor's extension and a signed-in profile; for Edge, macOS, and Linux nothing was found (note 35, from search summaries; brief 30 and hypothesis H-05).
- Whether a fresh data directory, an in-memory context, a browser under the debugging protocol, or a machine with no hardware key store gets a device-bound session, and whether the browser reports one. If the browser reports the registration header, the sign-in outcome can say "device-bound: seen, not seen, or unknown" from that evidence, and the keeper marks the session not movable on it; until then that mark rests on a refused restore (brief 30).
- A sign-in by a link sent by mail. The link opens in the person's everyday browser, which the engine does not drive, and the link is itself a credential. A route where the person pastes the link into the engine's sign-in window, held to the target's declared origins and never read into a result or a log, is open; it would widen what the window is, and needs its own record and test.
- Whether isolated browser contexts with copied cookies are worth supporting for targets known to be copy-safe.
- The hosted sign-in page on a server: brief 17.
- Where the authorization response of `oauth_authorization_code` is received on a server, where no loopback address reaches the person's browser. The console's address is the likely answer.
- The details of each operating system's sign-in broker come from general knowledge and must be verified per platform in the phase that builds them, Phase 7.
- How the states, the clocks, and the keep-alive window apply to a session with no identity, such as an open link on a fieldbus, beyond the empty lease and the one-run-at-a-time rule.
- Whether a failed attempt to reach the identity service during a silent renewal may be retried once later, instead of costing a person's session a new sign-in.
