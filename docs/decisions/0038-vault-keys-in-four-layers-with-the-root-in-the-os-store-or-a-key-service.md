# 0038. Vault keys in four layers, with the root in the OS store or a key service

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Saved sessions are as powerful as the people they belong to. ADR 0035 says only the owner and their agents may use a session; that needs more than an access check.

## Decision

Keys are layered: a root key per installation, a key per workspace, a key per owner, and a key per item. The root lives in the operating system's credential store on desktops and in an external key service on servers. An owner's key is unwrapped only for a job acting as that owner. No key is ever written to disk in the clear; the passphrase route on headless Linux is an explicit, weaker opt-in. No action reads a secret back. Web2MCP writes no cryptography of its own.

## Consequences

- Rotating an upper layer re-wraps keys without re-encrypting data.
- Someone controlling both the server and the key service can still decrypt; this is stated.
- Losing the local credential-store entry means signing in again.

## Alternatives considered

- One key for everything: simple, and any exposure is total.

## Revisit if

A deployment needs hardware-bound keys for runners.

## Spec sections

- [48-vault-and-keys.md](../spec/48-vault-and-keys.md)
- [44-session-keeper.md](../spec/44-session-keeper.md)
