# Research: web sign-in methods, and sign-in in an engine-owned browser profile

**Status:** Partial. Written on 2026-11-23 from web-search summaries and from the repository's own documents. No primary page could be opened: the session's network refused every host a page fetch was tried on (`developer.chrome.com`, `w3c.github.io`, `groups.google.com`, `learn.microsoft.com`, `scotthelme.co.uk`), so no standard, vendor page, or source file was read. Nothing was built, installed, or run in a browser. Each claim is tagged:

- **[repo]**: read in this repository.
- **[search]**: taken from a web-search summary; the page itself was not opened. Treat as a lead and re-check.
- **[memory]**: stated from general knowledge of the standard or product. Unverified.

Answers open briefs 6 and 30 of [11-open-research.md](11-open-research.md), in part. Feeds spec sections [215](../spec/2-kernel/215-sessions-and-the-session-keeper.md) and [503](../spec/5-security/503-approval-and-sign-in-surfaces.md), the designs of [`a2m-auth`](../architecture/crates/a2m-auth.md), [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md) and [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), hypothesis H-05, and roadmap packages 2.3 to 2.6. Extends [note 14](14-sessions-and-policy-engines.md). This note records facts and proposals; it does not change the design.

## Question

Which ways does a web application sign a person in, how can the engine tell which one a site uses without reading anything it must not read, and what happens to each in a browser whose data directory the engine owns, when a person signs in there and the cookies are kept between runs? Rule 5 binds: a person signs in; no MFA or CAPTCHA bypass, no stealth, no spoofing.

## Answer in short

- **The design already treats most methods the same way.** A person signs in in a visible window of the engine's own browser; the engine waits for a declared sign of success and reads no field ([decision 0254](../decisions/0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)) [repo]. Form, OIDC, SAML, and single sign-on are all "pages and redirects" to a browser, so one window serves them. Passkeys and second factors need nothing from the engine. This note found no method that forces a different provider.
- **Detection should be an observation of addresses, not of pages.** The browser's own word for the tab's address, and the redirect chain, are enough to name OIDC, SAML, form, and the HTTP challenge schemes (part 2). Reading a sign-in page's fields is neither needed nor allowed under decision 0254 [repo].
- **Detection can only recommend.** Several methods look alike from outside (OAuth versus OIDC; a form that is a step of an SSO chain). The result of `auth.detect` should be a recommended mode with the evidence, and a named stop when the evidence does not decide (rule 7).
- **Device-bound sessions (DBSC) are real and growing, not universal.** Chrome 146 made them generally available on Windows, backed by the TPM; macOS support is in Chrome 147 behind a rollout switch; Linux has none; software keys are only being explored [search]. A site opts in with a response header at sign-in. The engine can see that header (part 4).
- **Whether the engine's own profile can use DBSC is not established.** No source says how DBSC behaves with a fresh data directory, under DevTools control, in a virtual machine without a TPM, or in a container. These stay open and need a run on a real Windows machine (part 4).
- **A copied cookie for a bound session is expected to stop working at its next refresh** [search, inferred by the search summary from the design; no source states it]. Exporting cookies from a bound session and restoring them elsewhere is therefore not a safe way to move it. The engine already marks such entries `movable: false` [repo].
- **A fresh profile has no passkeys.** Each passkey provider is tied to the browser profile's account, the operating system, or a phone; none follows a new data directory (part 5).
- **Keeping cookies between runs works until a site's own rules end the session.** Session cookies, idle limits, rotating refresh tokens, partitioned storage, and one-session-per-account all end a kept session in ways the keeper can only observe (part 6).

## What was and was not reached

| Source | Reached | How |
| --- | --- | --- |
| Chrome's DBSC guide, the W3C DBSC draft, the Chrome intent-to-ship thread | No | Hosts refused. Only search summaries of them |
| Google security and Workspace posts on DBSC | Summaries only | Search results |
| Help Net Security, Hacker News, a DBSC practitioner's blog | Summaries only | Search results |
| Microsoft documentation on the primary refresh token and browser SSO | Summaries only | Search results |
| RFC 8252 (OAuth for native apps) | Summary only | Search result |
| Chrome documentation on passkeys and on the DevTools WebAuthn domain | Summaries only | Search results |
| The repository's spec, decisions, crate designs | Yes | Read |

Nothing private was put in a search query. Queries named protocols and browsers only.

## 1. The sign-in methods a web application uses

What each one is to a browser, and what it asks of the person. The "Provider" column is the design's own name [repo: `a2m-auth.md`].

| Method | What a browser sees | Who acts | Provider today |
| --- | --- | --- | --- |
| Form login | A page of the application with a password field; a post; a session cookie | A person types, or a stored credential is typed | `form_login` [repo] |
| OIDC redirect | A redirect to an authorization endpoint with `response_type`, `client_id`, `redirect_uri`, `scope`, `state`; later a redirect back with a one-use code [memory] | A person at the identity provider's pages | `sso_oidc` [repo] |
| OAuth 2.0 redirect, no OIDC | The same shape without an identity token [memory] | The same | `oauth_authorization_code` for the token side, in `a2m-auth` [repo] |
| SAML redirect or post | A redirect carrying `SAMLRequest`, or a post of `SAMLRequest` or `SAMLResponse` to the application's assertion address [memory]; the search found only that these bindings are browser redirects, not a detection guide [search] | A person at the identity provider's pages | `sso_saml` [repo] |
| Pop-up sign-in | A second window opens for the identity provider and closes with a message to the opener [memory] | A person | None. Decision 0254 makes a window opened by a page `unexpected_page` [repo] |
| Passkey (WebAuthn) | A script call to the browser's credential API; the browser or the operating system shows a prompt [memory] | A person, with a fingerprint, PIN, face, or a key | None needed; the person acts in the window |
| Magic link | The application emails a single-use address; opening it signs in [memory] | A person, in the browser that opens the link | None. See part 3 |
| HTTP Basic, Digest | A `401` with `WWW-Authenticate` and a scheme name [memory] | Stored credential | `http_basic`, `http_digest` [repo] |
| NTLM, Negotiate | The same header with those scheme names, then a challenge exchange on one connection [memory] | The operating system or a stored credential | `windows_integrated` [repo] |
| Client certificate | A TLS request for a certificate | Stored certificate | `client_certificate` [repo] |
| Device account (silent) | The identity provider signs the browser in with no prompt, through the operating system's broker | Nobody | `use_existing` [repo] |

## 2. Detecting which method a site uses

**What the engine may look at.** Decision 0254 allows the browser's own word for a tab's address and forbids reading fields on an identity provider's pages [repo]. The detection below uses addresses, response headers of the first request, and the redirect chain only. The kernel's rule 8 (locators only in screen models) is kept: nothing here is a locator.

**Signals, in the order they are cheapest and surest.** All are [memory] unless marked; each needs a run against a stand to prove.

1. **A `401` on the first request with `WWW-Authenticate`.** The scheme names (`Basic`, `Digest`, `NTLM`, `Negotiate`) name the method directly. This is a header, not a page.
2. **A redirect whose address has `SAMLRequest`** in its query, or a document whose post goes to an address with that name: SAML.
3. **A redirect whose address has `response_type=` and `client_id=`**: OAuth 2.0. With `scope` containing `openid`, or `nonce`, or a `.well-known/openid-configuration` document at the origin: OIDC. The discovery document is the standard check that a provider speaks OIDC rather than plain OAuth 2.0 [search: one source says the metadata page is part of OIDC and not of plain OAuth 2.0].
4. **A redirect to another origin that is declared as an identity provider** in the target: single sign-on, and the origin names the provider's kind.
5. **No redirect, a page of the application's own origin that asks for a password:** form login. This is a judgment from a page, so it is the weakest signal. The target's declared sign-in screen is the stronger one ([decision 0245](../decisions/0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md) [repo]).

**Not detectable from outside.** Passkey use, magic-link use, and the second factor an identity provider asks for are not visible in addresses. They appear only after the person has started. The note's proposal is to say so in the finding: "method of the first factor unknown", never a guess.

**A chain is not a method.** A site can redirect to SAML, then to a form at the identity provider, then to a second factor, then back. The recommended mode is the outermost: `window_login` or `sso_*` with the origins the chain showed, listed for the person to approve ([decision 0248](../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md) [repo]).

**Detecting expiry mid-run.** The spec already lists the signs: a redirect to an identity provider's host, a declared status, a sign-in screen appearing [repo: section 215]. Decision 0254 refuses a redirect as a declared sign inside a page's own request, because the page cannot see where it went [repo]. Nothing found here changes that. Under single sign-on the identity provider may end its own session while the application's cookie is still valid; the application's own probe is what shows it, so a probe is needed per target, not per method [memory].

## 3. Signing in, by method, in the engine's window

| Method | In the engine's window | What the engine must do | Problems |
| --- | --- | --- | --- |
| Form, typed by the person | Works | Wait for the declared sign | None new |
| OIDC, SAML | Works as pages and redirects | Hold the tab to the declared origins; export only the application's cookies [repo: 0254] | The identity provider's cookie stays in the engine's profile too. A later sign-in may then be silent or may show the account chooser; the keeper must not assume either |
| Google-style identity providers that refuse automated browsers | May refuse | Stop with a named failure, never alter the browser to get past it | Google's help page says it may stop sign-ins from browsers controlled by software [search]. Its developer guidance says a sign-in browser must not provide automation features [search]. The engine's browser is DevTools-controlled by design. Whether a window the person types into is refused is not known. If refused, the stop is the answer (rule 5), not a flag change |
| Passkey, platform authenticator | Works if the provider exists in the profile | None. The engine must never answer a passkey prompt | See part 5 |
| Passkey, phone (cross-device) | Works through a QR code and the phone [search] | None | Needs the phone near the machine |
| Magic link | Does not fit | Nothing safe today | The link opens in the person's everyday browser, which the engine does not read or drive (rule 5), so the session lands in the wrong profile. The link is also a credential: it must never pass through a tool input or a log. The only honest route is that the person pastes the link into the engine's window themself. That window has no address bar by design [repo: 0254, an engine-written first page]; whether to give it one is open (part 8) |
| Second factor by code, push, or key | Works as any browser window | None | The person answers; the engine never reads or sends the code. `totp` is the one exception: a stored secret the person put in the vault by choice [repo] |
| CAPTCHA | The person solves it | Nothing | A block is a named stop (rule 5) |

## 4. Device-bound session credentials

**What it is.** DBSC lets a site tie a session to a key pair the browser makes at sign-in and keeps in secure hardware. The site's cookie is short-lived; when it ends, the browser proves it holds the key and gets a new one [search]. The W3C draft describes the aim as letting a server verify that a session credential has not been exported from a device [search].

**The wire.** At sign-in the site sends the response header `Secure-Session-Registration`, which tells the browser to call a registration endpoint with the session's public key [search]. At refresh, the browser posts to the refresh endpoint with the request header `Sec-Secure-Session-Id`; the server answers `403` with `Secure-Session-Challenge`; the browser posts again with a signed proof [search]. A practitioner's write-up says the cookie value must change on every refresh or Chrome reports a challenge error, and that two failure modes silently turned a bound session into an ordinary bearer session [search]. Those facts are from a summary of a blog post; treat as a lead.

**Status, as found.**

| Platform | Status | Source |
| --- | --- | --- |
| Windows | Shipped in Chrome 145; generally available with Chrome 146 (April 2026); keys in the TPM; on by default for one large vendor's workspace users | [search: Google posts, Help Net Security] |
| macOS | In Chrome 147, gated by a rollout switch and off by default; keys in the Secure Enclave | [search: a practitioner's blog] |
| Linux | No support; the intent-to-ship thread answered "No" on every platform other than the first, and says other platforms will follow | [search: Chromium intent thread] |
| No hardware key store | Software keys are being explored, not shipped | [search: Google's post] |
| Edge | Not established by this note. Note 14's table lists Edge on Windows and macOS from one secondary source [repo: note 14] | Unverified |

**What was not found, and matters most.**

1. **Whether an automated browser, a fresh data directory, or a virtual machine without a TPM gets a bound session at all.** The search found only an inference that key binding "would likely fail" on a machine without a TPM [search, flagged by the summary as inference]. No source says what Chrome does then. The practitioner write-up mentions that Chrome may skip DBSC handling when the refresh endpoint is unreachable [search]. Whether the browser falls back to an ordinary cookie, or the session fails to start, is the question the brief asked and it is still open.
2. **Whether an enterprise policy controls DBSC.** No policy name was found. One vendor's workspace documentation lets an administrator exempt a group from DBSC [search]; that is the site's setting, not the browser's.
3. **What exporting the cookies of a bound session does.** Inferred from the design: the exported short-lived cookie works until it ends and cannot be renewed on another machine. No source states this [search]. The engine's own import (`Browser::import_state`) therefore may place a cookie that works for minutes and then stops [repo: 0254 item A7 for the function].
4. **Whether an engine-owned profile that exports and re-imports cookies between its own runs on the same machine is affected.** The key belongs to the browser profile or the device, not to a cookie string. If the key is in the TPM and tied to the profile, a fresh browser process on the same data directory may keep it; if the engine starts the browser in an in-memory context (as decision 0254 does for the sign-in window [repo]), the key may not survive. Not found in any source.

**How to tell from outside that a session is device-bound.** The one reliable sign is the response header `Secure-Session-Registration` on the sign-in response, or later `Sec-Secure-Session-Id` on requests to a refresh endpoint [search]. The engine's sign-in tab hears no network event at all today, by design [repo: 0254 item 2]. So the first sign of a bound session would be indirect: the session ending after the cookie's short life when restored, which the keeper already sees as "a restored session is not accepted" [repo: section 215]. A direct check needs the browser's own report; whether DevTools exposes bound sessions was not found, and is the single most useful thing to look up in the Chromium source or a run. This note proposes that the finding names the weaker sign as what it is: a guess from behaviour, marked unverified, never a statement.

## 5. Passkeys in a fresh profile

| Provider | Where the passkey lives | Present in a fresh engine profile? |
| --- | --- | --- |
| Chrome's own provider, Google Password Manager | The Google account, synced to browsers signed in to it; a new device asks for the manager's PIN or the phone's screen lock [search: Chrome documentation] | No. The profile must be signed in to the account, and the engine must not sign its profile in to a person's account without their act |
| Windows Hello | The computer; no sync or backup [search] | Yes for the computer's user, if Chrome offers it; not tied to the data directory [search, unverified for a non-default data directory] |
| macOS, Chrome profile | The Chrome profile, protected by a Keychain entry [search] | No; a new data directory is a new profile. Gone if the profile is removed |
| macOS, iCloud Keychain | The Apple account; one search source says Chrome supports it from Chrome 118, another Google page says it cannot [search, conflicting] | Unknown |
| A phone, by QR code | The phone | Yes, if the person has it with them |
| A security key | The key | Yes |

**Consequences.**

- A person who signs in by passkey in the engine's window will usually use the phone or a key the first time, or create a new passkey in that profile. A passkey made in the engine's profile is held by that profile and lost with its data directory [search for the macOS profile case]. The person should be told in the sign-in window's notice.
- **The product must not use a virtual authenticator to answer a passkey prompt.** The DevTools WebAuthn domain can add a virtual authenticator with automatic presence and verification simulation [search]. It is a testing tool and it fakes the person's presence. Using it in the product would be a bypass of user verification (rule 5). It is acceptable in `a2m-pack-web-browser`'s own tests against the test application, and nowhere else. Enabling the domain also switches the browser off from real authenticators [search], so even an accidental use would break a real passkey sign-in.
- A site that offers a passkey as the only first factor, and a person with none in the engine's profile, ends in a named stop (`needs_login`), not a fallback.

## 6. What breaks when cookies are kept between runs

The engine keeps a profile per identity [repo: section 215] and exports the application's cookies after a sign-in [repo: 0254]. What ends a kept session:

1. **Session cookies.** A cookie with no expiry is meant to end with the browser. Chrome can restore them: reports tie this to the "continue where you left off" setting and to background apps keeping the browser alive [search]. The engine cannot rely on either; a restored-but-rejected session must be treated as signed out, as section 215 already says [repo].
2. **Idle and absolute limits** set by the site, and for Entra the organization's sign-in frequency, which is a ceiling and not extended by activity [repo: note 14]. The keeper learns them from history [repo].
3. **Rotating refresh tokens.** Two renewals at once invalidate each other; the keeper's one-renewer rule covers it [repo: section 215].
4. **A bound session** (part 4): the cookie lives minutes, and a restore on another device or an import into a profile without the key fails at the next refresh.
5. **One session per account.** Signing in from the engine can sign the person out of their own browser. The probe and the "exclusive" mark cover it [repo].
6. **Storage that is not a cookie.** A session kept in page storage or an in-page token is not exported by a cookie export. Decision 0254 lists this as not built [repo].
7. **The identity provider's own cookie** stays in the profile and is not exported [repo: 0254]. A restored application cookie with the provider's cookie missing can sign out at the next provider check. Not tested.
8. **Partitioned and third-party cookies, and silent renewal in an iframe.** Browsers have been changing how a cookie set in a frame is stored; OIDC silent renewal in a frame is the usual casualty. The search for a source returned nothing, so this is [memory] and needs a run.

## 7. Silent sign-in through the device's account (hypothesis H-05)

- On Windows with Entra, a browser gets the device's primary refresh token through the operating system's broker. Edge does it natively. Chrome does it with a Microsoft extension, and Firefox from version 91 [search: Microsoft Entra change note].
- A community answer on Microsoft's help forum says Chrome falls back to a plain browser flow, with no device information, unless the profile is a managed work profile, the extension is installed and enabled, and the user is signed in to Chrome with the work account; creating a fresh profile often fixes it [search: community answer, not official].
- **For the engine's profile this means** a fresh data directory has no extension and no account, so it does not sign in silently on Chrome. Whether the engine's browser, if it were Edge, would is not found. The engine installing an extension into its own profile is a design change with its own rules (rule 5 allows the browser's own features; it is not decided). The first sign-in in the engine's window is therefore expected on every managed machine, which is what decision 0245 already assumes [repo].
- H-05 stays open. This note found a lead that makes "silently, with no setup" unlikely on Chrome, and no result for Edge, macOS, or Linux.

## What could not be verified

- Every DBSC fact is from search summaries. The registration and refresh header names, the platform table, and the Chrome versions need the Chrome guide and the W3C draft read in full.
- Whether DBSC works, fails, or falls back in an automated browser, a fresh data directory, an in-memory context, a virtual machine, or a container.
- Whether DevTools or any other interface reports that a session is bound.
- Whether Edge ships DBSC and on which platforms.
- Whether a Chrome enterprise policy controls DBSC.
- Whether Google or other providers refuse a window the person types into when the browser is DevTools-controlled. The search found only the policy text.
- SAML detection from a first redirect: no source was found; the signals are from memory.
- Pop-up sign-in under DevTools control, magic-link behaviour, partitioned cookies in frames, and Windows Hello passkeys under a non-default data directory.
- Whether iCloud Keychain passkeys work in Chrome: sources conflict.

## What this changes in the design

These are proposals. The lead applies them.

1. **Section 215, "Sessions bound to the device".** Replace the paragraph with: "Chrome ships Device Bound Session Credentials. The site opts in at sign-in with a response header; the browser keeps a key in secure hardware and proves it to renew a short-lived cookie. As of research note 35 it is generally available in Chrome on Windows, gated on macOS, and absent on Linux. Whether an engine-owned profile gets a bound session, and what happens without a hardware key store, is not established. A cookie exported from a bound session is expected to stop working at its next renewal, so the keeper marks an entry `movable: false` when it sees the registration header or when a restored session is refused after a short life."
2. **Section 215, "Open points".** Replace the first two bullets with: "Whether a fresh engine-owned profile signs in silently through the device's account: on Chrome for Windows it is expected not to, without the vendor's extension and a signed-in profile (note 35); unknown for Edge, macOS, and Linux. Whether a fresh data directory, an in-memory context, an automated browser, or a machine with no hardware key store gets a device-bound session; and whether the browser reports it."
3. **Section 215, a new short part "How the method is found".** Add: "`auth.detect` names a method from addresses and the first response's headers only: an HTTP challenge scheme, a redirect carrying `SAMLRequest`, a redirect with `response_type` and `client_id` (OIDC where the origin serves a discovery document), a declared identity-provider origin, or a declared sign-in screen. It recommends a mode with the evidence. A passkey, a magic link, and a second factor are not visible in addresses and are named unknown, never guessed."
4. **Section 503, sign-in surfaces.** Add: "The engine never answers a passkey or presence prompt. The DevTools virtual authenticator is used only in tests against the test application. A passkey made in the engine's profile belongs to that profile; the sign-in window's notice says so."
5. **Section 503 or 301, magic links.** Add as a stated limit, kind "not built": "A sign-in by emailed link is not supported. The link opens in the person's everyday browser, which the engine does not drive, and the link is a credential. A route where the person pastes it into the engine's window is open."
6. **Decision 0254 follow-up, a new record, proposed.** "The sign-in window may show an address field that accepts a single address, entered by the person, held to the target's declared origins, for the one case of a link the person copies from their own mail; the engine does not read the field's value into any tool result or log." Status Proposed; it widens what the window is and needs its own test.
7. **Hypothesis H-05 (docs/hypotheses/README.md).** Add: "2026-11-23, note 35: not tested. Search results say Chrome on Windows signs in silently only in a managed, signed-in profile with the vendor's extension; a fresh engine-owned directory has neither. Edge, macOS, and Linux not found."
8. **Brief 30 and brief 6 in 11-open-research.md.** Mark both "Partly answered by note 35"; keep a residual brief: "run Chrome and Edge on Windows and macOS against a site that sends `Secure-Session-Registration`, with a fresh data directory, an in-memory context, and a virtual machine without a TPM, and record whether a bound session is made, whether DevTools reports it, and what an exported cookie does at its next renewal."
9. **`a2m-pack-web-ui` design, expiry.** Add a finding field "device-bound: seen, not seen, unknown" to the sign-in outcome, set from the registration header only when the browser reports it, so `movable: false` rests on evidence and not on a guess.
10. **Roadmap packages 2.3 to 2.6.** No change to order. Add to the proof of the sign-in window package: a test that a passkey prompt in the test application is left unanswered by the engine and ends in `needs_login` after the person's time.

## Sources

- Chrome's DBSC guide, https://developer.chrome.com/docs/web-platform/device-bound-session-credentials (not opened; named in search results)
- W3C Device Bound Session Credentials draft, https://www.w3.org/TR/dbsc/ (not opened)
- Chromium intent to ship, https://groups.google.com/a/chromium.org/g/Blink-dev/c/Y-_oBoAsWy0 (not opened; summary only)
- Google security blog on DBSC, https://blog.google/security/protecting-cookies-with-device-bound-session-credentials/ (summary only)
- Help Net Security, https://helpnetsecurity.com/2026/04/10/google-chrome-device-bound-session-credentials (summary only)
- A practitioner's account of the macOS release and of shipping DBSC, https://scotthelme.co.uk/device-bound-session-credentials-lands-in-chrome-on-macos/ and https://scotthelme.co.uk/everything-i-learned-shipping-device-bound-session-credentials/ (summaries only)
- Workspace admin note on DBSC, https://workspaceupdates.googleblog.com/2026/05/prevent-account-takeovers-with-DBSC-now-generally-available-in-the-Chrome-browser-for-Windows.html (summary only)
- Microsoft Entra primary refresh token, https://learn.microsoft.com/entra/identity/devices/concept-primary-refresh-token (summary only)
- Microsoft community answer on Chrome SSO, https://learn.microsoft.com/en-us/answers/questions/5724246/chrome-sso-not-showing-device-information (summary only)
- Chrome passkeys on desktop, https://developer.chrome.com/blog/passkeys-gpm-desktop and https://support.google.com/chrome/answer/13168025 (summaries only)
- DevTools WebAuthn emulation, https://developer.chrome.com/docs/devtools/webauthn (summary only)
- Google on sign-in from automated or embedded browsers, https://support.google.com/accounts/answer/7675428 and https://developers.googleblog.com/2020/08/guidance-for-our-effort-to-block-less-secure-browser-and-apps.html (summaries only)
- RFC 8252, https://rfc-editor.org/rfc/rfc8252 (summary only)
- A discussion of session-cookie restore in Chrome, https://textslashplain.com/2019/06/24/surprise-undead-session-cookies/ (listed in results; not opened)
