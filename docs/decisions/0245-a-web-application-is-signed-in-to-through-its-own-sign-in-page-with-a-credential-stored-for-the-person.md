# 0245. A web application is signed in to through its own sign-in page, with a credential stored for the person

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the designs of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md), and [`a2m-auth`](../architecture/crates/a2m-auth.md); sentences of spec sections [301](../spec/3-packs/301-the-web-pack.md), [215](../spec/2-kernel/215-sessions-and-the-session-keeper.md), and [503](../spec/5-security/503-approval-and-sign-in-surfaces.md). Builds on decisions [0205](0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md), [0031](0031-the-browser-always-runs-with-a-profile-the-engine-owns.md), [0032](0032-one-live-session-per-identity-shared-by-tabs-with-a-single-renewer.md), [0143](0143-the-browser-engine-after-its-safety-audit.md), and point 1 of [0242](0242-the-owner-takes-the-recommended-answer-to-every-open-question.md).

## Context

The web pack read and wrote, and no web application that signs a person in through its own pages could be used: `form_login`, `sso_oidc`, and `sso_saml` were named and not built. The breadth review of 2026-11-18 ranked this second among the additions to the product. Research brief 30 (sign-in in engine-owned profiles, and device-bound sessions) is not answered; [research note 14](../research/14-sessions-and-policy-engines.md) answers part of it, and says a device-bound session cannot be saved and restored elsewhere.

## Decision

### Which way the person signs in

Two ways were weighed.

- **(a) The person stores the credential; the provider fills the application's form.** The person types the username and the password once, on the engine's own page of `session.login` (decision 0205) or with `secret.set`. They are sealed in the vault for that person. The provider fills the application's sign-in page in the engine's own browser. The engine holds the password, sealed, and opens it only to fill the page. A session can be renewed with nobody there, which unattended reads need. It works on a machine with no screen.
- **(b) The person signs in in a visible window of the engine's own browser.** The engine types nothing and reads no field; it waits for the declared sign that the person is signed in. The engine never holds the password. It is the only honest way where an identity provider's pages, a passkey, or a device prompt are involved. It needs a screen, and every renewal needs the person.

**Built: way (a)**, the simpler, and the one that renews. **Way (b) is designed below and not built** (Not yet): the browser engine has no visible window yet, and no machine this project tests on has a screen to prove one.

### `form_login`, as built

- **Declared in the target's files, with no locator outside a screen model** (rule 8). An `auth.yaml` entry names the provider `form_login` and, in `settings`, only names of parts of `screens/`: `screen` (the sign-in page, an address with no placeholder), `user_field` and `pass_field` (two `text` fields), `submit` (an action, whose `confirm` is the sign of success), `refused` (a variant that shows a refused credential), and optionally `blocked` (a variant that shows a block), `code_screen`, `code_field`, `code_submit` (whose `confirm` is the sign of success after a code), `code_refused`, and `wait_ms` (each wait for the page; 30 seconds by default). Its credentials are the roles `user` and `password`, vault references only. `attempt_limit` must be 1. The provider checks every name against the compiled screens before anything is opened.
- **Each outcome is a declared sign and a typed failure** (rules 1 and 7). Signed in: `Material` that holds the session. A refused credential: `session_expired`, `credential_refused`. A one-time code asked and nobody to give it, or a refused code: `session_expired`, `needs_mfa`. The declared block: `edge_blocked`, `sign_in_blocked`. A page that shows none of the signs in the time, or shows one before the credential was sent: `unknown_variant`, `unexpected_page`. A sign-in that succeeds and leaves no cookie: `unexpected_page`, since a session kept in page storage is not built.
- **Tried once.** The keeper honours `attempt_limit`, and the provider refuses an entry whose limit is not 1. Building it showed that the keeper counts a refusal of a sign-in a person asked for (`session.login`) as no attempt, and so tried the refused password again for the next run that asked: a second sending that a lockout policy would count. The provider now remembers, in memory, a digest of every credential the application refused, keyed by the session's key, and never sends one again; a test asks the stand and finds one attempt after three runs. The keeper's own count is owed to `a2m-auth` (below).
- **A second factor is the person's, each time** (rule 5). When the declared code screen shows, the provider asks the person through `PersonAsk::EnterCode`, which the code page of decision 0205 answers while the person's own sign-in is in flight. The code is registered with the redactor, typed once, and dropped. Nothing stores a code or a code's secret to answer later. With nobody there the stop is `needs_mfa`.
- **Only a published version's addresses take a stored credential.** Anyone who can write a draft could name an address of their own (the audit that decision 0248 answers). `AuthEntry` carries `EntrySurfaces` with `from_published`, which only the caller that loaded a published plan sets; `form_login` refuses a drafts' entry with `needs_login` before any browser opens, and a test shows the stand heard nothing. The page it fills is the sign-in screen's address on the surface's own origin; the tab is opened for that one origin, and the browser ends any move to another, at every redirect, before a field is located (decision 0143). A credential whose reference is not under `vault://<target>/` is refused.

### Where the session is, and what is on disk

- **Every tab is opened in an in-memory browser context.** The browser makes a context of its own (`Target.createBrowserContext`) when it starts; downloads are denied and permissions refused for it as for the default context. Its cookies and page storage are kept in memory and written to no file. The data directory still exists, mode 0700 on Linux and macOS: it holds the browser's own preferences, caches of its components, and no cookie or page storage of any page. A test signs in through the stand's page, closes the browser, and finds the session cookie in no file under the data directory.
- **After a sign-in, the session is the cookies the browser holds for the application's origin** (`Browser::export_state`). Each value is registered with the redactor and kept as its own secret part of the session's `Material`; their names, paths, and rules are a plain part. The keeper seals the material for its person (decision 0035: the seal's owner is the session's identity) when the entry is `movable`, and holds it in memory on the session's home runner. It is never written unsealed.
- **A run gets it back through its lease.** `inject` places it, as one value under a name only the web pack's drivers take; the `ui` and `page_http` drivers place it into the session's browser (`Browser::import_state`) before a tab opens, once for each session and epoch, so a cookie the application changed since is not overwritten by an older copy. Every session has its own profile, named from the session's identifier, so no two persons and no two sessions share a browser. A provider that places anything else is still refused by the web drivers.
- **Revoking a session closes its browser,** and with it the in-memory context and every cookie in it. The application's own sign-out is not built.

### An expired session during a run

- **The target declares how to tell.** An `expiry_signals` entry of a web surface is `{screen: <name>}`: a page that shows that screen's `ready`. Any other form is refused (`step_not_usable`).
- **The `ui` driver stops; the keeper renews.** A `wait` that finds the declared sign-in screen in place of its own stops with `session_expired`, `needs_login`, nothing sent. The engine reports it, the keeper checks with the target's probe and signs in again once with the stored credential, and the next lease is for the new epoch. A test does this against the stand's own expiry, with the probe run through the driver; the stand counts two sign-ins, and the old lease places nothing. With way (b) the same stop would wait for the person.
- **Not built:** the same sign in `page_http`, which still answers a redirect as `unexpected_status`.

### How each rule is held

| Rule | Kind | How |
| --- | --- | --- |
| 1, no AI at run time; 7, unknown means stop | Rule | The sign-in's steps are the compiled screens'; a page with no declared sign is `unexpected_page`; no text of the application becomes a step |
| 4, secrets never reach a caller | Rule | The username, the password, the code, and every cookie are registered with the redactor before they go anywhere; a test plants all of them and finds none in a value, a failure, the timeline, the logs, the ledger's files, or the browser's data directories. The username a page echoes is masked too |
| 5, operate openly | Rule | The engine's own browser and data directory, with `--enable-automation`; nothing gets past a second factor or a block; no code is stored; the person's everyday browser is never opened, driven, or read (no `OpenLink` is asked by this provider, which a test checks) |
| 8, knowledge in one place | Rule | The entry names screens; every locator is in `screens/` |
| One renewer | Convention, AGENTS.md | Only the keeper calls the provider; the driver stops and reports |
| A session is its person's | Owner decision 0035 | Sealed under the person's identity; one profile per session |
| A lockout is damage | Choice, this record | One attempt; a refused credential is never sent again |

### Way (b) and single sign-on, designed and not built

- **Way (b).** `Visibility::Window` starts the session's browser with a window, which shows the target and the person it is for, at the sign-in screen's address, in a tab opened for the surface's origin and the declared identity provider's origins. The engine fills and reads nothing; it waits for the declared sign of success, then exports the state as above. On a machine with no screen the window cannot open, and the stop is `needs_login` naming that. A renewal always waits for the person.
- **Single sign-on as a person meets it** (`sso_oidc`, `sso_saml` in a browser) is way (b) with one more declared list: the identity provider's origins, in the entry, each within the destinations the target declares. The tab may be on exactly those and the surface's own origin during the sign-in, and on the surface's alone after it; nothing is read on the identity provider's pages, and only the application's origin's cookies are exported. Not built: it needs the window.
- **A device-bound session** cannot be moved (note 14); such an entry is `movable: false`, and losing the browser means signing in again. How to tell one from outside is brief 30's, still open.

## Found by the audit of writes from inside a page, and settled here

The safety audit of 2026-11-18 on decision 0206 reported four faults in these two crates.

- **S4, serious (rule 4).** The page scrubbed three spellings of the anti-forgery token from a write's answer, and the engine's redactor never knows the token; an answer that quoted it in another spelling reached the caller as `said`. Now the helper compares the answer, after JSON escapes, HTML character references, and percent-encoding are undone in either order and in any case, with the token and with its first half; on a match the body is withheld whole in the page and `PageAnswer::withheld` says so; the write is then in doubt. The stand's refusal now quotes a token sent in the body again in capitals; the test failed before the change, with the token in `said`.
- **S3.** A values refusal a write's entry declares was settled as not written, though the browser may send a PUT twice and the refusal may answer the second sending of one that landed. It is now `possible_commit`, `no_confirmation`, with what the application said; the record is read back before anything is sent again. The test changed is the stand's 422; the resend itself is not reproduced here.
- **M6.** The acting tab now passes the page's service worker by, as the observing tab does, so a write reaches the application as sent. The stand's service worker answers one address itself; a test shows the acting tab's request reaching the application instead, and failed before the change.
- **M7.** The token, the password typed into a sign-in page, and the cookies the application sets do cross into the engine's process: an acting tab has the browser's network events on, and they carry request headers and bodies. The engine parses none of those fields into any type and keeps nothing of them. Switching the tab's network events off for the length of a write or a sign-in would avoid it; that is not built (Not yet).

## Consequences

- A web application that signs in by a form, with or without a one-time code, can be used, and an expired session is renewed with nobody there.
- `Material` holds a web session's cookies as secret parts; the browser's data directory holds none.
- `AuthEntry` has a new field, `surfaces: EntrySurfaces`, which the engine must fill from the same plan it builds the entry from, marking a published version's plan; until it does, `form_login` refuses every entry by name.
- Not built, each Not yet: way (b) and its window; `sso_oidc` and `sso_saml`; the application's own sign-out; a session kept in page storage; the expiry sign in `page_http`; switching network events off around a write; the keeper counting a refusal of a person's own sign-in. Not run: anything on Windows and macOS.
