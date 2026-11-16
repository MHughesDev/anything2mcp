# 8. Execution engine

Every operation call in every run goes through the same eight-part engine loop, whatever the mode. Drivers fill in how each part is done. A pipeline step calls its operation once, or once per row. A write repeats parts 4 to 8 for each commit point; a read skips parts 4 and 7.

1. **Guard.** Session valid, surface healthy, no edge block, policy allows the call.
2. **Choose.** Pick the binding by the mode rules and record why.
3. **Prepare.** Get what the driver needs from the session keeper: a page, a cookie jar, or a fresh token.
4. **Intend.** For a write, take the record lock, make the freshness check, then record the intended values in the ledger before acting ([section 42](42-run-ledger-design.md)).
5. **Act.** The driver performs the step.
6. **Confirm.** Wait for the declared confirmation: a response, a status, an element, or an upstream tool result.
7. **Read back.** Re-read the value through the binding's read-back rule and compare after normalization.
8. **Record.** Mark the commit point verified, with raw and normalized values as evidence, and release the record lock. For a read, store the result.

What each driver does in the Act and Read back parts:

| Driver | Act | Read back |
| --- | --- | --- |
| `ui` | Locate by ordered locators and fingerprint; widget driver sets the value, escalating from fill to typing to the native setter | The field, or preferably the save response |
| `page_http`, `http` | Build the request from the endpoint and CEL mappings; fetch any declared anti-forgery token first; validate the response against its schema | The matching GET endpoint |
| `api` | Call the generated client for the described operation | The described read operation |
| `site_tools` | Call the site's tool with typed arguments | The site's read tool, or the UI |
| `mcp` | Call the upstream MCP tool with typed arguments | The upstream read tool named in the binding |
| `desktop` | Use accessibility patterns: set value, invoke, select, expand | The element's current value pattern |

Waits are always on named conditions. Fixed sleeps are rejected by the file linter.

Every failure falls into one class, and the class alone decides what happens:

| Class | Example | Response |
| --- | --- | --- |
| `transient` | Timeout on a read, dropped connection, upstream 503 | Retry up to 2 times, reads and idempotent writes only |
| `session_expired` | Redirect to the identity provider, 401 from the backend | Session keeper tries a silent refresh once; otherwise stop with `needs_login`; resumable |
| `edge_blocked` | 403 or 429 from a bot defense, challenge page | Stop with evidence; no retry; alert |
| `drift` | Locator fails its fingerprint; response fails its schema; upstream tool changed | Stop; capture; mark the binding unhealthy; try the next verified binding for reads only |
| `unknown_variant` | Unexpected dialog, page, window, or status | Stop; capture; never guess |
| `rejected` | The app refused the input with a validation message | Stop; return the app's message and the record key |
| `mismatch` | Read-back differs from the intended value | Re-read once, then stop |
| `possible_commit` | Crash or timeout after a write was sent | Stop; reconcile on resume before anything else |
| `input` | Inputs or a source file fail validation | Reject before anything runs; return every problem at once |
| `busy` | Queue full, record locked, no runner available | No run is started, or the run waits; return a retry time |
| `policy` | Undeclared target, over a limit, stale approval, caller lacks the role | Stop before acting |
| `stale` | A record no longer matches what the preview saw, matches neither value on resume, or the application refuses the version sent with the write | Stop the run at that record; a new preview and approval are needed |
