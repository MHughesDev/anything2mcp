# 0321. A sign-in method is found from addresses, and a passkey or a mailed link is the person's

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Extends [decision 0254](0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md).

**Date:** 2026-11-23

**Changes:** [section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md) ("Finding out how a target signs in", "Sessions bound to the device", "Open points"); [section 503](../spec/5-security/503-approval-and-sign-in-surfaces.md) ("Signing in on one machine"); [SCOPE.md](../../SCOPE.md).

## Context

[Research note 35](../research/35-web-sign-in-methods-and-engine-owned-profiles.md) looked at how each web sign-in method shows itself, at device-bound session credentials, and at passkeys in a fresh profile. No primary page could be opened: every outside fact in it is from a search summary. This record therefore builds only on what follows from the design and the rules, and leaves the outside facts as open points: which platforms ship device-bound sessions, the header a site opts in with, and what an exported cookie does at renewal.

## Decision

1. **`auth.detect` names a method from addresses, redirects, and the first response's headers,** beside the traffic finding section 215 already has, and never from a sign-in page's fields (decision 0254). It adds two signs: a redirect carrying a `SAMLRequest`, or carrying `response_type` and `client_id`; and a redirect to an identity-provider origin or a sign-in screen the target declares. Each sign is proven against the test application before detection relies on it, never only against an outside description.
2. **What does not show is unknown.** A passkey, a link sent by mail, and a second factor do not show in addresses, so detection names them unknown and a person says which it is (rule 7). A finding is evidence for a recommendation, never a settled method.
3. **The engine never answers a passkey, presence, or second-factor prompt.** The person does, in the engine's window. The browser's virtual authenticator is used only in tests against the test application. The window's notice says that a passkey made there belongs to that profile and its provider.
4. **A sign-in by a mailed link is not supported.** The link opens in the person's everyday browser, which the engine never drives (rule 5), and the link is a credential. A route where the person brings the link into the engine's window is an open point: it widens what the window is, and needs its own record and test.
5. **A refused restore marks a session not movable.** Until a run shows how a device-bound session can be seen, a restored session the system refuses is marked not movable, so it is never restored on another runner.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Mark a session not movable when a registration header is seen | The header's name and the browser's report of it are known from search summaries only; kept as the open point it is |
| Detect a passkey from the page | It means reading the sign-in page, which decision 0254 forbids |
| Let the engine answer a passkey prompt through the virtual authenticator | It would sign in as the person without the person: rule 5 |

## Proof

Package 2.3's sign-in window, in the test application: each method the application serves is detected from its addresses and headers, and a passkey sign-in is named unknown; a passkey prompt in the window is left unanswered by the engine and ends in `needs_login` after the person's time; a restored session the application refuses is marked not movable.
