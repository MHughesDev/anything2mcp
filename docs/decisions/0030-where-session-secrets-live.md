# 0030. Where session secrets live

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

The system holds live sessions to business applications for many people. A single place holding all of them would make one compromise total.

## Decision

A session on a user's device never leaves it; the control plane knows only its status. Hosted user sessions are encrypted in the vault per user and sent to a server runner wrapped to that runner's key, only for a job it holds, and kept in memory. Service-account credentials are fetched by the runner from a secrets manager by reference where one exists.

## Consequences

- A compromised control plane does not yield device-held sessions.
- A compromised server runner yields only the sessions it holds at that moment.
- Device-bound sessions cannot fail over to another runner.

## Alternatives considered

- Control plane holds every session in the clear for simple failover. Rejected.

## Revisit if

An identity mode needs session movement this rule forbids.

## Spec sections

- [43-runner-protocol.md](../spec/2-kernel/223-the-runner-protocol.md)
- [16-identity-on-a-shared-server.md](../spec/6-running-it/602-identity-on-a-shared-server.md)
- [19-security-model.md](../spec/5-security/501-security-model.md)
