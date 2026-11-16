# 0031. The browser always runs with a profile the engine owns

**Status:** Accepted. Decided in the system design; the owner delegated these choices.

**Date:** 2026-11-15

## Context

Earlier sections said the engine could attach to a browser already running on a user's device and reuse an existing profile. Since Chrome 136 the DevTools protocol is refused on the default profile, Chrome on Windows binds its cookie store to the application, and driving a user's live browser this way is a known attack technique.

## Decision

The engine launches the browser only with a data directory it owns, one per identity. It never reads, copies, or decrypts another program's cookie store and never attaches to the user's everyday browser. Using the existing sign-in means the device's sign-in: the operating system's broker, or silent sign-in of the engine's own profile.

## Consequences

- A first sign-in in the engine's browser may be needed even on a device that is already signed in.
- An application that allows one session per account can sign the person out of their own browser.
- Web2MCP does nothing that security tools would flag as cookie theft.

## Alternatives considered

- Attach to the user's browser: no longer possible on the default profile, and unsafe.
- A browser extension inside the user's profile, with consent: possible later as its own mode.

## Revisit if

A consented, vendor-supported way to work inside the user's profile becomes available.

## Spec sections

- [44-session-keeper.md](../spec/44-session-keeper.md)
- [17-browser-engine-and-runtime.md](../spec/17-browser-engine-and-runtime.md)
