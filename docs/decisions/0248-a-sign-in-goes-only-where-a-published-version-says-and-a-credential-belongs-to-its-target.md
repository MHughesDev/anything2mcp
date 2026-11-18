# 0248. A sign-in goes only where a published version says, and a credential belongs to its target

**Status:** Accepted. Design decision. The owner had not read this record when it was merged. Points 1 to 3 carry out the lead agent's direction for the safety audit of 2026-11-18; the wording is the building agent's.

**Date:** 2026-11-18

## Context

A safety audit of the sign-in built by [decision 0205](0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md) found a chain (audit 5, findings F1, F2, F3, F4, and F6). Each was checked in the code before it was fixed.

- **F1, critical, confirmed.** `session.login` built the keeper's entry from the plan `plan_in` gave: the drafts', in a project that holds the target. For a provider that needs no person, `oauth_client_credentials`, with the client secret already stored, nothing was missing, so the handler skipped the person and called `Keeper::sign_in`. The worker called the provider with no check of where it sends, and the provider posted the stored secret to the drafted `token_endpoint` with its own HTTP client; the only check was that the address be `https` or a loopback IP address. An agent that may write drafts could choose where a stored secret went, with no publish and no person.
- **F2, serious, confirmed.** A credential's seal context was the workspace, the owner, the kind, and the reference: no target. A reference was checked for its shape only, so target A's `auth.yaml` could name target B's `vault://b/...` and use B's secret. The publish review did not name any reference.
- **F3, serious, confirmed, and wider than the audit said.** A sign-in from drafts was live, with no sandbox and no publish. A run under drafts was too: a trial or a discovery runs a drafted plan in the sandbox, but its lease still asked the keeper, which signed in from a stored credential on the drafts' entry, and then handed the session to a driver that sends to the drafts' surface addresses.
- **F6, minor, confirmed.** The person's page named the surface, the target, the environment, and the provider, and never an address.
- **F4, minor, confirmed as a reading of the code.** The connector takes the engine's "stopped" line from the engine's log by the child's process identifier, which another program of the same user can learn and write.

**The proof of the fault before the fix.** The test `a_drafted_sign_in_sends_no_stored_credential_until_its_person_sees_every_address` (in `crates/a2m-actions/tests/build.rs`) has the person store a client secret through one drafted sign-in, then has the agent move the drafted token endpoint to a second copy of the test application ("elsewhere") and call `session.login`. It was run against the code as it stood before this record, with only the test added. Elsewhere's trap counted the token request, carrying the stored secret, and the agent's call returned without the person being asked. With the fix the same test passes: elsewhere hears nothing, and the answer is the person's page.

## Decision

### 1. What a target's files say counts only once a person has published it

A sign-in is a run of declared steps to declared places, held to the rule every request is held to.

- **The entry says where its plan came from.** `EntrySurfaces` (on `AuthEntry`) carries `from_published`, which only the caller that loaded a published version's plan sets, and `shown_to_person`, which only the caller that took the person's act sets. `a2m-actions` fills both in `control::entry_of`, from the same plan the entry is built from: for a run, the plan is a drafts' when it is one the kernel keeps in memory as a draft, and a published version's otherwise; for `session.login`, from where `declared` found the declaration.
- **A stored credential is used only under a published version's entry.** The keeper refuses, with `session_expired`/`needs_login` and a sentence that says the sign-in is declared by drafts, each of: a lease's automatic sign-in from a stored credential under a drafts' entry; a lease under a drafts' entry whose credentials have run out; `Keeper::sign_in` with an entry neither published nor shown to the person. The worker refuses again, before any provider is called: `Work::SignIn` needs `from_published` or `shown_to_person`, and every `Work::Renew` needs `from_published`.
- **From drafts, `session.login` only asks the person.** Where the published version does not declare this sign-in exactly as the drafts do (the same provider, settings, references, surfaces, addresses, and declared destinations, compared by one digest), the declaration is the drafts', and every call, an agent's or not, goes to the person's page. A drafts' declaration that is the published version's in all of these counts as the published version's.
- **The page shows every address before anything is typed or sent (F6).** It names the surface, the provider, whether the declaration is published or the drafts' and "nobody has published it", and one line for every address a credential or a token of the sign-in is sent to, with what the address is (the surface `desk`, the setting `token_endpoint`), each marked "as the drafts declare it, not published" or "as the published version declares it", and says it is sent to no other address. The input is pinned with the declaration's digest; a press after the drafts changed is refused with `stale`/`drafts_changed` and sends nothing. The person's press marks the entry `shown_to_person`: it allows that one sign-in to those addresses, and no renewal.
- **A run under drafts uses a session only where its person allowed.** The keeper keeps, for each session, the addresses its person was shown when they carried out a drafted sign-in, and those a published version's entry declares. A lease under a drafts' entry is refused with `needs_login` when the entry would send the session's credentials to any other address; `Keeper::allows` answers the same question for `session.login`, which says a session holds only when it does. A drafts' entry never replaces a published one as the entry the keeper renews under.
- **Renewal.** The keeper renews a published sign-in without the person, against the addresses of the published entry it was last leased under. A session whose last sign-in was made under drafts is never renewed without its person, under the drafts or under a published version: the ladder goes straight to `needs_person`. Both records are in memory: after a restart, a run under drafts is refused until the person carries out the sign-in again on its page (no request is sent for a session that holds; the press only allows the addresses again).

### 2. Every address a provider sends to is a destination like any other

`SignInDestinations` is a plain value in `a2m-auth`, filled by `EntrySurfaces::of` from every surface address the plan declares in the session's environment: the allow-list a run's requests are held to (`Destinations` in `a2m-provider`, which layer 4 cannot name). An `http` or `https` address is allowed when its origin, scheme and host and port, is a declared surface's origin; any other declared address allows only itself. It has no setter.

`AuthProvider` gains one method with a default, `destinations(&self, entry) -> Vec<SignInAddress>`: every address beyond the entry's own surfaces that the provider itself sends a credential, a token, or the person to. The keeper checks each against the entry's `SignInDestinations` before any work (`sign_in`, `lease`, and the worker's `run`, which also refuses a sign-out's call to the provider), and the provider checks again with `SignInContext::check_address` just before each request. An address outside is `policy`/`destination_not_declared`, and nothing is sent. A redirect is never followed: the OAuth client's policy was already `none`, and a redirect answer is now named, `unknown_variant`/`unexpected_response`, "it answered with a redirect, which is not followed". `Keeper::sign_in_addresses` gives the checked list, the entry's surfaces and the provider's addresses, to the page and the publish review.

| Provider | Every address a secret, token, or the person can be sent to | What bounds it |
| --- | --- | --- |
| `api_key` | The key, in a header of the driver's requests to the entry's surfaces | The run's `Destinations` in the driver (`surface_not_declared`); the provider sends nothing itself |
| `bearer_static` | The token, the same | The same |
| `http_basic` | The password, as the `Basic` header, the same | The same |
| `http_digest` | The password, answered by the driver to a challenge on its connection to the surface | The same |
| `windows_integrated` | The password, for Negotiate or NTLM, the same | The same |
| `client_certificate` | The certificate and its key, as the TLS identity of the driver's connection to the surface | The same |
| `oauth_client_credentials` | The client secret, to `token_endpoint` (in the `Authorization` header or the body); the access token, to the entry's surfaces through the driver | `token_endpoint`: `SignInDestinations` (keeper before the work, provider before the request), `https` or a loopback IP address, no redirect, no proxy. The token: the run's `Destinations` |
| `oauth_authorization_code` | The person's browser, to `authorization_endpoint` with the client identifier, state, and proof-key challenge; the code, the proof-key verifier, the client secret if any, and the refresh token, to `token_endpoint`; the access token, to the entry's surfaces | Both endpoints as above; the link is checked before the person is given it. The token as above |

The pack providers are their packs': `form_login` (decision 0245) sends only to its covered surface's sign-in page and refuses every drafts' entry; `upstream_mcp` sends to its surface. Neither returns addresses of its own, which the default says.

### 3. A credential belongs to the target it was stored for

- A reference's first part is the target's name: `vault://<target>/...`. The keeper refuses an entry that names another target's reference, or a reference whose first part is no target's name, with `input`/`credential_of_another_target`, before anything is opened or sent. `StoredCredentials::open` refuses it again.
- The vault seals a credential with the target in its context: `SealContext::credential(workspace, owner, target, reference)`, whose item is `credential:<target>:<reference>`. `StoredCredentials::put` seals for the target the reference names; `open` opens under the session's target. A secret sealed for one target does not open under another, even under the same reference text, and the vault's test shows it.
- **Credentials stored before this record.** Nothing is released, so none needs to survive. One sealed under the old context does not open; `open` names it `input`/`credential_unreadable` and tells the person to store it again, and the person's page offers the field to type it.
- **The publish review** has one line per sign-in and environment: the surfaces it covers, the provider, each credential by role and reference (never a value), and every address a credential or a token of it is sent to. A sign-in the keeper would refuse (an undeclared address, another target's reference, a setting the provider refuses) is refused before the person is asked. A provider this engine does not hold is named on its line as one whose addresses are not known here, and is not refused at publish: every sign-in through it stops by name at run time.

### 4. F4 is not fixed, by choice

The stopped line changes only the sentence a failed start reports. A program that can write the engine's log runs as the same user, and that user's programs are inside this machine's trust boundary: such a program can already read the vault passphrase from the starting shell's environment, change the project, or stop the engine. A nonce passed on the child's standard input does not establish authorship against it either, because the same user may open the child's descriptors. The fix the audit proposes would buy nothing that boundary does not already give away. **Kind:** Choice, this record. The file is `crates/a2m-local/src/start.rs`, which another agent owns; nothing was changed there.

## What changed for the signed-in first hour, and for renewal

- **The first hour** (`crates/anything2mcp/tests/first_hour_signed_in.rs`) already went through the person's page, because nothing was stored. The page now also says the sign-in is declared by drafts and not published, and names the surface's address as where the token goes; the press carries the pinned declaration. The session the press made is allowed for the drafts' addresses, so the agent's second `session.login` says it holds, and the scout's discovery and trials under the drafts use it. Drafts that moved the surface to another address would make the scout stop with `needs_sign_in`, and `session.login` would show the person the new address. After the publish, the tool's run leases the session under the published entry.
- **Renewal** of a published sign-in is unchanged. Renewal under drafts no longer happens without the person.

## What was weighed

- **Allowing a drafted sign-in with a stored credential when every address it sends to is already a declared destination of a published surface.** Rejected. It is safe in what it sends where, but it makes "may an agent sign in without the person" depend on a comparison of two plans the person never sees, and a drafted entry can change what is sent there (another reference, another scope, another client) as well as where. The digest rule is the narrow form of it: a drafts' declaration that equals the published one in everything that decides what is sent where is the published one.
- **Refusing every drafted sign-in, as `form_login` does.** Rejected for the neutral providers: the scout needs a session before anything can be published, and the person's page is the gate a publish would be.
- **Checking destinations in `a2m-actions` alone.** Rejected: the keeper is the one signer, and the check must hold for every caller of it.
- **A target field in `SealContext`.** Rejected for now: the struct's fields are public and other crates build it whole. The target is authenticated through the item, built by one constructor.

## What is still open

- **The compiler does not refuse an undeclared address or another target's reference.** It holds no list of providers. The refusal is the publish review's, the person's page's, and the keeper's. A line in `a2m-files` that refuses a reference whose first part is not the target's name would make the compile say it too.
- **The session's allowances and its "made under drafts" mark are in memory.** After a restart a drafts' run is refused until the person presses again, which is safe; a session made under drafts and restored after a restart could then be renewed silently under a published entry, at the published addresses. Keeping the mark in the session's row would close that.
- **A published version's later addresses.** A session signed in under one version is renewed against the published entry it was last leased under, which may be a later version's. A person published that version and was shown its addresses.
- **The pack tests** of `a2m-pack-http`, `a2m-pack-data`, and `a2m-pack-mcp` build entries with `AuthEntry::new` and lease them without saying they are published; each needs `.published()` on that entry.
