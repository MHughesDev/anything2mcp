# 0205. A sign-in an agent asks for is carried out by its person on the engine's page

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the designs of [`a2m-actions`](../architecture/crates/a2m-actions.md), [`a2m-local`](../architecture/crates/a2m-local.md), and [`a2m-auth`](../architecture/crates/a2m-auth.md); sentences of spec sections [215](../spec/2-kernel/215-sessions-and-the-session-keeper.md) and [503](../spec/5-security/503-approval-and-sign-in-surfaces.md); the `session.login` row of the [action catalog](../reference/action-catalog.md); the agents' guide `sessions`. Builds on decisions [0163](0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md) and [0196](0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md).

## Context

The first hour of a system that needs a sign-in stopped at `session.login`, which was not built, and so did the `add` shortcut. Sections 215 and 503 said the engine never asks for a password in its own interface; they were written for a sign-in that is a web page, whose provider (`form_login`, in the web pack) is not built. The providers that are built hold or exchange a credential (`bearer_static`, `api_key`, `http_basic`, OAuth), and their credential must be in the vault before the keeper can sign in.

Unserved at the start (44 of the catalog's actions; the four `engine.*`/`self.update` are the client's): `browser.install`, `capture.start`, `capture.stop`, `client.connect`, `component.list`, `config.set`, `delegation.grant`, `delegation.list`, `delegation.revoke`, `endpoints.infer`, `grant.add`, `grant.list`, `grant.remove`, `plugin.install`, `plugin.list`, `policy.publish`, `policy.test`, `provider.disable`, `provider.enable`, `run.trace`, `run.undo`, `runner.drain`, `runner.enroll`, `runner.list`, `runner.revoke`, `screen.snapshot`, `session.end`, `session.list`, `session.login`, `session.revoke`, `shadow.report`, `spec.import`, `toolset.create`, `toolset.list`, `toolset.update`, `trace.export`, `unknown.settle`, `vault.rotate`, `version.activate`, `version.bundle`, `version.diff`, `version.import`, `version.migrate`, `version.rollback`.

## Decision

| Question | Decided | Alternative weighed |
| --- | --- | --- |
| What an agent names | `target`, and optionally `surface` and `environment`. Never a secret: the schema refuses any other field. The account is the caller's person; a dedicated account's name waits for toolsets | An `account` input. No identity but the caller's person exists on one machine yet |
| Which sign-in | The one the target's plan declares for the surface (the drafts in a project, the published version otherwise), turned into the keeper's entry by the same function a run's lease uses (`control::entry_of`), so the session a person signs in is the one a run leases. None declared: a success, `state: not_needed`, saying how to declare one | Reading `auth.yaml` apart from the plan. Two readings of one file would drift |
| When a person is asked | When the caller is an agent and the provider needs a person, or a credential the entry names is not stored for the person. The handler hands the act to the person surface itself (`Kernel::ask_person`, as `publish.request` does), so a sign-in that needs nobody is not shown to anyone. A session that holds already is said to, and nobody is asked | Marking the action a person's act always. A stored credential and a provider that needs no person would put a page before every sign-in for nothing |
| Where a credential is typed | On the act's page: an empty password field for each credential role, named by its vault reference. The press carries the fields with the one-time value (at most 16 KiB); only fields the page asked for are kept, each is a `Secret` from the moment it is read, and `ApprovalDesk::carry_out_with` registers it with the redactor before anything else and hands it to the handler in `Call::given`, never in the input, the result, or what is kept of the act. The handler seals it for the person under the entry's reference, as `secret.set` seals, and writes an audit line | The page signing in by itself, or the keeper taking the secret in a job. A job crosses the runner link, and a secret never rides in one |
| A one-time code | The provider asks through the keeper's own way (`PersonAsk::EnterCode`). The local surface answers it on the page of the same person's press of a sign-in while that press is carried out: the press's answer becomes a page that takes the code; its post hands the code to the provider once and answers with what the act came to. A post not from that page ends the sign-in. With no such press in flight the ask is `Unreachable`, and the keeper names `needs_mfa` | A code field on the first page. The person does not know yet whether a code will be asked |
| What the agent is told | `signed_in` (or `signing_in` when the keeper's wait ran out), the session's identifier, target, surface, environment, for whom, the provider, when signed in and until when, and `level`: `confirmed` when the provider's sign-in means the system accepted the credential, `held` when it only holds it. A failed sign-in is its failure by name, the system's words apart as `said` | Returning a lease or a handle. A run finds its session by target, environment, and person; nothing need be passed |
| Kept outcomes | A sign-in's outcome is not given back from the acts' kept list: the handler answers from the keeper's own state, and a sign-in asked again after a session ended is a new act | Reusing the kept answer for the window, as other acts do. It would say `signed_in` after a revoke |
| The scout | A discovery that fails `session_expired` or `credential_missing` marks its surface `needs_login`; the scout then stops before drafting, by name (`state: needs_sign_in`, exit person needed, `next: session.login`), storing no stage, so the same scout identifies again under the session. Each surface in the report says whether its findings needed the session (`session.needed`, with the provider) | Drafting from a look that missed the surface |
| `session.revoke`, `session.list`, `session.end` | Built beside it, over `Keeper::revoke` and `Keeper::status`, each audited; another person's session is not found | |
| The catalog's name | `session.revoke`, as the catalog has it, for what the brief called a logout | A new name |
| Spec 215 and 503 | Narrowed to what is true: the engine asks for no password of a sign-in that is a web page; a credential a provider holds is typed on the engine's own page or stored with `secret.set` | Keeping the sentence and refusing to build the sign-in |

Additions to `a2m-auth`: `StoredCredentials::holds` (nothing opened), `Keeper::signed_in` (waits for the outcome of a sign-in it started; signs in nothing), and `Returned::iter`, `is_empty`, `only`.

The `add` shortcut is unchanged; every one of its steps is built, and it stops by name at the scout when a declared sign-in has no session.

## Unserved at the end

40, each waiting for what is named: `unknown.settle` (the ledger settles by evidence; settling a question does not yet rewrite the draft it holds, so the scout's drafts would still not compile); `run.trace` and `trace.export` (a summary format of the trace, and a file write of evidence); `version.diff`, `version.rollback`, `version.activate`, `version.bundle`, `version.import`, `version.migrate` (the version store's operations, signed bundles, toolsets); `config.set` (writing a settings layer with the protected-setting rules); `client.connect` (per-client configuration files); `component.list`, `browser.install`, `provider.enable`, `provider.disable`, `plugin.install`, `plugin.list` (the component and plug-in managers); `capture.start`, `capture.stop`, `endpoints.infer`, `spec.import`, `screen.snapshot` (the packs' recording and inference); `shadow.report` (left to the lead with decision 0207); `run.undo` (reverse diffs, phase 9); `toolset.*`, `grant.*`, `policy.publish`, `policy.test`, `delegation.*` (server roles, phases 6 and 7); `runner.*` (the runner protocol, phase 7); `vault.rotate` (phase 9).

## Consequences

- The signed-in first hour runs end to end (`crates/anything2mcp/tests/first_hour_signed_in.rs`), in about 5 seconds; the token is in no output and no file.
- The test application's copy behind a bearer token now serves its description openly, at `/secure/bearer/api/openapi.json`, stating that each operation takes the token.
- The web packs' drafting places a surface by its origin only; the product test reaches the signed-in copy through a relay with an origin of its own. A base with a path is owed to the HTTP pack.
- An `auth.yaml` needs a probe that resolves, so the sign-in is declared after a first scout drafted a read. A drafted entry from a sign-in challenge is owed to the HTTP pack and `auth.detect`.
- Not built: the form sign-in with a second factor against the test application, which needs `form_login`; the code page is proven with a test provider.
- Not run: anything on Windows and macOS.
