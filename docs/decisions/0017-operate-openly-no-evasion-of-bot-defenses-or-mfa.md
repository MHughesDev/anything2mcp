# 0017. Operate openly: no evasion of bot defenses or MFA

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Automated access can be blocked by bot defenses, and sign-in often needs MFA. Evading either would put the user's account and employment at risk and may breach vendor terms.

## Decision

Web2MCP identifies itself as automation and respects rate limits. A block by a bot defense stops the run and is reported; the remedy is an allowlist from the application's owner or IT. People complete MFA and CAPTCHAs. Stored one-time codes are allowed only for dedicated accounts with IT approval. Web2MCP never imitates another application's client to get tokens.

## Consequences

- Some applications will be unreachable until their owner agrees.
- On server profiles a target cannot be published without a recorded authorization.

## Alternatives considered

- Stealth techniques. Rejected.

## Revisit if

Never.

## Spec sections

- [17-browser-engine-and-runtime.md](../spec/17-browser-engine-and-runtime.md)
- [19-security-model.md](../spec/19-security-model.md)
