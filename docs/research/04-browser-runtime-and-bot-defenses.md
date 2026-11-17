# Research: browser runtime management and bot defenses

**Status:** completed by a research agent on 2026-11-15. Feeds [spec section 302](../spec/3-packs/302-the-web-pack-browser-engine.md) and [section 211](../spec/2-kernel/211-concurrency.md).

## Questions

1. How should a long-running service manage browser lifecycle, memory, crashes, and concurrency?
2. Do enterprise bot defenses block automated browsers even for signed-in, licensed users, and what is the honest way to handle that?

## Findings

1. Browser contexts are fast, cheap, and isolated like incognito profiles inside one browser. Playwright's test runner gives each test a new context. [Playwright](https://playwright.dev/docs/browser-contexts)
2. Browserless today: reuse browsers and contexts instead of launching new browsers; cap concurrency; set the queue to about 1.5 to 2 times that (a full queue returns 429); reject new sessions above CPU and memory thresholds; scale out with more containers. [Browserless](https://docs.browserless.io/enterprise/docker/best-practices). Their 2018 write-up used one browser per job to contain crashes and warned that idle browsers degrade after a few hours. [Browserless blog](https://www.browserless.io/blog/observations-running-headless-browser)
3. Browserless v1 restarted kept-alive Chrome every 30 minutes by default and could exit when a health check failed. [Browserless v1](https://docs.browserless.io/baas/v1/docker/config)
4. Memory growth in long-lived pages and contexts is unresolved in Playwright. The workarounds are closing contexts and restarting the browser on a schedule. [Playwright issue 6319](https://github.com/microsoft/playwright/issues/6319)
5. Steel: every session has a hard timeout plus an optional idle timeout, can be released explicitly, and records why it ended, including `browser_crashed`. Their scaling guide uses a durable queue, a dispatcher that starts work only while below the session cap, and a dead-letter queue after two retries. [Steel lifecycle](https://docs.steel.dev/overview/sessions-api/session-lifecycle), [Steel scaling](https://llms.steel.dev/articles/scaling-browser-automation-to-hundreds-of-sessions/)
6. Crashes surface as page-crash and browser-disconnected events. In Docker, `--init` prevents zombie processes and `--ipc=host` prevents Chromium running out of shared memory. [Playwright Docker](https://playwright.dev/docs/docker)
7. Only one browser can use a given user-data directory at a time. A saved storage-state file holds cookies that can impersonate the user. sessionStorage is not saved, and IndexedDB is saved only on request. [Playwright auth](https://github.com/microsoft/playwright/blob/main/docs/src/auth.md)
8. Chrome 132 removed the old headless mode from the Chrome binary; new headless is the real browser. New headless still sends `HeadlessChrome/` in its User-Agent. [Chrome](https://developer.chrome.com/blog/removing-headless-old-from-chrome), [Playwright browsers](https://playwright.dev/docs/browsers)
9. Other automation giveaways: the `sec-ch-ua` header, `navigator.webdriver = true`, globals an automation library injects into the page, and side effects of DevTools-protocol traffic. [Castle](https://blog.castle.io/how-to-detect-headless-chrome-bots-instrumented-with-playwright/)
10. Bot-defense products judge each request, not the signed-in user:
    - Cloudflare scores requests 1 to 99 and has a JavaScript check that flags headless browsers. [Cloudflare](https://developers.cloudflare.com/bots/concepts/bot-detection-engines/)
    - Azure Front Door Premium sorts bots using IP reputation; custom Allow rules run before managed rules. [Azure](https://learn.microsoft.com/en-us/azure/frontdoor/waf-overview)
    - Akamai lets site owners define "friendly" custom bot categories. [Akamai](https://techdocs.akamai.com/cloud-security/docs/detection-methods)
    - Imperva flagged a company's own synthetic monitors as bad bots; the fix was allowlisting. [Dynatrace community](https://community.dynatrace.com/t5/Synthetic-Monitoring/Imperva-seeing-Dynatrace-synthetics-as-a-bad-bot/m-p/206343)
11. A Cloudflare challenge response carries the header `cf-mitigated: challenge`. [Cloudflare](https://developers.cloudflare.com/cloudflare-challenges/challenge-types/challenge-pages/detect-response). Cloudflare also has "signed agents" (Web Bot Auth) as a way for automation to identify itself honestly. [Cloudflare](https://developers.cloudflare.com/bots/concepts/bot/signed-agents)

## Recommendations adopted in the spec

1. One browser per worker and a fresh context per run, seeded from the stored session. Recycle the browser after N runs, about 30 minutes, or a memory limit, letting running jobs finish first.
2. A supervisor that listens for crash and disconnect events, enforces a hard and an idle timeout per run, and kills the whole process tree. A failed write is never replayed automatically unless it is idempotent.
3. A durable queue keyed by (app, identity): writes one at a time, reads two to four at once, an overall cap sized to memory, and an explicit busy result when the queue is full.
4. Do not hide automation markers. Ask the app owner or IT to allowlist the runner's outbound address plus a secret header or client certificate. Rate-limit per surface.
5. Treat any of these as `edge_blocked`: a 403 or 429 from the edge, `cf-mitigated`, a challenge or CAPTCHA page, or an unexpected sign-in redirect. Stop with no retry, save evidence, and alert a person.

## Uncertain

- No vendor documents whether its bot rules challenge signed-in sessions; it depends on each tenant's policy.
- Signed agents are aimed at public agents and may not fit an internal tool.
- Whether target apps limit concurrent sessions per user is unknown and must be probed per app.
- Restart and memory thresholds need measuring on the real engine.

## Note on the Rust engine

These findings describe Playwright-based services. Anything2MCP's browser engine speaks the DevTools protocol directly from Rust (see [09-rust-building-blocks.md](09-rust-building-blocks.md)). The lifecycle lessons carry over; the specific flags and behaviors must be re-verified against the chosen engine.
