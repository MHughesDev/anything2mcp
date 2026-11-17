# Sessions and policy engines

**Status:** Partial. Built from search-result summaries on 2026-11-15; no primary page was opened in full. It answers part of open briefs 4, 5, and 15, which stay open.

Feeds spec sections [44](../spec/44-session-keeper.md) and [45](../spec/45-policy-engine.md).

## The user's own browser profile is off limits

- Chrome's developers announced that from Chrome 136 the remote-debugging switches no longer work against the default data directory and must be given a separate one, which uses a different encryption key ([Chrome for Developers](https://developer.chrome.com/blog/remote-debugging-port?hl=en)).
- Google's stated reason: after cookies on Windows were bound to the Chrome application, attackers moved to the debugging interface to extract them ([same post](https://developer.chrome.com/blog/remote-debugging-port?hl=en)).
- App-Bound Encryption, introduced in 2024, is the Windows protection that stops other programs reading Chrome's cookie store; malware has repeatedly found ways around it ([SpyCloud](https://spycloud.com/blog/infostealers-bypass-new-chrome-security-feature/), [Red Canary](https://redcanary.com/blog/threat-intelligence/google-chrome-app-bound-encryption/)).
- Security research in 2026 still treats enabling the DevTools protocol inside a user's running browser as an attack technique ([SpecterOps](https://specterops.io/blog/2026/08/13/chrome-devtools-protocol-cookie-theft/)).

**Consequence.** An automation tool that drives or reads the user's everyday profile is doing what browser vendors are actively closing off and what security tools look for. Anything2MCP uses its own data directory for every identity and never reads another program's cookie store. The earlier claim that the engine can attach to a browser already running on a user's device is withdrawn.

## Sessions are becoming bound to devices

- Device Bound Session Credentials tie a session to a key pair created at sign-in and kept in secure hardware; cookies are short-lived, and the browser must prove it holds the key before they are renewed ([Chrome for Developers](https://developer.chrome.com/docs/web-platform/device-bound-session-credentials)).
- The feature reached general availability on Windows in Chrome 146 in April 2026, with macOS following ([Help Net Security](https://www.helpnetsecurity.com/?p=365699)).
- A support table checked in October 2026 lists Chrome and Edge on Windows and macOS, and no support on Linux ([Corbado](https://www.corbado.com/blog/device-bound-session-credentials-dbsc)).
- The same source notes the limit: a stolen short-lived cookie still works until it expires; what is blocked is renewing it elsewhere.

**Consequence.** A session on a site that uses this cannot be saved on one machine and restored on another. The session keeper marks such sessions not movable, and losing their runner means signing in again. Behavior on servers and virtual machines without a hardware key store was not established.

## Under Entra, the organization sets how long a browser stays signed in

- Conditional Access session controls include sign-in frequency and persistent browser session ([Microsoft Learn](https://learn.microsoft.com/entra/identity/conditional-access/concept-conditional-access-session)).
- A persistent browser session is what lets a user stay signed in after closing and reopening the browser ([Microsoft Learn](https://learn.microsoft.com/en-gb/entra/identity/conditional-access/concept-session-lifetime)).
- Microsoft replaced configurable refresh and session token lifetimes with these controls in January 2021 ([Microsoft Learn](https://learn.microsoft.com/nb-no/Entra/Identity/conditional-access/howto-conditional-access-session-lifetime)).
- Sign-in frequency is a ceiling on how long a session may last, not something activity extends, according to an answer on Microsoft's own forum ([Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/5852707/we-have-a-conditional-access-policy-not-honoring-t)).
- Continuous access evaluation lets a session be revoked in near real time when a user is disabled or a password changes ([Microsoft Learn training](https://learn.microsoft.com/en-us/training/modules/plan-implement-administer-conditional-access/9-implement-session-management)).
- Token protection is a separate control that forces device-bound tokens ([Office 365 for IT Pros](https://office365itpros.com/2026/09/08/continuous-access-evaluation-cae/)).

**Consequence.** The keeper can prevent an application's idle timeout and renew tokens, but it cannot extend the organization's maximum age, and it should not try. It learns the period from history and warns the person ahead of time. Where a session may not survive the browser closing, it keeps the browser open for a bounded window.

## A policy engine in Rust

- Cedar is an open-source authorization language with a production engine written in safe Rust ([Cedar security](https://docs.cedarpolicy.com/other/security.html)).
- Its decision rule: deny unless a permit applies; any matching forbid denies; a policy that errors during evaluation is skipped ([Cedar authorization](https://docs.cedarpolicy.com/auth/authorization.html)).
- The same page explains the design intent: forbids act as guardrails that later permits cannot cross.
- The crate is `cedar-policy`, at version 4.11 or 4.12 in search results ([docs.rs](https://docs.rs/crate/cedar-policy)).
- A 2026 engineering note from another project describes moving its sandbox policy to Cedar, citing a formal specification with the Rust implementation tested against it ([NVIDIA OpenShell](https://nvidia.github.io/OpenShell-Research/dev-notes/posts/2026-11-09-cedar-policy-language/)).

**Consequence.** Cedar's semantics are exactly the two properties Anything2MCP's policy needs, in a Rust crate. The skip-on-error rule could let a broken forbid fail open, so policies are validated before publishing and any evaluation error is treated as a denial. Its license, binary size, and speed were not checked and need a spike.

## Not verified

- Whether a new browser profile signs in silently through the device's account on each operating system and browser.
- How to detect a device-bound session from the outside.
- Cedar's license, footprint, decision latency, and the state of its partial evaluation.
- Which token claims carry groups and roles for each identity provider, and their size limits.
