# 44. The session keeper

The session keeper is the part of the engine that owns every signed-in state: it creates sessions, checks them, renews them, shares them between runs, and ends them. No driver, run, or caller ever signs in, refreshes a token, or touches a credential itself. This section turns the outline in [section 15](15-identity-sign-in-and-sessions.md) into a design and corrects two things that outline got wrong.

## Whose account, and who may use it

**The engine acts as the user, in the user's own account on the application.** Nothing in this section changes that. What it uses is the person's account; what it does not use is the profile folder of the person's everyday browser. The person signs in to their account inside a browser profile the engine owns, once, and from then on the engine works in that account with that person's permissions.

**A user's session belongs to that user alone.** A session signed in as a person can be used only by:

- that person, and
- agents acting for that person.

An agent counts as acting for a person in exactly three cases:

| Case | How the link is proved |
| --- | --- |
| The agent runs on the person's own machine | It reaches the local engine as that operating-system user |
| The agent connects to a server with the person's sign-in | Its token was issued to that person by the identity provider, with their consent, and names them as the subject |
| The agent has its own identity | The person has delegated to it: an explicit, revocable record naming the agent, the toolset, and an expiry, created while the person was signed in |

Nobody else can use the session: not another user, not another user's agent, not a service account, and not an administrator. An administrator can end a person's session but cannot run anything through it. The audit log records the person and, where there is one, the agent that acted for them.

On a shared server this is enforced in three places: the policy engine refuses any run whose resolved identity is not the caller's own, the scheduler hands a session only to runs carrying its owner, and the session's saved state is encrypted under a key for that owner.

## Two corrections to earlier sections

**The engine cannot drive the user's everyday browser profile, and must not try.** Since Chrome 136, the switches that open the DevTools protocol are ignored for the default profile and work only with a separate data directory, a change Google made because that route was being used to steal cookies ([research note 14](../research/14-sessions-and-policy-engines.md)). Chrome on Windows also encrypts its cookie store so that other programs cannot read it. Anything2MCP therefore:

- always runs the browser with its own data directory, one per identity;
- never reads, copies, or decrypts another program's cookie store;
- treats "use the existing sign-in" as meaning the sign-in the *device* provides (the operating system's sign-in broker, or a browser that signs its own new profile in silently through the device's account), not the user's existing browser profile.

Earlier statements that the engine can attach to a browser already running on a user's device are withdrawn.

**Runs share a live session; they do not each get a copy of it.** Earlier sections gave each run an isolated browser context seeded with the session's cookies. Copies drift: when one copy's cookie or token is rotated, the others go stale, and a session bound to its device may not survive copying at all. Runs on the same session therefore work in separate tabs of that session's one browser profile, sharing its live cookie store the way a person's tabs do. Different identities never share a process or a profile.

## What a session is

A session is the signed-in state of one identity on one target in one environment. It consists of whichever of these the target uses:

| Part | Where it lives |
| --- | --- |
| A browser profile with cookies and page storage | A data directory owned by the engine, on the session's home runner |
| A token set: access and refresh tokens | In memory on the home runner; encrypted in the vault if the session may be saved |
| A desktop application's own login | Inside that application, in the runner's desktop session |
| Stored credentials for a service account | The vault or an external secrets manager, by reference |

Every session has a home: the local engine, or one runner.

## Where the keeper runs

The keeper is one component with two halves. Its decisions (which sessions are wanted, when to check, when to ask a person, who may use a session) are made in the control plane. Its work (the probe, a renewal, a sign-in window) runs as jobs on the session's home runner. In a single process the two halves are the same code calling itself.

## States

| State | Meaning | Calls for this session |
| --- | --- | --- |
| `absent` | Never signed in, or ended | Return `needs_login` |
| `signing_in` | A person or a stored credential is signing in | Wait |
| `valid` | Signed in and recently confirmed | Run |
| `suspect` | One check failed | Wait while it is re-checked once |
| `renewing` | Silent renewal in progress | Wait |
| `needs_person` | The identity provider demands a person: MFA, a new password, a consent | Wait up to the login wait, then `needs_login` |
| `expired` | Ended and could not be renewed | Return `needs_login` |
| `revoked` | Ended on purpose | Return `needs_login`; nothing is retried |

## Three clocks, only two of which can be extended

| Clock | Example | What the keeper can do |
| --- | --- | --- |
| The application's idle timeout | Signed out after 30 minutes without activity | Prevent it with a cheap read before it runs out |
| Token or cookie expiry | An access token valid for an hour | Renew silently, the way the application itself would |
| The identity provider's maximum age | A rule that forces a fresh sign-in every so often | Nothing. A person must sign in again |

The third clock is set by the organization, not the application. Under Microsoft Entra, how long a browser stays signed in is governed by Conditional Access session controls: sign-in frequency sets a maximum age, and a separate control decides whether the session survives closing the browser ([research note 14](../research/14-sessions-and-policy-engines.md)). The keeper never tries to get around either.

**The keeper learns the clocks.** A target's auth file can declare its timeouts. Where it does not, the keeper records how long each session lasted and what ended it, and estimates each clock from that history. It uses the estimates to:

- time its checks;
- tell the person ahead of time when a sign-in will be needed again;
- hold back a long pipeline that would outlast the session, when the pipeline asks for headroom.

## Checks

- **The probe.** Each target declares one cheap signed-in read. The keeper runs it at half the idle timeout, 15 minutes by default, with jitter.
- **Only while wanted.** A session is kept alive for a window after its last use: 8 hours by default locally, set per toolset on servers. After that the keeper stops checking and lets it lapse. Standing access nobody is using is a risk with no benefit.
- **Signals of expiry.** A redirect to an identity provider's host, a 401 or 403 that matches the target's declared pattern, a login form appearing, a token past its expiry, or a challenge demanding fresh claims. Every driver reports these to the keeper instead of acting on them.

## Renewal

The keeper climbs a ladder and never repeats a rung:

1. **Silent.** Refresh the token, ask the operating system's broker, or load the application in a background tab so its own silent renewal runs.
2. **A light prompt.** The person approves a push notification or enters a code.
3. **A full sign-in.** The person signs in again in a window the engine opens.

**One renewal at a time.** Only the keeper renews, and only once per session however many runs noticed the expiry. Two simultaneous refreshes with a rotating refresh token invalidate each other; a single renewer makes that impossible. Runs that hit an expired session wait for the one renewal, then retry once if they are reads or idempotent writes.

**No loops, no lockouts.** A stored credential is tried once. If it is refused, the session goes to `needs_person` and an alert is raised. Repeated automatic attempts are how service accounts get locked out.

## Asking a person

- Only the session's own identity is asked, through URL-mode elicitation in their MCP client, a system notification, or a chat message ([section 49](49-approval-and-sign-in-surfaces.md)).
- Interactive calls wait up to 5 minutes by default. Pipelines stop as `needs_login` and resume after the sign-in.
- Where the sign-in happens depends on the profile:

| Profile | Where the person signs in |
| --- | --- |
| Local engine | A visible window of the engine's own browser |
| User-device runner | The same, on their own computer |
| Hosted user session on a server | A sign-in page that shows the server runner's browser; see [section 16](16-identity-on-a-shared-server.md) |
| Service account | Nobody; stored credentials, within the attempt limit |

## Sessions that end with the browser

Some organizations do not allow a browser session to survive closing the browser. For such a session the keeper keeps the browser process running instead of saving and restoring state, and holds off the local engine's idle stop ([section 39](39-local-engine-and-clients.md)) for as long as the keep-alive window lasts. When the window ends, the browser closes and the next use needs a sign-in. The keeper detects this case when a restored session is not accepted.

## Saving and moving a session

- **Saved state.** For a movable session, the keeper exports cookies, storage, and tokens from the engine's own profile and encrypts them in the vault. It restores them after a restart or on another runner.
- **Not movable.** A session is marked not movable when it is on a user's device, when the organization requires a managed device, or when the session is bound to the device's hardware. Chrome now ships Device Bound Session Credentials, which tie renewing a session's cookies to a key that cannot leave the machine, so a growing number of sessions cannot be moved at all ([research note 14](../research/14-sessions-and-policy-engines.md)).
- **When a home runner is lost.** A movable session is restored elsewhere. A session that is not movable is gone, and its owner signs in again on the new runner. This is deliberate: those protections exist to stop exactly this kind of copying.

## Sharing and isolation

- All runs allowed to use an identity share its one session.
- Each run works in its own tab. Tabs share cookies and local storage, and each has its own session storage, as in an ordinary browser.
- Each identity has its own browser process and profile directory. High-assurance servers add a separate operating-system user or container per identity.
- HTTP-mode runs share one cookie store and token cache per session, read by many and written only by the keeper.
- A session can be used only by runs whose identity, as chosen by policy, is the session's identity.
- Per-session parallelism follows [section 11](11-concurrency-many-agents-at-once.md).

**Applications that allow one session per account.** The probe checks whether signing in from the engine signs the person out of their own browser. If so, the target is marked exclusive and the person is told before they first sign in. The remedies are a service account, working outside the person's hours, or accepting the trade. A browser extension that works inside the person's own profile with their consent would remove the conflict; it is noted as a possible later mode, not designed.

## Ending a session

- `anything2mcp session revoke`, the admin console, or the identity provider disabling the user.
- On revoke the keeper closes the tabs, calls the application's sign-out if one is declared, deletes the profile directory and saved state, and records the event.
- A person can always see their own sessions and end them.

## What is recorded

A session timeline: created, checked, renewed, prompted, expired, moved, revoked, with times and causes. No cookie, token, or password appears in it. Metrics: time to renew, number of forced sign-ins, and how often checks fail.

## Open points

- How reliably a fresh engine-owned profile signs in silently through the device's account, per browser and operating system.
- How to detect a device-bound session with certainty, and how such sessions behave on servers and virtual machines with no hardware key store.
- Whether isolated contexts with copied cookies are worth supporting for targets that are known to be copy-safe.
- The hosted sign-in page, which is research brief 17.
