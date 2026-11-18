# 0211. Whether the engine may ever act in a session the person already has

**Status:** Proposed. Owner decision: it asks about rule five, which only the owner changes. Written by the coverage review of 2026-11-18; the owner has not read it. Nothing here is in force, and rule five stands as written until the owner answers.

**Date:** 2026-11-18

**Asks about:** rule five of [AGENTS.md](../../AGENTS.md#rules-that-never-bend); [decision 0031](0031-the-browser-always-runs-with-a-profile-the-engine-owns.md); [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md).

## Context

Rule five says the browser always runs with a data directory the engine owns, and the engine never reads, copies, or drives the person's everyday browser profile or another program's cookie store. Decision 0208 says the product reaches whatever the person's own computer can reach, under the ten rules.

The [coverage review](../plan/coverage-review-2026-11-18.md) found four places where the two meet, and the documents did not agree on one of them.

1. **The person's own signed-in browser.** It is the largest single draw of the tools in the landscape ([research note 27](../research/27-the-public-landscape-of-agent-tools.md), finding 1): an extension in the person's profile, or a connection to a browser the person started. Decision 0031 lists "a browser extension inside the user's profile, with consent" as "possible later as its own mode", and [section 704](../spec/7-project/704-readiness.md) listed it as "later, if ever". Rule five forbids it. The review corrected section 704 to say so.
2. **An application built on a browser runtime.** [Section 301](../spec/3-packs/301-the-web-pack.md) has the supervisor start such an application and drive it over the DevTools protocol. Started as it normally starts, it opens its own data directory: the person's signed-in state in that application, which is another program's store. The review made section 301 say it is started with a data directory the engine owns, or refused.
3. **What it costs.** Every web application and every such desktop application needs one sign-in in the engine's own profile. Where an application allows one session per person, the engine's sign-in may end the person's. Where a session is bound to the device and the profile, research brief 30 asks whether a new profile can sign in at all (hypothesis H-05).
4. **A permission the person grants the engine.** On macOS, the accessibility tree and Apple events each need the person to grant the engine a permission in the system's settings; reading another application's files may need Full Disk Access. The rule says "no getting past a device's protection". A protection the person lifts on purpose, for this program, through the system's own settings, is not plainly a bypass, but the rule does not say so, and [research note 26](../research/26-desktop-pack-before-it-is-built.md) already treats writing the permission row by script as something the product must never do.

## The safest way through that stays inside the rule

- Sign in once, in a visible window of the engine's own profile, and let the session keeper keep the session alive ([section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md)). This is the design.
- For an application built on a browser runtime, start it with a data directory the engine owns, and sign in there. Whether the common frameworks accept one is research brief 35.
- Where an application has another surface (its HTTP interface, its files, its command-line tool), use that.

## The questions for the owner

1. May the engine ever act inside the person's everyday browser, through an extension the person installs and pairs, that acts only in tabs the person marks, and shows when it is acting? If yes, it is its own mode, with its own decision record, and the rest of the rules hold: no secret leaves the browser, every write is read back, and a person approves writes.
2. May the engine ever start an application built on a browser runtime with that application's own data directory, the person having named it, so that the person's existing sign-in in it is used?
3. Is a permission the person grants the engine through the system's own settings (Accessibility, Automation, Full Disk Access) inside rule five, provided the engine never grants it to itself, never asks for more than the target declares, and `doctor.run` shows what it holds?

## Recommendation

Answer 3 yes: without it no desktop application on macOS can be reached at all, and the grant is the person's, open, and revocable in the system's own settings. Hold 1 and 2 at no for the first release: each would put the person's everyday sessions, with every site in them, within reach of a run, and both can be revisited once writes are proven on the engine's own profile.

## Alternatives weighed

- Treating question 3 as already answered. Refused: rule five is the owner's, and the review cannot read it either way for them.
- Folding the extension into the web pack's `ui` mode. Refused: a different browser, owned by the person, is a different surface with different guarantees, and would need its own mode whatever the answer.
