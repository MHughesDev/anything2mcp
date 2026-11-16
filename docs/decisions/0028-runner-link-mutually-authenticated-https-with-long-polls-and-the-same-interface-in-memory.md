# 0028. Runner link: mutually authenticated HTTPS with long polls, and the same interface in memory

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Runners must dial out, including from users' laptops behind corporate proxies, and the control plane must stay stateless so any replica can answer.

## Decision

Runners talk to the control plane with ordinary HTTPS requests and long polls over mutual TLS. The link is an interface with two carriers: HTTPS between machines and an in-memory channel inside one process. Delivery is at-least-once; every message carries a lease number and a sequence number.

## Consequences

- Single-process profiles exercise the same protocol code and tests as split deployments.
- Dispatch latency depends on waking a long poll, which needs database notifications across replicas.
- A streaming carrier can be added later behind the same interface.

## Alternatives considered

- A bidirectional streaming connection: lower latency, but pinned to one replica and fragile through some proxies.
- A message broker: one more service to run.

## Revisit if

Measured dispatch latency misses the budget for interactive reads.

## Spec sections

- [43-runner-protocol.md](../spec/43-runner-protocol.md)
- [24-networking.md](../spec/24-networking.md)
