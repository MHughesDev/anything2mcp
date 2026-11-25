# Research: the state of WebMCP, and whether the engine can call a site's tools

**Status:** completed on 2026-11-17 from the proposal's repository and from a short run against Chrome for Testing 153.0.8010.12, headless, on Linux, with the engine's kind of client (a DevTools-protocol client over a socket, from a throwaway script outside the repository). It answers brief 24 of [open research](11-open-research.md) and feeds roadmap package 8.2a ("Tools a site publishes, as a mode of the web pack"), [section 301](../spec/3-packs/301-the-web-pack.md), and [section 201](../spec/2-kernel/201-access-kinds-and-modes.md). The pages on `w3.org`, `webmachinelearning.github.io`, and `developer.chrome.com` were blocked in this session; the proposal was read from its GitHub repository instead, which holds the specification source, the explainer, and the implementation-status table.

## Question

What is the status of the WebMCP proposal and of Chrome's implementation? Can an external agent, here the engine, discover and call the tools a page registers through the DevTools protocol? How widely is it adopted? What follows for the `site_tools` mode?

## Answer

- **Status.** A Community Group draft of the Web Machine Learning group, not a standard. The specification source says `Status: CG-DRAFT`, `Level: None`. Editors are from Microsoft and Google. The imperative API is what it defines; a declarative form-based API is explored in a separate explainer and "is not part of the specification at this time".
- **Chrome and Edge ship it for trial.** The status table says an origin trial is live in Chrome 149 and Edge 150, and that a local flag exists. In the Chrome build used here (153.0.8010.12) no flag and no trial was needed: on a page served from `127.0.0.1`, which is a secure context, `document.modelContext` was present.
- **Yes, an external client can discover and call the tools,** and this does not need to run page script of our own. Chrome has an experimental DevTools domain, `WebMCP`: `enable`, `invokeTool`, `cancelInvocation`, and the events `toolsAdded`, `toolsRemoved`, `toolInvoked`, `toolResponded`. All four steps were run here (below).
- **Adoption is early.** The status table lists support in a browser's built-in assistant for Brave (a release "expected on January 5, 2027", which is in the future as of today), a desktop assistant app, and glasses; Firefox and Safari have standards-position issues open and no implementation. The table is the project's own and was not checked against those products.
- **For the design: keep `site_tools`, hold its shape until the draft settles, and never take a tool's own claims as facts.** Details below.

## What was read

| Claim | Source and how it was read |
| --- | --- |
| The specification source declares `Status: CG-DRAFT`, group `webml`, editors from Microsoft and Google | [`index.bs`](https://github.com/webmachinelearning/webmcp/blob/main/index.bs), header read |
| A page registers a tool with `document.modelContext.registerTool()`: a name, a description, an input schema, an optional output schema, an `execute` function, and annotations; an `AbortSignal` unregisters it | [Explainer](https://github.com/webmachinelearning/webmcp/blob/main/README.md), "Detailed Design", read in full |
| Annotations are four hints: read-only, untrusted content, consequential, debugging | Specification source, the `ToolAnnotations` dictionary |
| The API exists only in a secure context | Same: `SecureContext` on the interface |
| Tools are exposed by default to the page itself, same-origin frames, and the browser's built-in agent; a page shares with other origins only by naming them; cross-origin frames need the `tools` permission | Explainer, "Permissions policy and iframes" |
| `getTools()` and `executeTool()` let an in-page agent discover and run tools; a `toolchange` event announces changes | Explainer |
| The specification says a malicious site "can omit or misrepresent" its hints, and that protecting an agent from a hostile site is chiefly the agent's own job | Specification source, the security section, lines 1690 to 1704 |
| Open questions include user confirmation for consequential tools, streaming, progress, tools without an open tab (through service workers), and what happens to a response when a tool navigates | Explainer, "Open Questions" |
| Origin trial in Chrome 149 and Edge 150; positions open at Firefox and Safari; support in a Brave assistant, a desktop assistant, and glasses | [Implementation status](https://github.com/webmachinelearning/webmcp/blob/main/implementation-status.md), read in full |
| The DevTools protocol has a domain `WebMCP`, marked experimental, depending on `Runtime`, `Page`, and `DOM` | [`browser_protocol.json`](https://github.com/ChromeDevTools/devtools-protocol/blob/master/json/browser_protocol.json) on the master branch, read as data |

## What was run

One throwaway Node script (outside the repository, not kept) opened a tab on a page served from `127.0.0.1`, sent `WebMCP.enable`, registered two tools with `document.modelContext.registerTool()`, and recorded the events. Run three times with no flag, with `--enable-experimental-web-platform-features`, and with `--enable-features=WebMCPTesting`; the results were the same each time.

| Step | Result in Chrome for Testing 153.0.8010.12 |
| --- | --- |
| `typeof document.modelContext` on `about:blank` | `undefined`, because `about:blank` is not a secure context |
| The same on `http://127.0.0.1` | `object`, with no flag |
| `WebMCP.enable` | Accepted |
| Register a tool | `WebMCP.toolsAdded` arrived with the tool's name, description, input schema, annotations as `readOnly` and `untrustedContent`, the frame identifier, and the registration's stack trace |
| The `toolId` the protocol file describes | **Absent** from the tool in this build |
| `WebMCP.invokeTool` with `toolId` | Refused: "Failed to deserialize params.toolName ... mandatory field missing". This build wants `toolName`. The protocol file on master makes `toolName` optional and deprecated and prefers `toolId`, so the browser build and the published file differ by some weeks |
| `WebMCP.invokeTool` with `toolName` | Returned an invocation identifier at once; then `toolInvoked` (name, frame, the input as text) and `toolResponded` (status `Completed`, the tool's result as `output`) |
| A tool that throws | `toolResponded` with status `Error`, an empty `errorText`, and an `exception` object that holds the page's error |
| Unregister (abort the signal), then invoke the old name | `toolsRemoved` arrived; the invoke was refused with "Tool not found" |
| A tool registered with `consequentialHint: true` | The annotations the protocol reported did not include a consequential flag. Either the build does not carry it or it reports under another name; not settled |

Nothing was tried across frames, after a navigation, with a tool that navigates, or on Windows or macOS. The browser itself tried to reach two Google hosts while running; the session's proxy refused them and the run was not affected.

## What it means for the design

1. **`site_tools` can be a mode of the web pack with no script of the engine's own in the page.** The pack's browser engine already speaks the protocol; this adds one domain to its client. Rule 6 (Rust first) is untouched.
2. **A site's tool is a request that runs the site's own JavaScript.** Its effect is whatever the site's code does, and nothing in the protocol says what that is. The annotations are the site's claim, and the specification says they can be false. They must never set an effect class. A drafted operation over a site tool starts at `write` with unknown hazard, and the policy gate treats it as such until a developer declares otherwise in the operation file with a read-back of another mode ([rule 7](../../AGENTS.md): unknown means stop). `readOnly: true` may be recorded as a `claimed` fact on a finding, not as evidence.
3. **Bind a call to a tool by name and a digest of its schema, not by identifier.** This build has no identifier and requires the name; the later protocol offers an identifier that changes on every registration. A plan that names a tool records its name and the hash of its description and input schema; at run time, before a call, the driver takes the tool list the page announced, and a name that is missing, or whose hash differs, is a named stop (`drift`, in the family of section 304's changed-tool rule). The description is page text and is untrusted ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)): it is hashed and never shown to an agent as an instruction.
4. **Results are untrusted text.** An output of a site tool that is or holds text must be of a field type marked `untrusted`, as for streams ([section 309](../spec/3-packs/309-the-stream-pack.md)). The annotation `untrustedContent` is not trusted either.
5. **Waits.** Tools appear and disappear as the page changes; `toolsAdded` and `toolsRemoved` are named conditions to wait on, so no sleep is needed. A call ends on `toolResponded`, which needs a deadline of its own and `cancelInvocation` on expiry.
6. **Fate.** A call whose answer never arrives (page closed, navigated) has an unknown fate. For a write this is `possible_commit`; the explainer lists the navigation case as unsolved.
7. **The proposal moves.** The domain is experimental, the protocol file and the shipped browser already differ, and an `Agentic Web Working Group` charter draft is cited. The mode should sit behind one private interface in the pack and its tests run against the test application's own page, which the roadmap package already says registers a tool.
8. **Section 301 and section 201** say "an early standard, offered by few sites". That stays true. The correction is that the call path is the DevTools `WebMCP` domain, which the sections do not name, and that the draft has no stable identifier for a tool yet.

## What could not be verified

- The W3C, `webmachinelearning.github.io`, and Chrome documentation pages were blocked; nothing from them is used. The proposal text is from the repository at its `main` branch on 2026-11-17.
- Whether the `WebMCP` domain needs a flag in a stable (not Chrome for Testing) build, and which Chrome versions carry it.
- The adoption claims in the status table were not checked against the products.
- Behaviour across frames, after navigation, with `exposedTo`, and the declarative form tools.
- The browser flag name for local development ("enable-webmcp-testing" in the status table) was not needed and not tried.
- Windows and macOS.
- Whether a tool can be called while its tab is in the background, or in a page the engine did not open.

## Recommendation, and the alternative that lost

Build `site_tools` on the DevTools `WebMCP` domain, bound by name and schema digest, classified as unknown-effect until declared, with its outputs untrusted, behind a private interface; do not build the declarative form API or service-worker discovery. The alternative that lost is calling `document.modelContext.executeTool()` from a script the engine injects: it works in the same browser, but it puts the engine's code in the page's world, is limited by the page's origin rules, and gives no events to wait on. The domain gives both. Revisit when the domain leaves experimental status.
