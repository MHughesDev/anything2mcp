# 18. Safety, policy, and approvals

A policy gate sits between the front doors and the engine. Every call passes through it, and it can only narrow what a run may do. [Section 45](45-policy-engine.md) is the design of the engine behind the gate, and [section 49](49-approval-and-sign-in-surfaces.md) is what a person sees when asked to approve.

**Effect classes** decide the path a call takes:

| Effect | Examples | Preview | Approval | Automatic retry |
| --- | --- | --- | --- | --- |
| `read` | Get a record, list records | No | None | Yes |
| `write` | Create or update records | Required | Per policy: the caller's own confirmation, or a named approver | Idempotent steps only |
| `irreversible` | Delete, submit, send, pay | Required | A person, every run; optionally two people | Never |

**Controls**, in the order a call meets them:

1. **Authentication and grants.** The caller is identified and must hold a grant for the operation and its effect class.
2. **Permission manifest.** The run may touch only the surfaces, downloads, uploads, and destinations the operation declared.
3. **Input validation.** Inputs and the whole source are checked first. Bad input returns every problem at once and changes nothing.
4. **Preview.** The read steps run and produce a diff of current against proposed values, record by record.
5. **Approval.** A person approves that exact diff. The approval is stored server-side as a single-use record bound to the diff's hash, the tool version, the approver, and an expiry. If the app's data changes before the run, the hash no longer matches and the run stops. The check is made per record just before each write when the preview is older than a freshness window ([section 42](42-run-ledger-design.md)).
6. **Limits.** Maximum records per run, rate budgets per surface, quotas per caller, and a kill switch per target and for the whole system.
7. **Read-back.** Every write is verified.

**Approval channels.** An agent must never be able to approve its own action, so approvals arrive only through channels the calling agent cannot operate:

| Channel | How it works | Fits |
| --- | --- | --- |
| MCP URL-mode elicitation | The server sends the client a link to Web2MCP's approval page; the person approves in their browser, signed in as themselves. The MCP spec reserves URL mode for sensitive interactions that must not pass through the client ([MCP spec](https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation)) | Clients that support elicitation |
| Approval page link in the result | The result carries `needs_approval` and a link; the person opens it | Every client, including ones without elicitation |
| Chat approval | An approval card in Microsoft Teams or Slack, sent to the approver | Company deployments |
| Local prompt | A prompt raised by the engine itself, with an operating-system user-presence check where one exists; there is no approve command or tool ([section 39](39-local-engine-and-clients.md)) | Solo use |

**Avoiding approval fatigue.**

- One approval covers a whole pipeline diff, so a 40-record sheet is one decision.
- Policy can pre-approve low-risk writes, such as a user updating their own records under a size limit, while keeping the preview and audit.
- Standing approvals can be granted for a named pipeline, caller, and time window, and are revocable.

**Untrusted content from applications.** Text read from an app, such as notes and comments, enters the calling agent's context and may contain instructions aimed at the agent.

- Results are structured; free text sits only in fields marked `untrusted` in the output schema.
- Free-text fields are length-limited and stripped of markup and hidden characters.
- The permission manifest blocks undeclared destinations, so a hijacked agent cannot use a Web2MCP tool to send data elsewhere.
- Writes still need a person's approval of a visible diff.

**Supply chain.** Published versions are immutable, hashed, and signed by the publisher. The server loads only versions whose signature and hash are in its registry.

The approval and prompt-injection design rests on design judgment plus the MCP specification; dedicated research on enterprise approval patterns is still to do.
