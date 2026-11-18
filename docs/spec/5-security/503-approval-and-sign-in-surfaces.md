# 503. Approval and sign-in surfaces

Two moments need a person in front of a screen: approving a write, and signing in. Sections 212 and 215 say that these happen and who may do them. This section says what the person actually sees and what the engine has to build to show it, starting with the single-machine case. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

## What a person must be able to do

| Moment | The person needs | The engine needs |
| --- | --- | --- |
| Approve a write | To read the diff properly: every changed record, current beside proposed | Proof that a person, not the calling agent, said yes to this exact diff |
| Sign in | Where the system's sign-in is a web page, a real browser window, so the system's own sign-in and MFA work unchanged | To know when sign-in finished, without seeing the credentials |
| Answer a decision point | The question and its options | The answer, recorded with who gave it |
| Carry out a person's act an agent asked for, such as a publish | To read exactly what will be done: the action and every value of its input, and for a publish what each tool may do | Proof that a person, not the agent, said yes to this act, given for it alone ([decision 0163](../../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md)) |

## On one machine

A diff of forty records does not fit in a system dialog, and a web page alone could be approved by an agent with a shell. So the two are combined.

1. **Notify.** The engine raises a system notification and tells the calling client that the run is waiting, with a link.
2. **Show.** The link opens the diff page in the person's default browser. The engine serves that page from a listener on the loopback address that exists only while something is waiting, on a random port, at an address containing a one-time code. It checks the `Host` and `Origin` of every request and serves nothing else; the page's referrer policy is `same-origin`, so that a browser sends the page's own origin with a press (under `no-referrer` it sends `Origin: null`, which is refused).
3. **Confirm.** Pressing Approve on the page does not approve anything by itself. It asks the engine to raise the operating system's user-presence prompt: a fingerprint, face, PIN, or password prompt owned by the operating system. The approval is recorded only when that succeeds. On Windows it is Windows Hello, on macOS the system's own authentication with Touch ID, a paired watch, or the account password, and on a Linux desktop polkit, for an action an administrator has installed ([research note 20](../../research/20-operating-system-user-presence-checks.md)). Where the prompt can carry words, it names the tool, the target, and the first twelve characters of the diff's hash, which the page shows beside Approve; polkit's dialog cannot, and shows a fixed message. So on Linux the person cannot tell from the dialog which change it is for, and the surface does what it can around it: a prompt is raised only from a press on a page, and never for anything else while a page waits; one is up at a time; and the page tells the person to answer a prompt only right after their own press. The limit stays: a program of the same user that holds another waiting page's link can press that page just before the person presses theirs; the person's press is then told that another prompt is up, and a person who answers the dialog without reading that approves the other page's change ([decision 0158](../../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md)). One prompt is up at a time: a press while another prompt is up records nothing and says so. A prompt nobody answers is cancelled at the end of the approval window. Where no prompt can be raised, the page says so before anything is pressed, with the reason and what to do about it ([decision 0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md)).
4. **Bind.** The approval is stored against the diff's hash ([steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md)) and can be used once.

A person's act an agent asked for is shown the same way, on a page of its own, with a press of Carry out in place of Approve. A publish is pinned to the hash of the plan the page shows; drafts that change before the press publish nothing.

Where the operating system offers no such prompt, step 3 is a native confirmation dialog raised by the engine, showing the tool name and the counts; it is not in the first build. Over SSH, in a container, and on a headless machine there is neither, and nothing on that machine can approve. On Linux the same holds until a person with administrator rights has installed the policy file that ships in the product's binary: the engine writes a copy into its data directory and names the one command that installs it, which installs the copy only when its bytes are the file the binary carries; the engine never runs it ([decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md)). Section 222 states what each does and does not protect against.

**This is the one exception to section 222.** The engine opens no network port by default, and that stays true: the loopback listener is not a front door, exists only during a pending approval or sign-in, carries no API, and closes when nothing is waiting.

**No tray application in the first release.** A notification, the default browser, and the operating system's own prompt cover a single machine. A tray application arrives with user-device runners in Phase 7, where something must stay running and visible on a person's computer.

## Signing in on one machine

Signing in through a page is the web pack's: its auth providers drive a browser the engine owns. A sign-in provider outside the web pack that needs a person's consent in a browser gives the person a link to open in their own, and nothing drives that browser ([section 202](../2-kernel/202-kernel-and-packs.md)).

- The engine opens a visible window of its own browser, in the profile for that identity, at the system's sign-in page.
- The person signs in as they always do. Password managers, MFA prompts, and passkeys work as in any browser window.
- The session keeper watches for the target's signed-in probe to succeed, then closes the window or leaves it for the first run.
- The window shows which system and which Anything2MCP identity it is for. The engine never asks for the password of such a sign-in in its own interface.

The window is built for the web pack's `window_login`, `sso_oidc`, and `sso_saml`: the engine waits for the target's declared sign of success rather than the probe, and closes the window ([decision 0254](../../decisions/0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)). Also built is `form_login`: the person stores the username and password on the engine's own page, the provider fills the application's sign-in page in the engine's browser, and a one-time code the page asks for is asked of the person on the same page, each time ([decision 0245](../../decisions/0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md)).

A sign-in through a provider that holds or exchanges a credential is a person's act an agent asks for with `session.login`, shown on a page of its own as a publish is: the page names the target, the environment, the provider, the vault reference of each credential, whether the sign-in is declared by a published version or only by the drafts, and every address a credential or a token of it is sent to, with an empty field for each credential not stored; its declaration is pinned, so drafts that change before the press sign in nowhere ([decision 0248](../../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)); the press raises the presence prompt and carries what was typed to the engine alone, which seals it for the person. Where the provider then asks for a one-time code, the press's answer is a page that takes it, for that one sign-in ([decision 0205](../../decisions/0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md)).

## On a server

| Surface | How it works | Phase |
| --- | --- | --- |
| Approval page | The same diff page, served by the server's console, reachable only after the approver signs in to the organization's identity provider | 6 |
| Chat card | A card in the organization's chat tool with the counts and a link to the approval page; approving happens on the page | 6 |
| URL-mode elicitation | The MCP client is sent the approval page's address, which the person opens in their own browser | 6 |
| Hosted sign-in | A page that shows the server runner's browser to the person so they can sign in to the system there | 7; needs research brief 17 |

On a server the identity provider's sign-in is the proof of a person, so no operating-system prompt is involved.

## Rules for every surface

- **The page shows the diff the hash was computed from,** read from the stored diff, not rebuilt.
- **Approve and Decline are equally easy to press.** Declining ends the run as `expired` and records who declined.
- **An approval page expires** with the approval window, 15 minutes by default, and says so.
- **It states what is not a field change:** uploads, sends, or anything irreversible, in words, above the records.
- **It names the caller:** which agent or program started the run, and for whom.
- **Restricted fields** are shown only to approvers allowed to see them; others see that a restricted field changes.
- **It is usable without a mouse and by screen readers.**
- **No approval is ever implied by silence.** A timeout is a decline.

## What has to be built

| Piece | Language | Phase |
| --- | --- | --- |
| The diff page, sign-in status page, and decision-point form | TypeScript, in `web/console` | 3 |
| The short-lived loopback listener | Rust, in `a2m-local` | 3 |
| System notifications | Rust, through a cross-platform crate to be chosen | 3 |
| Operating-system user-presence prompts | Rust, per operating system: Linux first, then macOS and Windows ([research note 20](../../research/20-operating-system-user-presence-checks.md)) | 3 |
| The server console's approval pages | TypeScript | 6 |
| Chat cards | Rust, per chat tool | 6 |
| The hosted sign-in page | To be designed | 7 |

## Open points

- Settled by [research note 20](../../research/20-operating-system-user-presence-checks.md): which user-presence prompts a background process can raise on each operating system, and what `doctor` says where none is usable. Still open: whether the Windows prompt comes to the front for an engine with no window in the foreground, which needs a person to try.
- Whether opening the default browser is acceptable where that browser is locked down; the fallback is the engine's own browser window.
- The hosted sign-in page.
