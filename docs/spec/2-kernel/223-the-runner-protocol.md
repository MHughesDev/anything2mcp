# 223. The runner protocol

Runners do the work; the control plane decides, records, and hands work out. This section defines how the two talk: how a runner joins, how it gets a job, how the control plane knows it is alive, what happens when it is not, and how secrets reach it. The message table in the technical reference is the summary; this is the design. Not built today: `a2m-runner` is an empty crate, and the protocol waits for Phase 7.

## Shape

- **Runners dial out.** Every connection is opened by the runner, to one path on the control plane. A laptop or a locked-down network segment needs no inbound rule.
- **Plain HTTPS.** Requests and long polls over mutually authenticated TLS. This passes through corporate proxies that break long-lived streams, and any control-plane replica can answer any request, which keeps the front stateless.
- **One interface, two carriers.** Inside a single process the same interface is an in-memory channel. Split deployments use HTTPS. The same test suite runs against both, so the single-process profiles exercise the protocol every day.
- **The engine is the same on both sides.** It reaches everything outside its loop through one interface, `Control`. On a runner on another machine, `a2m-runner` implements `Control` over the link: the compiled target by its hash, the policy decision at guard, session material, a record lock, a rate budget, and the approval check are each a `need`; recording an intent is journaled and sent at once in an `events` call of its own, whose reply carries the ledger's token; other events and evidence go in the next batch. The engine on a runner opens no store, no policy engine, and no compiler.
- **One process runs the same loop.** In a single process, the runner is a loop inside `a2m-actions`: it claims a job from the scheduler, calls the engine with an in-process `Control` that answers by calling the scheduler, the keeper, policy, and the store directly, and completes the lease. The lease rules below apply unchanged: the lease is counted on a steady clock, heartbeats are sent by a task per held job, and the engine stops acting before the lease runs out. One conformance suite runs against both implementations of `Control` ([`a2m-runner`](../../architecture/crates/a2m-runner.md), [`a2m-actions`](../../architecture/crates/a2m-actions.md)).
- **At-least-once delivery.** Any request may be repeated. Every message carries the job's lease number and a sequence number, so repeats are recognized and stale ones refused.

Waiting for work by long poll is how established workflow systems connect their workers, and the same systems use heartbeats to notice a dead worker faster than a job timeout would ([research note 13](../../research/13-ledger-leases-and-runner-links.md)).

## Joining

| Runner kind | How it enrolls | What it may be given |
| --- | --- | --- |
| Server | An administrator issues a one-time enrollment token scoped to a workspace and a list of targets | Jobs for those targets, as a service account or a hosted user session |
| Desktop | The same, plus the operating system and applications it can drive | Desktop jobs for those applications |
| User device | The person signs in to the organization's identity provider from the runner | Only jobs that act as that person |

1. The runner creates a key pair and keeps the private key in the operating system's key store.
2. It presents the enrollment token or the person's sign-in with its public key.
3. The control plane issues a short-lived client certificate. The runner renews it over its existing connection well before it expires.
4. Revoking a runner refuses its connections, ends its leases, and invalidates the sessions it held.

## Calls

| Call | Purpose |
| --- | --- |
| `hello` | Report version, operating system, modes, slots, and targets; receive the heartbeat interval, lease length, and protocol version |
| `claim` | Long poll for work, up to 30 seconds; returns jobs that fit the runner's free slots |
| `events` | Report what happened, in order and in batches; the reply acknowledges intents and may carry a directive |
| `heartbeat` | Renew a lease; the reply gives the time remaining and may carry a directive |
| `need` | Ask for something a job requires: session material, a rate-budget grant, an approval check, an upload address |
| `result` | Final outcome of a job |
| `session-state` | Save a session's encrypted state to the vault |
| `bye` | Announce a planned stop |

A directive is `continue`, `cancel`, `drain`, or `pause`. `pause` is what a job hears while a kill switch covers it: stop at the next commit point and leave the run resumable ([decision 0093](../../decisions/0093-the-schedulers-interface-as-building-it-settled-it.md)).

## Matching work to runners

- A job states what it needs: target, environment, modes, operating system, identity, and session.
- A runner states what it offers and how many slots it has free per mode: browser contexts sized to memory, HTTP slots, and one slot per desktop application.
- **A session has a home.** A job that needs a live session goes to the runner holding it. If that runner is gone and the session is movable, its saved state is restored on another runner. A session bound to a user's device is never movable.
- If no runner fits, the job waits up to its queue limit, then the run returns `runner_unavailable` with what is missing, for example that the person's computer is off.

## Leases

A lease is the control plane's permission for one runner to work on one job for a limited time.

| Setting | Default | Meaning |
| --- | --- | --- |
| Lease length | 30 s | Granted as a duration, never as a clock time, so differing clocks do not matter |
| Heartbeat | Every 10 s | Each one renews the lease |
| Runner stops acting | 5 s before its own count of the lease runs out | Measured on the runner's steady clock from the moment it sent the request that was granted |
| Control plane declares the lease lost | 5 s after its own count runs out | The two margins leave a gap in which neither side believes the runner may act |
| Quarantine | 60 s, or the longest step timeout if greater | Wait before anyone else touches the run's records |

Every lease has a number that only goes up. The control plane refuses any event, result, or request carrying an older number. This is the standard defence against a holder that believes it still has a lease it has lost ([research note 13](../../research/13-ledger-leases-and-runner-links.md)).

## When a lease is lost

A lost lease means the control plane does not know whether the last write was sent.

1. The job is marked lost and the run stops with `possible_commit` and the code `runner_lost`.
2. The record locks the run held are quarantined, so no other run can write those records.
3. After the quarantine, a reconcile job with the next lease number reads back every record in doubt, as in [section 208](208-runs-the-ledger-and-resume.md). A reconcile that leaves a record in doubt is queued again only after a wait that doubles from five seconds to fifteen minutes, and a run stopped for a person, by any class but `possible_commit`, `transient`, or `busy`, is not queued again until someone resumes it ([decision 0193](../../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).
4. If policy allows, the run then resumes on its own; otherwise it waits for a person.
5. If the lost runner returns, it uploads what it journaled locally as late evidence. Late evidence is stored and shown, but changes no state.

A runner whose process ended and that starts again holds nothing: on one machine, where one process at a time can be the runner, every lease it held is declared lost at its start, by the same steps, and the quarantine is the longest step alone, because a process that ended cannot wake and act, and only a step it sent can still be in flight ([decision 0998](../../decisions/0998-a-runner-that-starts-again-holds-nothing-and-its-earlier-leases-are-lost-at-once.md)).

**The gap that cannot be closed.** The lease number protects the ledger, because the ledger can check it. The target system cannot: it has no idea Anything2MCP's leases exist. A runner that froze just after deciding to commit a write, such as clicking Save in the web pack, and thawed after its lease was lost, can still send that write. This is a known limit of any lease protecting a resource that cannot check it. What Anything2MCP does about it:

- The runner stops acting before its lease ends and re-checks the deadline immediately before every commit point.
- Steps are short, so the moment between the check and the write is small.
- The quarantine outlasts any step that was in flight.
- A late duplicate inside one run is harmless: the values are the ones already approved, written to the same key.
- Where the system offers a record version, the binding sends it, and a stale write is refused by the system itself.
- After a lost lease, the reconcile, once it has settled the run, re-reads in the same lease every record the ledger says was verified, and keeps any that differs as late evidence of the run. It is the reconcile's last part and not a job of its own, so the run keeps one live job ([decision 0347](../../decisions/0347-the-check-after-a-lost-runner-is-the-last-part-of-its-reconcile.md)).

What remains is a runner frozen for longer than the quarantine that then overwrites a record a different run has since changed, in a system with no version check. It is unlikely and it is detected afterwards, not prevented. The design records this instead of claiming otherwise.

## The local journal

Every runner keeps a small append-only journal on disk and syncs it before each commit point. The control plane's ledger remains the authority. The journal exists so that a runner cut off from the control plane can still account for what it did, and so that late evidence exists after a crash.

## Secrets

| Runner kind | Session material | Stored credentials |
| --- | --- | --- |
| User device | Never leaves the device. It stays in the device's key store; the control plane sees only whether the session is valid | Not applicable |
| Server, hosted user session | Encrypted in the vault for that user alone; sent to the runner wrapped to the runner's public key, only for a job it holds, and kept in memory | Not applicable |
| Server, service account | As above | The runner is given a reference and fetches the credential from the secrets manager with its own identity, where one is configured |

- A runner can ask only for material belonging to a job it currently leases.
- Evidence is redacted on the runner before upload. Large traces go straight to object storage through a short-lived upload address.
- A runner reaches only the surfaces its jobs declare, enforced on the runner from the job's permission manifest.

**What a compromised runner exposes.** A server runner exposes the sessions it is holding at that moment. A user-device runner exposes that one person. The control plane never holds a plain session secret for a user device, so compromising it does not hand over those sessions.

## Cancel, drain, and upgrade

- **Cancel** arrives in a reply. The runner finishes the commit point it is in, records it, and stops. A save is never abandoned halfway. Reads stop at once.
- **Drain** means take no new work, finish what is held, save session state, and leave. It is how runners are restarted and upgraded one at a time.
- **Versions.** `hello` carries a protocol version. The control plane supports the current and the previous version; an older runner is told to upgrade. User-device runners update themselves.

## A laptop that goes to sleep

A user-device runner on a laptop will lose its connection often. Closing the lid mid-run is a lost lease like any other: the run stops as `runner_lost`, and when the laptop wakes the runner reconnects, uploads its journal, the records are reconciled, and the run resumes if policy allows. Nothing special is needed beyond the rules above.

## Rate budgets

The control plane owns each surface's request budget. A runner asks for a grant, such as 20 requests over 5 seconds, and spends it locally, so respecting a system's limit does not cost a round trip per request.

## Speed targets

Time from a call being accepted to a runner starting it: under 20 ms on the same network. Intent acknowledgement: under 5 ms on the same network. Both are targets until measured in Phase 7.

## Open points

- Whether long polling is fast enough for interactive reads at scale, or a streaming carrier should be added behind the same interface.
- The certificate issuing design: a built-in authority, or the organization's own.
- Sensible quarantine defaults per mode, from measured step times.
