# 0008. Control plane plus runners

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

The same system must run on one laptop and as a company service, and must be able to act with a sign-in that exists only on a user's own device.

## Decision

The system is a control plane and runners. Both can run in one process or be split across machines. Runners connect outward to the control plane and pull work. Three kinds exist: server, desktop, and user-device runners.

## Consequences

- One design covers every deployment profile.
- User-device runners make per-user access possible where sessions are bound to managed devices.
- A runner protocol must be designed, versioned, and secured.

## Alternatives considered

- Single process only. Rejected: no path to a company server or to device-bound sign-ins.

## Revisit if

The split adds more complexity than the Solo profile can hide.

## Spec sections

- [04-system-overview.md](../archive/spec-2026-11/04-system-overview.md)
- [23-deployment-configurations.md](../spec/6-running-it/601-deployment-configurations.md)
