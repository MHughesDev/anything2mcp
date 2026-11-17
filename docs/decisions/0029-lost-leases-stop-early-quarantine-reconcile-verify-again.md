# 0029. Lost leases: stop early, quarantine, reconcile, verify again

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

A runner can lose its lease while a write is in flight. Lease numbers protect the ledger, but the target application cannot check them, so a stale runner's write cannot be refused there.

## Decision

Leases are granted as durations. The runner stops acting before its own count runs out and re-checks before every commit point; the control plane declares a loss only after a margin. The run's record locks are quarantined, a reconcile job reads the records back after the quarantine, and a later verification read alerts on any difference. Version preconditions are sent wherever the application supports them.

## Consequences

- A lost runner delays its run by at least the quarantine.
- A residual case remains and is documented: a runner frozen longer than the quarantine overwriting a record another run has since changed, in an application with no version check.
- Fault-injection tests must freeze a runner past its lease, not only kill it.

## Alternatives considered

- Reassign immediately: faster, with a wider window for two writers.
- Never reassign automatically: safest, but every dropped connection needs a person.

## Revisit if

The verification read ever finds a stale overwrite in practice.

## Spec sections

- [43-runner-protocol.md](../spec/2-kernel/223-the-runner-protocol.md)
- [42-run-ledger-design.md](../archive/spec-2026-11/42-run-ledger-design.md)
