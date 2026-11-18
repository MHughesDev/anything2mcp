# 0133. The presence prompt on each operating system, and what it is said to prove

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** [record 0039](0039-local-approval-a-diff-page-for-the-moment-then-an-operating-system-prompt.md), which left the prompt per system and the fallback open; [record 0117](0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md), point 12, whose `presence_available` and `system_prompt` change shape.

## Context

`a2m-local` asks for a person's presence through `PresencePrompt`, and ships only `NoPrompt`, so nothing can be approved on one machine. Research brief 27 asked which prompt each system offers a background process, what a program running as the user can do against it, and how to call it from Rust. [Research note 20](../research/20-operating-system-user-presence-checks.md) answers it from documentation and source, with a probe that type-checks each call path for Linux, macOS, and Windows and passes the licence and advisory checks. No prompt was pressed.

## Decision

1. **The prompt on each system.**
   - Windows 10 and 11: Windows Hello, through `IUserConsentVerifierInterop::RequestVerificationForWindowAsync`, with `CheckAvailabilityAsync` read first, owned by a small window of the engine's own. Crates: `windows` 0.62.2 with `Security_Credentials_UI`, `Win32_System_WinRT`, and `Win32_Foundation`; `windows-future` 0.3.2.
   - macOS 13 and later: `LAContext` with `deviceOwnerAuthentication` (Touch ID, a paired watch, or the account password). Crates: `objc2-local-authentication` 0.3.2, `objc2` 0.6.5, `objc2-foundation` 0.3.2, `block2` 0.6.2.
   - Linux desktops: polkit `CheckAuthorization` on the system bus, for the engine's own process, for one action of the project's own whose defaults are `allow_any` no, `allow_inactive` no, `allow_active` `auth_self`, never `auth_self_keep`. The policy file ships with the product; an administrator installs it. Crates: `zbus` 6.3.0 with default features off and `tokio` and `proxy` on; `zbus_polkit` 6.0.0.
   - SSH, containers, headless machines, and an engine outside the local active session: `NotAvailable`.
2. **The interface.** `PresencePrompt` stays. `presence_available` becomes a check made at run time that returns the kind and a reason, for `doctor.run`. `system_prompt` returns the system's prompt. `ask` takes a deadline, the end of the approval window, at which it cancels the prompt and answers `Dismissed`. One prompt at a time: a press while one is up is `busy`.
3. **The words.** Where the prompt carries text (Windows, macOS), it names the tool, the target, and the first twelve characters of the diff's hash, and the page shows the same twelve characters beside Approve. polkit lets an unprivileged caller pass no details, so its dialog shows the policy file's fixed message, which tells the person to check the page.
4. **The record** names which prompt answered: `windows_hello`, `local_authentication`, or `polkit`.
5. **No fallback dialog in the first build.** A dialog can be pressed by a program running as the user; it comes after the real prompts, records the check `not_available`, and never loosens a protected setting.
6. **What it is said to prove.** Sections 222 and 503 say that the prompt stops an agent acting through the engine and by synthetic input, not a program that replaces the engine or changes its files; that on Linux an agent that knows or has captured the password can pass it; and that a dialog alone can be pressed.
7. **Order.** Linux first, proved in CI with polkitd, the policy file, a test account with a made-up password, and a text agent registered for the engine's process. Then macOS and Windows together, tested in CI only through their availability checks and closed by one check by a person on each.

## Rejected

- **`CredUIPromptForWindowsCredentials` on the secure desktop.** It hands the typed password to the engine, which would then hold the person's Windows password and check it itself.
- **The `pkcheck` program on Linux.** It needs no crate and proves the same, but answers by exit code, ends a prompt only by being killed, and is packaged apart from polkitd on some distributions. It is the fallback if the bus crates fail a later check.
- **A polkit action that names owner users, so that our text could appear.** The installed file cannot know a machine's users, and an owner may check other users' processes.
- **Generated bindings over `windows-core` 0.100 now.** Lighter, and what the Windows crates' project now advises for libraries; deferred until `windows-sys` in the workspace moves to the same line, to avoid two lines at once.
- **A signed approval** with a key gated by presence (Windows Hello key credentials, a keychain key). Stronger evidence for an audit; later, with the server console that could hold the public key.

## What follows

- `a2m-local`'s design: the interface sketch, build-order step 5, and the open point on brief 27.
- Hypothesis H-12: "cannot be driven by an agent with a shell" is narrowed to "cannot be driven through the engine or by synthetic input"; its timing half still needs a person.
- Records 0120 and 0123: `zbus` 6 now passes the advisory check. Their crates still stand on `zbus` 5; two major versions of `zbus` in one tree should be avoided, so the desktop pack's and the vault's Linux back ends are worth rechecking against `zbus` 6.
- `doctor.run` reports the kind, the reason, and the remedy listed in note 20.

## What would reopen it

A prompt pressed by synthetic input from an ordinary program on Windows or macOS; a later advisory on the `zbus` 6 tree; or Windows Hello failing to come to the front for the engine in the hand check.
