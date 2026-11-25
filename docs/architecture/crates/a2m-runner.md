# `a2m-runner`

The runner role, for a runner on another machine than the control plane. A runner opens the connection outward, says what it can serve, claims a job under a lease, and calls the same engine as a single process does, with a `Control` whose every answer comes over the link. It keeps a local journal so that a runner cut off mid-write can still account for what it did. This crate also holds the control plane's end of the link. Nothing of it is built yet: the crate is an empty library, and every step of its build order is open.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 8, Roles |
| May depend on | Layers 1 to 7 |
| Depends on | `a2m-core`, `a2m-vault` (the runner's key and unwrapping session material), `a2m-logs`, `a2m-auth` (the keeper's work half), `a2m-provider` (the registry the product crate fills), `a2m-engine`, `a2m-sched` (the job and lease types), `a2m-actions` (`Paths`, settings, and, on the control plane's end, the `Kernel`). Outside: Tokio, Hyper with rustls, to be verified |
| Roadmap packages | Roadmap packages 7.1 (the link), 7.2 (enrollment, lost leases, reconcile after a lost runner), 7.3 (user-device runners) |
| Carries out | Spec section [223](../../spec/2-kernel/223-the-runner-protocol.md); the runner half of [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md); the [runner protocol](../../reference/technical-reference.md#runner-protocol) of the technical reference |

## Owns

- The runner process: enrollment, the outward connection, `hello`, `claim`, `events`, `heartbeat`, `need`, `result`, `session-state`, `bye`, and the directives `continue`, `cancel`, `drain`.
- `LinkControl`: the engine's `Control`, implemented by sending a `need` or an event over the link.
- The runner's lease clock, and the rule that it stops acting before its lease runs out.
- The local journal.
- Holding live sessions on this machine, as the home runner, through the keeper's work half and the packs' drivers.
- `LinkService`: the control plane's end, which answers each call by asking `a2m-sched` and the `Kernel`.

It must never: open the store; hold a plain session secret for longer than a job needs it; ask for material belonging to a job it does not lease; act on a job once its own count of the lease has run out; reach a surface the job's permission manifest does not declare.

## Interface

```rust
/// The runner process. Holds a provider registry and an engine, never a Kernel.
pub struct Runner { /* identity, link, registry, engine, keeper work half, journal, slots */ }
impl Runner {
    pub async fn enroll(cfg: RunnerSettings, proof: EnrollmentProof) -> Result<Enrolled, Failure>;
    pub async fn start(cfg: RunnerSettings, providers: ProviderRegistry) -> Result<Runner, Failure>;
    pub async fn run(self) -> StopReason;        // until drained, revoked, or stopped
}

/// The engine's Control over the link. One per held job.
pub struct LinkControl { /* link, lease number, sequence, steady-clock deadline, journal */ }
impl Control for LinkControl { /* each method below */ }

/// The control plane's end. A service the product crate mounts at the runner path.
pub struct LinkService { /* Kernel */ }
impl LinkService {
    pub fn new(kernel: Kernel, cfg: LinkSettings) -> Self;
    pub fn http(self) -> BoxedHttpService;
}
```

## Calls

Every call is a request from the runner over mutually authenticated HTTPS, to one path. Every message carries the job's lease number and a sequence number; repeats are recognized and older lease numbers refused. The calls and their purposes are those of spec section 223. What each does in this crate:

| Call | Runner side | Control plane side, in `LinkService` |
| --- | --- | --- |
| `hello` | Version, operating system, modes and free slots per mode, targets, identities, the sessions it holds | Checks the protocol version (current or previous); answers heartbeat interval, lease length, protocol version |
| `claim` | A long poll of up to 30 seconds while slots are free | `a2m-sched` claim with the runner's offer; the reply is a lease: job, lease number, length as a duration |
| `events` | Batches, in order, from the journal | Stored through the `Kernel`; the reply acknowledges intents with the ledger's tokens and may carry a directive |
| `heartbeat` | Every 10 seconds per held lease | Renews in `a2m-sched`; the reply gives the time remaining and may carry a directive |
| `need` | See `LinkControl` below | The same functions the in-process `Control` in `a2m-actions` calls |
| `result` | The job's outcome, after its last event is acknowledged | `a2m-sched` completes the job |
| `session-state` | A session's sealed state, to be saved | Stored in the vault for that identity alone |
| `bye` | Before a planned stop, after the journal is empty | Ends the runner's leases cleanly |

## `LinkControl`

The engine cannot tell whether it runs here or in one process. That is the point of the `Control` trait, and the same conformance tests run against both implementations.

The list is the spine's `Control`, in its order. The engine on a runner opens no store, no policy engine, and no compiler; this table is everything it receives.

| The engine asks | Sent as | Notes |
| --- | --- | --- |
| The compiled target by its hash | `need` | Checked with the check in `a2m-core` before it is used; cached by hash |
| The policy decision at guard | `need` | Decided and recorded by `a2m-policy` on the control plane |
| A session lease | `need` for session material, unless this runner already holds the session live | Material arrives wrapped to the runner's public key, is unwrapped with `a2m-vault`, and is kept in memory only |
| A record lock | `need` | Taken in `a2m-ledger` under the job's lease number; a wait for a held lock is queued by `a2m-sched` |
| A rate budget | `need` for a grant, such as a number of requests over a period | Spent locally; a new grant is asked for before it runs out |
| The approval check for a diff hash | `need` | Answered by `a2m-policy` on the control plane |
| Recording an intent, and its token | Journaled, then sent at once in an `events` call of its own | The reply carries the ledger's token; the driver's handle refuses a committing step without it, so no intent is acted on before the reply |
| Sending events and evidence | Journaled, then sent in the next `events` batch; large evidence through an upload address asked for by `need` | Redacted here before it is journaled or sent; large traces go straight to object storage |

Before every commit point `LinkControl` checks the deadline on the runner's steady clock and refuses to let the engine act within 5 seconds of it, whatever the link says.

## The lease clock

| Moment | Rule |
| --- | --- |
| Lease granted | The runner's count starts when it sent the request that was granted, not when the reply arrived |
| Each heartbeat | Renews; the new count starts from that request's sending |
| 5 seconds before the count ends | The runner stops acting. A step in flight finishes or stops at its own timeout; no new commit point starts |
| The count ends | The runner treats the lease as lost, stops the job, and journals what it knows |

The control plane declares a lease lost 5 seconds after its own count ends, so there is a gap in which neither side believes the runner may act.

## The local journal

| Item | Rule |
| --- | --- |
| Where | `journal/` in the data directory from `Paths` |
| What | Every event before it is sent; every intent and whether its acknowledgement arrived; every act |
| When it is synced | Before each commit point, so a write that was sent is always on disk first |
| How it is emptied | An entry is dropped once the control plane acknowledges it |
| After a lost lease | On reconnecting, the runner sends `hello` and uploads what remains as late evidence. Late evidence is stored and shown, and changes no state |
| Secrets | None. Entries are redacted before they are written |

## When things go wrong

| Event | What the runner does |
| --- | --- |
| The link drops | Keeps journaling; stops acting when its own count runs out; reconnects with growing delays |
| A `cancel` directive | Finishes the commit point it is in, records it, and stops. Reads stop at once |
| A `drain` directive | Claims nothing new, finishes what it holds, saves session state, sends `bye` |
| Revoked | Its connections are refused; it ends every live session it holds and deletes their profiles |
| The process dies | The control plane loses the lease; the run stops as `possible_commit` / `runner_lost` and is reconciled after the quarantine. On restart the runner uploads its journal |
| A laptop sleeps | Exactly as a lost lease; nothing special |

## Enrollment and identity

1. The runner creates a key pair and keeps the private key in the operating system's key store, through `a2m-vault`'s key-store back end.
2. It presents an enrollment token issued by an administrator, or on a user device the person's sign-in to the organization's identity provider, with its public key.
3. It receives a short-lived client certificate and renews it over its existing connection well before it expires.

A server runner may be given jobs for its enrolled targets as a service account or a hosted user session. A user-device runner is given only jobs that act as that person, and its session material never leaves the device: the control plane sees only whether the session is valid.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `anything2mcp` | `Runner` for the `runner` process command; `LinkService` mounted on the server listener in a control-plane role |

| This crate calls | For |
| --- | --- |
| `a2m-engine` | `run`, `reconcile`, `discover`, with a `LinkControl` |
| `a2m-auth` | The keeper's work half, for probe, renewal, and sign-in jobs on this home runner |
| `a2m-provider` | The registry, for drivers and auth providers |
| `a2m-vault` | The runner's key; unwrapping session material; sealing session state |
| `a2m-sched` | Job and lease types; on the control plane's end, claim, renew, complete |
| `a2m-actions` | `Paths`, settings; on the control plane's end, the `Kernel` |

## State

| State | Where |
| --- | --- |
| The private key | The operating system's key store |
| The client certificate | Memory, renewed |
| The journal | `journal/` in the data directory |
| Live sessions: browser profiles, tokens in memory, open connections | `profiles/` and memory, held by the packs' drivers |
| Held leases and their counts | Memory; lost with the process, which is a lost lease |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `possible_commit` | `runner_lost` | Raised on the control plane for a lease that ended without a result |
| `busy` | `runner_unavailable` | Raised on the control plane when no runner fits a job within its queue limit |
| `transient` | `connection_lost` | The link dropped during a call; repeated with the same sequence number |
| `policy` | `surface_not_declared` | A step tried to reach a surface outside the job's permission manifest |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| The same conformance suite passes against `LinkControl` and the in-process `Control` | One suite, two carriers, run on every build |
| A message with an older lease number is refused | A test that replays a message after a new lease |
| No write is sent within 5 seconds of the runner's deadline | A test with a slowed clock and a write pending |
| Every write sent is in the journal first | A test that kills the runner just after a write and reads the journal |
| Late evidence changes no state | A test that uploads a journal after the run was reconciled |
| A runner cannot fetch material for a job it does not lease | A test that asks with another job's lease |
| A runner killed mid-write leaves no duplicate and nothing unverified | The exit test of package 7.2, asked of the test application |

## Build order

1. Not built: `LinkService` and `LinkControl` over an in-memory carrier, passing the same suite as the in-process `Control`. This makes hypothesis H-21 measurable from the other side.
2. Not built: `hello`, `claim`, `heartbeat`, `result` over HTTPS with a fixed test certificate; one read job. Package 7.1.
3. Not built: `events` with intent acknowledgements, and the journal.
4. Not built: lost leases, quarantine, late evidence, `cancel` and `drain`. Package 7.2.
5. Not built: enrollment and certificate renewal; session material wrapped to the runner's key.
6. Not built: user-device runners. Package 7.3.

## Open points

- Whether long polling is fast enough for interactive reads; hypothesis H-07.
- The certificate authority: built in, or the organization's own.
- Quarantine defaults per mode, from measured step times.
- Whether the compiled target is sent with the lease or fetched by `need`. Fetching by hash with a cache is assumed here.
