# 0114. The web probe as built: three discoverers over an observing tab

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the design of [the web probe crate](../architecture/crates/a2m-pack-web-probe.md): its registrations, its interface, and what capture means until the base has a stream of exchanges.

## Context

The design gave the crate six discoverers, a stream of exchanges from the base, platform profiles from the base, and a sandbox port that hands over stored findings. Package E2 built the crate against what exists: an observing tab that can go to an address, locate, read, wait, and take a snapshot, and that reports each request it refused.

## Decision

- **The crate is a provider of its own, `web_probe`.** *Instead of:* registering inside the `web` provider. A pack is handed its context only while it registers, and the two crates of the web pack may not name each other. The product crate makes one cell of browsers and gives it to both; the crate keeps only the cell's observer.
- **Three discoverers are registered: `web_probe`, `web_snapshot`, `web_capture`.** `web_auth`, `web_endpoints`, and `web_declared` are not built and not declared.
- **Endpoint inference runs inside `web_capture`.** *Instead of:* a separate `web_endpoints` that reads stored exchange findings. The sandbox port as built hands over no stored finding, so a separate discoverer would have no input. The inference is a pure function, `infer_endpoints`, and can be registered apart when the port can.
- **Capture sees two kinds of exchange.** The request for each document the tab is sent to, with the shape of a JSON answer; and each request a page tried that the tab refused, which is `inferred` because the system never answered it. *Not seen:* the requests a page's own scripts read with. The base has no stream of exchanges yet, and the crate does not work around that.
- **A discovery looks as nobody.** The scope carries no session and no identity, so the profile is the engine's own account `discovery` of the target and environment. A page that asks for a sign-in is a finding of the probe and a `needs_login` failure of the other two.
- **A sign-in wall is told by three observations:** the document answered 401, the tab ended at another origin, or the page shows a password input. Such a page is not read.
- **A block is a status:** 403 is `forbidden_by_edge`, 429 is `rate_limited_by_edge`. `challenge_page` needs marks of the page and is not built.
- **A value is replaced by its type everywhere.** A screen finding keeps the accessible name of a role that is structure (a heading, a control, a column header, a link outside a record) and the type of every other name and of every value. An endpoint finding keeps, for a parameter, how many different values were seen, by digest. *Instead of:* keeping every literal beside a parameter, as the design said: a literal of a path is a record's identifier.
- **Tier two joins only endpoints that both answered with JSON whose top holds the same names.** *Instead of:* any two requests that differ in one segment, which would make one endpoint of two unrelated pages.
- **A link is followed only when it is a plain link to the same origin, its name is shared with no other link, and neither its address nor its name holds a word of `ACTION_WORDS`.** Words such as `new` and `add` are not on the list: opening an empty form is a read. Every link not followed is counted in the report.
- **A screen holds its fields and actions.** The finding kinds `field` and `action` are not declared: one `screen` finding holds the elements of its snapshot.
- **The `endpoint` finding is widened, not changed.** Its first seven names are the HTTP pack's; `answered`, `statuses`, `template`, `query`, `response`, and `seen` are added.
- **The focus of a discovery has three names:** `paths`, `ready_text`, and `tier_two`. A path can name a page of the surface and nothing else.

## Consequences

- The four ways a page could write during discovery are each proven against the test application's control interface: a page that writes when it loads, a worker, a socket, and a link that looks like an action.
- A page inside an application that holds a password input, such as a page for changing one, is taken for a wall and not read.
- The check that refuses the names of the acting types in this crate's source must match whole words: the crate names the cell `SharedBrowsers`.

## Revisit if

- The base gains a stream of exchanges: capture then reads what a page's scripts request, and the header allowlist applies.
- The sandbox port hands over stored findings: `web_endpoints` is then registered apart.
- The scope carries a session marked for discovery: the wall rule then applies only to a surface with none.
