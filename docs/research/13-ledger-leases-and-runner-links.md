# Ledger, leases, and runner links

**Status:** Partial. Built from search-result summaries on 2026-11-15; no primary page was opened in full. It answers part of open briefs 7 and 18, which stay open for a proper pass.

Feeds spec sections [208](../spec/2-kernel/208-runs-the-ledger-and-resume.md) and [223](../spec/2-kernel/223-the-runner-protocol.md).

## No engine gives exactly-once effects on someone else's system

- Durable-execution engines persist the orchestration, but the steps that touch the outside world can run more than once. Temporal's own documentation is cited for the point that activities may execute multiple times ([JobRunr's comparison](https://www.jobrunr.io/en/blog/durable-execution-without-a-workflow-engine/)).
- The dependable pattern is to give the effect an identity before it runs and reuse it on every retry, as payment APIs do with idempotency keys; where no key can exist, the fallback is compensation ([SoftwareSeni](https://www.softwareseni.com/durable-execution-and-who-owns-the-state-of-an-agent-run)).
- A vendor FAQ says the same of its own product and of its competitors: idempotency and reconciliation for outside calls stay with the application ([Diagrid](https://www.diagrid.io/faq/682-does-catalyst-guarantee-exactly-once-execution-for-external-api-calls-from-ai-agents)).
- A small database-backed step log gives at-least-once and nothing more; the receiving side has to supply the rest ([HackerNoon](https://hackernoon.com/you-dont-need-temporal-yet-durable-execution-for-ai-agents-in-150-lines)).

**Consequence.** The decision to build a small ledger instead of adopting an engine loses no guarantee an engine would have given. The wrapped applications rarely accept idempotency keys, so Anything2MCP supplies the receiving side itself: an intent recorded first, a keyed upsert, and a read-back to learn what happened.

## A job queue in Postgres

- The standard claim is a single statement that selects one pending row with `FOR UPDATE SKIP LOCKED` and updates it to running, so two workers never take the same job and neither waits ([DEV](https://dev.to/philip_mcclarence_2ef9475/for-transactional-job-queues-just-use-postgres-k10), [HackerNoon](https://hackernoon.com/skip-locked-vs-redis-when-should-you-use-a-database-queue)).
- A crashed worker is handled with a lease that expires and a heartbeat that extends it; something must periodically return expired jobs to the queue ([distributed job scheduling notes](https://alivedise.github.io/backend-engineering-essentials/distributed-systems/distributed-job-scheduling)).
- `LISTEN` and `NOTIFY` remove the polling delay.
- Job rows should hold references, not large payloads.

**Consequence.** The `jobs` table and claim query follow this pattern on Postgres. SQLite needs none of it, having one writer.

## Leases need fencing, and some resources cannot be fenced

- A lease can expire while its holder is paused; when the holder resumes it still believes it may act. Kleppmann's remedy is a number that increases with every grant, which the protected resource checks and refuses when stale ([Kleppmann](https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html), [HackerNoon](https://hackernoon.com/building-distributed-leases-with-consensus-heartbeats-and-fencing-tokens)).
- The check has to be made by the resource itself, atomically with the write. A resource that cannot check gives only an efficiency lock ([same source](https://hackernoon.com/building-distributed-leases-with-consensus-heartbeats-and-fencing-tokens)).
- Where fencing is impossible, correctness has to come from somewhere else: idempotency, a transaction, or a different design ([HackerNoon](https://hackernoon.com/distributed-locks-are-more-dangerous-than-they-look)).
- One write-up notes a further limit: a stale write that arrives before any newer one is not refused merely because its lease expired ([OneUptime](https://oneuptime.com/blog/post/2026-09-09-etcd-fenced-lock-leases-revisions/markdown)).

**Consequence.** Anything2MCP's ledger can check lease numbers, so it is fenced. The wrapped application cannot, so writes to it rest on idempotent upserts, version preconditions where the application has them, a quarantine, and a later verification read. The remaining window is written down in spec section 223.

## Workers that dial out

- Temporal workers wait for tasks by long poll; idle polls time out and are retried, which looks like warnings in logs but is normal ([Temporal community](https://community.temporal.io/t/worker-failed-to-poll-for-task/6891)).
- Heartbeats let the server notice a dead worker sooner than the overall job timeout would ([Temporal](https://docs.temporal.io/blog/activity-timeouts)).
- A worker that reports on a task the server already timed out is told the task no longer exists ([Temporal troubleshooting](https://docs.temporal.io/troubleshooting/request-failures)).
- Under heavy load heartbeats can be delayed enough to cause false timeouts ([Temporal community](https://community.temporal.io/t/worker-heartbeat-timeouts-and-activity-eviction-during-heavy-multitasking/19539)).

**Consequence.** Runners long-poll over HTTPS, heartbeat at a third of the lease, and have stale reports refused by lease number. The false-timeout risk is why the runner stops early by its own clock and the control plane waits a margin before declaring a loss.

## Not verified

- Measured latency of long-poll dispatch against a streaming connection.
- Whether corporate proxies in practice break HTTP/2 streams or WebSockets often enough to justify avoiding them.
- SQLite durability settings and their cost on each operating system.
- How mature runner-style systems issue and rotate worker certificates.
