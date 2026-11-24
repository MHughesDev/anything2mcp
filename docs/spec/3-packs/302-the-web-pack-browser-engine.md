# 302. The web pack: the browser engine

The `ui` and `page_http` modes run on a browser engine written in Rust that speaks the Chrome DevTools Protocol directly to Chrome, Edge, or Chromium, with no Node process in between. Built today, for what the design of [`a2m-pack-web-browser`](../../architecture/crates/a2m-pack-web-browser.md) marks done.

**Why not Playwright.** Playwright has no official Rust client; its other language clients pipe every call through a bundled Node driver. A direct Rust engine removes that process and its memory. Skyvern's Rust engine reports about 70% less memory and 2.55 times faster start than the equivalent Playwright process ([Skyvern, vendor-reported](https://www.skyvern.com/blog/rustwright-playwright-rewritten-in-rust-that-occupies-70-less-memory-and-is-2-55x-faster/)). The cost is that Playwright's waiting and actionability logic must be rebuilt. That is affordable here because the engine needs only the dozen built-in verbs over declared locators, not a general automation API.

**What the engine does.**

- **Locates by meaning.** Elements are resolved from the accessibility tree and a small injected helper, by role, name, label, text, or test id, through open shadow roots and declared frames.
- **Checks before acting.** Visible, enabled, stable in position, and able to receive the event, as one in-page call.
- **Waits on events.** In-page observers and protocol events signal readiness; nothing polls.
- **Observes and sends traffic.** Network events feed confirmations and read-backs; `page_http` requests are issued from the page's own context.
- **Launches with its own profile.** It launches its own pinned Chromium, or the installed Chrome or Edge, always with a data directory the engine owns. It does not attach to the user's everyday browser or read its profile: Chrome no longer allows the DevTools protocol on the default profile, and doing so is what cookie-stealing malware does ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)).

**Build or adopt.** The engine stands on a protocol client of its own, a few hundred lines over the browser's debugging pipe, and on no outside DevTools crate ([decision 0070](../../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md)). A spike compared it with existing Rust work, chiefly [chromiumoxide](https://scrapfly.io/blog/posts/best-rust-web-scraping-libraries) and Skyvern's MIT-licensed, alpha-stage [Rustwright](https://codestuff.io/repo-of-the-day/916): none can use the pipe on all three operating systems, and the part they would save is the small part ([research note 17](../../research/17-browser-engine-base.md)). Firefox and WebKit are out of scope for the first release; an optional Playwright sidecar plug-in can cover them later.

**Runtime.**

- **Tabs, not copies.** Each run gets its own tab in its session's browser profile, sharing the live sign-in. Each identity has its own browser process and profile ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)).
- **Restarts.** A runner's browser is recycled after a set number of runs, about 30 minutes, or a memory ceiling, letting the current run finish. After three hangs of its tabs it is recycled between runs too, which is built; a tab hangs when its page answers not even a probe after a call's deadline, and a hung tab is closed at once (decision 0999). Long-lived browser pages are known to grow in memory ([Playwright issue](https://github.com/microsoft/playwright/issues/6319)), and Browserless historically recycled every 30 minutes ([docs](https://docs.browserless.io/baas/v1/docker/config)).
- **Supervisor.** Detects page crashes and disconnects, enforces a hard and an idle timeout per run, and kills the whole process tree on failure. A crash during a write becomes `possible_commit`, never an automatic retry.
- **Fixed environment.** Viewport, locale, time zone, color scheme, and reduced motion are set per target and never vary.

**Bot defenses.** Products such as Cloudflare, Akamai, Imperva, and Azure Front Door judge each request, not the signed-in user, and can block an automated browser even for a licensed user ([Cloudflare](https://developers.cloudflare.com/bots/concepts/bot-detection-engines/), [Azure](https://learn.microsoft.com/en-us/azure/frontdoor/waf-overview)). Anything2MCP does not hide that it is automation.

- A 403 or 429 from the edge, a `cf-mitigated` header ([Cloudflare](https://developers.cloudflare.com/cloudflare-challenges/challenge-types/challenge-pages/detect-response)), a challenge page, or an unexpected sign-in redirect is classed `edge_blocked`: stop, keep evidence, alert a person.
- The supported fix is an allowlist from the app's owner or IT: the runner's outbound address plus a secret header or client certificate.
- Requests are rate-limited per surface.
