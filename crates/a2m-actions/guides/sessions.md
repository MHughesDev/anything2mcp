name: sessions

# Sessions and sign-in

A session is the signed-in state of one identity on one target in one environment. Only the session keeper signs in or renews. A driver, a run, and you never do.

## What you can and cannot do

- You can ask whether a target is signed in: `session.status`.
- You can ask the person to sign in: `session.login`. A person signs in, in a window the engine owns. Their password and any second factor stay in that window.
- You cannot sign in, read a cookie or a token, or use another person's session. Nobody can, an administrator included.
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
| `expiry_signals` | Signs that the session ended: `redirect_host`, `status` with `path`, or `visible` |
| `timeouts` | `idle_ms` and `absolute_ms`, when known |
| `settings` | The provider's own names and plain values. Never a credential |
| `credentials` | For `custom`: a role (such as `password`) to a vault reference |
| `attempt_limit` | Automatic sign-in attempts with stored credentials. Default 1 |

```yaml
auth:
  - surfaces: [desk]
    mode: managed_session
    provider: form_login
    probe: example_order_get
    probe_inputs: {order_id: "SO-1001"}
    settings: {sign_in_path: "/login"}
    timeouts: {idle_ms: 1800000}
    credentials:
      password: "vault://example/service/password"
    attempt_limit: 1
```

`auth.detect` says how each surface signs in, as claims for a person to confirm: whether the surface's kind keeps a session, which mode and provider `auth.yaml` declares for it, and whether its probe was answered or met a sign-in it needs. It recommends no mode or provider the files do not declare; those are named as unknown. A person confirms it by signing in.

## Rules

- The engine drives a browser it owns, with a data directory it owns. It never reads or copies a person's own browser profile.
- A credential is a vault reference in a file, never a value. A person sets a secret with `secret.set`, from a prompt or standard input.
- A surface with no sign-in still has a session, with an empty lease.
- Do not try to get past a second factor, a CAPTCHA, or a block. They are named stops.
