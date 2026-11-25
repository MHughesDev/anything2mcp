# Walkthrough: from nothing to a working tool

What using Anything2MCP should feel like, end to end, on one machine against the built-in test application. It is illustrative: outputs are shortened and details will change. It is not run as a test, but every command in it exists as written; a step whose action is not built yet says so. The path a person can walk today, against the test application's published description, is the README's "Try it from source". The exit test of Phase 5 (package 5.6) is `crates/anything2mcp/tests/first_hour.rs`, which follows a shorter path, from an empty folder to a published read an agent calls.

Commands a coding agent runs are shown as the agent sees them, with one JSON object returned per command.

## 1. Install and start a project

```text
pip install anything2mcp      # once it is published; until then, build from source
anything2mcp init
```

`init` makes the folder it runs in a project with `anything2mcp.toml`, an `AGENTS.md` that teaches the build loop, and an empty `targets/` folder. Nothing else needs configuring.

## 2. Add the application

```text
anything2mcp add example --address http://127.0.0.1:4010/ --kind web
anything2mcp session login example
```

```json
{"ok": true, "action": "target.add",
 "result": {"target": "example", "platform": "plain", "support_tier": "full",
            "sign_in": "form_login", "recommended_mode": "managed_session"},
 "next": {"action": "session.login", "args": {"target": "example"}}}
```

`add` takes the target's name, the address, and the kind of surface, which is never guessed from the address. A sign-in is asked for with `session login`: the person signs in once, on the engine's own page. The command returns exit code 3 until they have, and says so.

## 3. See where things stand

```text
anything2mcp status example
```

```json
{"ok": true, "result": {"target": "example", "state": "signed_in"},
 "next": {"action": "capture.start", "args": {"target": "example"}}}
```

## 4. Look at a screen

`screen snapshot` is not built yet: it answers `action_not_built`. When it is, it writes the snapshot to a file under `.anything2mcp/out/` and returns the path; the agent reads the file itself, and the snapshot is not returned inline.

## 5. Record the application being used

```text
anything2mcp learn example          # capture start: the answer is the page where the person begins
anything2mcp capture stop example   # how it ended, and the scout of what it saw
```

The person begins the capture on the engine's page, opens an order, and looks at its lines; only the structure of what they use is kept, never a value. `capture stop` scouts what it saw and drafts reads, held where a rule is not sure. `endpoints infer`, an inference of endpoints apart from a capture, is not built.

## 6. Write and check the drafts

The agent edits the files under `targets/example/`: a screen, an endpoint, and a read operation. Then:

```text
anything2mcp lint example
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
anything2mcp try example --tool example_order_get --input '{"order_id": "A-100"}'
anything2mcp test example
```

`try` runs the draft in the sandbox and returns the result with the binding used. `test` runs the publish gates and returns which passed, with evidence for any that did not.

## 8. Publish

```text
anything2mcp publish example
```

`anything2mcp ready example` does steps 7 and 8 in one, for every read a trial proved. The command returns exit code 3, with a link: a person must review. The person is shown the review summary, ordered by risk, and approves. The version is now published and its compiled target hash recorded.

## 9. Connect an agent and use the tool

```text
anything2mcp connect            # prints the block for any client
anything2mcp connect <client>   # writes it into the file a client profile names
```

The agent's tool list now includes the control tools. It finds and calls the new tool:

```json
{"run_id": "r_41c0", "tool": "example_order_get@1", "status": "completed", "mode": "http",
 "result": {"order_id": "A-100", "status": "draft", "lines": 3}}
```

## 10. A write, approved by a person

After the agent has also built `example_line_item_upsert` and the pipeline `example_order_sheet_enter`, and the person has published them:

```text
anything2mcp run example_order_sheet_enter --source-file sheet.xlsx --input '{"order_id": "A-100"}'
```

```json
{"ok": true, "action": "tool.call",
 "result": {"run_id": "r_8f21", "tool": "example_order_sheet_enter@1", "status": "needs_approval",
            "summary": "12 line items: 9 to create, 3 to update. Waiting for approval.",
            "diff": {"create": 9, "update": 3, "unchanged": 0, "detail": ".anything2mcp/out/r_8f21-diff.json"}},
 "next": {"action": "run.get", "args": {"run_id": "r_8f21"}, "when": "after the person approves"}}
```

The person gets a notification, reads the diff on the approval page, presses Approve, and confirms with the computer's own prompt. Then:

```text
anything2mcp run get r_8f21
```

```json
{"ok": true, "action": "run.get",
 "result": {"run_id": "r_8f21", "status": "completed", "mode": "ui",
            "summary": "12 line items verified: 9 created, 3 updated.",
            "trace": ".anything2mcp/out/r_8f21-trace.jsonl"}}
```

## 11. Run it again

The same command with the same sheet returns `completed` with every record `unchanged` and nothing written. That is the repeatability guarantee, visible to the person.

## What the walkthrough must show

- No file was written by hand before step 6, and no setting was changed.
- Every command told the agent what to do next.
- Nothing large entered the agent's context.
- A person signed in once, approved the publish, and approved the write. Nothing else needed them.
- The second run changed nothing.
