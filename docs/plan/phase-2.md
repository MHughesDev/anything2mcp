# Phase 2: Sessions

A draft task list, to be revised at the end of Phase 1 and agreed with the owner before starting.

**Outcome.** The engine signs in to an application once, as the person, in its own browser profile, keeps that sign-in alive within the organization's rules, shares it between runs, and never lets a secret out.

**Exit test.** An expired session stops every call with `needs_login`; ten runs that meet an expired session cause exactly one renewal; no secret appears in any trace, capture, log, or result; a read on one real application chosen by the owner, whose target files stay outside this repository, gives five identical runs.

**Read first.** The Phase 2 entry in the [phase map](phase-map.md). Brief 30 (sign-in in engine-owned profiles) decides how much of group C is possible; run it first.

## Task groups

| # | Group | Tasks | Done when |
| --- | --- | --- | --- |
| A | Vault | The four key layers of spec section 48; the operating-system key store back end on three systems; the passphrase route for Linux without a desktop session; the secret wrapper type; `secret.set` | A saved session cannot be read without the key store; no key is ever on disk in the clear; `doctor` names the store in use |
| B | Redaction | The redactor and its registry; header and cookie allowlist; the second scan before evidence leaves | A CI test plants a known secret in the test application's traffic and finds it in no evidence, log, or result |
| C | Auth providers | Form sign-in; redirect sign-in through a separate identity page; a one-time code step entered by a person; basic authentication; static keys and bearer tokens | Test-application case 13 passes for each |
| D | Discovery | `auth.detect`: the signals in spec section 15, with evidence and a recommended mode | It classifies each sign-in variant of the test application correctly |
| E | Session keeper | The eight states; the probe; the renewal ladder; one renewal at a time; the keep-alive window; learned clocks; sessions that end with the browser; a stored credential tried once | Case 14 passes; ten concurrent runs on an expired session cause one renewal; a refused stored credential is not retried |
| F | Sign-in window | A visible window of the engine's own browser, in the identity's profile, with its purpose shown | A person can sign in with a password manager and a one-time code; the engine never sees the password |
| G | Sharing and isolation | Runs as tabs on one live session; one browser process and profile per identity; the exclusive-session check | Case 15 is detected; two identities never share a process |
| H | Session actions | `session.status`, `session.login`, `session.revoke`; the session timeline | Revoking closes the tabs, deletes the profile and saved state, and is recorded |
| I | Diagnosis | `doctor.run`: browser, key store, network reach, session health, clock | Each check has a test that makes it fail and a printed fix |
| J | A real application | With the owner's go-ahead in that session: one read tool on one real application | Five identical runs; nothing about the application is committed to this repository |

## Not in this phase

Writes, approvals, tokens obtained on a caller's behalf, the operating system's sign-in broker, hosted sign-in on a server, and moving sessions between runners. Those arrive with Phases 3 and 7.

## Questions for the owner

- Which real application, and who has agreed to it being used.
- Where its target files will be kept.
- What the organization's sign-in rules are for that application, as far as the owner knows them.
