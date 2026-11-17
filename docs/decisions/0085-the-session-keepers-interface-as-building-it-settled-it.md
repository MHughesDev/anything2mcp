# 0085. The session keeper's interface, as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** two open points of the [`a2m-auth` design](../architecture/crates/a2m-auth.md) and of spec section 215: whether `SignInSurface` is a type, and the shape of the auth file's provider settings. Changes the interface sketched there and in [decision 0063](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md). **Adds sentences to [spec section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md), the [technical reference](../reference/technical-reference.md), the [file formats](../reference/file-formats.md), and the [ledger schema](../reference/ledger-schema.md).**

## Decision

**The state machine**

- **The moves are a table of their own.** Section 215 lists eight states and says no transition outside its table is allowed, but the table lists states, not moves. The 23 legal moves are now written on `legal_move` and in the crate design, and every write of a state passes that one function.
- **A change of state is one compare-and-set on the state and the epoch,** in one transaction with its timeline event and its saved state. The epoch counts the credentials: it increases when they are replaced or ended, and not when a session is merely being checked.
- **The winner of a move starts the work as a task of its own.** *Instead of:* running it inside the winning call, which would leave the session in `renewing` for good if that caller went away.
- **A call starts work once.** A call that started or waited through a sign-in or a renewal never starts another, and is given the failure that work ended with. That is what keeps ten waiting runs from making ten attempts in a row.
- **A session with a stored credential is signed in by the first lease that asks.** Section 215 says `absent` returns `needs_login`, and also that a service account's sign-in involves nobody. Both hold: a provider that needs a person is never started by a lease.
- **`attempt_limit` counts unconfirmed attempts.** Each automatic sign-in or renewal with a stored credential is counted until the other side confirms the credential: a successful exchange, for a provider that exchanges it, or a passed probe. A provider says which it is with `ProviderNeeds::confirms`. A provider that only holds a value cannot be refused when it signs in, so without this a wrong key would be offered again on every report. Failing to reach the identity service is not counted.
- **`NoWait` waits for the keeper's own work and never for a person.** *Instead of:* never waiting, which would stop every pipeline at each hourly renewal.
- **Waits are counted on the keeper's clock and looked at on `tick`.** Nothing sleeps, so tests move time.

**The lease**

- **A lease is a `SessionGrant`, plain data that can cross the runner link, and a handle to the home runner's live credentials.** `apply` reads them at the moment of use and refuses with `lease_stale` when their epoch is not the lease's. *Instead of:* a snapshot of the credentials inside the lease, which would go on placing a token after a revoke.
- **`apply` puts the redactor in front of the driver's sink,** so a value a provider builds while injecting, such as a Basic header, is registered like any other.
- **`LeaseRequest::acting_for` names the person the run acts for.** The session's own identity is part of its key, so comparing the key with itself would prove nothing; the keeper compares it with the person the front door authenticated.
- **The lease, not the keeper, answers `detect_expiry`.** A driver asks its lease whether what it saw is a sign; only the report goes to the keeper.

**The contract**

- **`AuthProvider` gains `check`, and `ProviderNeeds` says `person`, `silent_renewal`, `light_prompt`, `movable`, and `confirms`.** `check` lets an entry be refused before anything is opened, and gives the compiler's lint something to call.
- **The traits return boxed futures,** as the vault's do, and share its `BoxFuture`.
- **One context, `SignInContext`, serves sign-in, renewal, and sign-out.** It is opaque, so that more can be offered later without breaking a pack.
- **There is no `SignInSurface`.** What the worker needs from its runner is the way to the session's own person, `AskPerson`: a link to open, a code to enter, a notice. A pack's provider that opens a window of its own already holds its pack's handles. `KeeperWorker::run` takes `&dyn AskPerson`.
- **`KeeperWork::perform` takes a `KeeperJob`:** the session, its auth entry, and the work. `Work` gains `Forget` and `Restore`, and loses `Export`, which belongs to moving a session and is not built.
- **`totp` is not a provider of sessions.** It is `SignInContext::one_time_code`, which another provider calls.
- **Provider settings are a map of names to plain values, and credentials a map of roles to references,** in each `auth.yaml` entry. The names per provider are in the crate design. A provider refuses a name it does not know.
- **Stored credentials are the keeper's rows,** `session_credentials`, sealed under the owner's key; `Credentials` is the trait a runner with an outside secrets manager implements instead.

## Consequences

- `a2m-files` must carry `settings` and `credentials` in an auth entry; `a2m-actions` builds `AuthEntry` from the plan and `LeaseRequest` from the `ActAs` decision.
- The keeper remembers each session's entry only from its last use, in memory. After a restart a session is not probed until it is used again, which is the rule "only while wanted".
- A failure to reach the identity service during a silent renewal costs a person's session a new sign-in.

## What would reopen it

The first provider that needs a surface, if `AskPerson` and the handles a pack holds turn out not to be enough; or the runner link, if a grant needs more than it carries.
