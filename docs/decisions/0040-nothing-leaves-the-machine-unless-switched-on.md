# 0040. Nothing leaves the machine unless switched on

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Web2MCP holds signed-in sessions to business applications and sees business data. Many tools send usage statistics, crash reports, or update checks by default.

## Decision

By default the engine sends nothing to the project or anyone else: no usage statistics, no crash reports, no update checks. It contacts only configured targets, the identity provider, a configured control plane, and a configured monitoring endpoint. Each optional outbound feature is separately opt-in and states what it sends. A CI test fails if an unconfigured engine opens an outside connection.

## Consequences

- The project learns about problems only from what people choose to report.
- Security updates are not announced to an install automatically; release notes and package managers carry that.

## Alternatives considered

- Opt-out telemetry: more insight, and unacceptable to many organizations for a tool like this.

## Revisit if

Never for the default.

## Spec sections

- [41-packaging-and-release.md](../spec/41-packaging-and-release.md)
- [19-security-model.md](../spec/19-security-model.md)
