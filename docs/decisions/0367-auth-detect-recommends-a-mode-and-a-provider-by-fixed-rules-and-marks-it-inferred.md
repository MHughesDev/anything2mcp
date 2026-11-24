# 0367. `auth.detect` recommends a mode and a provider by fixed rules, and marks it inferred

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), in package 2.6. The owner had not read this record when it was merged. The number 0399 is a placeholder the lead renumbers on merge.

**Date:** 2026-11-24

**Changes:** [section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md) ("Finding out how a target signs in"); the designs of [`a2m-auth`](../architecture/crates/a2m-auth.md) (work item 5) and [`a2m-pack-http`](../architecture/crates/a2m-pack-http.md); the [roadmap](../plan/roadmap.md) row 2.6. Carries out [decision 0321](0321-a-sign-in-method-is-found-from-addresses-and-a-passkey-or-a-mailed-link-is-the-persons.md) and [decision 0085](0085-the-session-keepers-interface-as-building-it-settled-it.md)'s open point on `recommend`. Leaves [decision 0248](0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md) as it stands.

## Context

The web pack's `web_auth` discoverer classifies how a start page asks for a sign-in from the page's structure and where it ended, and sends nothing. Nothing used its findings: `auth.detect` showed only what the target's files declare and a count of findings by kind, and said "no rule recommends an identity mode the files do not declare, so none is guessed". Section 215 promised a fixed table from findings to a mode, a provider, and reasons. The HTTP pack found a description's operations and ignored its `securitySchemes`, and it recorded a surface's `WWW-Authenticate` schemes in an `endpoint` finding that no rule read.

## Decision

1. **`recommend(findings) -> Recommendation` of `a2m-auth` is a table, never a model (rule 1).** It reads the findings of kind `sign_in_method` and no others. Each names a sign (`SignInSign`, thirteen names) and the table has one row for each: the mode of `auth.yaml`, the provider, other providers that suit it, and a reason in fixed words. The same findings in any order give the same answer. The result holds no credential and no text the system supplied: a scheme's name or a realm never reaches it.
2. **A sign no row holds is a question, never a guess (rule 7).** A finding that names a sign the table lacks, or that cannot be read, adds one question and `method` to `unknown`. Status 401 with no scheme the rules know, the sign `challenge`, is a question. With no choice at all, `identity_mode` is unknown. A password page or a move to another identity page leaves `second_factor` unknown unless a finding says `asked`, because a second factor, a passkey, and a mailed link do not show before a sign-in is sent (decision 0321).
3. **The rows.** A password form is `managed_session` with `form_login`, and `window_login` for a person who types it themselves. A page that asks for a one-time code is `window_login`, since the engine never answers a second factor. A move to another origin is `sso_oidc`: single sign-on by OpenID Connect and by SAML look alike from outside, so it is reported as `sso_oidc` unless a `SAMLRequest` form post was seen, which is `sso_saml`. The web pack cannot see a form post while it reads and sends nothing, so today only `sso_oidc` arises from a page. An API key, Basic, Digest, a bearer token, Negotiate or NTLM, OAuth with a client's own credentials, and mutual TLS are `custom` with `api_key`, `http_basic`, `http_digest`, `bearer_static`, `windows_integrated`, `oauth_client_credentials`, and `client_certificate`. OAuth with a person's consent, and OpenID Connect declared by a description, are `managed_session` with `oauth_authorization_code`. `obo_token` stays deferred: it needs the caller's own token carried from the front door to the worker, and no sign names it.
4. **The recommendation is evidence for a person to confirm, marked `inferred`, beside what the files declare.** `auth.detect` gives each surface a `recommendation` object (`level`, `mode`, `provider`, `choices`, `questions`, `unknown`, `from`), and sets the surface's `level` to `inferred` only when the files declare nothing and a choice was found. What the target's files declare is kept as declared and is never replaced: the drafter keeps what is declared ([decision 0303](0303-a-declared-sign-in-is-kept-and-a-read-that-waits-for-one-is-reported-as-waiting.md)) and uses a recommendation only where nothing is. A person confirms by signing in; the engine holds no credential it was told here. Nothing in `auth.detect` signs in, and the identity of the person is never part of the answer.
5. **The HTTP pack classifies a sign-in from two places, and both are `sign_in_method` findings of the HTTP surface kind.** `http_description` reads a description's `components.securitySchemes` (OpenAPI 3) or `securityDefinitions` (Swagger 2), one finding for each distinct sign, at the level `inferred` and from a published description, with the limitation that nothing was sent with it. A scheme no rule covers, such as OAuth's password flow, is the sign `unrecognized`, which the table turns into a question. `http_surface` reads the `WWW-Authenticate` schemes of its own two probes and gives one finding for each distinct sign at the level `discovered`. Only the scheme's first word is read, and it is lowercased and mapped: an unknown word is the sign `challenge`.
6. **The names are held equal by tests.** The web pack's four signs are a subset of `SignInSign`, held by a test of the product (`the_signs_the_web_pack_names_are_signs_the_rules_know`); the HTTP pack holds its own by a test that parses each of its signs.

## Alternatives weighed

| Alternative | Why not |
| --- | --- |
| Let `recommend` read the HTTP pack's `endpoint` finding and its `challenges` list | The kernel crate would read a pack's own finding form. A pack names its signs in a finding of the neutral kind instead |
| Put the table in `a2m-actions` | Layer 4 holds no knowledge of providers; the table is about auth providers, and `a2m-auth` already names them |
| Choose one provider when several signs are found | A description often declares two schemes and the target's files, not the engine, say which the target uses. All choices are listed in the order of the table, with the first named in `mode` and `provider` |
| Open a question in the unknowns ledger for each unknown | The scout already opens the questions of its own stages; `auth.detect` is read only, so its questions are in its answer. A question that survives into a scout is opened by the scout |

## Proof

- `a2m-auth`: the table's rows, the SAML-or-OIDC rule, the unknown sign, and the order-independence, in `crates/a2m-auth/src/tests/recommend.rs`.
- `a2m-pack-http`: the scheme rules (`each_declared_security_scheme_is_classified_by_a_fixed_rule`), the challenge rules, and the finding counts of the stand's own description (11 operations and 2 signs), in the pack's tests.
- Through the product, `crates/anything2mcp/tests/sign_in_detect.rs`: each of the three variants of the test application's case 13 (a password form, a move to an identity page on another origin, and the form with the one-time code step switched on) is classified and recommended by `auth.detect`, and the application, asked afterwards, counted no sign-in form, no code, and no sign-in at its identity page; and the stand's description gives `api_key` and `bearer_static` for an `http` surface.

## Left

- A form post that carries a `SAMLRequest` is not seen, so `sso_saml` is not recommended from a page. A discoverer that follows the move would have to send a request to the identity page, which `web_auth` does not do.
- A client certificate asked for in a TLS handshake, whether the device has an operating-system broker, and the audience and scopes of a token in captured traffic are among the signs section 215 lists and are not detected yet.
- The recommendation is shown and not drafted: the scout does not write an `auth.yaml` from it.
