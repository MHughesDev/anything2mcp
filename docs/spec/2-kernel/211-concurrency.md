# 211. Concurrency: many callers at once

One running system serves many agents and applications at the same time. A scheduler, not the callers, decides what runs when, so callers never collide on a session or a record.

**What is shared and what is not.**

- **Sessions are shared.** One signed-in session per identity and target serves every caller allowed to use it, so ten agents do not mean ten sign-ins or ten MFA prompts.
- **Working surfaces are not shared.** Each run gets its own working surface on top of the shared session: a browser tab in the web pack, an HTTP client, a desktop handle, or a connection. The sign-in itself is shared live, not copied ([section 215](215-sessions-and-the-session-keeper.md)).
- **Records are locked, not sessions.** Writes take a lock on the record key, so two agents can write different records at once but never the same record. A lock is held for one record at a time and is quarantined, not released, if its run's lease is lost ([section 208](208-runs-the-ledger-and-resume.md)).

**The scheduler.**

| Setting | Default | Purpose |
| --- | --- | --- |
| Reads in parallel per session | 4 | Each mode's default is in its provider's manifest. HTTP reads can go higher; in the web pack, UI reads are limited by memory |
| Writes in parallel per session | 1 for UI, 4 for HTTP | Each mode's default is in its provider's manifest. Some systems misbehave with parallel saves from one user |
| Global browser contexts, in the web pack | Sized to memory | Protects the machine |
| Queue length per target | 100 | A full queue returns `busy` with a retry time instead of hanging |
| Priority | Interactive before batch | A person's request jumps a nightly pipeline |
| Per-caller quota | Configurable | Stops one agent from starving others |

**Duplicate calls.** An unfinished run is identified by its caller, tool version, and input hash ([section 208](208-runs-the-ledger-and-resume.md)). If the same caller sends the same call twice, the second gets the first run's id instead of a second run. Two different callers get two runs, and the record locks keep them off the same record.

**Desktop limits.** A desktop application usually has one window and one input focus, so desktop runs on one application are always serialized. The same holds for anything a mode's manifest marks as held alone by one write, such as a device link.

**Rate limits.** Each surface has a request budget, set from the system's documented limit or a conservative default. Calls over budget wait in the queue rather than hitting the system.
