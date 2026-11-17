# 0142. The presence prompt's interface and the Linux prompt, as built

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Carries out:** [record 0133](0133-the-presence-prompt-on-each-operating-system-and-what-it-is-said-to-prove.md), points 2 and 4 and the Linux part of points 1 and 7. **Changes:** [record 0117](0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md), points 5 and 12, and the interface sketch of the design of [`a2m-local`](../architecture/crates/a2m-local.md); [record 0131](0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md), what it owed on the engine lock and the idle hold.

## Context

Record 0133 chose the prompt for each operating system and the shape of the interface. Building the interface, the Linux prompt, and the approval page that shows the stored change settled the points below. Only Linux could be run. No dialog was raised on a desktop.

## Decision

**The interface.**

1. **Availability is a method of the prompt, asked each time.** `PresencePrompt::available` returns an `Availability`: the kind, the name of the operating system's prompt, a reason as one word, a sentence, and a remedy. It raises nothing. The free function `presence_available` asks the system's prompt. *Instead of* a free function alone, as record 0133 worded it: the surface must ask the prompt it holds, and a test must be able to stand in for it.
2. **`ask` takes the deadline as a `tokio::time::Instant`** and the prompt cancels itself then. The surface gives a page's prompt the end of that page's approval window, and a protected setting's prompt one window from the ask.
3. **One prompt at a time is the surface's rule, not each prompt's.** The surface holds one lock while a prompt is up. A second press is answered 503 and records nothing; `confirm_presence` fails with `busy`/`prompt_busy`. A new prompt implements nothing for it.
4. **A press is carried out in a task of its own.** A prompt waits for a person, and a browser that goes away meanwhile used to leave the page marked as pressed for ever.
5. **The answers are five.** `Present` carries the name of the prompt that answered. `Denied` is new: answered and not passed, apart from `Dismissed`. `NotAvailable` carries the `Availability`, so the page says why. Only `Present` records an approval with proof of presence.
6. **The record.** The policy engine's `PresenceCheck` has one word for every operating system's prompt, `os_prompt`, and this package may not change it. So an approval is still recorded as `os_prompt`; the name is in the answer, on the answer page, and in one function of the surface that maps it. What is owed: three more words in `PresenceCheck`, `polkit`, `local_authentication`, and `windows_hello`, each proving presence.

**The Linux prompt.**

7. **`zbus` 6.3.0 and `zbus_polkit` 6.0.0, for Linux only**, as record 0133 named them. Verified again in the workspace: both exist at those versions, every crate they add is MIT or MIT/Apache-2.0, and the licence, ban, source, and advisory checks pass with them in the tree. *Rejected again:* the `pkcheck` program, kept as the fallback if a later check fails.
8. **The action is `io.github.mhughesdev.anything2mcp.presence`,** after the repository's address, because the project has no domain of its own. The policy file is `crates/a2m-local/polkit/io.github.mhughesdev.anything2mcp.presence.policy`. A test holds the file to the id in the code and to the defaults: no, no, `auth_self`. If the project gets a domain, the id changes in one constant and one file name.
9. **Before anybody is asked, polkit is asked without interaction, and only "would have to ask" raises the prompt.** A yes at that point was given without asking anybody: an administrator's rule, an authorization that was kept, or an engine run as the administrator account, which polkit always allows. It is refused as `granted_without_asking`. *Instead of* trusting any yes from the check that allows interaction: that yes cannot tell a person's password from a rule. This also holds the "never keep the answer" rule at run time, whatever the installed file says.
10. **A missing policy file is told from any other refusal by polkit's list of actions,** not by the text of its error.
11. **Every ask connects to the system bus anew.** The bus can come and go while the engine runs, and a connection that ends takes its dialog along. A call that asks nobody is given ten seconds.
12. **Nothing of an error's text goes into a message:** only the error's name, cut to its plain characters.

**Around it.**

13. **`Channel::serve` returns the engine lock, still held,** and the product drops it after the kernel has closed. *Instead of* a guard the caller holds from the start: the channel already owns the lock while it serves, and handing it back needs no new type.
14. **`IdleHolds::busy` is asynchronous,** because what holds an idle engine open is now a read of the store: the product's hold asks `Kernel::runs_in_hand`, so a run with nobody connected is not lost with an idle engine. A store that cannot answer holds the engine open.
15. **The approval page shows the stored change** through `ApprovalDesk::preview`, only when its hash is the one the page was opened for, and every value as text. A page fills its places in one pass, so a value that looks like a place of the page is not filled. A change of more than 2,000 records is shown in part, says so, and cannot be approved on the page. *Instead of* paging links: no test here can make a change that large, and an approval of records nobody was shown is the unsafe side.
16. **`serve` says what approvals need** in one line before it is ready: the kind, the reason, and the remedy.

## What was run, and what was not

- Run: the whole mapping, against a made-up authority behind the traits the real bus implements: every verdict and fault before the prompt with every verdict and fault after it. Exactly one combination is presence, and a prompt is raised only after "would have to ask".
- Run: the real bus code against an address where nothing listens (`no_system_bus`), and against a real polkit 124 on a system bus of the test's own, on an abstract socket: with polkit stopped (`no_polkit`, through the bus's failed start of the service) and with polkit running and no policy file (`policy_not_installed`).
- Not run: anything that needs the policy file installed. This machine refused the installation of a file and of a test account, rightly. The CI job in the pull request prepares a runner for it. A CI machine has no seat, so no session there is local and active; the job writes a polkit rule that gives the test account `auth_self` for the action, which stands in for the seat and for nothing else.
- Never run: a dialog on a desktop. Whether a desktop's agent closes its dialog when the check is cancelled is still open, and needs a person once, under Wayland and under X11.

## What follows

- `doctor.run` cannot report the presence check yet. `PersonSurface` needs a method the dispatcher can ask.
- The packages do not carry the policy file. Section 605 owes where it ships and how a person gets it from a pip or npm install.
- The macOS and Windows prompts are each one file beside `presence.rs`, its `mod` line, and its arm in `system_prompt`, with their own reasons added to `PresenceReason`.
- Two major versions of `zbus` are not in the tree: record 0123 removed the other.

## What would reopen it

An advisory on the `zbus` 6 tree; a desktop agent that leaves its dialog up after a cancelled check; a polkit that answers "would have to ask" and then passes a check with nobody asked; or a change too large for the page that people need to approve on one machine.
