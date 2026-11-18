name: sessions

# Sessions and sign-in

A session is the signed-in state of one identity on one target in one environment. Only the session keeper signs in or renews. A driver, a run, and you never do.

## What you can and cannot do

- You can ask whether a target is signed in: `session.status`.
- You can ask the person to sign in: `session.login`, with the target and, if you want, the surface and the environment. Never pass a credential: the action has no field for one.
- You can end one of your person's own sessions: `session.revoke`, with the session's identifier from `session.status`.
- You cannot sign in, read a cookie or a token, or use another person's session. Nobody can, an administrator included.

## Signing in, step by step

1. Declare the sign-in in `auth.yaml` with `draft.write`: the surface, the provider, a probe (a drafted read, with `probe_inputs` if it takes any), and a vault reference for each credential, such as `token: "vault://example/service/token"`. No value goes in the file. A probe must be an operation that compiles, so a first scout comes before it.
2. Call `session.login` with the target. If every credential is stored and the provider needs no person, the keeper signs in at once. Otherwise you get `state: needs_person` and a `link`: give the link to your person. On that page they read what will be done, type each credential that is not stored, press Carry out, and answer this machine's own prompt. If the system then asks for a one-time code, the page asks them for it.
3. Call `session.login` again. It says `signed_in`, for whom, the environment, until when, and the `level`: `confirmed` when the system accepted the credential, `held` when the provider only holds it and the first use tests it.
4. Call `scout.run` again. A scout that stopped with `state: needs_sign_in` goes on under the session. A run that rests at `needs_login` is resumed with `run.resume`.

A scout stops by name, before it drafts, at a surface whose declared sign-in has no session. Its report says, for each surface, whether what was found there needed the session.
- A session signed in as a person is used only by that person, or an agent acting for them.
- No call names the account it runs as. Policy picks it.

## States

`absent`, `signing_in`, `valid`, `suspect`, `renewing`, `needs_person`. A call on an `absent` session fails with `session_expired`, `needs_login`. Tell the person, wait, then call again or resume the run.

## `auth.yaml`

A map with one key, `auth`: a list, one entry per group of surfaces that share a sign-in.

| Field | Meaning |
| --- | --- |
| `surfaces` | The surfaces this entry covers |
| `mode` | `use_existing`, `managed_session`, or `custom` |
| `provider` | An auth provider, such as `form_login` |
| `probe` | A cheap signed-in read: an operation of the target. Name `probe_inputs` for it |
| `expiry_signals` | Signs that the session ended: `redirect_host`, `status` with `path`, or `visible`; on a web surface, `screen`, a screen whose `ready` shows |
| `timeouts` | `idle_ms` and `absolute_ms`, when known |
| `settings` | The provider's own names and plain values. Never a credential |
| `credentials` | For `custom`: a role (such as `password`) to a vault reference |
| `attempt_limit` | Automatic sign-in attempts with stored credentials. Default 1 |

```yaml
auth:
  - surfaces: [app]
    mode: custom
    provider: form_login
    probe: example_account_get
    expiry_signals:
      - {screen: sign_in}
    settings:
      screen: sign_in
      user_field: username
      pass_field: password
      submit: sign_in
      refused: refused
    timeouts: {idle_ms: 1800000}
    credentials:
      user: "vault://example/app/user"
      password: "vault://example/app/password"
    attempt_limit: 1
```

For `form_login`, every setting names a part of a screen model in `screens/`: the sign-in screen, its two fields, the action that submits, and the variant that shows a refused credential. No locator goes in `auth.yaml`. Every credential reference starts with `vault://` and the target's own name.

`auth.detect` says how each surface signs in, as claims for a person to confirm: whether the surface's kind keeps a session, which mode and provider `auth.yaml` declares for it, and whether its probe was answered or met a sign-in it needs. It recommends no mode or provider the files do not declare; those are named as unknown. A person confirms it by signing in.

## Rules

- The engine drives a browser it owns, with a data directory it owns. It never reads or copies a person's own browser profile.
- A credential is a vault reference in a file, never a value. A person gives it on the sign-in page, or sets it beforehand with `secret.set`, from a prompt or standard input.
- A surface with no sign-in still has a session, with an empty lease.
- Do not try to get past a second factor, a CAPTCHA, or a block. They are named stops.
