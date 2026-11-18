# 0232. Whether a credential the machine holds for the person may be used through its holder

**Status:** Proposed. Owner decision: it asks how far rule five reaches, which only the owner changes. Written by the breadth review of 2026-11-18; the owner has not read it. Nothing here is in force, and rule five stands as written until the owner answers.

**Date:** 2026-11-18

**Asks about:** rule five of [AGENTS.md](../../AGENTS.md#rules-that-never-bend); [section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md), the sign-in mode `use_existing`; [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md). Beside [decision 0211](0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md), which asks about the person's browser, an application's own data directory, and permissions the person grants; this record asks about none of those.

## Context

Rule five says the engine never reads, copies, or drives the person's everyday browser profile or another program's cookie store, and gets past no device's protection. It was written about browsers. A person's machine holds credentials for them in other places, and many systems the machine can reach (decision 0208) accept no other:

- **The operating system's own sign-in.** On a company network a service accepts the person's logon through Negotiate, with no password typed: the system's security interface (on Windows) or the system's Kerberos library with the person's ticket (on macOS and Linux) answers the challenge. The built provider `windows_integrated` holds a password and hands it to the driver; nothing designs the case with no password. Section 215 already treats the system's sign-in broker as allowed (`use_existing`: "the operating system's sign-in broker ... Never a program's everyday sign-in store"), and `os_broker` is not built.
- **A key that signs only with the person present.** A smart card, through the system's own interface for it; a passkey, through the system's prompt in the engine's own browser profile. The key never leaves its holder; the person unlocks each use.
- **A signing agent another program runs.** An SSH agent, a GPG agent, the agent a password manager offers. The key never leaves the agent; the agent signs what it is asked to, often for every host the key opens, sometimes after asking the person.
- **Another program's stored credentials.** A cloud command-line tool's credential file or the program it runs to produce credentials, a version-control credential helper, a password manager's store.

The way through today, inside rule five, is a credential dedicated to the engine: a key or token stored with `secret.set`, a certificate in the vault, an OAuth registration the person or their IT made for it. It works where the system allows a second credential, and fails on a company network that signs in by the logon alone, and on a server reached only by SSH with the person's own key.

## The questions for the owner

1. **The system's integrated sign-in with the current logon.** Is answering a Negotiate challenge through the system's own security interface, as the person, with nothing read or stored by the engine, inside rule five as the system's own act, as section 215 already reads the sign-in broker?
2. **A key that signs only with the person present.** May the engine ask the system to sign with a smart card or a passkey, where the system itself asks the person each time (a PIN, a touch, a face), the engine never sees or stores the PIN, and the target declares the use?
3. **A signing agent another program runs.** May the engine ask an SSH or GPG agent to sign, never reading the key, for the one host a target declares?
4. **Another program's stored credentials.** May the engine read, or run the helper of, another program's credential store?

## Recommendation

- **1: yes.** It is the system's own sign-in, the same in kind as the broker section 215 already allows; the person's rights are exactly theirs and appear in the system's own logs. A provider per system, with the person's machine as its only home.
- **2: yes, with the person present at every signing.** The system's prompt is the person's act, and nothing is bypassed. Never with a PIN stored anywhere; a card with no person present is a stop.
- **3: no, for the first release.** An agent signs for everything its keys open, and it is another program's store in the plain sense of rule five. SSH to a person's machines with a key the engine's own vault holds, which the person adds to each machine, needs no answer here; its transport is not yet designed.
- **4: no.** It is what rule five forbids, read plainly.

What this record infers: that the owner's rule five is about whose store a credential is in and whether the engine reads it, not about the word "cookie". If the rule is meant only for browsers, questions 3 and 4 are design questions and come back to the agents.

## What changes if the owner answers yes to 1 and 2

- Section 215 gains the two cases under `use_existing`, each with what the person sees and what is a stop.
- `a2m-auth` gains a provider per operating system for 1, and one for 2 per kind of key holder, each with a decision record because the provider interface is a public contract.
- SCOPE.md moves each line it answers from Rule to Not yet.

## Alternatives weighed

- **Treat 1 as already answered by section 215.** Section 215 names the broker for tokens, not a challenge answered on the connection with the logon. The difference is small and the rule is the owner's, so it is asked.
- **Fold this into decision 0211.** That record asks about acting inside sessions the person already has in programs; this one asks about credentials held for the person by the system and by other programs. Kept apart so each can be answered alone.
