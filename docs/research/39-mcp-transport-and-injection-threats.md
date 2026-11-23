# Research: MCP transport security, and prompt injection and MCP-specific threats

**Status:** completed on 2026-11-23, and **Partial**. The site `modelcontextprotocol.io`, OWASP's cheat-sheet host, `invariantlabs.ai`, `simonwillison.net`, and `osv.dev` did not resolve from the research session. Four pages of the specification were read from the specification's repository on `raw.githubusercontent.com`, through a summarizing model, so quotes are as that model returned them and should be re-read. Everything about tool poisoning, shadowing, and rug pulls comes from search-result summaries. Nothing was built or run. Answers briefs 13 and 14 of [open research](11-open-research.md). Feeds packages 6.2 (MCP over Streamable HTTP as an OAuth resource server) and V5 (description bounds) of the [roadmap](../plan/roadmap.md), [spec section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md), [section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md), [section 221](../spec/2-kernel/221-the-tool-surface-agents-see.md), and [decision 0307](../decisions/0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md). Extends [note 06](06-mcp-protocol-and-authorization.md) and [note 18](18-mcp-server-base.md).

Tags: **[read]** the text was opened (through the summarizer where said). **[search]** from a search-result summary only; a lead. **[memory]** not opened.

## Question

Which threats do the MCP transports and the MCP protocol bring, as published? What defenses are proposed? Which of this product's rules and designs already answer each, and where is there a gap? The product's side of the question is narrow: it is an MCP server only. It is never an MCP client, so threats that live in a client or a host are named and set aside.

## Answer in short

- **DNS rebinding on a local HTTP server is real and common.** One advisory family covers the TypeScript SDK (CVE-2025-66414, fixed in 1.24.0, protection off by default) **[read]**, and search results list the same flaw in the Rust SDK (`rmcp`, CVE-2026-42559 / advisory GHSA-89vp-x53w-74fx), the Ruby and Go SDKs, and several servers **[search]**. The specification's rule is: validate `Origin` and answer 403; bind to loopback; authenticate **[read]**.
- **The product's first-release design avoids the whole class.** The local channel is a Unix socket or a named pipe with a peer-identity check, and no TCP port is open by default ([section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md)). Stdio is a proxy to that channel. The one loopback listener (approval and sign-in pages) already checks `Host` and `Origin` ([section 503](../spec/5-security/503-approval-and-sign-in-surfaces.md)). The gap is the **workstation profile's local HTTP** and package 6.2: neither is built, and neither has a written rule for `Host`, `Origin`, binding, or authentication.
- **The MCP authorization specification is a resource-server specification, and package 6.2 can follow it whole.** Servers MUST publish RFC 9728 metadata, validate that a token was issued for them (audience), never accept or pass through other tokens, and answer 401 or 403 correctly **[read]**. The product already forbids forwarding a caller's token ([section 602](../spec/6-running-it/602-identity-on-a-shared-server.md)). Stdio is told to skip it **[read]**.
- **Tool poisoning, rug pulls, and shadowing are attacks on the client side of the trust line.** They need a client that loads descriptions from a server it does not control. This product's server describes only tools a person published, from a required template. Rule 1 and section 502 already say text from the system never flows into a description. What it does not yet say: how a client can *detect* a changed description (rug pull), and what a description may contain (V5). Both are small.
- **Prompt injection through results is the larger live threat.** The product already marks system text as untrusted, bounds it, and strips control and bidirectional characters. Rule 1 means the server never obeys it. What remains is outside the product: the agent reads it. The honest statement is the one section 502 already makes ("What this cannot do"). One useful addition: say the same in the tool's `_meta`, so a client can treat those fields as data.

## How this was surveyed

| Source | Reached | How |
| --- | --- | --- |
| Specification repository files: transports, authorization, tools, index, for revision 2025-11-25 | Yes | Fetched from `raw.githubusercontent.com`; text returned by a summarizing model. The security-best-practices page returned 404 at the two paths tried |
| TypeScript SDK advisory GHSA-w48q-cv73-mx4w | Yes | GitHub page, summarized |
| Rust SDK advisories page | Partly | Title, date, and severity of GHSA-89vp-x53w-74fx only; no versions or CVE id on the page returned |
| `modelcontextprotocol.io`, OWASP cheat-sheet host, Invariant Labs, Simon Willison, OSV | No | Host did not resolve. Claims from these are **[search]** |
| Web searches | Yes | On the technical topics only. Nothing from this repository was sent |

The 2026-07-28 revision's text of these pages was not read. Note 06 says that revision removes protocol sessions and the legacy HTTP+SSE transport; where the 2025-11-25 text below differs, read the newer one first.

## Part A: transport security (brief 13)

### A1. Stdio

**Published.** A server MUST NOT write to stdout anything that is not a valid MCP message. Stderr may carry log text, and a client may ignore it **[read: transports]**. The authorization specification says stdio implementations SHOULD NOT follow it and should take credentials from the environment **[read: authorization]**. The OWASP guidance says stdio avoids a listening endpoint but does not limit what the server process can touch: files, network, credentials **[search: OWASP cheat sheet]**. The specification's local-server-compromise section (not read) concerns a client that runs a server's start command without consent **[search]**.

**Threat.** Two kinds. First, a stray write to stdout corrupts the stream; this is a fault, not an attack. Second, a client starts a program named by an untrusted configuration. The second is the client's problem. The product is the program being started.

**Answered here by.**

| Threat | Answer in this product |
| --- | --- |
| Stray stdout | The stdio mode is a proxy that "copies MCP messages ... and adds nothing" ([section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md)). Logs go to the engine's log, not the proxy's stdout. **Not written down:** a test that the proxy writes only valid messages to stdout and diagnostics only to stderr |
| Credentials for stdio | Rule 4. No secret reaches a caller; the engine holds them in the vault |
| The started program has wide reach | The proxy holds no engine logic and no state; the engine is a separate process that checks the connecting user |

### A2. Streamable HTTP on a local address: DNS rebinding

**Published.** A web page's script can be made to reach `127.0.0.1` by an attacker's DNS name that first resolves to the attacker and then to loopback. The browser then treats the local server as the page's own origin. The specification: servers MUST validate `Origin` on all incoming connections; if it is present and invalid, MUST answer 403 Forbidden; when running locally SHOULD bind only to `127.0.0.1`, not `0.0.0.0`; SHOULD implement authentication for all connections **[read: transports]**.

**Record of the flaw in SDKs.**
- TypeScript SDK before 1.24.0: protection was off by default, so an unauthenticated localhost server on either HTTP transport could be reached by a malicious website and its tools invoked. CVE-2025-66414, CWE-1188, CVSS v4 7.6, published 2025-12-02. The fix enables Host-header protection by default for servers made with the helper bound to localhost. The advisory adds that running unauthenticated HTTP-based MCP servers locally is not recommended **[read: GHSA-w48q-cv73-mx4w]**.
- The Rust SDK's Streamable HTTP server: advisory "DNS rebinding vulnerability in rmcp Streamable HTTP server transport", High, published 2026-04-29 **[read: title and date only]**. Search results give CVE-2026-42559, no Host check before 1.4.0 **[search]**.
- The same flaw is reported for the Ruby SDK (fixed in 0.23.0), the Go `mcp-go` transports, and a browser-automation server (CVE-2025-9611, fixed in 0.0.40), among others **[search]**.

**A point the sources disagree on [search].** One advisory says validating `Origin` is defense in depth and the check that blocks the attack is `Host`, because script in a page cannot forge `Host`; another says to enforce both. A plain command-line client sends no `Origin`, so an absent `Origin` has to be allowed while a present one is checked. The specification's text agrees: it speaks of a present and invalid `Origin` **[read]**.

**Defenses proposed.** Do not open a port (use stdio or a socket). Bind to loopback only. Check `Host` against a short list (`localhost`, `127.0.0.1`, `::1`, with the port). Check `Origin` when present. Require a bearer credential even on loopback, so a page that gets through still has no token. Do not allow a wildcard cross-origin response header.

**Answered here by.**

| Defense | Where the product stands |
| --- | --- |
| Do not open a port | Section 222: no TCP port by default. The SDKs reach the socket directly; the Python client on Windows does not yet |
| Peer identity | The engine asks the operating system who is on the other end and refuses another user (`peer_not_allowed`). A web page cannot present as the user over a Unix socket or pipe at all |
| The approval and sign-in listener | Section 503: loopback, random port, a one-time code in the address, `Host` and `Origin` checked, nothing else served. A one-time code in the path is a second barrier that rebinding alone does not give an attacker |
| **Local HTTP in the workstation profile** | **Gap.** Section 601 says it is "switched on" and not built. No document states its bind address, its `Host` and `Origin` rules, or that it needs a token. Its threat is exactly the advisories above |
| Package 6.2 | **Gap.** The roadmap proof is "a client signs in and calls a tool; a token for another audience is refused." Nothing in it names `Origin`, `Host`, or loopback binding |

### A3. Streamable HTTP on the network: the authorization specification

**Published [read: authorization, 2025-11-25, through the summarizer].**

| Party | MUST |
| --- | --- |
| Server | Publish RFC 9728 Protected Resource Metadata with `authorization_servers`, through `resource_metadata` in `WWW-Authenticate` on a 401 or at a well-known address |
| Server | Validate access tokens, and confirm each was issued specifically for it as audience (RFC 8707); reject others; never accept or transit other tokens |
| Server | Never pass a client's token to an upstream API; use a separate token the upstream's own authorization server issued |
| Server | Answer 401 for a missing or invalid token and 403 for too little permission; a 403 for scope SHOULD carry `WWW-Authenticate: Bearer error="insufficient_scope"` |
| Authorization server | OAuth 2.1; discovery by RFC 8414 or OpenID Connect; `code_challenge_methods_supported`; redirect URIs matched to registered ones |
| Client | Use RFC 9728 for discovery; send the RFC 8707 `resource` parameter on both requests; PKCE with `S256`; send a token only to the server it was issued for, in the `Authorization` header and never the query string; store tokens securely |

**Threats the specification and its guidance name [search; the best-practices page was not read].**

- **Token passthrough.** A server accepts tokens not issued to it, or forwards them. The result is a confused deputy: a downstream service trusts a token whose audience was never it, and audit trails name the wrong party.
- **Confused deputy in an MCP proxy.** A server that fronts a third-party API with one fixed OAuth client id can be made to hand an authorization code to an attacker, because the third party's consent screen is skipped after the first consent. Mitigation: per-client consent kept by the server, checked before the third-party flow starts.
- **Session hijacking.** A guessable or leaked session identifier lets another caller inject events. Note 06 says the 2026-07-28 revision removes protocol sessions, which removes the identifier.
- **SSRF through discovery.** A client that fetches metadata addresses given by a server can be sent to internal addresses.
- **Scope inflation.** Broad scopes make a stolen token costly. Request the least.

**Answered here by.**

| Threat | Where the product stands |
| --- | --- |
| Token passthrough | Section 602 and note 06: the product is a resource server only; "a caller's token is never forwarded to a target". `per_user_token` uses an On-Behalf-Of exchange, a new token for the target's API. Matches the specification |
| Audience validation | The roadmap proof for 6.2 tests it. Must also test a token with no `aud`, with `aud` as a list, and a token for the metadata address in place of the resource address |
| Confused deputy of the proxy kind | **Does not arise:** the product holds no OAuth client of its own toward a third party on behalf of MCP callers. Where it signs in to a target, that is a sign-in by a person on the engine's page, bound to the target ([decision 0248](../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)). A different confused deputy does arise: a caller's tool call that reaches a target with the service's stored credential. Section 502 answers it: policy picks the identity, never the caller; one decider |
| Session hijacking | No protocol session on 2026-07-28; the `run_id` is a handle checked against the caller's identity. Not verified in code |
| SSRF | Section 502: scheme, host, and port never come from an input; redirects stay inside declared surfaces. The product is not a client of an authorization server it did not configure: the identity provider is set by the organization |
| Scope | Section 602 and the toolsets: toolsets have their own endpoints; rights are enforced at call time |
| No 401 loop | **Not written down:** what `WWW-Authenticate` carries when the caller is the local user over HTTP with no token |

**What the specification leaves open [read].** It makes `Origin` validation a MUST and authentication a SHOULD for Streamable HTTP; for stdio it asks for neither. A server that follows only the MUSTs is open to a local process or a page that rebinds. This product asks for more, and should say so.

## Part B: prompt injection and MCP-specific threats (brief 14)

### B1. Indirect prompt injection through results

**Published.** An agent reads text it did not write (a record, a page, a file) and obeys instructions in it. OWASP's LLM list ranks prompt injection first, and lists excessive agency separately **[memory; one search result's claim that it is ranked otherwise was doubted]**. The MCP specification's principles: tools are arbitrary code execution; descriptions and annotations are untrusted unless from a trusted server; there SHOULD be a human in the loop who can deny a call; a client should show inputs before calling to avoid exfiltration **[read: index, tools]**. A published framing of the severe case is an agent that has private data, reads untrusted text, and can send data out **[memory; the source page did not load]**.

**Defenses proposed.** Keep untrusted text out of the instruction channel; limit what the agent can do after reading it; confirm sensitive actions with a person; filter output; limit the tool set per task.

**Answered here by.**

| Defense | Where the product stands |
| --- | --- |
| Structured results, system text only in fields marked untrusted, bounded, stripped of markup | Section 502, "Nothing from the system becomes an instruction" |
| Control, bidirectional, and invisible characters written as escapes | Decision 0193, as cited in section 502 |
| Validation messages from the system kept apart from the product's own words | Decision 0162 |
| The server itself obeys nothing it reads | Rule 1: no AI at run time. Nothing in a run improvises. A planted instruction in a record cannot change a plan |
| Human in the loop for what matters | Approvals through the presence prompt; "there is no approve command, tool, or route on a local machine"; no client label makes a caller a person |
| Limits on blast radius | Records per run, rate, quota; effect classes; protected settings |
| What it cannot do | Section 502 says so: after a read tool returns, the agent's own other tools are outside the product's control |

**Gap.** The marking of "untrusted" fields exists in the product's result schema. Nothing says that the **door** tells a client which fields those are in a form a client can use, for example in the tool's output schema. A client that cannot see which fields are system text cannot treat them as data.

### B2. Tool poisoning

**Published.** Instructions are hidden in a tool's description or schema. The user's interface shows a short name; the model reads the whole text. Invariant Labs' notice demonstrated it against a code editor, leading the model to read and send key files **[search]**. Later write-ups and an OWASP entry (MCP03:2025, tool poisoning, listed with rug pulls and shadowing; the entry's own text speaks of schema poisoning) describe the same **[search]**. One paper calls it a form of indirect prompt injection **[search]**.

**Defenses proposed.** Treat every server as untrusted; show full descriptions to the user; scan descriptions with a proxy; sign or hash the description set; a second model that vets descriptions **[search]**. The last has no place in this product's run path (rule 1).

**Answered here by.** The threat needs a third party to write the description. Here:
- Descriptions are "generated from a required template", written by the target's author, reviewed by a person, and text captured from the system never flows into them unreviewed (section 221, section 502).
- Publishing needs a person, and the risk-ordered review shows "Changed descriptions" ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md), table row).
- The linter flags "descriptions that address the reader as an instruction".
- V5 (decision 0307, point 6) adds a bound: plain text, at most 1,024 characters, no control characters, and the door tells a client whose words it is.

**Gaps and sharpening for V5.**
1. The bound applies to the `description`. A tool also has an input schema with field descriptions, enum names, defaults, and an output schema. Poisoning has been reported through schema fields as well as the top description **[search]**. A bound on the description alone leaves these open. Proposal below.
2. "No control characters" is narrower than what section 502 and decision 0193 already escape in result text (bidirectional marks, invisible format characters, line and paragraph separators). A description with a right-to-left override can read differently to a reviewer than to a model. The same set should be refused in a description.
3. A length bound is not a content bound: 1,024 characters is enough for an instruction. The remaining defense is the reviewer and the linter. The note should say so and not claim more. The linter's rule is "addresses the reader as an instruction"; a plain list of its patterns (second person imperative aimed at the model, mentions of other tools, mentions of files outside the target, requests to hide something from the user) would make it testable. This is a heuristic and will be evaded; it is a help to the reviewer.
4. The target name in `_meta` tells a client where a tool came from. It is useful only if a client displays it. The product cannot make a client do so.

### B3. Rug pull (changed definitions after approval)

**Published.** A server's descriptions are clean when a user approves the server, and change later. Clients that approve once and reload the list on each connection (or on a list-changed notification) run the new text unseen. The gap is "between initial tool approval and ongoing verification" **[search]**. The protocol lets a server announce that its tool list changed: servers that declared `listChanged` SHOULD send the notification **[read: tools]**. The defense named is pinning: record a hash of each description and schema at approval, and compare it on each later load **[search]**.

**Answered here by.**
- A published version is immutable, and a tool is generated from published files, so a running engine cannot change a description between requests. A change is a new version, which a person publishes after a review that shows "Changed descriptions".
- The review of an imported bundle is the same, and imported write operations start disabled (section 502).

**Gaps.**
1. **Nothing tells a client that a description changed.** The tool's `_meta` carries the target and the version (section 221). It does not carry a digest of the description and schemas. A client that wants to pin has only the version string, which an attacker who controls a server controls too; here the engine is the same product, but the client cannot know that.
2. **The `listChanged` rule is not written.** Which events send it: a publish, a version switch for an environment, a toolset change? A version switch that changes descriptions is the product's rug pull and should always raise it. Not found in the documents read.
3. **The version a client sees can differ from the version that is reviewed.** If an environment follows a "current" pointer, the description can change at the moment the pointer moves. The review happened at publish, the pointer move may be separate.

### B4. Cross-server shadowing

**Published.** One server's description changes how the model uses another server's tool; for example it tells the model to alter the arguments it sends to a trusted server. The shadowing server never needs to be called. The shared context is the weakness **[search: Invariant Labs through secondary write-ups]**.

**Answered here by.** The product cannot see other servers; the attack acts on the agent's context. What the product controls is its own side:
- Its tool names are fixed by the rule `{target}_{module}_{object}_{verb}` with built-ins prefixed `a2m_` ([AGENTS.md](../../AGENTS.md)), which makes a name clash with another server's tool unlikely and a lookalike by this server impossible.
- Its descriptions never mention other tools except by a name the same target publishes ("what to prefer instead", section 221). **Not enforced:** the linter does not check that a description names only tools in the same version.
- Read of another server's text is not possible to this server; it is not an MCP client (rule: no AI at run time; no client role).

A client that loads this server beside a hostile one is protected only by that client. Say so in the section; do not claim otherwise.

### B5. Confused deputy, in the product's own terms

**Published.** A deputy holding authority of its own acts for a caller who lacks it **[search]**.

**Here.** The product holds stored credentials for targets and acts through them. A caller must not gain through the product what the caller's own identity would not allow. Answers already written: policy decides, never the caller; a call cannot name the account it runs as; a user's session is theirs alone; the vault never hands a secret to a caller (rule 4); a credential belongs to the target it was stored for (decision 0248); one engine, one local user, and every local client is the same principal and every one an agent. The one open point is **local**: all local clients share one principal, so an agent steered by a hostile page or file can use any tool the person's agent could. Rule 3 and the approvals answer the writes; the reads are bounded by toolsets. Say so in one sentence.

### B6. Other threats the sources name, quickly

| Threat | Source | Product |
| --- | --- | --- |
| Local server with wide host access (file traversal, credential theft) | OWASP guidance **[search]** | Allowed file roots; paths checked in `a2m-provider`; rule 5 |
| Name collision and typosquat of servers | **[search]** | Not applicable: installed by the person |
| Over-broad scopes | Specification **[read]** | Toolsets, least-privilege per endpoint |
| Output that carries markup the client renders (links, images that leak data) | **[memory]** | Section 502: stripped of markup. Not verified that image and link syntax in Markdown is removed rather than HTML only |
| Annotations trusted by a client | Specification: clients MUST treat annotations as untrusted unless from a trusted server **[read: tools]** | The product sets `readOnlyHint` and the others from the effect class. They are true by construction, and a client cannot know that. The product must never rely on a client honoring them |

## What was not verified

- The 2026-07-28 text of the transport, authorization, tools, and security-best-practices pages. Findings are from 2025-11-25.
- The security-best-practices page itself. Its threats are taken from search summaries.
- The Rust SDK advisory's affected and fixed versions and its CVE number.
- The OWASP MCP Top 10 entries and the OWASP MCP cheat sheet: search only.
- Invariant Labs' original text, and the phrasing "rug pull" and "shadowing" as they defined them. Search summaries agree with each other, but all are secondary.
- Whether any client actually pins descriptions or acts on a digest in `_meta`. No client was tested; a digest is useful at least to the product's own SDKs and to a reviewer.
- Whether the product's `a2m-mcp` today checks `Origin`, `Host`, or audience. Package 6.2 is held; the code was not read.
- The claim that the stdio proxy writes only valid messages to stdout. Not tested here.

## What this changes in the design

All items are **proposals**. The lead applies them.

1. **Section 222, "The local channel" (add a bullet after "No TCP port").** Proposed sentences: "Where local HTTP is switched on (the workstation profile), the listener binds a loopback address only, never an unspecified one. It answers 403 to a request whose `Host` is not one of the loopback names with the listener's own port, and to a request that carries an `Origin` that is not the listener's own; a request with no `Origin` is allowed to continue to the credential check. It needs a bearer credential on every request: the engine issues it to the person's own client files, mode 0600, and it is never sent in a query string. A listener with no credential check does not start (`local_http_unauthenticated`)." Add the new failure code to the crate's table when built.
2. **Section 601, workstation profile row.** Proposed sentence: "Local HTTP is the one place in the first release that a web page's script could reach, and it is held to the rules of section 222 for a loopback listener."
3. **Roadmap package 6.2, proof.** Proposed replacement of the proof: "A client signs in and calls a tool; a token for another audience, a token with no audience, and a token whose audience is the metadata address are each refused with 401; a request with a foreign `Origin` or an unlisted `Host` is refused with 403 before any token is read; the response to a 401 carries the `resource_metadata` address."
4. **Section 502, new heading "Reaching the engine through a browser".** Proposed text: "A web page can make a browser send requests to a loopback address, and can make a name resolve to it after the page loaded. Nothing in the engine is reached by a browser except two things: the approval and sign-in listener, which answers only with the one-time code in its address and checks `Host` and `Origin`, and local HTTP where it is switched on, which is held to the rules in section 222. A check of `Host` and `Origin` is a second barrier. The first is that the page has no credential."
5. **Section 502, "Nothing from the system becomes an instruction" (add a bullet).** Proposed: "A tool's whole definition is bounded text, not only its description: the names, descriptions, defaults, and enum values in its input and output schemas follow the same rules as its description, and the compiler refuses what breaks them. A length bound limits what a description can hold. It does not prove the text harmless. The reviewer, shown every changed description first, and the linter, which looks for text that tells the reader what to do, are the barriers, and neither is complete."
6. **Decision 0307, point 6 (V5). Proposed addition, as a new record that supersedes the point:** "The bound covers every free-text field of a tool's definition: the description, each field description in the input and output schemas, enum labels, and defaults of text type; each is at most 1,024 characters, and a tool's definition has at most 8,192 in all. The same characters are refused in them that decision 0193 writes as escapes in a result: control characters, bidirectional marks and isolates, invisible format characters, and line and paragraph separators. A newline and a tab are allowed in a description."
7. **Section 221, the sentence that lists `_meta` on a tool.** Proposed addition: "Each published tool in a list also carries, under `anything2mcp/digest`, a SHA-256 of its name, description, input schema, output schema, and annotations in canonical form, so that a client can pin a tool and notice when it changes."
8. **Section 221 or 222 (new short rule on list-changed).** Proposed: "The MCP door declares `listChanged` and sends the notification whenever the tools an endpoint lists change: a version is published, an environment's version is switched, or a toolset's membership changes. A change of description always raises it. An environment follows a published version, never a moving pointer that can change what a client sees without a publish."
9. **Section 221, results.** Proposed addition: "The output schema of a tool names which fields hold the system's words, with `x-anything2mcp-untrusted: true` on each, so that a client can treat them as data."
10. **Section 502, "What this cannot do".** Proposed addition of two sentences: "Anything2MCP is never an MCP client. A hostile server loaded beside it in the same agent can write text that steers the agent's use of this server's tools, and only the agent's host can stop that. The annotations the product sets are true of the tool, and a client has no way to know it, so nothing in the product relies on a client honoring them."
11. **Section 502, linter bullet in "A manipulated building agent".** Proposed: "The lint rule for a description that addresses the reader lists its patterns in the lint's own documentation and has a test for each: an imperative sentence aimed at the model, a mention of another tool that this version does not publish, a mention of a file or address outside the target, and a request to hide something from the person. It is a help to the reviewer and is known to be evadable."
12. **Spec section 222, stdio bullet or the proxy's design in `docs/architecture/crates`.** Proposed sentence and test: "The stdio proxy writes to its standard output only complete MCP messages and writes diagnostics to its standard error only." A test: start the proxy with a failing engine and check standard output holds no non-message bytes.
13. **Note 06 and the table in the research README.** Mark brief 13 and brief 14 as answered by this note; add the note's row.

## Sources

Specification repository (revision 2025-11-25, read through a summarizing model):
- https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/specification/2025-11-25/basic/transports.mdx
- https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/specification/2025-11-25/basic/authorization.mdx
- https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/specification/2025-11-25/server/tools.mdx
- https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/docs/specification/2025-11-25/index.mdx

Advisories:
- https://github.com/modelcontextprotocol/typescript-sdk/security/advisories/GHSA-w48q-cv73-mx4w (read)
- https://github.com/modelcontextprotocol/rust-sdk/security/advisories (title and date of GHSA-89vp-x53w-74fx only)
- https://osv.dev/vulnerability/GHSA-89vp-x53w-74fx (did not load)

Search-result pages, not opened:
- https://modelcontextprotocol.io/specification/2025-11-25/basic/security_best_practices
- https://cheatsheetseries.owasp.org/cheatsheets/MCP_Security_Cheat_Sheet.html
- https://owasp.org/www-project-mcp-top-10/2025/MCP03-2025%E2%80%93Tool-Poisoning
- https://www.elastic.co/security-labs/mcp-tools-attack-defense-recommendations
- https://www.netskope.com/blog/securing-llm-superpowers-when-tools-turn-hostile-in-mcp
- https://www.descope.com/learn/post/mcp-tool-poisoning
- https://arxiv.org/pdf/2508.12538
- https://arxiv.org/pdf/2512.06556v2
- https://www.tenable.com/plugins/was/114885
- https://advisories.gitlab.com/pkg/npm/@playwright/mcp/
- https://github.com/modelcontextprotocol/typescript-sdk/security/advisories/GHSA-w48q-cv73-mx4w (also above)
