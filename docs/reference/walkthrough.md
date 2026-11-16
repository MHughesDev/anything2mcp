# Walkthrough: from nothing to a working tool

What using Web2MCP should feel like, end to end, on one machine against the built-in test application. It is illustrative: outputs are shortened and details will change as the commands are built. It doubles as the acceptance test for Phase 5, which runs these steps and checks each result.

Commands a coding agent runs are shown as the agent sees them, with one JSON object returned per command.

## 1. Install and start a project

```text
pip install web2mcp
web2mcp init
```

`init` creates a project folder with `web2mcp.toml`, an `AGENTS.md` that teaches the build loop, and an empty `targets/` folder. Nothing else needs configuring.

## 2. Add the application

```text
web2mcp add http://localhost:4010
```

```json
{"ok": true, "action": "target.add",
 "result": {"target": "example", "platform": "plain", "support_tier": "full",
            "sign_in": "form_login", "recommended_mode": "managed_session"},
 "next": {"action": "session.login", "args": {"target": "example"}}}
```

A browser window opens at the application's sign-in page. The person signs in once. The command returns exit code 3 until they have, and says so.

## 3. See where things stand

```text
web2mcp status
```

```json
{"ok": true, "result": {"target": "example", "state": "signed_in"},
 "next": {"action": "screen.snapshot"}}
```

## 4. Look at a screen

```text
web2mcp screen snapshot --url /orders/A-100/items
```

```json
{"ok": true, "result": {"path": ".web2mcp/out/snapshot-01.txt", "elements": 14, "matches": []},
 "next": {"action": "capture.start"}}
```

The agent reads the file itself. The snapshot is not returned inline.

## 5. Record the application being used

```text
web2mcp capture start
web2mcp capture stop
web2mcp endpoints infer
```

Between start and stop, the person or the agent opens an order and looks at its lines. Exploration cannot send anything but reads. `infer` returns the endpoints it found, with schemas, and writes draft files.

## 6. Write and check the drafts

The agent edits the files under `targets/example/`: a screen, an endpoint, and a read operation. Then:

```text
web2mcp lint
```

```json
{"ok": false, "error": {"code": "lint_failed", "class": "input",
 "message": "2 problems",
 "problems": [
   {"file": "operations/order_get.yaml", "line": 9, "rule": "contracts.output_schema",
    "message": "The operation has no output schema.", "fix": "Add `outputs`, usually a reference to the endpoint's response."},
   {"file": "screens/line_item_form.yaml", "line": 14, "rule": "locating.fingerprint",
    "message": "The field `length` has no fingerprint.", "fix": "Add `fingerprint` with `near_text` or `section`."}]}}
```

The agent fixes both and runs `lint` again until it passes.

## 7. Try it and test it

```text
web2mcp try example_order_get --input order_id=A-100
web2mcp test
```

`try` runs the draft in the sandbox and returns the result with the binding used. `test` runs the publish gates and returns which passed, with evidence for any that did not.

## 8. Publish

```text
web2mcp publish
```

The command returns exit code 3: a person must review. The person is shown the review summary, ordered by risk, and approves. The version is now published and its plan hash recorded.

## 9. Connect an agent and use the tool

```text
web2mcp connect <client>
```

The agent's tool list now includes the control tools. It finds and calls the new tool:

```json
{"run_id": "r_41c0", "tool": "example_order_get@1", "status": "completed", "mode": "http",
 "result": {"order_id": "A-100", "status": "draft", "lines": 3}}
```

## 10. A write, approved by a person

After the agent has also built `example_line_item_upsert` and the pipeline `example_order_sheet_enter`, and the person has published them:

```text
web2mcp run example_order_sheet_enter --source sheet.xlsx --input order_id=A-100
```

```json
{"ok": true, "action": "tool.call",
 "result": {"run_id": "r_8f21", "tool": "example_order_sheet_enter@1", "status": "needs_approval",
            "summary": "12 line items: 9 to create, 3 to update. Waiting for approval.",
            "diff": {"create": 9, "update": 3, "unchanged": 0, "detail": ".web2mcp/out/r_8f21-diff.json"}},
 "next": {"action": "run.get", "args": {"run_id": "r_8f21"}, "when": "after the person approves"}}
```

The person gets a notification, reads the diff on the approval page, presses Approve, and confirms with the computer's own prompt. Then:

```text
web2mcp run get r_8f21
```

```json
{"ok": true, "action": "run.get",
 "result": {"run_id": "r_8f21", "status": "completed", "mode": "ui",
            "summary": "12 line items verified: 9 created, 3 updated.",
            "trace": ".web2mcp/out/r_8f21-trace.jsonl"}}
```

## 11. Run it again

The same command with the same sheet returns `completed` with every record `unchanged` and nothing written. That is the repeatability guarantee, visible to the person.

## What the walkthrough must show

- No file was written by hand before step 6, and no setting was changed.
- Every command told the agent what to do next.
- Nothing large entered the agent's context.
- A person signed in once, approved the publish, and approved the write. Nothing else needed them.
- The second run changed nothing.
